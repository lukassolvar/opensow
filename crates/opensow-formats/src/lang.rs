//! Language selection and text tables.
//!
//! - `Data/lang.dat` holds one ASCII digit, the language id (`'0'` in the Steam install).
//! - The id selects `Data/Language/lang{EN,SK,CZ,PL,HU,RU,CS,CT}.txt` (jump table at `0x466f20` in
//!   `State of War.exe`; the same order as the in-game language menu).
//! - The Steam build's files are UTF-8 with a BOM and CRLF line breaks. The exe reads the whole file, splits it with
//!   `strtok(buf, "@")` (so empty fields are skipped) and indexes the pieces: [`LangFile::get`]. No shipped file has
//!   `@@`, so splitting and `strtok` agree. Every file holds 364 strings (index 0 = "New Game" in English); a BOM in front
//!   of the first `@` and a lone `\r\n` after the last one are not strings. How the exe treats the BOM when numbering
//!   (the original, pre-Steam files had none) was not traced.
//! - Menu and dialog text is drawn through Windows GDI: the exe converts with `MultiByteToWideChar(CP_UTF8)` and calls
//!   `TextOutW` twice (a shadow at +2,+2, then the text; helper at `0x466f40`). The bitmap fonts of [`crate::font`] only
//!   cover ASCII 32..=125, so they cannot show the accented or Chinese strings.

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Slovak,
    Czech,
    Polish,
    Hungarian,
    Russian,
    ChineseSimplified,
    ChineseTraditional,
}

impl Language {
    /// In id order (the value stored in `lang.dat`).
    pub const ALL: [Language; 8] = [
        Language::English,
        Language::Slovak,
        Language::Czech,
        Language::Polish,
        Language::Hungarian,
        Language::Russian,
        Language::ChineseSimplified,
        Language::ChineseTraditional,
    ];

    pub fn id(self) -> u8 {
        Language::ALL.iter().position(|&l| l == self).unwrap() as u8
    }

    pub fn from_id(id: u8) -> Option<Language> {
        Language::ALL.get(id as usize).copied()
    }

    pub fn code(self) -> &'static str {
        ["EN", "SK", "CZ", "PL", "HU", "RU", "CS", "CT"][self.id() as usize]
    }

    pub fn from_code(code: &str) -> Option<Language> {
        Language::ALL.into_iter().find(|l| l.code().eq_ignore_ascii_case(code))
    }

    /// `langEN.txt` etc., relative to `Data/Language`.
    pub fn file_name(self) -> String {
        format!("lang{}.txt", self.code())
    }
}

/// Reads `Data/lang.dat`: a single ASCII digit.
pub fn read_lang_dat(data: &[u8]) -> Option<Language> {
    let b = *data.first()?;
    b.checked_sub(b'0').and_then(Language::from_id)
}

#[derive(Debug, Clone)]
pub struct LangFile {
    pub strings: Vec<String>,
}

impl LangFile {
    pub fn parse(data: &[u8]) -> Result<LangFile> {
        let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
        let text = std::str::from_utf8(data).map_err(|e| Error::Invalid(format!("language file is not UTF-8: {e}")))?;
        // Like strtok: empty pieces (before the first '@', after the last) are not strings. Line breaks inside a piece are kept.
        let strings: Vec<String> = text.split('@').filter(|s| !s.is_empty() && *s != "\r\n").map(str::to_owned).collect();
        if strings.is_empty() {
            return Err(Error::Invalid("language file has no strings".into()));
        }
        Ok(LangFile { strings })
    }

    pub fn get(&self, index: usize) -> Option<&str> {
        self.strings.get(index).map(String::as_str)
    }

    /// Indices of strings containing characters that show the text was damaged by a bad re-encoding:
    /// U+FFFD replacement characters or half-width katakana (what single-byte Hungarian codes turn into when read as Shift-JIS).
    pub fn damaged(&self) -> Vec<usize> {
        self.strings
            .iter()
            .enumerate()
            .filter(|(_, s)| s.chars().any(|c| c == '\u{FFFD}' || ('\u{FF61}'..='\u{FF9F}').contains(&c)))
            .map(|(i, _)| i)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_ids() {
        assert_eq!(Language::from_id(2), Some(Language::Czech));
        assert_eq!(Language::Czech.file_name(), "langCZ.txt");
        assert_eq!(Language::from_code("ct"), Some(Language::ChineseTraditional));
        assert_eq!(Language::ChineseSimplified.id(), 6);
        assert_eq!(read_lang_dat(b"0"), Some(Language::English));
        assert_eq!(read_lang_dat(b"7\r\n"), Some(Language::ChineseTraditional));
        assert_eq!(read_lang_dat(b"8"), None);
        assert_eq!(read_lang_dat(b""), None);
    }

    #[test]
    fn parses_bom_crlf_and_multiline_fields() {
        let mut d = vec![0xEF, 0xBB, 0xBF];
        d.extend("@New Game@První\r\ndruhý řádek@Konec@\r\n".as_bytes());
        let f = LangFile::parse(&d).unwrap();
        assert_eq!(f.strings, ["New Game", "První\r\ndruhý řádek", "Konec"]);
        assert_eq!(f.get(1), Some("První\r\ndruhý řádek"));
        assert!(f.damaged().is_empty());
    }

    #[test]
    fn detects_damage_and_rejects_non_utf8() {
        let f = LangFile::parse("@ok@j\u{FFFD}t\u{FFFD}k@\u{FF9C}j@".as_bytes()).unwrap();
        assert_eq!(f.damaged(), [1, 2]);
        assert!(LangFile::parse(b"@bad\xE1\x20@").is_err());
        assert!(LangFile::parse(b"@@").is_err());
    }
}
