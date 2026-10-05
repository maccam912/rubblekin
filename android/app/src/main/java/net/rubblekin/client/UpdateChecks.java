package net.rubblekin.client;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InterruptedIOException;
import java.io.OutputStream;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Set;

/** Small, platform-independent limits shared by network downloads and cache revalidation. */
final class UpdateChecks {
    interface Progress { void bytes(long count) throws IOException; }

    static void trustedUrl(URL url) throws IOException {
        java.util.List<String> hosts = java.util.Arrays.asList("github.com", "release-assets.githubusercontent.com",
                "objects.githubusercontent.com", "github-releases.githubusercontent.com");
        UpdateManifest.require(url.getProtocol().equals("https") && url.getUserInfo() == null
                && (url.getPort() == -1 || url.getPort() == 443) && hosts.contains(url.getHost()),
                "Untrusted update redirect");
    }

    static String manifest(InputStream input) throws IOException {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        byte[] buffer = new byte[8192];
        int count;
        while ((count = input.read(buffer)) != -1) {
            cancelled();
            UpdateManifest.require(bytes.size() + count <= UpdateManifest.MAX_MANIFEST_BYTES,
                    "Update manifest is too large");
            bytes.write(buffer, 0, count);
        }
        return bytes.toString(StandardCharsets.UTF_8.name());
    }

    static void apk(InputStream input, OutputStream output, UpdateManifest release, Progress progress) throws IOException {
        MessageDigest digest = sha256();
        byte[] buffer = new byte[64 * 1024];
        long total = 0;
        int count;
        while ((count = input.read(buffer)) != -1) {
            cancelled();
            total += count;
            UpdateManifest.require(total <= release.size, "Update is larger than its manifest");
            digest.update(buffer, 0, count);
            output.write(buffer, 0, count);
            progress.bytes(total);
        }
        UpdateManifest.require(total == release.size, "Update download is incomplete");
        UpdateManifest.require(hex(digest.digest()).equals(release.sha256), "Update checksum does not match");
    }

    static void identity(String packageName, long version, Set<String> apkSigners,
                         long installedVersion, Set<String> installedSigners, UpdateManifest release) throws IOException {
        UpdateManifest.require(UpdateManifest.PACKAGE.equals(packageName), "Downloaded APK has the wrong package");
        UpdateManifest.require(version == release.versionCode, "Downloaded APK has the wrong version");
        UpdateManifest.require(release.newerThan(installedVersion), "This update is already installed");
        UpdateManifest.require(!apkSigners.isEmpty() && apkSigners.equals(installedSigners),
                "Downloaded APK signing certificate does not match the installed game");
    }

    static MessageDigest sha256() {
        try { return MessageDigest.getInstance("SHA-256"); }
        catch (NoSuchAlgorithmException exception) { throw new AssertionError(exception); }
    }

    static String hex(byte[] bytes) {
        StringBuilder result = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) result.append(String.format(java.util.Locale.ROOT, "%02x", value & 255));
        return result.toString();
    }

    static void cancelled() throws InterruptedIOException {
        if (Thread.currentThread().isInterrupted()) throw new InterruptedIOException("Update cancelled");
    }
}
