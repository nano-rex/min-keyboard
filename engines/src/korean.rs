//! Romanized Korean input. Words come from a lexicon keyed by Revised Romanization; anything
//! else is composed syllable by syllable, so every input produces Hangul.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::sync::OnceLock;

use crate::{for_each_line, for_each_line_if_present, prefixed, split_candidates, Assets, Candidates};

const MAX_CANDIDATES: usize = 32;
const MAX_COMPLETIONS: usize = 20;
const LONGEST_SYLLABLE: usize = 8;

pub const INITIALS: [&str; 19] = ["g", "kk", "n", "d", "tt", "r", "m", "b", "pp", "s", "ss", "", "j", "jj", "ch", "k", "t", "p", "h"];
pub const MEDIALS: [&str; 21] =
    ["a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo", "we", "wi", "yu", "eu", "ui", "i"];
/// Typed spellings for each final consonant index (0 = none). Only simple finals are typed.
const FINALS: [&[&str]; 28] = [
    &[],
    &["k", "g"],
    &["kk"],
    &[],
    &["n"],
    &[],
    &[],
    &["t", "d"],
    &["l", "r"],
    &[],
    &[],
    &[],
    &[],
    &[],
    &[],
    &[],
    &["m"],
    &["p", "b"],
    &[],
    &["s"],
    &["ss"],
    &["ng"],
    &["j"],
    &["ch"],
    &[],
    &[],
    &[],
    &["h"],
];

struct Entry {
    words: Vec<String>,
    score: i64,
}

pub struct KoreanEngine {
    lexicon: BTreeMap<String, Entry>,
    shortcuts: HashMap<String, Vec<String>>,
}

impl KoreanEngine {
    pub fn new(assets: &dyn Assets) -> io::Result<Self> {
        let mut lexicon = BTreeMap::new();
        for_each_line(assets, "korean_lexicon.tsv", |line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 || parts[0].is_empty() {
                return;
            }
            let score = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            lexicon.insert(parts[0].to_string(), Entry { words: split_candidates(parts[1]), score });
        })?;
        let mut shortcuts: HashMap<String, Vec<String>> = HashMap::new();
        for_each_line_if_present(assets, "korean_shortcuts.tsv", |line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 && !parts[0].is_empty() {
                shortcuts.entry(parts[0].to_lowercase()).or_default().push(parts[1].to_string());
            }
        })?;
        Ok(KoreanEngine { lexicon, shortcuts })
    }

    pub fn candidates(&self, raw_input: &str) -> Vec<String> {
        let input = raw_input.to_lowercase();
        if input.is_empty() {
            return Vec::new();
        }
        let mut results = Candidates::default();
        if let Some(exact) = self.lexicon.get(&input) {
            results.add_all(&exact.words);
        }
        if let Some(shortcut) = self.shortcuts.get(&input) {
            results.add_all(shortcut);
        }
        if let Some(composed) = compose(&input) {
            results.add(&composed);
        }
        let mut matches: Vec<&Entry> = Vec::new();
        for (_, entry) in prefixed(&self.lexicon, &input, &input) {
            matches.push(entry);
            if matches.len() > 3000 {
                break;
            }
        }
        matches.sort_by_key(|entry| std::cmp::Reverse(entry.score));
        for entry in matches.iter().take(MAX_COMPLETIONS) {
            if let Some(first) = entry.words.first() {
                results.add(first);
            }
        }
        if results.is_empty() {
            results.add(raw_input);
        }
        results.into_vec(MAX_CANDIDATES)
    }
}

impl KoreanEngine {
    /// Candidates for keys typed on the two-set (Dubeolsik) Hangul keyboard, such as "ㅎㅏㄴㄱㅜㄱ":
    /// the composed text first, then words that continue the same keys, most frequent first.
    pub fn key_candidates(&self, keys: &str) -> Vec<String> {
        if keys.is_empty() {
            return Vec::new();
        }
        let typed = compose_keys(keys);
        let mut results = Candidates::default();
        results.add(&typed);
        // Words are found by their letter-by-letter transliteration, then kept only if they
        // start with the same keys ("gan" also finds 가나, which ㄱㅏㄴ can still become).
        let prefix = transliterate(&typed);
        let mut matches: Vec<(i64, &String)> = Vec::new();
        let entries = self.lexicon.get_key_value(&prefix).into_iter().chain(prefixed(&self.lexicon, &prefix, &prefix));
        for (_, entry) in entries.take(3000) {
            for word in &entry.words {
                if decompose_keys(word).is_some_and(|k| k.starts_with(keys)) {
                    matches.push((entry.score, word));
                }
            }
        }
        matches.sort_by_key(|&(score, _)| std::cmp::Reverse(score));
        for (_, word) in matches.iter().take(MAX_COMPLETIONS) {
            results.add(word);
        }
        results.into_vec(MAX_CANDIDATES)
    }
}

// ------------------------------------------------------------- Hangul keys ----

/// The compatibility jamo for each initial, medial and final index (final 0 is none).
const INITIAL_JAMO: [char; 19] =
    ['ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ'];
const MEDIAL_JAMO: [char; 21] =
    ['ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ', 'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ'];
const FINAL_JAMO: [char; 28] = [
    '\0', 'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ', 'ㅁ', 'ㅂ', 'ㅄ', 'ㅅ', 'ㅆ', 'ㅇ',
    'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];
/// Final consonants letter by letter, as the lexicon's transliterated keys spell them.
const FINAL_LETTERS: [&str; 28] = [
    "", "g", "kk", "gs", "n", "nj", "nh", "d", "l", "lg", "lm", "lb", "ls", "lt", "lp", "lh", "m", "b", "bs", "s", "ss", "ng", "j", "ch",
    "k", "t", "p", "h",
];
/// Vowels and final consonants typed with two keys.
const COMPOUND_VOWELS: [(char, char, char); 7] = [
    ('ㅗ', 'ㅏ', 'ㅘ'),
    ('ㅗ', 'ㅐ', 'ㅙ'),
    ('ㅗ', 'ㅣ', 'ㅚ'),
    ('ㅜ', 'ㅓ', 'ㅝ'),
    ('ㅜ', 'ㅔ', 'ㅞ'),
    ('ㅜ', 'ㅣ', 'ㅟ'),
    ('ㅡ', 'ㅣ', 'ㅢ'),
];
const COMPOUND_FINALS: [(char, char, char); 11] = [
    ('ㄱ', 'ㅅ', 'ㄳ'),
    ('ㄴ', 'ㅈ', 'ㄵ'),
    ('ㄴ', 'ㅎ', 'ㄶ'),
    ('ㄹ', 'ㄱ', 'ㄺ'),
    ('ㄹ', 'ㅁ', 'ㄻ'),
    ('ㄹ', 'ㅂ', 'ㄼ'),
    ('ㄹ', 'ㅅ', 'ㄽ'),
    ('ㄹ', 'ㅌ', 'ㄾ'),
    ('ㄹ', 'ㅍ', 'ㄿ'),
    ('ㄹ', 'ㅎ', 'ㅀ'),
    ('ㅂ', 'ㅅ', 'ㅄ'),
];

fn position(table: &[char], c: char) -> Option<usize> {
    table.iter().position(|&t| t == c)
}

fn combine(table: &[(char, char, char)], first: char, second: char) -> Option<char> {
    table.iter().find(|&&(a, b, _)| a == first && b == second).map(|&(_, _, c)| c)
}

fn split(table: &[(char, char, char)], c: char) -> Option<(char, char)> {
    table.iter().find(|&&(_, _, d)| d == c).map(|&(a, b, _)| (a, b))
}

/// Whether a key types a vowel (ㅏ to ㅣ).
pub fn is_vowel_key(c: char) -> bool {
    ('ㅏ'..='ㅣ').contains(&c)
}

/// Whether `c` is a key of the Hangul keyboard: a consonant or vowel compatibility jamo.
pub fn is_jamo_key(c: char) -> bool {
    ('ㄱ'..='ㅣ').contains(&c)
}

/// Composes Dubeolsik keys into Hangul: "ㅎㅏㄴㄱㅜㄱ" is 한국. A final consonant moves to the
/// next syllable when a vowel follows (각 + ㅏ = 가가); keys that make no syllable stay as jamo.
pub fn compose_keys(keys: &str) -> String {
    let mut out = String::new();
    // The syllable being built; its final may be two keys (ㄹ, ㄱ for ㄺ).
    let (mut lead, mut vowel, mut tail): (Option<char>, Option<char>, Vec<char>) = (None, None, Vec::new());
    let flush = |out: &mut String, lead: &mut Option<char>, vowel: &mut Option<char>, tail: &mut Vec<char>| {
        match (lead.take(), vowel.take()) {
            (Some(l), Some(v)) => out.push(syllable(l, v, tail).unwrap_or('?')),
            (l, v) => out.extend(l.into_iter().chain(v).chain(tail.iter().copied())),
        }
        tail.clear();
    };
    for key in keys.chars() {
        if is_vowel_key(key) {
            match vowel {
                None => vowel = Some(key),
                Some(v) if tail.is_empty() => match combine(&COMPOUND_VOWELS, v, key) {
                    Some(compound) => vowel = Some(compound),
                    None => {
                        flush(&mut out, &mut lead, &mut vowel, &mut tail);
                        vowel = Some(key);
                    }
                },
                Some(_) => {
                    let moved = tail.pop();
                    flush(&mut out, &mut lead, &mut vowel, &mut tail);
                    (lead, vowel) = (moved, Some(key));
                }
            }
        } else {
            let fits = lead.is_some()
                && vowel.is_some()
                && match tail.as_slice() {
                    [] => position(&FINAL_JAMO, key).is_some(),
                    [first] => combine(&COMPOUND_FINALS, *first, key).is_some(),
                    _ => false,
                };
            if fits {
                tail.push(key);
            } else {
                flush(&mut out, &mut lead, &mut vowel, &mut tail);
                lead = Some(key);
            }
        }
    }
    flush(&mut out, &mut lead, &mut vowel, &mut tail);
    out
}

fn syllable(lead: char, vowel: char, tail: &[char]) -> Option<char> {
    let final_jamo = match *tail {
        [] => None,
        [c] => Some(c),
        [a, b] => Some(combine(&COMPOUND_FINALS, a, b)?),
        _ => return None,
    };
    let fin = match final_jamo {
        Some(c) => position(&FINAL_JAMO, c)?,
        None => 0,
    };
    let code = (position(&INITIAL_JAMO, lead)? * 21 + position(&MEDIAL_JAMO, vowel)?) * 28 + fin;
    char::from_u32(0xAC00 + code as u32)
}

/// The keys that type a word (한국 is "ㅎㅏㄴㄱㅜㄱ"), or `None` if it is not all Hangul.
pub fn decompose_keys(word: &str) -> Option<String> {
    let mut keys = String::new();
    for c in word.chars() {
        if is_jamo_key(c) {
            keys.push(c);
            continue;
        }
        let code = (c as u32).checked_sub(0xAC00).filter(|&code| code < 11172)? as usize;
        keys.push(INITIAL_JAMO[code / 588]);
        for jamo in [MEDIAL_JAMO[(code % 588) / 28], FINAL_JAMO[code % 28]] {
            match split(&COMPOUND_VOWELS, jamo).or_else(|| split(&COMPOUND_FINALS, jamo)) {
                Some((a, b)) => keys.extend([a, b]),
                None if jamo != '\0' => keys.push(jamo),
                None => {}
            }
        }
    }
    Some(keys)
}

/// Letter-by-letter romanization (한국 is "hangug"), matching the lexicon's transliterated keys.
/// Lone jamo are spelled by their sound.
fn transliterate(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if let Some(code) = (c as u32).checked_sub(0xAC00).filter(|&code| code < 11172) {
            let code = code as usize;
            out.push_str(INITIALS[code / 588]);
            out.push_str(MEDIALS[(code % 588) / 28]);
            out.push_str(FINAL_LETTERS[code % 28]);
        } else if let Some(i) = position(&INITIAL_JAMO, c) {
            out.push_str(INITIALS[i]);
        } else if let Some(i) = position(&MEDIAL_JAMO, c) {
            out.push_str(MEDIALS[i]);
        }
    }
    out
}

/// Final consonants as pronounced at the end of a syllable.
const FINAL_SOUNDS: [&str; 28] = [
    "", "k", "k", "k", "n", "n", "n", "t", "l", "k", "m", "l", "l", "l", "p", "l", "m", "p", "p", "t", "t", "ng", "t", "t", "k", "t", "p",
    "t",
];

/// Revised Romanization of a word with its common sound changes: a final consonant moves onto a
/// following vowel (한국어 is hangugeo), k, t, p become ng, n, m before n or m (감사합니다 is
/// gamsahamnida), and ㄹ next to ㄴ or ㄹ is l (신라 is silla). Other characters are kept.
pub fn romanize(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let parts = |c: char| {
        let code = (c as u32).checked_sub(0xAC00).filter(|&code| code < 11172)? as usize;
        Some((code / 588, (code % 588) / 28, code % 28))
    };
    for (index, &c) in chars.iter().enumerate() {
        let Some((initial, medial, fin)) = parts(c) else {
            out.push(c);
            continue;
        };
        let previous = index.checked_sub(1).and_then(|i| parts(chars[i]));
        let following = chars.get(index + 1).and_then(|&c| parts(c));
        let lead = match previous {
            // The previous final has moved onto this syllable.
            Some((_, _, f)) if initial == 11 && !matches!(f, 0 | 21) => "",
            Some((_, _, f)) if initial == 5 && matches!(FINAL_SOUNDS[f], "l" | "n") => "l",
            _ => INITIALS[initial],
        };
        out.push_str(lead);
        out.push_str(MEDIALS[medial]);
        if fin == 0 {
            continue;
        }
        if following.is_some_and(|f| f.0 == 11) && fin != 21 {
            out.push_str(FINAL_LETTERS[fin]);
            continue;
        }
        let coda = match (FINAL_SOUNDS[fin], following.map(|f| INITIALS[f.0])) {
            ("k", Some("n" | "m")) => "ng",
            ("t", Some("n" | "m")) => "n",
            ("p", Some("n" | "m")) => "m",
            ("n", Some("r")) => "l",
            (sound, _) => sound,
        };
        out.push_str(coda);
    }
    out
}

fn syllables() -> &'static HashMap<String, char> {
    static MAP: OnceLock<HashMap<String, char>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::new();
        let syllable = |code: usize| char::from_u32(0xAC00 + code as u32).unwrap_or('?');
        for (initial, lead) in INITIALS.iter().enumerate() {
            for (medial, vowel) in MEDIALS.iter().enumerate() {
                let open = format!("{lead}{vowel}");
                let base = (initial * 21 + medial) * 28;
                map.entry(open.clone()).or_insert(syllable(base));
                for (fin, spellings) in FINALS.iter().enumerate().skip(1) {
                    for spelling in *spellings {
                        map.entry(format!("{open}{spelling}")).or_insert(syllable(base + fin));
                    }
                }
            }
        }
        // "l" is also typed as an initial ㄹ (e.g. loanwords).
        for (medial, vowel) in MEDIALS.iter().enumerate() {
            map.entry(format!("l{vowel}")).or_insert(syllable((5 * 21 + medial) * 28));
        }
        map
    })
}

/// Splits romanized input into Hangul syllables, preferring fewer syllables and syllables that
/// start with a consonant ("hanguk" = 한국, not 항욱). Returns `None` if the input cannot be parsed.
pub fn compose(input: &str) -> Option<String> {
    let table = syllables();
    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let mut cost = vec![u32::MAX; n + 1];
    let mut previous = vec![0usize; n + 1];
    let mut syllable = vec!['\0'; n + 1];
    cost[0] = 0;
    for i in 0..n {
        if cost[i] == u32::MAX {
            continue;
        }
        for j in i + 1..=n.min(i + LONGEST_SYLLABLE) {
            let piece: String = chars[i..j].iter().collect();
            let Some(&hangul) = table.get(&piece) else { continue };
            let starts_with_vowel = "aeiouwy".contains(chars[i]);
            let next = cost[i] + 10 + u32::from(starts_with_vowel && i > 0);
            if next < cost[j] {
                cost[j] = next;
                previous[j] = i;
                syllable[j] = hangul;
            }
        }
    }
    if cost[n] == u32::MAX {
        return None;
    }
    let mut out = Vec::new();
    let mut at = n;
    while at > 0 {
        out.push(syllable[at]);
        at = previous[at];
    }
    Some(out.iter().rev().collect())
}
