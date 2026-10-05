import json
from pathlib import Path
import plistlib
import stat
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import package


COMMIT = "a" * 40


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.binaries = self.root / "binaries"
        self.binaries.mkdir()
        for name in ("rubblekin", "rubblekin.exe", "rubblekin-launcher", "rubblekin-launcher.exe"):
            (self.binaries / name).write_bytes(b"fake executable")
        self.output = self.root / "dist"

    def all_packages(self):
        for target in package.TARGETS:
            package.package(self.binaries, self.output, target, COMMIT, sign=False)

    def test_all_platform_packages_match_manifest_and_keep_executable_mode(self):
        self.all_packages()
        manifest = package.assemble_manifest(self.output, COMMIT)
        self.assertEqual(set(manifest["platforms"]), set(package.TARGETS))
        self.assertEqual(manifest["tag"], f"client-{COMMIT}")
        for target, entry in manifest["platforms"].items():
            with zipfile.ZipFile(self.output / entry["asset"]) as archive:
                executable = archive.getinfo(entry["executable"])
                self.assertTrue(executable.external_attr >> 16 & stat.S_IXUSR)
                self.assertIn("OFL.txt", archive.namelist())
                self.assertEqual(archive.read(entry["executable"]), b"fake executable")
            with zipfile.ZipFile(self.output / package.archive_name(target, launcher=True)) as archive:
                self.assertIn(package.executable_path(target, launcher=True), archive.namelist())
        for checksum in (self.output / "SHA256SUMS").read_text().splitlines():
            expected, filename = checksum.split("  ")
            self.assertEqual(expected, package.digest(self.output / filename))

    def test_macos_bundle_owns_binary_and_valid_plist(self):
        target = "aarch64-apple-darwin"
        package.package(self.binaries, self.output, target, COMMIT, sign=False)
        for launcher in (False, True):
            with zipfile.ZipFile(self.output / package.archive_name(target, launcher)) as archive:
                executable = package.executable_path(target, launcher)
                bundle = executable.split("/", 1)[0]
                plist = plistlib.loads(archive.read(f"{bundle}/Contents/Info.plist"))
                self.assertEqual(plist["CFBundleExecutable"], Path(executable).name)
                self.assertEqual(plist["RubblekinCommit"], COMMIT)
                self.assertEqual(plist["CFBundlePackageType"], "APPL")
                if not launcher:
                    self.assertEqual(archive.read(f"{bundle}/Contents/Resources/OFL.txt"), archive.read("OFL.txt"))

    def test_manifest_rejects_incomplete_release(self):
        package.package(self.binaries, self.output, package.TARGETS[0], COMMIT, sign=False)
        with self.assertRaises(FileNotFoundError):
            package.assemble_manifest(self.output, COMMIT)

    def test_manifest_rejects_mixed_commits_and_archive_corruption(self):
        self.all_packages()
        metadata_path = self.output / f"{package.TARGETS[0]}.json"
        metadata = json.loads(metadata_path.read_text())
        metadata["commit"] = "b" * 40
        metadata_path.write_text(json.dumps(metadata))
        with self.assertRaisesRegex(ValueError, "metadata mismatch"):
            package.assemble_manifest(self.output, COMMIT)
        metadata["commit"] = COMMIT
        metadata_path.write_text(json.dumps(metadata))
        (self.output / metadata["client"]["asset"]).write_bytes(b"corrupted")
        with self.assertRaisesRegex(ValueError, "metadata mismatch"):
            package.assemble_manifest(self.output, COMMIT)

    def test_manifest_rejects_missing_launcher(self):
        self.all_packages()
        (self.output / package.archive_name(package.TARGETS[0], launcher=True)).unlink()
        with self.assertRaisesRegex(ValueError, "missing launcher"):
            package.assemble_manifest(self.output, COMMIT)

    def android_apk(self):
        apk = self.root / "android.apk"
        with zipfile.ZipFile(apk, "w") as archive:
            archive.writestr("AndroidManifest.xml", b"compiled manifest")
            archive.writestr("classes.dex", b"Java GameActivity")
            archive.writestr("lib/arm64-v8a/librubblekin_client.so", b"\x7fELFclient")
            archive.writestr("assets/OFL.txt", b"font license")
        return apk

    def test_android_apk_is_required_and_checksummed_without_changing_desktop_schema(self):
        self.all_packages()
        with self.assertRaises(FileNotFoundError):
            package.assemble_manifest(self.output, COMMIT, require_android=True)
        package.package_android(self.android_apk(), self.output, COMMIT)
        manifest = package.assemble_manifest(self.output, COMMIT, require_android=True)
        self.assertEqual(manifest["schema_version"], 1)
        self.assertEqual(set(manifest["platforms"]), set(package.TARGETS))
        self.assertIn(package.ANDROID_ASSET, (self.output / "SHA256SUMS").read_text())
        self.assertFalse((self.output / package.ANDROID_MANIFEST).exists())

    def updater_apk(self):
        with patch("package.subprocess.run", return_value=subprocess.CompletedProcess(
            [], 0, "package: name='net.rubblekin.client' versionCode='42' versionName='0.1.0-test'\n", ""
        )):
            return package.package_android(self.android_apk(), self.output, COMMIT, updater=True,
                                           aapt2=Path("sdk/aapt2"), expected_version_code=42)

    def test_updater_manifest_has_verified_apk_identity_and_independent_schema(self):
        self.all_packages()
        metadata = self.updater_apk()
        desktop = package.assemble_manifest(self.output, COMMIT, require_android_updater=True,
                                            expected_version_code=42)
        self.assertEqual(desktop["schema_version"], 1)
        self.assertEqual(set(desktop["platforms"]), set(package.TARGETS))
        self.assertNotIn("version_code", desktop)
        self.assertEqual(json.loads((self.output / package.ANDROID_MANIFEST).read_text()), {
            "schema_version": 1,
            "commit": COMMIT,
            "tag": f"client-{COMMIT}",
            "target": package.ANDROID_TARGET,
            "version_code": 42,
            "client": {
                "asset": package.ANDROID_ASSET,
                "sha256": package.digest(self.output / package.ANDROID_ASSET),
                "size": (self.output / package.ANDROID_ASSET).stat().st_size,
                "package_id": "net.rubblekin.client",
                "signing": "public-development-key",
            },
        })
        self.assertEqual(metadata["version_code"], 42)
        checksums = dict(line.split("  ")[::-1] for line in (self.output / "SHA256SUMS").read_text().splitlines())
        self.assertEqual(len(checksums), 11)
        self.assertEqual(checksums[package.ANDROID_MANIFEST], package.digest(self.output / package.ANDROID_MANIFEST))
        self.assertEqual(checksums[package.ANDROID_ASSET], package.digest(self.output / package.ANDROID_ASSET))

    def test_updater_release_requires_new_metadata_and_first_parent_count(self):
        self.all_packages()
        package.package_android(self.android_apk(), self.output, COMMIT)
        with self.assertRaisesRegex(ValueError, "positive integer"):
            package.assemble_manifest(self.output, COMMIT, require_android_updater=True, expected_version_code=42)
        self.updater_apk()
        with self.assertRaisesRegex(ValueError, "first-parent count"):
            package.assemble_manifest(self.output, COMMIT, require_android_updater=True, expected_version_code=43)
        with self.assertRaisesRegex(ValueError, "positive integer"):
            package.assemble_manifest(self.output, COMMIT, require_android_updater=True)

    def test_updater_metadata_rejects_version_size_digest_and_identity_changes(self):
        self.all_packages()
        original = self.updater_apk()
        metadata_path = self.output / f"{package.ANDROID_TARGET}.json"
        changes = [
            ("version_code", None), ("version_code", True), ("version_code", 0),
            ("version_code", -1), ("version_code", "42"), ("version_code", 42.0),
            ("version_code", package.ANDROID_MAX_VERSION_CODE + 1),
            ("commit", "b" * 40), ("target", package.TARGETS[0]), ("extra", "unsupported"),
            ("client.size", original["client"]["size"] + 1),
            ("client.size", float(original["client"]["size"])),
            ("client.size", 0), ("client.size", True), ("client.size", "1"),
            ("client.size", package.ANDROID_MAX_APK_SIZE + 1),
            ("client.sha256", "b" * 64), ("client.package_id", "net.other.client"),
            ("client.asset", "other.apk"), ("client.signing", "unknown-key"),
        ]
        for field, value in changes:
            with self.subTest(field=field, value=value):
                metadata = json.loads(json.dumps(original))
                if field.startswith("client."):
                    metadata["client"][field.split(".")[1]] = value
                else:
                    metadata[field] = value
                metadata_path.write_text(json.dumps(metadata))
                with self.assertRaises(ValueError):
                    package.assemble_manifest(self.output, COMMIT, require_android_updater=True,
                                              expected_version_code=42)

    def test_updater_apk_is_inspected_with_aapt2_before_metadata_is_written(self):
        apk = self.android_apk()
        result = subprocess.CompletedProcess([], 0,
            "package: name='net.rubblekin.client' versionCode='42' versionName='0.1.0-test'\n", "")
        with patch("package.subprocess.run", return_value=result) as run:
            metadata = package.package_android(apk, self.output, COMMIT, updater=True,
                                               aapt2=Path("sdk/aapt2"), expected_version_code=42)
        run.assert_called_once_with(["sdk/aapt2", "dump", "badging", str(apk)], capture_output=True, text=True)
        self.assertEqual(metadata["version_code"], 42)
        self.assertEqual(metadata["client"]["size"], apk.stat().st_size)

    def test_updater_apk_inspection_rejects_wrong_or_unreadable_compiled_metadata(self):
        apk = self.android_apk()
        bad_results = [
            subprocess.CompletedProcess([], 1, "", "invalid compiled manifest"),
            subprocess.CompletedProcess([], 0, "", ""),
            subprocess.CompletedProcess([], 0, "package: name='net.other.client' versionCode='42'\n", ""),
            subprocess.CompletedProcess([], 0, "package: name='net.rubblekin.client' versionCode='0'\n", ""),
            subprocess.CompletedProcess([], 0, "package: name='net.rubblekin.client' versionCode='-1'\n", ""),
            subprocess.CompletedProcess([], 0, "package: name='net.rubblekin.client' versionCode='43'\n", ""),
        ]
        for result in bad_results:
            with self.subTest(result=result), patch("package.subprocess.run", return_value=result):
                with self.assertRaises(ValueError):
                    package.package_android(apk, self.output, COMMIT, updater=True,
                                            aapt2=Path("sdk/aapt2"), expected_version_code=42)
                self.assertFalse(self.output.exists())

    def test_updater_metadata_requires_inspection_tool_and_expected_version(self):
        apk = self.android_apk()
        with self.assertRaisesRegex(ValueError, "aapt2"):
            package.package_android(apk, self.output, COMMIT, updater=True, expected_version_code=42)
        for expected in (None, 0, -1, True, "42", package.ANDROID_MAX_VERSION_CODE + 1):
            with self.subTest(expected=expected), patch("package.subprocess.run") as run:
                with self.assertRaisesRegex(ValueError, "positive integer"):
                    package.package_android(apk, self.output, COMMIT, updater=True,
                                            aapt2=Path("sdk/aapt2"), expected_version_code=expected)
                run.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_android_apk_commit_and_checksum_must_match_release(self):
        self.all_packages()
        package.package_android(self.android_apk(), self.output, "b" * 40)
        with self.assertRaisesRegex(ValueError, "metadata mismatch for Android"):
            package.assemble_manifest(self.output, COMMIT, require_android=True)
        package.package_android(self.android_apk(), self.output, COMMIT)
        (self.output / package.ANDROID_ASSET).write_bytes(b"damaged transfer")
        with self.assertRaisesRegex(ValueError, "metadata mismatch for Android"):
            package.assemble_manifest(self.output, COMMIT, require_android=True)

    def test_android_package_requires_native_client_and_license(self):
        apk = self.root / "incomplete.apk"
        with zipfile.ZipFile(apk, "w") as archive:
            archive.writestr("AndroidManifest.xml", b"manifest")
        with self.assertRaisesRegex(ValueError, "APK is missing"):
            package.package_android(apk, self.output, COMMIT)

    def test_invalid_commit_is_rejected_before_writing(self):
        for commit in ("a" * 7, "A" * 40, "../bad", "g" * 40):
            with self.subTest(commit=commit), self.assertRaises(ValueError):
                package.package(self.binaries, self.output, package.TARGETS[0], commit, sign=False)
        self.assertFalse(self.output.exists())

    def test_symlinks_are_not_archived(self):
        # Windows CI may not permit creating a symlink without Developer Mode.
        try:
            (self.binaries / "symlink").symlink_to(self.binaries / "rubblekin")
        except OSError:
            self.skipTest("creating symlinks is not permitted")
        with self.assertRaisesRegex(ValueError, "symlink"):
            package.write_zip(self.binaries, self.root / "archive.zip")


if __name__ == "__main__":
    unittest.main()
