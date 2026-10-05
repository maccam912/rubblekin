package net.rubblekin.client;

import org.json.JSONObject;
import org.junit.Test;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InterruptedIOException;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.util.Collections;

import static org.junit.Assert.*;

public class UpdateChecksTest {
    private static final byte[] APK = "a small signed APK fixture".getBytes(StandardCharsets.UTF_8);

    private static JSONObject document() throws Exception {
        JSONObject client = new JSONObject()
                .put("asset", UpdateManifest.ASSET)
                .put("sha256", UpdateChecks.hex(UpdateChecks.sha256().digest(APK)))
                .put("size", APK.length)
                .put("package_id", UpdateManifest.PACKAGE)
                .put("signing", "public-development-key");
        String commit = "0123456789abcdef0123456789abcdef01234567";
        return new JSONObject().put("schema_version", 1).put("commit", commit)
                .put("tag", "client-" + commit).put("target", "aarch64-linux-android")
                .put("version_code", 15).put("client", client);
    }

    private static UpdateManifest release() throws Exception { return UpdateManifest.parse(document().toString()); }

    @Test public void downloadUrlIsDerivedAndUpdatesAreStrictlyNewer() throws Exception {
        JSONObject json = document().put("url", "https://attacker.invalid/fake.apk");
        UpdateManifest release = UpdateManifest.parse(json.toString());
        assertEquals("https://github.com/maccam912/rubblekin/releases/download/client-" + release.commit + "/" + UpdateManifest.ASSET,
                release.downloadUrl().toString());
        assertTrue(release.newerThan(14));
        assertFalse(release.newerThan(15));
        assertFalse(release.newerThan(16));
    }

    @Test public void versionsRejectCoercionAndOverflow() throws Exception {
        for (Object value : new Object[]{"15", true, 15.5, 0, -1, 2_147_483_648L}) {
            JSONObject json = document().put("version_code", value);
            assertThrows("version " + value, IOException.class, () -> UpdateManifest.parse(json.toString()));
        }
        assertEquals(Integer.MAX_VALUE, UpdateManifest.parse(document().put("version_code", Integer.MAX_VALUE).toString()).versionCode);
        String decimal = document().toString().replace("\"version_code\":15", "\"version_code\":15.0");
        assertThrows(IOException.class, () -> UpdateManifest.parse(decimal));
    }

    @Test public void assetIdentityAndMetadataLimitsAreEnforced() throws Exception {
        for (String field : new String[]{"asset", "package_id", "signing", "sha256"}) {
            JSONObject json = document();
            json.getJSONObject("client").put(field, "wrong");
            assertThrows(field, IOException.class, () -> UpdateManifest.parse(json.toString()));
        }
        for (Object size : new Object[]{"26", true, 26.5, 0, -1, UpdateManifest.MAX_APK_BYTES + 1}) {
            JSONObject json = document();
            json.getJSONObject("client").put("size", size);
            assertThrows("size " + size, IOException.class, () -> UpdateManifest.parse(json.toString()));
        }
        assertThrows(IOException.class, () -> UpdateManifest.parse(document().put("tag", "../../bad").toString()));
        assertThrows(IOException.class, () -> UpdateManifest.parse(document().put("commit", "A123456789abcdef0123456789abcdef01234567").toString()));
        assertThrows(IOException.class, () -> UpdateManifest.parse(document().put("schema_version", 2).toString()));
        assertThrows(IOException.class, () -> UpdateManifest.parse(document().put("target", "x86_64-linux-android").toString()));
    }

    @Test public void manifestReadsHaveAHardByteLimit() throws Exception {
        byte[] allowed = new byte[UpdateManifest.MAX_MANIFEST_BYTES];
        assertEquals(allowed.length, UpdateChecks.manifest(new ByteArrayInputStream(allowed)).length());
        byte[] excess = new byte[allowed.length + 1];
        assertThrows(IOException.class, () -> UpdateChecks.manifest(new ByteArrayInputStream(excess)));
    }

    @Test public void redirectsRemainOnHttpsGithubAssetHosts() throws Exception {
        UpdateChecks.trustedUrl(new URL("https://release-assets.githubusercontent.com/path?sig=123"));
        UpdateChecks.trustedUrl(new URL("https://github.com:443/path"));
        for (String url : new String[]{"http://github.com/path", "https://github.com.attacker.invalid/path",
                "https://attacker@github.com/path", "https://github.com:444/path", "https://example.com/path"}) {
            assertThrows(url, IOException.class, () -> UpdateChecks.trustedUrl(new URL(url)));
        }
    }

    @Test public void streamVerifiesExactBytesAndChecksum() throws Exception {
        UpdateManifest release = release();
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        long[] progress = {0};
        UpdateChecks.apk(new ByteArrayInputStream(APK), output, release, count -> progress[0] = count);
        assertArrayEquals(APK, output.toByteArray());
        assertEquals(APK.length, progress[0]);
        byte[] shortApk = java.util.Arrays.copyOf(APK, APK.length - 1);
        byte[] longApk = java.util.Arrays.copyOf(APK, APK.length + 1);
        byte[] corrupt = APK.clone();
        corrupt[2] ^= 1;
        for (byte[] invalid : new byte[][]{shortApk, longApk, corrupt}) {
            assertThrows(IOException.class, () -> UpdateChecks.apk(new ByteArrayInputStream(invalid),
                    new ByteArrayOutputStream(), release, count -> {}));
        }
    }

    @Test public void cancelledStreamStopsBeforeWriting() throws Exception {
        UpdateManifest release = release();
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        Thread.currentThread().interrupt();
        try {
            assertThrows(InterruptedIOException.class, () -> UpdateChecks.apk(new ByteArrayInputStream(APK), output, release, count -> {}));
            assertEquals(0, output.size());
        } finally { Thread.interrupted(); }
    }

    @Test public void apkIdentityRequiresInstalledCertificateAndNewerVersion() throws Exception {
        UpdateManifest release = release();
        java.util.Set<String> signer = Collections.singleton("current certificate");
        UpdateChecks.identity(UpdateManifest.PACKAGE, 15, signer, 14, signer, release);
        assertThrows(IOException.class, () -> UpdateChecks.identity("wrong.package", 15, signer, 14, signer, release));
        assertThrows(IOException.class, () -> UpdateChecks.identity(UpdateManifest.PACKAGE, 14, signer, 14, signer, release));
        assertThrows(IOException.class, () -> UpdateChecks.identity(UpdateManifest.PACKAGE, 15, signer, 15, signer, release));
        assertThrows(IOException.class, () -> UpdateChecks.identity(UpdateManifest.PACKAGE, 15,
                Collections.singleton("different certificate"), 14, signer, release));
        assertThrows(IOException.class, () -> UpdateChecks.identity(UpdateManifest.PACKAGE, 15,
                Collections.emptySet(), 14, Collections.emptySet(), release));
    }
}
