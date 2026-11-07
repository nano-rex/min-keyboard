//! Builds the frequency-ranked runtime dictionaries in `assets/`.
//!
//! Every subcommand reads source files given on the command line (see
//! docs/dictionary-sources.md for where to download them) and writes one compact asset:
//!
//! ```text
//! dicttool english --frequency en_full.txt --spelling /usr/share/dict/american-english
//! dicttool malay --frequency ms_full.txt
//! dicttool pinyin-sources --cedict cedict_1_0_ts_utf-8_mdbg.txt.gz --fcitx-simplified zhwiki.dict.yaml \
//!     --opencc-characters TSCharacters.txt --opencc-phrases TSPhrases.txt
//! dicttool chinese --jieba dict.txt --frequency zh_cn_full.txt \
//!     --opencc-characters TSCharacters.txt --opencc-phrases TSPhrases.txt
//! dicttool japanese --skk SKK-JISYO.L --frequency ja_full.txt
//! dicttool korean --frequency ko_full.txt
//! ```
//!
//! Word lists are written most-frequent first; the keyboard uses the line number as the
//! frequency rank. `pinyin-sources` merges the pinyin lexicon from its sources; run `chinese`
//! afterwards to rank it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{exit, Command, Stdio};

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("dicttool: {message}");
    exit(1)
}

fn assets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

// ------------------------------------------------------------------- input ----

/// Splits lines like Python's `str.splitlines()`.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            lines.push(&text[start..i]);
            let mut end = i + c.len_utf8();
            if c == '\r' && chars.peek().map(|&(_, n)| n) == Some('\n') {
                chars.next();
                end += 1;
            }
            start = end;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn read_text(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|e| fail(format!("cannot read {}: {e}", path.display())));
    String::from_utf8(bytes).unwrap_or_else(|_| fail(format!("{} is not UTF-8", path.display())))
}

/// Reads text, dropping bytes that are not valid UTF-8 (Python's `errors='ignore'`).
fn read_text_lossy(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|e| fail(format!("cannot read {}: {e}", path.display())));
    String::from_utf8_lossy(&bytes).chars().filter(|&c| c != '\u{FFFD}').collect()
}

/// Runs a system tool that turns a file into UTF-8 text on stdout.
fn decode_with(program: &str, args: &[&str], path: &Path) -> String {
    let output = Command::new(program)
        .args(args)
        .arg(path)
        .stderr(Stdio::inherit())
        .output()
        .unwrap_or_else(|e| fail(format!("cannot run {program}: {e}")));
    if !output.status.success() {
        fail(format!("{program} could not decode {}", path.display()));
    }
    String::from_utf8(output.stdout).unwrap_or_else(|_| fail(format!("{program} produced invalid UTF-8")))
}

/// Words and counts in file order, like a Python dict.
#[derive(Default)]
struct Counts {
    items: Vec<(String, u64)>,
    index: HashMap<String, usize>,
}

impl Counts {
    fn add(&mut self, word: &str, count: u64) {
        match self.index.get(word) {
            Some(&i) => self.items[i].1 += count,
            None => {
                self.index.insert(word.to_string(), self.items.len());
                self.items.push((word.to_string(), count));
            }
        }
    }

    fn get(&self, word: &str) -> u64 {
        self.index.get(word).map(|&i| self.items[i].1).unwrap_or(0)
    }

    /// Most frequent first; ties keep file order.
    fn by_frequency(&self) -> Vec<(&str, u64)> {
        let mut sorted: Vec<(&str, u64)> = self.items.iter().map(|(w, c)| (w.as_str(), *c)).collect();
        sorted.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
        sorted
    }
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// Reads a `word count` list (FrequencyWords format).
fn read_frequency(path: &Path, minimum: u64) -> Counts {
    let mut counts = Counts::default();
    for line in split_lines(&read_text(path)) {
        let Some((word, count)) = line.rsplit_once(' ') else { continue };
        if !is_digits(count) {
            continue;
        }
        let count: u64 = count.parse().unwrap_or(0);
        if count >= minimum {
            counts.add(word, count);
        }
    }
    counts
}

fn write_lines(name: &str, lines: &[String]) {
    let path = assets_dir().join(name);
    let mut text = lines.join("\n");
    text.push('\n');
    fs::write(&path, &text).unwrap_or_else(|e| fail(format!("cannot write {}: {e}", path.display())));
    println!("wrote assets/{name}: {} lines, {} KiB", lines.len(), text.len() / 1024);
}

// ----------------------------------------------------------------- options ----

struct Args {
    values: HashMap<String, String>,
}

impl Args {
    fn parse(args: &[String], allowed: &[&str]) -> Args {
        let mut values = HashMap::new();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let Some(name) = arg.strip_prefix("--") else { fail(format!("unexpected argument {arg}")) };
            if !allowed.contains(&name) {
                fail(format!("unknown option --{name}; expected one of --{}", allowed.join(", --")));
            }
            let value = iter.next().unwrap_or_else(|| fail(format!("--{name} needs a value")));
            values.insert(name.to_string(), value.clone());
        }
        Args { values }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.values.get(name).map(PathBuf::from).unwrap_or_else(|| fail(format!("--{name} is required")))
    }

    fn optional_path(&self, name: &str) -> Option<PathBuf> {
        self.values.get(name).map(PathBuf::from)
    }

    fn number(&self, name: &str, default: u64) -> u64 {
        self.values.get(name).map(|v| v.parse().unwrap_or_else(|_| fail(format!("--{name} must be a number")))).unwrap_or(default)
    }
}

// ----------------------------------------------------------- English, Malay ----

/// `^[a-z]+(?:['-][a-z]+)*$`
fn is_latin_word(word: &str) -> bool {
    !word.is_empty() && word.split(['\'', '-']).all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase()))
}

fn always_capitalized(word: &str) -> Option<&'static str> {
    match word {
        "i" => Some("I"),
        "i'm" => Some("I'm"),
        "i'll" => Some("I'll"),
        "i'd" => Some("I'd"),
        "i've" => Some("I've"),
        _ => None,
    }
}

fn build_english(args: &Args) {
    let mut spelling: HashMap<String, String> = HashMap::new();
    for line in split_lines(&read_text_lossy(&args.path("spelling"))) {
        let word = line.trim();
        if word.is_empty() || !word.is_ascii() {
            continue;
        }
        let lower = word.to_lowercase();
        if lower.ends_with("'s") {
            continue; // possessives are produced by typing, not suggested
        }
        // Prefer the lower-case spelling when both "polish" and "Polish" exist.
        if !spelling.contains_key(&lower) || word == lower {
            spelling.insert(lower, word.to_string());
        }
    }
    let counts = read_frequency(&args.path("frequency"), 3);
    let (limit, extra, slang) =
        (args.number("limit", 80000) as usize, args.number("extra", 20000) as usize, args.number("slang-threshold", 20000));
    let mut ranked = Vec::new();
    let mut seen = HashSet::new();
    for (word, count) in counts.by_frequency() {
        if !is_latin_word(word) || seen.contains(word) {
            continue;
        }
        if let Some(capitalized) = always_capitalized(word) {
            ranked.push(capitalized.to_string());
        } else if let Some(spelled) = spelling.get(word) {
            ranked.push(spelled.clone());
        } else if count >= slang && word.len() > 1 {
            ranked.push(word.to_string()); // frequent informal words such as "gonna" or "lol"
        } else {
            continue;
        }
        seen.insert(word.to_string());
        if ranked.len() >= limit {
            break;
        }
    }
    // Keep dictionary words that never appeared in subtitles, after the ranked ones.
    let alphabetical: BTreeMap<&String, &String> = spelling.iter().collect();
    for (lower, word) in alphabetical {
        if ranked.len() >= limit + extra {
            break;
        }
        if !seen.contains(lower.as_str()) && is_latin_word(lower) && (2..=20).contains(&lower.len()) {
            ranked.push(word.clone());
            seen.insert(lower.clone());
        }
    }
    write_lines("english_words.txt", &ranked);
}

fn build_malay(args: &Args) {
    let counts = read_frequency(&args.path("frequency"), args.number("minimum", 3));
    let ranked: Vec<String> = counts
        .by_frequency()
        .into_iter()
        .filter(|(word, _)| is_latin_word(word) && (word.chars().count() > 1 || *word == "a" || *word == "i"))
        .map(|(word, _)| word.to_string())
        .take(args.number("limit", 60000) as usize)
        .collect();
    write_lines("malay_words.txt", &ranked);
}

// ----------------------------------------------------------------- Chinese ----

/// OpenCC phrase and character tables, converting the way OpenCC's simple mode does.
struct OpenCc {
    characters: HashMap<char, String>,
    phrases: HashMap<String, String>,
}

impl OpenCc {
    fn load(characters: Option<&Path>, phrases: Option<&Path>) -> Self {
        let table = |path: Option<&Path>| -> HashMap<String, String> {
            let Some(path) = path else { return HashMap::new() };
            let mut map = HashMap::new();
            for line in split_lines(&read_text(path)) {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((source, values)) = line.split_once('\t') else { continue };
                if let Some(first) = values.split_whitespace().next() {
                    map.insert(source.to_string(), first.to_string());
                }
            }
            map
        };
        let characters = table(characters)
            .into_iter()
            .filter_map(|(source, target)| {
                let mut chars = source.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Some((c, target)),
                    _ => None,
                }
            })
            .collect();
        OpenCc { characters, phrases: table(phrases) }
    }

    fn convert(&self, text: &str) -> String {
        if let Some(phrase) = self.phrases.get(text) {
            return phrase.clone();
        }
        text.chars().map(|c| self.characters.get(&c).cloned().unwrap_or_else(|| c.to_string())).collect()
    }
}

/// Candidates kept per pinyin key, most frequent first.
const MAX_PER_KEY: usize = 96;

fn build_chinese(args: &Args) {
    let mut scores: HashMap<String, f64> = HashMap::new();
    let mut raise = |word: &str, score: f64| {
        let entry = scores.entry(word.to_string()).or_insert(0.0);
        *entry = entry.max(score);
    };
    for line in split_lines(&read_text(&args.path("jieba"))) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && is_digits(parts[1]) {
            raise(parts[0], parts[1].parse().unwrap_or(0.0));
        }
    }
    for (word, count) in &read_frequency(&args.path("frequency"), 1).items {
        raise(word, *count as f64);
    }
    let opencc = OpenCc::load(Some(&args.path("opencc-characters")), Some(&args.path("opencc-phrases")));
    // Frequent words first; unseen words keep their previous relative order.
    let rank = |candidates: Vec<&str>, convert: bool| -> Vec<String> {
        let mut keyed: Vec<(f64, usize, &str)> = candidates
            .into_iter()
            .enumerate()
            .map(|(index, word)| {
                let lookup = if convert { opencc.convert(word) } else { word.to_string() };
                (-scores.get(&lookup).copied().unwrap_or(0.0).ln_1p(), index, word)
            })
            .collect();
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        keyed.into_iter().map(|(_, _, word)| word.to_string()).collect()
    };
    let mut lines = Vec::new();
    for line in split_lines(&read_text(&assets_dir().join("pinyin_lexicon.tsv"))) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        // Rare words are dropped only now that the list is ranked.
        let mut simplified = rank(parts[1].split('|').filter(|v| !v.is_empty()).collect(), false);
        let mut traditional = rank(parts[2].split('|').filter(|v| !v.is_empty()).collect(), true);
        simplified.truncate(MAX_PER_KEY);
        traditional.truncate(MAX_PER_KEY);
        // The fourth column is the top candidate's frequency, used to segment whole sentences.
        let top = simplified.first().and_then(|w| scores.get(w)).copied().unwrap_or(0.0) as u64;
        lines.push(format!("{}\t{}\t{}\t{top}", parts[0], simplified.join("|"), traditional.join("|")));
    }
    add_variants(&mut lines);
    write_lines("pinyin_lexicon.tsv", &lines);
}

/// Words written with characters that CC-CEDICT files as Traditional forms only, though they are
/// also used in Simplified text: (pinyin, (word, the word it follows) in Simplified and in Traditional).
type Variant = (&'static str, (&'static str, &'static str), (&'static str, &'static str));
const VARIANTS: &[Variant] = &[
    ("ni", ("妳", "呢"), ("妳", "呢")),
    ("nimen", ("妳们", "你们"), ("妳們", "你們")),
    ("nihao", ("妳好", "你好"), ("妳好", "你好")),
    ("ta", ("牠", "它"), ("牠", "它")),
    ("tamen", ("牠们", "它们"), ("牠們", "它們")),
];

/// Adds [`VARIANTS`] to the lexicon lines (`pinyin<TAB>simplified<TAB>traditional[<TAB>frequency]`).
/// Only existing keys are extended: a new key would stop the input from being segmented ("nide"
/// would no longer give 你的).
fn add_variants(lines: &mut [String]) {
    fn insert(column: &str, word: &str, after: &str) -> String {
        let mut words: Vec<&str> = column.split('|').filter(|w| !w.is_empty()).collect();
        if !words.contains(&word) {
            let at = words.iter().position(|w| *w == after).map_or(words.len(), |i| i + 1);
            words.insert(at, word);
        }
        words.join("|")
    }
    for &(key, (simplified, after_simplified), (traditional, after_traditional)) in VARIANTS {
        let Some(line) = lines.iter_mut().find(|line| line.split('\t').next() == Some(key)) else { continue };
        let mut columns: Vec<String> = line.split('\t').map(str::to_string).collect();
        if columns.len() < 3 {
            continue;
        }
        columns[1] = insert(&columns[1], simplified, after_simplified);
        columns[2] = insert(&columns[2], traditional, after_traditional);
        *line = columns.join("\t");
    }
}

/// Adds the [`VARIANTS`] to the shipped lexicon without rebuilding it.
fn build_pinyin_variants() {
    let text = read_text(&assets_dir().join("pinyin_lexicon.tsv"));
    let mut lines: Vec<String> = split_lines(&text).into_iter().map(str::to_string).collect();
    add_variants(&mut lines);
    write_lines("pinyin_lexicon.tsv", &lines);
}

/// Splits a CC-CEDICT line into (traditional, simplified, pinyin), like
/// `^(\S+)\s+(\S+)\s+\[([^\]]+)\]`.
fn parse_cedict(line: &str) -> Option<(&str, &str, &str)> {
    let field = |text: &str| -> Option<(usize, usize)> {
        let end = text.find(char::is_whitespace)?;
        let next = text[end..].find(|c: char| !c.is_whitespace())? + end;
        (end > 0).then_some((end, next))
    };
    let (end, next) = field(line)?;
    let (traditional, rest) = (&line[..end], &line[next..]);
    let (end, next) = field(rest)?;
    let (simplified, rest) = (&rest[..end], &rest[next..]);
    let pinyin = rest.strip_prefix('[')?.split(']').next()?;
    (!pinyin.is_empty() && rest[1 + pinyin.len()..].starts_with(']')).then_some((traditional, simplified, pinyin))
}

/// Merges CC-CEDICT and a Fcitx dictionary into the pinyin lexicon (unranked, and with every
/// candidate; run `chinese` next to rank them and keep the most frequent).
fn build_pinyin_sources(args: &Args) {
    const MAX_FCITX_WORDS: usize = 100_000;

    #[derive(Default)]
    struct Bucket {
        simplified: Vec<String>,
        traditional: Vec<String>,
        seen: HashSet<(bool, String)>,
    }
    impl Bucket {
        fn add(&mut self, traditional: bool, value: &str) {
            if !value.is_empty() && self.seen.insert((traditional, value.to_string())) {
                if traditional { &mut self.traditional } else { &mut self.simplified }.push(value.to_string());
            }
        }
    }

    /// Lower-case letters and apostrophes, with ü typed as v ("Lu:4 se4" -> "lvse").
    fn normalize(pinyin: &str) -> String {
        pinyin.to_lowercase().replace("u:", "v").replace('ü', "v").chars().filter(|&c| c.is_ascii_lowercase() || c == '\'').collect()
    }

    let mut buckets: BTreeMap<String, Bucket> = BTreeMap::new();
    let out = assets_dir().join("pinyin_lexicon.tsv");
    if out.exists() {
        for line in split_lines(&read_text(&out)) {
            let columns: Vec<&str> = line.split('\t').collect();
            if columns.len() < 3 {
                continue;
            }
            let bucket = buckets.entry(columns[0].to_string()).or_default();
            for value in columns[1].split('|') {
                bucket.add(false, value);
            }
            for value in columns[2].split('|') {
                bucket.add(true, value);
            }
        }
    }
    if let Some(path) = args.optional_path("cedict") {
        let text = if path.extension().is_some_and(|e| e == "gz") { decode_with("gzip", &["-dc"], &path) } else { read_text(&path) };
        for line in split_lines(&text) {
            // `traditional simplified [pin1 yin1] /definition/`
            let Some((traditional, simplified, inner)) = parse_cedict(line) else { continue };
            let key = normalize(inner);
            if !key.is_empty() {
                let bucket = buckets.entry(key).or_default();
                bucket.add(false, simplified);
                bucket.add(true, traditional);
            }
        }
    }
    let opencc = OpenCc::load(args.optional_path("opencc-characters").as_deref(), args.optional_path("opencc-phrases").as_deref());
    if let Some(path) = args.optional_path("fcitx-simplified") {
        let mut loaded = 0;
        for line in split_lines(&read_text(&path)) {
            if line.is_empty() || line.starts_with('#') || line.starts_with("---") || line.starts_with("...") {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 2 || !fields[1].chars().any(char::is_alphabetic) {
                continue;
            }
            let (word, key) = (fields[0], normalize(&fields[1..].join(" ")));
            if key.is_empty() || !word.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
                continue;
            }
            let bucket = buckets.entry(key).or_default();
            bucket.add(false, word);
            bucket.add(true, &opencc.convert(word));
            loaded += 1;
            if loaded >= MAX_FCITX_WORDS {
                break;
            }
        }
    }
    // Nothing is dropped here: cutting an unranked list would lose common characters such as 西
    // and 间, which sort late.
    let longest_first = |values: &mut Vec<String>| {
        values.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()).then_with(|| a.cmp(b)));
        values.join("|")
    };
    let lines: Vec<String> = buckets
        .iter_mut()
        .map(|(key, bucket)| format!("{key}\t{}\t{}", longest_first(&mut bucket.simplified), longest_first(&mut bucket.traditional)))
        .collect();
    write_lines("pinyin_lexicon.tsv", &lines);
}

/// Writes `pinyin_readings.tsv`: each word in the pinyin lexicon (both scripts) and its pinyin
/// with tone marks from CC-CEDICT, for showing the pronunciation of selected text. A single
/// character with several readings gets the one it has in most CC-CEDICT words (说 is shuō, as
/// in 说话 and 小说); a longer word gets the reading under which the lexicon ranks it highest.
/// Grammar particles, which words rarely contain, are set by hand.
fn build_pinyin_readings(args: &Args) {
    // Where each word ranks under each toneless key: (key, word) -> position.
    let mut rank: HashMap<(String, String), usize> = HashMap::new();
    for line in split_lines(&read_text(&assets_dir().join("pinyin_lexicon.tsv"))) {
        let columns: Vec<&str> = line.split('\t').collect();
        if columns.len() < 3 {
            continue;
        }
        for column in &columns[1..3] {
            for (i, word) in column.split('|').enumerate() {
                rank.entry((columns[0].to_string(), word.to_string())).or_insert(i);
            }
        }
    }
    let path = args.path("cedict");
    let text = if path.extension().is_some_and(|e| e == "gz") { decode_with("gzip", &["-dc"], &path) } else { read_text(&path) };
    // How often each character is read each way inside longer words.
    let mut uses: HashMap<(char, String), usize> = HashMap::new();
    for line in split_lines(&text) {
        let Some((traditional, simplified, pinyin)) = parse_cedict(line) else { continue };
        let syllables: Vec<&str> = pinyin.split_whitespace().collect();
        for word in [simplified, traditional] {
            let chars: Vec<char> = word.chars().collect();
            if chars.len() < 2 || chars.len() != syllables.len() || (word == traditional && traditional == simplified) {
                continue;
            }
            for (c, syllable) in chars.iter().zip(&syllables) {
                if let Some(reading) = tone_marks(syllable) {
                    *uses.entry((*c, reading)).or_default() += 1;
                }
            }
        }
    }
    // word -> (lexicon rank or, for a character, minus its uses; is a proper noun; reading)
    let mut best: BTreeMap<String, (i64, bool, String)> = BTreeMap::new();
    for line in split_lines(&text) {
        let Some((traditional, simplified, pinyin)) = parse_cedict(line) else { continue };
        let key: String = pinyin.to_lowercase().replace("u:", "v").chars().filter(|c| c.is_ascii_lowercase()).collect();
        let Some(reading) = tone_marks(pinyin) else { continue };
        let proper = pinyin.starts_with(|c: char| c.is_ascii_uppercase());
        for word in [simplified, traditional] {
            let Some(&position) = rank.get(&(key.clone(), word.to_string())) else { continue };
            let mut chars = word.chars();
            let order = match (chars.next(), chars.next()) {
                (Some(c), None) => -(uses.get(&(c, reading.clone())).copied().unwrap_or(0) as i64),
                _ => position as i64,
            };
            let candidate = (order, proper, reading.clone());
            match best.get(word) {
                Some(current) if (current.0, current.1) <= (candidate.0, candidate.1) => {}
                _ => _ = best.insert(word.to_string(), candidate),
            }
        }
    }
    // Particles, and characters whose everyday reading is rare inside longer words.
    let set_by_hand = [
        ("了", "le"),
        ("的", "de"),
        ("着", "zhe"),
        ("著", "zhe"),
        ("得", "de"),
        ("吗", "ma"),
        ("嗎", "ma"),
        ("呢", "ne"),
        ("吧", "ba"),
        ("么", "me"),
        ("麼", "me"),
        ("还", "hái"),
        ("還", "hái"),
        ("都", "dōu"),
        ("更", "gèng"),
        ("角", "jiǎo"),
        ("解", "jiě"),
        ("将", "jiāng"),
        ("將", "jiāng"),
    ];
    for (word, reading) in set_by_hand {
        best.insert(word.to_string(), (0, false, reading.to_string()));
    }
    let lines: Vec<String> = best.into_iter().map(|(word, (_, _, reading))| format!("{word}\t{reading}")).collect();
    write_lines("pinyin_readings.tsv", &lines);
}

/// "ni3 hao3" -> "nǐhǎo", "Xi1 an1" -> "xī'ān"; `None` for readings that are not plain pinyin.
fn tone_marks(pinyin: &str) -> Option<String> {
    let mut out = String::new();
    for syllable in pinyin.split_whitespace() {
        let syllable = syllable.to_lowercase().replace("u:", "ü");
        let (letters, tone) = match syllable.char_indices().last()? {
            (i, c @ '1'..='5') => (&syllable[..i], c as u32 - '0' as u32),
            _ => return None,
        };
        if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_lowercase() || c == 'ü') {
            return None;
        }
        // The mark goes on a or e, on the o of "ou", or else on the last vowel.
        let vowels: Vec<(usize, char)> = letters.char_indices().filter(|(_, c)| "aeiouü".contains(*c)).collect();
        let at = vowels
            .iter()
            .find(|(_, c)| *c == 'a' || *c == 'e')
            .or_else(|| letters.find("ou").and_then(|i| vowels.iter().find(|(j, _)| *j == i)))
            .or(vowels.last())
            .map(|&(i, _)| i);
        if !out.is_empty() && letters.starts_with(['a', 'e', 'o']) {
            out.push('\''); // xī'ān, not xīān
        }
        for (i, c) in letters.char_indices() {
            if Some(i) == at && (1..=4).contains(&tone) {
                let marks = match c {
                    'a' => "āáǎà",
                    'e' => "ēéěè",
                    'i' => "īíǐì",
                    'o' => "ōóǒò",
                    'u' => "ūúǔù",
                    _ => "ǖǘǚǜ",
                };
                out.push(marks.chars().nth(tone as usize - 1)?);
            } else {
                out.push(c);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

// ---------------------------------------------------------------- Japanese ----

/// `^[ぁ-ゖー]+$`
fn is_hiragana(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| ('ぁ'..='ゖ').contains(&c) || c == 'ー')
}

/// SKK dictionaries are EUC-JIS-2004; UTF-8 copies are accepted as they are.
fn read_skk(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|e| fail(format!("cannot read {}: {e}", path.display())));
    match String::from_utf8(bytes) {
        Ok(text) => text,
        // glibc decodes 0xA1BD as U+2014; Python's euc_jis_2004 codec, which built the shipped
        // lexicon, gives U+2015. Keep the shipped form so rebuilds do not churn.
        Err(_) => decode_with("iconv", &["-f", "EUC-JISX0213", "-t", "UTF-8"], path).replace('\u{2014}', "\u{2015}"),
    }
}

/// Turns SKK okuri-nasi entries into `reading<TAB>candidate|candidate<TAB>score` lines.
fn build_japanese(args: &Args) {
    let frequency = read_frequency(&args.path("frequency"), 1);
    let raw = read_skk(&args.path("skk"));
    let (max_reading, per_reading, kana_minimum) =
        (args.number("max-reading", 12) as usize, args.number("per-reading", 16) as usize, args.number("kana-minimum", 20));
    let mut entries: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut okuri_nasi = false;
    for line in split_lines(&raw) {
        if line.starts_with(";; okuri-nasi entries") {
            okuri_nasi = true;
            continue;
        }
        if !okuri_nasi || line.is_empty() || line.starts_with(';') {
            continue;
        }
        let (reading, rest) = line.split_once(" /").unwrap_or((line, ""));
        if !is_hiragana(reading) || reading.chars().count() > max_reading {
            continue;
        }
        let mut candidates: Vec<String> = Vec::new();
        for item in rest.split('/') {
            let word = item.split(';').next().unwrap_or("").trim();
            if word.is_empty() || word.starts_with('(') || word.contains('\u{FFFD}') || candidates.iter().any(|c| c == word) {
                continue;
            }
            candidates.push(word.to_string());
        }
        if !candidates.is_empty() {
            entries.insert(reading.to_string(), candidates);
        }
    }
    // Common kana-only words from subtitles are kept so they can be completed by prefix.
    for (word, count) in &frequency.items {
        if is_hiragana(word) && *count >= kana_minimum && word.chars().count() > 1 {
            entries.entry(word.clone()).or_default();
        }
    }
    let lines: Vec<String> = entries
        .iter()
        .map(|(reading, candidates)| {
            // SKK lists candidates in a curated order. Subtitle counts are not used to reorder
            // them because they ignore readings (上 is common, but rarely read かみ).
            let candidates = &candidates[..candidates.len().min(per_reading)];
            // The score ranks readings against each other when completing a prefix.
            let score = frequency.get(reading).max(candidates.first().map(|w| frequency.get(w)).unwrap_or(0));
            format!("{reading}\t{}\t{score}", candidates.join("|"))
        })
        .collect();
    write_lines("japanese_lexicon.tsv", &lines);
}

/// Writes `japanese_readings.tsv`: words with kanji from the shipped lexicon and their kana
/// reading, for showing the pronunciation of selected text. A word listed under several readings
/// gets the one that lists it first; among those, the reading with more words, which is usually
/// the everyday one (日本 is listed first under にほん, にっぽん and じっぽん).
///
/// With `--skk`, kanji written with a kana ending are added from SKK's okuri-ari entries, keyed
/// by the kanji and the ending's first letter: `食b` is た (食べる), `行k` is い (行く).
fn build_japanese_readings(args: &Args) {
    let per_reading = args.number("per-reading", 4) as usize;
    let mut best: BTreeMap<String, (usize, i64, i64, String)> = BTreeMap::new();
    for line in split_lines(&read_text(&assets_dir().join("japanese_lexicon.tsv"))) {
        let columns: Vec<&str> = line.split('\t').collect();
        if columns.len() < 2 {
            continue;
        }
        let score: i64 = columns.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        let words = columns[1].split('|').filter(|w| !w.is_empty()).count() as i64;
        for (position, word) in columns[1].split('|').filter(|w| !w.is_empty()).take(per_reading).enumerate() {
            let has_kanji = word.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || c == '々');
            if !has_kanji || word.chars().count() > 8 {
                continue;
            }
            let candidate = (position, -words, -score, columns[0].to_string());
            match best.get(word) {
                Some(current) if (current.0, current.1, current.2) <= (candidate.0, candidate.1, candidate.2) => {}
                _ => _ = best.insert(word.to_string(), candidate),
            }
        }
    }
    if let Some(path) = args.optional_path("skk") {
        let raw = read_skk(&path);
        let mut okuri_ari = false;
        for line in split_lines(&raw) {
            if line.starts_with(";; okuri-ari entries") {
                okuri_ari = true;
                continue;
            }
            if line.starts_with(";; okuri-nasi entries") {
                break;
            }
            if !okuri_ari || line.starts_with(';') {
                continue;
            }
            let Some((head, rest)) = line.split_once(" /") else { continue };
            let Some(letter) = head.chars().last().filter(char::is_ascii_lowercase) else { continue };
            let reading = &head[..head.len() - 1];
            if !is_hiragana(reading) {
                continue;
            }
            let words: Vec<&str> =
                rest.split('/').map(|item| item.split(';').next().unwrap_or("").trim()).filter(|w| !w.is_empty()).collect();
            // A reading that starts voiced (がk for 書く) is the form inside compounds; the plain
            // one is preferred, then the reading with more words (いk over ゆk for 行く).
            let voiced = reading.starts_with(|c: char| "がぎぐげござじずぜぞだぢづでどばびぶべぼぱぴぷぺぽ".contains(c));
            let count = words.len() as i64;
            for (position, word) in words.iter().take(per_reading).enumerate() {
                let all_kanji = word.chars().all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || c == '々');
                if !all_kanji || word.chars().count() > 3 {
                    continue;
                }
                let candidate = (position, i64::from(voiced), -count, reading.to_string());
                let key = format!("{word}{letter}");
                match best.get(&key) {
                    Some(current) if (current.0, current.1, current.2) <= (candidate.0, candidate.1, candidate.2) => {}
                    _ => _ = best.insert(key, candidate),
                }
            }
        }
    }
    // Everyday readings that the dictionary lists after a formal one.
    let set_by_hand = [
        ("日本語", "にほんご"),
        ("昨日", "きのう"),
        ("一日", "いちにち"),
        ("今年", "ことし"),
        ("一人", "ひとり"),
        ("二人", "ふたり"),
        ("行t", "い"),
        ("入r", "はい"),
    ];
    for (word, reading) in set_by_hand {
        best.insert(word.to_string(), (0, 0, 0, reading.to_string()));
    }
    let lines: Vec<String> = best.into_iter().map(|(word, (_, _, _, reading))| format!("{word}\t{reading}")).collect();
    write_lines("japanese_readings.tsv", &lines);
}

// ------------------------------------------------------------------ Korean ----

const INITIALS: [&str; 19] = ["g", "kk", "n", "d", "tt", "r", "m", "b", "pp", "s", "ss", "", "j", "jj", "ch", "k", "t", "p", "h"];
const MEDIALS: [&str; 21] =
    ["a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo", "we", "wi", "yu", "eu", "ui", "i"];
/// Final consonants as written before a vowel (liaison) and as pronounced at the end of a syllable.
const FINAL_LIAISON: [&str; 28] = [
    "", "g", "kk", "gs", "n", "nj", "nh", "d", "l", "lg", "lm", "lb", "ls", "lt", "lp", "lh", "m", "b", "bs", "s", "ss", "ng", "j", "ch",
    "k", "t", "p", "h",
];
const FINAL_CODA: [&str; 28] = [
    "", "k", "k", "k", "n", "n", "n", "t", "l", "k", "m", "l", "l", "l", "p", "l", "m", "p", "p", "t", "t", "ng", "t", "t", "k", "t", "p",
    "t",
];

/// (initial, medial, final) for each syllable, or `None` unless the word is all Hangul syllables.
fn hangul_parts(word: &str) -> Option<Vec<(usize, usize, usize)>> {
    let parts: Option<Vec<_>> = word
        .chars()
        .map(|c| {
            let code = (c as u32).checked_sub(0xAC00).filter(|&code| code < 11172)? as usize;
            Some((code / 588, (code % 588) / 28, code % 28))
        })
        .collect();
    parts.filter(|p| !p.is_empty())
}

/// Revised Romanization with liaison and the common nasal assimilations.
fn romanize(word: &str) -> Option<String> {
    let parts = hangul_parts(word)?;
    let mut out = String::new();
    for (index, &(initial, medial, final_)) in parts.iter().enumerate() {
        let following = parts.get(index + 1);
        let mut lead = INITIALS[initial];
        if index > 0 && initial == 11 && !matches!(parts[index - 1].2, 0 | 21) {
            lead = ""; // the previous final consonant has moved onto this syllable
        }
        out.push_str(lead);
        out.push_str(MEDIALS[medial]);
        if final_ == 0 {
            continue;
        }
        if following.is_some_and(|f| f.0 == 11) && final_ != 21 {
            out.push_str(FINAL_LIAISON[final_]);
            continue;
        }
        let mut coda = FINAL_CODA[final_];
        if following.is_some_and(|f| matches!(INITIALS[f.0], "n" | "m")) {
            coda = match coda {
                "k" => "ng",
                "t" => "n",
                "p" => "m",
                other => other,
            };
        }
        if following.is_some_and(|f| f.0 == 5) && coda == "n" {
            coda = "l";
        }
        out.push_str(coda);
    }
    Some(out)
}

/// Letter-by-letter romanization, as many people type it.
fn transliterate(word: &str) -> Option<String> {
    Some(hangul_parts(word)?.iter().map(|&(i, m, f)| format!("{}{}{}", INITIALS[i], MEDIALS[m], FINAL_LIAISON[f])).collect())
}

fn build_korean(args: &Args) {
    let counts = read_frequency(&args.path("frequency"), args.number("minimum", 5));
    let (limit, per_key) = (args.number("limit", 80000) as usize, args.number("per-key", 8) as usize);
    let mut keys: BTreeMap<String, Vec<(u64, String)>> = BTreeMap::new();
    let mut taken = 0;
    for (word, count) in counts.by_frequency() {
        if hangul_parts(word).is_none() {
            continue;
        }
        let romanized = romanize(word);
        let transliterated = transliterate(word);
        let mut spellings = vec![romanized.clone()];
        if transliterated != romanized {
            spellings.push(transliterated);
        }
        for key in spellings.into_iter().flatten().filter(|k| !k.is_empty()) {
            keys.entry(key).or_default().push((count, word.to_string()));
        }
        taken += 1;
        if taken >= limit {
            break;
        }
    }
    let lines: Vec<String> = keys
        .iter_mut()
        .map(|(key, words)| {
            words.sort_by_key(|&(count, _)| std::cmp::Reverse(count));
            let best = words.iter().map(|(count, _)| *count).max().unwrap_or(0);
            let shown: Vec<&str> = words.iter().take(per_key).map(|(_, w)| w.as_str()).collect();
            format!("{key}\t{}\t{best}", shown.join("|"))
        })
        .collect();
    write_lines("korean_lexicon.tsv", &lines);
}

// -------------------------------------------------------------------- main ----

const USAGE: &str = "\
Usage: dicttool <command> [options]

Commands:
  english         --frequency en_full.txt --spelling words.txt [--limit 80000] [--extra 20000] [--slang-threshold 20000]
  malay           --frequency ms_full.txt [--minimum 3] [--limit 60000]
  pinyin-sources  [--cedict cedict.txt[.gz]] [--fcitx-simplified zhwiki.dict.yaml] [--opencc-characters F] [--opencc-phrases F]
  pinyin-variants (adds a few variant characters to assets/pinyin_lexicon.tsv; `chinese` does this too)
  chinese         --jieba dict.txt --frequency zh_cn_full.txt --opencc-characters TSCharacters.txt --opencc-phrases TSPhrases.txt
  pinyin-readings --cedict cedict.txt[.gz] (after `chinese`; pinyin with tone marks for selected text)
  japanese-readings [--skk SKK-JISYO.L] [--per-reading 4] (after `japanese`; kana readings of kanji for selected text)
  japanese        --skk SKK-JISYO.L --frequency ja_full.txt [--max-reading 12] [--per-reading 16] [--kana-minimum 20]
  korean          --frequency ko_full.txt [--minimum 5] [--limit 80000] [--per-key 8]

Outputs go to assets/. See docs/dictionary-sources.md for the sources.
";

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(command) = args.first() else {
        print!("{USAGE}");
        exit(2)
    };
    let rest = &args[1..];
    match command.as_str() {
        "english" => build_english(&Args::parse(rest, &["frequency", "spelling", "limit", "extra", "slang-threshold"])),
        "malay" => build_malay(&Args::parse(rest, &["frequency", "minimum", "limit"])),
        "pinyin-sources" => {
            build_pinyin_sources(&Args::parse(rest, &["cedict", "fcitx-simplified", "opencc-characters", "opencc-phrases"]))
        }
        "pinyin-variants" => build_pinyin_variants(),
        "pinyin-readings" => build_pinyin_readings(&Args::parse(rest, &["cedict"])),
        "japanese-readings" => build_japanese_readings(&Args::parse(rest, &["skk", "per-reading"])),
        "chinese" => build_chinese(&Args::parse(rest, &["jieba", "frequency", "opencc-characters", "opencc-phrases"])),
        "japanese" => build_japanese(&Args::parse(rest, &["skk", "frequency", "max-reading", "per-reading", "kana-minimum"])),
        "korean" => build_korean(&Args::parse(rest, &["frequency", "minimum", "limit", "per-key"])),
        "-h" | "--help" | "help" => print!("{USAGE}"),
        other => fail(format!("unknown command {other}\n\n{USAGE}")),
    }
    let _ = io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_words() {
        assert!(is_latin_word("don't"));
        assert!(is_latin_word("well-known"));
        assert!(!is_latin_word("-a"));
        assert!(!is_latin_word("a--b"));
        assert!(!is_latin_word("Hello"));
    }

    #[test]
    fn korean_romanization() {
        assert_eq!(romanize("한국어").as_deref(), Some("hangugeo"));
        assert_eq!(transliterate("한국어").as_deref(), Some("hangugeo"));
        assert_eq!(romanize("감사합니다").as_deref(), Some("gamsahamnida"));
        assert_eq!(transliterate("감사합니다").as_deref(), Some("gamsahabnida"));
        assert_eq!(romanize("신라").as_deref(), Some("silra"), "ㄴ before ㄹ becomes l, the initial stays r");
        assert_eq!(hangul_parts("abc"), None);
    }

    #[test]
    fn cedict_lines() {
        assert_eq!(parse_cedict("中國 中国 [Zhong1 guo2] /China/"), Some(("中國", "中国", "Zhong1 guo2")));
        assert_eq!(parse_cedict("綠色  绿色 [lu:4 se4] /green/"), Some(("綠色", "绿色", "lu:4 se4")));
        assert_eq!(parse_cedict("# comment"), None);
        assert_eq!(parse_cedict("a b [] /x/"), None);
    }

    #[test]
    fn variants_follow_their_anchor_once() {
        let mut lines = vec!["ni\t你|呢|泥\t你|呢|妳|泥\t3".to_string(), "nimen\t你们\t你們".to_string()];
        add_variants(&mut lines);
        add_variants(&mut lines);
        assert_eq!(lines, ["ni\t你|呢|妳|泥\t你|呢|妳|泥\t3", "nimen\t你们|妳们\t你們|妳們"]);
    }

    #[test]
    fn pinyin_tone_marks() {
        assert_eq!(tone_marks("ni3 hao3").as_deref(), Some("nǐhǎo"));
        assert_eq!(tone_marks("Xi1 an1").as_deref(), Some("xī'ān"));
        assert_eq!(tone_marks("lu:4 se4").as_deref(), Some("lǜsè"));
        assert_eq!(tone_marks("gou3 xiong2").as_deref(), Some("gǒuxióng"));
        assert_eq!(tone_marks("dui4 de5").as_deref(), Some("duìde"));
        assert_eq!(tone_marks("A A"), None);
    }

    #[test]
    fn python_line_splitting() {
        assert_eq!(split_lines("a\r\nb\rc\n\nd"), vec!["a", "b", "c", "", "d"]);
        assert_eq!(split_lines("a\n"), vec!["a"]);
    }
}
