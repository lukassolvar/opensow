//! Weapon offset tables: `units\{top}.po1` and `units\{top}.po2` (one pair per weapon top, in
//! `sprites.data` and loose in `Data/units`).
//!
//! A file is a list of tables, each 32 `(i16 dx, i16 dy)` points (128 bytes), one point per top-sprite facing (the
//! two rows of 16 frames, running clockwise from north). `.po1` has one table per simultaneous projectile or barrel
//! (`size / 128`: 1 for most tops, 2 for twin guns `1AA`, 6 for `3AA`); `.po2` has one table (the weapon's reference
//! point).
//!
//! The projectile spawner (`0x47cf70`, loaders `0x437f10`) indexes `po1[variant * 32 + facing]` and starts the shot at
//! `unit position - (dx, dy)`, so the stored value is the *negated* offset from the unit's sprite anchor to the muzzle.
//! The points trace a squashed circle around the turret pivot (about 16 px above the anchor, radius = barrel length,
//! vertical radius about 0.7x for the 3/4 view), e.g. `1C.po1[8] = (-20, -1)` puts the muzzle 20 px to the right,
//! which is where the barrel is drawn at frame 8.

use crate::{Error, Result};

pub const FACINGS: usize = 32;
const TABLE_BYTES: usize = FACINGS * 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireTables {
    tables: Vec<[(i16, i16); FACINGS]>,
}

impl FireTables {
    pub fn parse(data: &[u8]) -> Result<FireTables> {
        if data.is_empty() || data.len() % TABLE_BYTES != 0 {
            return Err(Error::Invalid(format!("offset file size {} is not a multiple of {TABLE_BYTES}", data.len())));
        }
        let tables = data
            .chunks_exact(TABLE_BYTES)
            .map(|t| {
                std::array::from_fn(|i| {
                    let p = &t[i * 4..i * 4 + 4];
                    (i16::from_le_bytes([p[0], p[1]]), i16::from_le_bytes([p[2], p[3]]))
                })
            })
            .collect();
        Ok(FireTables { tables })
    }

    /// Number of tables (barrels / simultaneous projectiles for `.po1`).
    pub fn variants(&self) -> usize {
        self.tables.len()
    }

    /// The raw stored point (subtracted from the unit position by the game).
    pub fn raw(&self, variant: usize, facing: usize) -> Option<(i16, i16)> {
        self.tables.get(variant)?.get(facing).copied()
    }

    /// Offset from the unit's sprite anchor to the muzzle (screen pixels, y down).
    pub fn muzzle(&self, variant: usize, facing: usize) -> Option<(i32, i32)> {
        self.raw(variant, facing).map(|(x, y)| (-(x as i32), -(y as i32)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(f: impl Fn(usize) -> (i16, i16)) -> Vec<u8> {
        (0..FACINGS).flat_map(|i| { let (x, y) = f(i); [x.to_le_bytes(), y.to_le_bytes()].concat() }).collect()
    }

    #[test]
    fn parses_variants_and_negates() {
        let mut d = table(|i| (i as i16, -(i as i16)));
        d.extend(table(|_| (7, 9)));
        let t = FireTables::parse(&d).unwrap();
        assert_eq!(t.variants(), 2);
        assert_eq!(t.raw(0, 3), Some((3, -3)));
        assert_eq!(t.muzzle(1, 31), Some((-7, -9)));
        assert_eq!(t.raw(2, 0), None);
        assert_eq!(t.raw(0, 32), None);
    }

    #[test]
    fn rejects_bad_sizes() {
        assert!(FireTables::parse(&[]).is_err());
        assert!(FireTables::parse(&[0; 127]).is_err());
        assert!(FireTables::parse(&[0; 129]).is_err());
    }
}
