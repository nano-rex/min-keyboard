//! Runs the input engines against the real bundled assets.

use std::path::PathBuf;
use std::sync::OnceLock;

use engines::{japanese, korean, DirAssets, JapaneseEngine, KoreanEngine, LatinEngine, PinyinEngine, ScriptMode};

struct All {
    english: LatinEngine,
    malay: LatinEngine,
    pinyin: PinyinEngine,
    japanese: JapaneseEngine,
    korean: KoreanEngine,
}

fn all() -> &'static All {
    static ALL: OnceLock<All> = OnceLock::new();
    ALL.get_or_init(|| {
        let assets = DirAssets(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets"));
        All {
            english: LatinEngine::english(&assets).unwrap(),
            malay: LatinEngine::malay(&assets).unwrap(),
            pinyin: PinyinEngine::new(&assets).unwrap(),
            japanese: JapaneseEngine::new(&assets).unwrap(),
            korean: KoreanEngine::new(&assets).unwrap(),
        }
    })
}

fn assert_first(expected: &str, candidates: Vec<String>) {
    assert!(!candidates.is_empty(), "no candidates, expected {expected}");
    assert_eq!(candidates[0], expected, "{candidates:?}");
}

fn assert_contains(expected: &str, candidates: Vec<String>) {
    assert!(candidates.iter().any(|c| c == expected), "{expected} missing from {candidates:?}");
}

#[test]
fn english_completes_by_frequency() {
    let english = &all().english;
    assert_first("the", english.candidates("the"));
    assert_contains("there", english.candidates("the"));
    assert_contains("tomorrow", english.candidates("tomo"));
    assert_contains("definitely", english.candidates("defin"));
}

#[test]
fn english_corrects_typos() {
    let english = &all().english;
    assert_first("the", english.candidates("teh"));
    assert_contains("receive", english.candidates("recieve"));
    assert_eq!(english.auto_correction("teh").as_deref(), Some("the"));
    assert_eq!(english.auto_correction("recieve").as_deref(), Some("receive"));
    assert_eq!(english.auto_correction("definately").as_deref(), Some("definitely"));
}

#[test]
fn english_autocorrect_leaves_real_words_alone() {
    let english = &all().english;
    for word in ["were", "form", "hel", "cat"] {
        assert_eq!(english.auto_correction(word), None, "{word}");
    }
}

#[test]
fn english_fixes_contractions_and_capitals() {
    let english = &all().english;
    assert_eq!(english.auto_correction("dont").as_deref(), Some("don't"));
    assert_eq!(english.auto_correction("im").as_deref(), Some("I'm"));
    assert_eq!(english.auto_correction("i").as_deref(), Some("I"));
    assert_eq!(english.auto_correction("monday").as_deref(), Some("Monday"));
    assert_first("I'm", english.candidates("im"));
    assert!(english.is_known_word("wdym"));
}

#[test]
fn malay_words() {
    let malay = &all().malay;
    assert_first("saya", malay.candidates("saya"));
    assert_contains("terima", malay.candidates("teri"));
    assert_contains("bagaimana", malay.candidates("bagai"));
    assert_eq!(malay.auto_correction("bagaimna").as_deref(), Some("bagaimana"));
    assert_eq!(malay.auto_correction("makan"), None);
}

#[test]
fn pinyin_ranks_common_characters_first() {
    let pinyin = &all().pinyin;
    assert_first("我", pinyin.candidates("wo", ScriptMode::Simplified));
    assert_first("你", pinyin.candidates("ni", ScriptMode::Simplified));
    assert_first("是", pinyin.candidates("shi", ScriptMode::Simplified));
    assert_first("的", pinyin.candidates("de", ScriptMode::Simplified));
    assert_first("謝謝", pinyin.candidates("xiexie", ScriptMode::Traditional));
    assert_first("不知道", pinyin.candidates("bzd", ScriptMode::Simplified));
}

#[test]
fn pinyin_segments_sentences() {
    let pinyin = &all().pinyin;
    assert_first("我爱你", pinyin.candidates("woaini", ScriptMode::Simplified));
    assert_first("今天天气很好", pinyin.candidates("jintiantianqihenhao", ScriptMode::Simplified));
    assert_first("我愛你", pinyin.candidates("woaini", ScriptMode::Traditional));
    assert_first("你好", pinyin.candidates("nihao", ScriptMode::Simplified));
    assert!(pinyin.candidates("'", ScriptMode::Simplified).iter().all(|c| !c.is_empty()));
}

#[test]
fn romaji_to_kana() {
    assert_eq!(japanese::to_hiragana("konnichiha", true), "こんにちは");
    assert_eq!(japanese::to_hiragana("kitte", true), "きって");
    assert_eq!(japanese::to_hiragana("hon", true), "ほん");
    assert_eq!(japanese::to_hiragana("honn", true), "ほん");
    assert_eq!(japanese::to_hiragana("shin'you", true), "しんよう");
    assert_eq!(japanese::to_hiragana("kyaku", true), "きゃく");
    assert_eq!(japanese::to_hiragana("cha", true), "ちゃ");
    assert_eq!(japanese::to_hiragana("hon", false), "ほn");
    assert_eq!(japanese::to_katakana("かたかな"), "カタカナ");
}

#[test]
fn japanese_converts_to_kanji() {
    let japanese = &all().japanese;
    assert_first("私", japanese.candidates("watashi"));
    assert_first("日本", japanese.candidates("nihon"));
    assert_contains("にほん", japanese.candidates("nihon"));
    assert_contains("ニホン", japanese.candidates("nihon"));
    assert_contains("記者", japanese.candidates("kisha"));
}

#[test]
fn korean_words_and_syllables() {
    let korean = &all().korean;
    assert_first("안녕하세요", korean.candidates("annyeonghaseyo"));
    assert_first("감사합니다", korean.candidates("gamsahamnida"));
    assert_contains("사랑해", korean.candidates("saranghae"));
    assert_eq!(korean::compose("hanguk").as_deref(), Some("한국"));
    assert_eq!(korean::compose("saranghae").as_deref(), Some("사랑해"));
    assert_contains("한국어", korean.candidates("hangug"));
}

#[test]
fn edit_distance_prefers_neighbouring_keys() {
    assert_eq!(engines::latin::distance("teh", "the", 2.0), 0.8);
    assert_eq!(engines::latin::distance("cat", "cap", 2.0), 1.0);
    assert!((engines::latin::distance("cat", "cst", 2.0) - 0.6).abs() < 1e-9);
    assert_eq!(engines::latin::distance("a", "abcd", 1.0), 2.0);
}
