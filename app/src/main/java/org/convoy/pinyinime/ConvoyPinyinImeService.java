package org.convoy.pinyinime;

import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.drawable.GradientDrawable;
import android.inputmethodservice.InputMethodService;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.widget.Button;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.SeekBar;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.List;

public class ConvoyPinyinImeService extends InputMethodService {
    private static final int LIGHT_BG = Color.parseColor("#F5F5F5");
    private static final int LIGHT_PANEL = Color.parseColor("#E5E5E5");
    private static final int LIGHT_TEXT = Color.parseColor("#111111");
    private static final int LIGHT_ACTIVE = Color.parseColor("#C5E4FF");
    private static final int DARK_BG = Color.parseColor("#1B1B1B");
    private static final int DARK_PANEL = Color.parseColor("#303030");
    private static final int DARK_TEXT = Color.parseColor("#F2F2F2");
    private static final int DARK_ACTIVE = Color.parseColor("#426B8F");

    private enum InputMode {
        SIMPLIFIED,
        TRADITIONAL,
        ENGLISH
    }

    private static final String KEY_SHIFT = "Shift";
    private static final String KEY_BACKSPACE = "⌫";
    private static final String KEY_SPACE = "Space";
    private static final String KEY_ENTER = "Enter";
    private static final String KEY_SYMBOLS = "Symbols";
    private static final String KEY_LETTERS = "ABC";
    private static final String KEY_MORE_SYMBOLS = "=\\<";

    private static final String[] ROW1 = {"q", "w", "e", "r", "t", "y", "u", "i", "o", "p"};
    private static final String[] ROW2 = {"a", "s", "d", "f", "g", "h", "j", "k", "l"};
    private static final String[] ROW3 = {KEY_SHIFT, "z", "x", "c", "v", "b", "n", "m", KEY_BACKSPACE};
    private static final String[] ROW4 = {KEY_SYMBOLS, ",", KEY_SPACE, ".", KEY_ENTER};

    private static final String[] EN_SYMBOL_ROW1 = {"1", "2", "3", "4", "5", "6", "7", "8", "9", "0"};
    private static final String[] EN_SYMBOL_ROW2 = {"-", "/", ":", ";", "(", ")", "$", "&", "@", "'"};
    private static final String[] EN_SYMBOL_ROW3 = {KEY_MORE_SYMBOLS, ".", ",", "?", "!", "\"", "#", "%", "*", KEY_BACKSPACE};
    private static final String[] EN_SYMBOL_ROW4 = {KEY_LETTERS, "=", KEY_SPACE, KEY_ENTER};
    private static final String[] EN_SYMBOL2_ROW1 = {"[", "]", "{", "}", "<", ">", "^", "~", "|", "\\"};
    private static final String[] EN_SYMBOL2_ROW2 = {"`", "_", "+", "=", "€", "£", "¥", "•", "§", "©"};
    private static final String[] EN_SYMBOL2_ROW3 = {KEY_SYMBOLS, "®", "™", "✓", "×", "÷", "°", "¶", "∆", KEY_BACKSPACE};
    private static final String[] EN_SYMBOL2_ROW4 = {KEY_LETTERS, ";", KEY_SPACE, KEY_ENTER};

    private static final String[] CN_SYMBOL_ROW1 = {"1", "2", "3", "4", "5", "6", "7", "8", "9", "0"};
    private static final String[] CN_SYMBOL_ROW2 = {"（", "）", "《", "》", "“", "”", "‘", "’", "￥", "'"};
    private static final String[] CN_SYMBOL_ROW3 = {KEY_MORE_SYMBOLS, "。", "，", "？", "！", "：", "；", "、", "…", KEY_BACKSPACE};
    private static final String[] CN_SYMBOL_ROW4 = {KEY_LETTERS, "·", KEY_SPACE, KEY_ENTER};
    private static final String[] CN_SYMBOL2_ROW1 = {"【", "】", "「", "」", "『", "』", "〈", "〉", "〔", "〕"};
    private static final String[] CN_SYMBOL2_ROW2 = {"※", "￥", "€", "°", "㎡", "→", "←", "↑", "↓", "±"};
    private static final String[] CN_SYMBOL2_ROW3 = {KEY_SYMBOLS, "＋", "＝", "／", "＼", "＿", "﹣", "％", "＃", KEY_BACKSPACE};
    private static final String[] CN_SYMBOL2_ROW4 = {KEY_LETTERS, "｜", KEY_SPACE, KEY_ENTER};
    private static final long REPEAT_INITIAL_DELAY_MS = 350L;
    private static final long REPEAT_INTERVAL_MS = 60L;

    private PinyinEngine pinyinEngine;
    private EnglishEngine englishEngine;
    private final Handler repeatHandler = new Handler(Looper.getMainLooper());
    private final StringBuilder composing = new StringBuilder();
    private final List<String> currentCandidates = new ArrayList<>();
    private InputMode inputMode = InputMode.ENGLISH;
    private boolean shiftOn = false;
    private boolean symbolsMode = false;
    private boolean symbolsPageTwo = false;
    private boolean suggestionsExpanded = false;

    private TextView composingText;
    private LinearLayout candidateContainer;
    private HorizontalScrollView candidateScroll;
    private Button candidatePrev;
    private Button candidateNext;
    private Button candidateExpand;
    private Button modeSwitch;
    private LinearLayout keyboardContainer;
    private LinearLayout suggestionList;
    private ScrollView suggestionScroll;
    private LinearLayout suggestionPanel;
    private SeekBar suggestionSlider;
    private LinearLayout row1;
    private LinearLayout row2;
    private LinearLayout row3;
    private LinearLayout row4;
    private View rootView;
    private Runnable repeatRunnable;
    private Button repeatingButton;
    private String repeatingKey;
    private boolean repeatTriggered;
    private boolean syncingSuggestionSlider;

    @Override
    public View onCreateInputView() {
        View root = getLayoutInflater().inflate(R.layout.input_view, null);
        rootView = root;
        if (pinyinEngine == null) {
            pinyinEngine = new PinyinEngine(this);
        }
        if (englishEngine == null) {
            englishEngine = new EnglishEngine(this);
        }
        composingText = root.findViewById(R.id.composing_text);
        candidateContainer = root.findViewById(R.id.candidate_container);
        candidateScroll = root.findViewById(R.id.candidate_scroll);
        candidatePrev = root.findViewById(R.id.candidate_prev);
        candidateNext = root.findViewById(R.id.candidate_next);
        candidateExpand = root.findViewById(R.id.candidate_expand);
        modeSwitch = root.findViewById(R.id.mode_switch);
        keyboardContainer = root.findViewById(R.id.keyboard_container);
        suggestionList = root.findViewById(R.id.suggestion_list);
        suggestionScroll = root.findViewById(R.id.suggestion_scroll);
        suggestionPanel = root.findViewById(R.id.suggestion_panel);
        suggestionSlider = root.findViewById(R.id.suggestion_slider);
        row1 = root.findViewById(R.id.row1);
        row2 = root.findViewById(R.id.row2);
        row3 = root.findViewById(R.id.row3);
        row4 = root.findViewById(R.id.row4);

        candidatePrev.setOnClickListener(v -> scrollCandidates(-1));
        candidateNext.setOnClickListener(v -> scrollCandidates(1));
        candidateExpand.setOnClickListener(v -> toggleSuggestionPanel());
        modeSwitch.setOnClickListener(v -> cycleInputMode());
        installPressFeedback(candidatePrev);
        installPressFeedback(candidateNext);
        installPressFeedback(candidateExpand);
        installPressFeedback(modeSwitch);
        suggestionSlider.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener() {
            @Override
            public void onProgressChanged(SeekBar seekBar, int progress, boolean fromUser) {
                if (!fromUser || suggestionScroll == null || syncingSuggestionSlider) {
                    return;
                }
                suggestionScroll.scrollTo(0, progress);
            }

            @Override
            public void onStartTrackingTouch(SeekBar seekBar) {
            }

            @Override
            public void onStopTrackingTouch(SeekBar seekBar) {
            }
        });
        suggestionScroll.getViewTreeObserver().addOnScrollChangedListener(this::syncSuggestionSlider);

        rebuildKeyboard();
        applyThemeColors();
        refreshComposingUi();
        refreshCandidates();
        return root;
    }

    @Override
    public void onStartInput(EditorInfo attribute, boolean restarting) {
        super.onStartInput(attribute, restarting);
        stopRepeat();
        composing.setLength(0);
        shiftOn = false;
        refreshComposingUi();
        refreshCandidates();
        updateSuggestionPanelVisibility();
        applyThemeColors();
    }

    private void rebuildKeyboard() {
        row1.removeAllViews();
        row2.removeAllViews();
        row3.removeAllViews();
        row4.removeAllViews();
        if (symbolsMode) {
            if (inputMode == InputMode.ENGLISH) {
                if (symbolsPageTwo) {
                    addRow(row1, EN_SYMBOL2_ROW1);
                    addRow(row2, EN_SYMBOL2_ROW2);
                    addRow(row3, EN_SYMBOL2_ROW3);
                    addRow(row4, EN_SYMBOL2_ROW4);
                } else {
                    addRow(row1, EN_SYMBOL_ROW1);
                    addRow(row2, EN_SYMBOL_ROW2);
                    addRow(row3, EN_SYMBOL_ROW3);
                    addRow(row4, EN_SYMBOL_ROW4);
                }
            } else {
                if (symbolsPageTwo) {
                    addRow(row1, CN_SYMBOL2_ROW1);
                    addRow(row2, CN_SYMBOL2_ROW2);
                    addRow(row3, CN_SYMBOL2_ROW3);
                    addRow(row4, CN_SYMBOL2_ROW4);
                } else {
                    addRow(row1, CN_SYMBOL_ROW1);
                    addRow(row2, CN_SYMBOL_ROW2);
                    addRow(row3, CN_SYMBOL_ROW3);
                    addRow(row4, CN_SYMBOL_ROW4);
                }
            }
        } else {
            addRow(row1, ROW1);
            addRow(row2, ROW2);
            addRow(row3, ROW3);
            addRow(row4, ROW4);
        }
        applyThemeColors();
    }

    private void addRow(LinearLayout row, String[] keys) {
        for (String key : keys) {
            Button button = new Button(this);
            button.setText(labelFor(key));
            if (Build.VERSION.SDK_INT >= 26) {
                button.setAutoSizeTextTypeWithDefaults(TextView.AUTO_SIZE_TEXT_TYPE_UNIFORM);
            }
            button.setSingleLine(false);
            button.setMaxLines(2);
            button.setGravity(Gravity.CENTER);
            LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, widthFor(key));
            lp.setMargins(2, 2, 2, 2);
            button.setLayoutParams(lp);
            installKeyTouchHandler(button, key);
            row.addView(button);
        }
    }

    private float widthFor(String key) {
        if (KEY_SPACE.equals(key)) {
            return 3.0f;
        }
        if (KEY_SHIFT.equals(key) || KEY_BACKSPACE.equals(key) || KEY_SYMBOLS.equals(key)
                || KEY_LETTERS.equals(key) || KEY_MORE_SYMBOLS.equals(key) || KEY_ENTER.equals(key)) {
            return 1.5f;
        }
        return 1f;
    }

    private String labelFor(String key) {
        if (KEY_SHIFT.equals(key)) {
            return getString(R.string.key_shift);
        }
        if (KEY_SPACE.equals(key)) {
            if (inputMode == InputMode.ENGLISH) {
                return getString(R.string.key_space);
            }
            return inputMode == InputMode.TRADITIONAL
                    ? getString(R.string.key_space_tw)
                    : getString(R.string.key_space_cn);
        }
        if (KEY_ENTER.equals(key)) {
            if (inputMode == InputMode.ENGLISH) {
                return getString(R.string.key_enter);
            }
            return inputMode == InputMode.TRADITIONAL
                    ? getString(R.string.key_enter_tw)
                    : getString(R.string.key_enter_cn);
        }
        if (KEY_BACKSPACE.equals(key)) {
            return getString(R.string.key_backspace);
        }
        if (KEY_SYMBOLS.equals(key)) {
            return getString(R.string.key_symbols);
        }
        if (KEY_LETTERS.equals(key)) {
            return getString(R.string.key_back_to_letters);
        }
        if (KEY_MORE_SYMBOLS.equals(key)) {
            return KEY_MORE_SYMBOLS;
        }
        if (key.length() == 1 && Character.isLetter(key.charAt(0))) {
            return shiftOn ? key.toUpperCase() : key;
        }
        return key;
    }

    private void onKeyPressed(String key) {
        InputConnection ic = getCurrentInputConnection();
        if (ic == null) {
            return;
        }

        switch (key) {
            case KEY_SHIFT:
                shiftOn = !shiftOn;
                rebuildKeyboard();
                return;
            case KEY_BACKSPACE:
                handleBackspace(ic);
                return;
            case KEY_SPACE:
                handleSpace(ic);
                return;
            case KEY_ENTER:
                handleEnter(ic);
                ic.sendKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER));
                ic.sendKeyEvent(new KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_ENTER));
                return;
            case KEY_SYMBOLS:
                symbolsMode = true;
                symbolsPageTwo = false;
                rebuildKeyboard();
                return;
            case KEY_MORE_SYMBOLS:
                symbolsMode = true;
                symbolsPageTwo = !symbolsPageTwo;
                rebuildKeyboard();
                return;
            case KEY_LETTERS:
                symbolsMode = false;
                symbolsPageTwo = false;
                rebuildKeyboard();
                return;
            default:
                handleTextKey(ic, key);
        }
    }

    private void setInputMode(InputMode nextMode) {
        inputMode = nextMode;
        rebuildKeyboard();
        refreshComposingUi();
        refreshCandidates();
    }

    private void cycleInputMode() {
        switch (inputMode) {
            case ENGLISH:
                setInputMode(InputMode.SIMPLIFIED);
                break;
            case SIMPLIFIED:
                setInputMode(InputMode.TRADITIONAL);
                break;
            case TRADITIONAL:
            default:
                setInputMode(InputMode.ENGLISH);
                break;
        }
    }

    private void handleTextKey(InputConnection ic, String key) {
        String value = labelFor(key);
        char ch = value.charAt(0);
        if (symbolsMode) {
            commitComposingForCurrentMode(ic);
            ic.commitText(value, 1);
            return;
        }

        if (inputMode == InputMode.ENGLISH) {
            if (pinyinEngine.isComposingChar(ch)) {
                composing.append(Character.toLowerCase(ch));
                ic.commitText(String.valueOf(ch), 1);
                refreshComposingUi();
                refreshCandidates();
                return;
            }
            clearComposingState();
            ic.commitText(String.valueOf(ch), 1);
            return;
        }

        if (pinyinEngine.isComposingChar(ch)) {
            composing.append(Character.toLowerCase(ch));
            refreshComposingUi();
            refreshCandidates();
            return;
        }
        commitComposingForCurrentMode(ic);
        ic.commitText(String.valueOf(ch), 1);
    }

    private void handleBackspace(InputConnection ic) {
        if (inputMode == InputMode.ENGLISH && composing.length() > 0) {
            composing.deleteCharAt(composing.length() - 1);
            ic.deleteSurroundingText(1, 0);
            refreshComposingUi();
            refreshCandidates();
        } else if (composing.length() > 0) {
            composing.deleteCharAt(composing.length() - 1);
            refreshComposingUi();
            refreshCandidates();
        } else {
            ic.deleteSurroundingText(1, 0);
        }
    }

    private void handleSpace(InputConnection ic) {
        if (inputMode == InputMode.ENGLISH) {
            applyAutoCorrectIfNeeded(ic);
            clearComposingState();
        }
        ic.commitText(" ", 1);
    }

    private void handleEnter(InputConnection ic) {
        if (inputMode == InputMode.ENGLISH) {
            applyAutoCorrectIfNeeded(ic);
            clearComposingState();
            return;
        }
        commitComposingForCurrentMode(ic);
    }

    private void commitComposingForCurrentMode(InputConnection ic) {
        if (composing.length() == 0) {
            return;
        }
        commitComposingAsRaw(ic);
    }

    private List<String> getCandidates() {
        if (composing.length() == 0) {
            return java.util.Collections.emptyList();
        }
        if (inputMode == InputMode.ENGLISH) {
            return englishEngine.getCandidates(composing.toString());
        }
        PinyinEngine.ScriptMode scriptMode = inputMode == InputMode.TRADITIONAL
                ? PinyinEngine.ScriptMode.TRADITIONAL
                : PinyinEngine.ScriptMode.SIMPLIFIED;
        return pinyinEngine.getCandidates(composing.toString(), scriptMode);
    }

    private void commitComposingAsRaw(InputConnection ic) {
        if (composing.length() == 0) {
            return;
        }
        if (inputMode != InputMode.ENGLISH) {
            ic.commitText(formatForEnglish(composing.toString()), 1);
        }
        clearComposingState();
    }

    private void commitCandidate(InputConnection ic, String candidate) {
        if (inputMode == InputMode.ENGLISH && composing.length() > 0) {
            ic.deleteSurroundingText(composing.length(), 0);
        }
        ic.commitText(formatForEnglish(candidate), 1);
        clearComposingState();
        if (ImePreferences.isAutoSpaceEnabled(this)) {
            ic.commitText(" ", 1);
        }
    }

    private String formatForEnglish(String text) {
        if (inputMode != InputMode.ENGLISH || !shiftOn || text.isEmpty()) {
            return text;
        }
        return Character.toUpperCase(text.charAt(0)) + text.substring(1);
    }

    private void applyAutoCorrectIfNeeded(InputConnection ic) {
        if (inputMode != InputMode.ENGLISH || composing.length() == 0 || !ImePreferences.isAutoCorrectEnabled(this)) {
            return;
        }
        List<String> candidates = englishEngine.getCandidates(composing.toString());
        if (candidates.isEmpty()) {
            return;
        }
        String top = candidates.get(0);
        if (top.equalsIgnoreCase(composing.toString())) {
            return;
        }
        ic.deleteSurroundingText(composing.length(), 0);
        ic.commitText(formatForEnglish(top), 1);
    }

    private void clearComposingState() {
        composing.setLength(0);
        refreshComposingUi();
        refreshCandidates();
    }

    private void refreshComposingUi() {
        if (composingText == null) {
            return;
        }
        if (composing.length() == 0) {
            composingText.setText(inputMode == InputMode.ENGLISH
                    ? getString(R.string.composing_hint_en)
                    : getString(R.string.composing_hint));
            composingText.setAlpha(0.65f);
        } else {
            composingText.setText(formatForEnglish(composing.toString()));
            composingText.setAlpha(1f);
        }
    }

    private void refreshCandidates() {
        if (candidateContainer == null) {
            return;
        }
        candidateContainer.removeAllViews();
        if (suggestionList != null) {
            suggestionList.removeAllViews();
        }
        currentCandidates.clear();
        currentCandidates.addAll(getCandidates());
        LinearLayout currentSuggestionRow = null;
        for (int i = 0; i < currentCandidates.size(); i++) {
            final String candidate = currentCandidates.get(i);
            Button button = new Button(this);
            button.setText(formatForEnglish(candidate));
            button.setSingleLine(true);
            styleButton(button);
            installPressFeedback(button);
            LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT);
            lp.setMargins(4, 0, 4, 0);
            button.setLayoutParams(lp);
            button.setOnClickListener(v -> {
                InputConnection ic = getCurrentInputConnection();
                if (ic != null) {
                    commitCandidate(ic, candidate);
                }
            });
            candidateContainer.addView(button);
            if (i % 3 == 0) {
                currentSuggestionRow = createSuggestionRow();
                suggestionList.addView(currentSuggestionRow);
            }
            addExpandedCandidate(currentSuggestionRow, candidate);
        }
        boolean hasCandidates = !currentCandidates.isEmpty();
        candidatePrev.setEnabled(hasCandidates);
        candidateNext.setEnabled(hasCandidates);
        candidateExpand.setEnabled(hasCandidates);
        suggestionSlider.setEnabled(hasCandidates);
        if (candidateScroll != null) {
            candidateScroll.post(() -> candidateScroll.scrollTo(0, 0));
        }
        if (suggestionScroll != null) {
            suggestionScroll.post(() -> {
                suggestionScroll.scrollTo(0, 0);
                syncSuggestionSlider();
            });
        }
        if (!hasCandidates && suggestionsExpanded) {
            suggestionsExpanded = false;
            updateSuggestionPanelVisibility();
        }
        updateExpandButtonLabel();
    }

    private void scrollCandidates(int direction) {
        if (currentCandidates.isEmpty() || candidateScroll == null) {
            return;
        }
        int deltaPx = Math.max(120, candidateScroll.getWidth() / 2) * direction;
        candidateScroll.smoothScrollBy(deltaPx, 0);
    }

    private LinearLayout createSuggestionRow() {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT);
        lp.setMargins(0, 0, 0, 4);
        row.setLayoutParams(lp);
        return row;
    }

    private void addExpandedCandidate(LinearLayout row, String candidate) {
        if (suggestionList == null || row == null) {
            return;
        }
        Button button = new Button(this);
        button.setText(formatForEnglish(candidate));
        button.setSingleLine(false);
        button.setGravity(Gravity.CENTER);
        styleButton(button);
        installPressFeedback(button);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                0,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                1f);
        lp.setMargins(2, 2, 2, 2);
        button.setLayoutParams(lp);
        button.setOnClickListener(v -> {
            InputConnection ic = getCurrentInputConnection();
            if (ic != null) {
                commitCandidate(ic, candidate);
            }
        });
        row.addView(button);
    }

    private void toggleSuggestionPanel() {
        if (currentCandidates.isEmpty()) {
            return;
        }
        suggestionsExpanded = !suggestionsExpanded;
        updateSuggestionPanelVisibility();
        applyThemeColors();
    }

    private void updateSuggestionPanelVisibility() {
        if (keyboardContainer == null || suggestionPanel == null) {
            return;
        }
        keyboardContainer.setVisibility(suggestionsExpanded ? View.GONE : View.VISIBLE);
        suggestionPanel.setVisibility(suggestionsExpanded ? View.VISIBLE : View.GONE);
        updateExpandButtonLabel();
    }

    private void updateExpandButtonLabel() {
        if (candidateExpand == null) {
            return;
        }
        candidateExpand.setText(suggestionsExpanded
                ? getString(R.string.candidate_collapse)
                : getString(R.string.candidate_expand));
    }

    private void syncSuggestionSlider() {
        if (suggestionSlider == null || suggestionScroll == null || suggestionList == null) {
            return;
        }
        int scrollRange = Math.max(0, suggestionList.getHeight() - suggestionScroll.getHeight());
        syncingSuggestionSlider = true;
        suggestionSlider.setMax(Math.max(1, scrollRange));
        suggestionSlider.setProgress(Math.min(suggestionScroll.getScrollY(), scrollRange));
        suggestionSlider.setEnabled(scrollRange > 0 && !currentCandidates.isEmpty());
        syncingSuggestionSlider = false;
    }

    private void installKeyTouchHandler(Button button, String key) {
        button.setOnTouchListener((v, event) -> {
            switch (event.getAction()) {
                case MotionEvent.ACTION_DOWN:
                    repeatTriggered = false;
                    stylePressed(button);
                    if (isRepeatableKey(key)) {
                        startRepeat(button, key);
                    }
                    return true;
                case MotionEvent.ACTION_UP:
                    boolean triggered = repeatTriggered;
                    stopRepeat();
                    if (!triggered) {
                        onKeyPressed(key);
                    }
                    applyThemeColors();
                    return true;
                case MotionEvent.ACTION_CANCEL:
                    stopRepeat();
                    applyThemeColors();
                    return true;
                default:
                    return false;
            }
        });
    }

    private void installPressFeedback(Button button) {
        button.setOnTouchListener((v, event) -> {
            if (event.getAction() == MotionEvent.ACTION_DOWN) {
                stylePressed(button);
            } else if (event.getAction() == MotionEvent.ACTION_UP || event.getAction() == MotionEvent.ACTION_CANCEL) {
                button.post(this::applyThemeColors);
            }
            return false;
        });
    }

    private void stylePressed(Button button) {
        boolean darkMode = ImePreferences.isDarkMode(this);
        button.setBackground(keyBackground(darkMode ? DARK_ACTIVE : LIGHT_ACTIVE, darkMode ? Color.TRANSPARENT : Color.rgb(120, 170, 210)));
    }

    private boolean isRepeatableKey(String key) {
        return !KEY_SHIFT.equals(key)
            && !KEY_SYMBOLS.equals(key)
            && !KEY_MORE_SYMBOLS.equals(key)
            && !KEY_LETTERS.equals(key)
            && !KEY_ENTER.equals(key);
    }

    private void startRepeat(Button button, String key) {
        stopRepeat();
        repeatingButton = button;
        repeatingKey = key;
        repeatRunnable = new Runnable() {
            @Override
            public void run() {
                if (repeatingKey == null) {
                    return;
                }
                repeatTriggered = true;
                onKeyPressed(repeatingKey);
                if (repeatingButton != null) {
                    stylePressed(repeatingButton);
                }
                repeatHandler.postDelayed(this, REPEAT_INTERVAL_MS);
            }
        };
        repeatHandler.postDelayed(repeatRunnable, REPEAT_INITIAL_DELAY_MS);
    }

    private void stopRepeat() {
        if (repeatRunnable != null) {
            repeatHandler.removeCallbacks(repeatRunnable);
        }
        repeatRunnable = null;
        repeatingButton = null;
        repeatingKey = null;
        repeatTriggered = false;
    }

    private void applyThemeColors() {
        if (rootView == null || composingText == null || candidatePrev == null || candidateNext == null || candidateExpand == null || modeSwitch == null) {
            return;
        }
        boolean darkMode = ImePreferences.isDarkMode(this);
        int bg = darkMode ? DARK_BG : LIGHT_BG;
        int panel = darkMode ? DARK_PANEL : LIGHT_PANEL;
        int text = darkMode ? DARK_TEXT : LIGHT_TEXT;

        rootView.setBackgroundColor(bg);
        composingText.setBackgroundColor(panel);
        composingText.setTextColor(text);
        styleButton(candidatePrev);
        styleButton(candidateNext);
        styleButton(candidateExpand);
        if (suggestionScroll != null) {
            suggestionScroll.setBackgroundColor(panel);
        }
        if (suggestionSlider != null && Build.VERSION.SDK_INT >= 21) {
            ColorStateList tint = ColorStateList.valueOf(darkMode ? DARK_ACTIVE : LIGHT_ACTIVE);
            suggestionSlider.setThumbTintList(tint);
            suggestionSlider.setProgressTintList(tint);
        }
        modeSwitch.setText(labelForCurrentMode());
        styleModeButton(modeSwitch, true, darkMode);
        styleChildren(row1);
        styleChildren(row2);
        styleChildren(row3);
        styleChildren(row4);
        styleChildren(candidateContainer);
        styleChildren(suggestionList);
    }

    private void styleChildren(ViewGroup parent) {
        if (parent == null) {
            return;
        }
        for (int i = 0; i < parent.getChildCount(); i++) {
            View child = parent.getChildAt(i);
            if (child instanceof Button) {
                styleButton((Button) child);
            }
        }
    }

    private void styleButton(Button button) {
        boolean darkMode = ImePreferences.isDarkMode(this);
        button.setTextColor(darkMode ? DARK_TEXT : LIGHT_TEXT);
        button.setBackground(keyBackground(darkMode ? DARK_PANEL : Color.WHITE, darkMode ? Color.rgb(75, 75, 75) : Color.rgb(210, 210, 210)));
        button.setPadding(dp(2), dp(2), dp(2), dp(2));
    }

    private void styleModeButton(Button button, boolean active, boolean darkMode) {
        if (button == null) {
            return;
        }
        button.setTextColor(darkMode ? DARK_TEXT : LIGHT_TEXT);
        button.setBackground(keyBackground(active ? (darkMode ? DARK_ACTIVE : LIGHT_ACTIVE) : (darkMode ? DARK_PANEL : Color.WHITE), darkMode ? Color.rgb(75, 75, 75) : Color.rgb(180, 180, 180)));
    }

    private GradientDrawable keyBackground(int fill, int stroke) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(fill);
        drawable.setCornerRadius(dp(5));
        if (stroke != Color.TRANSPARENT) {
            drawable.setStroke(dp(1), stroke);
        }
        return drawable;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    private String labelForCurrentMode() {
        switch (inputMode) {
            case ENGLISH:
                return getString(R.string.mode_en);
            case TRADITIONAL:
                return getString(R.string.mode_tw);
            case SIMPLIFIED:
            default:
                return getString(R.string.mode_cn);
        }
    }
}
