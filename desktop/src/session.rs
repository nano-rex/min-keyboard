//! What a key does: composition, candidates, commits and hotkeys. Independent of X11 and
//! Wayland, which only translate their key events into keysyms and apply the [`Outcome`].

use crate::core::{preedit_for, Core, Language};
use crate::keysym::{self, *};

/// What a key press did.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// Whether the key was used; otherwise the application receives it, after `commit`.
    pub handled: bool,
    /// Text to insert into the application.
    pub commit: String,
    /// The input method was switched on or off, or to another language.
    pub mode_changed: bool,
}

impl Outcome {
    fn handled(commit: String) -> Outcome {
        Outcome { handled: true, commit, mode_changed: false }
    }

    fn passed(commit: String) -> Outcome {
        Outcome { handled: false, commit, mode_changed: false }
    }
}

/// One composition: the keys typed so far and the candidates for them.
#[derive(Default)]
pub struct Session {
    input: String,
    candidates: Vec<String>,
    cursor: usize,
    /// The user moved through the candidates, so Space and digits pick from them. Conversion
    /// languages are always in this state; word languages only after Tab or an arrow key.
    engaged: bool,
    double_quote_open: bool,
    single_quote_open: bool,
}

/// What the candidate window shows.
#[derive(Debug, Default, PartialEq, Clone)]
pub struct View {
    pub preedit: String,
    pub candidates: Vec<String>,
    /// The index of the first candidate shown, and the page size.
    pub page_start: usize,
    pub highlight: Option<usize>,
    pub has_previous_page: bool,
    pub has_next_page: bool,
    pub labels: bool,
}

impl Session {
    pub fn is_composing(&self) -> bool {
        !self.input.is_empty()
    }

    pub fn clear(&mut self) {
        self.input.clear();
        self.candidates.clear();
        self.cursor = 0;
        self.engaged = false;
    }

    /// The composition as it is shown; committed as it is when focus moves away.
    pub fn preedit(&self, core: &Core) -> String {
        if self.input.is_empty() {
            String::new()
        } else {
            preedit_for(core.language, &self.input)
        }
    }

    /// The composition as it is committed when it is not converted: Japanese finishes a
    /// trailing "n" as ん.
    fn final_text(&self, core: &Core) -> String {
        match core.language {
            Language::Japanese if !self.input.is_empty() => engines::japanese::to_hiragana(&self.input, true),
            _ => self.preedit(core),
        }
    }

    /// Ends the composition, returning the text to commit.
    pub fn finish(&mut self, core: &Core) -> String {
        let text = self.final_text(core);
        self.clear();
        text
    }

    pub fn view(&self, core: &Core) -> View {
        let page = core.config.page_size;
        let start = self.cursor / page * page;
        let end = (start + page).min(self.candidates.len());
        let only_typed = self.candidates.len() == 1 && self.candidates[0] == self.preedit(core);
        let picks = self.digits_pick(core);
        View {
            preedit: self.preedit(core),
            candidates: if only_typed || self.input.is_empty() { Vec::new() } else { self.candidates[start..end].to_vec() },
            page_start: start,
            highlight: (self.engaged || !core.language.is_word_language()).then_some(self.cursor - start),
            has_previous_page: start > 0,
            has_next_page: end < self.candidates.len(),
            labels: picks,
        }
    }

    /// Picks the candidate at `index` in the page shown, as a click does.
    pub fn click(&mut self, core: &Core, index: usize) -> Option<String> {
        let page = core.config.page_size;
        let absolute = self.cursor / page * page + index;
        let text = self.candidates.get(absolute)?.clone();
        self.clear();
        Some(text)
    }

    fn refresh(&mut self, core: &mut Core) {
        self.candidates = core.dictionaries.candidates(core.language, &self.input);
        self.cursor = 0;
        self.engaged = false;
    }

    fn selected(&self, core: &Core) -> String {
        if self.engaged || !core.language.is_word_language() {
            if let Some(candidate) = self.candidates.get(self.cursor) {
                return candidate.clone();
            }
        }
        self.preedit(core)
    }

    fn digits_pick(&self, core: &Core) -> bool {
        match core.language {
            Language::English | Language::Malay | Language::Korean | Language::Zhuyin => self.engaged,
            _ => true,
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.candidates.is_empty() {
            return;
        }
        let last = self.candidates.len() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
        self.engaged = true;
    }

    fn move_page(&mut self, core: &Core, pages: isize) {
        let page = core.config.page_size as isize;
        let start = self.cursor as isize / page * page;
        let target = start + pages * page;
        if target >= 0 && (target as usize) < self.candidates.len() {
            self.cursor = target as usize;
        }
        self.engaged = !self.candidates.is_empty();
    }

    /// Turns a typed character into what the composition stores, if it composes in `language`.
    fn composing_char(&self, language: Language, c: char) -> Option<char> {
        let starting = self.input.is_empty();
        match language {
            Language::English | Language::Malay => (c.is_alphabetic() || (!starting && c == '\'')).then_some(c),
            Language::Simplified | Language::Traditional => (c.is_ascii_lowercase() || (!starting && c == '\'')).then_some(c),
            Language::Japanese => (c.is_ascii_lowercase() || c == '-' || (!starting && c == '\'')).then_some(c),
            Language::KoreanRomanized => c.is_ascii_lowercase().then_some(c),
            Language::Korean => dubeolsik(c),
            Language::Zhuyin => zhuyin(c),
        }
    }

    fn punctuation(&mut self, core: &Core, c: char) -> Option<String> {
        if !core.config.fullwidth_punctuation {
            return None;
        }
        let quote = |open: &mut bool, left: &str, right: &str| {
            *open = !*open;
            Some(if *open { left } else { right }.to_string())
        };
        let mapped = match (core.language, c) {
            (Language::Japanese, ',') => "、",
            (Language::Japanese, '.') => "。",
            (Language::Japanese, '[') => "「",
            (Language::Japanese, ']') => "」",
            (Language::Japanese, '/') => "・",
            (Language::Japanese, '~') => "〜",
            (Language::Simplified | Language::Traditional, ',') => "，",
            (Language::Simplified | Language::Traditional, '.') => "。",
            (Language::Simplified | Language::Traditional | Language::Zhuyin, '\\') => "、",
            (Language::Simplified, '[') => "【",
            (Language::Simplified, ']') => "】",
            (Language::Traditional, '[') => "「",
            (Language::Traditional, ']') => "」",
            (Language::Simplified | Language::Traditional, '<') => "《",
            (Language::Simplified | Language::Traditional, '>') => "》",
            (Language::Simplified, '$') => "￥",
            (Language::Simplified | Language::Traditional, '^') => "……",
            (Language::Simplified | Language::Traditional, '_') => "——",
            (Language::Simplified | Language::Traditional | Language::Zhuyin, '"') => {
                return quote(&mut self.double_quote_open, "“", "”");
            }
            (Language::Simplified | Language::Traditional, '\'') => return quote(&mut self.single_quote_open, "‘", "’"),
            // The Zhuyin keyboard uses , . ; / - for symbols, so its punctuation is on Shift.
            (Language::Zhuyin, '<') => "，",
            (Language::Zhuyin, '>') => "。",
            (Language::Zhuyin, '{') => "「",
            (Language::Zhuyin, '}') => "」",
            (Language::Simplified | Language::Traditional | Language::Japanese | Language::Zhuyin, '?') => "？",
            (Language::Simplified | Language::Traditional | Language::Japanese | Language::Zhuyin, '!') => "！",
            (Language::Simplified | Language::Traditional | Language::Japanese | Language::Zhuyin, ':') => "：",
            (Language::Simplified | Language::Traditional | Language::Japanese, ';') => "；",
            (Language::Simplified | Language::Traditional | Language::Japanese | Language::Zhuyin, '(') => "（",
            (Language::Simplified | Language::Traditional | Language::Japanese | Language::Zhuyin, ')') => "）",
            (Language::Simplified | Language::Traditional | Language::Zhuyin, '~') => "～",
            _ => return None,
        };
        Some(mapped.to_string())
    }

    fn switch_to(&mut self, core: &mut Core, language: Option<Language>, enabled: bool) -> Outcome {
        let commit = self.finish(core);
        core.enabled = enabled;
        if let Some(language) = language {
            core.language = language;
        }
        Outcome { handled: true, commit, mode_changed: true }
    }

    fn hotkey(&mut self, core: &mut Core, sym: u32, state: u32) -> Option<Outcome> {
        let config = &core.config;
        if config.toggle.is_some_and(|h| h.matches(sym, state)) {
            let enabled = !core.enabled;
            return Some(self.switch_to(core, None, enabled));
        }
        if config.cycle.is_some_and(|h| h.matches(sym, state)) {
            let languages = &config.languages;
            let next = if core.enabled {
                let at = languages.iter().position(|&l| l == core.language).unwrap_or(0);
                languages[(at + 1) % languages.len()]
            } else {
                core.language
            };
            return Some(self.switch_to(core, Some(next), true));
        }
        let language = config.language_hotkeys.iter().find(|(_, h)| h.matches(sym, state)).map(|&(l, _)| l)?;
        Some(self.switch_to(core, Some(language), true))
    }

    /// Handles a key press. `state` holds the X modifier masks.
    pub fn key(&mut self, core: &mut Core, sym: u32, state: u32) -> Outcome {
        if keysym::is_modifier(sym) {
            return Outcome::default();
        }
        if let Some(outcome) = self.hotkey(core, sym, state) {
            return outcome;
        }
        if !core.enabled {
            return Outcome::default();
        }
        let language = core.language;
        let word = language.is_word_language();
        let c = keysym::to_char(sym);

        if state & COMMAND_MODIFIERS != 0 {
            return Outcome::passed(self.finish(core));
        }

        if self.input.is_empty() {
            if let Some(c) = c {
                if let Some(stored) = self.composing_char(language, c) {
                    self.input.push(stored);
                    self.refresh(core);
                    return Outcome::handled(String::new());
                }
                if let Some(text) = self.punctuation(core, c) {
                    return Outcome::handled(text);
                }
            }
            return Outcome::default();
        }

        match sym {
            ESCAPE => {
                self.clear();
                return Outcome::handled(String::new());
            }
            BACKSPACE => {
                self.input.pop();
                if self.input.is_empty() {
                    self.clear();
                } else {
                    self.refresh(core);
                }
                return Outcome::handled(String::new());
            }
            RETURN | KP_ENTER => {
                if word {
                    if self.engaged {
                        let text = self.selected(core);
                        self.clear();
                        return Outcome::handled(text);
                    }
                    return Outcome::passed(self.finish(core));
                }
                let text = if language == Language::Zhuyin { self.selected(core) } else { self.final_text(core) };
                self.clear();
                return Outcome::handled(text);
            }
            TAB | DOWN => {
                self.move_cursor(1);
                return Outcome::handled(String::new());
            }
            ISO_LEFT_TAB | UP => {
                self.move_cursor(-1);
                return Outcome::handled(String::new());
            }
            RIGHT | LEFT if !word => {
                self.move_cursor(if sym == RIGHT { 1 } else { -1 });
                return Outcome::handled(String::new());
            }
            PAGE_DOWN | PAGE_UP => {
                self.move_page(core, if sym == PAGE_DOWN { 1 } else { -1 });
                return Outcome::handled(String::new());
            }
            _ => {}
        }

        let Some(c) = c else {
            // Home, End, Delete, F keys and the like: keep what was typed, then let them through.
            return Outcome::passed(self.finish(core));
        };

        if c == ' ' {
            let text = self.selected(core);
            self.clear();
            // Word languages and romanized Korean separate words with the space itself.
            return if word || language == Language::KoreanRomanized { Outcome::passed(text) } else { Outcome::handled(text) };
        }

        if let Some(digit) = c.to_digit(10).filter(|d| (1..=9).contains(d)) {
            if self.digits_pick(core) {
                let page = core.config.page_size;
                let index = self.cursor / page * page + digit as usize - 1;
                if digit as usize <= page {
                    if let Some(text) = self.candidates.get(index).cloned() {
                        self.clear();
                        return Outcome::handled(text);
                    }
                }
                return Outcome::handled(String::new());
            }
        }

        if matches!(language, Language::Simplified | Language::Traditional) && (c == '-' || c == '=') {
            self.move_page(core, if c == '=' { 1 } else { -1 });
            return Outcome::handled(String::new());
        }

        if let Some(stored) = self.composing_char(language, c) {
            self.input.push(stored);
            self.refresh(core);
            return Outcome::handled(String::new());
        }

        // Punctuation and other symbols end the word: conversion languages take the highlighted
        // candidate, word languages keep what was typed.
        let text = self.selected(core);
        self.clear();
        match self.punctuation(core, c) {
            Some(mark) => Outcome::handled(text + &mark),
            None => Outcome::passed(text),
        }
    }
}

/// The two-set (Dubeolsik) Korean layout on a US keyboard.
pub fn dubeolsik(c: char) -> Option<char> {
    let jamo = match c {
        'Q' => 'ㅃ',
        'W' => 'ㅉ',
        'E' => 'ㄸ',
        'R' => 'ㄲ',
        'T' => 'ㅆ',
        'O' => 'ㅒ',
        'P' => 'ㅖ',
        _ => match c.to_ascii_lowercase() {
            'q' => 'ㅂ',
            'w' => 'ㅈ',
            'e' => 'ㄷ',
            'r' => 'ㄱ',
            't' => 'ㅅ',
            'y' => 'ㅛ',
            'u' => 'ㅕ',
            'i' => 'ㅑ',
            'o' => 'ㅐ',
            'p' => 'ㅔ',
            'a' => 'ㅁ',
            's' => 'ㄴ',
            'd' => 'ㅇ',
            'f' => 'ㄹ',
            'g' => 'ㅎ',
            'h' => 'ㅗ',
            'j' => 'ㅓ',
            'k' => 'ㅏ',
            'l' => 'ㅣ',
            'z' => 'ㅋ',
            'x' => 'ㅌ',
            'c' => 'ㅊ',
            'v' => 'ㅍ',
            'b' => 'ㅠ',
            'n' => 'ㅜ',
            'm' => 'ㅡ',
            _ => return None,
        },
    };
    Some(jamo)
}

/// The standard (Dàqiān) Zhuyin layout on a US keyboard, with the tone keys 6 3 4 7.
pub fn zhuyin(c: char) -> Option<char> {
    const KEYS: &str = "1qaz2wsxedcrfv5tgbyhnujm8ik,9ol.0p;/-6347";
    const SYMBOLS: &str = "ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙㄧㄨㄩㄚㄛㄜㄝㄞㄟㄠㄡㄢㄣㄤㄥㄦˊˇˋ˙";
    let at = KEYS.chars().position(|k| k == c)?;
    SYMBOLS.chars().nth(at)
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::*;
    use crate::core::{Config, Dictionaries};

    /// One loaded set of dictionaries for all tests; each test sets its own language.
    fn core() -> std::sync::MutexGuard<'static, Core> {
        static CORE: OnceLock<Mutex<Core>> = OnceLock::new();
        let guard = CORE
            .get_or_init(|| {
                let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets");
                Mutex::new(Core { config: Config::default(), enabled: true, language: Language::English, dictionaries: Dictionaries::new(assets) })
            })
            .lock();
        let mut core = guard.unwrap_or_else(|e| e.into_inner());
        core.config = Config::default();
        core.enabled = true;
        core
    }

    fn type_text(session: &mut Session, core: &mut Core, text: &str) -> String {
        let mut committed = String::new();
        for c in text.chars() {
            let outcome = session.key(core, keysym::from_char(c), if c.is_ascii_uppercase() { SHIFT } else { 0 });
            committed += &outcome.commit;
            if !outcome.handled {
                committed.push(c);
            }
        }
        committed
    }

    #[test]
    fn pinyin() {
        let mut core = core();
        core.language = Language::Simplified;
        let mut s = Session::default();
        assert_eq!(type_text(&mut s, &mut core, "nihao"), "");
        assert_eq!(s.preedit(&core), "nihao");
        assert_eq!(s.view(&core).candidates[0], "你好");
        assert_eq!(type_text(&mut s, &mut core, " "), "你好");
        assert!(!s.is_composing());
        type_text(&mut s, &mut core, "wo");
        let second = s.view(&core).candidates[1].clone();
        assert_eq!(type_text(&mut s, &mut core, "2"), second);
        type_text(&mut s, &mut core, "wo");
        let third = s.view(&core).candidates[2].clone();
        assert_eq!(s.click(&core, 2), Some(third));
        assert_eq!(type_text(&mut s, &mut core, "woaini,"), "我爱你，");
        assert_eq!(type_text(&mut s, &mut core, "abc"), "");
        assert_eq!(s.key(&mut core, RETURN, 0), Outcome::handled("abc".into()));
    }

    #[test]
    fn pinyin_quotes_and_letters() {
        let mut core = core();
        core.language = Language::Traditional;
        let mut s = Session::default();
        assert_eq!(type_text(&mut s, &mut core, "\""), "“");
        assert_eq!(type_text(&mut s, &mut core, "\""), "”");
        assert_eq!(type_text(&mut s, &mut core, "woaini "), "我愛你");
        // Capitals are not pinyin and go straight through.
        assert_eq!(type_text(&mut s, &mut core, "A"), "A");
        core.config.fullwidth_punctuation = false;
        assert_eq!(type_text(&mut s, &mut core, "."), ".");
    }

    #[test]
    fn english_never_autocorrects() {
        let mut core = core();
        core.language = Language::English;
        let mut s = Session::default();
        assert_eq!(type_text(&mut s, &mut core, "teh"), "");
        assert!(s.view(&core).candidates.iter().any(|c| c == "the"));
        assert_eq!(s.view(&core).highlight, None);
        assert_eq!(type_text(&mut s, &mut core, " "), "teh ");
        assert_eq!(type_text(&mut s, &mut core, "Hello, world2"), "Hello, world2");
        // Tab engages the suggestions; Space then takes the highlighted one.
        type_text(&mut s, &mut core, "wdym");
        let first = s.view(&core).candidates[0].clone();
        assert_eq!(s.key(&mut core, TAB, 0), Outcome::handled(String::new()));
        let second = s.view(&core).candidates[1].clone();
        assert_ne!(first, second);
        assert_eq!(type_text(&mut s, &mut core, " "), second + " ");
    }

    #[test]
    fn japanese() {
        let mut core = core();
        core.language = Language::Japanese;
        let mut s = Session::default();
        type_text(&mut s, &mut core, "nihon");
        assert_eq!(s.preedit(&core), "にほn");
        assert_eq!(s.key(&mut core, RETURN, 0).commit, "にほん");
        assert_eq!(type_text(&mut s, &mut core, "nihon "), "日本");
        type_text(&mut s, &mut core, "arigatou");
        let best = s.view(&core).candidates[0].clone();
        assert_eq!(type_text(&mut s, &mut core, "."), best + "。");
        assert_eq!(type_text(&mut s, &mut core, "-"), "");
        assert_eq!(s.preedit(&core), "ー");
    }

    #[test]
    fn korean_dubeolsik() {
        let mut core = core();
        core.language = Language::Korean;
        let mut s = Session::default();
        type_text(&mut s, &mut core, "gksrnr");
        assert_eq!(s.preedit(&core), "한국");
        assert_eq!(type_text(&mut s, &mut core, " "), "한국 ");
        type_text(&mut s, &mut core, "dkssud");
        assert_eq!(s.preedit(&core), "안녕");
        assert_eq!(s.key(&mut core, BACKSPACE, 0), Outcome::handled(String::new()));
        assert_eq!(s.preedit(&core), "안녀");
    }

    #[test]
    fn korean_romanized() {
        let mut core = core();
        core.language = Language::KoreanRomanized;
        let mut s = Session::default();
        assert_eq!(type_text(&mut s, &mut core, "gamsahamnida "), "감사합니다 ");
    }

    #[test]
    fn zhuyin() {
        let mut core = core();
        core.language = Language::Zhuyin;
        let mut s = Session::default();
        // ㄋㄧˇ ㄏㄠˇ
        type_text(&mut s, &mut core, "su3cl3");
        assert_eq!(s.preedit(&core), "ㄋㄧˇㄏㄠˇ");
        assert_eq!(type_text(&mut s, &mut core, " "), "你好");
        assert_eq!(type_text(&mut s, &mut core, ">"), "。");
    }

    #[test]
    fn hotkeys_and_modifiers() {
        let mut core = core();
        core.language = Language::Simplified;
        let mut s = Session::default();
        type_text(&mut s, &mut core, "ni");
        // A shortcut keeps what was typed and goes to the application.
        assert_eq!(s.key(&mut core, 'c' as u32, CONTROL), Outcome::passed("ni".into()));
        let toggled = s.key(&mut core, ' ' as u32, CONTROL);
        assert!(toggled.handled && toggled.mode_changed);
        assert!(!core.enabled);
        assert_eq!(s.key(&mut core, 'a' as u32, 0), Outcome::default());
        let japanese = s.key(&mut core, '4' as u32, CONTROL);
        assert!(japanese.mode_changed && core.enabled);
        assert_eq!(core.language, Language::Japanese);
        let cycled = s.key(&mut core, 0xffc9, CONTROL);
        assert!(cycled.mode_changed);
        assert_eq!(core.language, Language::Korean);
        assert_eq!(s.key(&mut core, 0xffe1, SHIFT), Outcome::default());
    }

    #[test]
    fn layouts() {
        assert_eq!(super::zhuyin('1'), Some('ㄅ'));
        assert_eq!(super::zhuyin('-'), Some('ㄦ'));
        assert_eq!(super::zhuyin('7'), Some('˙'));
        assert_eq!(dubeolsik('R'), Some('ㄲ'));
        assert_eq!(dubeolsik('K'), Some('ㅏ'));
    }
}
