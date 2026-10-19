package org.convoy.pinyinime;

import android.content.Context;
import android.content.SharedPreferences;
import android.content.res.Configuration;

public final class ImePreferences {
    public static final String THEME_SYSTEM = "system";
    public static final String THEME_LIGHT = "light";
    public static final String THEME_DARK = "dark";

    private static final String PREFS_NAME = "convoy_pinyin_prefs";
    private static final String KEY_DARK_MODE = "dark_mode";  // before the theme setting existed
    private static final String KEY_THEME = "theme";
    private static final String KEY_AUTO_CORRECT = "auto_correct";
    private static final String KEY_AUTO_SPACE = "auto_space";
    private static final String KEY_VIBRATION = "key_vibration";
    private static final String KEY_POPUP = "key_popup";
    private static final String KEY_NUMBER_HINTS = "number_row_hints";
    private static final String KEY_GRAMMAR_BUTTON = "grammar_button";
    private static final String KEY_MODEL_NAME = "grammar_model_name";
    private static final String KEY_LANGUAGE_PREFIX = "language_";

    private ImePreferences() {
    }

    /** "system", "light" or "dark". Users of the old dark-mode switch keep their choice. */
    public static String theme(Context context) {
        SharedPreferences prefs = prefs(context);
        if (prefs.contains(KEY_THEME)) return prefs.getString(KEY_THEME, THEME_SYSTEM);
        if (prefs.contains(KEY_DARK_MODE)) return prefs.getBoolean(KEY_DARK_MODE, false) ? THEME_DARK : THEME_LIGHT;
        return THEME_SYSTEM;
    }

    public static void setTheme(Context context, String theme) {
        prefs(context).edit().putString(KEY_THEME, theme).apply();
    }

    public static boolean isDarkMode(Context context) {
        String theme = theme(context);
        if (THEME_DARK.equals(theme)) return true;
        if (THEME_LIGHT.equals(theme)) return false;
        int night = context.getResources().getConfiguration().uiMode & Configuration.UI_MODE_NIGHT_MASK;
        return night == Configuration.UI_MODE_NIGHT_YES;
    }

    public static boolean isAutoCorrectEnabled(Context context) {
        return prefs(context).getBoolean(KEY_AUTO_CORRECT, true);
    }

    public static void setAutoCorrectEnabled(Context context, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_AUTO_CORRECT, enabled).apply();
    }

    public static boolean isAutoSpaceEnabled(Context context) {
        return prefs(context).getBoolean(KEY_AUTO_SPACE, false);
    }

    public static void setAutoSpaceEnabled(Context context, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_AUTO_SPACE, enabled).apply();
    }

    public static boolean isVibrationEnabled(Context context) {
        return prefs(context).getBoolean(KEY_VIBRATION, true);
    }

    public static void setVibrationEnabled(Context context, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_VIBRATION, enabled).apply();
    }

    public static boolean isKeyPopupEnabled(Context context) {
        return prefs(context).getBoolean(KEY_POPUP, true);
    }

    public static void setKeyPopupEnabled(Context context, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_POPUP, enabled).apply();
    }

    public static boolean isNumberHintsEnabled(Context context) {
        return prefs(context).getBoolean(KEY_NUMBER_HINTS, true);
    }

    public static void setNumberHintsEnabled(Context context, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_NUMBER_HINTS, enabled).apply();
    }

    public static boolean isGrammarButtonEnabled(Context context) {
        return prefs(context).getBoolean(KEY_GRAMMAR_BUTTON, true);
    }

    public static void setGrammarButtonEnabled(Context context, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_GRAMMAR_BUTTON, enabled).apply();
    }

    public static String modelName(Context context) {
        return prefs(context).getString(KEY_MODEL_NAME, "");
    }

    public static void setModelName(Context context, String name) {
        prefs(context).edit().putString(KEY_MODEL_NAME, name).apply();
    }

    public static boolean isLanguageEnabled(Context context, String language) {
        return prefs(context).getBoolean(KEY_LANGUAGE_PREFIX + language, true);
    }

    public static void setLanguageEnabled(Context context, String language, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_LANGUAGE_PREFIX + language, enabled).apply();
    }

    private static SharedPreferences prefs(Context context) {
        return context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE);
    }
}
