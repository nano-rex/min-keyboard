//! Romaji to kana conversion with kanji candidates from an SKK-derived lexicon.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::sync::OnceLock;

use crate::{for_each_line, for_each_line_if_present, prefixed, split_candidates, Assets, Candidates};

const MAX_CANDIDATES: usize = 40;
const MAX_COMPLETIONS: usize = 16;
const LONGEST_ROMAJI: usize = 4;

struct Entry {
    candidates: Vec<String>,
    score: i64,
}

pub struct JapaneseEngine {
    lexicon: BTreeMap<String, Entry>,
    shortcuts: HashMap<String, Vec<String>>,
}

impl JapaneseEngine {
    pub fn new(assets: &dyn Assets) -> io::Result<Self> {
        let mut lexicon = BTreeMap::new();
        for_each_line(assets, "japanese_lexicon.tsv", |line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 || parts[0].is_empty() {
                return;
            }
            let score = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            lexicon.insert(parts[0].to_string(), Entry { candidates: split_candidates(parts[1]), score });
        })?;
        let mut shortcuts: HashMap<String, Vec<String>> = HashMap::new();
        for_each_line_if_present(assets, "japanese_shortcuts.tsv", |line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 && !parts[0].is_empty() {
                shortcuts.entry(parts[0].to_lowercase()).or_default().push(parts[1].to_string());
            }
        })?;
        Ok(JapaneseEngine { lexicon, shortcuts })
    }

    pub fn candidates(&self, raw_input: &str) -> Vec<String> {
        let input = raw_input.to_lowercase();
        if input.is_empty() {
            return Vec::new();
        }
        let hiragana = to_hiragana(&input, false);
        let finished = to_hiragana(&input, true);
        let mut results = Candidates::default();
        if let Some(shortcut) = self.shortcuts.get(&input) {
            results.add_all(shortcut);
        }
        if let Some(exact) = self.lexicon.get(&finished) {
            results.add_all(&exact.candidates);
        }
        results.add(&finished);
        results.add(&to_katakana(&finished));

        // Complete from the kana typed so far (ignoring a trailing unfinished consonant).
        let prefix = strip_trailing_latin(&hiragana);
        if !prefix.is_empty() {
            let mut matches: Vec<(&String, &Entry)> = Vec::new();
            for (key, entry) in prefixed(&self.lexicon, prefix, prefix) {
                if *key != finished {
                    matches.push((key, entry));
                }
                if matches.len() > 2000 {
                    break;
                }
            }
            matches.sort_by(|a, b| b.1.score.cmp(&a.1.score).then_with(|| a.0.chars().count().cmp(&b.0.chars().count())));
            for (key, entry) in matches.iter().take(MAX_COMPLETIONS) {
                results.add(entry.candidates.first().unwrap_or(key));
            }
        }
        results.into_vec(MAX_CANDIDATES)
    }
}

fn romaji() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let table: &[(&str, &str)] = &[
            ("a", "あ"),
            ("i", "い"),
            ("u", "う"),
            ("e", "え"),
            ("o", "お"),
            ("ka", "か"),
            ("ki", "き"),
            ("ku", "く"),
            ("ke", "け"),
            ("ko", "こ"),
            ("ga", "が"),
            ("gi", "ぎ"),
            ("gu", "ぐ"),
            ("ge", "げ"),
            ("go", "ご"),
            ("sa", "さ"),
            ("si", "し"),
            ("shi", "し"),
            ("su", "す"),
            ("se", "せ"),
            ("so", "そ"),
            ("za", "ざ"),
            ("zi", "じ"),
            ("ji", "じ"),
            ("zu", "ず"),
            ("ze", "ぜ"),
            ("zo", "ぞ"),
            ("ta", "た"),
            ("ti", "ち"),
            ("chi", "ち"),
            ("tu", "つ"),
            ("tsu", "つ"),
            ("te", "て"),
            ("to", "と"),
            ("da", "だ"),
            ("di", "ぢ"),
            ("du", "づ"),
            ("de", "で"),
            ("do", "ど"),
            ("na", "な"),
            ("ni", "に"),
            ("nu", "ぬ"),
            ("ne", "ね"),
            ("no", "の"),
            ("ha", "は"),
            ("hi", "ひ"),
            ("hu", "ふ"),
            ("fu", "ふ"),
            ("he", "へ"),
            ("ho", "ほ"),
            ("ba", "ば"),
            ("bi", "び"),
            ("bu", "ぶ"),
            ("be", "べ"),
            ("bo", "ぼ"),
            ("pa", "ぱ"),
            ("pi", "ぴ"),
            ("pu", "ぷ"),
            ("pe", "ぺ"),
            ("po", "ぽ"),
            ("ma", "ま"),
            ("mi", "み"),
            ("mu", "む"),
            ("me", "め"),
            ("mo", "も"),
            ("ya", "や"),
            ("yu", "ゆ"),
            ("yo", "よ"),
            ("ye", "いぇ"),
            ("ra", "ら"),
            ("ri", "り"),
            ("ru", "る"),
            ("re", "れ"),
            ("ro", "ろ"),
            ("la", "ら"),
            ("li", "り"),
            ("lu", "る"),
            ("le", "れ"),
            ("lo", "ろ"),
            ("wa", "わ"),
            ("wi", "うぃ"),
            ("we", "うぇ"),
            ("wo", "を"),
            ("nn", "ん"),
            ("n'", "ん"),
            ("xn", "ん"),
            ("va", "ゔぁ"),
            ("vi", "ゔぃ"),
            ("vu", "ゔ"),
            ("ve", "ゔぇ"),
            ("vo", "ゔぉ"),
            ("fa", "ふぁ"),
            ("fi", "ふぃ"),
            ("fe", "ふぇ"),
            ("fo", "ふぉ"),
            ("ja", "じゃ"),
            ("ju", "じゅ"),
            ("je", "じぇ"),
            ("jo", "じょ"),
            ("sha", "しゃ"),
            ("shu", "しゅ"),
            ("she", "しぇ"),
            ("sho", "しょ"),
            ("cha", "ちゃ"),
            ("chu", "ちゅ"),
            ("che", "ちぇ"),
            ("cho", "ちょ"),
            ("tsa", "つぁ"),
            ("thi", "てぃ"),
            ("dhi", "でぃ"),
            ("twu", "とぅ"),
            ("dwu", "どぅ"),
            ("xa", "ぁ"),
            ("xi", "ぃ"),
            ("xu", "ぅ"),
            ("xe", "ぇ"),
            ("xo", "ぉ"),
            ("xya", "ゃ"),
            ("xyu", "ゅ"),
            ("xyo", "ょ"),
            ("xtu", "っ"),
            ("xtsu", "っ"),
            ("xwa", "ゎ"),
            ("ltu", "っ"),
            ("lya", "ゃ"),
            ("lyu", "ゅ"),
            ("lyo", "ょ"),
            ("-", "ー"),
            (",", "、"),
            (".", "。"),
        ];
        let mut map: HashMap<String, String> = table.iter().map(|&(k, v)| (k.to_string(), v.to_string())).collect();
        // Contracted sounds: kya, gya, sya, zya, tya, dya, nya, hya, bya, pya, mya, rya.
        let contracted = [
            ("k", "き"),
            ("g", "ぎ"),
            ("s", "し"),
            ("z", "じ"),
            ("t", "ち"),
            ("c", "ち"),
            ("d", "ぢ"),
            ("n", "に"),
            ("h", "ひ"),
            ("b", "び"),
            ("p", "ぴ"),
            ("m", "み"),
            ("r", "り"),
            ("j", "じ"),
        ];
        let small_y = [("ya", "ゃ"), ("yu", "ゅ"), ("ye", "ぇ"), ("yo", "ょ")];
        for (consonant, kana) in contracted {
            for (y, small) in small_y {
                map.entry(format!("{consonant}{y}")).or_insert_with(|| format!("{kana}{small}"));
            }
        }
        map
    })
}

/// Converts romaji to hiragana. With `finish`, a trailing "n" becomes ん; otherwise
/// unconverted letters are left as typed so the caller can tell the input is unfinished.
pub fn to_hiragana(romaji_text: &str, finish: bool) -> String {
    let table = romaji();
    let chars: Vec<char> = romaji_text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Doubled consonant -> small tsu ("kitte").
        if i + 1 < chars.len() && c == chars[i + 1] && is_consonant(c) && c != 'n' {
            out.push('っ');
            i += 1;
            continue;
        }
        // "n" before a consonant (other than y) or apostrophe -> ん.
        if c == 'n' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next == 'n' {
                // "konnichiha": the second n starts the next syllable; "honn": both make one ん.
                let after = chars.get(i + 2).copied();
                out.push('ん');
                i += if after.is_some_and(|a| "aeiouy".contains(a)) { 1 } else { 2 };
                continue;
            }
            if next == '\'' {
                out.push('ん');
                i += 2;
                continue;
            }
            if is_consonant(next) && next != 'y' {
                out.push('ん');
                i += 1;
                continue;
            }
        }
        let longest = LONGEST_ROMAJI.min(chars.len() - i);
        let found = (1..=longest).rev().find_map(|len| {
            let piece: String = chars[i..i + len].iter().collect();
            table.get(&piece).map(|kana| (kana, len))
        });
        match found {
            Some((kana, len)) => {
                out.push_str(kana);
                i += len;
            }
            None => {
                out.push(c);
                i += 1;
            }
        }
    }
    if finish && out.ends_with('n') {
        out.pop();
        out.push('ん');
    }
    out
}

/// Hepburn romaji for kana: "きょうと" is "kyouto", "がっこう" is "gakkou", "ラーメン" is
/// "raamen". Other characters are kept.
pub fn to_romaji(kana: &str) -> String {
    const BASE: &str = "あa いi うu えe おo かka きki くku けke こko がga ぎgi ぐgu げge ごgo \
        さsa しshi すsu せse そso ざza じji ずzu ぜze ぞzo たta ちchi つtsu てte とto だda ぢji づzu でde どdo \
        なna にni ぬnu ねne のno はha ひhi ふfu へhe ほho ばba びbi ぶbu べbe ぼbo ぱpa ぴpi ぷpu ぺpe ぽpo \
        まma みmi むmu めme もmo やya ゆyu よyo らra りri るru れre ろro わwa ゐi ゑe をwo んn ゔvu \
        ぁa ぃi ぅu ぇe ぉo ゃya ゅyu ょyo ゎwa";
    static TABLE: OnceLock<HashMap<char, &'static str>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        BASE.split_whitespace()
            .filter_map(|pair| {
                let mut chars = pair.char_indices();
                let (_, kana) = chars.next()?;
                let (at, _) = chars.next()?;
                Some((kana, &pair[at..]))
            })
            .collect()
    });
    let chars: Vec<char> = kana.chars().map(katakana_to_hiragana).collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            // Small tsu doubles the next consonant ("tch" before ch).
            'っ' => {
                if let Some(romaji) = next.and_then(|n| table.get(&n)) {
                    out.push_str(if romaji.starts_with("ch") { "t" } else { &romaji[..1] });
                }
                i += 1;
                continue;
            }
            // The long-vowel mark repeats the vowel before it.
            'ー' => {
                if let Some(vowel) = out.chars().last().filter(|v| "aeiou".contains(*v)) {
                    out.push(vowel);
                }
                i += 1;
                continue;
            }
            _ => {}
        }
        let Some(&romaji) = table.get(&c) else {
            out.push(c);
            i += 1;
            continue;
        };
        match next {
            // きゃ is kya, しゃ is sha, じょ is jo.
            Some(small @ ('ゃ' | 'ゅ' | 'ょ')) if romaji.ends_with('i') && romaji.len() > 1 => {
                let stem = &romaji[..romaji.len() - 1];
                let vowel = &table[&small][1..];
                out.push_str(stem);
                if !matches!(stem, "sh" | "ch" | "j") {
                    out.push('y');
                }
                out.push_str(vowel);
                i += 2;
            }
            // ふぁ is fa, てぃ is ti, うぃ is wi.
            Some(small @ ('ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ')) if romaji != "n" => {
                let stem = if romaji == "u" { "w" } else { romaji.trim_end_matches(['a', 'i', 'u', 'e', 'o']) };
                out.push_str(stem);
                out.push_str(table[&small]);
                i += 2;
            }
            _ => {
                out.push_str(romaji);
                // ん before a vowel or y is written n' (kin'en, not kinen).
                if c == 'ん'
                    && next.and_then(|n| table.get(&katakana_to_hiragana(n))).is_some_and(|r| r.starts_with(['a', 'i', 'u', 'e', 'o', 'y']))
                {
                    out.push('\'');
                }
                i += 1;
            }
        }
    }
    out
}

fn katakana_to_hiragana(c: char) -> char {
    if ('ァ'..='ヶ').contains(&c) {
        char::from_u32(c as u32 - 0x60).unwrap_or(c)
    } else {
        c
    }
}

pub fn to_katakana(hiragana: &str) -> String {
    hiragana.chars().map(|c| if ('ぁ'..='ゖ').contains(&c) { char::from_u32(c as u32 + 0x60).unwrap_or(c) } else { c }).collect()
}

fn strip_trailing_latin(text: &str) -> &str {
    text.trim_end_matches(|c: char| c.is_ascii())
}

fn is_consonant(c: char) -> bool {
    c.is_ascii_lowercase() && !"aeiou".contains(c)
}
