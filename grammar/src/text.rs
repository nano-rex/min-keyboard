//! The text side of the grammar check: picking the language, building the prompt, cleaning up the
//! model's answer and showing what changed.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    Mandarin,
    Malay,
}

impl Language {
    pub fn name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Mandarin => "Mandarin Chinese",
            Language::Malay => "Malay",
        }
    }

    /// One worked example per language. Small models follow an example far more reliably than
    /// instructions alone.
    pub fn example(self) -> (&'static str, &'static str) {
        match self {
            Language::English => (
                "he dont know where there going tomorow, me and her was waiting",
                "He doesn't know where they're going tomorrow. She and I were waiting.",
            ),
            Language::Mandarin => ("我昨天去了图书馆看书了，因为我明天有考试的。", "我昨天去图书馆看书了，因为我明天有考试。"),
            Language::Malay => {
                ("Saya ada tiga kucing-kucing dan dia suka makan ikan sangat.", "Saya ada tiga ekor kucing dan dia sangat suka makan ikan.")
            }
        }
    }
}

const MALAY_WORDS: &[&str] = &[
    "yang",
    "dan",
    "saya",
    "tidak",
    "tak",
    "ini",
    "itu",
    "untuk",
    "dengan",
    "akan",
    "ada",
    "di",
    "ke",
    "dari",
    "kita",
    "kami",
    "mereka",
    "sudah",
    "belum",
    "boleh",
    "dia",
    "apa",
    "nak",
    "aku",
    "awak",
    "kamu",
    "pada",
    "juga",
    "atau",
    "telah",
    "sangat",
    "lagi",
    "mahu",
    "hendak",
    "kerana",
    "tetapi",
    "bagaimana",
    "mengapa",
    "kenapa",
    "bila",
    "sini",
    "sana",
    "mana",
    "semua",
    "satu",
    "orang",
    "baik",
    "terima",
    "kasih",
    "selamat",
    "makan",
    "pergi",
    "datang",
    "rumah",
    "sekolah",
    "kerja",
    "hari",
];
const ENGLISH_WORDS: &[&str] = &[
    "the", "and", "is", "are", "was", "were", "to", "of", "in", "that", "it", "you", "i", "he", "she", "we", "they", "this", "have", "has",
    "with", "for", "not", "be", "on", "at", "my", "your", "will",
];

fn is_han(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// Chinese characters mean Mandarin. Otherwise common function words decide between Malay and
/// English, with the keyboard's current mode as the tie-breaker.
pub fn detect_language(text: &str, malay_mode: bool) -> Language {
    let han = text.chars().filter(|&c| is_han(c)).count();
    let letters = text.chars().filter(|&c| !is_han(c) && c.is_alphabetic()).count();
    if han > 0 && han * 2 >= letters / 4 {
        return Language::Mandarin;
    }
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !(c.is_ascii_lowercase() || c == '\'' || c == '-')).collect();
    let malay = words.iter().filter(|w| MALAY_WORDS.contains(w)).count();
    let english = words.iter().filter(|w| ENGLISH_WORDS.contains(w)).count();
    match malay.cmp(&english) {
        std::cmp::Ordering::Greater => Language::Malay,
        std::cmp::Ordering::Less => Language::English,
        std::cmp::Ordering::Equal if malay_mode => Language::Malay,
        std::cmp::Ordering::Equal => Language::English,
    }
}

/// True when the text is mostly Japanese kana or Korean Hangul, which the check does not cover.
pub fn is_unsupported_script(text: &str) -> bool {
    let kana_or_hangul = text
        .chars()
        .filter(|&c| matches!(c, '\u{3040}'..='\u{30ff}' | '\u{ac00}'..='\u{d7af}' | '\u{1100}'..='\u{11ff}' | '\u{3130}'..='\u{318f}'))
        .count();
    kana_or_hangul > 0 && kana_or_hangul * 4 >= text.chars().filter(|c| c.is_alphabetic()).count()
}

pub fn system_prompt(language: Language) -> String {
    let language = language.name();
    format!(
        "You are a careful proofreader for {language}. \
         Find and fix every grammar, spelling, punctuation and word-choice mistake in the text the user gives you \
         (for example verb tenses, agreement, wrong words that sound alike, missing words, measure words and word order) \
         so that it is correct, natural {language}. \
         Keep the original meaning, tone, names, line breaks and writing system \
         (Chinese written in Traditional characters stays Traditional, Simplified stays Simplified). \
         Do not translate, do not answer questions in the text, do not add anything, and do not explain. \
         If the text is already correct, return it unchanged. \
         Reply with the corrected text only."
    )
}

pub fn request(language: Language) -> String {
    format!("Proofread this {} text:\n\n", language.name())
}

/// Removes a thinking block, code fences and wrapping quotes the model may add.
pub fn clean_answer(raw: &str) -> String {
    let mut text = raw;
    if let Some(at) = text.find("</think>") {
        text = &text[at + "</think>".len()..];
    }
    let mut text = text.trim();
    if let Some(rest) = text.strip_prefix("```") {
        if let (Some(first_line), Some(fence)) = (rest.find('\n'), rest.rfind("```")) {
            if fence > first_line {
                text = rest[first_line + 1..fence].trim();
            }
        }
    }
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') && text[1..].find('"') == Some(text.len() - 2) {
        text = &text[1..text.len() - 1];
    }
    text.to_string()
}

pub fn changed(before: &str, after: &str) -> bool {
    before.trim() != after.trim()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Same,
    Removed,
    Added,
}

fn is_cjk(c: char) -> bool {
    is_han(c) || ('\u{3000}'..='\u{30ff}').contains(&c) || ('\u{ff00}'..='\u{ffef}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c)
}

/// Words with their spaces kept separate; every CJK character is its own token.
pub fn diff_tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut word_start = None;
    for (i, c) in text.char_indices() {
        if (c.is_alphanumeric() && !is_cjk(c)) || c == '\'' || c == '-' {
            word_start.get_or_insert(i);
            continue;
        }
        if let Some(start) = word_start.take() {
            tokens.push(&text[start..i]);
        }
        tokens.push(&text[i..i + c.len_utf8()]);
    }
    if let Some(start) = word_start {
        tokens.push(&text[start..]);
    }
    tokens
}

/// A word-level diff (character-level for Chinese), with adjacent parts of a kind merged.
pub fn diff(before: &str, after: &str) -> Vec<(Change, String)> {
    let (a, b) = (diff_tokens(before), diff_tokens(after));
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[at(i, j)] = if a[i] == b[j] { lcs[at(i + 1, j + 1)] + 1 } else { lcs[at(i + 1, j)].max(lcs[at(i, j + 1)]) };
        }
    }
    let mut parts: Vec<(Change, String)> = Vec::new();
    let mut push = |kind: Change, text: &str| match parts.last_mut() {
        Some((last, existing)) if *last == kind => existing.push_str(text),
        _ => parts.push((kind, text.to_string())),
    };
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            push(Change::Same, a[i]);
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[at(i + 1, j)] >= lcs[at(i, j + 1)]) {
            push(Change::Removed, a[i]); // removals first: "-go +went"
            i += 1;
        } else {
            push(Change::Added, b[j]);
            j += 1;
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_languages() {
        assert_eq!(detect_language("我昨天去了图书馆", false), Language::Mandarin);
        assert_eq!(detect_language("Saya tidak tahu di mana dia", false), Language::Malay);
        assert_eq!(detect_language("I was going to the shop", true), Language::English);
        assert_eq!(detect_language("Okay", true), Language::Malay);
        assert!(is_unsupported_script("これはペンです"));
        assert!(is_unsupported_script("감사합니다"));
        assert!(!is_unsupported_script("我爱你"));
    }

    #[test]
    fn cleans_answers() {
        assert_eq!(clean_answer("<think>\n\n</think>\n\nHello there."), "Hello there.");
        assert_eq!(clean_answer("```text\nFixed.\n```"), "Fixed.");
        assert_eq!(clean_answer("\"Quoted.\""), "Quoted.");
        assert_eq!(clean_answer("\"One\" and \"two\""), "\"One\" and \"two\"");
    }

    #[test]
    fn diffs_words_and_characters() {
        let parts = diff("he go home", "He went home");
        assert_eq!(
            parts,
            vec![
                (Change::Removed, "he".into()),
                (Change::Added, "He".into()),
                (Change::Same, " ".into()),
                (Change::Removed, "go".into()),
                (Change::Added, "went".into()),
                (Change::Same, " home".into()),
            ]
        );
        let parts = diff("我去了图书馆了", "我去图书馆了");
        assert_eq!(parts, vec![(Change::Same, "我去".into()), (Change::Removed, "了".into()), (Change::Same, "图书馆了".into())]);
    }
}
