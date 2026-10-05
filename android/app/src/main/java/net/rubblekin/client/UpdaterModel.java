package net.rubblekin.client;

import android.app.Application;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.content.pm.Signature;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import androidx.annotation.NonNull;
import androidx.lifecycle.AndroidViewModel;
import androidx.lifecycle.LiveData;
import androidx.lifecycle.MutableLiveData;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.HashSet;
import java.util.Set;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

/** One retained worker; only the activity opens Settings, the installer, or the game. */
public final class UpdaterModel extends AndroidViewModel {
    enum Phase { IDLE, CHECKING, AVAILABLE, DOWNLOADING, VERIFYING, READY, CURRENT, ERROR }

    static final class State {
        final Phase phase;
        final String status;
        final UpdateManifest release;
        final int progress;
        final boolean installRequested;

        State(Phase phase, String status, UpdateManifest release, int progress, boolean installRequested) {
            this.phase = phase;
            this.status = status;
            this.release = release;
            this.progress = progress;
            this.installRequested = installRequested;
        }

        boolean busy() { return phase == Phase.CHECKING || phase == Phase.DOWNLOADING || phase == Phase.VERIFYING; }
    }

    private final MutableLiveData<State> states = new MutableLiveData<>();
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private final Handler main = new Handler(Looper.getMainLooper());
    private volatile int generation;
    private volatile HttpURLConnection connection;
    private Future<?> pending;
    private State state;
    private final File directory;

    public UpdaterModel(@NonNull Application application) {
        super(application);
        directory = new File(application.getCacheDir(), "updates");
        state = new State(Phase.IDLE, "Ready to check for updates.", null, 0, false);
        states.setValue(state);
    }

    LiveData<State> states() { return states; }

    String installedLabel() {
        try {
            PackageInfo info = installed();
            return "Installed " + info.versionName + " · build " + version(info);
        } catch (PackageManager.NameNotFoundException exception) {
            return "Installed game";
        }
    }

    void beginIfNeeded() {
        if (state.phase != Phase.IDLE) return;
        submit((token) -> {
            // A process restart preserves a completed download, never an installer request.
            UpdateManifest cached = cachedRelease();
            cleanupCache(cached);
            if (cached != null && cached.newerThan(version(installed())) && apkFile(cached).isFile()) {
                publish(token, new State(Phase.READY, "Downloaded update is ready to install.", cached, 100, false));
            } else {
                check(token);
            }
        }, new State(Phase.CHECKING, "Checking the latest release…", null, 0, false));
    }

    void retry() {
        if (state.busy()) return;
        submit(this::check, new State(Phase.CHECKING, "Checking the latest release…", null, 0, false));
    }

    void update() {
        if (state.busy() || state.release == null) return;
        UpdateManifest release = state.release;
        if (apkFile(release).isFile()) prepareInstall();
        else submit((token) -> download(token, release),
                new State(Phase.DOWNLOADING, "Downloading update…", release, 0, false));
    }

    void prepareInstall() {
        if (state.busy() || state.release == null) return;
        UpdateManifest release = state.release;
        submit((token) -> {
            try { verify(apkFile(release), release); }
            catch (IOException exception) {
                // A rejected cache entry must not trap Retry in a permanent verification loop.
                if (!Thread.currentThread().isInterrupted()) {
                    apkFile(release).delete();
                    UpdateManifest cached = cachedRelease();
                    if (cached != null && cached.commit.equals(release.commit)) new File(directory, "ready.json").delete();
                }
                throw exception;
            }
            publish(token, new State(Phase.READY, "Confirm installation in Android.", release, 100, true));
        }, new State(Phase.VERIFYING, "Verifying the update before installation…", release, 100, false));
    }

    // Consumed before leaving the activity, so recreation cannot reopen an installer.
    boolean consumeInstallRequest(State request) {
        if (state != request || !request.installRequested) return false;
        set(new State(Phase.READY, "Update is ready. Play installed at any time.", request.release, 100, false));
        return true;
    }

    File readyApk() { return state.release == null ? null : apkFile(state.release); }

    void installPermissionDenied() { notice("Installation permission was not enabled. You can retry or play installed."); }
    void externalFailure(String message) { notice(message); }

    void installerReturned() {
        UpdateManifest release = state.release;
        if (release == null) return;
        try {
            if (!release.newerThan(version(installed()))) {
                set(new State(Phase.CURRENT, "The update is installed. Ready to play.", null, 0, false));
            } else notice("Update was not installed. You can retry installation or play installed.");
        } catch (PackageManager.NameNotFoundException exception) {
            notice("Could not check the installed version. You can still play installed.");
        }
    }

    void cancelForPlay() {
        cancelWork();
    }

    void cancelDownload() {
        if (!state.busy()) return;
        cancelWork();
        set(new State(Phase.ERROR, "Download cancelled. Play installed or retry.", state.release, 0, false));
    }

    private void notice(String message) {
        set(new State(state.release == null ? Phase.ERROR : Phase.READY, message, state.release, state.progress, false));
    }

    private interface Operation { void run(int token) throws Exception; }

    private void submit(Operation operation, State initial) {
        cancelWork();
        int token = generation;
        set(initial);
        pending = worker.submit(() -> {
            try { operation.run(token); }
            catch (Exception exception) {
                // SocketTimeoutException also extends InterruptedIOException: report timeouts as failures.
                if (Thread.currentThread().isInterrupted()) return;
                Log.w("RubblekinUpdater", "Update operation failed", exception);
                publish(token, new State(Phase.ERROR,
                        "The update couldn't complete. Play installed or retry.", initial.release, 0, false));
            }
        });
    }

    private void check(int token) throws Exception {
        String json;
        long deadline = System.nanoTime() + 120_000_000_000L;
        HttpURLConnection request = open(new URL(UpdateManifest.MANIFEST_URL), deadline);
        try (InputStream input = request.getInputStream()) {
            json = UpdateChecks.manifest(new DeadlineStream(input, deadline));
        } finally { close(request); }
        UpdateManifest release = UpdateManifest.parse(json);
        if (!release.newerThan(version(installed()))) {
            publish(token, new State(Phase.CURRENT, "The installed game is up to date.", null, 0, false));
        } else {
            boolean ready = apkFile(release).isFile();
            publish(token, new State(ready ? Phase.READY : Phase.AVAILABLE,
                    ready ? "Downloaded update is ready to install."
                            : "Build " + release.versionCode + " is available (" + megabytes(release.size) + " MB).",
                    release, ready ? 100 : 0, false));
        }
    }

    private void download(int token, UpdateManifest release) throws Exception {
        UpdateManifest.require(directory.isDirectory() || directory.mkdirs(), "Cannot create update cache");
        File partial = File.createTempFile("download-", ".part", directory);
        long deadline = System.nanoTime() + 1_200_000_000_000L;
        HttpURLConnection request = null;
        try {
            request = open(release.downloadUrl(), deadline);
            long length = request.getContentLengthLong();
            UpdateManifest.require(length == -1 || length == release.size, "Update size differs from its manifest");
            int[] previousPercent = {-1};
            try (InputStream input = request.getInputStream(); OutputStream output = new FileOutputStream(partial)) {
                UpdateChecks.apk(new DeadlineStream(input, deadline), output, release, bytes -> {
                    int percent = (int) (bytes * 100 / release.size);
                    if (percent != previousPercent[0]) {
                        previousPercent[0] = percent;
                        publish(token, new State(Phase.DOWNLOADING,
                                "Downloading " + megabytes(bytes) + " / " + megabytes(release.size) + " MB…",
                                release, percent, false));
                    }
                });
            }
            publish(token, new State(Phase.VERIFYING, "Checking the APK package and signing certificate…", release, 100, false));
            verifyIdentity(partial, release);
            UpdateChecks.cancelled();
            move(partial, apkFile(release));
            saveRelease(release);
            cleanupCache(release);
            publish(token, new State(Phase.READY, "Update downloaded and verified. Tap Install update to continue.", release, 100, false));
        } finally {
            if (request != null) close(request);
            if (partial.exists()) partial.delete();
        }
    }

    private void verify(File file, UpdateManifest release) throws Exception {
        UpdateManifest.require(file.isFile(), "Cached update was removed; retry the download");
        try (InputStream input = new FileInputStream(file)) {
            UpdateChecks.apk(input, new OutputStream() { @Override public void write(int value) {}
                @Override public void write(byte[] bytes, int offset, int length) {} }, release, bytes -> {});
        }
        verifyIdentity(file, release);
    }

    private void verifyIdentity(File file, UpdateManifest release) throws Exception {
        PackageInfo current = installed();
        PackageInfo archive = getApplication().getPackageManager().getPackageArchiveInfo(file.getAbsolutePath(), signatureFlags());
        UpdateManifest.require(archive != null, "Downloaded file is not an Android package");
        UpdateChecks.identity(archive.packageName, version(archive), signers(archive),
                version(current), signers(current), release);
    }

    @SuppressWarnings("deprecation")
    private PackageInfo installed() throws PackageManager.NameNotFoundException {
        return getApplication().getPackageManager().getPackageInfo(UpdateManifest.PACKAGE, signatureFlags());
    }

    @SuppressWarnings("deprecation")
    private static int signatureFlags() {
        return Build.VERSION.SDK_INT >= 28 ? PackageManager.GET_SIGNING_CERTIFICATES : PackageManager.GET_SIGNATURES;
    }

    @SuppressWarnings("deprecation")
    private static long version(PackageInfo info) {
        return Build.VERSION.SDK_INT >= 28 ? info.getLongVersionCode() : info.versionCode;
    }

    @SuppressWarnings("deprecation")
    private static Set<String> signers(PackageInfo info) {
        Signature[] signatures = Build.VERSION.SDK_INT >= 28
                ? (info.signingInfo == null ? null : info.signingInfo.getApkContentsSigners()) : info.signatures;
        Set<String> result = new HashSet<>();
        if (signatures != null) for (Signature signature : signatures)
            result.add(UpdateChecks.hex(UpdateChecks.sha256().digest(signature.toByteArray())));
        return result;
    }

    private UpdateManifest cachedRelease() {
        try (InputStream input = new FileInputStream(new File(directory, "ready.json"))) {
            return UpdateManifest.parse(UpdateChecks.manifest(input));
        } catch (IOException exception) { return null; }
    }

    private void saveRelease(UpdateManifest release) throws IOException {
        File temporary = new File(directory, "ready.json.part");
        try (FileOutputStream output = new FileOutputStream(temporary)) {
            output.write(release.json.getBytes(StandardCharsets.UTF_8));
            output.getFD().sync();
        }
        move(temporary, new File(directory, "ready.json"));
    }

    private File apkFile(UpdateManifest release) { return new File(directory, release.commit + ".apk"); }

    private void cleanupCache(UpdateManifest keep) {
        File[] files = directory.listFiles();
        if (files == null) return;
        String keepName = keep == null ? "" : apkFile(keep).getName();
        for (File file : files) {
            String name = file.getName();
            if (name.endsWith(".part") || (name.matches("[0-9a-f]{40}\\.apk") && !name.equals(keepName))) file.delete();
        }
    }

    private static void move(File from, File to) throws IOException {
        try { Files.move(from.toPath(), to.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING); }
        catch (java.nio.file.AtomicMoveNotSupportedException exception) {
            Files.move(from.toPath(), to.toPath(), StandardCopyOption.REPLACE_EXISTING);
        }
    }

    private HttpURLConnection open(URL url, long deadline) throws IOException {
        for (int redirects = 0; redirects <= 5; redirects++) {
            UpdateChecks.cancelled();
            UpdateManifest.require(System.nanoTime() < deadline, "Update request timed out");
            UpdateChecks.trustedUrl(url);
            HttpURLConnection request = (HttpURLConnection) url.openConnection();
            connection = request;
            request.setInstanceFollowRedirects(false);
            request.setConnectTimeout(15_000);
            request.setReadTimeout(20_000);
            request.setRequestProperty("Accept-Encoding", "identity");
            request.setRequestProperty("User-Agent", "Rubblekin-Android-Updater/1");
            int status;
            try { status = request.getResponseCode(); }
            catch (IOException exception) { close(request); throw exception; }
            if (status == HttpURLConnection.HTTP_OK) return request;
            String location = request.getHeaderField("Location");
            close(request);
            if ((status == 301 || status == 302 || status == 303 || status == 307 || status == 308) && location != null) {
                url = new URL(url, location);
            } else throw new IOException("Release server returned HTTP " + status);
        }
        throw new IOException("Too many update redirects");
    }

    private void close(HttpURLConnection request) {
        request.disconnect();
        if (connection == request) connection = null;
    }

    private void cancelWork() {
        generation++;
        if (pending != null) pending.cancel(true);
        HttpURLConnection active = connection;
        if (active != null) active.disconnect();
    }

    private void publish(int token, State next) {
        main.post(() -> { if (generation == token) set(next); });
    }

    private void set(State next) { state = next; states.setValue(next); }

    private static String megabytes(long bytes) {
        return String.format(java.util.Locale.ROOT, "%.1f", bytes / (1024.0 * 1024.0));
    }

    private static final class DeadlineStream extends java.io.FilterInputStream {
        private final long deadline;
        DeadlineStream(InputStream input, long deadline) { super(input); this.deadline = deadline; }
        @Override public int read(byte[] bytes, int offset, int length) throws IOException {
            UpdateChecks.cancelled();
            UpdateManifest.require(System.nanoTime() < deadline, "Update download timed out");
            return super.read(bytes, offset, length);
        }
    }

    @Override protected void onCleared() {
        cancelWork();
        worker.shutdownNow();
    }
}
