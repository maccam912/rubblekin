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
ROOT = Path(__file__).resolve().parents[2]


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
            write_zip(stage, output_dir / archive_name(target, launcher))
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


def assemble_manifest(directory, commit):
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
    manifest = subcommands.add_parser("manifest")
    manifest.add_argument("--directory", type=Path, required=True)
    manifest.add_argument("--commit", required=True)
    arguments = parser.parse_args()
    if arguments.command == "package":
        package(arguments.binary_dir, arguments.output_dir, arguments.target, arguments.commit)
    else:
        assemble_manifest(arguments.directory, arguments.commit)


if __name__ == "__main__":
    main()
