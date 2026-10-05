from pathlib import Path
import os
import unittest
from unittest.mock import patch

import plan
import publish


OLD = "1" * 40
MIDDLE = "2" * 40
NEW = "3" * 40
HISTORY = [NEW, MIDDLE, OLD]


class PlanTests(unittest.TestCase):
    def test_batches_every_new_first_parent_commit_in_order(self):
        commits = plan.select_commits("push", OLD, NEW, "", HISTORY, [MIDDLE, NEW], set(HISTORY))
        self.assertEqual(commits, [MIDDLE, NEW])

    def test_bootstrap_skips_commits_without_release_tooling(self):
        commits = plan.select_commits("push", OLD, NEW, "", HISTORY, [MIDDLE, NEW], {NEW})
        self.assertEqual(commits, [NEW])

    def test_dispatch_can_recover_an_older_main_commit(self):
        commits = plan.select_commits("workflow_dispatch", "", NEW, OLD, HISTORY, [OLD], set(HISTORY))
        self.assertEqual(commits, [OLD])

    def test_rejects_non_main_commit_and_oversized_matrix(self):
        with self.assertRaisesRegex(ValueError, "first-parent"):
            plan.select_commits("push", OLD, NEW, "", [OLD], [NEW], {NEW})
        commits = [f"{number:040x}" for number in range(65)]
        with self.assertRaisesRegex(ValueError, "64"):
            plan.select_commits("push", OLD, NEW, "", commits, commits, set(commits))


class PromotionTests(unittest.TestCase):
    def test_out_of_order_build_completion_cannot_roll_latest_back(self):
        self.assertTrue(publish.should_promote(NEW, f"client-{OLD}", HISTORY))
        self.assertFalse(publish.should_promote(MIDDLE, f"client-{NEW}", HISTORY))
        self.assertFalse(publish.should_promote(OLD, f"client-{NEW}", HISTORY))

    def test_old_in_flight_build_after_force_push_cannot_become_latest(self):
        self.assertFalse(publish.should_promote(OLD, None, [NEW]))
        self.assertTrue(publish.should_promote(NEW, f"client-{OLD}", [NEW]))

    def test_no_existing_release_and_same_release_rerun(self):
        self.assertTrue(publish.should_promote(NEW, None, HISTORY))
        self.assertFalse(publish.should_promote(NEW, f"client-{NEW}", HISTORY))

    @patch("publish.assemble_manifest", return_value={"tag": f"client-{NEW}"})
    @patch("publish.command")
    @patch("publish.api")
    def test_draft_upload_finishes_before_publication_and_promotion(self, api, command, _manifest):
        api.side_effect = [None, [], {"id": 7, "draft": True}, {"id": 7, "draft": False}, {"tag_name": f"client-{OLD}"}, {}]
        command.side_effect = ["", "", "\n".join(HISTORY)]
        publish.publish(Path("dist"), NEW, "owner/repository")
        self.assertEqual(api.call_args_list[2].args[3]["make_latest"], "false")
        self.assertEqual(api.call_args_list[3].args[3], {"draft": False, "make_latest": "false"})
        self.assertEqual(api.call_args_list[-1].args[3], {"make_latest": "true"})
        self.assertEqual(command.call_args_list[0].args[0][:3], ["gh", "release", "upload"])

    @patch("publish.assemble_manifest", return_value={"tag": f"client-{NEW}"})
    @patch("publish.command", side_effect=RuntimeError("upload failed"))
    @patch("publish.api")
    def test_upload_failure_leaves_draft_unpublished(self, api, _command, _manifest):
        api.side_effect = [None, [], {"id": 7, "draft": True}]
        with self.assertRaisesRegex(RuntimeError, "upload failed"):
            publish.publish(Path("dist"), NEW, "owner/repository")
        self.assertEqual(api.call_count, 3)

    @patch("publish.assemble_manifest", return_value={"tag": f"client-{NEW}"})
    @patch("publish.command")
    @patch("publish.api")
    def test_interrupted_draft_upload_resumes_existing_release(self, api, command, _manifest):
        draft = {"id": 7, "draft": True, "tag_name": f"client-{NEW}"}
        api.side_effect = [None, [draft], {"id": 7, "draft": False}, {"tag_name": f"client-{NEW}"}]
        command.side_effect = ["", "", "\n".join(HISTORY)]
        publish.publish(Path("dist"), NEW, "owner/repository")
        self.assertFalse(any(len(call.args) > 2 and call.args[2] == "POST" for call in api.call_args_list))
        self.assertEqual(api.call_args_list[2].args[1], "releases/7")
        self.assertEqual(command.call_args_list[0].args[0][:3], ["gh", "release", "upload"])

    @patch("publish.assemble_manifest", return_value={"tag": f"client-{NEW}"})
    @patch("publish.command")
    @patch("publish.api", return_value={"id": 7, "draft": False, "assets": []})
    def test_published_incomplete_release_cannot_be_clobbered(self, _api, command, _manifest):
        with self.assertRaisesRegex(ValueError, "incomplete"):
            publish.publish(Path("dist"), NEW, "owner/repository")
        command.assert_not_called()

    @patch("publish.assemble_manifest", return_value={"tag": f"client-{NEW}"})
    @patch("publish.command")
    @patch("publish.api")
    def test_android_release_requires_and_uploads_apk(self, api, command, manifest):
        api.side_effect = [None, [], {"id": 7, "draft": True}, {"id": 7, "draft": False}, {"tag_name": f"client-{NEW}"}]
        command.side_effect = ["", "", "\n".join(HISTORY)]
        publish.publish(Path("dist"), NEW, "owner/repository", android=True)
        manifest.assert_called_once_with(Path("dist"), NEW, require_android=True,
                                         require_android_updater=False, expected_version_code=None)
        self.assertIn("dist/" + publish.ANDROID_ASSET, command.call_args_list[0].args[0])
        self.assertNotIn("dist/" + publish.ANDROID_MANIFEST, command.call_args_list[0].args[0])
        self.assertIn("public development signing key", api.call_args_list[2].args[3]["body"])
        self.assertNotIn("checks for updates before play", api.call_args_list[2].args[3]["body"])

    @patch("publish.assemble_manifest", return_value={"tag": f"client-{NEW}"})
    @patch("publish.command")
    @patch("publish.api")
    def test_updater_release_requires_and_uploads_separate_android_manifest(self, api, command, manifest):
        api.side_effect = [None, [], {"id": 7, "draft": True}, {"id": 7, "draft": False}, {"tag_name": f"client-{NEW}"}]
        command.side_effect = ["", "", "\n".join(HISTORY)]
        publish.publish(Path("dist"), NEW, "owner/repository", android_updater=True, expected_version_code=42)
        manifest.assert_called_once_with(Path("dist"), NEW, require_android=False,
                                         require_android_updater=True, expected_version_code=42)
        assets = command.call_args_list[0].args[0]
        self.assertIn("dist/" + publish.ANDROID_ASSET, assets)
        self.assertIn("dist/" + publish.ANDROID_MANIFEST, assets)
        self.assertIn("dist/client-manifest.json", assets)
        self.assertIn("dist/SHA256SUMS", assets)
        self.assertIn("checks for updates before play", api.call_args_list[2].args[3]["body"])

    @patch("publish.assemble_manifest", return_value={"tag": f"client-{NEW}"})
    @patch("publish.command")
    @patch("publish.api")
    def test_published_updater_release_requires_manifest_even_if_all_other_assets_exist(self, api, command, _manifest):
        filenames = ["client-manifest.json", "SHA256SUMS", publish.ANDROID_ASSET]
        filenames += [publish.archive_name(target, launcher) for target in publish.TARGETS for launcher in (False, True)]
        api.return_value = {"id": 7, "draft": False, "assets": [
            {"name": name, "state": "uploaded"} for name in filenames
        ]}
        with self.assertRaisesRegex(ValueError, "incomplete"):
            publish.publish(Path("dist"), NEW, "owner/repository", android_updater=True, expected_version_code=42)
        command.assert_not_called()

    @patch("publish.assemble_manifest", return_value={"tag": f"client-{NEW}"})
    @patch("publish.command")
    @patch("publish.api")
    def test_complete_published_updater_release_preserves_immutable_assets(self, api, command, _manifest):
        filenames = ["client-manifest.json", "SHA256SUMS", publish.ANDROID_ASSET, publish.ANDROID_MANIFEST]
        filenames += [publish.archive_name(target, launcher) for target in publish.TARGETS for launcher in (False, True)]
        api.side_effect = [{"id": 7, "draft": False, "assets": [
            {"name": name, "state": "uploaded"} for name in filenames
        ]}, {"tag_name": f"client-{NEW}"}]
        command.side_effect = ["", "\n".join(HISTORY)]
        publish.publish(Path("dist"), NEW, "owner/repository", android_updater=True, expected_version_code=42)
        self.assertFalse(any(call.args[0][:3] == ["gh", "release", "upload"] for call in command.call_args_list))


class TagAndPermissionTests(unittest.TestCase):
    @patch("publish.api")
    def test_reserves_immutable_commit_ref(self, api):
        api.side_effect = [None, {}]
        publish.reserve_tag("owner/repository", NEW)
        self.assertEqual(api.call_args_list[-1].args[3], {"ref": f"refs/tags/client-{NEW}", "sha": NEW})

    @patch("publish.api")
    def test_existing_ref_is_never_moved(self, api):
        api.return_value = {"object": {"type": "commit", "sha": OLD}}
        with self.assertRaisesRegex(ValueError, "does not point"):
            publish.reserve_tag("owner/repository", NEW)
        self.assertEqual(api.call_count, 1)

    @patch.dict(os.environ, {"RELEASE_TOKEN": "test-fallback-credential"})
    @patch("publish.command")
    def test_permission_fallback_is_only_used_after_mutation_rejection(self, command):
        command.side_effect = [RuntimeError("HTTP 403"), "{}"]
        publish.api("owner/repository", "git/refs", "POST", {"ref": "example"})
        self.assertNotIn("env", command.call_args_list[0].kwargs)
        self.assertEqual(command.call_args_list[1].kwargs["env"]["GH_TOKEN"], "test-fallback-credential")

    @patch.dict(os.environ, {"RELEASE_TOKEN": "test-fallback-credential"})
    @patch("publish.command", side_effect=RuntimeError("HTTP 500"))
    def test_server_failure_does_not_escalate_credentials(self, command):
        with self.assertRaises(RuntimeError):
            publish.api("owner/repository", "git/refs", "POST", {})
        self.assertEqual(command.call_count, 1)


if __name__ == "__main__":
    unittest.main()
