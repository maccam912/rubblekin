package net.rubblekin.client;

import android.os.Bundle;
import android.view.View;
import androidx.core.view.ViewCompat;
import androidx.core.view.WindowInsetsCompat;
import com.google.androidgamesdk.GameActivity;

/** GameActivity supplies the native lifecycle, multitouch, and software keyboard. */
public final class MainActivity extends GameActivity {
    private native void requestNativeExit();

    /** Called through JNI from Rust; instance methods avoid worker class-loader issues. */
    public void reportRustPanic(String message, String stack) {
        RubblekinApplication.reportRust(message, stack, io.sentry.SentryLevel.FATAL);
    }

    public void reportRustError(String message, String stack) {
        RubblekinApplication.reportRust(message, stack, io.sentry.SentryLevel.ERROR);
    }

    public void setCrashContext(String phase, String graphics) {
        io.sentry.Sentry.configureScope(scope -> {
            scope.setTag("client.phase", phase);
            scope.setTag("graphics", graphics);
        });
    }

    static {
        System.loadLibrary("rubblekin_client");
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        // Android 15+ draws edge-to-edge: adjustResize alone no longer reduces
        // the native SurfaceView. Resize its container for the keyboard so
        // Bevy sees the available height and can scroll the focused field.
        View content = findViewById(android.R.id.content);
        ViewCompat.setOnApplyWindowInsetsListener(content, (view, insets) -> {
            int keyboardHeight = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom;
            view.setPadding(0, 0, 0, keyboardHeight);
            return insets;
        });
        ViewCompat.requestApplyInsets(content);
        hideSystemUi();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            hideSystemUi();
        }
    }

    @Override
    public void finish() {
        // Bevy's AndroidApp is process-global and supports one GameActivity
        // lifetime. Request exit while it is still resumed: winit stops Bevy
        // updates on pause and ignores Destroy. Bevy drops its local server
        // (saving the world), then android_main ends this process and window.
        io.sentry.Sentry.flush(2000);
        requestNativeExit();
    }

    @Override
    protected void onDestroy() {
        // Configuration changes are handled in-place by the manifest. If the
        // OS destroys this activity directly, end the process before native
        // onDestroy waits indefinitely for winit's suspended loop.
        io.sentry.Sentry.flush(2000);
        android.os.Process.killProcess(android.os.Process.myPid());
        super.onDestroy();
    }

    @SuppressWarnings("deprecation")
    private void hideSystemUi() {
        getWindow().getDecorView().setSystemUiVisibility(
            View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
            | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
            | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
            | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
            | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
            | View.SYSTEM_UI_FLAG_FULLSCREEN);
    }
}
