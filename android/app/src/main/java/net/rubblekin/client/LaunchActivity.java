package net.rubblekin.client;

import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.Intent;
import android.graphics.Typeface;
import android.net.Uri;
import android.os.Bundle;
import android.provider.Settings;
import android.view.View;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.ProgressBar;
import android.widget.TextView;

import androidx.activity.result.ActivityResultLauncher;
import androidx.activity.result.contract.ActivityResultContracts;
import androidx.appcompat.app.AppCompatActivity;
import androidx.core.content.FileProvider;
import androidx.core.view.ViewCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.lifecycle.ViewModelProvider;

import java.io.File;

/** Native entry point: referencing the game by name postpones loading Bevy until Play. */
public final class LaunchActivity extends AppCompatActivity {
    private UpdaterModel model;
    private TextView status;
    private TextView installed;
    private Button update;
    private Button retry;
    private Button cancel;
    private ProgressBar progress;
    private boolean waitingPermission;
    private boolean waitingInstaller;
    private boolean installAfterPermission;

    private final ActivityResultLauncher<Intent> permissionScreen = registerForActivityResult(
            new ActivityResultContracts.StartActivityForResult(), result -> {
                waitingPermission = false;
                // This Settings screen has no success result. Read the actual permission on return.
                if (getPackageManager().canRequestPackageInstalls()) {
                    installAfterPermission = true;
                    requestInstallWhenReady();
                }
                else model.installPermissionDenied();
            });

    private final ActivityResultLauncher<Intent> installer = registerForActivityResult(
            new ActivityResultContracts.StartActivityForResult(), result -> {
                waitingInstaller = false;
                // Self replacement may terminate us before this callback. Next launch also reads PackageInfo.
                model.installerReturned();
                installed.setText(model.installedLabel());
            });

    @Override protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_launch);
        if (savedInstanceState != null) {
            waitingPermission = savedInstanceState.getBoolean("waiting_permission");
            waitingInstaller = savedInstanceState.getBoolean("waiting_installer");
            installAfterPermission = savedInstanceState.getBoolean("install_after_permission");
        }
        model = new ViewModelProvider(this).get(UpdaterModel.class);
        status = findViewById(R.id.update_status);
        installed = findViewById(R.id.installed_version);
        update = findViewById(R.id.update_button);
        retry = findViewById(R.id.retry_button);
        cancel = findViewById(R.id.cancel_button);
        progress = findViewById(R.id.update_progress);
        installed.setText(model.installedLabel());
        update.setOnClickListener(view -> model.update());
        retry.setOnClickListener(view -> model.retry());
        cancel.setOnClickListener(view -> model.cancelDownload());
        findViewById(R.id.play_button).setOnClickListener(view -> {
            model.cancelForPlay();
            Intent game = new Intent().setClassName(this, "net.rubblekin.client.MainActivity");
            startActivity(game);
            finish();
        });
        View root = findViewById(R.id.updater_root);
        ViewCompat.setOnApplyWindowInsetsListener(root, (view, insets) -> {
            androidx.core.graphics.Insets bars = insets.getInsets(
                    WindowInsetsCompat.Type.systemBars() | WindowInsetsCompat.Type.displayCutout());
            view.setPadding(bars.left, bars.top, bars.right, bars.bottom);
            return insets;
        });
        ViewCompat.requestApplyInsets(root);
        try { applyFont(root, Typeface.createFromAsset(getAssets(), "AtkinsonHyperlegible-Regular.ttf")); }
        catch (RuntimeException ignored) { /* Use the system font if the optional asset is unavailable. */ }
        model.states().observe(this, this::render);
        model.beginIfNeeded();
    }

    private void render(UpdaterModel.State state) {
        status.setText(state.status);
        boolean external = waitingPermission || waitingInstaller;
        update.setEnabled(!state.busy() && !external && state.release != null);
        update.setAlpha(update.isEnabled() ? 1.0f : 0.45f);
        update.setText(state.phase == UpdaterModel.Phase.READY ? "Install update" : "Update");
        retry.setEnabled(!state.busy() && !external);
        retry.setAlpha(retry.isEnabled() ? 1.0f : 0.45f);
        cancel.setVisibility(state.phase == UpdaterModel.Phase.DOWNLOADING ? View.VISIBLE : View.GONE);
        progress.setVisibility(state.busy() ? View.VISIBLE : View.INVISIBLE);
        progress.setIndeterminate(state.phase != UpdaterModel.Phase.DOWNLOADING);
        progress.setProgress(state.progress);
        if (state.installRequested && model.consumeInstallRequest(state)) install();
        else requestInstallWhenReady();
    }

    private void requestInstallWhenReady() {
        if (!installAfterPermission) return;
        UpdaterModel.State state = model.states().getValue();
        if (state != null && !state.busy() && state.release != null) {
            installAfterPermission = false;
            model.prepareInstall();
        }
    }

    @SuppressWarnings("deprecation")
    private void install() {
        if (waitingPermission || waitingInstaller) return;
        if (!getPackageManager().canRequestPackageInstalls()) {
            waitingPermission = true;
            disableExternalActions();
            try {
                permissionScreen.launch(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                        Uri.parse("package:" + getPackageName())));
            } catch (ActivityNotFoundException | SecurityException exception) {
                waitingPermission = false;
                model.externalFailure("Android could not open installation settings. Play installed or retry.");
            }
            return;
        }
        File file = model.readyApk();
        if (file == null || !file.isFile()) {
            model.externalFailure("Cached update was removed. Retry the download or play installed.");
            return;
        }
        try {
            Uri uri = FileProvider.getUriForFile(this, getPackageName() + ".updates", file);
            Intent intent = new Intent(Intent.ACTION_INSTALL_PACKAGE)
                    .setDataAndType(uri, "application/vnd.android.package-archive")
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    .putExtra(Intent.EXTRA_RETURN_RESULT, true);
            intent.setClipData(ClipData.newRawUri("Rubblekin update", uri));
            waitingInstaller = true;
            disableExternalActions();
            // Direct launch avoids Android package-visibility false negatives from resolveActivity().
            installer.launch(intent);
        } catch (ActivityNotFoundException | IllegalArgumentException | SecurityException exception) {
            waitingInstaller = false;
            model.externalFailure("Android could not open the package installer. Play installed or retry.");
        }
    }

    private void disableExternalActions() {
        update.setEnabled(false);
        retry.setEnabled(false);
        cancel.setVisibility(View.GONE);
    }

    @Override protected void onSaveInstanceState(Bundle saved) {
        saved.putBoolean("waiting_permission", waitingPermission);
        saved.putBoolean("waiting_installer", waitingInstaller);
        saved.putBoolean("install_after_permission", installAfterPermission);
        super.onSaveInstanceState(saved);
    }

    private static void applyFont(View view, Typeface typeface) {
        if (view instanceof TextView) ((TextView) view).setTypeface(typeface);
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int index = 0; index < group.getChildCount(); index++) applyFont(group.getChildAt(index), typeface);
        }
    }
}
