#!/usr/bin/env python3
"""Package native client/launcher binaries and assemble their update manifest."""

import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import zipfile


TARGETS = (
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
)
ANDROID_TARGET = "aarch64-linux-android"
ANDROID_ASSET = f"rubblekin-client-{ANDROID_TARGET}.apk"
ANDROID_MANIFEST = "android-manifest.json"
ANDROID_PACKAGE_ID = "net.rubblekin.client"
ANDROID_SIGNING = "public-development-key"
ANDROID_MAX_VERSION_CODE = 2_147_483_647
ANDROID_MAX_APK_SIZE = 256 * 1024 * 1024
ROOT = Path(__file__).resolve().parents[2]
# Keep releases installable by existing launchers (their bounds are unchanged).
DESKTOP_MAX_SIZE = 512 * 1024 * 1024


def validate_commit(commit):
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("commit must be a full lowercase Git SHA")
    return commit


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def executable_path(target, launcher=False):
    name = "rubblekin-launcher" if launcher else "rubblekin"
    if "apple-darwin" in target:
        app_name = "Rubblekin Launcher" if launcher else "Rubblekin"
        return f"{app_name}.app/Contents/MacOS/{name}"
    return name + (".exe" if "windows" in target else "")


def archive_name(target, launcher=False):
    product = "launcher" if launcher else "client"
    return f"rubblekin-{product}-{target}.zip"


def write_zip(source, output):
    """Retain executable bits; omit symlinks and host-specific ZIP metadata."""
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(source.rglob("*")):
            if path.is_symlink():
                raise ValueError(f"refusing to package a symlink: {path}")
            if not path.is_file():
                continue
            info = zipfile.ZipInfo(path.relative_to(source).as_posix())
            info.create_system = 3
            mode = 0o755 if path.stat().st_mode & stat.S_IXUSR else 0o644
            info.external_attr = (stat.S_IFREG | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            with path.open("rb") as content, archive.open(info, "w") as destination:
                shutil.copyfileobj(content, destination)


def stage_product(binary_dir, stage, target, commit, launcher=False, sign=True):
    relative = executable_path(target, launcher)
    executable = stage / relative
    executable.parent.mkdir(parents=True)
    binary_name = Path(relative).name
    source = binary_dir / binary_name
    if not source.is_file() or source.is_symlink():
        raise ValueError(f"missing regular binary: {source}")
    shutil.copyfile(source, executable)
    executable.chmod(0o755)
    if target == "x86_64-unknown-linux-gnu":
        # CI uploads the original ELF to Sentry before packaging. Strip only
        # this staged copy, preserving the build ID and runtime/unwind sections.
        subprocess.run(["strip", "--strip-debug", str(executable)], check=True)
    license_path = stage / "OFL.txt"
    shutil.copyfile(ROOT / "assets/fonts/OFL.txt", license_path)
    if "apple-darwin" in target:
        bundle = executable.parents[2]
        resources = bundle / "Contents/Resources"
        resources.mkdir()
        shutil.copyfile(ROOT / "assets/fonts/OFL.txt", resources / "OFL.txt")
        plist = {
            "CFBundleExecutable": binary_name,
            "CFBundleIdentifier": "net.rubblekin.launcher" if launcher else "net.rubblekin.client",
            "CFBundleName": "Rubblekin Launcher" if launcher else "Rubblekin",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": "0.1.0",
            "CFBundleVersion": "1",
            "NSHighResolutionCapable": True,
            "RubblekinCommit": commit,
        }
        with (bundle / "Contents/Info.plist").open("wb") as output:
            plistlib.dump(plist, output)
        if sign:
            if sys.platform != "darwin":
                raise ValueError("macOS bundles must be signed on a macOS runner")
            # Ad-hoc signing makes a structurally valid bundle. Developer ID
            # signing/notarization needs a separately configured certificate.
            subprocess.run(["codesign", "--force", "--sign", "-", str(bundle)], check=True)
            subprocess.run(["codesign", "--verify", "--deep", "--strict", str(bundle)], check=True)
    size = executable.stat().st_size
    if not 0 < size <= DESKTOP_MAX_SIZE:
        raise ValueError(f"packaged executable {relative} is {size} bytes; launcher limit is {DESKTOP_MAX_SIZE} bytes")
    return relative


def package(binary_dir, output_dir, target, commit, sign=True):
    validate_commit(commit)
    if target not in TARGETS:
        raise ValueError(f"unsupported target: {target}")
    output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="rubblekin-package-") as directory:
        for launcher in (False, True):
            stage = Path(directory) / ("launcher" if launcher else "client")
            stage_product(binary_dir, stage, target, commit, launcher, sign)
            archive = output_dir / archive_name(target, launcher)
            write_zip(stage, archive)
            if archive.stat().st_size > DESKTOP_MAX_SIZE:
                raise ValueError(f"packaged archive {archive.name} exceeds the launcher download limit of {DESKTOP_MAX_SIZE} bytes")
    asset = archive_name(target)
    metadata = {
        "commit": commit,
        "target": target,
        "client": {
            "asset": asset,
            "sha256": digest(output_dir / asset),
            "executable": executable_path(target),
        },
    }
    (output_dir / f"{target}.json").write_text(json.dumps(metadata, indent=2) + "\n")
    return metadata


def validate_version_code(version_code):
    if type(version_code) is not int or not 1 <= version_code <= ANDROID_MAX_VERSION_CODE:
        raise ValueError("Android version code must be a positive integer no larger than 2147483647")
    return version_code


def validate_apk_size(size):
    if type(size) is not int or not 1 <= size <= ANDROID_MAX_APK_SIZE:
        raise ValueError("Android APK size must be an integer from 1 byte to 256 MiB")
    return size


def apk_version_code(apk, aapt2, expected_version_code):
    """Read the compiled manifest; don't trust a version supplied by the build."""
    validate_version_code(expected_version_code)
    result = subprocess.run([str(aapt2), "dump", "badging", str(apk)], capture_output=True, text=True)
    if result.returncode:
        raise ValueError(f"aapt2 could not inspect APK: {result.stderr.strip()}")
    matched = re.search(r"^package: name='([^']+)' versionCode='([0-9]+)'(?:\s|$)", result.stdout, re.MULTILINE)
    if matched is None or matched[1] != ANDROID_PACKAGE_ID:
        raise ValueError("APK package identity or version code is invalid")
    version_code = validate_version_code(int(matched[2]))
    if version_code != expected_version_code:
        raise ValueError("APK version code does not match the commit's full first-parent count")
    return version_code


def package_android(apk, output_dir, commit, *, updater=False, aapt2=None, expected_version_code=None):
    """Record an installable prototype APK separately from desktop updates."""
    validate_commit(commit)
    if not apk.is_file() or apk.is_symlink():
        raise ValueError(f"missing regular APK: {apk}")
    with zipfile.ZipFile(apk) as archive:
        required = {"AndroidManifest.xml", "classes.dex", "lib/arm64-v8a/librubblekin_client.so", "assets/OFL.txt"}
        if not required.issubset(archive.namelist()) or archive.testzip() is not None:
            raise ValueError("APK is missing its manifest, Java activity, ARM64 library, or font license")
        with archive.open("lib/arm64-v8a/librubblekin_client.so") as library:
            if library.read(4) != b"\x7fELF":
                raise ValueError("APK native library is not an ELF binary")
    version_code = None
    if updater:
        if aapt2 is None:
            raise ValueError("Updater APK metadata requires aapt2 inspection")
        validate_apk_size(apk.stat().st_size)
        version_code = apk_version_code(apk, aapt2, expected_version_code)
    output_dir.mkdir(parents=True, exist_ok=True)
    destination = output_dir / ANDROID_ASSET
    if apk.resolve() != destination.resolve():
        shutil.copyfile(apk, destination)
    metadata = {
        "commit": commit,
        "target": ANDROID_TARGET,
        "client": {
            "asset": ANDROID_ASSET,
            "sha256": digest(destination),
            "package_id": ANDROID_PACKAGE_ID,
            "signing": ANDROID_SIGNING,
        },
    }
    if updater:
        metadata["version_code"] = version_code
        metadata["client"]["size"] = validate_apk_size(destination.stat().st_size)
    (output_dir / f"{ANDROID_TARGET}.json").write_text(json.dumps(metadata, indent=2) + "\n")
    return metadata


def assemble_manifest(directory, commit, require_android=False, require_android_updater=False,
                      expected_version_code=None):
    """Fail closed if any platform is missing, mixed, or changed in transit."""
    validate_commit(commit)
    platforms = {}
    for target in TARGETS:
        metadata = json.loads((directory / f"{target}.json").read_text())
        expected = {
            "asset": archive_name(target),
            "sha256": digest(directory / archive_name(target)),
            "executable": executable_path(target),
        }
        if metadata != {"commit": commit, "target": target, "client": expected}:
            raise ValueError(f"package metadata mismatch for {target}")
        if not (directory / archive_name(target, launcher=True)).is_file():
            raise ValueError(f"missing launcher for {target}")
        platforms[target] = expected
    manifest = {
        "schema_version": 1,
        "commit": commit,
        "tag": f"client-{commit}",
        "platforms": platforms,
    }
    manifest_path = directory / "client-manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    assets = [manifest_path]
    assets += [directory / archive_name(target, launcher) for target in TARGETS for launcher in (False, True)]
    if require_android or require_android_updater:
        metadata = json.loads((directory / f"{ANDROID_TARGET}.json").read_text())
        expected = {
            "commit": commit,
            "target": ANDROID_TARGET,
            "client": {
                "asset": ANDROID_ASSET,
                "sha256": digest(directory / ANDROID_ASSET),
                "package_id": ANDROID_PACKAGE_ID,
                "signing": ANDROID_SIGNING,
            },
        }
        if require_android_updater:
            expected["version_code"] = validate_version_code(metadata.get("version_code"))
            if expected["version_code"] != validate_version_code(expected_version_code):
                raise ValueError("Android metadata version code does not match the commit's full first-parent count")
            expected["client"]["size"] = validate_apk_size((directory / ANDROID_ASSET).stat().st_size)
            validate_apk_size(metadata.get("client", {}).get("size"))
        if metadata != expected:
            raise ValueError("package metadata mismatch for Android")
        assets.append(directory / ANDROID_ASSET)
        if require_android_updater:
            android_manifest = {
                "schema_version": 1,
                "commit": commit,
                "tag": f"client-{commit}",
                "target": ANDROID_TARGET,
                "version_code": expected["version_code"],
                "client": expected["client"],
            }
            android_path = directory / ANDROID_MANIFEST
            android_path.write_text(json.dumps(android_manifest, indent=2, sort_keys=True) + "\n")
            assets.append(android_path)
    checksums = "".join(f"{digest(path)}  {path.name}\n" for path in sorted(assets))
    (directory / "SHA256SUMS").write_text(checksums)
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    build = subcommands.add_parser("package")
    build.add_argument("--target", choices=TARGETS, required=True)
    build.add_argument("--binary-dir", type=Path, required=True)
    build.add_argument("--output-dir", type=Path, required=True)
    build.add_argument("--commit", required=True)
    android = subcommands.add_parser("android")
    android.add_argument("--apk", type=Path, required=True)
    android.add_argument("--output-dir", type=Path, required=True)
    android.add_argument("--commit", required=True)
    android.add_argument("--android-updater", action="store_true")
    android.add_argument("--aapt2", type=Path)
    android.add_argument("--expected-version-code", type=int)
    manifest = subcommands.add_parser("manifest")
    manifest.add_argument("--directory", type=Path, required=True)
    manifest.add_argument("--commit", required=True)
    manifest.add_argument("--android", action="store_true")
    manifest.add_argument("--android-updater", action="store_true")
    manifest.add_argument("--expected-version-code", type=int)
    arguments = parser.parse_args()
    if arguments.command == "package":
        package(arguments.binary_dir, arguments.output_dir, arguments.target, arguments.commit)
    elif arguments.command == "android":
        package_android(arguments.apk, arguments.output_dir, arguments.commit, updater=arguments.android_updater,
                        aapt2=arguments.aapt2, expected_version_code=arguments.expected_version_code)
    else:
        assemble_manifest(arguments.directory, arguments.commit, arguments.android, arguments.android_updater,
                          arguments.expected_version_code)


if __name__ == "__main__":
    main()
