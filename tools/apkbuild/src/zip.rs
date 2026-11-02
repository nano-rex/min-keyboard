//! Just enough ZIP to repackage what `aapt2 link` writes: read its entries without
//! decompressing them, add stored entries, and lay everything out the way `zipalign` would
//! (stored data 4-byte aligned, native libraries page aligned).

use std::io;

use crate::hash::crc32;

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL: u32 = 0x0605_4b50;
/// 1981-01-01 00:00, so builds are reproducible.
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = (1 << 5) | 1 | (1 << 9);

pub const STORED: u16 = 0;

pub struct Entry {
    pub name: String,
    pub method: u16,
    pub crc: u32,
    pub uncompressed_size: u32,
    /// The data as it goes into the archive (compressed when `method` is deflate).
    pub data: Vec<u8>,
}

impl Entry {
    pub fn stored(name: &str, data: Vec<u8>) -> Self {
        Entry { name: name.to_string(), method: STORED, crc: crc32(&data), uncompressed_size: data.len() as u32, data }
    }
}

fn u16_at(b: &[u8], at: usize) -> io::Result<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(truncated)
}

fn u32_at(b: &[u8], at: usize) -> io::Result<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(truncated)
}

fn truncated() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "truncated zip")
}

/// Reads every entry of a ZIP file, keeping compressed data as is.
pub fn read(bytes: &[u8]) -> io::Result<Vec<Entry>> {
    let min = bytes.len().saturating_sub(22 + 0xFFFF);
    let eocd = (min..=bytes.len().saturating_sub(22))
        .rev()
        .find(|&at| u32_at(bytes, at).ok() == Some(END_OF_CENTRAL))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no end of central directory"))?;
    let count = u16_at(bytes, eocd + 10)? as usize;
    let mut at = u32_at(bytes, eocd + 16)? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(bytes, at)? != CENTRAL_HEADER {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "bad central directory entry"));
        }
        let method = u16_at(bytes, at + 10)?;
        let crc = u32_at(bytes, at + 16)?;
        let compressed_size = u32_at(bytes, at + 20)? as usize;
        let uncompressed_size = u32_at(bytes, at + 24)?;
        let name_len = u16_at(bytes, at + 28)? as usize;
        let extra_len = u16_at(bytes, at + 30)? as usize;
        let comment_len = u16_at(bytes, at + 32)? as usize;
        let local = u32_at(bytes, at + 42)? as usize;
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + name_len).ok_or_else(truncated)?).into_owned();
        if u32_at(bytes, local)? != LOCAL_HEADER {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("bad local header for {name}")));
        }
        let data_start = local + 30 + u16_at(bytes, local + 26)? as usize + u16_at(bytes, local + 28)? as usize;
        let data = bytes.get(data_start..data_start + compressed_size).ok_or_else(truncated)?.to_vec();
        entries.push(Entry { name, method, crc, uncompressed_size, data });
        at += 46 + name_len + extra_len + comment_len;
    }
    Ok(entries)
}

/// Where each section of the written archive starts, for the APK signer.
pub struct Layout {
    pub central_directory_offset: usize,
    pub end_of_central_directory_offset: usize,
}

/// Writes the archive. Stored entries are aligned to 4 bytes, or to `page` for `.so` files.
pub fn write(entries: &[Entry], page: usize) -> (Vec<u8>, Layout) {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = out.len();
        let name = entry.name.as_bytes();
        let alignment = if entry.method != STORED {
            1
        } else if entry.name.ends_with(".so") {
            page
        } else {
            4
        };
        let unpadded = offset + 30 + name.len();
        let padding = (alignment - unpadded % alignment) % alignment;
        let header = |buf: &mut Vec<u8>, central: bool| {
            if central {
                buf.extend_from_slice(&CENTRAL_HEADER.to_le_bytes());
                buf.extend_from_slice(&20u16.to_le_bytes()); // made by
            } else {
                buf.extend_from_slice(&LOCAL_HEADER.to_le_bytes());
            }
            buf.extend_from_slice(&20u16.to_le_bytes()); // version needed
            buf.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
            buf.extend_from_slice(&entry.method.to_le_bytes());
            buf.extend_from_slice(&DOS_TIME.to_le_bytes());
            buf.extend_from_slice(&DOS_DATE.to_le_bytes());
            buf.extend_from_slice(&entry.crc.to_le_bytes());
            buf.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
            buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        };
        header(&mut out, false);
        out.extend_from_slice(&(padding as u16).to_le_bytes());
        out.extend_from_slice(name);
        out.extend(std::iter::repeat_n(0u8, padding));
        out.extend_from_slice(&entry.data);

        header(&mut central, true);
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&(offset as u32).to_le_bytes());
        central.extend_from_slice(name);
    }
    let central_directory_offset = out.len();
    out.extend_from_slice(&central);
    let end_of_central_directory_offset = out.len();
    out.extend_from_slice(&END_OF_CENTRAL.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]); // disk numbers
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&(central_directory_offset as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment
    (out, Layout { central_directory_offset, end_of_central_directory_offset })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_alignment() {
        let entries = vec![Entry::stored("a.txt", b"hello".to_vec()), Entry::stored("lib/arm64-v8a/libx.so", vec![7; 100])];
        let (bytes, layout) = write(&entries, 16384);
        let back = read(&bytes).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].data, b"hello");
        assert_eq!(back[1].crc, crc32(&[7; 100]));
        let so_offset = bytes.windows(100).position(|w| w == [7u8; 100]).unwrap();
        assert_eq!(so_offset % 16384, 0);
        assert_eq!(u32_at(&bytes, layout.central_directory_offset).unwrap(), CENTRAL_HEADER);
    }
}
