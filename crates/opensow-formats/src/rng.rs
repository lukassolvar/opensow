//! The game's deterministic random numbers, decoded from `State of War.exe` (`0x437b10`, loader `0x437540`, start-up `0x422f5f`).
//!
//! `Data/generic.file` (111111 bytes, identical in every language pack, looks like noise) is a table of little-endian
//! 16-bit words. `rand(n)` (`0x437b10`) returns 0 for `n == 0`, else `table[index] % n` and advances `index`, wrapping to 0
//! when it passes `0xc346` (so 50 001 words are used). The index starts at 0 (`[0x71c0f4] = 0` at start-up). All game logic that
//! must stay in lockstep in multiplayer uses this generator; purely visual randomness uses `0x437cd0`, which draws from a
//! second table filled at start-up with the C runtime's `rand()` and is not reproducible.

/// Number of table entries the generator walks through (`0..=0xc346`).
pub const TABLE_LEN: usize = 0xc346 + 1;

/// The game's lockstep random numbers: the table read from `generic.file` and a position in it.
#[derive(Debug, Clone)]
pub struct GameRng {
    table: Vec<u16>,
    index: usize,
}

impl GameRng {
    /// Puts the table position back (the only state a savegame keeps; `index()` reads it).
    pub fn set_index(&mut self, i: usize) {
        self.index = i % self.table.len().max(1);
    }

    /// Reads `generic.file`; `None` when it is too short.
    pub fn from_file(data: &[u8]) -> Option<GameRng> {
        if data.len() < TABLE_LEN * 2 {
            return None;
        }
        let table = data.chunks_exact(2).take(TABLE_LEN).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Some(GameRng { table, index: 0 })
    }

    /// `rand(n)` of the exe.
    pub fn next(&mut self, n: u16) -> u16 {
        if n == 0 {
            return 0;
        }
        let v = self.table[self.index];
        self.index += 1;
        if self.index >= TABLE_LEN {
            self.index = 0;
        }
        v % n
    }

    /// The current position in the table.
    pub fn index(&self) -> usize {
        self.index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_the_table_and_wraps() {
        let data: Vec<u8> = (0..TABLE_LEN as u32).flat_map(|i| ((i % 1000) as u16).to_le_bytes()).collect();
        let mut r = GameRng::from_file(&data).unwrap();
        assert_eq!((r.next(7), r.next(7), r.next(0)), (0, 1, 0));
        assert_eq!(r.index(), 2);
        for _ in 2..TABLE_LEN {
            r.next(10);
        }
        assert_eq!(r.index(), 0);
        assert!(GameRng::from_file(&data[..100]).is_none());
    }
}
