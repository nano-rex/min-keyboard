//! Word completion and spelling correction for languages written with the Latin alphabet
//! (English and Malay). Word lists are ordered most frequent first, so a word's line number is
//! its frequency rank.

use std::collections::HashMap;
use std::io;

use crate::{for_each_line, for_each_line_if_present, Assets, Candidates};

const MAX_CANDIDATES: usize = 32;
const MAX_CORRECTIONS: usize = 6;
const QWERTY_ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];

pub struct LatinEngine {
    /// Display form, by frequency rank.
    words: Vec<String>,
    /// Lower-case forms, alphabetical.
    sorted_keys: Vec<String>,
    /// Rank of `sorted_keys[i]`.
    sorted_ranks: Vec<usize>,
    rank_by_key: HashMap<String, usize>,
    /// `key(words[rank])`, which is always ASCII.
    keys: Vec<String>,
    ranks_by_length: HashMap<usize, Vec<usize>>,
    /// Shortcuts in insertion order; `alias_index` finds one by key.
    aliases: Vec<(String, Vec<String>)>,
    alias_index: HashMap<String, usize>,
}

#[derive(Clone, Copy)]
struct Scored {
    rank: usize,
    distance: f64,
    score: f64,
}

impl Scored {
    fn new(rank: usize, distance: f64) -> Self {
        // Each edit costs about as much as being 20x rarer.
        Scored { rank, distance, score: distance * 3.0 + (rank as f64 + 10.0).ln() }
    }
}

impl LatinEngine {
    fn load(
        assets: &dyn Assets,
        words_asset: &str,
        shortcuts_asset: Option<&str>,
        built_in_aliases: Vec<(String, Vec<String>)>,
    ) -> io::Result<Self> {
        let mut words = Vec::new();
        let mut rank_by_key = HashMap::new();
        for_each_line(assets, words_asset, |line| {
            let word = line.trim();
            if !word.is_empty() && !rank_by_key.contains_key(&key(word)) {
                rank_by_key.insert(key(word), words.len());
                words.push(word.to_string());
            }
        })?;
        let mut sorted_keys: Vec<String> = rank_by_key.keys().cloned().collect();
        sorted_keys.sort();
        let sorted_ranks = sorted_keys.iter().map(|k| rank_by_key[k]).collect();
        let keys: Vec<String> = words.iter().map(|w| key(w)).collect();
        let mut ranks_by_length: HashMap<usize, Vec<usize>> = HashMap::new();
        for (rank, k) in keys.iter().enumerate() {
            ranks_by_length.entry(k.len()).or_default().push(rank);
        }
        let mut engine = LatinEngine {
            words,
            sorted_keys,
            sorted_ranks,
            rank_by_key,
            keys,
            ranks_by_length,
            aliases: Vec::new(),
            alias_index: HashMap::new(),
        };
        for (shortcut, expansions) in built_in_aliases {
            engine.put_alias(shortcut, expansions);
        }
        if let Some(shortcuts) = shortcuts_asset {
            let mut loaded = Vec::new();
            for_each_line_if_present(assets, shortcuts, |line| {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 2 && !parts[0].trim().is_empty() && !parts[1].trim().is_empty() {
                    loaded.push((parts[0].trim().to_lowercase(), vec![parts[1].trim().to_string()]));
                }
            })?;
            for (shortcut, expansions) in loaded {
                engine.put_alias(shortcut, expansions);
            }
        }
        Ok(engine)
    }

    /// Replaces an alias's expansions in place, or appends a new alias.
    fn put_alias(&mut self, shortcut: String, expansions: Vec<String>) {
        match self.alias_index.get(&shortcut) {
            Some(&index) => self.aliases[index].1 = expansions,
            None => {
                self.alias_index.insert(shortcut.clone(), self.aliases.len());
                self.aliases.push((shortcut, expansions));
            }
        }
    }

    fn alias(&self, shortcut: &str) -> Option<&Vec<String>> {
        self.alias_index.get(shortcut).map(|&index| &self.aliases[index].1)
    }

    pub fn english(assets: &dyn Assets) -> io::Result<Self> {
        Self::load(assets, "english_words.txt", Some("english_shortcuts.tsv"), english_aliases())
    }

    pub fn malay(assets: &dyn Assets) -> io::Result<Self> {
        Self::load(assets, "malay_words.txt", Some("malay_shortcuts.tsv"), Vec::new())
    }

    pub fn is_known_word(&self, input: &str) -> bool {
        let k = key(input);
        self.rank_by_key.contains_key(&k) || self.alias(&k).is_some()
    }

    /// Candidates for the word being typed: shortcuts, the best match, the literal input, completions and corrections.
    pub fn candidates(&self, raw_input: &str) -> Vec<String> {
        let input = key(raw_input);
        if input.is_empty() {
            return Vec::new();
        }
        let mut results = Candidates::default();
        if let Some(alias) = self.alias(&input) {
            results.add_all(alias);
        }
        let exact = self.rank_by_key.get(&input);
        let completion_ranks = self.completion_ranks(&input, MAX_CANDIDATES);
        let completions: Vec<&String> = completion_ranks.iter().map(|&rank| &self.words[rank]).collect();
        let scored = self.score_corrections(&input);
        let corrections: Vec<&String> = scored.iter().take(MAX_CORRECTIONS).map(|s| &self.words[s.rank]).collect();

        if let Some(&rank) = exact {
            results.add(&self.words[rank]);
        } else if !scored.is_empty() && !likely_unfinished(&scored[0], &completion_ranks) {
            results.add(corrections[0]);
        } else if let Some(first) = completions.first() {
            results.add(first);
        }
        results.add(raw_input);
        for alternative in self.alias_completions(&input) {
            results.add(alternative);
        }
        // Interleave so both completions and corrections show up in the first few slots.
        let (mut c, mut k) = (0, 0);
        while results.len() < MAX_CANDIDATES && (c < completions.len() || k < corrections.len()) {
            for _ in 0..2 {
                if c < completions.len() {
                    results.add(completions[c]);
                    c += 1;
                }
            }
            if k < corrections.len() {
                results.add(corrections[k]);
                k += 1;
            }
        }
        results.into_vec(MAX_CANDIDATES)
    }

    /// The word to substitute when the user finishes an unknown word, or `None` to leave it alone.
    /// Only confident fixes are returned: the typed word must be unknown and a close, common word must exist.
    pub fn auto_correction(&self, raw_input: &str) -> Option<String> {
        let input = key(raw_input);
        if let Some(&rank) = self.rank_by_key.get(&input) {
            // Known word: only fix capitalisation of words that are always capitalised ("i", "monday").
            let word = &self.words[rank];
            return (word != raw_input && *word != word.to_lowercase() && raw_input == input).then(|| word.clone());
        }
        if let Some(alias) = self.alias(&input) {
            // Missing apostrophes: "dont" -> "don't", "im" -> "I'm". Multi-word shortcuts stay as suggestions.
            let first = &alias[0];
            return (key(&first.replace('\'', "")) == input.replace('\'', "")).then(|| first.clone());
        }
        if input.len() < 2 {
            return None;
        }
        let best = *self.score_corrections(&input).first()?;
        let limit = if input.len() <= 4 { 1.0 } else { 2.0 };
        if best.distance > limit || likely_unfinished(&best, &self.completion_ranks(&input, 1)) {
            return None;
        }
        Some(self.words[best.rank].clone())
    }

    /// Frequency ranks of words that start with the prefix (excluding the prefix itself), most frequent first.
    fn completion_ranks(&self, prefix: &str, limit: usize) -> Vec<usize> {
        let start = self.sorted_keys.partition_point(|k| k.as_str() < prefix);
        let mut ranks: Vec<usize> = self.sorted_keys[start..]
            .iter()
            .zip(&self.sorted_ranks[start..])
            .take_while(|(k, _)| k.starts_with(prefix))
            .filter(|(k, _)| k.as_str() != prefix)
            .map(|(_, &rank)| rank)
            .collect();
        ranks.sort_unstable();
        ranks.truncate(limit);
        ranks
    }

    fn alias_completions(&self, input: &str) -> Vec<&String> {
        self.aliases
            .iter()
            .filter(|(k, _)| k != input && k.starts_with(input) && k.len() <= input.len() + 2)
            .flat_map(|(_, values)| values)
            .collect()
    }

    fn score_corrections(&self, input: &str) -> Vec<Scored> {
        let max_distance = if input.len() <= 4 { 1.0 } else { 2.0 };
        let mut scored = Vec::new();
        let mut rows = Rows::default();
        let letters = letter_counts(input);
        for length in input.len().saturating_sub(2).max(1)..=input.len() + 2 {
            let Some(ranks) = self.ranks_by_length.get(&length) else { continue };
            for &rank in ranks {
                let candidate = &self.keys[rank];
                if candidate == input || letter_bound(&letters, candidate) > max_distance {
                    continue;
                }
                let d = damerau(input.as_bytes(), candidate.as_bytes(), max_distance, |x, y| adjacent(x as char, y as char), &mut rows);
                if d <= max_distance {
                    scored.push(Scored::new(rank, d));
                }
            }
        }
        scored.sort_by(|a, b| a.score.total_cmp(&b.score));
        scored
    }
}

/// True when the input looks like the start of a common word rather than a typo: some
/// completion is no more than 20x rarer than the best correction ("hel" -> help, not he).
fn likely_unfinished(correction: &Scored, completion_ranks: &[usize]) -> bool {
    completion_ranks.first().is_some_and(|&first| first < (correction.rank + 10) * 20)
}

/// Damerau-Levenshtein distance where substituting a neighbouring key costs less. Stops early above `limit`.
pub fn distance(a: &str, b: &str, limit: f64) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    damerau(&a, &b, limit, adjacent, &mut Rows::default())
}

/// How often each key character (a-z, apostrophe, hyphen) occurs.
fn letter_counts(key: &str) -> [i16; 28] {
    let mut counts = [0i16; 28];
    for b in key.bytes() {
        counts[letter_index(b)] += 1;
    }
    counts
}

fn letter_index(b: u8) -> usize {
    match b {
        b'a'..=b'z' => (b - b'a') as usize,
        b'\'' => 26,
        _ => 27,
    }
}

/// A lower bound on [`distance`] from letter counts alone, to skip most of the dictionary cheaply.
/// An insertion or deletion (cost 1) changes the summed count difference by 1, a substitution
/// (cost at least 0.6) by at most 2 and a transposition by 0, so the distance is at least 0.3
/// times that difference. Words above the limit by this bound are above it by `distance` too.
fn letter_bound(input: &[i16; 28], candidate: &str) -> f64 {
    let mut counts = *input;
    for b in candidate.bytes() {
        counts[letter_index(b)] -= 1;
    }
    0.3 * counts.iter().map(|c| c.unsigned_abs() as u32).sum::<u32>() as f64
}

/// The three most recent rows of the distance table, reused between calls.
#[derive(Default)]
struct Rows {
    before: Vec<f64>,
    previous: Vec<f64>,
    current: Vec<f64>,
}

fn damerau<T: Copy + PartialEq>(a: &[T], b: &[T], limit: f64, adjacent: impl Fn(T, T) -> bool, rows: &mut Rows) -> f64 {
    let (n, m) = (a.len(), b.len());
    if (n as f64 - m as f64).abs() > limit {
        return limit + 1.0;
    }
    // Row i of the table only needs rows i-1 and i-2 (for transpositions).
    let Rows { before, previous, current } = rows;
    for row in [&mut *before, &mut *previous, &mut *current] {
        row.clear();
        row.resize(m + 1, 0.0);
    }
    for (j, cell) in previous.iter_mut().enumerate() {
        *cell = j as f64;
    }
    for i in 1..=n {
        current[0] = i as f64;
        let mut row_min = f64::MAX;
        for j in 1..=m {
            let (x, y) = (a[i - 1], b[j - 1]);
            let substitution = if x == y {
                0.0
            } else if adjacent(x, y) {
                0.6
            } else {
                1.0
            };
            let mut value = (previous[j] + 1.0).min(current[j - 1] + 1.0).min(previous[j - 1] + substitution);
            if i > 1 && j > 1 && x == b[j - 2] && a[i - 2] == y {
                value = value.min(before[j - 2] + 0.8);
            }
            current[j] = value;
            row_min = row_min.min(value);
        }
        if row_min > limit {
            return limit + 1.0;
        }
        std::mem::swap(before, previous);
        std::mem::swap(previous, current);
    }
    previous[m]
}

fn adjacent(a: char, b: char) -> bool {
    match (key_position(a), key_position(b)) {
        (Some((ra, ca)), Some((rb, cb))) => ra.abs_diff(rb) <= 1 && ca.abs_diff(cb) <= 1,
        _ => false,
    }
}

fn key_position(c: char) -> Option<(usize, usize)> {
    static POSITIONS: std::sync::OnceLock<[Option<(usize, usize)>; 128]> = std::sync::OnceLock::new();
    let positions = POSITIONS.get_or_init(|| {
        let mut table = [None; 128];
        for (row, keys) in QWERTY_ROWS.iter().enumerate() {
            for (column, k) in keys.bytes().enumerate() {
                table[k as usize] = Some((row, column));
            }
        }
        table
    });
    positions.get(c as usize).copied().flatten()
}

/// Lower-cases and keeps only letters, apostrophes and hyphens.
pub fn key(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).filter(|&ch| ch.is_ascii_lowercase() || ch == '\'' || ch == '-').collect()
}

fn english_aliases() -> Vec<(String, Vec<String>)> {
    let pairs: &[&[&str]] = &[
        &["im", "I'm"],
        &["ive", "I've"],
        &["ill", "I'll"],
        &["id", "I'd"],
        &["youre", "you're"],
        &["youve", "you've"],
        &["youll", "you'll"],
        &["youd", "you'd"],
        &["hes", "he's"],
        &["hed", "he'd"],
        &["hell", "he'll"],
        &["shes", "she's"],
        &["shed", "she'd"],
        &["shell", "she'll"],
        &["its", "it's", "its"],
        &["itd", "it'd"],
        &["itll", "it'll"],
        &["thats", "that's"],
        &["theres", "there's"],
        &["heres", "here's"],
        &["whats", "what's"],
        &["whos", "who's"],
        &["wheres", "where's"],
        &["whens", "when's"],
        &["whys", "why's"],
        &["hows", "how's"],
        &["dont", "don't"],
        &["doesnt", "doesn't"],
        &["didnt", "didn't"],
        &["cant", "can't"],
        &["couldnt", "couldn't"],
        &["wouldnt", "wouldn't"],
        &["shouldnt", "shouldn't"],
        &["wont", "won't"],
        &["isnt", "isn't"],
        &["arent", "aren't"],
        &["wasnt", "wasn't"],
        &["werent", "weren't"],
        &["hasnt", "hasn't"],
        &["havent", "haven't"],
        &["hadnt", "hadn't"],
        &["mustnt", "mustn't"],
        &["neednt", "needn't"],
        &["mightnt", "mightn't"],
        &["mustve", "must've"],
        &["couldve", "could've"],
        &["wouldve", "would've"],
        &["shouldve", "should've"],
        &["theyre", "they're"],
        &["theyve", "they've"],
        &["theyll", "they'll"],
        &["theyd", "they'd"],
        &["weve", "we've"],
        &["were", "we're", "were"],
        &["well", "we'll", "well"],
        &["wed", "we'd"],
        &["lets", "let's", "lets"],
        &["thered", "there'd"],
        &["therell", "there'll"],
        // Keep the core shortcuts available even if an OEM build omits optional assets.
        &["wdym", "what do you mean"],
        &["idk", "I don't know"],
        &["btw", "by the way"],
        &["omw", "on my way"],
        &["asap", "as soon as possible"],
    ];
    pairs.iter().map(|pair| (pair[0].to_string(), pair[1..].iter().map(|s| s.to_string()).collect())).collect()
}
