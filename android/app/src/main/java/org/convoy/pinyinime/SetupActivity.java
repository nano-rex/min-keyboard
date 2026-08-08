package org.convoy.pinyinime;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;
import android.provider.Settings;
import android.view.View;
import android.view.inputmethod.InputMethodManager;
import android.widget.Button;
import android.widget.Switch;

public class SetupActivity extends Activity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        setTheme(ImePreferences.isDarkMode(this)
            ? android.R.style.Theme_DeviceDefault_NoActionBar
            : android.R.style.Theme_DeviceDefault_Light_NoActionBar);
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_setup);

        Button openSettings = findViewById(R.id.open_input_settings);
        Button openPicker = findViewById(R.id.open_input_picker);
        Switch darkMode = findViewById(R.id.dark_mode_toggle);
        Switch autoCorrect = findViewById(R.id.auto_correct_toggle);
        Switch autoSpace = findViewById(R.id.auto_space_toggle);

        openSettings.setOnClickListener(v -> startActivity(new Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)));
        openPicker.setOnClickListener(this::showPicker);

        darkMode.setChecked(ImePreferences.isDarkMode(this));
        darkMode.setOnCheckedChangeListener((buttonView, isChecked) -> {
            ImePreferences.setDarkMode(this, isChecked);
            recreate();
        });

        autoCorrect.setChecked(ImePreferences.isAutoCorrectEnabled(this));
        autoCorrect.setOnCheckedChangeListener((buttonView, isChecked) ->
            ImePreferences.setAutoCorrectEnabled(this, isChecked));

        autoSpace.setChecked(ImePreferences.isAutoSpaceEnabled(this));
        autoSpace.setOnCheckedChangeListener((buttonView, isChecked) ->
            ImePreferences.setAutoSpaceEnabled(this, isChecked));
    }

    private void showPicker(View ignored) {
        Object service = getSystemService(INPUT_METHOD_SERVICE);
        if (service instanceof InputMethodManager) {
            ((InputMethodManager) service).showInputMethodPicker();
        }
    }
}
