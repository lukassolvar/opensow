//! Team colours (not in the exe): the skirmish lets each player pick a colour (docs/skirmish.md). The game ships its team art
//! twice, blue (`units\\*_blue`, `builds\\blue\\`) and green (`units\\*_gren`, `builds\\green\\`); any other colour is that art
//! with its team pixels turned to another hue ([`recolour`]). Blue on the blue art and green on the green art stay the originals.

use opensow_formats::sprite::Frame;

/// A colour a player can play in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TeamColour {
    Blue,
    Green,
    Red,
    Yellow,
    Teal,
    Purple,
    Pink,
}

/// The art a sheet was painted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    Blue,
    Green,
}

impl Paint {
    /// The hue range (degrees) of the sheet's team pixels.
    fn band(self) -> (f32, f32) {
        match self {
            Paint::Blue => (190.0, 270.0),
            Paint::Green => (75.0, 170.0),
        }
    }

    /// Saturation and value gains that bring the sheet's team pixels to a common strength (the blue art is paler and lighter:
    /// mean s 0.51 / v 0.53 against the green art's 0.83 / 0.40, measured over unit and building sheets), so one colour looks
    /// the same on either player.
    fn gains(self) -> (f32, f32) {
        match self {
            Paint::Blue => (1.45, 0.88),
            Paint::Green => (0.9, 1.18),
        }
    }

    pub fn colour(self) -> TeamColour {
        match self {
            Paint::Blue => TeamColour::Blue,
            Paint::Green => TeamColour::Green,
        }
    }
}

impl TeamColour {
    /// The picker's order.
    pub const ALL: [TeamColour; 7] =
        [TeamColour::Blue, TeamColour::Green, TeamColour::Red, TeamColour::Yellow, TeamColour::Teal, TeamColour::Purple, TeamColour::Pink];

    /// The game's two sides as the exe colours them: player 0 blue, player 1 green.
    pub const DEFAULT: [TeamColour; 2] = [TeamColour::Blue, TeamColour::Green];

    /// Hue (degrees), saturation and value factors of the recoloured team pixels.
    fn paint(self) -> (f32, f32, f32) {
        match self {
            TeamColour::Blue => (228.0, 1.0, 1.0),
            TeamColour::Green => (115.0, 1.0, 1.0),
            TeamColour::Red => (0.0, 1.0, 1.0),
            TeamColour::Yellow => (54.0, 0.95, 1.35),
            TeamColour::Teal => (178.0, 1.0, 1.05),
            TeamColour::Purple => (278.0, 1.0, 1.1),
            TeamColour::Pink => (318.0, 0.8, 1.2),
        }
    }

    /// The colour in menus (the setup screen's side marks, names and arrows); blue and green are the screen's old ones.
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            TeamColour::Blue => (110, 150, 255),
            TeamColour::Green => (90, 230, 110),
            TeamColour::Red => (255, 90, 80),
            TeamColour::Yellow => (240, 220, 60),
            TeamColour::Teal => (60, 220, 210),
            TeamColour::Purple => (180, 110, 255),
            TeamColour::Pink => (255, 120, 200),
        }
    }

    /// The radar's dots; blue and green are the exe's RGB565 `0x64bf` / `0x07e0` (docs/hud.md).
    pub fn radar(self) -> (u8, u8, u8) {
        match self {
            TeamColour::Blue => (99, 150, 255),
            TeamColour::Green => (0, 255, 0),
            other => other.rgb(),
        }
    }

    /// The next (or previous) colour in [`TeamColour::ALL`] that is not `taken`.
    pub fn step(self, forward: bool, taken: TeamColour) -> TeamColour {
        let n = Self::ALL.len();
        let mut i = Self::ALL.iter().position(|&c| c == self).unwrap_or(0);
        loop {
            i = if forward { (i + 1) % n } else { (i + n - 1) % n };
            if Self::ALL[i] != taken {
                return Self::ALL[i];
            }
        }
    }
}

/// The player and the paint of a team sheet by its name (`units\\{name}_blue.ps6` / `_gren`, their `_blueboom` / `_grenboom`
/// pieces, `builds\\blue\\` / `builds\\green\\`); `None` for everything else (shadows and masks carry no colour).
pub fn sheet(name: &str) -> Option<(usize, Paint)> {
    let n = name.to_ascii_lowercase();
    if !n.ends_with(".ps6") {
        return None;
    }
    if n.starts_with("builds\\blue\\") {
        return Some((0, Paint::Blue));
    }
    if n.starts_with("builds\\green\\") {
        return Some((1, Paint::Green));
    }
    let stem = n.strip_prefix("units\\")?.trim_end_matches(".ps6");
    if stem.ends_with("_blue") || stem.ends_with("_blueboom") {
        Some((0, Paint::Blue))
    } else if stem.ends_with("_gren") || stem.ends_with("_grenboom") {
        Some((1, Paint::Green))
    } else {
        None
    }
}

fn hsv(p: u16) -> (f32, f32, f32) {
    let r = ((p >> 11) & 31) as f32 / 31.0;
    let g = ((p >> 5) & 63) as f32 / 63.0;
    let b = (p & 31) as f32 / 31.0;
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d == 0.0 {
        0.0
    } else if mx == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if mx == 0.0 { 0.0 } else { d / mx }, mx)
}

fn rgb565(h: f32, s: f32, v: f32) -> u16 {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let q = |f: f32, max: f32| ((f + m) * max).round().clamp(0.0, max) as u16;
    (q(r, 31.0) << 11) | (q(g, 63.0) << 5) | q(b, 31.0)
}

/// `frame` (painted in `from`) with its team pixels - saturated ones in the paint's hue range - in colour `to`; light, shade and
/// every other pixel stay.
pub fn recolour(frame: &Frame, from: Paint, to: TeamColour) -> Frame {
    let (lo, hi) = from.band();
    let (gs, gv) = from.gains();
    let (hue, ks, kv) = to.paint();
    let pixels = frame
        .pixels
        .iter()
        .zip(&frame.mask)
        .map(|(&p, &m)| {
            let (h, s, v) = hsv(p);
            if m && (lo..=hi).contains(&h) && s > 0.2 && v > 0.08 {
                rgb565(hue, (s * gs * ks).min(1.0), (v * gv * kv).min(1.0))
            } else {
                p
            }
        })
        .collect();
    Frame { pixels, enhanced: Default::default(), ..frame.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pixels: Vec<u16>) -> Frame {
        let n = pixels.len();
        Frame { width: n as u16, height: 1, hot_x: 0, hot_y: 0, extra: [0; 4], mask: vec![true; n], pixels, enhanced: Default::default() }
    }

    #[test]
    fn only_the_team_pixels_change_hue() {
        // a mid blue, a mid green, the gold of the buildings' frames, grey, black
        let (blue, green, gold, grey) = (rgb565(230.0, 0.6, 0.6), rgb565(110.0, 0.8, 0.5), rgb565(38.0, 0.7, 0.8), rgb565(0.0, 0.0, 0.5));
        let f = frame(vec![blue, green, gold, grey, 0]);
        let red = recolour(&f, Paint::Blue, TeamColour::Red);
        assert_eq!(&red.pixels[1..], &f.pixels[1..], "green, gold, grey and black are not blue's team pixels");
        let (h, s, _) = hsv(red.pixels[0]);
        assert!(!(15.0..345.0).contains(&h) && s > 0.5, "blue turned red: hue {h}, saturation {s}");
        let teal = recolour(&f, Paint::Green, TeamColour::Teal);
        assert_eq!([teal.pixels[0], teal.pixels[2], teal.pixels[3]], [blue, gold, grey]);
        assert!((170.0..190.0).contains(&hsv(teal.pixels[1]).0));
    }

    #[test]
    fn uncovered_pixels_are_left_alone() {
        let mut f = frame(vec![rgb565(230.0, 0.6, 0.6)]);
        f.mask[0] = false;
        assert_eq!(recolour(&f, Paint::Blue, TeamColour::Pink).pixels, f.pixels);
    }

    #[test]
    fn team_sheets_are_told_by_name() {
        assert_eq!(sheet("units\\1A_blue.ps6"), Some((0, Paint::Blue)));
        assert_eq!(sheet("units\\2C_gren.ps6"), Some((1, Paint::Green)));
        assert_eq!(sheet("units\\1A_grenboom.ps6"), Some((1, Paint::Green)));
        assert_eq!(sheet("builds\\Green\\_heavy.ps6"), Some((1, Paint::Green)));
        assert_eq!(sheet("builds\\blue\\sentry01.ps6"), Some((0, Paint::Blue)));
        for other in ["units\\1A_blue.sha", "builds\\shadow\\_heavy.sha", "builds\\mask\\_heavy.msk", "units\\1A_fire.ps6", "extras\\ikony.ps6"] {
            assert_eq!(sheet(other), None, "{other}");
        }
    }

    #[test]
    fn stepping_skips_the_other_players_colour() {
        assert_eq!(TeamColour::Blue.step(true, TeamColour::Green), TeamColour::Red);
        assert_eq!(TeamColour::Red.step(false, TeamColour::Green), TeamColour::Blue);
        assert_eq!(TeamColour::Blue.step(false, TeamColour::Pink), TeamColour::Purple, "wraps around past the taken one");
    }
}
