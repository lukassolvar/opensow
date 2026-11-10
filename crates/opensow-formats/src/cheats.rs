//! The chat line's cheat codes, decoded from `State of War.exe` (the key-character function `0x426c90`, docs/chat.md).
//!
//! The window procedure passes every typed character to `0x426c90`. Enter (13) in a single-player game compares the typed
//! text, character by character with a length test before each one, against nine phrases; a phrase matches when the text
//! **starts with it** (nothing checks what follows) and the comparison is case sensitive (the typed text keeps the case the
//! shift key gave). Each phrase has its own effect.

/// One of the exe's cheat phrases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cheat {
    /// `give me money` (`0x426f64`): +1000 credits for the local player.
    Money,
    /// `speed up my research` (`0x4272a0`): +200 research points.
    Research,
    /// `give me fighter` (`0x42751e`): one fighter (air force code 32).
    Fighter,
    /// `give me bomber` (`0x427786`): one bomber (code 30).
    Bomber,
    /// `give me meteor storm` (`0x427ad2`): one asteroid strike (code 34).
    MeteorStorm,
    /// `give me carryall` (`0x427d86`): one carryall (code 31).
    Carryall,
    /// `give me tripler` (`0x428014`): one tripler (code 33).
    Tripler,
    /// `give me one minute` (`0x428310`): +1 minute on the mission countdown while it still runs (`[0x5baa99] == 0`).
    OneMinute,
    /// `draw fps` (`0x428460`): shows the frame counter (`[0x5baad5]`, drawn by `0x4302e0` as "FPS: n" at the screen middle).
    DrawFps,
}

/// The phrases in the exe's order of comparison.
pub const PHRASES: [(&str, Cheat); 9] = [
    ("give me money", Cheat::Money),
    ("speed up my research", Cheat::Research),
    ("give me fighter", Cheat::Fighter),
    ("give me bomber", Cheat::Bomber),
    ("give me meteor storm", Cheat::MeteorStorm),
    ("give me carryall", Cheat::Carryall),
    ("give me tripler", Cheat::Tripler),
    ("give me one minute", Cheat::OneMinute),
    ("draw fps", Cheat::DrawFps),
];

impl Cheat {
    /// The air force code a cheat hands out (`0x453e80` argument), `None` for the other cheats.
    pub fn air_code(self) -> Option<i32> {
        match self {
            Cheat::Bomber => Some(30),
            Cheat::Carryall => Some(31),
            Cheat::Fighter => Some(32),
            Cheat::Tripler => Some(33),
            Cheat::MeteorStorm => Some(34),
            _ => None,
        }
    }
}

/// Credits the money cheat adds (`add [0x517af8 + 4*player], 0x3e8`).
pub const MONEY: i32 = 1000;
/// Research points the research cheat adds (`add [0x517b04 + 4*player], 0xc8`).
pub const RESEARCH: i32 = 200;

/// The cheat a typed line triggers: every phrase is tried in order, the first one the text starts with wins.
pub fn parse(text: &str) -> Option<Cheat> {
    PHRASES.iter().find(|(p, _)| text.starts_with(p)).map(|&(_, c)| c)
}

/// The chat line stops taking characters once its pixel width reaches this (`cmp [ebp-0x18], 0x18b` after the width query
/// at `0x428530`), measured with the message font.
pub const MAX_LINE_WIDTH: i32 = 0x18b;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phrase_is_found() {
        for &(p, c) in &PHRASES {
            assert_eq!(parse(p), Some(c), "{p}");
        }
    }

    #[test]
    fn a_phrase_matches_by_prefix_and_case_sensitively() {
        assert_eq!(parse("give me moneyyy"), Some(Cheat::Money));
        assert_eq!(parse("give me mone"), None);
        assert_eq!(parse("Give me money"), None);
        assert_eq!(parse("hello"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn the_air_forces_use_the_exes_codes() {
        let codes: Vec<_> = PHRASES.iter().filter_map(|&(_, c)| c.air_code()).collect();
        assert_eq!(codes, [32, 30, 34, 31, 33]);
    }
}
