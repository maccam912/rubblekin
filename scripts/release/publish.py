#!/usr/bin/env python3
"""Publish complete commit releases, then promote latest in a serialized job."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess

from package import ANDROID_ASSET, TARGETS, archive_name, assemble_manifest, validate_commit


def command(arguments, **kwargs):
    result = subprocess.run(arguments, text=True, capture_output=True, **kwargs)
    if result.returncode:
        raise RuntimeError(f"{arguments[0]} failed: {result.stderr.strip()}")
    return result.stdout


def api(repository, endpoint, method="GET", payload=None, missing_ok=False):
    arguments = ["gh", "api", "--method", method, f"repos/{repository}/{endpoint}"]
    if payload is not None:
        arguments += ["--input", "-"]
    try:
        content = command(arguments, input=json.dumps(payload) if payload is not None else None)
    except RuntimeError as error:
        if missing_ok and "(HTTP 404)" in str(error):
            return None
        permission_error = any(status in str(error) for status in ("HTTP 403", "HTTP 404"))
        fallback = os.environ.get("RELEASE_TOKEN")
        if method != "GET" and permission_error and fallback:
            # GITHUB_TOKEN cannot write historical workflow changes. Only use the
            # optional scoped credential after that ordinary token is rejected.
            environment = os.environ.copy()
            environment["GH_TOKEN"] = fallback
            content = command(arguments, input=json.dumps(payload), env=environment)
        else:
            raise
    return json.loads(content)


def should_promote(commit, latest_tag, history):
    """Commit ancestry, not completion time, determines the latest client."""
    if commit not in history:
        return False
    latest_commit = latest_tag.removeprefix("client-") if latest_tag else None
    if latest_commit not in history:
        return True
    return history.index(commit) < history.index(latest_commit)


def find_release(repository, tag):
    # The tags endpoint excludes drafts. The authenticated releases list includes
    # them, so a failed/interrupted upload can resume rather than create a duplicate.
    release = api(repository, f"releases/tags/{tag}", missing_ok=True)
    if release is not None:
        return release
    page = 1
    while True:
        releases = api(repository, f"releases?per_page=100&page={page}")
        for release in releases:
            if release["tag_name"] == tag:
                return release
        if len(releases) < 100:
            return None
        page += 1


def reserve_tag(repository, commit):
    validate_commit(commit)
    tag = f"client-{commit}"
    existing = api(repository, f"git/ref/tags/{tag}", missing_ok=True)
    if existing is not None:
        if existing["object"]["type"] != "commit" or existing["object"]["sha"] != commit:
            raise ValueError(f"{tag} already exists and does not point directly to {commit}")
        return
    try:
        api(repository, "git/refs", "POST", {"ref": f"refs/tags/{tag}", "sha": commit})
    except RuntimeError as error:
        raise RuntimeError(
            f"Could not reserve {tag}. GitHub can require Workflows write permission for historical "
            "commits whose workflow files differ from main. Configure the optional RELEASE_TOKEN "
            "repository secret with Contents and Workflows write permissions, or create/push the "
            f"lightweight tag {tag} at {commit} using an authorized account and rerun. {error}"
        ) from error


def publish(directory, commit, repository, android=False):
    validate_commit(commit)
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("invalid GitHub repository")
    manifest = assemble_manifest(directory, commit, require_android=android)
    tag = manifest["tag"]
    assets = [directory / "client-manifest.json", directory / "SHA256SUMS"]
    assets += [directory / archive_name(target, launcher) for target in TARGETS for launcher in (False, True)]
    if android:
        assets.append(directory / ANDROID_ASSET)
    release = find_release(repository, tag)
    if release is None:
        notes = (
            f"Automatic client build from `{commit}`.\n\n"
            "Download the **rubblekin-launcher** archive for your computer, extract it, and open the launcher. "
            "It installs the latest complete client release on startup. Client archives are also available for direct use.\n\n"
            "Targets: Linux x86-64 (built on Ubuntu 22.04), Windows x86-64, and macOS Apple Silicon / Intel. "
            "macOS applications use ad-hoc signing, without Developer ID notarization; first launch may need approval in "
            "System Settings → Privacy & Security. Windows may show an unsigned-app warning. "
            "Linux needs its normal desktop windowing libraries and a working graphics driver.\n\n"
            f"[Usage and current limits](https://github.com/{repository}/blob/{commit}/README.md). "
            "`SHA256SUMS` covers the archives and update manifest.\n"
        )
        if android:
            notes += (
                "\nAndroid: sideload the ARM64 APK on Android 8 or newer with Vulkan support. "
                "This prototype uses a public development signing key and is debuggable; it is not a store release. "
                "Successive builds retain the same signing identity so updates preserve local worlds.\n"
            )
        release = api(repository, "releases", "POST", {
            "tag_name": tag,
            "target_commitish": commit,
            "name": f"Client {commit[:12]}",
            "body": notes,
            "draft": True,
            "prerelease": False,
            "make_latest": "false",
        })
    if release["draft"]:
        command(["gh", "release", "upload", tag, *map(str, assets), "--repo", repository, "--clobber"])
        release = api(repository, f"releases/{release['id']}", "PATCH", {"draft": False, "make_latest": "false"})
    else:
        # Reruns never replace files in a published release.
        expected = {path.name for path in assets}
        uploaded = {asset["name"] for asset in release["assets"] if asset["state"] == "uploaded"}
        if not expected.issubset(uploaded):
            raise ValueError(f"published release {tag} is incomplete; repair it explicitly")
        print(f"{tag} already published; preserving its assets.")
    # This script runs under the workflow's shared publication concurrency group.
    # Refresh main immediately before comparing, including after force pushes.
    command(["git", "fetch", "--no-tags", "origin", "+refs/heads/main:refs/remotes/origin/main"])
    history = command(["git", "rev-list", "--first-parent", "origin/main"]).splitlines()
    latest = api(repository, "releases/latest", missing_ok=True)
    latest_tag = latest["tag_name"] if latest else None
    if should_promote(commit, latest_tag, history):
        api(repository, f"releases/{release['id']}", "PATCH", {"make_latest": "true"})
        print(f"Promoted {tag} to latest.")
    else:
        print(f"Kept latest at {latest_tag}; {tag} cannot move it backwards.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--android", action="store_true", help="Require and publish the Android APK alongside desktop packages")
    arguments = parser.parse_args()
    try:
        publish(arguments.directory, arguments.commit, arguments.repository, arguments.android)
    except RuntimeError as error:
        if any(status in str(error) for status in ("HTTP 403", "HTTP 404")):
            raise RuntimeError(
                f"GitHub rejected publication for client-{arguments.commit}. Historical commits that change "
                "workflow files can need the optional RELEASE_TOKEN secret with repository Contents and "
                "Workflows write permissions. Alternatively create/push the lightweight tag "
                f"client-{arguments.commit} at {arguments.commit} with an authorized account, then rerun. {error}"
            ) from error
        raise


if __name__ == "__main__":
    main()
