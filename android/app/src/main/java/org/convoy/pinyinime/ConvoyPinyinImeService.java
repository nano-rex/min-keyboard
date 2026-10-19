package org.convoy.pinyinime;

import android.content.ComponentCallbacks2;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.graphics.Typeface;
import android.inputmethodservice.InputMethodService;
import android.os.Handler;
import android.os.Looper;
import android.text.SpannableStringBuilder;
import android.text.Spanned;
import android.text.TextUtils;
import android.text.style.ForegroundColorSpan;
import android.text.style.StrikethroughSpan;
import android.text.style.StyleSpan;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.HapticFeedbackConstants;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.widget.Button;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.PopupWindow;
import android.widget.ScrollView;
import android.widget.SeekBar;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

public class ConvoyPinyinImeService extends InputMethodService {
    private enum InputMode {
        SIMPLIFIED,
        TRADITIONAL,
        JAPANESE,
        KOREAN,
        ENGLISH,
        MALAY
    }

    private static final String KEY_SHIFT = "Shift";
    private static final String KEY_BACKSPACE = "⌫";
    private static final String KEY_SPACE = "Space";
    private static final String KEY_ENTER = "Enter";
    private static final String KEY_SYMBOLS = "Symbols";
    private static final String KEY_LETTERS = "ABC";
    private static final String KEY_MORE_SYMBOLS = "=\\<";

    private static final String[] ROW1 = {"q", "w", "e", "r", "t", "y", "u", "i", "o", "p"};
    private static final String[] ROW1_DIGITS = {"1", "2", "3", "4", "5", "6", "7", "8", "9", "0"};
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
    private static final long LONG_PRESS_MS = 350L;

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final StringBuilder composing = new StringBuilder();
    private final List<String> currentCandidates = new ArrayList<>();
    private InputMode inputMode = InputMode.ENGLISH;
    private boolean shiftOn = false;
    private boolean symbolsMode = false;
    private boolean symbolsPageTwo = false;
    private boolean suggestionsExpanded = false;

    private Engines engines;
    private GrammarChecker grammarChecker;
    private KeyboardTheme theme;

    private View rootView;
    private LinearLayout candidateContainer;
    private HorizontalScrollView candidateScroll;
    private TextView composingLabel;
    private Button candidatePrev;
    private Button candidateNext;
    private Button candidateExpand;
    private Button grammarButton;
    private Button modeSwitch;
    private LinearLayout keyboardContainer;
    private LinearLayout suggestionList;
    private ScrollView suggestionScroll;
    private LinearLayout suggestionPanel;
    private SeekBar suggestionSlider;
    private LinearLayout grammarPanel;
    private TextView grammarTitle;
    private TextView grammarBody;
    private Button grammarApply;
    private Button grammarDismiss;
    private LinearLayout row1;
    private LinearLayout row2;
    private LinearLayout row3;
    private LinearLayout row4;
    private PopupWindow keyPreview;
    private TextView keyPreviewText;

    private Runnable repeatRunnable;
    private Runnable longPressRunnable;
    private boolean pressConsumed;
    private boolean syncingSuggestionSlider;

    private String grammarOriginal;
    private boolean grammarFromSelection;
    private String grammarResult;

    /** Loads each language's dictionary on first use, on a background thread where possible. */
    private final class Engines {
        private LatinEngine english;
        private LatinEngine malay;
        private PinyinEngine pinyin;
        private JapaneseEngine japanese;
        private KoreanEngine korean;
        private final AssetOpener assets = name -> getAssets().open(name);

        synchronized LatinEngine english() {
            if (english == null) english = LatinEngine.english(assets);
            return english;
        }

        synchronized LatinEngine malay() {
            if (malay == null) malay = LatinEngine.malay(assets);
            return malay;
        }

        synchronized PinyinEngine pinyin() {
            if (pinyin == null) pinyin = new PinyinEngine(assets);
            return pinyin;
        }

        synchronized JapaneseEngine japanese() {
            if (japanese == null) japanese = new JapaneseEngine(assets);
            return japanese;
        }

        synchronized KoreanEngine korean() {
            if (korean == null) korean = new KoreanEngine(assets);
            return korean;
        }

        void preload(InputMode mode) {
            new Thread(() -> {
                switch (mode) {
                    case SIMPLIFIED:
                    case TRADITIONAL: pinyin(); break;
                    case JAPANESE: japanese(); break;
                    case KOREAN: korean(); break;
                    case MALAY: malay(); break;
                    default: english();
                }
            }, "min-keyboard-dictionary").start();
        }
    }

    @Override
    public void onCreate() {
        super.onCreate();
        engines = new Engines();
        grammarChecker = GrammarChecker.get(this);
        engines.preload(InputMode.ENGLISH);
    }

    @Override
    public void onDestroy() {
        grammarChecker.release();
        super.onDestroy();
    }

    @Override
    public void onTrimMemory(int level) {
        super.onTrimMemory(level);
        // Give the AI model's memory back as soon as the system asks for it.
        if (level >= ComponentCallbacks2.TRIM_MEMORY_RUNNING_MODERATE) {
            grammarChecker.release();
        }
    }

    @Override
    public void onLowMemory() {
        super.onLowMemory();
        grammarChecker.release();
    }

    @Override
    public View onCreateInputView() {
        View root = getLayoutInflater().inflate(R.layout.input_view, null);
        rootView = root;
        candidateContainer = root.findViewById(R.id.candidate_container);
        candidateScroll = root.findViewById(R.id.candidate_scroll);
        composingLabel = root.findViewById(R.id.composing_label);
        candidatePrev = root.findViewById(R.id.candidate_prev);
        candidateNext = root.findViewById(R.id.candidate_next);
        candidateExpand = root.findViewById(R.id.candidate_expand);
        grammarButton = root.findViewById(R.id.grammar_button);
        modeSwitch = root.findViewById(R.id.mode_switch);
        keyboardContainer = root.findViewById(R.id.keyboard_container);
        suggestionList = root.findViewById(R.id.suggestion_list);
        suggestionScroll = root.findViewById(R.id.suggestion_scroll);
        suggestionPanel = root.findViewById(R.id.suggestion_panel);
        suggestionSlider = root.findViewById(R.id.suggestion_slider);
        grammarPanel = root.findViewById(R.id.grammar_panel);
        grammarTitle = root.findViewById(R.id.grammar_title);
        grammarBody = root.findViewById(R.id.grammar_body);
        grammarApply = root.findViewById(R.id.grammar_apply);
        grammarDismiss = root.findViewById(R.id.grammar_dismiss);
        row1 = root.findViewById(R.id.row1);
        row2 = root.findViewById(R.id.row2);
        row3 = root.findViewById(R.id.row3);
        row4 = root.findViewById(R.id.row4);

        candidatePrev.setOnClickListener(v -> scrollCandidates(-1));
        candidateNext.setOnClickListener(v -> scrollCandidates(1));
        candidateExpand.setOnClickListener(v -> toggleSuggestionPanel());
        modeSwitch.setOnClickListener(v -> cycleInputMode());
        grammarButton.setOnClickListener(v -> startGrammarCheck());
        grammarApply.setOnClickListener(v -> applyGrammarResult());
        grammarDismiss.setOnClickListener(v -> closeGrammarPanel());
        suggestionSlider.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener() {
            @Override
            public void onProgressChanged(SeekBar seekBar, int progress, boolean fromUser) {
                if (fromUser && !syncingSuggestionSlider) suggestionScroll.scrollTo(0, progress);
            }

            @Override
            public void onStartTrackingTouch(SeekBar seekBar) {
            }

            @Override
            public void onStopTrackingTouch(SeekBar seekBar) {
            }
        });
        suggestionScroll.getViewTreeObserver().addOnScrollChangedListener(this::syncSuggestionSlider);

        keyPreviewText = new TextView(this);
        keyPreviewText.setGravity(Gravity.CENTER);
        keyPreviewText.setTextSize(TypedValue.COMPLEX_UNIT_SP, 28);
        keyPreview = new PopupWindow(keyPreviewText);
        keyPreview.setClippingEnabled(false);
        keyPreview.setTouchable(false);
        keyPreview.setAnimationStyle(0);

        theme = KeyboardTheme.of(ImePreferences.isDarkMode(this));
        rebuildKeyboard();
        refreshCandidates();
        return root;
    }

    @Override
    public void onStartInput(EditorInfo attribute, boolean restarting) {
        super.onStartInput(attribute, restarting);
        stopRepeat();
        if (!isLanguageEnabled(inputMode)) {
            inputMode = firstEnabledMode();
        }
        composing.setLength(0);
        shiftOn = false;
        refreshCandidates();
        updateSuggestionPanelVisibility();
    }

    @Override
    public void onStartInputView(EditorInfo info, boolean restarting) {
        super.onStartInputView(info, restarting);
        // Settings may have changed while the keyboard was hidden (theme, hints, languages).
        theme = KeyboardTheme.of(ImePreferences.isDarkMode(this));
        if (rootView != null) {
            rebuildKeyboard();
            refreshCandidates();
        }
    }

    @Override
    public void onFinishInputView(boolean finishingInput) {
        super.onFinishInputView(finishingInput);
        dismissKeyPreview();
        stopRepeat();
        if (grammarPanel != null && grammarPanel.getVisibility() == View.VISIBLE) closeGrammarPanel();
    }

    @Override
    public void onUpdateSelection(int oldSelStart, int oldSelEnd, int newSelStart, int newSelEnd,
                                  int candidatesStart, int candidatesEnd) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd);
        // The user moved the cursor away from the word being typed: finish it where it is.
        if (isLatin(inputMode) && composing.length() > 0 && (newSelStart != candidatesEnd || newSelEnd != candidatesEnd)) {
            composing.setLength(0);
            InputConnection ic = getCurrentInputConnection();
            if (ic != null) ic.finishComposingText();
            refreshCandidates();
        }
    }

    // ------------------------------------------------------------------ keys ----

    private void rebuildKeyboard() {
        row1.removeAllViews();
        row2.removeAllViews();
        row3.removeAllViews();
        row4.removeAllViews();
        boolean latin = isLatin(inputMode);
        if (symbolsMode) {
            addRow(row1, latin ? (symbolsPageTwo ? EN_SYMBOL2_ROW1 : EN_SYMBOL_ROW1) : (symbolsPageTwo ? CN_SYMBOL2_ROW1 : CN_SYMBOL_ROW1));
            addRow(row2, latin ? (symbolsPageTwo ? EN_SYMBOL2_ROW2 : EN_SYMBOL_ROW2) : (symbolsPageTwo ? CN_SYMBOL2_ROW2 : CN_SYMBOL_ROW2));
            addRow(row3, latin ? (symbolsPageTwo ? EN_SYMBOL2_ROW3 : EN_SYMBOL_ROW3) : (symbolsPageTwo ? CN_SYMBOL2_ROW3 : CN_SYMBOL_ROW3));
            addRow(row4, latin ? (symbolsPageTwo ? EN_SYMBOL2_ROW4 : EN_SYMBOL_ROW4) : (symbolsPageTwo ? CN_SYMBOL2_ROW4 : CN_SYMBOL_ROW4));
            row2.setPadding(0, 0, 0, 0);
        } else {
            addRow(row1, ROW1);
            addRow(row2, ROW2);
            addRow(row3, ROW3);
            addRow(row4, ROW4);
            row2.setPadding(dp(16), 0, dp(16), 0);
        }
        applyTheme();
    }

    private void addRow(LinearLayout row, String[] keys) {
        boolean hints = !symbolsMode && row == row1 && ImePreferences.isNumberHintsEnabled(this);
        for (int i = 0; i < keys.length; i++) {
            String key = keys[i];
            KeyView button = new KeyView(this);
            button.setText(labelFor(key));
            button.setAllCaps(false);
            button.setSingleLine(true);
            button.setGravity(Gravity.CENTER);
            button.setMinHeight(dp(50));
            button.setMinimumHeight(dp(50));
            button.setMinWidth(0);
            button.setMinimumWidth(0);
            button.setStateListAnimator(null);
            button.setIncludeFontPadding(false);
            button.setContentDescription(descriptionFor(key));
            button.setTag(key);
            if (hints) button.setHint(ROW1_DIGITS[i], Color.GRAY);
            LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(0, dp(50), widthFor(key));
            lp.setMargins(dp(3), dp(4), dp(3), dp(4));
            button.setLayoutParams(lp);
            installKeyTouchHandler(button, key);
            row.addView(button);
        }
    }

    private float widthFor(String key) {
        if (KEY_SPACE.equals(key)) return 4.0f;
        if (KEY_SHIFT.equals(key) || KEY_BACKSPACE.equals(key) || KEY_SYMBOLS.equals(key)
                || KEY_LETTERS.equals(key) || KEY_MORE_SYMBOLS.equals(key) || KEY_ENTER.equals(key)) {
            return 1.5f;
        }
        return 1f;
    }

    private boolean isFunctionKey(String key) {
        return KEY_SHIFT.equals(key) || KEY_BACKSPACE.equals(key) || KEY_SYMBOLS.equals(key)
                || KEY_LETTERS.equals(key) || KEY_MORE_SYMBOLS.equals(key);
    }

    private String labelFor(String key) {
        if (",".equals(key) && !isLatin(inputMode)) return getString(R.string.key_comma_cn);
        if (".".equals(key) && !isLatin(inputMode)) return getString(R.string.key_period_cn);
        switch (key) {
            case KEY_SHIFT: return getString(R.string.key_shift);
            case KEY_SPACE: return spaceLabel();
            case KEY_ENTER: return getString(R.string.key_enter);
            case KEY_BACKSPACE: return getString(R.string.key_backspace);
            case KEY_SYMBOLS: return getString(R.string.key_symbols);
            case KEY_LETTERS: return getString(R.string.key_back_to_letters);
            case KEY_MORE_SYMBOLS: return KEY_MORE_SYMBOLS;
            default:
                if (key.length() == 1 && Character.isLetter(key.charAt(0))) {
                    return shiftOn ? key.toUpperCase() : key;
                }
                return key;
        }
    }

    private String descriptionFor(String key) {
        switch (key) {
            case KEY_SHIFT: return "Shift";
            case KEY_SPACE: return "Space";
            case KEY_ENTER: return "Enter";
            case KEY_BACKSPACE: return "Delete";
            case KEY_SYMBOLS: return "Symbols";
            case KEY_LETTERS: return "Letters";
            case KEY_MORE_SYMBOLS: return "More symbols";
            default: return key;
        }
    }

    private String spaceLabel() {
        switch (inputMode) {
            case MALAY: return getString(R.string.space_ms);
            case SIMPLIFIED: return getString(R.string.space_cn);
            case TRADITIONAL: return getString(R.string.space_tw);
            case JAPANESE: return getString(R.string.space_ja);
            case KOREAN: return getString(R.string.space_ko);
            case ENGLISH:
            default: return getString(R.string.space_en);
        }
    }

    private void onKeyPressed(String key) {
        InputConnection ic = getCurrentInputConnection();
        if (ic == null) return;
        switch (key) {
            case KEY_SHIFT:
                shiftOn = !shiftOn;
                rebuildKeyboard();
                refreshCandidates();
                return;
            case KEY_BACKSPACE:
                handleBackspace(ic);
                return;
            case KEY_SPACE:
                handleSpace(ic);
                return;
            case KEY_ENTER:
                handleEnter(ic);
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
                handleTextKey(ic, labelFor(key));
        }
    }

    // ---------------------------------------------------------------- modes ----

    private static boolean isLatin(InputMode mode) {
        return mode == InputMode.ENGLISH || mode == InputMode.MALAY;
    }

    private void setInputMode(InputMode nextMode) {
        if (!isLanguageEnabled(nextMode)) return;
        InputConnection ic = getCurrentInputConnection();
        if (ic != null) finishComposing(ic);
        inputMode = nextMode;
        engines.preload(nextMode);
        rebuildKeyboard();
        refreshCandidates();
    }

    private void cycleInputMode() {
        InputMode[] modes = InputMode.values();
        int start = inputMode.ordinal();
        for (int offset = 1; offset <= modes.length; offset++) {
            InputMode next = modes[(start + offset) % modes.length];
            if (isLanguageEnabled(next)) {
                setInputMode(next);
                return;
            }
        }
    }

    private boolean isLanguageEnabled(InputMode mode) {
        return ImePreferences.isLanguageEnabled(this, languageKey(mode));
    }

    private InputMode firstEnabledMode() {
        if (isLanguageEnabled(InputMode.ENGLISH)) return InputMode.ENGLISH;
        for (InputMode mode : InputMode.values()) {
            if (isLanguageEnabled(mode)) return mode;
        }
        return InputMode.ENGLISH;
    }

    private static String languageKey(InputMode mode) {
        switch (mode) {
            case SIMPLIFIED: return "simplified";
            case TRADITIONAL: return "traditional";
            case JAPANESE: return "japanese";
            case KOREAN: return "korean";
            case MALAY: return "malay";
            case ENGLISH:
            default: return "english";
        }
    }

    private String labelForCurrentMode() {
        switch (inputMode) {
            case ENGLISH: return getString(R.string.mode_en);
            case MALAY: return getString(R.string.mode_ms);
            case TRADITIONAL: return getString(R.string.mode_tw);
            case JAPANESE: return getString(R.string.mode_ja);
            case KOREAN: return getString(R.string.mode_ko);
            case SIMPLIFIED:
            default: return getString(R.string.mode_cn);
        }
    }

    // --------------------------------------------------------------- typing ----

    private LatinEngine latinEngine() {
        return inputMode == InputMode.MALAY ? engines.malay() : engines.english();
    }

    private static boolean isComposingChar(char ch) {
        return (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') || ch == '\'';
    }

    private void handleTextKey(InputConnection ic, String value) {
        char ch = value.charAt(0);
        if (symbolsMode || !isComposingChar(ch) || value.length() > 1) {
            finishComposing(ic);
            ic.commitText(value, 1);
            return;
        }
        if (isLatin(inputMode)) {
            composing.append(ch);
            ic.setComposingText(composing, 1);
        } else {
            composing.append(Character.toLowerCase(ch));
        }
        refreshCandidates();
    }

    private void handleBackspace(InputConnection ic) {
        if (composing.length() > 0) {
            composing.deleteCharAt(composing.length() - 1);
            if (isLatin(inputMode)) ic.setComposingText(composing, 1);
            refreshCandidates();
            return;
        }
        CharSequence selected = ic.getSelectedText(0);
        if (selected != null && selected.length() > 0) {
            ic.commitText("", 1);
        } else {
            ic.deleteSurroundingText(1, 0);
        }
    }

    private void handleSpace(InputConnection ic) {
        if (isLatin(inputMode)) {
            commitLatinWord(ic, true);
            ic.commitText(" ", 1);
        } else if (composing.length() > 0) {
            commitTopCandidate(ic);
        } else {
            ic.commitText(inputMode == InputMode.KOREAN ? " " : "　", 1);
        }
    }

    private void handleEnter(InputConnection ic) {
        if (isLatin(inputMode)) {
            commitLatinWord(ic, true);
        } else if (composing.length() > 0) {
            // Enter commits the letters as typed, like most CJK keyboards.
            ic.commitText(composing.toString(), 1);
            clearComposingState();
            return;
        }
        EditorInfo info = getCurrentInputEditorInfo();
        int action = info == null ? EditorInfo.IME_ACTION_NONE : info.imeOptions & EditorInfo.IME_MASK_ACTION;
        boolean multiLine = info != null && (info.inputType & EditorInfo.TYPE_TEXT_FLAG_MULTI_LINE) != 0;
        if (!multiLine && action != EditorInfo.IME_ACTION_NONE && action != EditorInfo.IME_ACTION_UNSPECIFIED
                && (info.imeOptions & EditorInfo.IME_FLAG_NO_ENTER_ACTION) == 0) {
            ic.performEditorAction(action);
        } else {
            ic.sendKeyEvent(new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER));
            ic.sendKeyEvent(new KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_ENTER));
        }
    }

    /** Commits the word being typed, applying a confident auto-correction if enabled. */
    private void commitLatinWord(InputConnection ic, boolean allowCorrection) {
        if (composing.length() == 0) return;
        String typed = composing.toString();
        String word = typed;
        if (allowCorrection && ImePreferences.isAutoCorrectEnabled(this)) {
            String fix = latinEngine().autoCorrection(typed);
            if (fix != null) word = matchCase(fix, typed);
        }
        ic.commitText(word, 1);
        clearComposingState();
    }

    private void commitTopCandidate(InputConnection ic) {
        List<String> candidates = getCandidates();
        ic.commitText(candidates.isEmpty() ? composing.toString() : candidates.get(0), 1);
        clearComposingState();
    }

    /** Ends any composition before inserting something else. */
    private void finishComposing(InputConnection ic) {
        if (composing.length() == 0) return;
        if (isLatin(inputMode)) {
            commitLatinWord(ic, true);
        } else {
            commitTopCandidate(ic);
        }
    }

    private void commitCandidate(InputConnection ic, String candidate) {
        ic.commitText(isLatin(inputMode) ? matchCase(candidate, composing.toString()) : candidate, 1);
        clearComposingState();
        if (ImePreferences.isAutoSpaceEnabled(this) && isLatin(inputMode)) {
            ic.commitText(" ", 1);
        }
    }

    /** Keeps the user's capitalisation: "Teh" -> "The", and Shift capitalises suggestions. */
    private String matchCase(String word, String typed) {
        if (word.isEmpty()) return word;
        boolean capital = shiftOn || (!typed.isEmpty() && Character.isUpperCase(typed.charAt(0)));
        return capital ? Character.toUpperCase(word.charAt(0)) + word.substring(1) : word;
    }

    private List<String> getCandidates() {
        if (composing.length() == 0) return Collections.emptyList();
        String input = composing.toString();
        switch (inputMode) {
            case ENGLISH:
            case MALAY: return latinEngine().getCandidates(input);
            case JAPANESE: return engines.japanese().getCandidates(input);
            case KOREAN: return engines.korean().getCandidates(input);
            case TRADITIONAL: return engines.pinyin().getCandidates(input, PinyinEngine.ScriptMode.TRADITIONAL);
            case SIMPLIFIED:
            default: return engines.pinyin().getCandidates(input, PinyinEngine.ScriptMode.SIMPLIFIED);
        }
    }

    private void clearComposingState() {
        composing.setLength(0);
        refreshCandidates();
    }

    // ----------------------------------------------------------- candidates ----

    private void refreshCandidates() {
        if (candidateContainer == null) return;
        candidateContainer.removeAllViews();
        suggestionList.removeAllViews();
        currentCandidates.clear();
        currentCandidates.addAll(getCandidates());
        LinearLayout currentRow = null;
        for (int i = 0; i < currentCandidates.size(); i++) {
            final String candidate = currentCandidates.get(i);
            String shown = isLatin(inputMode) ? matchCase(candidate, composing.toString()) : candidate;
            Button button = new Button(this);
            button.setText(shown);
            button.setSingleLine(true);
            button.setAllCaps(false);
            button.setMinWidth(dp(44));
            button.setMinimumWidth(dp(44));
            button.setMinHeight(0);
            button.setMinimumHeight(0);
            button.setStateListAnimator(null);
            button.setTextSize(TypedValue.COMPLEX_UNIT_SP, 17);
            button.setPadding(dp(12), 0, dp(12), 0);
            button.setTag(i == 0 ? "first" : null);
            LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, dp(36));
            lp.setMargins(dp(2), 0, dp(2), 0);
            button.setLayoutParams(lp);
            button.setOnClickListener(v -> {
                haptic(v);
                InputConnection ic = getCurrentInputConnection();
                if (ic != null) commitCandidate(ic, candidate);
            });
            candidateContainer.addView(button);
            if (i % 3 == 0) {
                currentRow = new LinearLayout(this);
                currentRow.setOrientation(LinearLayout.HORIZONTAL);
                suggestionList.addView(currentRow, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
            }
            addExpandedCandidate(currentRow, candidate, shown);
        }
        boolean hasCandidates = !currentCandidates.isEmpty();
        candidatePrev.setVisibility(hasCandidates ? View.VISIBLE : View.INVISIBLE);
        candidateNext.setVisibility(hasCandidates ? View.VISIBLE : View.INVISIBLE);
        candidateExpand.setEnabled(hasCandidates);
        suggestionSlider.setEnabled(hasCandidates);
        boolean showComposing = !isLatin(inputMode) && composing.length() > 0;
        composingLabel.setVisibility(showComposing ? View.VISIBLE : View.GONE);
        if (showComposing) composingLabel.setText(composingPreview());
        boolean grammarAvailable = ImePreferences.isGrammarButtonEnabled(this)
                && inputMode != InputMode.JAPANESE && inputMode != InputMode.KOREAN;
        grammarButton.setVisibility(grammarAvailable && !hasCandidates ? View.VISIBLE : View.GONE);
        candidateScroll.post(() -> candidateScroll.scrollTo(0, 0));
        suggestionScroll.post(() -> {
            suggestionScroll.scrollTo(0, 0);
            syncSuggestionSlider();
        });
        if (!hasCandidates && suggestionsExpanded) {
            suggestionsExpanded = false;
            updateSuggestionPanelVisibility();
        }
        updateExpandButtonLabel();
        applyTheme();
    }

    /** What the user has typed, shown left of the candidates (pinyin, kana, or Hangul). */
    private String composingPreview() {
        String typed = composing.toString();
        if (inputMode == InputMode.JAPANESE) return JapaneseEngine.toHiragana(typed, false);
        if (inputMode == InputMode.KOREAN) {
            String hangul = KoreanEngine.compose(typed);
            return hangul != null ? hangul : typed;
        }
        return typed;
    }

    private void addExpandedCandidate(LinearLayout row, String candidate, String shown) {
        Button button = new Button(this);
        button.setText(shown);
        button.setAllCaps(false);
        button.setGravity(Gravity.CENTER);
        button.setStateListAnimator(null);
        button.setTextSize(TypedValue.COMPLEX_UNIT_SP, 17);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(0, dp(48), 1f);
        lp.setMargins(dp(3), dp(3), dp(3), dp(3));
        button.setLayoutParams(lp);
        button.setOnClickListener(v -> {
            haptic(v);
            InputConnection ic = getCurrentInputConnection();
            if (ic != null) commitCandidate(ic, candidate);
        });
        row.addView(button);
    }

    private void scrollCandidates(int direction) {
        if (currentCandidates.isEmpty()) return;
        candidateScroll.smoothScrollBy(Math.max(dp(80), candidateScroll.getWidth() / 2) * direction, 0);
    }

    private void toggleSuggestionPanel() {
        if (currentCandidates.isEmpty()) return;
        suggestionsExpanded = !suggestionsExpanded;
        updateSuggestionPanelVisibility();
    }

    private void updateSuggestionPanelVisibility() {
        if (keyboardContainer == null) return;
        boolean grammarOpen = grammarPanel.getVisibility() == View.VISIBLE;
        keyboardContainer.setVisibility(suggestionsExpanded || grammarOpen ? View.GONE : View.VISIBLE);
        suggestionPanel.setVisibility(suggestionsExpanded && !grammarOpen ? View.VISIBLE : View.GONE);
        updateExpandButtonLabel();
    }

    private void updateExpandButtonLabel() {
        candidateExpand.setText(suggestionsExpanded ? R.string.candidate_collapse : R.string.candidate_expand);
    }

    private void syncSuggestionSlider() {
        if (suggestionSlider == null) return;
        int range = Math.max(0, suggestionList.getHeight() - suggestionScroll.getHeight());
        syncingSuggestionSlider = true;
        suggestionSlider.setMax(Math.max(1, range));
        suggestionSlider.setProgress(Math.min(suggestionScroll.getScrollY(), range));
        suggestionSlider.setEnabled(range > 0 && !currentCandidates.isEmpty());
        syncingSuggestionSlider = false;
    }

    // ------------------------------------------------------- touch feedback ----

    private void installKeyTouchHandler(KeyView button, String key) {
        button.setOnTouchListener((v, event) -> {
            switch (event.getActionMasked()) {
                case MotionEvent.ACTION_DOWN:
                    pressConsumed = false;
                    haptic(v);
                    styleKey(button, key, true);
                    if (isPreviewKey(key)) showKeyPreview(button, labelFor(key));
                    if (KEY_BACKSPACE.equals(key)) {
                        startRepeat(key);
                    } else if (button.hint() != null) {
                        startLongPress(button);
                    }
                    return true;
                case MotionEvent.ACTION_UP:
                    boolean consumed = pressConsumed;
                    stopRepeat();
                    dismissKeyPreview();
                    styleKey(button, key, false);
                    if (!consumed) onKeyPressed(key);
                    return true;
                case MotionEvent.ACTION_CANCEL:
                    stopRepeat();
                    dismissKeyPreview();
                    styleKey(button, key, false);
                    return true;
                default:
                    return false;
            }
        });
    }

    private boolean isPreviewKey(String key) {
        return ImePreferences.isKeyPopupEnabled(this) && !isFunctionKey(key) && !KEY_SPACE.equals(key) && !KEY_ENTER.equals(key);
    }

    private void showKeyPreview(View key, String label) {
        if (rootView == null || rootView.getWindowToken() == null) return;
        keyPreviewText.setText(label);
        keyPreviewText.setTextColor(theme.text);
        keyPreviewText.setBackground(theme.keyBackground(theme.popup, getResources().getDisplayMetrics().density));
        int width = Math.round(key.getWidth() * 1.25f);
        int height = Math.round(key.getHeight() * 1.35f);
        int[] location = new int[2];
        key.getLocationInWindow(location);
        int x = location[0] - (width - key.getWidth()) / 2;
        int y = location[1] - height + dp(4);
        keyPreview.setWidth(width);
        keyPreview.setHeight(height);
        if (keyPreview.isShowing()) {
            keyPreview.update(x, y, width, height);
        } else {
            keyPreview.showAtLocation(rootView, Gravity.NO_GRAVITY, x, y);
        }
    }

    private void dismissKeyPreview() {
        if (keyPreview != null && keyPreview.isShowing()) keyPreview.dismiss();
    }

    private void haptic(View view) {
        if (ImePreferences.isVibrationEnabled(this)) {
            view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP);
        }
    }

    private void startLongPress(KeyView button) {
        stopRepeat();
        longPressRunnable = () -> {
            pressConsumed = true;
            haptic(button);
            if (keyPreview.isShowing()) keyPreviewText.setText(button.hint());
            InputConnection ic = getCurrentInputConnection();
            if (ic != null) handleTextKey(ic, button.hint());
        };
        handler.postDelayed(longPressRunnable, LONG_PRESS_MS);
    }

    private void startRepeat(String key) {
        stopRepeat();
        repeatRunnable = new Runnable() {
            @Override
            public void run() {
                pressConsumed = true;
                onKeyPressed(key);
                handler.postDelayed(this, REPEAT_INTERVAL_MS);
            }
        };
        handler.postDelayed(repeatRunnable, REPEAT_INITIAL_DELAY_MS);
    }

    private void stopRepeat() {
        if (repeatRunnable != null) handler.removeCallbacks(repeatRunnable);
        if (longPressRunnable != null) handler.removeCallbacks(longPressRunnable);
        repeatRunnable = null;
        longPressRunnable = null;
    }

    // ---------------------------------------------------------------- theme ----

    private void applyTheme() {
        if (rootView == null) return;
        float density = getResources().getDisplayMetrics().density;
        rootView.setBackgroundColor(theme.background);
        for (LinearLayout row : new LinearLayout[]{row1, row2, row3, row4}) {
            for (int i = 0; i < row.getChildCount(); i++) {
                View child = row.getChildAt(i);
                if (child instanceof KeyView) styleKey((KeyView) child, (String) child.getTag(), false);
            }
        }
        for (Button button : new Button[]{candidatePrev, candidateNext, candidateExpand, grammarButton, grammarDismiss}) {
            button.setTextColor(theme.textMuted);
            button.setBackground(theme.pill(Color.TRANSPARENT, density));
        }
        grammarButton.setTextColor(theme.accent);
        grammarButton.setTypeface(Typeface.DEFAULT_BOLD);
        modeSwitch.setText(labelForCurrentMode());
        modeSwitch.setTextColor(theme.accent);
        modeSwitch.setTypeface(Typeface.DEFAULT_BOLD);
        modeSwitch.setBackground(theme.pill(theme.candidateHighlight, density));
        composingLabel.setTextColor(theme.textMuted);
        for (int i = 0; i < candidateContainer.getChildCount(); i++) {
            Button chip = (Button) candidateContainer.getChildAt(i);
            boolean first = "first".equals(chip.getTag());
            chip.setTextColor(theme.text);
            chip.setTypeface(first ? Typeface.DEFAULT_BOLD : Typeface.DEFAULT);
            chip.setBackground(theme.pill(first ? theme.candidateHighlight : Color.TRANSPARENT, density));
        }
        for (int r = 0; r < suggestionList.getChildCount(); r++) {
            ViewGroup row = (ViewGroup) suggestionList.getChildAt(r);
            for (int i = 0; i < row.getChildCount(); i++) {
                Button button = (Button) row.getChildAt(i);
                button.setTextColor(theme.text);
                button.setBackground(theme.keyBackground(theme.key, density));
            }
        }
        suggestionScroll.setBackgroundColor(theme.background);
        suggestionSlider.setThumbTintList(ColorStateList.valueOf(theme.accent));
        suggestionSlider.setProgressTintList(ColorStateList.valueOf(theme.accent));
        grammarPanel.setBackground(theme.keyBackground(theme.key, density));
        grammarTitle.setTextColor(theme.textMuted);
        grammarBody.setTextColor(theme.text);
        grammarApply.setTextColor(theme.accentText);
        grammarApply.setBackground(theme.pill(theme.accent, density));
        grammarApply.setAlpha(grammarApply.isEnabled() ? 1f : 0.4f);
    }

    private void styleKey(KeyView button, String key, boolean pressed) {
        float density = getResources().getDisplayMetrics().density;
        boolean function = isFunctionKey(key);
        boolean enter = KEY_ENTER.equals(key);
        int fill;
        if (enter) {
            fill = pressed ? theme.functionKeyPressed : theme.accent;
        } else if (function) {
            fill = pressed ? theme.functionKeyPressed : theme.functionKey;
        } else {
            fill = pressed ? theme.keyPressed : theme.key;
        }
        button.setBackground(theme.keyBackground(fill, density));
        button.setTextColor(enter ? theme.accentText : (KEY_SHIFT.equals(key) && shiftOn ? theme.accent : theme.text));
        if (KEY_SPACE.equals(key)) {
            button.setTextColor(theme.textMuted);
            button.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13);
        } else if (enter) {
            button.setTextSize(TypedValue.COMPLEX_UNIT_SP, 26);
        } else if (function) {
            button.setTextSize(TypedValue.COMPLEX_UNIT_SP, KEY_SYMBOLS.equals(key) || KEY_LETTERS.equals(key) || KEY_MORE_SYMBOLS.equals(key) ? 15 : 20);
        } else {
            button.setTextSize(TypedValue.COMPLEX_UNIT_SP, 21);
        }
        button.setPadding(0, 0, 0, 0);
        if (button.hint() != null) button.setHint(button.hint(), theme.textMuted);
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    // -------------------------------------------------------- grammar check ----

    private void startGrammarCheck() {
        InputConnection ic = getCurrentInputConnection();
        if (ic == null) return;
        finishComposing(ic);
        CharSequence selected = ic.getSelectedText(0);
        String text;
        if (selected != null && selected.toString().trim().length() > 0) {
            text = selected.toString();
            grammarFromSelection = true;
        } else {
            CharSequence before = ic.getTextBeforeCursor(2000, 0);
            String all = before == null ? "" : before.toString();
            // The current paragraph: everything after the last line break.
            text = all.substring(all.lastIndexOf('\n') + 1);
            grammarFromSelection = false;
        }
        String trimmed = text.replaceAll("\\s+$", "");
        if (!grammarFromSelection && trimmed.length() < text.length()) {
            text = trimmed;  // the trailing spaces stay in place; only the text before them is checked
        }
        boolean chineseMode = inputMode == InputMode.SIMPLIFIED || inputMode == InputMode.TRADITIONAL;
        String language = GrammarLanguage.detect(text, inputMode == InputMode.MALAY);
        if (chineseMode && !GrammarLanguage.MANDARIN.equals(language) && text.codePoints().anyMatch(c -> c >= 0x4e00 && c <= 0x9fff)) {
            language = GrammarLanguage.MANDARIN;
        }
        grammarOriginal = text;
        grammarResult = null;
        grammarPanel.setVisibility(View.VISIBLE);
        suggestionsExpanded = false;
        updateSuggestionPanelVisibility();
        grammarTitle.setText("AI check · " + language);
        grammarApply.setEnabled(false);
        applyTheme();
        if (text.trim().isEmpty()) {
            grammarBody.setText("Type or select a sentence first, then tap ✓.");
            return;
        }
        grammarBody.setText(text);
        grammarChecker.check(text, language, new GrammarChecker.Listener() {
            @Override
            public void onStatus(String message) {
                if (grammarPanel.getVisibility() == View.VISIBLE && grammarResult == null) grammarTitle.setText(message);
            }

            @Override
            public void onPartial(String partial) {
                if (grammarPanel.getVisibility() == View.VISIBLE) grammarBody.setText(partial);
            }

            @Override
            public void onResult(String corrected) {
                if (grammarPanel.getVisibility() != View.VISIBLE) return;
                grammarResult = corrected;
                boolean changed = TextDiff.changed(grammarOriginal, corrected);
                grammarTitle.setText(changed ? "Suggested corrections — review before replacing" : "No mistakes found ✓");
                grammarBody.setText(changed ? diffText(grammarOriginal, corrected) : grammarOriginal);
                grammarApply.setEnabled(changed);
                applyTheme();
            }

            @Override
            public void onError(String message) {
                if (grammarPanel.getVisibility() != View.VISIBLE) return;
                grammarTitle.setText("AI check unavailable");
                grammarBody.setText(message);
            }
        });
    }

    private CharSequence diffText(String before, String after) {
        SpannableStringBuilder text = new SpannableStringBuilder();
        for (TextDiff.Part part : TextDiff.diff(before, after)) {
            int start = text.length();
            text.append(part.text);
            if (part.kind == TextDiff.Kind.ADDED) {
                text.setSpan(new ForegroundColorSpan(theme.added), start, text.length(), Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                text.setSpan(new StyleSpan(Typeface.BOLD), start, text.length(), Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
            } else if (part.kind == TextDiff.Kind.REMOVED) {
                text.setSpan(new ForegroundColorSpan(theme.removed), start, text.length(), Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                text.setSpan(new StrikethroughSpan(), start, text.length(), Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
            }
        }
        return text;
    }

    private void applyGrammarResult() {
        InputConnection ic = getCurrentInputConnection();
        if (ic == null || grammarResult == null) return;
        if (grammarFromSelection) {
            CharSequence selected = ic.getSelectedText(0);
            if (selected == null || !TextUtils.equals(selected, grammarOriginal)) {
                grammarTitle.setText("The selection changed. Check again.");
                return;
            }
            ic.commitText(grammarResult, 1);
        } else {
            CharSequence before = ic.getTextBeforeCursor(grammarOriginal.length() + 64, 0);
            String text = before == null ? "" : before.toString();
            String stripped = text.replaceAll("\\s+$", "");
            String trailing = text.substring(stripped.length());
            if (!stripped.endsWith(grammarOriginal)) {
                grammarTitle.setText("The text changed. Check again.");
                return;
            }
            ic.beginBatchEdit();
            ic.deleteSurroundingText(grammarOriginal.length() + trailing.length(), 0);
            ic.commitText(grammarResult + trailing, 1);
            ic.endBatchEdit();
        }
        closeGrammarPanel();
    }

    private void closeGrammarPanel() {
        grammarChecker.cancel();
        grammarPanel.setVisibility(View.GONE);
        grammarResult = null;
        updateSuggestionPanelVisibility();
    }
}
