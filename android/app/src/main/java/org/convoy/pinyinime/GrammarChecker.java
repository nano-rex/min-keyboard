package org.convoy.pinyinime;

import android.content.Context;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.Looper;
import android.os.Process;

import java.io.File;

/**
 * Runs the on-device grammar model. The model is loaded on demand on a background-priority
 * thread and unloaded after a short idle period or when Android reports memory pressure, so it
 * never holds RAM while the user is doing something else.
 */
final class GrammarChecker {
    private static final long IDLE_UNLOAD_MS = 60_000L;
    private static final String MODEL_FILE = "models/grammar.gguf";

    interface Listener {
        void onStatus(String message);
        void onPartial(String text);
        void onResult(String corrected);
        void onError(String message);
    }

    /** Called from native code with the answer generated so far. */
    interface PartialCallback {
        void onPartial(String text);
    }

    private static Boolean libraryAvailable;
    private static GrammarChecker instance;

    private final Context context;
    private final Handler main = new Handler(Looper.getMainLooper());
    private HandlerThread thread;
    private Handler worker;
    private boolean loaded;
    private final Runnable idleUnload = this::unloadNow;

    private GrammarChecker(Context context) {
        this.context = context.getApplicationContext();
    }

    /** One per process: the keyboard and the settings screen share the loaded model. */
    static synchronized GrammarChecker get(Context context) {
        if (instance == null) instance = new GrammarChecker(context);
        return instance;
    }

    static File modelFile(Context context) {
        return new File(context.getFilesDir(), MODEL_FILE);
    }

    static boolean hasModel(Context context) {
        return modelFile(context).isFile();
    }

    static synchronized boolean isSupported() {
        if (libraryAvailable == null) {
            try {
                System.loadLibrary("minkb_grammar");
                libraryAvailable = true;
            } catch (UnsatisfiedLinkError error) {
                libraryAvailable = false;  // e.g. a 32-bit device; the library is built for 64-bit only
            }
        }
        return libraryAvailable;
    }

    /** Proofreads {@code text}. Callbacks arrive on the main thread. */
    void check(String text, String language, Listener listener) {
        if (!isSupported()) {
            listener.onError("The AI grammar check is not available on this device's CPU.");
            return;
        }
        File model = modelFile(context);
        if (!model.isFile()) {
            listener.onError("No AI model installed. Open Min Keyboard settings → AI grammar check.");
            return;
        }
        ensureWorker();
        worker.removeCallbacks(idleUnload);
        worker.post(() -> {
            if (!loaded) {
                ModelAdvisor.DeviceSpecs specs = DeviceProfiler.measure(context);
                String problem = ModelAdvisor.loadProblem(specs, model.length());
                if (problem != null) {
                    post(() -> listener.onError(problem));
                    return;
                }
                post(() -> listener.onStatus("Loading AI model…"));
                String error = nativeLoad(model.getPath(), ModelAdvisor.threads(specs), ModelAdvisor.CONTEXT_TOKENS);
                if (!error.isEmpty()) {
                    post(() -> listener.onError(error));
                    return;
                }
                loaded = true;
            }
            post(() -> listener.onStatus("Checking " + language + "…"));
            String result = nativeCheck(text, language, partial -> post(() -> listener.onPartial(partial)));
            if (result.startsWith("error: ")) {
                String message = result.substring(7);
                if (!"cancelled".equals(message)) post(() -> listener.onError(capitalize(message) + "."));
            } else {
                post(() -> listener.onResult(result));
            }
            worker.postDelayed(idleUnload, IDLE_UNLOAD_MS);
        });
    }

    void cancel() {
        if (isSupported()) nativeCancel();
    }

    /** Frees the model's memory now (e.g. on onTrimMemory). */
    void release() {
        if (worker == null) return;
        cancel();
        worker.removeCallbacks(idleUnload);
        worker.post(this::unloadNow);
    }

    private void unloadNow() {
        if (loaded) {
            nativeUnload();
            loaded = false;
        }
    }

    private void ensureWorker() {
        if (worker != null) return;
        // Background priority: the native threads inherit it, so foreground apps are not starved.
        thread = new HandlerThread("min-keyboard-grammar", Process.THREAD_PRIORITY_BACKGROUND);
        thread.start();
        worker = new Handler(thread.getLooper());
    }

    private void post(Runnable runnable) {
        main.post(runnable);
    }

    private static String capitalize(String text) {
        return text.isEmpty() ? text : Character.toUpperCase(text.charAt(0)) + text.substring(1);
    }

    private static native String nativeLoad(String path, int threads, int context);

    private static native void nativeUnload();

    private static native long nativeModelBytes();

    private static native void nativeCancel();

    private static native String nativeCheck(String text, String language, PartialCallback callback);
}
