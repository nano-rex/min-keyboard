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
            let (count, text) = self.segment(piece, mode, p == pieces.len() - 1)?;
            result.push_str(&text);
            segments += count;
        }
        ((segments >= 2 || input.contains('\'')) && !result.is_empty()).then_some(result)
    }

    /// The cheapest split of one apostrophe-free piece: (word count, joined top candidates).
    fn segment(&self, piece: &str, mode: ScriptMode, allow_prefix_at_end: bool) -> Option<(usize, String)> {
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
