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
    pub briefing: String,
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
        let mut pos = i;
        let mut strings = Vec::new();
        while strings.len() < 4 {
            while exe.get(pos) == Some(&0) {
                pos += 1;
            }
            let end = pos + exe.get(pos..)?.iter().position(|&b| b == 0)?;
            strings.push(String::from_utf8_lossy(&exe[pos..end]).into_owned());
            pos = end;
        }
        let mut it = strings.into_iter();
        let (heading, coordinate, briefing, objective) = (it.next()?, it.next()?, it.next()?, it.next()?);
        found.push(LandText { heading, coordinate, briefing, objective });
        from = pos;
    }
    (found.len() == LANDS.len()).then_some(found)
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
        assert_eq!(t[3], LandText { heading: "TERRITORY : 04   DIFFICULTY : 2 / 10".into(), coordinate: "Coordinate: A4".into(), briefing: "Brief 4.".into(), objective: "Objective: 4".into() });
        assert_eq!(case_of(22), Some(10));
        assert_eq!(case_of(5), None);
    }

    #[test]
    fn rejects_a_short_or_truncated_table() {
        let mut v = fake();
        v.truncate(v.len() - 40);
        assert!(parse_texts(&v).is_none());
        assert!(parse_texts(b"nothing here").is_none());
    }
}
