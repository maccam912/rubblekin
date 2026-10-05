package net.rubblekin.client;

import org.json.JSONException;
import org.json.JSONObject;

import java.io.IOException;
import java.net.URL;

/** The release contract intentionally has no caller-controlled download URL. */
final class UpdateManifest {
    static final String MANIFEST_URL = "https://github.com/maccam912/rubblekin/releases/latest/download/android-manifest.json";
    static final String PACKAGE = "net.rubblekin.client";
    static final String ASSET = "rubblekin-client-aarch64-linux-android.apk";
    static final int MAX_MANIFEST_BYTES = 64 * 1024;
    static final long MAX_APK_BYTES = 256L * 1024 * 1024;

    final String commit;
    final String tag;
    final long versionCode;
    final String sha256;
    final long size;
    final String json;

    private UpdateManifest(String commit, String tag, long versionCode, String sha256, long size, String json) {
        this.commit = commit;
        this.tag = tag;
        this.versionCode = versionCode;
        this.sha256 = sha256;
        this.size = size;
        this.json = json;
    }

    static UpdateManifest parse(String source) throws IOException {
        try {
            JSONObject document = new JSONObject(source);
            require(integer(document, "schema_version") == 1, "Unsupported update manifest");
            String commit = string(document, "commit");
            require(commit.matches("[0-9a-f]{40}"), "Invalid release commit");
            String tag = string(document, "tag");
            require(tag.equals("client-" + commit), "Invalid release tag");
            require(string(document, "target").equals("aarch64-linux-android"), "Wrong update platform");
            long version = integer(document, "version_code");
            require(version > 0 && version <= Integer.MAX_VALUE, "Invalid update version");
            JSONObject client = document.getJSONObject("client");
            require(string(client, "asset").equals(ASSET), "Unexpected update asset");
            require(string(client, "package_id").equals(PACKAGE), "Wrong update package");
            require(string(client, "signing").equals("public-development-key"), "Unexpected signing identity");
            String hash = string(client, "sha256");
            require(hash.matches("[0-9a-f]{64}"), "Invalid update checksum");
            long size = integer(client, "size");
            require(size > 0 && size <= MAX_APK_BYTES, "Invalid update size");
            return new UpdateManifest(commit, tag, version, hash, size, source);
        } catch (JSONException exception) {
            throw new IOException("Invalid update manifest", exception);
        }
    }

    URL downloadUrl() throws IOException {
        return new URL("https://github.com/maccam912/rubblekin/releases/download/" + tag + "/" + ASSET);
    }

    boolean newerThan(long installedVersion) { return versionCode > installedVersion; }

    private static long integer(JSONObject object, String key) throws JSONException, IOException {
        Object value = object.get(key);
        require(value instanceof Integer || value instanceof Long, "Invalid integer: " + key);
        return ((Number) value).longValue();
    }

    private static String string(JSONObject object, String key) throws JSONException, IOException {
        Object value = object.get(key);
        require(value instanceof String, "Invalid string: " + key);
        return (String) value;
    }

    static void require(boolean valid, String message) throws IOException {
        if (!valid) throw new IOException(message);
    }
}
