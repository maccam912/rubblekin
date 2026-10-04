"""Exercise event planning against small real Git histories, without GitHub."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("plan.py").resolve()
RELEASE_SCRIPT = SCRIPT.parents[1] / "release" / "plan.py"
SPEC = importlib.util.spec_from_file_location("ci_plan", SCRIPT)
ci_plan = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci_plan)
import selection


class PlanTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        previous = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, previous)
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "CI Test")
        self.git("config", "user.email", "ci@example.invalid")
        self.initial = self.commit({
            "scripts/release/package.py": "# release support\n",
            "crates/launcher/Cargo.toml": "# launcher\n",
            ".github/workflows/client-release.yml": "# release workflow\n",
            "README.md": "first\n",
        })

    def git(self, *arguments):
        return subprocess.check_output(["git", *arguments], text=True, stderr=subprocess.PIPE).strip()

    def commit(self, files):
        for name, content in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        self.git("add", ".")
        self.git("commit", "-qm", "test change")
        result = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", result)
        return result

    def push(self, after, before=None, ref="refs/heads/main"):
        return ci_plan.plan("push", ref, before or self.initial, after)

    def test_push_checks_every_introduced_commit_but_not_later_main_head(self):
        middle = self.commit({"README.md": "second\n"})
        after = self.commit({"crates/core/src/lib.rs": "// changed core\n"})
        later = self.commit({"README.md": "fourth\n"})
        result = self.push(after)
        self.assertEqual(result["commits"], [middle, after])
        self.assertNotIn(later, result["commits"])
        self.assertEqual(len(result["client_matrix"]["include"]), 8)
        self.assertTrue(result["release_clients"])
        self.assertTrue(result["build_server"])

    def test_merge_checks_first_parent_commits_only(self):
        self.git("checkout", "-qb", "feature")
        side = self.commit({"feature.txt": "side branch\n"})
        self.git("checkout", "-q", "main")
        main = self.commit({"main.txt": "main branch\n"})
        self.git("merge", "--no-ff", "-qm", "merge feature", "feature")
        after = self.git("rev-parse", "HEAD")
        self.git("update-ref", "refs/remotes/origin/main", after)
        result = self.push(after)
        self.assertEqual(result["commits"], [main, after])
        self.assertNotIn(side, result["commits"])

    def test_documentation_change_still_releases_clients_without_server_build(self):
        after = self.commit({"README.md": "docs only\n"})
        result = self.push(after)
        self.assertTrue(result["release_clients"])
        self.assertFalse(result["build_server"])

    def test_server_input_filter_covers_sources_manifests_and_ci(self):
        paths = [
            "Cargo.toml", "Cargo.lock", "crates/core/src/lib.rs",
            "crates/server/src/main.rs", "crates/client/Cargo.toml",
            "crates/launcher/Cargo.toml", "Dockerfile", ".dockerignore",
            ".github/workflows/ci.yml", "scripts/ci/plan.py",
        ]
        before = self.initial
        for number, path in enumerate(paths):
            with self.subTest(path=path):
                after = self.commit({path: f"input {number}\n"})
                self.assertTrue(self.push(after, before)["build_server"])
                before = after
        for path in ("DESIGN.md", "crates/client/src/main.rs", "scripts/release/smoke.py", "scripts/city.py"):
            with self.subTest(path=path):
                after = self.commit({path: "client or docs only\n"})
                self.assertFalse(self.push(after, before)["build_server"])
                before = after

    def test_deleting_server_input_triggers_build(self):
        before = self.commit({"crates/server/src/old.rs": "// removed later\n"})
        (self.root / "crates/server/src/old.rs").unlink()
        after = self.commit({"README.md": "removal\n"})
        self.assertTrue(self.push(after, before)["build_server"])

    def test_initial_push_checks_head_only_and_builds_server(self):
        result = self.push(self.initial, selection.ZERO_SHA)
        self.assertEqual(result["commits"], [self.initial])
        self.assertTrue(result["build_server"])

    def test_historical_dispatch_checks_only_requested_commit_and_never_server(self):
        after = self.commit({"README.md": "new head\n"})
        result = ci_plan.plan("workflow_dispatch", "refs/heads/main", "", after, requested=self.initial)
        self.assertEqual(result["commits"], [self.initial])
        self.assertTrue(result["release_clients"])
        self.assertFalse(result["build_server"])
        result = ci_plan.plan("workflow_dispatch", "refs/heads/main", "", after, requested=after)
        self.assertFalse(result["build_server"])

    def test_default_dispatch_builds_head_clients_and_server(self):
        result = ci_plan.plan("workflow_dispatch", "refs/heads/main", "", self.initial)
        self.assertEqual(result["commits"], [self.initial])
        self.assertTrue(result["release_clients"])
        self.assertTrue(result["build_server"])

    def test_nonmain_push_and_dispatch_only_check_event_head(self):
        after = self.commit({"crates/core/src/lib.rs": "// source\n"})
        for event in ("push", "workflow_dispatch"):
            with self.subTest(event=event):
                result = ci_plan.plan(event, "refs/heads/feature", self.initial, after)
                self.assertEqual(result["commits"], [after])
                self.assertEqual(result["client_matrix"], {"include": []})
                self.assertFalse(result["release_clients"])
                self.assertFalse(result["build_server"])
        with self.assertRaisesRegex(ValueError, "requires refs/heads/main"):
            ci_plan.plan("workflow_dispatch", "refs/heads/feature", "", after, requested=self.initial)

    def test_pull_request_uses_merge_base_and_never_releases_clients(self):
        self.git("checkout", "-qb", "feature")
        after = self.commit({"README.md": "feature documentation\n"})
        self.git("checkout", "-q", "main")
        base = self.commit({"crates/core/src/lib.rs": "// main changed independently\n"})
        result = ci_plan.plan("pull_request", "refs/pull/1/merge", "", after, base=base)
        self.assertEqual(result["commits"], [after])
        self.assertFalse(result["release_clients"])
        self.assertFalse(result["build_server"])
        self.git("checkout", "-q", "feature")
        source = self.commit({"crates/server/src/main.rs": "// feature source\n"})
        result = ci_plan.plan("pull_request", "refs/pull/1/merge", "", source, base=base)
        self.assertTrue(result["build_server"])

    def test_missing_commit_and_nonfastforward_inputs_fail_clearly(self):
        missing = "f" * 40
        with self.assertRaisesRegex(ValueError, "before commit .* unavailable"):
            self.push(self.initial, missing)
        with self.assertRaisesRegex(ValueError, "after is a deleted"):
            self.push(selection.ZERO_SHA)
        after = self.commit({"README.md": "next\n"})
        with self.assertRaisesRegex(ValueError, "non-fast-forward"):
            self.push(self.initial, after)
        with self.assertRaisesRegex(ValueError, "requires --base"):
            ci_plan.plan("pull_request", "refs/pull/1/merge", "", after)

    def test_release_must_be_on_main_first_parent_history(self):
        self.git("checkout", "-qb", "feature")
        after = self.commit({"feature.txt": "outside main\n"})
        self.git("update-ref", "refs/remotes/origin/main", self.initial)
        with self.assertRaisesRegex(ValueError, "first-parent"):
            ci_plan.plan("workflow_dispatch", "refs/heads/main", "", self.initial, requested=after)

    def test_pretooling_commits_are_skipped(self):
        self.git("rm", "-q", "scripts/release/package.py")
        before_tooling = self.commit({"README.md": "before tooling\n"})
        after = self.commit({"scripts/release/package.py": "# support restored\n"})
        result = self.push(after)
        self.assertEqual(result["commits"], [after])
        self.assertNotIn(before_tooling, result["commits"])

    def test_platform_matrix_limit_is_enforced_by_shared_selection(self):
        commits = [f"{number + 1:040x}" for number in range(65)]
        selected = selection.select_commits("push", "", commits[63], "", commits, commits[:64], set(commits))
        self.assertEqual(len(selection.client_matrix(selected)["include"]), 256)
        with self.assertRaisesRegex(ValueError, "more than 64"):
            selection.select_commits("push", "", commits[-1], "", commits, commits, set(commits))

    def test_planning_executes_only_read_only_git_commands(self):
        after = self.commit({"crates/core/src/lib.rs": "// source\n"})
        refs = self.git("show-ref")
        status = self.git("status", "--porcelain")
        with patch("selection.subprocess.run", wraps=subprocess.run) as run:
            self.push(after)
        for call in run.call_args_list:
            command = call.args[0]
            self.assertEqual(command[0], "git")
            self.assertIn(command[1], {"cat-file", "rev-list", "merge-base", "diff"})
        self.assertEqual(self.git("show-ref"), refs)
        self.assertEqual(self.git("status", "--porcelain"), status)

    def test_cli_outputs_and_legacy_release_cli_remain_compatible(self):
        output = self.root / "github-output"
        environment = {**os.environ, "GITHUB_OUTPUT": str(output)}
        common = ["--event", "push", "--before", selection.ZERO_SHA, "--after", self.initial]
        subprocess.run([os.sys.executable, str(SCRIPT), "--ref", "refs/heads/main", *common], env=environment, check=True, capture_output=True)
        result = dict(line.split("=", 1) for line in output.read_text().splitlines())
        self.assertEqual(json.loads(result["commits"]), [self.initial])
        self.assertEqual(result["release_clients"], "true")
        self.assertEqual(result["build_server"], "true")
        self.assertEqual(len(json.loads(result["client_matrix"])["include"]), 4)
        output.unlink()
        subprocess.run([os.sys.executable, str(RELEASE_SCRIPT), *common, "--repository", "owner/repo"], env=environment, check=True, capture_output=True)
        legacy = dict(line.split("=", 1) for line in output.read_text().splitlines())
        self.assertEqual(legacy["commits"], result["commits"])
        self.assertEqual(legacy["matrix"], result["client_matrix"])


if __name__ == "__main__":
    unittest.main()
