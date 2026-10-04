#!/usr/bin/env python3
"""Enumerate the commits introduced onto main by this push."""

import argparse
import json
import os
import subprocess
import sys

from package import validate_commit
from publish import reserve_tag


PLATFORMS = (
    ("x86_64-unknown-linux-gnu", "ubuntu-22.04"),
    ("x86_64-pc-windows-msvc", "windows-2022"),
    ("aarch64-apple-darwin", "macos-14"),
    ("x86_64-apple-darwin", "macos-15-intel"),
)
MAX_COMMITS = 64  # GitHub permits at most 256 matrix jobs, with four per commit.


def git(*arguments):
    return subprocess.check_output(["git", *arguments], text=True).strip()


def select_commits(event, before, after, requested, history, introduced, supported):
    """Pure selection keeps the push/dispatch rules covered without GitHub."""
    if event == "workflow_dispatch":
        candidates = [validate_commit(requested or after)]
    else:
        candidates = introduced
    commits = [commit for commit in candidates if commit in supported]
    if not commits:
        raise ValueError("no commits contain the client release tooling")
    if any(commit not in history for commit in commits):
        raise ValueError("releases must come from main's first-parent history")
    if len(commits) > MAX_COMMITS:
        raise ValueError("push contains more than 64 releasable commits; dispatch individual commit releases")
    return commits


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event", choices=("push", "workflow_dispatch"), required=True)
    parser.add_argument("--before", default="")
    parser.add_argument("--after", required=True)
    parser.add_argument("--commit", default="")
    parser.add_argument("--repository", required=True)
    arguments = parser.parse_args()
    validate_commit(arguments.after)
    history = git("rev-list", "--first-parent", "origin/main").splitlines()
    if arguments.event == "push":
        before = arguments.before
        if before and before != "0" * 40:
            validate_commit(before)
            ancestry = subprocess.run(["git", "merge-base", "--is-ancestor", before, arguments.after])
            if ancestry.returncode:
                raise ValueError("non-fast-forward push: dispatch releases for the desired main commits explicitly")
            introduced = git("rev-list", "--first-parent", "--reverse", f"{before}..{arguments.after}").splitlines()
        else:
            introduced = [arguments.after]
    else:
        introduced = [validate_commit(arguments.commit or arguments.after)]
    supported = set()
    for commit in introduced:
        paths = ("scripts/release/package.py", "crates/launcher/Cargo.toml", ".github/workflows/client-release.yml")
        if all(subprocess.run(["git", "cat-file", "-e", f"{commit}:{path}"], capture_output=True).returncode == 0 for path in paths):
            supported.add(commit)
        else:
            print(f"Skipping {commit}: predates the launcher/release tooling.", file=sys.stderr)
    commits = select_commits(arguments.event, arguments.before, arguments.after, arguments.commit, history, introduced, supported)
    # Reserve immutable refs while the push head is still current. This also
    # avoids release-creation permission changes if main advances during builds.
    for commit in commits:
        try:
            reserve_tag(arguments.repository, commit)
        except RuntimeError as error:
            # Historical permission failures must not prevent another commit's
            # build/release. Its own publication job will retry with diagnostics.
            print(f"::warning::{error}", file=sys.stderr)
    matrix = {"include": [{"commit": commit, "target": target, "runner": runner} for commit in commits for target, runner in PLATFORMS]}
    output = f"commits={json.dumps(commits)}\nmatrix={json.dumps(matrix)}\n"
    with open(os.environ["GITHUB_OUTPUT"], "a") as destination:
        destination.write(output)
    print(f"Building {len(commits)} commit(s) on {len(PLATFORMS)} platforms.")


if __name__ == "__main__":
    main()
