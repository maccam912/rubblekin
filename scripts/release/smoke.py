#!/usr/bin/env python3
"""Extract the produced archives and smoke-test the actual packaged binaries."""

import argparse
from pathlib import Path
import subprocess
import tempfile
import zipfile

from package import TARGETS, archive_name, executable_path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    arguments = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="rubblekin-smoke-") as directory:
        for launcher in (False, True):
            output = Path(directory) / ("launcher" if launcher else "client")
            with zipfile.ZipFile(arguments.directory / archive_name(arguments.target, launcher)) as archive:
                archive.extractall(output)
                for member in archive.infolist():
                    (output / member.filename).chmod((member.external_attr >> 16) & 0o777)
            executable = output / executable_path(arguments.target, launcher)
            if "apple-darwin" in arguments.target:
                subprocess.run(["codesign", "--verify", "--deep", "--strict", str(executable.parents[2])], check=True)
            subprocess.run([str(executable), "--help"], cwd=directory, check=True, timeout=30)


if __name__ == "__main__":
    main()
