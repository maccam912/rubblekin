import json
from pathlib import Path
import plistlib
import stat
import tempfile
import unittest
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
