package net.rubblekin.client;

import android.app.Application;
import android.util.Log;
import io.sentry.Sentry;
import io.sentry.SentryEvent;
import io.sentry.SentryLevel;
import io.sentry.protocol.Message;
import io.sentry.android.core.SentryAndroid;

/** Initialize before either the updater or GameActivity loads native code. */
public final class RubblekinApplication extends Application {
    @Override
    public void onCreate() {
        super.onCreate();
        if (BuildConfig.SENTRY_DSN.isEmpty()) {
            Log.w("RubblekinCrash", "Sentry is disabled: no project DSN was configured");
            return;
        }
        SentryAndroid.init(this, options -> {
            options.setDsn(BuildConfig.SENTRY_DSN);
            options.setRelease("rubblekin@" + BuildConfig.BUILD_COMMIT);
            options.setEnvironment(BuildConfig.SENTRY_ENVIRONMENT);
            options.setDebug("verification".equals(BuildConfig.SENTRY_ENVIRONMENT));
            options.setEnableNdk(true);
            options.setEnableScopeSync(true);
            options.setSendDefaultPii(false);
            options.setTracesSampleRate(0.0);
            options.setEnableAutoActivityLifecycleTracing(false);
            options.setEnableFramesTracking(false);
            options.setEnableAutoSessionTracking(false);
            options.setEnableUserInteractionBreadcrumbs(false);
            options.setEnableSystemEventBreadcrumbs(false);
            options.setAttachScreenshot(false);
            options.setAttachViewHierarchy(false);
            options.setBeforeSend((event, hint) -> {
                event.setUser(null);
                event.setServerName(null);
                return event;
            });
        });
        Sentry.configureScope(scope -> {
            scope.setTag("build.commit", BuildConfig.BUILD_COMMIT);
            scope.setTag("client.platform", "android");
            scope.setTag("client.phase", "updater");
        });
    }

    static void reportRust(String text, String stack, SentryLevel level) {
        if (!Sentry.isEnabled()) {
            Log.w("RubblekinCrash", "Cannot report Rust failure: Sentry is disabled");
            return;
        }
        SentryEvent event = new SentryEvent();
        Message message = new Message();
        message.setFormatted(text);
        event.setMessage(message);
        event.setLevel(level);
        event.setExtra("rust.backtrace", stack);
        event.setTag("error.source", "rust");
        Log.i("RubblekinCrash", "Recorded Rust failure: " + Sentry.captureEvent(event));
        // A fatal panic may abort without running destructors. Give the Android
        // SDK a bounded chance to cache/send the event before native termination.
        Sentry.flush(2000);
    }
}
