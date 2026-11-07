//! Pinyin to Chinese conversion. Lexicon candidates are stored most frequent first. Input that is
//! not a single dictionary entry ("woaini") is segmented into the fewest dictionary words.

use std::collections::BTreeMap;
use std::io;

use crate::{for_each_line, java_split, prefixed, split_candidates, Assets, Candidates};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptMode {
    Simplified,
    Traditional,
}

const LEXICON_ASSET: &str = "pinyin_lexicon.tsv";
const SHORTCUTS_ASSET: &str = "pinyin_shortcuts.tsv";
const MAX_CANDIDATES: usize = 64;
const MAX_KEY_LENGTH: usize = 30;

/// Roughly the token count behind the lexicon frequencies (jieba's dictionary totals ~6e7).
fn log_corpus_size() -> f64 {
    6e7f64.ln()
}

struct Entry {
    simplified: Vec<String>,
    traditional: Vec<String>,
    /// Segmentation cost of the top candidate: -log(frequency / corpus size).
    cost: f64,
}

impl Entry {
    fn new(simplified: Vec<String>, traditional: Vec<String>, frequency: u64) -> Self {
        Entry { simplified, traditional, cost: log_corpus_size() - (frequency as f64 + 1.0).ln() }
    }

    fn candidates(&self, mode: ScriptMode) -> &[String] {
        match mode {
            ScriptMode::Traditional => &self.traditional,
            ScriptMode::Simplified => &self.simplified,
        }
    }
}

pub struct PinyinEngine {
    lexicon: BTreeMap<String, Entry>,
    shortcuts: BTreeMap<String, Entry>,
}

impl PinyinEngine {
    pub fn new(assets: &dyn Assets) -> io::Result<Self> {
        let mut engine = PinyinEngine { lexicon: BTreeMap::new(), shortcuts: BTreeMap::new() };
        engine.add_fallback_shortcuts();
        let shortcuts = &mut engine.shortcuts;
        for_each_line(assets, SHORTCUTS_ASSET, |line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 && !normalize(parts[0]).is_empty() {
                shortcuts.insert(normalize(parts[0]), Entry::new(split_candidates(parts[1]), split_candidates(parts[2]), 0));
            }
        })?;
        let lexicon = &mut engine.lexicon;
        for_each_line(assets, LEXICON_ASSET, |line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 && !normalize(parts[0]).is_empty() {
                let frequency = parts.get(3).and_then(|f| f.parse().ok()).unwrap_or(0);
                lexicon.insert(normalize(parts[0]), Entry::new(split_candidates(parts[1]), split_candidates(parts[2]), frequency));
            }
        })?;
        Ok(engine)
    }

    pub fn is_composing_char(ch: char) -> bool {
        ch.is_ascii_lowercase() || ch == '\''
    }

    pub fn candidates(&self, raw_input: &str, mode: ScriptMode) -> Vec<String> {
        let input = normalize(raw_input);
        if input.is_empty() {
            return Vec::new();
        }
        let compact = input.replace('\'', "");
        let mut results = Candidates::default();
        if let Some(shortcut) = self.shortcuts.get(&compact) {
            add_all(&mut results, shortcut.candidates(mode));
        }
        let exact = self.lexicon.get(&compact);
        if exact.is_none() || input.contains('\'') {
            if let Some(sentence) = self.sentence(&input, mode) {
                results.add(&sentence);
            }
        }
        if let Some(exact) = exact {
            add_all(&mut results, exact.candidates(mode));
        }
        for (_, entry) in prefixed(&self.lexicon, &compact, &compact) {
            if results.len() >= MAX_CANDIDATES {
                break;
            }
            add_all(&mut results, entry.candidates(mode));
        }
        // Single characters for the first syllable, so long input can still be typed piece by piece.
        if let Some(first) = self.first_word(&input, mode) {
            results.add(first);
        }
        results.into_vec(MAX_CANDIDATES)
    }

    /// Splits the input into the most probable sequence of lexicon words (a unigram model) and joins each key's top candidate.
    /// The last piece may be an unfinished syllable, completed with the most frequent match.
    /// Apostrophes force a split. Returns `None` when the input cannot be segmented.
    pub fn sentence(&self, input: &str, mode: ScriptMode) -> Option<String> {
        let pieces = java_split(input, '\'');
        let mut result = String::new();
        let mut segments = 0;
        for (p, piece) in pieces.iter().enumerate() {
            if piece.is_empty() {
                continue;
            }
            let (count, text) = self.segment(piece, mode, p == pieces.len() - 1, None)?;
            result.push_str(&text);
            segments += count;
        }
        ((segments >= 2 || input.contains('\'')) && !result.is_empty()).then_some(result)
    }

    /// Candidates for pinyin typed syllable by syllable, as the Bopomofo keyboard does: words may
    /// span several syllables but never split one.
    pub fn syllable_candidates(&self, syllables: &[String], mode: ScriptMode) -> Vec<String> {
        let compact = syllables.concat();
        if compact.is_empty() {
            return Vec::new();
        }
        let mut cuts = vec![false; compact.len() + 1];
        let mut at = 0;
        cuts[0] = true;
        for syllable in syllables {
            at += syllable.len();
            cuts[at] = true;
        }
        let mut results = Candidates::default();
        let exact = self.lexicon.get(&compact);
        if exact.is_none() {
            if let Some((count, text)) = self.segment(&compact, mode, true, Some(&cuts)) {
                if count >= 2 {
                    results.add(&text);
                }
            }
        }
        if let Some(exact) = exact {
            add_all(&mut results, exact.candidates(mode));
        }
        for (_, entry) in prefixed(&self.lexicon, &compact, &compact) {
            if results.len() >= MAX_CANDIDATES {
                break;
            }
            add_all(&mut results, entry.candidates(mode));
        }
        // The first syllable on its own, so long input can be typed piece by piece.
        if let Some(first) = self.lexicon.get(&syllables[0]).and_then(|entry| top(entry, mode)) {
            results.add(first);
        }
        results.into_vec(MAX_CANDIDATES)
    }

    /// The cheapest split of one apostrophe-free piece: (word count, joined top candidates).
    /// With `cuts`, words may only start and end where it is true.
    fn segment(&self, piece: &str, mode: ScriptMode, allow_prefix_at_end: bool, cuts: Option<&[bool]>) -> Option<(usize, String)> {
        // Normalized input is ASCII, so byte offsets are character offsets.
        let n = piece.len();
        let mut best = vec![f64::MAX; n + 1];
        let mut count = vec![0usize; n + 1];
        let mut word: Vec<Option<&str>> = vec![None; n + 1];
        let mut previous = vec![0usize; n + 1];
        best[0] = 0.0;
        for i in 0..n {
            if best[i] == f64::MAX {
                continue;
            }
            for j in i + 1..=n.min(i + MAX_KEY_LENGTH) {
                if cuts.is_some_and(|cuts| !cuts[j]) {
                    continue;
                }
                let key = &piece[i..j];
                let mut entry = self.lexicon.get(key);
                if entry.is_none() && j == n && allow_prefix_at_end {
                    entry = self.completion(key, mode);
                }
                let Some(entry) = entry else { continue };
                let Some(candidate) = top(entry, mode) else { continue };
                if best[i] + entry.cost < best[j] {
                    best[j] = best[i] + entry.cost;
                    count[j] = count[i] + 1;
                    word[j] = Some(candidate);
                    previous[j] = i;
                }
            }
        }
        if best[n] == f64::MAX {
            return None;
        }
        let mut parts = Vec::new();
        let mut at = n;
        while at > 0 {
            parts.push(word[at].unwrap_or_default());
            at = previous[at];
        }
        parts.reverse();
        Some((count[n], parts.concat()))
    }

    fn first_word(&self, input: &str, mode: ScriptMode) -> Option<&str> {
        let compact = input.replace('\'', "");
        let longest = compact.len().saturating_sub(1).min(6);
        (1..=longest).rev().find_map(|j| self.lexicon.get(&compact[..j]).and_then(|entry| top(entry, mode)))
    }

    /// The most frequent entry whose key completes the prefix, e.g. the unfinished last syllable.
    fn completion(&self, prefix: &str, mode: ScriptMode) -> Option<&Entry> {
        let mut best: Option<&Entry> = None;
        for (_, entry) in prefixed(&self.lexicon, prefix, prefix).take(200) {
            if top(entry, mode).is_some() && best.is_none_or(|b| entry.cost < b.cost) {
                best = Some(entry);
            }
        }
        best
    }

    fn add_fallback_shortcuts(&mut self) {
        for (key, simplified, traditional) in
            [("bzd", "不知道", "不知道"), ("wsm", "为什么", "為什麼"), ("yyds", "永远的神", "永遠的神"), ("xswl", "笑死我了", "笑死我了")]
        {
            self.shortcuts.insert(key.to_string(), Entry::new(vec![simplified.to_string()], vec![traditional.to_string()], 0));
        }
    }
}

fn top(entry: &Entry, mode: ScriptMode) -> Option<&str> {
    entry.candidates(mode).first().map(String::as_str)
}

fn add_all(results: &mut Candidates, candidates: &[String]) {
    for candidate in candidates {
        if results.len() >= MAX_CANDIDATES {
            return;
        }
        if !candidate.is_empty() {
            results.add(candidate);
        }
    }
}

// ---------------------------------------------------------------- Bopomofo ----

/// Whether a key belongs to the Bopomofo (Zhuyin) keyboard: a symbol or a tone mark.
pub fn is_bopomofo_key(c: char) -> bool {
    ('ㄅ'..='ㄩ').contains(&c) || is_tone_mark(c)
}

fn is_tone_mark(c: char) -> bool {
    matches!(c, 'ˊ' | 'ˇ' | 'ˋ' | '˙')
}

/// The pinyin each Bopomofo initial stands for.
fn initial(c: char) -> Option<&'static str> {
    Some(match c {
        'ㄅ' => "b",
        'ㄆ' => "p",
        'ㄇ' => "m",
        'ㄈ' => "f",
        'ㄉ' => "d",
        'ㄊ' => "t",
        'ㄋ' => "n",
        'ㄌ' => "l",
        'ㄍ' => "g",
        'ㄎ' => "k",
        'ㄏ' => "h",
        'ㄐ' => "j",
        'ㄑ' => "q",
        'ㄒ' => "x",
        'ㄓ' => "zh",
        'ㄔ' => "ch",
        'ㄕ' => "sh",
        'ㄖ' => "r",
        'ㄗ' => "z",
        'ㄘ' => "c",
        'ㄙ' => "s",
        _ => return None,
    })
}

fn is_medial(c: char) -> bool {
    matches!(c, 'ㄧ' | 'ㄨ' | 'ㄩ')
}

/// Spells one Bopomofo syllable in pinyin, with ü as v: ㄐㄩㄢ is "juan", ㄨㄟ is "wei", ㄓ is "zhi".
fn syllable(lead: Option<&str>, medial: Option<char>, fin: Option<char>) -> String {
    let rhyme = |c: char| match c {
        'ㄚ' => "a",
        'ㄛ' => "o",
        'ㄜ' | 'ㄝ' => "e",
        'ㄞ' => "ai",
        'ㄟ' => "ei",
        'ㄠ' => "ao",
        'ㄡ' => "ou",
        'ㄢ' => "an",
        'ㄣ' => "en",
        'ㄤ' => "ang",
        'ㄥ' => "eng",
        _ => "er",
    };
    let initial = lead.unwrap_or("");
    let ending = match (medial, fin) {
        (None, None) if matches!(initial, "zh" | "ch" | "sh" | "r" | "z" | "c" | "s") => "i".to_string(),
        (None, None) => String::new(),
        (None, Some(f)) => rhyme(f).to_string(),
        (Some('ㄧ'), f) => match (f, lead.is_some()) {
            (None, false) => "yi".into(),
            (Some('ㄣ'), false) => "yin".into(),
            (Some('ㄥ'), false) => "ying".into(),
            (Some('ㄡ'), true) => "iu".into(),
            (None, true) => "i".into(),
            (Some('ㄣ'), true) => "in".into(),
            (Some('ㄥ'), true) => "ing".into(),
            (Some(f), with_initial) => {
                let rhyme = match f {
                    'ㄝ' => "e",
                    'ㄡ' => "ou",
                    f => rhyme(f),
                };
                if with_initial {
                    format!("i{rhyme}")
                } else {
                    format!("y{rhyme}")
                }
            }
        },
        (Some('ㄨ'), f) => match (f, lead.is_some()) {
            (None, false) => "wu".into(),
            (None, true) => "u".into(),
            (Some('ㄟ'), true) => "ui".into(),
            (Some('ㄣ'), true) => "un".into(),
            (Some('ㄥ'), true) => "ong".into(),
            (Some(f), with_initial) => format!("{}{}", if with_initial { "u" } else { "w" }, rhyme(f)),
        },
        // ㄩ is u after j, q and x, v (ü) after n and l, and yu with no initial.
        (Some(_), f) => {
            let rest = match f {
                None => "",
                Some('ㄝ') => "e",
                Some('ㄢ') => "an",
                Some('ㄣ') => "n",
                Some('ㄥ') => {
                    return format!("{}{}", if lead.is_some() { initial } else { "y" }, if lead.is_some() { "iong" } else { "ong" })
                }
                Some(f) => rhyme(f),
            };
            let vowel = match initial {
                "" => "yu",
                "n" | "l" => "v",
                _ => "u",
            };
            return format!("{initial}{vowel}{rest}");
        }
    };
    format!("{initial}{ending}")
}

/// Turns Bopomofo keys into pinyin with an apostrophe between syllables: "ㄋㄧˇㄏㄠˇ" is "ni'hao".
pub fn bopomofo_to_pinyin(keys: &str) -> String {
    bopomofo_syllables(keys).join("'")
}

/// The pinyin syllables of Bopomofo keys. A tone mark ends a syllable; so does a key that
/// cannot continue it.
pub fn bopomofo_syllables(keys: &str) -> Vec<String> {
    let mut syllables: Vec<String> = Vec::new();
    let (mut lead, mut medial, mut fin): (Option<&str>, Option<char>, Option<char>) = (None, None, None);
    let mut flush = |lead: &mut Option<&str>, medial: &mut Option<char>, fin: &mut Option<char>| {
        if lead.is_some() || medial.is_some() || fin.is_some() {
            syllables.push(syllable(lead.take(), medial.take(), fin.take()));
        }
    };
    for c in keys.chars() {
        if is_tone_mark(c) {
            flush(&mut lead, &mut medial, &mut fin);
        } else if let Some(i) = initial(c) {
            flush(&mut lead, &mut medial, &mut fin);
            lead = Some(i);
        } else if is_medial(c) {
            if medial.is_some() || fin.is_some() {
                flush(&mut lead, &mut medial, &mut fin);
            }
            medial = Some(c);
        } else if ('ㄚ'..='ㄦ').contains(&c) {
            if fin.is_some() {
                flush(&mut lead, &mut medial, &mut fin);
            }
            fin = Some(c);
        }
    }
    flush(&mut lead, &mut medial, &mut fin);
    syllables
}

/// Lower-cases and keeps letters and apostrophes; ü is typed as v.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_lowercase() || ch == '\'' {
            out.push(ch);
        } else if ch == 'ü' {
            out.push('v');
        }
    }
    out
}
