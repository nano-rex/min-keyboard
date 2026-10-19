package org.convoy.pinyinime;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.widget.Button;

/** A key that can show a small long-press hint (for example a digit) in its top-right corner. */
final class KeyView extends Button {
    private final Paint hintPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private String hint;

    KeyView(Context context) {
        super(context);
        hintPaint.setTextAlign(Paint.Align.RIGHT);
        hintPaint.setTextSize(10 * getResources().getDisplayMetrics().scaledDensity);
    }

    void setHint(String hint, int color) {
        this.hint = hint;
        hintPaint.setColor(color);
        invalidate();
    }

    String hint() {
        return hint;
    }

    @Override
    protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        if (hint != null) {
            float density = getResources().getDisplayMetrics().density;
            // TextView scrolls its canvas to centre single-line text, so draw relative to the scroll.
            canvas.drawText(hint, getScrollX() + getWidth() - 6 * density, getScrollY() + hintPaint.getTextSize() + 4 * density, hintPaint);
        }
    }
}
