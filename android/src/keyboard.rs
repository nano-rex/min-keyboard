//! The keyboard itself: layout, drawing, touch handling and typing logic. It talks to Android
//! only through the [`Editor`], [`Host`] and [`Canvas`] traits, so it runs in desktop tests too.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use engines::{japanese, korean, Assets, JapaneseEngine, KoreanEngine, LatinEngine, PinyinEngine, ScriptMode};
use grammar::{Change, Language, Update};

use crate::theme::{self, Theme};

// ---------------------------------------------------------------- platform ----

/// The text field being typed into (Android's `InputConnection` and `EditorInfo`).
pub trait Editor {
    /// False when there is no input connection; key presses are then ignored.
    fn connected(&self) -> bool;
    fn commit_text(&mut self, text: &str);
    fn set_composing_text(&mut self, text: &str);
    fn finish_composing_text(&mut self);
    /// Deletes `count` code points before the cursor.
    fn delete_before(&mut self, count: i32);
    fn selected_text(&mut self) -> Option<String>;
    /// `EditorInfo.imeOptions` and `EditorInfo.inputType`, if there is an editor.
    fn editor_info(&mut self) -> Option<(i32, i32)>;
    fn perform_editor_action(&mut self, action: i32);
    fn send_enter_key(&mut self);
}

/// Services of the keyboard's view.
pub trait Host {
    fn haptic(&mut self);
    /// Calls [`Keyboard::on_timer`] after `delay_ms`, replacing any pending timer.
    fn schedule_timer(&mut self, delay_ms: i64);
    fn cancel_timer(&mut self);
    fn invalidate(&mut self);
}

/// The on-device AI grammar check, which runs in the background (see `grammar::Worker`).
pub trait Proofreader {
    /// Starts checking `text`; `on_update` may be called on another thread.
    fn check(&mut self, text: &str, language: Language, on_update: Box<dyn Fn(Update) + Send>);
    fn cancel(&mut self);
    /// Frees the model's memory, for example when the system runs low.
    fn release(&mut self);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Rect { left, top, right, bottom }
    }
    pub fn width(&self) -> f32 {
        self.right - self.left
    }
    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
    pub fn center_x(&self) -> f32 {
        (self.left + self.right) / 2.0
    }
    pub fn center_y(&self) -> f32 {
        (self.top + self.bottom) / 2.0
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
    fn offset(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.left + dx, self.top + dy, self.right + dx, self.bottom + dy)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    /// Size in pixels.
    pub size: f32,
    pub color: u32,
    pub bold: bool,
    pub align: Align,
}

/// Drawing operations, implemented with `android.graphics.Canvas` on the device.
pub trait Canvas {
    fn fill(&mut self, color: u32);
    fn round_rect(&mut self, rect: Rect, radius: f32, color: u32);
    fn text(&mut self, text: &str, x: f32, baseline: f32, style: TextStyle);
    fn measure(&mut self, text: &str, size: f32, bold: bool) -> f32;
    /// Font ascent (negative) and descent for the size.
    fn metrics(&mut self, size: f32, bold: bool) -> (f32, f32);
    fn clip(&mut self, rect: Rect);
    fn unclip(&mut self);
}

// ---------------------------------------------------------------- settings ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Simplified,
    Traditional,
    Japanese,
    Korean,
    English,
    Malay,
}

pub const MODES: [Mode; 6] = [Mode::Simplified, Mode::Traditional, Mode::Japanese, Mode::Korean, Mode::English, Mode::Malay];

impl Mode {
    pub fn is_latin(self) -> bool {
        matches!(self, Mode::English | Mode::Malay)
    }

    /// The preference name used by the settings screen.
    pub fn language_key(self) -> &'static str {
        match self {
            Mode::Simplified => "simplified",
            Mode::Traditional => "traditional",
            Mode::Japanese => "japanese",
            Mode::Korean => "korean",
            Mode::Malay => "malay",
            Mode::English => "english",
        }
    }

    fn button_label(self) -> &'static str {
        match self {
            Mode::English => "EN",
            Mode::Malay => "BM",
            Mode::Traditional => "繁",
            Mode::Japanese => "日",
            Mode::Korean => "한",
            Mode::Simplified => "中",
        }
    }

    fn space_label(self) -> &'static str {
        match self {
            Mode::Malay => "Bahasa Melayu",
            Mode::Simplified => "简体中文",
            Mode::Traditional => "繁體中文",
            Mode::Japanese => "日本語",
            Mode::Korean => "한국어",
            Mode::English => "English",
        }
    }

    fn index(self) -> usize {
        MODES.iter().position(|&m| m == self).unwrap_or(0)
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub dark: bool,
    pub auto_correct: bool,
    pub auto_space: bool,
    pub vibration: bool,
    pub key_popup: bool,
    pub number_hints: bool,
    /// Show the AI check button while text is selected.
    pub grammar_button: bool,
    /// Enabled flags, indexed like [`MODES`].
    pub languages: [bool; 6],
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dark: false,
            auto_correct: true,
            auto_space: false,
            vibration: true,
            key_popup: true,
            number_hints: true,
            grammar_button: false,
            languages: [true; 6],
        }
    }
}

impl Settings {
    fn enabled(&self, mode: Mode) -> bool {
        self.languages[mode.index()]
    }
}

// ----------------------------------------------------------------- engines ----

/// Loads each language's dictionary on first use, on a background thread where possible.
pub struct Engines {
    assets: Arc<dyn Assets + Send + Sync>,
    english: OnceLock<Option<LatinEngine>>,
    malay: OnceLock<Option<LatinEngine>>,
    pinyin: OnceLock<Option<PinyinEngine>>,
    japanese: OnceLock<Option<JapaneseEngine>>,
    korean: OnceLock<Option<KoreanEngine>>,
    /// Bumped whenever a dictionary finishes loading, so stale candidate lists get refreshed.
    generation: AtomicU64,
    on_loaded: Box<dyn Fn() + Send + Sync>,
}

fn loaded<T>(name: &str, result: std::io::Result<T>) -> Option<T> {
    result.map_err(|e| crate::log_error(&format!("cannot load the {name} dictionary: {e}"))).ok()
}

impl Engines {
    pub fn new(assets: Arc<dyn Assets + Send + Sync>, on_loaded: Box<dyn Fn() + Send + Sync>) -> Arc<Self> {
        Arc::new(Engines {
            assets,
            english: OnceLock::new(),
            malay: OnceLock::new(),
            pinyin: OnceLock::new(),
            japanese: OnceLock::new(),
            korean: OnceLock::new(),
            generation: AtomicU64::new(0),
            on_loaded,
        })
    }

    fn get<'a, T>(&self, cell: &'a OnceLock<Option<T>>, block: bool, load: impl FnOnce() -> Option<T>) -> Option<&'a T> {
        if block {
            cell.get_or_init(|| {
                let engine = load();
                self.generation.fetch_add(1, Ordering::SeqCst);
                engine
            })
            .as_ref()
        } else {
            cell.get().and_then(Option::as_ref)
        }
    }

    fn latin(&self, mode: Mode, block: bool) -> Option<&LatinEngine> {
        let assets = &*self.assets;
        if mode == Mode::Malay {
            self.get(&self.malay, block, || loaded("Malay", LatinEngine::malay(assets)))
        } else {
            self.get(&self.english, block, || loaded("English", LatinEngine::english(assets)))
        }
    }

    fn pinyin(&self, block: bool) -> Option<&PinyinEngine> {
        let assets = &*self.assets;
        self.get(&self.pinyin, block, || loaded("pinyin", PinyinEngine::new(assets)))
    }

    fn japanese(&self, block: bool) -> Option<&JapaneseEngine> {
        let assets = &*self.assets;
        self.get(&self.japanese, block, || loaded("Japanese", JapaneseEngine::new(assets)))
    }

    fn korean(&self, block: bool) -> Option<&KoreanEngine> {
        let assets = &*self.assets;
        self.get(&self.korean, block, || loaded("Korean", KoreanEngine::new(assets)))
    }

    fn is_loaded(&self, mode: Mode) -> bool {
        match mode {
            Mode::English => self.english.get().is_some(),
            Mode::Malay => self.malay.get().is_some(),
            Mode::Simplified | Mode::Traditional => self.pinyin.get().is_some(),
            Mode::Japanese => self.japanese.get().is_some(),
            Mode::Korean => self.korean.get().is_some(),
        }
    }

    /// Starts loading the mode's dictionary in the background.
    pub fn preload(self: &Arc<Self>, mode: Mode) {
        if self.is_loaded(mode) {
            return;
        }
        let engines = Arc::clone(self);
        let spawned = std::thread::Builder::new().name("min-keyboard-dictionary".into()).spawn(move || {
            match mode {
                Mode::Simplified | Mode::Traditional => _ = engines.pinyin(true),
                Mode::Japanese => _ = engines.japanese(true),
                Mode::Korean => _ = engines.korean(true),
                Mode::English | Mode::Malay => _ = engines.latin(mode, true),
            }
            (engines.on_loaded)();
        });
        if let Err(e) = spawned {
            crate::log_error(&format!("cannot start the dictionary thread: {e}"));
        }
    }

    /// Candidates for the input, or `None` while the dictionary is still loading (unless `block`).
    pub fn candidates(&self, mode: Mode, input: &str, block: bool) -> Option<Vec<String>> {
        if input.is_empty() {
            return Some(Vec::new());
        }
        Some(match mode {
            Mode::English | Mode::Malay => self.latin(mode, block)?.candidates(input),
            Mode::Japanese => self.japanese(block)?.candidates(input),
            Mode::Korean => self.korean(block)?.candidates(input),
            Mode::Traditional => self.pinyin(block)?.candidates(input, ScriptMode::Traditional),
            Mode::Simplified => self.pinyin(block)?.candidates(input, ScriptMode::Simplified),
        })
    }

    fn auto_correction(&self, mode: Mode, typed: &str) -> Option<String> {
        self.latin(mode, true)?.auto_correction(typed)
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

// -------------------------------------------------------------------- keys ----

const KEY_SHIFT: &str = "Shift";
const KEY_BACKSPACE: &str = "⌫";
const KEY_SPACE: &str = "Space";
const KEY_ENTER: &str = "Enter";
const KEY_SYMBOLS: &str = "Symbols";
const KEY_LETTERS: &str = "ABC";
const KEY_MORE_SYMBOLS: &str = "=\\<";

const ROW1: &[&str] = &["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"];
const ROW1_DIGITS: &[&str] = &["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
const ROW2: &[&str] = &["a", "s", "d", "f", "g", "h", "j", "k", "l"];
const ROW3: &[&str] = &[KEY_SHIFT, "z", "x", "c", "v", "b", "n", "m", KEY_BACKSPACE];
const ROW4: &[&str] = &[KEY_SYMBOLS, ",", KEY_SPACE, ".", KEY_ENTER];

const EN_SYMBOLS: [&[&str]; 4] = [
    &["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
    &["-", "/", ":", ";", "(", ")", "$", "&", "@", "'"],
    &[KEY_MORE_SYMBOLS, ".", ",", "?", "!", "\"", "#", "%", "*", KEY_BACKSPACE],
    &[KEY_LETTERS, "=", KEY_SPACE, KEY_ENTER],
];
const EN_SYMBOLS_2: [&[&str]; 4] = [
    &["[", "]", "{", "}", "<", ">", "^", "~", "|", "\\"],
    &["`", "_", "+", "=", "€", "£", "¥", "•", "§", "©"],
    &[KEY_SYMBOLS, "®", "™", "✓", "×", "÷", "°", "¶", "∆", KEY_BACKSPACE],
    &[KEY_LETTERS, ";", KEY_SPACE, KEY_ENTER],
];
const CN_SYMBOLS: [&[&str]; 4] = [
    &["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
    &["（", "）", "《", "》", "“", "”", "‘", "’", "￥", "'"],
    &[KEY_MORE_SYMBOLS, "。", "，", "？", "！", "：", "；", "、", "…", KEY_BACKSPACE],
    &[KEY_LETTERS, "·", KEY_SPACE, KEY_ENTER],
];
const CN_SYMBOLS_2: [&[&str]; 4] = [
    &["【", "】", "「", "」", "『", "』", "〈", "〉", "〔", "〕"],
    &["※", "￥", "€", "°", "㎡", "→", "←", "↑", "↓", "±"],
    &[KEY_SYMBOLS, "＋", "＝", "／", "＼", "＿", "﹣", "％", "＃", KEY_BACKSPACE],
    &[KEY_LETTERS, "｜", KEY_SPACE, KEY_ENTER],
];

const GRAMMAR_BUTTON: &str = "✓ Check grammar";

const REPEAT_INITIAL_DELAY_MS: i64 = 350;
const REPEAT_INTERVAL_MS: i64 = 60;
const LONG_PRESS_MS: i64 = 350;

// EditorInfo constants.
const IME_MASK_ACTION: i32 = 0xff;
const IME_ACTION_UNSPECIFIED: i32 = 0;
const IME_ACTION_NONE: i32 = 1;
const IME_FLAG_NO_ENTER_ACTION: i32 = 0x4000_0000;
const TYPE_TEXT_FLAG_MULTI_LINE: i32 = 0x0002_0000;

fn is_function_key(key: &str) -> bool {
    matches!(key, KEY_SHIFT | KEY_BACKSPACE | KEY_SYMBOLS | KEY_LETTERS | KEY_MORE_SYMBOLS)
}

fn weight(key: &str) -> f32 {
    match key {
        KEY_SPACE => 4.0,
        _ if is_function_key(key) || key == KEY_ENTER => 1.5,
        _ => 1.0,
    }
}

fn is_composing_char(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '\''
}

/// Keeps the user's capitalisation: "Teh" -> "The", and Shift capitalises suggestions.
fn match_case(word: &str, typed: &str, shift_on: bool) -> String {
    let capital = shift_on || typed.chars().next().is_some_and(char::is_uppercase);
    let mut chars = word.chars();
    match chars.next() {
        Some(first) if capital => first.to_uppercase().chain(chars).collect(),
        _ => word.to_string(),
    }
}

struct Key {
    key: &'static str,
    rect: Rect,
    hint: Option<&'static str>,
}

// ------------------------------------------------------------------- touch ----

#[derive(Clone, Copy, Debug)]
pub enum Touch {
    Down { id: i32, x: f32, y: f32 },
    Move { id: i32, x: f32, y: f32 },
    Up { id: i32, x: f32, y: f32 },
    Cancel,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Target {
    Key(usize),
    ModeButton,
    Prev,
    Next,
    Expand,
    Candidate(usize),
    StripDrag,
    PanelItem(usize),
    PanelDrag,
    Slider,
    GrammarButton,
    GrammarClose,
    GrammarReplace,
    GrammarDrag,
    Nothing,
}

#[derive(Clone, Copy, Debug)]
struct Press {
    id: i32,
    target: Target,
    start_x: f32,
    start_y: f32,
    /// Scroll position when the press started, for drags.
    start_scroll: f32,
    /// Set when a long press or key repeat already acted on this press.
    consumed: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Timer {
    None,
    Repeat(i32),
    LongPress(i32),
}

// ------------------------------------------------------------ grammar check ----

/// The AI check's panel, shown in place of the keys.
struct GrammarPanel {
    /// Numbers the checks so updates from a cancelled one are ignored.
    request: u64,
    original: String,
    title: String,
    /// The text shown: the selection, the answer so far, or the changes.
    body: Vec<(Change, String)>,
    /// The corrected text, once it differs from the selection.
    result: Option<String>,
    /// The check has answered or failed.
    finished: bool,
    scroll: f32,
    /// Wrapped lines of `body` for a width, rebuilt when either changes.
    lines: Option<(f32, Vec<Vec<Run>>)>,
}

/// A piece of one wrapped line of the panel's text.
struct Run {
    x: f32,
    width: f32,
    text: String,
    change: Change,
}

impl GrammarPanel {
    fn set_body(&mut self, body: Vec<(Change, String)>) {
        self.body = body;
        self.lines = None;
    }

    fn apply(&mut self, update: Update) {
        match update {
            Update::Status(status) if self.result.is_none() => self.title = status,
            Update::Status(_) => {}
            Update::Partial(text) => self.set_body(vec![(Change::Same, text)]),
            Update::Done(corrected) => {
                self.finished = true;
                if grammar::changed(&self.original, &corrected) {
                    self.title = "Suggested corrections — review before replacing".into();
                    self.set_body(grammar::diff(&self.original, &corrected));
                    self.result = Some(corrected);
                } else {
                    self.title = "No mistakes found ✓".into();
                    self.set_body(vec![(Change::Same, self.original.clone())]);
                }
            }
            Update::Error(message) => {
                self.finished = true;
                self.title = "AI check unavailable".into();
                self.set_body(vec![(Change::Same, message)]);
            }
        }
    }
}

// ---------------------------------------------------------------- keyboard ----

pub struct Keyboard {
    engines: Arc<Engines>,
    settings: Settings,
    theme: &'static Theme,
    density: f32,
    scaled_density: f32,
    width: f32,

    composing: String,
    candidates: Vec<String>,
    candidates_dirty: bool,
    seen_generation: u64,
    mode: Mode,
    shift_on: bool,
    symbols_mode: bool,
    symbols_page_two: bool,
    suggestions_expanded: bool,

    keys: Vec<Key>,
    presses: Vec<Press>,
    timer: Timer,
    preview: Option<(Rect, String)>,
    strip_scroll: f32,
    strip_content: f32,
    panel_scroll: f32,

    /// Whether the editor has text selected, which offers the AI check.
    selection: bool,
    proofreader: Option<Box<dyn Proofreader>>,
    grammar: Option<GrammarPanel>,
    grammar_requests: u64,
    /// Updates from the check, delivered here from the background and applied when drawing.
    grammar_updates: Arc<Mutex<Vec<(u64, Update)>>>,
}

impl Keyboard {
    pub fn new(engines: Arc<Engines>, settings: Settings, density: f32, scaled_density: f32) -> Self {
        engines.preload(Mode::English);
        let mut keyboard = Keyboard {
            engines,
            theme: theme::of(settings.dark),
            settings,
            density,
            scaled_density,
            width: 0.0,
            composing: String::new(),
            candidates: Vec::new(),
            candidates_dirty: true,
            seen_generation: 0,
            mode: Mode::English,
            shift_on: false,
            symbols_mode: false,
            symbols_page_two: false,
            suggestions_expanded: false,
            keys: Vec::new(),
            presses: Vec::new(),
            timer: Timer::None,
            preview: None,
            strip_scroll: 0.0,
            strip_content: 0.0,
            panel_scroll: 0.0,
            selection: false,
            proofreader: None,
            grammar: None,
            grammar_requests: 0,
            grammar_updates: Arc::new(Mutex::new(Vec::new())),
        };
        keyboard.rebuild();
        keyboard
    }

    pub fn set_proofreader(&mut self, proofreader: Option<Box<dyn Proofreader>>) {
        self.proofreader = proofreader;
    }

    /// Frees the AI model's memory (on low memory); an open check panel shows why it stopped.
    pub fn release_proofreader(&mut self) {
        if let Some(proofreader) = &mut self.proofreader {
            proofreader.release();
        }
        if let Some(panel) = self.grammar.as_mut().filter(|p| !p.finished) {
            panel.apply(Update::Error("Stopped because the device is low on memory.".into()));
        }
    }

    fn dp(&self, value: f32) -> f32 {
        value * self.density
    }

    fn sp(&self, value: f32) -> f32 {
        value * self.scaled_density
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn composing(&self) -> &str {
        &self.composing
    }

    // ------------------------------------------------------------ lifecycle ----

    /// Settings may have changed while the keyboard was hidden (theme, hints, languages).
    pub fn apply_settings(&mut self, settings: Settings) {
        self.theme = theme::of(settings.dark);
        self.settings = settings;
        self.rebuild();
        self.candidates_dirty = true;
    }

    pub fn on_start_input(&mut self, host: &mut dyn Host, has_selection: bool) {
        self.stop_timer(host);
        self.selection = has_selection;
        self.close_grammar();
        if !self.settings.enabled(self.mode) {
            self.mode = self.first_enabled_mode();
            self.engines.preload(self.mode);
        }
        self.composing.clear();
        self.shift_on = false;
        self.rebuild();
        self.schedule_refresh(host);
    }

    pub fn on_finish_input_view(&mut self, host: &mut dyn Host) {
        self.close_grammar();
        self.preview = None;
        self.presses.clear();
        self.stop_timer(host);
        host.invalidate();
    }

    pub fn on_update_selection(&mut self, editor: &mut dyn Editor, host: &mut dyn Host, sel_start: i32, sel_end: i32, candidates_end: i32) {
        let selection = sel_start != sel_end;
        if selection != self.selection {
            self.selection = selection;
            host.invalidate();
        }
        // The user moved the cursor away from the word being typed: finish it where it is.
        if self.mode.is_latin() && !self.composing.is_empty() && (sel_start != candidates_end || sel_end != candidates_end) {
            self.composing.clear();
            editor.finish_composing_text();
            self.schedule_refresh(host);
        }
    }

    // --------------------------------------------------------------- layout ----

    pub fn height(&self) -> f32 {
        self.dp(4.0 + 44.0 + 2.0 + 4.0 * 58.0 + 6.0)
    }

    pub fn set_width(&mut self, width: f32) {
        if (width - self.width).abs() > 0.5 {
            self.width = width;
            self.rebuild();
        }
    }

    fn bar_rect(&self) -> Rect {
        Rect::new(self.dp(4.0), self.dp(4.0), self.width - self.dp(4.0), self.dp(48.0))
    }

    fn keys_area(&self) -> Rect {
        let top = self.bar_rect().bottom + self.dp(2.0);
        Rect::new(self.dp(4.0), top, self.width - self.dp(4.0), top + self.dp(4.0 * 58.0))
    }

    fn rows(&self) -> [&'static [&'static str]; 4] {
        let latin = self.mode.is_latin();
        match (self.symbols_mode, latin, self.symbols_page_two) {
            (false, _, _) => [ROW1, ROW2, ROW3, ROW4],
            (true, true, false) => EN_SYMBOLS,
            (true, true, true) => EN_SYMBOLS_2,
            (true, false, false) => CN_SYMBOLS,
            (true, false, true) => CN_SYMBOLS_2,
        }
    }

    fn rebuild(&mut self) {
        self.keys.clear();
        let area = self.keys_area();
        let hints = !self.symbols_mode && self.settings.number_hints;
        for (r, row) in self.rows().iter().enumerate() {
            let padding = if !self.symbols_mode && r == 1 { self.dp(16.0) } else { 0.0 };
            let left = area.left + padding;
            let available = area.width() - 2.0 * padding;
            let total: f32 = row.iter().map(|k| weight(k)).sum();
            let top = area.top + r as f32 * self.dp(58.0);
            let mut x = left;
            for (i, &key) in row.iter().enumerate() {
                let w = available * weight(key) / total;
                let rect = Rect::new(x + self.dp(3.0), top + self.dp(4.0), x + w - self.dp(3.0), top + self.dp(54.0));
                let hint = (hints && r == 0).then(|| ROW1_DIGITS[i]);
                self.keys.push(Key { key, rect, hint });
                x += w;
            }
        }
    }

    fn label_for(&self, key: &str) -> String {
        let latin = self.mode.is_latin();
        match key {
            "," if !latin => "，".into(),
            "." if !latin => "。".into(),
            KEY_SHIFT => "⇧".into(),
            KEY_SPACE => self.mode.space_label().into(),
            KEY_ENTER => "↵".into(),
            KEY_BACKSPACE => "⌫".into(),
            KEY_SYMBOLS => "?123".into(),
            KEY_LETTERS => "ABC".into(),
            _ => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) if c.is_alphabetic() && self.shift_on => c.to_uppercase().collect(),
                    _ => key.into(),
                }
            }
        }
    }

    // ----------------------------------------------------------------- keys ----

    fn on_key_pressed(&mut self, key: &str, editor: &mut dyn Editor, host: &mut dyn Host) {
        if !editor.connected() {
            return;
        }
        match key {
            KEY_SHIFT => {
                self.shift_on = !self.shift_on;
                self.schedule_refresh(host);
            }
            KEY_BACKSPACE => self.handle_backspace(editor, host),
            KEY_SPACE => self.handle_space(editor, host),
            KEY_ENTER => self.handle_enter(editor, host),
            KEY_SYMBOLS => {
                self.symbols_mode = true;
                self.symbols_page_two = false;
                self.rebuild();
            }
            KEY_MORE_SYMBOLS => {
                self.symbols_mode = true;
                self.symbols_page_two = !self.symbols_page_two;
                self.rebuild();
            }
            KEY_LETTERS => {
                self.symbols_mode = false;
                self.symbols_page_two = false;
                self.rebuild();
            }
            _ => {
                let label = self.label_for(key);
                self.handle_text_key(&label, editor, host);
            }
        }
        host.invalidate();
    }

    // ---------------------------------------------------------------- modes ----

    fn set_mode(&mut self, next: Mode, editor: &mut dyn Editor, host: &mut dyn Host) {
        if !self.settings.enabled(next) {
            return;
        }
        if editor.connected() {
            self.finish_composing(editor, host);
        }
        self.mode = next;
        self.engines.preload(next);
        self.rebuild();
        self.schedule_refresh(host);
    }

    fn cycle_mode(&mut self, editor: &mut dyn Editor, host: &mut dyn Host) {
        let start = self.mode.index();
        for offset in 1..=MODES.len() {
            let next = MODES[(start + offset) % MODES.len()];
            if self.settings.enabled(next) {
                self.set_mode(next, editor, host);
                return;
            }
        }
    }

    fn first_enabled_mode(&self) -> Mode {
        if self.settings.enabled(Mode::English) {
            return Mode::English;
        }
        MODES.into_iter().find(|&m| self.settings.enabled(m)).unwrap_or(Mode::English)
    }

    // --------------------------------------------------------------- typing ----

    fn handle_text_key(&mut self, value: &str, editor: &mut dyn Editor, host: &mut dyn Host) {
        let mut chars = value.chars();
        let Some(ch) = chars.next() else { return };
        if self.symbols_mode || !is_composing_char(ch) || chars.next().is_some() {
            self.finish_composing(editor, host);
            editor.commit_text(value);
            return;
        }
        if self.mode.is_latin() {
            self.composing.push(ch);
            editor.set_composing_text(&self.composing);
        } else {
            self.composing.push(ch.to_ascii_lowercase());
        }
        self.schedule_refresh(host);
    }

    fn handle_backspace(&mut self, editor: &mut dyn Editor, host: &mut dyn Host) {
        if !self.composing.is_empty() {
            self.composing.pop();
            if self.mode.is_latin() {
                editor.set_composing_text(&self.composing);
            }
            self.schedule_refresh(host);
            return;
        }
        match editor.selected_text() {
            Some(selected) if !selected.is_empty() => editor.commit_text(""),
            _ => editor.delete_before(1),
        }
    }

    fn handle_space(&mut self, editor: &mut dyn Editor, host: &mut dyn Host) {
        if self.mode.is_latin() {
            self.commit_latin_word(editor, host, true);
            editor.commit_text(" ");
        } else if !self.composing.is_empty() {
            self.commit_top_candidate(editor, host);
        } else {
            editor.commit_text(if self.mode == Mode::Korean { " " } else { "\u{3000}" });
        }
    }

    fn handle_enter(&mut self, editor: &mut dyn Editor, host: &mut dyn Host) {
        if self.mode.is_latin() {
            self.commit_latin_word(editor, host, true);
        } else if !self.composing.is_empty() {
            // Enter commits the letters as typed, like most CJK keyboards.
            editor.commit_text(&self.composing);
            self.clear_composing(host);
            return;
        }
        let info = editor.editor_info();
        let (options, input_type) = info.unwrap_or((0, 0));
        let action = if info.is_none() { IME_ACTION_NONE } else { options & IME_MASK_ACTION };
        let multi_line = info.is_some() && input_type & TYPE_TEXT_FLAG_MULTI_LINE != 0;
        if !multi_line && action != IME_ACTION_NONE && action != IME_ACTION_UNSPECIFIED && options & IME_FLAG_NO_ENTER_ACTION == 0 {
            editor.perform_editor_action(action);
        } else {
            editor.send_enter_key();
        }
    }

    /// Commits the word being typed, applying a confident auto-correction if enabled.
    fn commit_latin_word(&mut self, editor: &mut dyn Editor, host: &mut dyn Host, allow_correction: bool) {
        if self.composing.is_empty() {
            return;
        }
        let typed = self.composing.clone();
        let mut word = typed.clone();
        if allow_correction && self.settings.auto_correct {
            if let Some(fix) = self.engines.auto_correction(self.mode, &typed) {
                word = match_case(&fix, &typed, self.shift_on);
            }
        }
        editor.commit_text(&word);
        self.clear_composing(host);
    }

    fn commit_top_candidate(&mut self, editor: &mut dyn Editor, host: &mut dyn Host) {
        let candidates = self.engines.candidates(self.mode, &self.composing, true).unwrap_or_default();
        let text = candidates.into_iter().next().unwrap_or_else(|| self.composing.clone());
        editor.commit_text(&text);
        self.clear_composing(host);
    }

    /// Ends any composition before inserting something else.
    fn finish_composing(&mut self, editor: &mut dyn Editor, host: &mut dyn Host) {
        if self.composing.is_empty() {
            return;
        }
        if self.mode.is_latin() {
            self.commit_latin_word(editor, host, true);
        } else {
            self.commit_top_candidate(editor, host);
        }
    }

    fn commit_candidate(&mut self, candidate: &str, editor: &mut dyn Editor, host: &mut dyn Host) {
        let text = if self.mode.is_latin() { match_case(candidate, &self.composing, self.shift_on) } else { candidate.to_string() };
        editor.commit_text(&text);
        self.clear_composing(host);
        if self.settings.auto_space && self.mode.is_latin() {
            editor.commit_text(" ");
        }
    }

    fn clear_composing(&mut self, host: &mut dyn Host) {
        self.composing.clear();
        self.schedule_refresh(host);
    }

    // ----------------------------------------------------------- candidates ----

    fn schedule_refresh(&mut self, host: &mut dyn Host) {
        self.candidates_dirty = true;
        host.invalidate();
    }

    /// Recomputes the candidates if the input changed or a dictionary finished loading.
    fn refresh_candidates(&mut self) {
        let generation = self.engines.generation();
        if !self.candidates_dirty && generation == self.seen_generation {
            return;
        }
        self.seen_generation = generation;
        match self.engines.candidates(self.mode, &self.composing, false) {
            Some(candidates) => {
                self.candidates = candidates;
                self.candidates_dirty = false;
            }
            // Still loading: show nothing and try again once the dictionary is ready.
            None => self.candidates.clear(),
        }
        self.strip_scroll = 0.0;
        self.panel_scroll = 0.0;
        if self.candidates.is_empty() {
            self.suggestions_expanded = false;
        }
    }

    fn shown(&self, candidate: &str) -> String {
        if self.mode.is_latin() {
            match_case(candidate, &self.composing, self.shift_on)
        } else {
            candidate.to_string()
        }
    }

    /// What the user has typed, shown left of the candidates (pinyin, kana, or Hangul).
    fn composing_preview(&self) -> String {
        match self.mode {
            Mode::Japanese => japanese::to_hiragana(&self.composing, false),
            Mode::Korean => korean::compose(&self.composing).unwrap_or_else(|| self.composing.clone()),
            _ => self.composing.clone(),
        }
    }

    fn show_composing_label(&self) -> bool {
        !self.mode.is_latin() && !self.composing.is_empty()
    }

    // ------------------------------------------------------------ top bar ----

    fn mode_button(&self) -> Rect {
        let bar = self.bar_rect();
        let top = bar.center_y() - self.dp(18.0);
        Rect::new(bar.left, top, bar.left + self.dp(44.0), top + self.dp(36.0))
    }

    fn prev_button(&self) -> Rect {
        let m = self.mode_button();
        Rect::new(m.right + self.dp(2.0), m.top, m.right + self.dp(30.0), m.bottom)
    }

    fn expand_button(&self) -> Rect {
        let m = self.mode_button();
        let right = self.bar_rect().right;
        Rect::new(right - self.dp(40.0), m.top, right, m.bottom)
    }

    fn next_button(&self) -> Rect {
        let e = self.expand_button();
        Rect::new(e.left - self.dp(30.0), e.top, e.left - self.dp(2.0), e.bottom)
    }

    /// Width of the composing label, measured and capped at 120dp; zero when hidden.
    fn composing_label_width(&self, canvas: &mut dyn Canvas) -> f32 {
        if !self.show_composing_label() {
            return 0.0;
        }
        canvas.measure(&self.composing_preview(), self.sp(13.0), false).min(self.dp(120.0))
    }

    fn strip_rect(&self, label_width: f32) -> Rect {
        let prev = self.prev_button();
        let left = if label_width > 0.0 { prev.right + label_width + self.dp(4.0) } else { prev.right };
        Rect::new(left, self.bar_rect().top, self.next_button().left, self.bar_rect().bottom)
    }

    /// Chip rectangles in strip coordinates (before scrolling).
    fn chip_rects(&self, canvas: &mut dyn Canvas, strip: Rect) -> Vec<Rect> {
        let mut x = strip.left;
        let top = strip.center_y() - self.dp(18.0);
        self.candidates
            .iter()
            .enumerate()
            .map(|(i, candidate)| {
                let width = (canvas.measure(&self.shown(candidate), self.sp(17.0), i == 0) + self.dp(24.0)).max(self.dp(44.0));
                let rect = Rect::new(x + self.dp(2.0), top, x + self.dp(2.0) + width, top + self.dp(36.0));
                x += width + self.dp(4.0);
                rect
            })
            .collect()
    }

    fn scroll_candidates(&mut self, direction: f32, strip_width: f32) {
        if self.candidates.is_empty() {
            return;
        }
        let step = self.dp(80.0).max(strip_width / 2.0);
        self.strip_scroll = (self.strip_scroll + step * direction).clamp(0.0, (self.strip_content - strip_width).max(0.0));
    }

    fn toggle_panel(&mut self) {
        if !self.candidates.is_empty() {
            self.suggestions_expanded = !self.suggestions_expanded;
            self.panel_scroll = 0.0;
        }
    }

    // ---------------------------------------------------------------- panel ----

    fn panel_grid(&self) -> Rect {
        let area = self.keys_area();
        Rect::new(area.left, area.top, area.right - self.dp(52.0), area.bottom)
    }

    fn slider_rect(&self) -> Rect {
        let area = self.keys_area();
        Rect::new(area.right - self.dp(48.0), area.top, area.right, area.bottom)
    }

    fn panel_row_height(&self) -> f32 {
        self.dp(54.0)
    }

    fn panel_max_scroll(&self) -> f32 {
        let rows = self.candidates.len().div_ceil(3) as f32;
        (rows * self.panel_row_height() - self.panel_grid().height()).max(0.0)
    }

    /// The cell of candidate `i`, in panel coordinates before scrolling.
    fn panel_cell(&self, i: usize) -> Rect {
        let grid = self.panel_grid();
        let cell_width = grid.width() / 3.0;
        let (row, column) = ((i / 3) as f32, (i % 3) as f32);
        let left = grid.left + column * cell_width;
        let top = grid.top + row * self.panel_row_height();
        Rect::new(left + self.dp(3.0), top + self.dp(3.0), left + cell_width - self.dp(3.0), top + self.dp(51.0))
    }

    fn panel_hit(&self, x: f32, y: f32) -> Option<usize> {
        if !self.panel_grid().contains(x, y) {
            return None;
        }
        (0..self.candidates.len()).find(|&i| self.panel_cell(i).offset(0.0, -self.panel_scroll).contains(x, y))
    }

    fn set_panel_scroll_from_slider(&mut self, y: f32) {
        let track = self.slider_rect();
        let fraction = ((y - track.top - self.dp(12.0)) / (track.height() - self.dp(24.0))).clamp(0.0, 1.0);
        self.panel_scroll = fraction * self.panel_max_scroll();
    }

    // ---------------------------------------------------------------- touch ----

    fn hit_test(&self, x: f32, y: f32, canvas: &mut dyn Canvas) -> Target {
        if self.mode_button().contains(x, y) {
            return Target::ModeButton;
        }
        if self.expand_button().contains(x, y) {
            return Target::Expand;
        }
        let has_candidates = !self.candidates.is_empty();
        if has_candidates && self.prev_button().contains(x, y) {
            return Target::Prev;
        }
        if has_candidates && self.next_button().contains(x, y) {
            return Target::Next;
        }
        if self.grammar_button_visible() && self.grammar_button(canvas).contains(x, y) {
            return Target::GrammarButton;
        }
        let strip = self.strip_rect(self.composing_label_width(canvas));
        if strip.contains(x, y) {
            let chips = self.chip_rects(canvas, strip);
            return chips
                .iter()
                .position(|r| r.offset(-self.strip_scroll, 0.0).contains(x, y))
                .map(Target::Candidate)
                .unwrap_or(Target::StripDrag);
        }
        if self.grammar.is_some() {
            return if self.grammar_close_rect().contains(x, y) {
                Target::GrammarClose
            } else if self.grammar_replace_rect().contains(x, y) {
                Target::GrammarReplace
            } else if self.keys_area().contains(x, y) {
                Target::GrammarDrag
            } else {
                Target::Nothing
            };
        }
        if self.suggestions_expanded {
            if self.slider_rect().contains(x, y) {
                return Target::Slider;
            }
            return self.panel_hit(x, y).map(Target::PanelItem).unwrap_or(if self.panel_grid().contains(x, y) {
                Target::PanelDrag
            } else {
                Target::Nothing
            });
        }
        // Keys fill their row, so pick the nearest key in the touched row (margins included).
        let row_height = self.dp(58.0);
        let area = self.keys_area();
        if y < area.top - self.dp(2.0) || y > area.bottom + self.dp(6.0) {
            return Target::Nothing;
        }
        let row = (((y - area.top) / row_height).floor().max(0.0) as usize).min(3);
        let row_top = area.top + row as f32 * row_height + self.dp(4.0);
        self.keys
            .iter()
            .enumerate()
            .filter(|(_, k)| (k.rect.top - row_top).abs() < 1.0)
            .min_by(|(_, a), (_, b)| {
                let da = (a.rect.center_x() - x).abs() - a.rect.width() / 2.0;
                let db = (b.rect.center_x() - x).abs() - b.rect.width() / 2.0;
                da.total_cmp(&db)
            })
            .map(|(i, _)| Target::Key(i))
            .unwrap_or(Target::Nothing)
    }

    fn is_preview_key(&self, key: &str) -> bool {
        self.settings.key_popup && !is_function_key(key) && key != KEY_SPACE && key != KEY_ENTER
    }

    fn haptic(&self, host: &mut dyn Host) {
        if self.settings.vibration {
            host.haptic();
        }
    }

    fn preview_rect(&self, key: Rect) -> Rect {
        let width = key.width() * 1.25;
        let height = key.height() * 1.35;
        let left = (key.left - (width - key.width()) / 2.0).clamp(0.0, (self.width - width).max(0.0));
        let top = (key.top - height + self.dp(4.0)).max(0.0);
        Rect::new(left, top, left + width, top + height)
    }

    pub fn on_touch(&mut self, touch: Touch, editor: &mut dyn Editor, host: &mut dyn Host, canvas: &mut dyn Canvas) {
        self.refresh_candidates();
        self.apply_grammar_updates();
        let slop = self.dp(8.0);
        match touch {
            Touch::Down { id, x, y } => {
                let target = self.hit_test(x, y, canvas);
                let start_scroll = match target {
                    Target::Candidate(_) | Target::StripDrag => self.strip_scroll,
                    Target::GrammarDrag => self.grammar.as_ref().map_or(0.0, |p| p.scroll),
                    _ => self.panel_scroll,
                };
                self.presses.retain(|p| p.id != id);
                self.presses.push(Press { id, target, start_x: x, start_y: y, start_scroll, consumed: false });
                match target {
                    Target::Key(index) => {
                        let (key, rect, hint) = (self.keys[index].key, self.keys[index].rect, self.keys[index].hint);
                        self.haptic(host);
                        self.preview = self.is_preview_key(key).then(|| (self.preview_rect(rect), self.label_for(key)));
                        if key == KEY_BACKSPACE {
                            self.timer = Timer::Repeat(id);
                            host.schedule_timer(REPEAT_INITIAL_DELAY_MS);
                        } else if hint.is_some() {
                            self.timer = Timer::LongPress(id);
                            host.schedule_timer(LONG_PRESS_MS);
                        } else {
                            self.stop_timer(host);
                        }
                    }
                    Target::Slider => self.set_panel_scroll_from_slider(y),
                    _ => {}
                }
                host.invalidate();
            }
            Touch::Move { id, x, y } => {
                let Some(press) = self.presses.iter_mut().find(|p| p.id == id) else { return };
                match press.target {
                    Target::Candidate(_) if (x - press.start_x).abs() > slop => press.target = Target::StripDrag,
                    Target::PanelItem(_) if (y - press.start_y).abs() > slop => press.target = Target::PanelDrag,
                    _ => {}
                }
                let press = *press;
                match press.target {
                    Target::StripDrag => {
                        let strip_width = self.strip_rect(self.composing_label_width(canvas)).width();
                        self.strip_scroll =
                            (press.start_scroll - (x - press.start_x)).clamp(0.0, (self.strip_content - strip_width).max(0.0));
                        host.invalidate();
                    }
                    Target::PanelDrag => {
                        self.panel_scroll = (press.start_scroll - (y - press.start_y)).clamp(0.0, self.panel_max_scroll());
                        host.invalidate();
                    }
                    Target::Slider => {
                        self.set_panel_scroll_from_slider(y);
                        host.invalidate();
                    }
                    Target::GrammarDrag => {
                        let max = self.grammar_max_scroll();
                        if let Some(panel) = &mut self.grammar {
                            panel.scroll = (press.start_scroll - (y - press.start_y)).clamp(0.0, max);
                        }
                        host.invalidate();
                    }
                    _ => {}
                }
            }
            Touch::Up { id, x, y } => {
                let Some(position) = self.presses.iter().position(|p| p.id == id) else { return };
                let press = self.presses.remove(position);
                if matches!(self.timer, Timer::Repeat(t) | Timer::LongPress(t) if t == id) {
                    self.stop_timer(host);
                }
                match press.target {
                    Target::Key(index) => {
                        self.preview = None;
                        if !press.consumed {
                            if let Some(key) = self.keys.get(index).map(|k| k.key) {
                                self.on_key_pressed(key, editor, host);
                            }
                        }
                    }
                    target => self.click(target, x, y, editor, host, canvas),
                }
                host.invalidate();
            }
            Touch::Cancel => {
                self.presses.clear();
                self.preview = None;
                self.stop_timer(host);
                host.invalidate();
            }
        }
    }

    /// A tap on something other than a key; it counts only if released where it started.
    fn click(&mut self, target: Target, x: f32, y: f32, editor: &mut dyn Editor, host: &mut dyn Host, canvas: &mut dyn Canvas) {
        if self.hit_test(x, y, canvas) != target {
            return;
        }
        match target {
            Target::ModeButton => self.cycle_mode(editor, host),
            Target::Expand => self.toggle_panel(),
            Target::Prev | Target::Next => {
                let strip_width = self.strip_rect(self.composing_label_width(canvas)).width();
                self.scroll_candidates(if target == Target::Prev { -1.0 } else { 1.0 }, strip_width);
            }
            Target::Candidate(i) | Target::PanelItem(i) => {
                if let Some(candidate) = self.candidates.get(i).cloned() {
                    self.haptic(host);
                    if editor.connected() {
                        self.commit_candidate(&candidate, editor, host);
                    }
                }
            }
            Target::GrammarButton => {
                self.haptic(host);
                self.start_grammar(editor, host);
            }
            Target::GrammarClose => self.close_grammar(),
            Target::GrammarReplace => self.replace_with_correction(editor),
            _ => {}
        }
    }

    /// Key repeat and long-press, scheduled with [`Host::schedule_timer`].
    pub fn on_timer(&mut self, editor: &mut dyn Editor, host: &mut dyn Host) {
        match self.timer {
            Timer::Repeat(id) => {
                let Some(press) = self.presses.iter_mut().find(|p| p.id == id) else { return self.stop_timer(host) };
                press.consumed = true;
                let Target::Key(index) = press.target else { return };
                let key = self.keys[index].key;
                self.on_key_pressed(key, editor, host);
                host.schedule_timer(REPEAT_INTERVAL_MS);
            }
            Timer::LongPress(id) => {
                self.timer = Timer::None;
                let Some(press) = self.presses.iter_mut().find(|p| p.id == id) else { return };
                press.consumed = true;
                let Target::Key(index) = press.target else { return };
                let Some(hint) = self.keys[index].hint else { return };
                self.haptic(host);
                if let Some(preview) = &mut self.preview {
                    preview.1 = hint.to_string();
                }
                if editor.connected() {
                    self.handle_text_key(hint, editor, host);
                }
                host.invalidate();
            }
            Timer::None => {}
        }
    }

    fn stop_timer(&mut self, host: &mut dyn Host) {
        if self.timer != Timer::None {
            host.cancel_timer();
        }
        self.timer = Timer::None;
    }

    fn pressed(&self, index: usize) -> bool {
        self.presses.iter().any(|p| p.target == Target::Key(index))
    }

    // -------------------------------------------------------- grammar check ----

    fn grammar_button_visible(&self) -> bool {
        self.settings.grammar_button
            && self.proofreader.is_some()
            && self.selection
            && self.grammar.is_none()
            && self.composing.is_empty()
            && self.candidates.is_empty()
    }

    fn grammar_button(&self, canvas: &mut dyn Canvas) -> Rect {
        let strip = self.strip_rect(0.0);
        let width = canvas.measure(GRAMMAR_BUTTON, self.sp(15.0), true) + self.dp(28.0);
        let top = strip.center_y() - self.dp(18.0);
        let left = strip.left + self.dp(2.0);
        Rect::new(left, top, (left + width).min(strip.right), top + self.dp(36.0))
    }

    fn grammar_title_rect(&self) -> Rect {
        let area = self.keys_area();
        Rect::new(area.left + self.dp(12.0), area.top + self.dp(4.0), area.right - self.dp(12.0), area.top + self.dp(30.0))
    }

    fn grammar_body_rect(&self) -> Rect {
        let area = self.keys_area();
        Rect::new(area.left + self.dp(12.0), area.top + self.dp(32.0), area.right - self.dp(12.0), area.bottom - self.dp(58.0))
    }

    fn grammar_close_rect(&self) -> Rect {
        let area = self.keys_area();
        Rect::new(area.left + self.dp(8.0), area.bottom - self.dp(52.0), area.center_x() - self.dp(4.0), area.bottom - self.dp(8.0))
    }

    fn grammar_replace_rect(&self) -> Rect {
        let area = self.keys_area();
        Rect::new(area.center_x() + self.dp(4.0), area.bottom - self.dp(52.0), area.right - self.dp(8.0), area.bottom - self.dp(8.0))
    }

    fn grammar_line_height(&self) -> f32 {
        self.sp(16.0) * 1.45
    }

    fn grammar_max_scroll(&self) -> f32 {
        let lines = self.grammar.as_ref().and_then(|p| p.lines.as_ref()).map_or(0, |(_, lines)| lines.len());
        (lines as f32 * self.grammar_line_height() - self.grammar_body_rect().height()).max(0.0)
    }

    /// Checks the selected text in the background and opens the panel that shows the result.
    fn start_grammar(&mut self, editor: &mut dyn Editor, host: &mut dyn Host) {
        let text = editor.selected_text().unwrap_or_default();
        self.grammar_requests += 1;
        let request = self.grammar_requests;
        let mut language = grammar::detect_language(&text, self.mode == Mode::Malay);
        let chinese_mode = matches!(self.mode, Mode::Simplified | Mode::Traditional);
        if chinese_mode && text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
            language = Language::Mandarin;
        }
        let mut panel = GrammarPanel {
            request,
            original: text.clone(),
            title: format!("AI check · {}", language.name()),
            body: vec![(Change::Same, text.clone())],
            result: None,
            finished: false,
            scroll: 0.0,
            lines: None,
        };
        self.suggestions_expanded = false;
        if text.trim().is_empty() {
            panel.apply(Update::Error("This app does not let keyboards read the selected text.".into()));
        } else if grammar::is_unsupported_script(&text) {
            panel.apply(Update::Error("The AI check works with English, Mandarin and Malay.".into()));
        } else if let Some(proofreader) = &mut self.proofreader {
            let updates = Arc::clone(&self.grammar_updates);
            proofreader.check(
                &text,
                language,
                Box::new(move |update| updates.lock().unwrap_or_else(|e| e.into_inner()).push((request, update))),
            );
        }
        self.grammar = Some(panel);
        host.invalidate();
    }

    fn apply_grammar_updates(&mut self) {
        let updates = std::mem::take(&mut *self.grammar_updates.lock().unwrap_or_else(|e| e.into_inner()));
        let Some(panel) = &mut self.grammar else { return };
        for (request, update) in updates {
            if request == panel.request {
                panel.apply(update);
            }
        }
    }

    /// Replaces the selection with the correction, if the selection is still what was checked.
    fn replace_with_correction(&mut self, editor: &mut dyn Editor) {
        let Some(panel) = &mut self.grammar else { return };
        let Some(result) = panel.result.clone() else { return };
        if editor.selected_text().as_deref() != Some(panel.original.as_str()) {
            panel.title = "The selection changed. Select the text and check again.".into();
            return;
        }
        editor.commit_text(&result);
        self.close_grammar();
    }

    fn close_grammar(&mut self) {
        if self.grammar.take().is_some() {
            if let Some(proofreader) = &mut self.proofreader {
                proofreader.cancel();
            }
        }
    }

    /// Wraps the panel's text into lines of runs that fit `width`.
    fn wrap(&self, canvas: &mut dyn Canvas, body: &[(Change, String)], width: f32) -> Vec<Vec<Run>> {
        let size = self.sp(16.0);
        let mut lines: Vec<Vec<Run>> = vec![Vec::new()];
        let mut x = 0.0;
        for (change, text) in body {
            let bold = *change == Change::Added;
            for word in grammar::text::diff_tokens(text) {
                if word == "\n" {
                    lines.push(Vec::new());
                    x = 0.0;
                    continue;
                }
                let word_width = canvas.measure(word, size, bold);
                // A word wider than the panel is broken between characters.
                let pieces: Vec<(String, f32)> = if word_width > width {
                    word.chars().map(|c| (c.to_string(), canvas.measure(&c.to_string(), size, bold))).collect()
                } else {
                    vec![(word.to_string(), word_width)]
                };
                for (piece, piece_width) in pieces {
                    if x + piece_width > width && x > 0.0 {
                        lines.push(Vec::new());
                        x = 0.0;
                        if piece.trim().is_empty() {
                            continue; // no space at the start of a wrapped line
                        }
                    }
                    let line = lines.last_mut().unwrap();
                    match line.last_mut() {
                        Some(run) if run.change == *change => {
                            run.text.push_str(&piece);
                            run.width += piece_width;
                        }
                        _ => line.push(Run { x, width: piece_width, text: piece, change: *change }),
                    }
                    x += piece_width;
                }
            }
        }
        lines
    }

    fn draw_grammar(&mut self, canvas: &mut dyn Canvas) {
        let theme = self.theme;
        let area = self.keys_area();
        canvas.round_rect(Rect::new(area.left, area.top + self.dp(2.0), area.right, area.bottom), self.dp(10.0), theme.key);
        let body = self.grammar_body_rect();
        let stale = |panel: &GrammarPanel| panel.lines.as_ref().is_none_or(|(width, _)| (width - body.width()).abs() > 0.5);
        if let Some(panel) = self.grammar.as_ref().filter(|p| stale(p)) {
            let lines = self.wrap(canvas, &panel.body, body.width());
            self.grammar.as_mut().unwrap().lines = Some((body.width(), lines));
        }
        let max_scroll = self.grammar_max_scroll();
        if let Some(panel) = &mut self.grammar {
            panel.scroll = panel.scroll.min(max_scroll);
        }
        let Some(panel) = &self.grammar else { return };

        let title_rect = self.grammar_title_rect();
        let size = self.sp(13.0);
        let title = self.fit(canvas, &panel.title, size, false, title_rect.width(), false);
        let (ascent, descent) = canvas.metrics(size, false);
        let baseline = title_rect.center_y() - (ascent + descent) / 2.0;
        canvas.text(&title, title_rect.left, baseline, TextStyle { size, color: theme.text_muted, bold: false, align: Align::Left });

        let size = self.sp(16.0);
        let line_height = self.grammar_line_height();
        let (ascent, descent) = canvas.metrics(size, false);
        canvas.clip(body);
        for (i, line) in panel.lines.iter().flat_map(|(_, lines)| lines).enumerate() {
            let top = body.top + i as f32 * line_height - panel.scroll;
            if top + line_height < body.top || top > body.bottom {
                continue;
            }
            let baseline = top + line_height / 2.0 - (ascent + descent) / 2.0;
            for run in line {
                let (color, bold) = match run.change {
                    Change::Same => (theme.text, false),
                    Change::Added => (theme.added, true),
                    Change::Removed => (theme.removed, false),
                };
                canvas.text(&run.text, body.left + run.x, baseline, TextStyle { size, color, bold, align: Align::Left });
                if run.change == Change::Removed {
                    let y = baseline + (ascent + descent) / 2.0;
                    let line = Rect::new(body.left + run.x, y - self.dp(0.75), body.left + run.x + run.width, y + self.dp(0.75));
                    canvas.round_rect(line, 0.0, theme.removed);
                }
            }
        }
        canvas.unclip();

        let pressed = |target: Target| self.presses.iter().any(|p| p.target == target);
        let close = self.grammar_close_rect();
        let close_fill = if pressed(Target::GrammarClose) { theme.function_key_pressed } else { theme.function_key };
        self.key_background(canvas, close, close_fill);
        self.centered_text(canvas, "Close", close, self.sp(15.0), theme.text, false);
        let replace = self.grammar_replace_rect();
        let (fill, color) = match (panel.result.is_some(), pressed(Target::GrammarReplace)) {
            (true, false) => (theme.accent, theme.accent_text),
            (true, true) => (theme.function_key_pressed, theme.text),
            (false, _) => (theme.function_key, theme.text_muted),
        };
        self.key_background(canvas, replace, fill);
        self.centered_text(canvas, "Replace", replace, self.sp(15.0), color, true);
    }

    // ------------------------------------------------------------- drawing ----

    /// A rounded key with a thin darker edge at the bottom, like a physical key cap.
    fn key_background(&self, canvas: &mut dyn Canvas, rect: Rect, fill: u32) {
        let radius = self.dp(7.0);
        canvas.round_rect(rect, radius, self.theme.key_shadow);
        canvas.round_rect(Rect { bottom: rect.bottom - self.dp(1.5), ..rect }, radius, fill);
    }

    fn centered_text(&self, canvas: &mut dyn Canvas, text: &str, rect: Rect, size: f32, color: u32, bold: bool) {
        let (ascent, descent) = canvas.metrics(size, bold);
        let baseline = rect.center_y() - (ascent + descent) / 2.0;
        canvas.text(text, rect.center_x(), baseline, TextStyle { size, color, bold, align: Align::Center });
    }

    /// Shortens text with a leading or trailing ellipsis so it fits `width`.
    fn fit(&self, canvas: &mut dyn Canvas, text: &str, size: f32, bold: bool, width: f32, from_start: bool) -> String {
        if canvas.measure(text, size, bold) <= width {
            return text.to_string();
        }
        let mut chars: Vec<char> = text.chars().collect();
        while !chars.is_empty() {
            if from_start {
                chars.remove(0);
            } else {
                chars.pop();
            }
            let candidate: String = chars.iter().collect();
            let candidate = if from_start { format!("…{candidate}") } else { format!("{candidate}…") };
            if canvas.measure(&candidate, size, bold) <= width {
                return candidate;
            }
        }
        "…".into()
    }

    pub fn draw(&mut self, canvas: &mut dyn Canvas) {
        self.refresh_candidates();
        self.apply_grammar_updates();
        let theme = self.theme;
        canvas.fill(theme.background);
        let bar_text = self.sp(15.0);

        // Top bar: language, typed text, candidates, expand.
        let mode = self.mode_button();
        canvas.round_rect(mode, self.dp(18.0), theme.candidate_highlight);
        self.centered_text(canvas, self.mode.button_label(), mode, bar_text, theme.accent, true);
        let has_candidates = !self.candidates.is_empty();
        if has_candidates {
            self.centered_text(canvas, "‹", self.prev_button(), bar_text, theme.text_muted, false);
            self.centered_text(canvas, "›", self.next_button(), bar_text, theme.text_muted, false);
        }
        let expand = if self.suggestions_expanded { "▴" } else { "▾" };
        self.centered_text(canvas, expand, self.expand_button(), bar_text, theme.text_muted, false);

        let label_width = self.composing_label_width(canvas);
        if label_width > 0.0 {
            let size = self.sp(13.0);
            let text = self.fit(canvas, &self.composing_preview(), size, false, label_width, true);
            let (ascent, descent) = canvas.metrics(size, false);
            let x = self.prev_button().right + self.dp(2.0);
            let baseline = self.bar_rect().center_y() - (ascent + descent) / 2.0;
            canvas.text(&text, x, baseline, TextStyle { size, color: theme.text_muted, bold: false, align: Align::Left });
        }
        let strip = self.strip_rect(label_width);
        let chips = self.chip_rects(canvas, strip);
        self.strip_content = chips.last().map(|r| r.right + self.dp(2.0) - strip.left).unwrap_or(0.0);
        canvas.clip(strip);
        for (i, chip) in chips.iter().enumerate() {
            let chip = chip.offset(-self.strip_scroll, 0.0);
            if chip.right < strip.left || chip.left > strip.right {
                continue;
            }
            let first = i == 0;
            if first {
                canvas.round_rect(chip, self.dp(18.0), theme.candidate_highlight);
            }
            let text = self.shown(&self.candidates[i]);
            self.centered_text(canvas, &text, chip, self.sp(17.0), theme.text, first);
        }
        canvas.unclip();
        if self.grammar_button_visible() {
            let button = self.grammar_button(canvas);
            canvas.round_rect(button, self.dp(18.0), theme.candidate_highlight);
            self.centered_text(canvas, GRAMMAR_BUTTON, button, self.sp(15.0), theme.accent, true);
        }

        if self.grammar.is_some() {
            self.draw_grammar(canvas);
        } else if self.suggestions_expanded {
            self.draw_panel(canvas);
        } else {
            self.draw_keys(canvas);
        }

        if let Some((rect, label)) = &self.preview {
            self.key_background(canvas, *rect, theme.popup);
            self.centered_text(canvas, label, *rect, self.sp(28.0), theme.text, false);
        }
    }

    fn draw_keys(&self, canvas: &mut dyn Canvas) {
        let theme = self.theme;
        for (i, key) in self.keys.iter().enumerate() {
            let pressed = self.pressed(i);
            let function = is_function_key(key.key);
            let enter = key.key == KEY_ENTER;
            let fill = match (enter, function, pressed) {
                (true, _, true) | (false, true, true) => theme.function_key_pressed,
                (true, _, false) => theme.accent,
                (false, true, false) => theme.function_key,
                (false, false, true) => theme.key_pressed,
                (false, false, false) => theme.key,
            };
            self.key_background(canvas, key.rect, fill);
            let (size, color) = if key.key == KEY_SPACE {
                (self.sp(13.0), theme.text_muted)
            } else if enter {
                (self.sp(26.0), theme.accent_text)
            } else if function {
                let small = matches!(key.key, KEY_SYMBOLS | KEY_LETTERS | KEY_MORE_SYMBOLS);
                (self.sp(if small { 15.0 } else { 20.0 }), if key.key == KEY_SHIFT && self.shift_on { theme.accent } else { theme.text })
            } else {
                (self.sp(21.0), theme.text)
            };
            let cap = Rect { bottom: key.rect.bottom - self.dp(1.5), ..key.rect };
            let label = self.label_for(key.key);
            let label = self.fit(canvas, &label, size, false, cap.width() - self.dp(4.0), false);
            self.centered_text(canvas, &label, cap, size, color, false);
            if let Some(hint) = key.hint {
                let size = self.sp(10.0);
                let style = TextStyle { size, color: theme.text_muted, bold: false, align: Align::Right };
                canvas.text(hint, key.rect.right - self.dp(6.0), key.rect.top + size + self.dp(4.0), style);
            }
        }
    }

    fn draw_panel(&self, canvas: &mut dyn Canvas) {
        let theme = self.theme;
        let grid = self.panel_grid();
        canvas.clip(grid);
        for i in 0..self.candidates.len() {
            let cell = self.panel_cell(i).offset(0.0, -self.panel_scroll);
            if cell.bottom < grid.top || cell.top > grid.bottom {
                continue;
            }
            self.key_background(canvas, cell, theme.key);
            let size = self.sp(17.0);
            let text = self.fit(canvas, &self.shown(&self.candidates[i]), size, false, cell.width() - self.dp(8.0), false);
            self.centered_text(canvas, &text, cell, size, theme.text, false);
        }
        canvas.unclip();
        // A vertical slider that scrolls the list, beside it.
        let track = self.slider_rect();
        let max = self.panel_max_scroll();
        let line = Rect::new(
            track.center_x() - self.dp(2.0),
            track.top + self.dp(12.0),
            track.center_x() + self.dp(2.0),
            track.bottom - self.dp(12.0),
        );
        canvas.round_rect(line, self.dp(2.0), theme.key_shadow);
        let fraction = if max > 0.0 { self.panel_scroll / max } else { 0.0 };
        let y = line.top + fraction * line.height();
        let thumb = Rect::new(track.center_x() - self.dp(9.0), y - self.dp(9.0), track.center_x() + self.dp(9.0), y + self.dp(9.0));
        canvas.round_rect(thumb, self.dp(9.0), if max > 0.0 { theme.accent } else { theme.text_muted });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[derive(Default)]
    struct FakeEditor {
        text: String,
        composing: String,
        actions: Vec<i32>,
        info: Option<(i32, i32)>,
        selected: Option<String>,
    }

    impl Editor for FakeEditor {
        fn connected(&self) -> bool {
            true
        }
        fn commit_text(&mut self, text: &str) {
            self.composing.clear();
            // Committing replaces the selection, if any.
            if let Some(selected) = self.selected.take() {
                if let Some(at) = self.text.rfind(&selected) {
                    self.text.replace_range(at..at + selected.len(), text);
                    return;
                }
            }
            self.text.push_str(text);
        }
        fn set_composing_text(&mut self, text: &str) {
            self.composing = text.to_string();
        }
        fn finish_composing_text(&mut self) {
            let composing = std::mem::take(&mut self.composing);
            self.text.push_str(&composing);
        }
        fn delete_before(&mut self, count: i32) {
            for _ in 0..count {
                self.text.pop();
            }
        }
        fn selected_text(&mut self) -> Option<String> {
            self.selected.clone()
        }
        fn editor_info(&mut self) -> Option<(i32, i32)> {
            self.info
        }
        fn perform_editor_action(&mut self, action: i32) {
            self.actions.push(action);
        }
        fn send_enter_key(&mut self) {
            self.text.push('\n');
        }
    }

    #[derive(Default)]
    struct FakeHost {
        timers: Vec<i64>,
    }

    impl Host for FakeHost {
        fn haptic(&mut self) {}
        fn schedule_timer(&mut self, delay_ms: i64) {
            self.timers.push(delay_ms);
        }
        fn cancel_timer(&mut self) {}
        fn invalidate(&mut self) {}
    }

    /// Every character is 10px wide.
    struct FakeCanvas;

    impl Canvas for FakeCanvas {
        fn fill(&mut self, _: u32) {}
        fn round_rect(&mut self, _: Rect, _: f32, _: u32) {}
        fn text(&mut self, _: &str, _: f32, _: f32, _: TextStyle) {}
        fn measure(&mut self, text: &str, _: f32, _: bool) -> f32 {
            text.chars().count() as f32 * 10.0
        }
        fn metrics(&mut self, size: f32, _: bool) -> (f32, f32) {
            (-size * 0.8, size * 0.2)
        }
        fn clip(&mut self, _: Rect) {}
        fn unclip(&mut self) {}
    }

    /// Answers every check at once with `corrected`, and records what it was asked.
    struct FakeProofreader {
        corrected: String,
        asked: Arc<Mutex<Vec<(String, Language)>>>,
    }

    impl Proofreader for FakeProofreader {
        fn check(&mut self, text: &str, language: Language, on_update: Box<dyn Fn(Update) + Send>) {
            self.asked.lock().unwrap().push((text.to_string(), language));
            on_update(Update::Status("Checking…".into()));
            on_update(Update::Done(self.corrected.clone()));
        }
        fn cancel(&mut self) {}
        fn release(&mut self) {}
    }

    fn tap(kb: &mut Keyboard, rect: Rect, editor: &mut FakeEditor) {
        let mut host = FakeHost::default();
        let (x, y) = (rect.center_x(), rect.center_y());
        kb.on_touch(Touch::Down { id: 0, x, y }, editor, &mut host, &mut FakeCanvas);
        kb.on_touch(Touch::Up { id: 0, x, y }, editor, &mut host, &mut FakeCanvas);
    }

    fn keyboard() -> Keyboard {
        let assets = Arc::new(engines::DirAssets(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets")));
        let engines = Engines::new(assets, Box::new(|| {}));
        // Load synchronously so the tests do not race the background thread.
        engines.latin(Mode::English, true);
        engines.pinyin(true);
        let mut keyboard = Keyboard::new(engines, Settings::default(), 2.0, 2.0);
        keyboard.set_width(720.0);
        keyboard
    }

    fn tap_key(kb: &mut Keyboard, key: &str, editor: &mut FakeEditor) {
        let index = kb.keys.iter().position(|k| k.key == key).unwrap_or_else(|| panic!("no key {key}"));
        let r = kb.keys[index].rect;
        let mut host = FakeHost::default();
        kb.on_touch(Touch::Down { id: 0, x: r.center_x(), y: r.center_y() }, editor, &mut host, &mut FakeCanvas);
        kb.on_touch(Touch::Up { id: 0, x: r.center_x(), y: r.center_y() }, editor, &mut host, &mut FakeCanvas);
    }

    fn type_text(kb: &mut Keyboard, text: &str, editor: &mut FakeEditor) {
        for ch in text.chars() {
            let key = match ch {
                ' ' => KEY_SPACE.to_string(),
                c => c.to_string(),
            };
            tap_key(kb, &key, editor);
        }
    }

    #[test]
    fn autocorrects_on_space() {
        let mut kb = keyboard();
        let mut editor = FakeEditor::default();
        type_text(&mut kb, "teh ", &mut editor);
        assert_eq!(editor.text, "the ");
        type_text(&mut kb, "dont", &mut editor);
        assert_eq!(editor.composing, "dont");
        tap_key(&mut kb, KEY_SPACE, &mut editor);
        assert_eq!(editor.text, "the don't ");
    }

    #[test]
    fn shift_capitalises_and_backspace_edits_composition() {
        let mut kb = keyboard();
        let mut editor = FakeEditor::default();
        tap_key(&mut kb, KEY_SHIFT, &mut editor);
        type_text(&mut kb, "h", &mut editor);
        tap_key(&mut kb, KEY_SHIFT, &mut editor);
        type_text(&mut kb, "ii", &mut editor);
        tap_key(&mut kb, KEY_BACKSPACE, &mut editor);
        assert_eq!(editor.composing, "Hi");
        tap_key(&mut kb, KEY_ENTER, &mut editor);
        assert_eq!(editor.text, "Hi\n");
    }

    #[test]
    fn enter_performs_editor_action() {
        let mut kb = keyboard();
        let mut editor = FakeEditor { info: Some((3, 1)), ..Default::default() }; // IME_ACTION_SEARCH
        tap_key(&mut kb, KEY_ENTER, &mut editor);
        assert_eq!(editor.actions, vec![3]);
    }

    #[test]
    fn pinyin_space_commits_top_candidate() {
        let mut kb = keyboard();
        let mut editor = FakeEditor::default();
        let mut host = FakeHost::default();
        kb.set_mode(Mode::Simplified, &mut editor, &mut host);
        type_text(&mut kb, "woaini", &mut editor);
        assert_eq!(editor.text, "", "pinyin is not shown in the editor while composing");
        tap_key(&mut kb, KEY_SPACE, &mut editor);
        assert_eq!(editor.text, "我爱你");
        tap_key(&mut kb, ",", &mut editor);
        assert_eq!(editor.text, "我爱你，");
    }

    #[test]
    fn tapping_a_candidate_commits_it() {
        let mut kb = keyboard();
        let mut editor = FakeEditor::default();
        type_text(&mut kb, "tomo", &mut editor);
        kb.draw(&mut FakeCanvas);
        let index = kb.candidates.iter().position(|c| c == "tomorrow").expect("tomorrow suggested");
        let strip = kb.strip_rect(0.0);
        let chip = kb.chip_rects(&mut FakeCanvas, strip)[index];
        if chip.right > strip.right {
            kb.strip_scroll = chip.left - strip.left;
        }
        let (x, y) = (chip.center_x() - kb.strip_scroll, chip.center_y());
        let mut host = FakeHost::default();
        kb.on_touch(Touch::Down { id: 0, x, y }, &mut editor, &mut host, &mut FakeCanvas);
        kb.on_touch(Touch::Up { id: 0, x, y }, &mut editor, &mut host, &mut FakeCanvas);
        assert_eq!(editor.text, "tomorrow");
    }

    #[test]
    fn long_press_types_the_digit_and_backspace_repeats() {
        let mut kb = keyboard();
        let mut editor = FakeEditor::default();
        let mut host = FakeHost::default();
        let q = kb.keys.iter().find(|k| k.key == "q").unwrap().rect;
        kb.on_touch(Touch::Down { id: 0, x: q.center_x(), y: q.center_y() }, &mut editor, &mut host, &mut FakeCanvas);
        assert_eq!(host.timers, vec![LONG_PRESS_MS]);
        kb.on_timer(&mut editor, &mut host);
        kb.on_touch(Touch::Up { id: 0, x: q.center_x(), y: q.center_y() }, &mut editor, &mut host, &mut FakeCanvas);
        assert_eq!(editor.text, "1");

        let back = kb.keys.iter().find(|k| k.key == KEY_BACKSPACE).unwrap().rect;
        editor.text = "abc".into();
        kb.on_touch(Touch::Down { id: 1, x: back.center_x(), y: back.center_y() }, &mut editor, &mut host, &mut FakeCanvas);
        kb.on_timer(&mut editor, &mut host);
        kb.on_timer(&mut editor, &mut host);
        kb.on_touch(Touch::Up { id: 1, x: back.center_x(), y: back.center_y() }, &mut editor, &mut host, &mut FakeCanvas);
        assert_eq!(editor.text, "a", "two repeats and no extra delete on release");
    }

    #[test]
    fn mode_cycle_skips_disabled_languages() {
        let mut kb = keyboard();
        kb.apply_settings(Settings { languages: [false, false, true, false, true, false], ..Default::default() });
        let mut editor = FakeEditor::default();
        let mut host = FakeHost::default();
        kb.cycle_mode(&mut editor, &mut host);
        assert_eq!(kb.mode(), Mode::Japanese);
        kb.cycle_mode(&mut editor, &mut host);
        assert_eq!(kb.mode(), Mode::English);
    }

    #[test]
    fn grammar_check_replaces_the_selection() {
        let mut kb = keyboard();
        let asked = Arc::new(Mutex::new(Vec::new()));
        kb.set_proofreader(Some(Box::new(FakeProofreader { corrected: "He goes home.".into(), asked: Arc::clone(&asked) })));
        let mut editor = FakeEditor { text: "Today he go home. Bye".into(), ..Default::default() };
        let mut host = FakeHost::default();
        editor.selected = Some("he go home.".into());
        kb.on_update_selection(&mut editor, &mut host, 6, 17, -1);
        assert!(!kb.grammar_button_visible(), "off by default");

        kb.apply_settings(Settings { grammar_button: true, ..Default::default() });
        kb.draw(&mut FakeCanvas);
        assert!(kb.grammar_button_visible());
        kb.on_update_selection(&mut editor, &mut host, 17, 17, -1);
        assert!(!kb.grammar_button_visible(), "only while text is selected");
        kb.on_update_selection(&mut editor, &mut host, 6, 17, -1);

        let button = kb.grammar_button(&mut FakeCanvas);
        tap(&mut kb, button, &mut editor);
        assert_eq!(*asked.lock().unwrap(), vec![("he go home.".to_string(), Language::English)]);
        kb.draw(&mut FakeCanvas);
        let panel = kb.grammar.as_ref().expect("panel open");
        assert_eq!(panel.result.as_deref(), Some("He goes home."));
        assert!(panel.body.iter().any(|(change, text)| *change == Change::Added && text == "goes"));

        let replace = kb.grammar_replace_rect();
        tap(&mut kb, replace, &mut editor);
        assert_eq!(editor.text, "Today He goes home. Bye");
        assert!(kb.grammar.is_none(), "replacing closes the panel");
    }

    #[test]
    fn grammar_check_refuses_a_changed_selection() {
        let mut kb = keyboard();
        let asked = Arc::new(Mutex::new(Vec::new()));
        kb.set_proofreader(Some(Box::new(FakeProofreader { corrected: "我去图书馆了。".into(), asked: Arc::clone(&asked) })));
        kb.apply_settings(Settings { grammar_button: true, ..Default::default() });
        let mut editor =
            FakeEditor { text: "我去了图书馆了。".into(), selected: Some("我去了图书馆了。".into()), ..Default::default() };
        let mut host = FakeHost::default();
        kb.on_update_selection(&mut editor, &mut host, 0, 8, -1);
        kb.draw(&mut FakeCanvas);
        let button = kb.grammar_button(&mut FakeCanvas);
        tap(&mut kb, button, &mut editor);
        assert_eq!(asked.lock().unwrap()[0].1, Language::Mandarin);
        editor.selected = Some("图书馆".into());
        kb.draw(&mut FakeCanvas);
        let replace = kb.grammar_replace_rect();
        tap(&mut kb, replace, &mut editor);
        assert_eq!(editor.text, "我去了图书馆了。", "nothing replaced");
        assert!(kb.grammar.as_ref().unwrap().title.contains("selection changed"));
        let close = kb.grammar_close_rect();
        tap(&mut kb, close, &mut editor);
        assert!(kb.grammar.is_none());
    }

    #[test]
    fn symbols_pages() {
        let mut kb = keyboard();
        let mut editor = FakeEditor::default();
        tap_key(&mut kb, KEY_SYMBOLS, &mut editor);
        tap_key(&mut kb, "@", &mut editor);
        tap_key(&mut kb, KEY_MORE_SYMBOLS, &mut editor);
        tap_key(&mut kb, "€", &mut editor);
        tap_key(&mut kb, KEY_LETTERS, &mut editor);
        tap_key(&mut kb, "a", &mut editor);
        assert_eq!(editor.text, "@€");
        assert_eq!(editor.composing, "a");
    }
}
