//! Sound-effect ids, decoded from `State of War.exe` (loader `0x406dc0..0x407453` calling `0x407470(path, id)`; players `0x4075b0`
//! and `0x4077e0`, which take the id as their first argument).
//!
//! Paths are relative to `Data/Sounds`, lower-case as in the exe, written with `/`; resolve them case-insensitively (`GameDir::resolve`). Id 71 is unused.
//! Known uses: 53 `tt-capture`, 57 `tt-repair`, 58 `tt-Upgrade` (the Advancer's beam sounds, `0x4111b0`), 55 `tt-dest` (order
//! acknowledged, `0x46e5c0`), 72 `turret-place` (turret built), 4-6 `canon` shots, 1-3 `artilery`, 0 `antiair`, 23-28 `explodes\*`.

pub const SFX: [(u8, &str); 72] = [
    (0, "sfx/groundunits/antiair/01.wav"),
    (1, "sfx/groundunits/artilery/01.wav"),
    (2, "sfx/groundunits/artilery/02.wav"),
    (3, "sfx/groundunits/artilery/03.wav"),
    (4, "sfx/groundunits/canon/01.wav"),
    (5, "sfx/groundunits/canon/02.wav"),
    (6, "sfx/groundunits/canon/03.wav"),
    (7, "sfx/groundunits/special/01.wav"),
    (8, "sfx/groundunits/special/02.wav"),
    (9, "sfx/groundunits/special/03.wav"),
    (10, "sfx/groundunits/flamer/01.wav"),
    (11, "sfx/turets/01.wav"),
    (12, "sfx/turets/02.wav"),
    (13, "sfx/turets/03.wav"),
    (14, "sfx/turets/04.wav"),
    (15, "sfx/turets/05.wav"),
    (16, "sfx/groundunits/walker/01.wav"),
    (17, "sfx/groundunits/walker/03.wav"),
    (18, "sfx/groundunits/walker/05.wav"),
    (19, "sfx/groundunits/walker/nuke.wav"),
    (20, "sfx/groundunits/antiair/strelba.wav"),
    (21, "sfx/groundunits/artilery/strelba.wav"),
    (22, "sfx/groundunits/canon/strelba.wav"),
    (23, "sfx/explodes/airunit.wav"),
    (24, "sfx/explodes/building.wav"),
    (25, "sfx/explodes/building2.wav"),
    (26, "sfx/explodes/sentry.wav"),
    (27, "sfx/explodes/unit.wav"),
    (28, "sfx/explodes/unit-turet.wav"),
    (29, "sfx/cinky/mainmenu-ok.wav"),
    (30, "sfx/cinky/mainmenu-move.wav"),
    (31, "sfx/cinky/mainmenu-fly.wav"),
    (32, "sfx/cinky/save-type.wav"),
    (33, "sfx/cinky/options-arrow.wav"),
    (34, "sfx/cinky/options-onoff.wav"),
    (35, "sfx/cinky/ingame-come.wav"),
    (36, "sfx/cinky/ingame-move.wav"),
    (37, "sfx/cinky/ingame-select.wav"),
    (38, "sfx/cinky/ingame-ano.wav"),
    (39, "sfx/cinky/ingame-nie.wav"),
    (40, "sfx/cinky/loadsave-pic.wav"),
    (41, "sfx/cinky/briefing-location.wav"),
    (42, "sfx/cinky/briefing-type.wav"),
    (43, "sfx/cinky/briefing-type02.wav"),
    (44, "sfx/cinky/briefing-rect01.wav"),
    (45, "sfx/cinky/briefing-rect02.wav"),
    (46, "sfx/cinky/briefing-blik.wav"),
    (47, "sfx/cinky/statistic-top.wav"),
    (48, "sfx/cinky/statistic-up.wav"),
    (49, "sfx/cinky2/airicon-come.wav"),
    (50, "sfx/cinky2/icon-move.wav"),
    (51, "sfx/cinky2/infobox-income.wav"),
    (52, "sfx/cinky2/infobox-outcome.wav"),
    (53, "sfx/cinky2/tt-capture.wav"),
    (54, "sfx/cinky2/tt-click.wav"),
    (55, "sfx/cinky2/tt-dest.wav"),
    (56, "sfx/cinky2/tt-fire.wav"),
    (57, "sfx/cinky2/tt-repair.wav"),
    (58, "sfx/cinky2/tt-Upgrade.wav"),
    (59, "sfx/cinky2/unit-attack.wav"),
    (60, "sfx/cinky2/unit-select.wav"),
    (61, "sfx/cinky2/unit-dest.wav"),
    (62, "sfx/AirForces/carryall-land.wav"),
    (63, "sfx/AirForces/hercules.wav"),
    (64, "sfx/AirForces/hercules-hvizd.wav"),
    (65, "sfx/AirForces/hercules-boom.wav"),
    (66, "sfx/AirForces/stihac.wav"),
    (67, "sfx/AirForces/stihac-boom.wav"),
    (68, "sfx/cinky2/airforce-dest.wav"),
    (69, "sfx/AirForces/tripler-fire.wav"),
    (70, "sfx/AirForces/meteor.wav"),
    (72, "sfx/turets/turret-place.wav"),
];

pub fn path(id: u8) -> Option<&'static str> {
    SFX.iter().find(|s| s.0 == id).map(|s| s.1)
}

/// Muzzle sound of a weapon kind (`docs/projectiles.md`, `opensow weapons`): the volley spawner `0x47cf40` plays one
/// sound at the fire point after it creates the bullets (switch on the kind through the byte table `0x47d28c` and the jump
/// table `0x47d244`). `jitter` is the fourth argument of `0x4075b0`: a random pitch change of up to 10 % (off for
/// walkers 1 and 3). Kinds 14, 15, 44 and every non-weapon kind make no muzzle sound.
pub fn muzzle(kind: u8) -> Option<(u8, bool)> {
    let id = match kind {
        1..=3 | 42 => 0,
        4 => 1,
        5 => 2,
        6 => 3,
        7 => 4,
        8 => 5,
        9 => 6,
        10..=12 => 10,
        13 => 7,
        41 => 16,
        43 => 17,
        45 => 18,
        109..=113 => kind - 109 + 11,
        _ => return None,
    };
    Some((id, !matches!(kind, 41 | 43)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup() {
        assert_eq!(path(58), Some("sfx/cinky2/tt-Upgrade.wav"));
        assert!(path(71).is_none());
        assert_eq!(SFX.len(), 72);
    }

    #[test]
    fn muzzle_sounds() {
        assert_eq!(muzzle(5), Some((2, true)));
        assert_eq!(muzzle(41), Some((16, false)));
        assert_eq!(muzzle(111), Some((13, true)));
        assert_eq!(muzzle(14), None);
        assert!(muzzle(200).is_none());
        for k in 0..=255u8 {
            if let Some((id, _)) = muzzle(k) {
                assert!(path(id).is_some());
            }
        }
    }
}
