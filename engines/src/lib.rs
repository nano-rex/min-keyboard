//! Input engines for Min Keyboard: word completion and correction for Latin-script languages,
//! pinyin to Chinese, romaji to Japanese and romanized Korean. The engines only depend on std,
//! so they run unchanged in the Android library and in desktop tests.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

pub mod japanese;
pub mod korean;
pub mod latin;
pub mod pinyin;
pub mod readings;

pub use japanese::JapaneseEngine;
pub use korean::KoreanEngine;
pub use latin::LatinEngine;
pub use pinyin::{PinyinEngine, ScriptMode};

/// Where the dictionaries come from: the APK's assets on Android, a folder in tests.
pub trait Assets {
    /// The asset's bytes, or `ErrorKind::NotFound` when it does not exist.
    fn read(&self, name: &str) -> io::Result<Vec<u8>>;
}

/// Reads assets from a directory, such as the repository's `assets/` folder.
pub struct DirAssets(pub PathBuf);

impl Assets for DirAssets {
    fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        std::fs::read(self.0.join(name))
    }
}

/// Calls `handler` for every line of an asset.
pub(crate) fn for_each_line(assets: &dyn Assets, name: &str, mut handler: impl FnMut(&str)) -> io::Result<()> {
    let bytes = assets.read(name).map_err(|e| io::Error::new(e.kind(), format!("failed to load asset {name}: {e}")))?;
    for line in String::from_utf8_lossy(&bytes).lines() {
        handler(line);
    }
    Ok(())
}

/// Like [`for_each_line`] but treats a missing asset as empty.
pub(crate) fn for_each_line_if_present(assets: &dyn Assets, name: &str, handler: impl FnMut(&str)) -> io::Result<()> {
    match assets.read(name) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(_) => for_each_line(assets, name, handler),
    }
}

/// An insertion-ordered set of candidates, like Java's `LinkedHashSet<String>`.
#[derive(Default)]
pub(crate) struct Candidates {
    items: Vec<String>,
    seen: HashSet<String>,
}

impl Candidates {
    pub(crate) fn add(&mut self, value: &str) {
        if !self.seen.contains(value) {
            self.seen.insert(value.to_string());
            self.items.push(value.to_string());
        }
    }

    pub(crate) fn add_all<'a>(&mut self, values: impl IntoIterator<Item = &'a String>) {
        for value in values {
            self.add(value);
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(crate) fn into_vec(mut self, limit: usize) -> Vec<String> {
        self.items.truncate(limit);
        self.items
    }
}

/// Splits on `separator` and drops trailing empty pieces, like Java's `String.split`.
pub(crate) fn java_split(text: &str, separator: char) -> Vec<&str> {
    let mut parts: Vec<&str> = text.split(separator).collect();
    while parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

/// Splits a `a|b|c` block into its non-empty candidates.
pub(crate) fn split_candidates(block: &str) -> Vec<String> {
    block.split('|').filter(|part| !part.is_empty()).map(str::to_string).collect()
}

/// Iterates the map's keys after `start` (exclusive) that begin with `prefix`.
pub(crate) fn prefixed<'a: 'b, 'b, V>(
    map: &'a std::collections::BTreeMap<String, V>,
    start: &str,
    prefix: &'b str,
) -> impl Iterator<Item = (&'a String, &'a V)> + 'b {
    use std::ops::Bound;
    map.range::<str, _>((Bound::Excluded(start), Bound::Unbounded)).take_while(move |(key, _)| key.starts_with(prefix))
}
