package org.convoy.pinyinime;

import android.graphics.Color;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.LayerDrawable;

/** Colours and key shapes for the light and dark keyboard. */
final class KeyboardTheme {
    final boolean dark;
    final int background;
    final int key;
    final int keyPressed;
    final int functionKey;
    final int functionKeyPressed;
    final int keyShadow;
    final int text;
    final int textMuted;
    final int accent;
    final int accentText;
    final int candidateHighlight;
    final int added;
    final int removed;
    final int popup;

    private KeyboardTheme(boolean dark) {
        this.dark = dark;
        if (dark) {
            background = Color.parseColor("#1B1C1F");
            key = Color.parseColor("#3B3D42");
            keyPressed = Color.parseColor("#55585E");
            functionKey = Color.parseColor("#2A2C30");
            functionKeyPressed = Color.parseColor("#44474C");
            keyShadow = Color.parseColor("#0E0F11");
            text = Color.parseColor("#E8EAED");
            textMuted = Color.parseColor("#9AA0A6");
            accent = Color.parseColor("#8AB4F8");
            accentText = Color.parseColor("#0B1D3A");
            candidateHighlight = Color.parseColor("#2F3A4D");
            added = Color.parseColor("#81C995");
            removed = Color.parseColor("#F28B82");
            popup = Color.parseColor("#4A4D53");
        } else {
            background = Color.parseColor("#E6E8EC");
            key = Color.WHITE;
            keyPressed = Color.parseColor("#D2D6DC");
            functionKey = Color.parseColor("#CDD2D9");
            functionKeyPressed = Color.parseColor("#B4BAC3");
            keyShadow = Color.parseColor("#A9AFB8");
            text = Color.parseColor("#1F1F1F");
            textMuted = Color.parseColor("#5F6368");
            accent = Color.parseColor("#1A73E8");
            accentText = Color.WHITE;
            candidateHighlight = Color.parseColor("#D3E3FD");
            added = Color.parseColor("#137333");
            removed = Color.parseColor("#C5221F");
            popup = Color.WHITE;
        }
    }

    static KeyboardTheme of(boolean dark) {
        return new KeyboardTheme(dark);
    }

    /** A rounded key with a thin darker edge at the bottom, like a physical key cap. */
    Drawable keyBackground(int fill, float density) {
        float radius = 7 * density;
        GradientDrawable shadow = new GradientDrawable();
        shadow.setColor(keyShadow);
        shadow.setCornerRadius(radius);
        GradientDrawable cap = new GradientDrawable();
        cap.setColor(fill);
        cap.setCornerRadius(radius);
        LayerDrawable layers = new LayerDrawable(new Drawable[]{shadow, cap});
        layers.setLayerInset(1, 0, 0, 0, Math.round(1.5f * density));
        return layers;
    }

    /** A flat pill, used for candidates and bar buttons. */
    Drawable pill(int fill, float density) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(fill);
        drawable.setCornerRadius(18 * density);
        return drawable;
    }
}
