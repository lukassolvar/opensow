//! The data of State of War Warmonger that the base game does not have (docs/warmonger.md).
//!
//! Warmonger is a standalone expansion on the same engine with the same assets; what it adds is eleven campaign missions on the same
//! 22-land world map (land `L` = mission `M<L>`), six skirmish maps, and the land texts, which are not in language files but compiled
//! into its exe (`0x43d250` picks one of 11 cases from the table `0x43d6e8`).

/// The lands that have a mission, in the order of the texts in the exe (case 0..10 of the table `0x43d6e8`).
pub const LANDS: [usize; 11] = [1, 2, 3, 4, 6, 7, 11, 12, 15, 20, 22];

/// The four strings the exe shows for a land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandText {
    /// `TERRITORY : nn   DIFFICULTY : d / 10`
    pub heading: String,
    /// `Coordinate: X8A3`
    pub coordinate: String,
    /// The briefing text.
    pub briefing: String,
    /// The objective text.
    pub objective: String,
}

/// Mission number -> index into [`LANDS`] (and the texts).
pub fn case_of(land: usize) -> Option<usize> {
    LANDS.iter().position(|&l| l == land)
}

/// Reads the eleven land texts out of Warmonger's exe: each starts at a `TERRITORY : nn` heading followed by the coordinate, briefing
/// and objective strings (NUL terminated, padded). Returns them in case order; `None` when the exe has not exactly 11 of them.
pub fn parse_texts(exe: &[u8]) -> Option<Vec<LandText>> {
    const KEY: &[u8] = b"TERRITORY : ";
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(i) = find(exe, KEY, from) {
        let (strings, pos) = strings_at(exe, i, 4)?;
        let [heading, coordinate, briefing, objective] = strings.try_into().ok()?;
        found.push(LandText { heading, coordinate, briefing, objective });
        from = pos;
    }
    (found.len() == LANDS.len()).then_some(found)
}

/// Warmonger's English lines of the mission hint boxes. Its hint routines (`0x42f650`, `0x42fa30`) are the base game's
/// (`0x4303e0`, `0x430770`) instruction for instruction, but they push three string literals where the base game pushes the
/// language strings 280, 280 and 281 (docs/warmonger.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hints {
    /// No slot at its limit (missions 1 and 6, box 0x94); the same text as the base game's string 280.
    pub upgrade: String,
    /// One slot at its limit (mission 1, box 0xbc); the base game shows string 280 again.
    pub upgrade_next: String,
    /// Eight fighters in stock (mission 1, box 0xc1); the base game's 281 names the A.A. turrets on the island.
    pub fighters: String,
}

/// Reads [`Hints`] out of Warmonger's exe: the three NUL-terminated strings from the first `Now increase tech-level`.
pub fn parse_hints(exe: &[u8]) -> Option<Hints> {
    let (strings, _) = strings_at(exe, find(exe, b"Now increase tech-level", 0)?, 3)?;
    let [upgrade, upgrade_next, fighters] = strings.try_into().ok()?;
    Some(Hints { upgrade, upgrade_next, fighters })
}

/// `n` NUL-terminated strings from `pos`, skipping the NUL padding between them, and the position after the last.
fn strings_at(exe: &[u8], mut pos: usize, n: usize) -> Option<(Vec<String>, usize)> {
    let mut strings = Vec::with_capacity(n);
    while strings.len() < n {
        while exe.get(pos) == Some(&0) {
            pos += 1;
        }
        let end = pos + exe.get(pos..)?.iter().position(|&b| b == 0)?;
        strings.push(String::from_utf8_lossy(&exe[pos..end]).into_owned());
        pos = end;
    }
    Some((strings, pos))
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake() -> Vec<u8> {
        let mut v = b"junk\0\0".to_vec();
        for n in 1..=11 {
            v.extend(format!("TERRITORY : {n:02}   DIFFICULTY : 2 / 10\0\0\0Coordinate: A{n}\0\0Brief {n}.\0\0\0\0Objective: {n}\0").bytes());
            v.extend(b"\0\0\0");
        }
        v
    }

    #[test]
    fn parses_eleven_land_texts() {
        let t = parse_texts(&fake()).unwrap();
        assert_eq!(t.len(), 11);
        assert_eq!(
            t[3],
            LandText {
                heading: "TERRITORY : 04   DIFFICULTY : 2 / 10".into(),
                coordinate: "Coordinate: A4".into(),
                briefing: "Brief 4.".into(),
                objective: "Objective: 4".into()
            }
        );
        assert_eq!(case_of(22), Some(10));
        assert_eq!(case_of(5), None);
    }

    #[test]
    fn parses_the_three_hint_lines_after_the_first_one() {
        let exe = b"credits\0\0\0Now increase tech-level here.\0\0\0\0Now increase tech-level of next.\0\0\0Now use fighters.\0\0\0sprites2\0";
        assert_eq!(
            parse_hints(exe),
            Some(Hints {
                upgrade: "Now increase tech-level here.".into(),
                upgrade_next: "Now increase tech-level of next.".into(),
                fighters: "Now use fighters.".into()
            })
        );
        assert_eq!(parse_hints(b"Now increase tech-level here.\0\0only two\0"), None);
        assert_eq!(parse_hints(b"nothing here"), None);
    }

    #[test]
    fn rejects_a_short_or_truncated_table() {
        let mut v = fake();
        v.truncate(v.len() - 40);
        assert!(parse_texts(&v).is_none());
        assert!(parse_texts(b"nothing here").is_none());
    }
}
