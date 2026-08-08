package org.convoy.pinyinime;

import android.content.Context;
import android.content.SharedPreferences;

public final class ImePreferences {
    private static final String PREFS_NAME = "convoy_pinyin_prefs";
    private static final String KEY_DARK_MODE = "dark_mode";
    private static final String KEY_AUTO_CORRECT = "auto_correct";
    private static final String KEY_AUTO_SPACE = "auto_space";
    private static final String KEY_LANGUAGE_PREFIX = "language_";

    private ImePreferences() {
    }

    public static boolean isDarkMode(Context context) {
        return prefs(context).getBoolean(KEY_DARK_MODE, false);
    }

    public static void setDarkMode(Context context, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_DARK_MODE, enabled).commit();
    }

    public static boolean isAutoCorrectEnabled(Context context) {
        return prefs(context).getBoolean(KEY_AUTO_CORRECT, true);
    }

    public static void setAutoCorrectEnabled(Context context, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_AUTO_CORRECT, enabled).commit();
    }

    public static boolean isAutoSpaceEnabled(Context context) {
        return prefs(context).getBoolean(KEY_AUTO_SPACE, false);
    }

    public static void setAutoSpaceEnabled(Context context, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_AUTO_SPACE, enabled).commit();
    }

    public static boolean isLanguageEnabled(Context context, String language) {
        return prefs(context).getBoolean(KEY_LANGUAGE_PREFIX + language, true);
    }

    public static void setLanguageEnabled(Context context, String language, boolean enabled) {
        prefs(context).edit().putBoolean(KEY_LANGUAGE_PREFIX + language, enabled).commit();
    }

    private static SharedPreferences prefs(Context context) {
        return context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE);
    }
}
