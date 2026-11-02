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
