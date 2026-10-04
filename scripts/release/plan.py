#!/usr/bin/env python3
"""Enumerate the commits introduced onto main by this push."""

import argparse
import json
import os

# Re-export selection helpers for existing callers of this script.
from selection import MAX_COMMITS, PLATFORMS, client_matrix, release_commits, select_commits


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event", choices=("push", "workflow_dispatch"), required=True)
    parser.add_argument("--before", default="")
    parser.add_argument("--after", required=True)
    parser.add_argument("--commit", default="")
    # Retained for compatibility; planning no longer writes remote tags.
    parser.add_argument("--repository", default="")
    arguments = parser.parse_args()
    commits = release_commits(arguments.event, arguments.before, arguments.after, arguments.commit)
    matrix = client_matrix(commits)
    output = f"commits={json.dumps(commits)}\nmatrix={json.dumps(matrix)}\n"
    with open(os.environ["GITHUB_OUTPUT"], "a") as destination:
        destination.write(output)
    print(f"Building {len(commits)} commit(s) on {len(PLATFORMS)} platforms.")


if __name__ == "__main__":
    main()
