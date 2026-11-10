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

    /// The languages the port offers: the ones whose shipped text and language-pack art are complete. The Polish and Russian
    /// files are English text with placeholders and the Hungarian one is damaged, with no art of their own (docs/text.md); a
    /// launcher setting, `lang.dat` or `--lang` naming one of those falls back to English.
    pub const SUPPORTED: [Language; 5] = [Language::English, Language::Slovak, Language::Czech, Language::ChineseSimplified, Language::ChineseTraditional];

    pub fn supported(self) -> bool {
        Language::SUPPORTED.contains(&self)
    }

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

    /// The folder below `Data/DiffData` with this language's voices and pictures (both Chinese scripts share one); English has none.
    pub fn diff_dir(self) -> Option<&'static str> {
        match self {
            Language::English => None,
            Language::Slovak => Some("SK"),
            Language::Czech => Some("CZ"),
            Language::Polish => Some("PL"),
            Language::Hungarian => Some("HU"),
            Language::Russian => Some("RU"),
            Language::ChineseSimplified | Language::ChineseTraditional => Some("CHINNESSE"),
        }
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

/// The English text of the strings `with_supplement` may replace (file indices).
const SUPPLEMENT_ENGLISH: &[(usize, &str)] = &[
    (262, "Selected level:"), (265, "2Green player has joined the game"), (266, "2Green player has left the game"), (267, "On"), (268, "Off"),
    (278, "Green Player has left the game"), (279, "Blue Player has left the game"), (282, "No Info Available"), (283, "NAME: %s"),
    (284, "BLAST: %s"), (285, "SHELL: %s"), (286, "TYPE: Ground unit"), (287, "TYPE: Walker robot"), (288, "TYPE: Production"),
    (289, "TYPE: Sentry Building"), (290, "TYPE: Air Forces"), (291, "TYPE: Advancer"), (292, "TYPE: Building"), (293, "OFFENCE: %d"),
    (294, "DEFENCE: %d"), (295, "OFFENCE: n/a"), (296, "OFFENCE: special"), (297, "OFFENCE: ultimate"), (298, "SCOPE: %s"),
    (299, "SPEED: %s"), (300, "SPEED: %d"), (301, "cargo: reinforcements"),
];

const SUPPLEMENT_SK: &[(usize, &str)] = &[
    (262, "Vybraná úroveň:"), (265, "2Zelený hráč sa pripojil k hre"), (266, "2Zelený hráč opustil hru"), (267, "Zap"), (268, "Vyp"),
    (278, "Zelený hráč opustil hru"), (279, "Modrý hráč opustil hru"), (282, "Informácie nie sú k dispozícii"), (283, "NÁZOV: %s"),
    (284, "VÝBUCH: %s"), (285, "NÁBOJ: %s"), (286, "TYP: Pozemná jednotka"), (287, "TYP: Kráčajúci robot"), (288, "TYP: Výroba"),
    (289, "TYP: Strážna veža"), (290, "TYP: Letectvo"), (291, "TYP: Advancer"), (292, "TYP: Budova"), (293, "ÚTOK: %d"),
    (294, "OBRANA: %d"), (295, "ÚTOK: n/a"), (296, "ÚTOK: špeciálny"), (297, "ÚTOK: najvyšší"), (298, "DOSAH: %s"),
    (299, "RÝCHLOSŤ: %s"), (300, "RÝCHLOSŤ: %d"), (301, "náklad: posily"),
];

const SUPPLEMENT_CZ: &[(usize, &str)] = &[
    (262, "Vybraná úroveň:"), (265, "2Zelený hráč se připojil ke hře"), (266, "2Zelený hráč opustil hru"), (267, "Zap"), (268, "Vyp"),
    (278, "Zelený hráč opustil hru"), (279, "Modrý hráč opustil hru"), (282, "Informace nejsou k dispozici"), (283, "NÁZEV: %s"),
    (284, "VÝBUCH: %s"), (285, "NÁBOJ: %s"), (286, "TYP: Pozemní jednotka"), (287, "TYP: Kráčející robot"), (288, "TYP: Výroba"),
    (289, "TYP: Strážní věž"), (290, "TYP: Letectvo"), (291, "TYP: Advancer"), (292, "TYP: Budova"), (293, "ÚTOK: %d"),
    (294, "OBRANA: %d"), (295, "ÚTOK: n/a"), (296, "ÚTOK: speciální"), (297, "ÚTOK: nejvyšší"), (298, "DOSAH: %s"),
    (299, "RYCHLOST: %s"), (300, "RYCHLOST: %d"), (301, "náklad: posily"),
];

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

    /// Fills in strings the shipped file leaves in English where the original game never had a translation (info box labels,
    /// On/Off, a few lobby lines). These are this port's own translations, not data from the install: only Slovak and Czech, and only
    /// strings that equal the English one (so a later fixed install file wins). `docs/text.md`.
    pub fn with_supplement(mut self, language: Language) -> LangFile {
        let table: &[(usize, &str)] = match language {
            Language::Slovak => SUPPLEMENT_SK,
            Language::Czech => SUPPLEMENT_CZ,
            _ => return self,
        };
        for &(i, text) in table {
            let is_english = SUPPLEMENT_ENGLISH.iter().any(|&(k, e)| k == i && self.strings.get(i).is_some_and(|s| s.trim() == e));
            if is_english {
                self.strings[i] = text.to_owned();
            }
        }
        self
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
    fn only_complete_translations_are_offered() {
        assert!(Language::Czech.supported() && Language::Slovak.supported() && Language::ChineseTraditional.supported());
        assert!(!Language::Polish.supported() && !Language::Hungarian.supported() && !Language::Russian.supported());
        assert_eq!(Language::SUPPORTED[0], Language::English);
    }

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

#[cfg(test)]
mod diff_dir_tests {
    use super::*;

    #[test]
    fn language_packs() {
        assert_eq!(Language::English.diff_dir(), None);
        assert_eq!(Language::Slovak.diff_dir(), Some("SK"));
        assert_eq!(Language::ChineseTraditional.diff_dir(), Language::ChineseSimplified.diff_dir());
    }
}
