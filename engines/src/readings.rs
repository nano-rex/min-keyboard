//! How selected text is pronounced: pinyin for Chinese, romaji for Japanese and Revised
//! Romanization for Korean. Chinese and Japanese words are looked up in sorted
//! `word<TAB>reading` tables, which stay one string in memory and are searched in place.

use std::io;

use crate::{japanese, korean, Assets};

pub const PINYIN_READINGS: &str = "pinyin_readings.tsv";
pub const JAPANESE_READINGS: &str = "japanese_readings.tsv";

/// Longest word looked up, in characters.
const LONGEST_WORD: usize = 8;

/// A `word<TAB>reading` table sorted by word.
pub struct ReadingTable {
    text: String,
    /// Byte offsets of each line's start, tab and end.
    lines: Vec<(u32, u32, u32)>,
}

impl ReadingTable {
    pub fn load(assets: &dyn Assets, name: &str) -> io::Result<Self> {
        let bytes = assets.read(name).map_err(|e| io::Error::new(e.kind(), format!("failed to load asset {name}: {e}")))?;
        let text = String::from_utf8(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, format!("{name} is not UTF-8")))?;
        let mut lines = Vec::new();
        let mut start = 0;
        for line in text.split_inclusive('\n') {
            let end = start + line.trim_end_matches(['\n', '\r']).len();
            if let Some(tab) = line.find('\t') {
                lines.push((start as u32, (start + tab) as u32, end as u32));
            }
            start += line.len();
        }
        // Lookups need byte order; the tables are written that way, but do not rely on it.
        if !lines.windows(2).all(|w| text[w[0].0 as usize..w[0].1 as usize] <= text[w[1].0 as usize..w[1].1 as usize]) {
            lines.sort_by(|a, b| text[a.0 as usize..a.1 as usize].cmp(&text[b.0 as usize..b.1 as usize]));
        }
        Ok(ReadingTable { text, lines })
    }

    pub fn get(&self, word: &str) -> Option<&str> {
        let word_at = |&(start, tab, _): &(u32, u32, u32)| &self.text[start as usize..tab as usize];
        let i = self.lines.binary_search_by(|line| word_at(line).cmp(word)).ok()?;
        let (_, tab, end) = self.lines[i];
        Some(&self.text[tab as usize + 1..end as usize])
    }

    /// The longest word at the start of `chars` that `accept` allows, with its length in characters.
    fn longest(&self, chars: &[char], accept: impl Fn(char) -> bool) -> Option<(usize, &str)> {
        let limit = chars.iter().take(LONGEST_WORD).take_while(|&&c| accept(c)).count();
        (1..=limit).rev().find_map(|n| self.get(&chars[..n].iter().collect::<String>()).map(|reading| (n, reading)))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Script {
    Chinese,
    Japanese,
    Korean,
}

fn is_han(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c) || c == '々' || c == '〇'
}

fn is_hiragana(c: char) -> bool {
    ('ぁ'..='ゖ').contains(&c)
}

fn is_kana(c: char) -> bool {
    is_hiragana(c) || ('ァ'..='ヺ').contains(&c) || c == 'ー'
}

fn is_hangul(c: char) -> bool {
    ('가'..='힣').contains(&c)
}

/// Which language's pronunciation to show for `text`: Japanese if it has kana, Korean if it
/// has Hangul, and Chinese for Han characters alone (Japanese if `kanji_are_japanese`).
pub fn detect(text: &str, kanji_are_japanese: bool) -> Option<Script> {
    if text.chars().any(is_kana) {
        Some(Script::Japanese)
    } else if text.chars().any(is_hangul) {
        Some(Script::Korean)
    } else if text.chars().any(is_han) {
        Some(if kanji_are_japanese { Script::Japanese } else { Script::Chinese })
    } else {
        None
    }
}

/// Joins romanized words with spaces and turns full-width punctuation into ASCII, keeping any
/// other text as it is.
#[derive(Default)]
struct Line {
    out: String,
    /// The last thing written was a romanized word.
    after_word: bool,
}

impl Line {
    fn word(&mut self, word: &str) {
        let last = self.out.chars().last();
        if last.is_some_and(|c| !c.is_whitespace() && !"([{\"'“‘".contains(c)) {
            self.out.push(' ');
        }
        self.out.push_str(word);
        self.after_word = true;
    }

    fn other(&mut self, c: char) {
        let c = match c {
            '，' | '、' => ',',
            '。' | '．' => '.',
            '！' => '!',
            '？' => '?',
            '：' => ':',
            '；' => ';',
            '（' => '(',
            '）' => ')',
            '「' | '」' | '『' | '』' => '"',
            '\u{3000}' => ' ',
            _ => c,
        };
        if self.after_word && c.is_alphanumeric() {
            self.out.push(' ');
        }
        self.out.push(c);
        self.after_word = false;
    }
}

/// Pinyin with tone marks, word by word: "我爱你。" is "wǒ ài nǐ."
pub fn chinese(table: &ReadingTable, text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut line = Line::default();
    let mut i = 0;
    while i < chars.len() {
        match table.longest(&chars[i..], is_han) {
            Some((n, reading)) => {
                line.word(reading);
                i += n;
            }
            None => {
                line.other(chars[i]);
                i += 1;
            }
        }
    }
    line.out
}

/// Hepburn romaji, word by word: "日本語を話します" is "nihongo wo hanashimasu". Kanji with a kana
/// ending (話します) are read from the ending's first letter; the particles は, へ and を on
/// their own are read wa, e and o.
pub fn japanese(table: &ReadingTable, text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut line = Line::default();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_kana(c) {
            // A run of hiragana, or of katakana.
            let hiragana = is_hiragana(c);
            let n = chars[i..].iter().take_while(|&&k| is_kana(k) && (k == 'ー' || is_hiragana(k) == hiragana)).count();
            let run: String = chars[i..i + n].iter().collect();
            let romaji = match run.as_str() {
                "は" => "wa".to_string(),
                "へ" => "e".to_string(),
                "を" => "o".to_string(),
                _ => japanese::to_romaji(&run),
            };
            line.word(&romaji);
            i += n;
            continue;
        }
        let plain = table.longest(&chars[i..], is_han);
        // Kanji followed by a kana ending: 食 + べる is た + べる.
        let kanji = chars[i..].iter().take(3).take_while(|&&k| is_han(k)).count();
        let with_ending = (1..=kanji).rev().find_map(|n| {
            // Particles after a noun are not endings (私は, 日本の).
            let ending = chars.get(i + n).copied().filter(|&k| is_hiragana(k) && !"はをへのとも".contains(k))?;
            let letter = ending_letter(ending)?;
            let key: String = chars[i..i + n].iter().chain(std::iter::once(&letter)).collect();
            table.get(&key).map(|reading| (n, reading))
        });
        match (plain, with_ending) {
            (plain, Some((n, reading))) if plain.is_none_or(|(p, _)| n >= p) => {
                // The ending belongs to the same word.
                let ending = chars[i + n..].iter().take_while(|&&k| is_hiragana(k)).count();
                let kana: String = reading.chars().chain(chars[i + n..i + n + ending].iter().copied()).collect();
                line.word(&japanese::to_romaji(&kana));
                i += n + ending;
            }
            (Some((n, reading)), _) => {
                line.word(&japanese::to_romaji(reading));
                i += n;
            }
            _ => {
                line.other(c);
                i += 1;
            }
        }
    }
    line.out
}

/// The letter SKK files a kana ending under: か is k, っ is t, ん is n, あ is a.
fn ending_letter(kana: char) -> Option<char> {
    match kana {
        'っ' => Some('t'),
        'ん' => Some('n'),
        _ => {
            let romaji = japanese::to_romaji(&kana.to_string());
            // ち, つ and ふ are filed under t, t and h, like the rest of their rows.
            match romaji.as_str() {
                "chi" | "tsu" => Some('t'),
                "fu" => Some('h'),
                _ => romaji.chars().next().filter(char::is_ascii_lowercase),
            }
        }
    }
}

/// Revised Romanization, keeping spaces and punctuation: "한국어 공부" is "hangugeo gongbu".
pub fn korean(text: &str) -> String {
    let mut line = Line::default();
    let mut word = String::new();
    let flush = |line: &mut Line, word: &mut String| {
        if !word.is_empty() {
            line.word(&korean::romanize(word));
            word.clear();
        }
    };
    for c in text.chars() {
        if is_hangul(c) {
            word.push(c);
        } else {
            flush(&mut line, &mut word);
            line.other(c);
        }
    }
    flush(&mut line, &mut word);
    line.out
}
