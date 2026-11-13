//! What the backends share: the languages, the configuration file and the dictionaries.

use std::path::{Path, PathBuf};

use engines::{japanese, korean, pinyin, DirAssets, JapaneseEngine, KoreanEngine, LatinEngine, PinyinEngine, ScriptMode};

use crate::keysym;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Language {
    English,
    Malay,
    Simplified,
    Traditional,
    Zhuyin,
    Japanese,
    Korean,
    KoreanRomanized,
}

impl Language {
    pub const ALL: [Language; 8] = [
        Language::English,
        Language::Malay,
        Language::Simplified,
        Language::Traditional,
        Language::Zhuyin,
        Language::Japanese,
        Language::Korean,
        Language::KoreanRomanized,
    ];

    /// The name used in the config file.
    pub fn id(self) -> &'static str {
        match self {
            Language::English => "english",
            Language::Malay => "malay",
            Language::Simplified => "simplified",
            Language::Traditional => "traditional",
            Language::Zhuyin => "zhuyin",
            Language::Japanese => "japanese",
            Language::Korean => "korean",
            Language::KoreanRomanized => "korean_romanized",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Malay => "Bahasa Melayu",
            Language::Simplified => "简体中文 (拼音)",
            Language::Traditional => "繁體中文 (拼音)",
            Language::Zhuyin => "繁體中文 (注音)",
            Language::Japanese => "日本語",
            Language::Korean => "한국어 (두벌식)",
            Language::KoreanRomanized => "한국어 (romanized)",
        }
    }

    /// Word languages show the typed word as it is and only suggest; conversion languages turn
    /// what is typed into something else, so Space picks a candidate.
    pub fn is_word_language(self) -> bool {
        matches!(self, Language::English | Language::Malay | Language::Korean)
    }

    fn enabled_by_default(self) -> bool {
        !matches!(self, Language::Zhuyin | Language::KoreanRomanized)
    }

    fn default_hotkey(self) -> Option<&'static str> {
        match self {
            Language::English => Some("Control+1"),
            Language::Simplified => Some("Control+2"),
            Language::Traditional => Some("Control+3"),
            Language::Japanese => Some("Control+4"),
            Language::Korean => Some("Control+5"),
            Language::Malay => Some("Control+6"),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hotkey {
    pub sym: u32,
    pub modifiers: u32,
}

impl Hotkey {
    /// Parses "Control+Shift+F12" style names, as the Tkinter edition's config uses.
    pub fn parse(text: &str) -> Option<Hotkey> {
        let mut modifiers = 0;
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (key, mods) = parts.split_last()?;
        for name in mods {
            modifiers |= match name.to_ascii_lowercase().as_str() {
                "control" | "ctrl" => keysym::CONTROL,
                "shift" => keysym::SHIFT,
                "alt" | "mod1" => keysym::MOD1,
                "super" | "mod4" | "win" => keysym::MOD4,
                _ => return None,
            };
        }
        Some(Hotkey { sym: keysym::lowercase(keysym::from_config_name(key)?), modifiers })
    }

    pub fn matches(&self, sym: u32, state: u32) -> bool {
        keysym::lowercase(sym) == self.sym && state & (keysym::SHIFT | keysym::COMMAND_MODIFIERS) == self.modifiers
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub toggle: Option<Hotkey>,
    pub cycle: Option<Hotkey>,
    pub language_hotkeys: Vec<(Language, Hotkey)>,
    pub languages: Vec<Language>,
    pub fullwidth_punctuation: bool,
    pub page_size: usize,
    pub font_size: f32,
    pub dark: bool,
    pub fonts: Vec<PathBuf>,
    pub start_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config::parse("")
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("min-keyboard").join("config.ini")
}

impl Config {
    pub fn load() -> Config {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => Config::parse(&text),
            Err(_) => Config::default(),
        }
    }

    pub fn parse(text: &str) -> Config {
        let mut section = String::new();
        let mut values: Vec<(String, String, String)> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.trim().to_ascii_lowercase();
            } else if let Some((key, value)) = line.split_once('=').or_else(|| line.split_once(':')) {
                values.push((section.clone(), key.trim().to_ascii_lowercase(), value.trim().to_string()));
            }
        }
        let get = |section: &str, key: &str| values.iter().rev().find(|(s, k, _)| s == section && k == key).map(|(_, _, v)| v.as_str());
        let flag = |value: Option<&str>, default: bool| match value.map(str::to_ascii_lowercase).as_deref() {
            Some("true" | "yes" | "on" | "1") => true,
            Some("false" | "no" | "off" | "0") => false,
            _ => default,
        };
        let hotkey = |key: &str, default: Option<&str>| match get("hotkeys", key) {
            Some(value) if value.is_empty() || value.eq_ignore_ascii_case("none") => None,
            Some(value) => Hotkey::parse(value),
            None => default.and_then(Hotkey::parse),
        };
        let languages: Vec<Language> = Language::ALL.into_iter().filter(|l| flag(get("languages", l.id()), l.enabled_by_default())).collect();
        let language_hotkeys = languages.iter().filter_map(|&l| hotkey(l.id(), l.default_hotkey()).map(|h| (l, h))).collect();
        let fonts = get("ime", "fonts")
            .map(|list| list.split(',').map(str::trim).filter(|p| !p.is_empty()).map(expand_home).collect())
            .unwrap_or_default();
        Config {
            toggle: hotkey("toggle", Some("Control+space")),
            cycle: hotkey("mode", Some("Control+F12")),
            language_hotkeys,
            languages: if languages.is_empty() { vec![Language::English] } else { languages },
            fullwidth_punctuation: !get("ime", "punctuation").is_some_and(|v| v.eq_ignore_ascii_case("ascii")),
            page_size: get("ime", "page_size").and_then(|v| v.parse().ok()).unwrap_or(9usize).clamp(1, 9),
            font_size: get("ime", "font_size").and_then(|v| v.parse().ok()).unwrap_or(18f32).clamp(8.0, 72.0),
            dark: get("ime", "theme").is_some_and(|v| v.eq_ignore_ascii_case("dark")),
            fonts,
            start_enabled: flag(get("ime", "start_enabled"), false),
        }
    }
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(path),
    }
}

/// Finds the dictionaries: `$MIN_KEYBOARD_ASSETS`, then `../share/min-keyboard/assets` next to
/// the installed binary, then the repository's `assets/` for development builds.
pub fn assets_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MIN_KEYBOARD_ASSETS") {
        return PathBuf::from(dir);
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("../share/min-keyboard/assets"));
            candidates.push(dir.join("assets"));
        }
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets"));
    candidates.iter().find(|d| d.join("english_words.txt").exists()).cloned().unwrap_or_else(|| candidates.last().cloned().unwrap_or_default())
}

/// The dictionaries, each loaded the first time its language is used.
pub struct Dictionaries {
    assets: DirAssets,
    english: Option<LatinEngine>,
    malay: Option<LatinEngine>,
    pinyin: Option<PinyinEngine>,
    japanese: Option<JapaneseEngine>,
    korean: Option<KoreanEngine>,
}

fn load<'a, T>(slot: &'a mut Option<T>, what: &str, make: impl FnOnce() -> std::io::Result<T>) -> Option<&'a T> {
    if slot.is_none() {
        match make() {
            Ok(engine) => *slot = Some(engine),
            Err(e) => {
                eprintln!("min-keyboard: cannot load the {what} dictionary: {e}");
                return None;
            }
        }
    }
    slot.as_ref()
}

impl Dictionaries {
    pub fn new(dir: PathBuf) -> Dictionaries {
        Dictionaries { assets: DirAssets(dir), english: None, malay: None, pinyin: None, japanese: None, korean: None }
    }

    /// Loads a language's dictionary now, so the first key is not slow.
    pub fn preload(&mut self, language: Language) {
        self.candidates(language, "");
    }

    pub fn candidates(&mut self, language: Language, input: &str) -> Vec<String> {
        let assets = &self.assets;
        let result = match language {
            Language::English => load(&mut self.english, "English", || LatinEngine::english(assets)).map(|e| e.candidates(input)),
            Language::Malay => load(&mut self.malay, "Malay", || LatinEngine::malay(assets)).map(|e| e.candidates(input)),
            Language::Simplified => load(&mut self.pinyin, "pinyin", || PinyinEngine::new(assets)).map(|e| e.candidates(input, ScriptMode::Simplified)),
            Language::Traditional => load(&mut self.pinyin, "pinyin", || PinyinEngine::new(assets)).map(|e| e.candidates(input, ScriptMode::Traditional)),
            Language::Zhuyin => load(&mut self.pinyin, "pinyin", || PinyinEngine::new(assets))
                .map(|e| e.syllable_candidates(&pinyin::bopomofo_syllables(input), ScriptMode::Traditional)),
            Language::Japanese => load(&mut self.japanese, "Japanese", || JapaneseEngine::new(assets)).map(|e| e.candidates(input)),
            Language::Korean => load(&mut self.korean, "Korean", || KoreanEngine::new(assets)).map(|e| e.key_candidates(input)),
            Language::KoreanRomanized => load(&mut self.korean, "Korean", || KoreanEngine::new(assets)).map(|e| e.candidates(input)),
        };
        if input.is_empty() {
            return Vec::new();
        }
        result.unwrap_or_else(|| vec![preedit_for(language, input)])
    }
}

/// What the composition looks like while it is typed.
pub fn preedit_for(language: Language, input: &str) -> String {
    match language {
        Language::Japanese => japanese::to_hiragana(input, false),
        Language::Korean => korean::compose_keys(input),
        _ => input.to_string(),
    }
}

/// The shared state: whether the input method is on, the current language and the dictionaries.
pub struct Core {
    pub config: Config,
    pub enabled: bool,
    pub language: Language,
    pub dictionaries: Dictionaries,
}

impl Core {
    pub fn new(config: Config, assets: PathBuf) -> Core {
        Core { enabled: config.start_enabled, language: config.languages[0], config, dictionaries: Dictionaries::new(assets) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config() {
        let config = Config::parse(
            "[hotkeys]\nmode = Control+F12\nenglish = Control+1\ntoggle = Alt+grave\nmalay = none\n[languages]\nkorean = false\nzhuyin = true\n[ime]\npunctuation = ascii\npage_size = 5\ntheme = dark\n",
        );
        assert_eq!(config.cycle, Some(Hotkey { sym: 0xffc9, modifiers: keysym::CONTROL }));
        assert_eq!(config.toggle, Some(Hotkey { sym: 0x60, modifiers: keysym::MOD1 }));
        assert!(!config.languages.contains(&Language::Korean));
        assert!(config.languages.contains(&Language::Zhuyin));
        assert!(!config.language_hotkeys.iter().any(|(l, _)| *l == Language::Malay));
        assert!(!config.fullwidth_punctuation);
        assert_eq!(config.page_size, 5);
        assert!(config.dark);
        let defaults = Config::default();
        assert_eq!(defaults.toggle, Some(Hotkey { sym: 0x20, modifiers: keysym::CONTROL }));
        assert_eq!(defaults.languages[0], Language::English);
    }

    #[test]
    fn hotkeys_match() {
        let key = Hotkey::parse("Control+space").unwrap();
        assert!(key.matches(0x20, keysym::CONTROL | keysym::MOD2));
        assert!(!key.matches(0x20, keysym::CONTROL | keysym::SHIFT));
        assert!(!key.matches(0x20, 0));
        let digit = Hotkey::parse("Control+1").unwrap();
        assert!(digit.matches('1' as u32, keysym::CONTROL));
        let letter = Hotkey::parse("Control+Shift+K").unwrap();
        assert!(letter.matches('K' as u32, keysym::CONTROL | keysym::SHIFT));
        assert!(!letter.matches('k' as u32, keysym::CONTROL));
    }
}
