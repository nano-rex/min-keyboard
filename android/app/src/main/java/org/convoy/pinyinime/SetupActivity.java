package org.convoy.pinyinime;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.SystemClock;
import android.provider.OpenableColumns;
import android.provider.Settings;
import android.view.View;
import android.view.inputmethod.InputMethodManager;
import android.widget.Button;
import android.widget.ProgressBar;
import android.widget.RadioGroup;
import android.widget.Switch;
import android.widget.TextView;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.security.MessageDigest;
import java.util.Locale;

public class SetupActivity extends Activity {
    private static final int REQUEST_MODEL = 41;
    private static final byte[] GGUF_MAGIC = {'G', 'G', 'U', 'F'};

    private TextView deviceReport;
    private TextView deviceVerdict;
    private TextView aiStatus;
    private ProgressBar aiProgress;
    private Button aiDownload;
    private Button aiImport;
    private Button aiTest;
    private Button aiRemove;
    private ModelAdvisor.Verdict verdict;
    private boolean busy;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        setTheme(ImePreferences.isDarkMode(this)
                ? android.R.style.Theme_DeviceDefault_NoActionBar
                : android.R.style.Theme_DeviceDefault_Light_NoActionBar);
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_setup);

        findViewById(R.id.open_input_settings).setOnClickListener(v -> startActivity(new Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)));
        findViewById(R.id.open_input_picker).setOnClickListener(this::showPicker);

        RadioGroup themeGroup = findViewById(R.id.theme_group);
        String theme = ImePreferences.theme(this);
        themeGroup.check(ImePreferences.THEME_DARK.equals(theme) ? R.id.theme_dark
                : ImePreferences.THEME_LIGHT.equals(theme) ? R.id.theme_light : R.id.theme_system);
        themeGroup.setOnCheckedChangeListener((group, id) -> {
            ImePreferences.setTheme(this, id == R.id.theme_dark ? ImePreferences.THEME_DARK
                    : id == R.id.theme_light ? ImePreferences.THEME_LIGHT : ImePreferences.THEME_SYSTEM);
            recreate();
        });

        bindSwitch(R.id.vibration_toggle, ImePreferences.isVibrationEnabled(this), on -> ImePreferences.setVibrationEnabled(this, on));
        bindSwitch(R.id.popup_toggle, ImePreferences.isKeyPopupEnabled(this), on -> ImePreferences.setKeyPopupEnabled(this, on));
        bindSwitch(R.id.number_hints_toggle, ImePreferences.isNumberHintsEnabled(this), on -> ImePreferences.setNumberHintsEnabled(this, on));
        bindSwitch(R.id.auto_correct_toggle, ImePreferences.isAutoCorrectEnabled(this), on -> ImePreferences.setAutoCorrectEnabled(this, on));
        bindSwitch(R.id.auto_space_toggle, ImePreferences.isAutoSpaceEnabled(this), on -> ImePreferences.setAutoSpaceEnabled(this, on));
        bindSwitch(R.id.grammar_button_toggle, ImePreferences.isGrammarButtonEnabled(this), on -> ImePreferences.setGrammarButtonEnabled(this, on));
        bindLanguage(R.id.language_english_toggle, "english");
        bindLanguage(R.id.language_malay_toggle, "malay");
        bindLanguage(R.id.language_simplified_toggle, "simplified");
        bindLanguage(R.id.language_traditional_toggle, "traditional");
        bindLanguage(R.id.language_japanese_toggle, "japanese");
        bindLanguage(R.id.language_korean_toggle, "korean");

        deviceReport = findViewById(R.id.device_report);
        deviceVerdict = findViewById(R.id.device_verdict);
        aiStatus = findViewById(R.id.ai_status);
        aiProgress = findViewById(R.id.ai_progress);
        aiDownload = findViewById(R.id.ai_download);
        aiImport = findViewById(R.id.ai_import);
        aiTest = findViewById(R.id.ai_test);
        aiRemove = findViewById(R.id.ai_remove);
        findViewById(R.id.ai_device_check).setOnClickListener(v -> analyzeDevice());
        aiDownload.setOnClickListener(v -> openDownload());
        aiImport.setOnClickListener(v -> pickModelFile());
        aiTest.setOnClickListener(v -> testSpeed());
        aiRemove.setOnClickListener(v -> confirmRemove());
        analyzeDevice();
    }

    private interface Toggle {
        void set(boolean on);
    }

    private void bindSwitch(int id, boolean value, Toggle toggle) {
        Switch control = findViewById(id);
        control.setChecked(value);
        control.setOnCheckedChangeListener((button, checked) -> toggle.set(checked));
    }

    private void bindLanguage(int id, String language) {
        bindSwitch(id, ImePreferences.isLanguageEnabled(this, language), on -> ImePreferences.setLanguageEnabled(this, language, on));
    }

    private void showPicker(View ignored) {
        InputMethodManager manager = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
        if (manager != null) manager.showInputMethodPicker();
    }

    // ------------------------------------------------------------ device check ----

    /** Measures the device and shows which model fits, or that none does. */
    private void analyzeDevice() {
        ModelAdvisor.DeviceSpecs specs = DeviceProfiler.measure(this);
        verdict = ModelAdvisor.advise(specs);
        deviceReport.setText(DeviceProfiler.describe(specs));
        StringBuilder text = new StringBuilder();
        if (!GrammarChecker.isSupported()) {
            text.append("✗ The AI engine is not available for this CPU (").append(specs.abi).append(").");
            verdict = null;
        } else if (verdict.recommended == null) {
            text.append("✗ No AI model is suitable for this device.");
        } else {
            text.append("✓ Suitable model: ").append(verdict.recommended.name)
                    .append(" (").append(ModelAdvisor.size(verdict.recommended.fileBytes)).append(" download)\n")
                    .append(verdict.recommended.note);
        }
        if (verdict != null) {
            for (String reason : verdict.reasons) text.append("\n• ").append(reason);
            if (verdict.recommended != null) {
                text.append("\n• The AI will use ").append(verdict.threads).append(" of ").append(specs.cores)
                        .append(" CPU cores at background priority and unloads after a minute of inactivity.");
            }
        }
        deviceVerdict.setText(text);
        refreshModelStatus();
    }

    private void refreshModelStatus() {
        File model = GrammarChecker.modelFile(this);
        boolean installed = model.isFile();
        boolean suitable = verdict != null && verdict.recommended != null;
        aiDownload.setEnabled(suitable && !busy);
        aiImport.setEnabled(verdict != null && verdict.deviceSupported && !busy);
        aiTest.setEnabled(installed && !busy);
        aiRemove.setEnabled(installed && !busy);
        if (suitable) aiDownload.setText("Download " + verdict.recommended.name);
        if (busy) return;
        if (installed) {
            String name = ImePreferences.modelName(this);
            aiStatus.setText("Installed: " + (name.isEmpty() ? model.getName() : name) + " (" + ModelAdvisor.size(model.length())
                    + "). Tap ✓ above the keys to check what you are writing.");
        } else if (suitable) {
            aiStatus.setText("No model installed. Download " + verdict.recommended.fileName
                    + " in your browser, then tap \"" + getString(R.string.ai_import) + "\" and pick the file.");
        } else {
            aiStatus.setText("");
        }
    }

    private void openDownload() {
        if (verdict == null || verdict.recommended == null) return;
        try {
            startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(verdict.recommended.url)));
        } catch (Exception error) {
            aiStatus.setText("No browser found. Download this file on another device:\n" + verdict.recommended.url);
        }
    }

    // ------------------------------------------------------------- import ----

    private void pickModelFile() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*");
        startActivityForResult(intent, REQUEST_MODEL);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != REQUEST_MODEL || resultCode != RESULT_OK || data == null || data.getData() == null) return;
        Uri uri = data.getData();
        String name = "model.gguf";
        long size = -1;
        try (Cursor cursor = getContentResolver().query(uri, null, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                int nameColumn = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                int sizeColumn = cursor.getColumnIndex(OpenableColumns.SIZE);
                if (nameColumn >= 0) name = cursor.getString(nameColumn);
                if (sizeColumn >= 0 && !cursor.isNull(sizeColumn)) size = cursor.getLong(sizeColumn);
            }
        }
        // Refuse before copying anything if the model is too big for this device.
        String problem = size > 0 ? ModelAdvisor.importProblem(DeviceProfiler.measure(this), size) : null;
        if (problem != null) {
            new AlertDialog.Builder(this).setTitle("This model is not suitable").setMessage(problem)
                    .setPositiveButton(android.R.string.ok, null).show();
            return;
        }
        importModel(uri, name, size);
    }

    private void importModel(Uri uri, String displayName, long expectedSize) {
        busy = true;
        aiProgress.setVisibility(View.VISIBLE);
        aiProgress.setProgress(0);
        aiStatus.setText("Installing " + displayName + "…");
        refreshModelStatus();
        new Thread(() -> {
            File target = GrammarChecker.modelFile(this);
            File temporary = new File(target.getPath() + ".part");
            String error = null;
            String sha = null;
            try {
                GrammarChecker.get(this).release();
                File parent = target.getParentFile();
                if (parent != null && !parent.isDirectory() && !parent.mkdirs()) throw new IllegalStateException("cannot create model folder");
                MessageDigest digest = MessageDigest.getInstance("SHA-256");
                try (InputStream in = getContentResolver().openInputStream(uri); OutputStream out = new FileOutputStream(temporary)) {
                    if (in == null) throw new IllegalStateException("cannot open the file");
                    byte[] buffer = new byte[1 << 16];
                    long copied = 0;
                    int count;
                    boolean first = true;
                    while ((count = in.read(buffer)) > 0) {
                        if (first) {
                            for (int i = 0; i < GGUF_MAGIC.length; i++) {
                                if (count < 4 || buffer[i] != GGUF_MAGIC[i]) throw new IllegalArgumentException("This is not a GGUF model file.");
                            }
                            first = false;
                        }
                        out.write(buffer, 0, count);
                        digest.update(buffer, 0, count);
                        copied += count;
                        if (expectedSize > 0) {
                            int percent = (int) (copied * 100 / expectedSize);
                            runOnUiThread(() -> aiProgress.setProgress(percent));
                        }
                    }
                    if (copied == 0) throw new IllegalArgumentException("The file is empty.");
                }
                // Check the size again in case the provider did not report it.
                String problem = ModelAdvisor.importProblem(DeviceProfiler.measure(this), temporary.length());
                if (problem != null && expectedSize <= 0) throw new IllegalArgumentException(problem);
                if (target.exists() && !target.delete()) throw new IllegalStateException("cannot replace the old model");
                if (!temporary.renameTo(target)) throw new IllegalStateException("cannot save the model");
                StringBuilder hex = new StringBuilder();
                for (byte b : digest.digest()) hex.append(String.format(Locale.ROOT, "%02x", b));
                sha = hex.toString();
            } catch (Exception e) {
                error = e.getMessage() == null ? e.getClass().getSimpleName() : e.getMessage();
                //noinspection ResultOfMethodCallIgnored
                temporary.delete();
            }
            String finalError = error;
            String finalSha = sha;
            runOnUiThread(() -> {
                busy = false;
                aiProgress.setVisibility(View.GONE);
                if (finalError != null) {
                    refreshModelStatus();
                    aiStatus.setText("Install failed: " + finalError);
                    return;
                }
                ModelAdvisor.Model known = ModelAdvisor.bySha256(finalSha);
                ImePreferences.setModelName(this, known != null ? known.name : displayName + " (not a recommended model)");
                refreshModelStatus();
            });
        }, "min-keyboard-model-import").start();
    }

    private void confirmRemove() {
        new AlertDialog.Builder(this).setTitle("Remove the AI model?")
                .setMessage("This frees " + ModelAdvisor.size(GrammarChecker.modelFile(this).length()) + " of storage.")
                .setNegativeButton(android.R.string.cancel, null)
                .setPositiveButton("Remove", (dialog, which) -> {
                    GrammarChecker.get(this).release();
                    //noinspection ResultOfMethodCallIgnored
                    GrammarChecker.modelFile(this).delete();
                    ImePreferences.setModelName(this, "");
                    refreshModelStatus();
                })
                .show();
    }

    // --------------------------------------------------------------- speed ----

    /** Runs one real check so the user sees how fast the model is on this device. */
    private void testSpeed() {
        busy = true;
        refreshModelStatus();
        long start = SystemClock.elapsedRealtime();
        String sample = "She go to school yesterday and buyed two book.";
        aiStatus.setText("Testing with: \"" + sample + "\"");
        GrammarChecker.get(this).check(sample, GrammarLanguage.ENGLISH, new GrammarChecker.Listener() {
            @Override
            public void onStatus(String message) {
                aiStatus.setText(message);
            }

            @Override
            public void onPartial(String text) {
                aiStatus.setText(text);
            }

            @Override
            public void onResult(String corrected) {
                double seconds = (SystemClock.elapsedRealtime() - start) / 1000.0;
                finishTest(String.format(Locale.ROOT, "Took %.1f s (including loading the model).\n→ %s%s", seconds, corrected,
                        seconds > 30 ? "\nThis is slow on this device; a smaller model would respond faster." : ""));
            }

            @Override
            public void onError(String message) {
                finishTest(message);
            }
        });
    }

    private void finishTest(String message) {
        busy = false;
        refreshModelStatus();
        aiStatus.setText(message);
    }
}
