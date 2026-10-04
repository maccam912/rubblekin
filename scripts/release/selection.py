"""Read-only commit and platform selection shared by CI and release planning."""

import subprocess
import sys

from package import validate_commit


PLATFORMS = (
    ("x86_64-unknown-linux-gnu", "ubuntu-22.04"),
    ("x86_64-pc-windows-msvc", "windows-2022"),
    ("aarch64-apple-darwin", "macos-14"),
    ("x86_64-apple-darwin", "macos-15-intel"),
)
MAX_COMMITS = 64  # GitHub permits at most 256 matrix jobs, with four per commit.
ZERO_SHA = "0" * 40


def git(*arguments):
    result = subprocess.run(["git", *arguments], capture_output=True, text=True)
    if result.returncode:
        raise ValueError(f"git {' '.join(arguments)} failed: {result.stderr.strip()}")
    return result.stdout.strip()


def require_commit(commit, name):
    validate_commit(commit)
    if commit == ZERO_SHA:
        raise ValueError(f"{name} is a deleted or missing commit (all-zero SHA)")
    try:
        git("cat-file", "-e", f"{commit}^{{commit}}")
    except ValueError as error:
        raise ValueError(f"{name} commit {commit} is unavailable; fetch its history before planning") from error
    return commit


def introduced_commits(before, after):
    """Use the immutable event range, even when origin/main has advanced."""
    require_commit(after, "after")
    if not before or before == ZERO_SHA:
        return [after]
    require_commit(before, "before")
    ancestry = subprocess.run(
        ["git", "merge-base", "--is-ancestor", before, after], capture_output=True
    )
    if ancestry.returncode:
        raise ValueError("non-fast-forward push: dispatch releases for the desired main commits explicitly")
    return git("rev-list", "--first-parent", "--reverse", f"{before}..{after}").splitlines()


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


def release_commits(event, before, after, requested=""):
    require_commit(after, "after")
    history = git("rev-list", "--first-parent", "origin/main").splitlines()
    if event == "push":
        introduced = introduced_commits(before, after)
    elif event == "workflow_dispatch":
        introduced = [require_commit(requested or after, "requested")]
    else:
        raise ValueError("releases support push or workflow_dispatch events only")
    supported = set()
    for commit in introduced:
        paths = ("scripts/release/package.py", "crates/launcher/Cargo.toml", ".github/workflows/client-release.yml")
        if all(subprocess.run(["git", "cat-file", "-e", f"{commit}:{path}"], capture_output=True).returncode == 0 for path in paths):
            supported.add(commit)
        else:
            print(f"Skipping {commit}: predates the launcher/release tooling.", file=sys.stderr)
    return select_commits(event, before, after, requested, history, introduced, supported)


def client_matrix(commits):
    return {"include": [{"commit": commit, "target": target, "runner": runner} for commit in commits for target, runner in PLATFORMS]}
