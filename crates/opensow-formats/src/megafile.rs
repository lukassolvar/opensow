//! `CMegaFile` archives: `sprites.info` (index) + `sprites.data` (payload).
//!
//! Index layout:
//! ```text
//! u32 magic       0x01010101
//! u32 count
//! count × { name: NUL-terminated, each byte +10; u32 offset; u32 size }
//! ```
//! Offsets point into the `.data` file; entries are stored back to back.

use crate::{Error, Reader, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The first word of an index file.
pub const MAGIC: u32 = 0x0101_0101;
const NAME_SHIFT: u8 = 10;

/// One file of the archive, as the index lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Original Windows-style path, e.g. `units\tur_05_blue.ps6`.
    pub name: String,
    /// Where the file starts in the `.data` file.
    pub offset: u32,
    /// Its length in bytes.
    pub size: u32,
}

/// Builds `sprites.info` / `sprites.data` contents from (game path, bytes) pairs, in the given order.
pub fn build(files: &[(String, Vec<u8>)]) -> (Vec<u8>, Vec<u8>) {
    let (mut info, mut data) = (Vec::new(), Vec::new());
    info.extend(MAGIC.to_le_bytes());
    info.extend((files.len() as u32).to_le_bytes());
    for (name, bytes) in files {
        info.extend(name.bytes().map(|b| b.wrapping_add(NAME_SHIFT)));
        info.push(0);
        info.extend((data.len() as u32).to_le_bytes());
        info.extend((bytes.len() as u32).to_le_bytes());
        data.extend(bytes);
    }
    (info, data)
}

/// Parses an index file (`sprites.info`).
pub fn parse_index(info: &[u8]) -> Result<Vec<Entry>> {
    let mut r = Reader::new(info);
    let magic = r.u32("megafile magic")?;
    if magic != MAGIC {
        return Err(Error::Invalid(format!("megafile magic {magic:#010x}")));
    }
    let count = r.u32("megafile count")?;
    (0..count)
        .map(|_| {
            let name = r.cstr("megafile name")?.iter().map(|b| b.wrapping_sub(NAME_SHIFT) as char).collect();
            Ok(Entry { name, offset: r.u32("megafile offset")?, size: r.u32("megafile size")? })
        })
        .collect()
}

/// An opened archive; reads entries lazily from the data file.
pub struct MegaFile {
    /// Every file of the archive, in index order.
    pub entries: Vec<Entry>,
    data_path: PathBuf,
}

impl MegaFile {
    /// Opens an archive: reads the whole index, and remembers where the data file is.
    pub fn open(info_path: &Path, data_path: &Path) -> Result<MegaFile> {
        let entries = parse_index(&std::fs::read(info_path)?)?;
        let data_len = std::fs::metadata(data_path)?.len();
        if let Some(e) = entries.iter().find(|e| e.offset as u64 + e.size as u64 > data_len) {
            return Err(Error::Invalid(format!("entry {} extends past end of {}", e.name, data_path.display())));
        }
        Ok(MegaFile { entries, data_path: data_path.to_owned() })
    }

    /// Case-insensitive lookup by game path (`\` or `/` separators).
    pub fn find(&self, name: &str) -> Option<&Entry> {
        let norm = |s: &str| s.replace('/', "\\").to_ascii_lowercase();
        let want = norm(name);
        self.entries.iter().find(|e| norm(&e.name) == want)
    }

    /// Reads one file's bytes from the data file.
    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>> {
        let mut f = File::open(&self.data_path)?;
        f.seek(SeekFrom::Start(entry.offset as u64))?;
        let mut buf = vec![0; entry.size as usize];
        f.read_exact(&mut buf)?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shifted_names() {
        let mut info = Vec::new();
        info.extend(MAGIC.to_le_bytes());
        info.extend(1u32.to_le_bytes());
        info.extend(b"a\\b.ps6".iter().map(|b| b + NAME_SHIFT));
        info.push(0);
        info.extend(16u32.to_le_bytes());
        info.extend(32u32.to_le_bytes());
        let e = parse_index(&info).unwrap();
        assert_eq!(e, vec![Entry { name: "a\\b.ps6".into(), offset: 16, size: 32 }]);
    }
}
