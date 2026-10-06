#!/usr/bin/env python3
"""Upload exact Rust build symbols to Sentry before packaging/signing.

The DSN is public client configuration. Only this build-time command receives
SENTRY_AUTH_TOKEN; the token is never embedded in a game binary or APK.
"""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

CLI_VERSION = "3.8.0"


def upload(binary_dir, android=False):
    for name in ("SENTRY_AUTH_TOKEN", "SENTRY_ORG", "SENTRY_PROJECT"):
        if not os.environ.get(name):
            raise ValueError(f"Set {name} to upload crash-report symbols")
    if android:
        files = [binary_dir / "librubblekin_client.so"]
    elif sys.platform == "darwin":
        symbols = binary_dir / "rubblekin.dSYM"
        if symbols.exists():
            shutil.rmtree(symbols)
        subprocess.run(["dsymutil", str(binary_dir / "rubblekin"), "-o", str(symbols)], check=True)
        files = [symbols]
    elif sys.platform == "win32":
        files = [binary_dir / "rubblekin.exe", binary_dir / "rubblekin.pdb"]
    else:
        files = [binary_dir / "rubblekin"]
    for path in files:
        if not path.exists():
            raise ValueError(f"Missing debug-symbol input: {path}")
    npx = shutil.which("npx.cmd" if os.name == "nt" else "npx")
    if not npx:
        raise ValueError("Install Node.js to run the pinned Sentry CLI")
    subprocess.run([
        npx, "--yes", f"@sentry/cli@{CLI_VERSION}", "debug-files", "upload",
        "--include-sources", "--wait", *map(str, files),
    ], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary-dir", required=True, type=Path)
    parser.add_argument("--android", action="store_true")
    args = parser.parse_args()
    upload(args.binary_dir, args.android)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from None
