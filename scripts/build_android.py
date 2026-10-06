#!/usr/bin/env python3
"""Build a development-signed ARM64 Android APK with an optimized Rust client."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
TARGET = "aarch64-linux-android"
NDK_VERSION = "28.2.13676358"
APK_NAME = f"rubblekin-client-{TARGET}.apk"


def run(arguments, **kwargs):
    subprocess.run(arguments, cwd=ROOT, check=True, **kwargs)


def git(*arguments):
    return subprocess.check_output(["git", *arguments], cwd=ROOT, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "target/android")
    parser.add_argument("--profile", choices=("dev", "release"), default="release")
    parser.add_argument("--install", action="store_true", help="Install over the existing app using adb")
    parser.add_argument("--device", help="adb serial when more than one device is connected")
    args = parser.parse_args()

    if git("rev-parse", "--is-shallow-repository") == "true":
        parser.error("Android version codes require full history: git fetch --unshallow")

    sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    if not sdk and sys.platform == "darwin":
        sdk = str(Path.home() / "Library/Android/sdk")
    if not sdk or not Path(sdk).is_dir():
        parser.error("Set ANDROID_HOME to an Android SDK with platform 36, build-tools 36.0.0, and NDK " + NDK_VERSION)
    sdk = Path(sdk).resolve()
    ndk = sdk / "ndk" / NDK_VERSION
    if not ndk.is_dir():
        parser.error(f'Install the pinned NDK: sdkmanager "ndk;{NDK_VERSION}"')
    if not shutil.which("cargo-ndk"):
        parser.error("Install cargo-ndk: cargo install cargo-ndk --version 4.1.2 --locked")
    targets = subprocess.check_output(["rustup", "target", "list", "--installed"], text=True).splitlines()
    if TARGET not in targets:
        parser.error(f"Install the Android Rust target: rustup target add {TARGET}")

    environment = os.environ.copy()
    environment["ANDROID_HOME"] = str(sdk)
    environment["ANDROID_NDK_HOME"] = str(ndk)
    environment.setdefault("CARGO_BUILD_JOBS", "4")
    environment.setdefault("CARGO_INCREMENTAL", "0")
    # Android's 16 KB page-size devices need matching ELF and APK alignment.
    environment["CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS"] = (
        environment.get("CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS", "")
        + " -C link-arg=-Wl,-z,max-page-size=16384"
    ).strip()
    # Android requests its shared library explicitly; desktop builds use the Rust library.
    run([
        "cargo", "ndk", "--platform", "26", "--target", "arm64-v8a",
        "--output-dir", "android/app/src/main/jniLibs",
        "rustc", "--locked", "--profile", args.profile, "--lib", "--crate-type", "cdylib",
        "-p", "rubblekin_client",
    ], env=environment)

    # First-parent main releases have increasing version codes. Sideloading an
    # older historical build may need adb install -r -d (this APK is debuggable).
    commit = git("rev-parse", "HEAD")
    dirty = "-dirty" if git("status", "--porcelain") else ""
    version_code = git("rev-list", "--first-parent", "--count", "HEAD")
    wrapper = ROOT / "android" / ("gradlew.bat" if os.name == "nt" else "gradlew")
    packaged_apk = ROOT / "android/app/build/outputs/apk/debug/app-debug.apk"
    # AGP's incremental ZIP updates can retain the previous native library as
    # unreachable bytes. Repackage a fresh APK while keeping compilation caches.
    packaged_apk.unlink(missing_ok=True)
    run([
        str(wrapper), "--no-daemon", "--console=plain", "-p", str(ROOT / "android"),
        f"-PrubblekinVersionCode={version_code}",
        f"-PrubblekinVersionName=0.1.0-{commit[:12]}{dirty}", "assembleDebug",
    ], env=environment)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    output = args.output_dir.resolve() / APK_NAME
    shutil.copyfile(packaged_apk, output)
    print(f"\nDevelopment-signed APK: {output}")
    if args.install:
        adb = sdk / "platform-tools" / ("adb.exe" if os.name == "nt" else "adb")
        arguments = [str(adb)]
        if args.device:
            arguments += ["-s", args.device]
        run([*arguments, "install", "-r", str(output)], env=environment)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        raise SystemExit(error.returncode) from None
