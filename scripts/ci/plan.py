#!/usr/bin/env python3
"""Plan read-only checks and downstream builds for one GitHub event."""

import argparse
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "release"))
from selection import ZERO_SHA, client_matrix, git, release_commits, require_commit


SERVER_FILES = {
    "Cargo.toml", "Cargo.lock", "crates/client/Cargo.toml",
    "crates/launcher/Cargo.toml", "Dockerfile", ".dockerignore",
}
SERVER_DIRECTORIES = (
    "crates/core/", "crates/server/", ".github/workflows/", "scripts/ci/",
)


def server_inputs_changed(base, after):
    require_commit(base, "base")
    paths = git("diff", "--name-only", "-z", "--no-renames", base, after).split("\0")
    return any(path in SERVER_FILES or path.startswith(SERVER_DIRECTORIES) for path in paths)


def plan(event, ref, before, after, base="", requested=""):
    require_commit(after, "after")
    if requested and event != "workflow_dispatch":
        raise ValueError("--commit is only supported for workflow_dispatch")
    on_main = ref == "refs/heads/main"
    if requested and not on_main:
        raise ValueError("dispatching a release commit requires refs/heads/main")
    releases = on_main and event in ("push", "workflow_dispatch")
    commits = release_commits(event, before, after, requested) if releases else [after]

    build_server = False
    if event == "pull_request":
        if not base:
            raise ValueError("pull_request planning requires --base")
        require_commit(base, "base")
        merge_base = git("merge-base", base, after)
        build_server = server_inputs_changed(merge_base, after)
    elif event == "push" and on_main:
        build_server = not before or before == ZERO_SHA or server_inputs_changed(before, after)
    elif event == "workflow_dispatch" and on_main:
        # An explicit historical rebuild must not republish an old server image.
        build_server = not requested

    return {
        "commits": commits,
        "client_matrix": client_matrix(commits if releases else []),
        "release_clients": releases,
        "build_server": build_server,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event", choices=("push", "pull_request", "workflow_dispatch"), required=True)
    parser.add_argument("--ref", required=True)
    parser.add_argument("--before", default="")
    parser.add_argument("--after", required=True)
    parser.add_argument("--base", default="")
    parser.add_argument("--commit", default="")
    arguments = parser.parse_args()
    try:
        result = plan(arguments.event, arguments.ref, arguments.before, arguments.after, arguments.base, arguments.commit)
    except ValueError as error:
        parser.error(str(error))
    with open(os.environ["GITHUB_OUTPUT"], "a") as destination:
        for key, value in result.items():
            destination.write(f"{key}={json.dumps(value)}\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
