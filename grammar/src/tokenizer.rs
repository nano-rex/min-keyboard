//! The byte-level BPE tokenizer of Qwen models (GPT-2 style), built from the vocabulary and
//! merge list stored in the GGUF file.

use std::collections::HashMap;
use std::io;

use crate::gguf::{Gguf, Value};

/// GGUF token types.
const TOKEN_NORMAL: i64 = 1;

pub struct Tokenizer {
    tokens: Vec<Vec<u8>>,
    ids: HashMap<Vec<u8>, u32>,
    /// (left, right) -> (rank, merged token).
    merges: HashMap<(u32, u32), (u32, u32)>,
    /// The token of each byte, through GPT-2's byte-to-character table.
    byte_tokens: [u32; 256],
    /// Control and user-defined tokens such as `<|im_start|>`, decoded as their text.
    special: HashMap<String, u32>,
    is_special: Vec<bool>,
    /// Tokens that end generation.
    pub end_of_generation: Vec<u32>,
}

/// GPT-2 maps every byte to a printable character so tokens can be stored as text.
fn byte_to_char() -> [char; 256] {
    let mut table = ['\0'; 256];
    let mut next = 256u32;
    for b in 0..256u32 {
        let printable = (33..=126).contains(&b) || (161..=172).contains(&b) || (174..=255).contains(&b);
        let c = if printable {
            b
        } else {
            next += 1;
            next - 1
        };
        table[b as usize] = char::from_u32(c).unwrap();
    }
    table
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

impl Tokenizer {
    pub fn from_gguf(gguf: &Gguf) -> io::Result<Tokenizer> {
        if gguf.get("tokenizer.ggml.model").and_then(Value::as_str) != Some("gpt2") {
            return Err(invalid("The model's tokenizer is not supported (a Qwen model is needed)."));
        }
        let strings = |key: &str| -> io::Result<Vec<&str>> {
            let values = gguf.get(key).and_then(Value::as_array).ok_or_else(|| invalid("The model has no tokenizer."))?;
            Ok(values.iter().map(|v| v.as_str().unwrap_or("")).collect())
        };
        let texts = strings("tokenizer.ggml.tokens")?;
        let types: Vec<i64> = gguf
            .get("tokenizer.ggml.token_type")
            .and_then(Value::as_array)
            .map(|v| v.iter().map(|t| t.as_int().unwrap_or(TOKEN_NORMAL)).collect())
            .unwrap_or_default();
        let to_char = byte_to_char();
        let mut from_char = HashMap::new();
        for (b, &c) in to_char.iter().enumerate() {
            from_char.insert(c, b as u8);
        }

        let mut tokens = Vec::with_capacity(texts.len());
        let mut ids = HashMap::with_capacity(texts.len());
        let mut special = HashMap::new();
        let mut is_special = Vec::with_capacity(texts.len());
        for (id, text) in texts.iter().enumerate() {
            let kind = types.get(id).copied().unwrap_or(TOKEN_NORMAL);
            let bytes: Vec<u8> = if kind == TOKEN_NORMAL {
                // Byte-level text back to the bytes it stands for.
                text.chars().map(|c| from_char.get(&c).copied().unwrap_or(b'?')).collect()
            } else {
                special.insert(text.to_string(), id as u32);
                text.as_bytes().to_vec()
            };
            if kind == TOKEN_NORMAL {
                ids.entry(bytes.clone()).or_insert(id as u32);
            }
            is_special.push(kind != TOKEN_NORMAL);
            tokens.push(bytes);
        }
        let mut byte_tokens = [0u32; 256];
        for (b, slot) in byte_tokens.iter_mut().enumerate() {
            *slot = *ids.get(&vec![b as u8]).ok_or_else(|| invalid("The model's vocabulary lacks byte tokens."))?;
        }

        let mut merges = HashMap::new();
        for (rank, line) in strings("tokenizer.ggml.merges")?.iter().enumerate() {
            let Some((left, right)) = line.split_once(' ') else { continue };
            let decode = |s: &str| -> Vec<u8> { s.chars().map(|c| from_char.get(&c).copied().unwrap_or(b'?')).collect() };
            let (left, right) = (decode(left), decode(right));
            let merged = [left.as_slice(), right.as_slice()].concat();
            if let (Some(&l), Some(&r), Some(&m)) = (ids.get(&left), ids.get(&right), ids.get(&merged)) {
                merges.entry((l, r)).or_insert((rank as u32, m));
            }
        }

        let mut end_of_generation = Vec::new();
        if let Some(eos) = gguf.get("tokenizer.ggml.eos_token_id").and_then(Value::as_int) {
            end_of_generation.push(eos as u32);
        }
        for name in ["<|im_end|>", "<|endoftext|>", "<|eot_id|>"] {
            if let Some(&id) = special.get(name) {
                end_of_generation.push(id);
            }
        }
        Ok(Tokenizer { tokens, ids, merges, byte_tokens, special, is_special, end_of_generation })
    }

    pub fn vocab_size(&self) -> usize {
        self.tokens.len()
    }

    pub fn special(&self, name: &str) -> Option<u32> {
        self.special.get(name).copied()
    }

    /// Encodes plain text; special-token names in it are treated as ordinary text.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::new();
        for piece in pre_tokenize(text) {
            if let Some(&id) = self.ids.get(piece.as_bytes()) {
                out.push(id);
                continue;
            }
            self.bpe(piece.as_bytes(), &mut out);
        }
        out
    }

    fn bpe(&self, bytes: &[u8], out: &mut Vec<u32>) {
        let mut parts: Vec<u32> = bytes.iter().map(|&b| self.byte_tokens[b as usize]).collect();
        loop {
            let best = parts
                .windows(2)
                .enumerate()
                .filter_map(|(i, pair)| self.merges.get(&(pair[0], pair[1])).map(|&(rank, merged)| (rank, i, merged)))
                .min();
            let Some((_, i, merged)) = best else { break };
            parts[i] = merged;
            parts.remove(i + 1);
        }
        out.extend(parts);
    }

    /// The bytes of a token; special tokens other than text-like ones decode to nothing.
    pub fn token_bytes(&self, id: u32) -> &[u8] {
        match self.tokens.get(id as usize) {
            Some(_) if self.is_special[id as usize] => &[],
            Some(bytes) => bytes,
            None => &[],
        }
    }

    /// The token's text including special tokens, for debugging.
    pub fn token_text(&self, id: u32) -> String {
        String::from_utf8_lossy(self.tokens.get(id as usize).map(Vec::as_slice).unwrap_or(b"")).into_owned()
    }

    pub fn decode(&self, ids: &[u32]) -> String {
        let bytes: Vec<u8> = ids.iter().flat_map(|&id| self.token_bytes(id).iter().copied()).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

fn is_letter(c: char) -> bool {
    c.is_alphabetic()
}

fn is_number(c: char) -> bool {
    c.is_numeric()
}

fn is_newline(c: char) -> bool {
    c == '\r' || c == '\n'
}

/// Splits text the way Qwen2's pre-tokenizer regex does:
/// `(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}| ?[^\s\p{L}\p{N}]+[\r\n]*|\s*[\r\n]+|\s+(?!\S)|\s+`
pub fn pre_tokenize(text: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|&(_, c)| c);
    let byte = |i: usize| chars.get(i).map(|&(b, _)| b).unwrap_or(text.len());
    let mut pieces = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let end = match_piece(&at, i);
        pieces.push(&text[byte(i)..byte(end)]);
        i = end;
    }
    pieces
}

/// The end (exclusive, in chars) of the piece starting at `i`; always more than `i`.
fn match_piece(at: &dyn Fn(usize) -> Option<char>, i: usize) -> usize {
    let c = at(i).unwrap();
    // Contractions.
    if c == '\'' {
        let lower = |k: usize| at(i + k).map(|c| c.to_ascii_lowercase());
        match (lower(1), lower(2)) {
            (Some('s' | 't' | 'm' | 'd'), _) => return i + 2,
            (Some('r'), Some('e')) | (Some('v'), Some('e')) | (Some('l'), Some('l')) => return i + 3,
            _ => {}
        }
    }
    let letters_from = |mut k: usize| {
        while at(k).is_some_and(is_letter) {
            k += 1;
        }
        k
    };
    // An optional leading non-letter, then letters.
    if is_letter(c) {
        return letters_from(i + 1);
    }
    if !is_newline(c) && !is_number(c) && at(i + 1).is_some_and(is_letter) {
        return letters_from(i + 2);
    }
    if is_number(c) {
        return i + 1;
    }
    // An optional space, then punctuation, then any line breaks.
    let is_punct = |c: char| !c.is_whitespace() && !is_letter(c) && !is_number(c);
    let start = if c == ' ' && at(i + 1).is_some_and(is_punct) { i + 1 } else { i };
    if at(start).is_some_and(is_punct) {
        let mut k = start;
        while at(k).is_some_and(is_punct) {
            k += 1;
        }
        while at(k).is_some_and(is_newline) {
            k += 1;
        }
        return k;
    }
    // Whitespace.
    let mut end = i;
    while at(end).is_some_and(char::is_whitespace) {
        end += 1;
    }
    if let Some(last_newline) = (i..end).rev().find(|&k| at(k).is_some_and(is_newline)) {
        return last_newline + 1;
    }
    if at(end).is_none() || end - i == 1 {
        return end;
    }
    // Leave the last space to start the next word.
    end - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_tokenizer_splits_like_qwen() {
        assert_eq!(pre_tokenize("Hello world"), ["Hello", " world"]);
        assert_eq!(
            pre_tokenize("he dont know, they're 2024!"),
            ["he", " dont", " know", ",", " they", "'re", " ", "2", "0", "2", "4", "!"]
        );
        assert_eq!(pre_tokenize("a  b\n\nc"), ["a", " ", " b", "\n\n", "c"]);
        assert_eq!(pre_tokenize("我爱你。好"), ["我爱你", "。好"]);
        assert_eq!(pre_tokenize("end.  "), ["end", ".", "  "]);
        assert_eq!(pre_tokenize("x:\n  y"), ["x", ":\n", " ", " y"]);
        assert_eq!(pre_tokenize("(hi)"), ["(hi", ")"]);
    }
}
