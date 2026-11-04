//! The Space pause and the screens after a mission (docs/pause.md, docs/results.md).
//!
//! Pure functions of the exe: the frame effects of the pause (`0x45d3a0`) and of the "mission completed / failed" banner
//! (`0x45d400`), and the arithmetic of the statistics screen (`0x44cbd0` draw, `0x44d370` one bar, `0x44d770` update).

/// Banner state (states 9 / 0xa, `0x4357a0`): a click or key leaves only after this many ticks (`cmp [state+0xa8], 0x78`, 2 s at 60 Hz).
pub const BANNER_TICKS: i32 = 0x78;

/// Tallest bar in pixels (`imul eax, eax, 0x8c` in `0x44d370`).
pub const BAR_MAX: i32 = 0x8c;

/// Statistics screen: the counter advances every tick; a tick sound (48) every 4 ticks while a bar grows (`0x44d770`).
pub const SOUND_PERIOD: i32 = 4;

/// Pixel transform of `0x45d400` for RGB565 (`[0x517aec] == 2`): red kept, green and blue reduced to their top three bits placed
/// in the low bits of their fields (green / 8, blue / 4). A dark red tint.
pub fn tint565(p: u16) -> u16 {
    (p & 0xf800) | ((((p >> 1) & 0x380) | (p & 0x1c)) >> 2)
}

/// The RGB555 branch of `0x45d400` (`0x45d4ad`): red kept, green and blue / 4 (`(p >> 2) & 0xe7`).
pub fn tint555(p: u16) -> u16 {
    ((p >> 2) & 0xe7) | (p & 0x7c00)
}

/// `0x45d400` on a `w` x `h` RGB565 frame: every even row is tinted, every odd row is black (it works on `h / 2` row pairs, so a
/// last unpaired row of an odd height is left alone).
pub fn banner_rows(px: &mut [u16], w: usize, h: usize, is565: bool) {
    for y in 0..h / 2 * 2 {
        let row = &mut px[y * w..(y + 1) * w];
        if y % 2 == 0 {
            row.iter_mut().for_each(|p| *p = if is565 { tint565(*p) } else { tint555(*p) });
        } else {
            row.fill(0);
        }
    }
}

/// `0x45d3a0` (the pause): every even row is black, odd rows stay as they are.
pub fn pause_rows(px: &mut [u16], w: usize, h: usize) {
    for y in (0..h).step_by(2) {
        px[y * w..(y + 1) * w].fill(0);
    }
}

/// The 8 counted values of one side in the order of `[0x6bf78c + side * 0x80 + 4 * k]`: buildings captured, buildings lost,
/// units produced, units lost, turrets built, turrets lost, air forces sent, air forces lost. Four pictures, two pairs each.
pub type SideStats = [u32; 8];

/// The strings (1-based lang ids passed to `0x466ae0`) under the eight pairs: CAPTURED, LOST, PRODUCED, LOST, BUILT, LOST, SENT, LOST.
pub const PAIR_LABELS: [u32; 8] = [0x163, 0x164, 0x165, 0x164, 0x166, 0x164, 0x167, 0x164];

/// Bar centres of the two sides of each pair, relative to the screen centre x (four slots per row; pairs 0..4 top, 4..8 bottom).
pub const PAIR_X: [(i32, i32); 4] = [(-0xfa, -0xe1), (-0x5f, -0x46), (0x4c, 0x65), (0xdd, 0xf6)];

/// Bar base y relative to the screen centre: top row `cy - 0x28`, bottom row `cy + 0xc7`.
pub const ROW_BASE: [i32; 2] = [-0x28, 0xc7];

/// Label top-left (x from the centre, y from the centre) for each pair (`0x44d500`, `0x44d6c3`).
pub const LABEL_POS: [(i32, i32); 8] = [(-0x11b, -0x1b), (-0x68, -0x1b), (0x2b, -0x1b), (0xd8, -0x1b), (-0x102, 0xd4), (-0x68, 0xd4), (0x3f, 0xd4), (0xd8, 0xd4)];

/// The largest value of pair `k` (both sides), `M_k`.
pub fn pair_max(stats: &[SideStats; 2], k: usize) -> u32 {
    stats[0][k].max(stats[1][k])
}

/// The scale of the bars of pair `k`: the larger maximum of its picture's two pairs.
pub fn pane_max(stats: &[SideStats; 2], k: usize) -> u32 {
    let p = k / 2 * 2;
    pair_max(stats, p).max(pair_max(stats, p + 1))
}

/// The sum of the four picture maxima `S` that sets the speed of the count.
pub fn speed_sum(stats: &[SideStats; 2]) -> u32 {
    (0..4).map(|i| pane_max(stats, i * 2)).sum()
}

/// The count shown after `t` ticks (`[state+8]` in `0x44cbd0`): `2t` for S >= 600, `t` for S >= 400, `t / 2` for S >= 200, else `t / 3`.
pub fn counter(t: i32, sum: u32) -> i32 {
    if sum >= 600 {
        t * 2
    } else if sum >= 400 {
        t
    } else if sum >= 200 {
        t / 2
    } else {
        t / 3
    }
}

/// One bar against its pair's counter `c` (`0x44d370`): the value shown (`min(c, value)`, 0 while `c <= 0`) and whether the bar
/// has reached its value. The "top" flag (`pair_top`) is separate.
pub fn pair_state(c: i32, value: u32) -> (i32, bool) {
    if c <= 0 {
        return (0, false);
    }
    let v = value as i32;
    if c - v < 0 {
        (c, false)
    } else {
        (v, true)
    }
}

/// Height in pixels of a bar showing `shown` against the picture maximum (`shown * 0x8c / max`, 0 when `max` is 0).
pub fn bar_height(shown: i32, max: u32) -> i32 {
    if max == 0 {
        0
    } else {
        shown.min(max as i32) * BAR_MAX / max as i32
    }
}

/// Where the counter is for each pair: the first pair uses `c`, the next `c - M_0 - 2`, and so on (`sub M; add -2`).
/// Returns the 8 pair counters and whether the whole animation has finished (the last pair reached its maximum).
pub fn pair_counters(c: i32, stats: &[SideStats; 2]) -> ([i32; 8], bool) {
    let mut out = [0; 8];
    let mut rest = c;
    for k in 0..8 {
        out[k] = rest;
        rest -= pair_max(stats, k) as i32 + 2;
    }
    (out, out[7] >= pair_max(stats, 7) as i32)
}

/// `top` flags of `0x44cbd0`: `0 <= c_k - M_k < 2` (the pair has just reached its maximum).
pub fn pair_top(ck: i32, max: u32) -> bool {
    let d = ck - max as i32;
    (0..2).contains(&d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_keeps_red_and_quarters_the_rest() {
        assert_eq!(tint565(0xffff), 0xf8e7);
        assert_eq!(tint565(0xf800), 0xf800);
        assert_eq!(tint565(0x07e0), 0xe0);
        assert_eq!(tint565(0x001f), 0x07);
        assert_eq!(tint555(0x7fff), 0x7ce7);
    }

    #[test]
    fn rows_alternate() {
        let (w, h) = (4, 4);
        let mut a = vec![0xffffu16; w * h];
        banner_rows(&mut a, w, h, true);
        assert!(a[..w].iter().all(|&p| p == tint565(0xffff)));
        assert!(a[w..2 * w].iter().all(|&p| p == 0));
        let mut b = vec![0x1234u16; w * h];
        pause_rows(&mut b, w, h);
        assert!(b[..w].iter().all(|&p| p == 0));
        assert!(b[w..2 * w].iter().all(|&p| p == 0x1234));
        assert!(b[2 * w..3 * w].iter().all(|&p| p == 0));
    }

    #[test]
    fn counter_speeds_follow_the_sum() {
        assert_eq!(counter(30, 600), 60);
        assert_eq!(counter(30, 599), 30);
        assert_eq!(counter(30, 399), 15);
        assert_eq!(counter(31, 199), 10);
    }

    #[test]
    fn bars_scale_to_the_picture_maximum() {
        assert_eq!(bar_height(10, 10), BAR_MAX);
        assert_eq!(bar_height(5, 10), 70);
        assert_eq!(bar_height(0, 0), 0);
    }

    #[test]
    fn pairs_fill_one_after_another() {
        let mut s = [[0u32; 8]; 2];
        s[0][0] = 3;
        s[1][0] = 5;
        s[0][1] = 2;
        let (c, done) = pair_counters(4, &s);
        assert_eq!(c[0], 4);
        assert_eq!(c[1], 4 - 5 - 2);
        assert!(!done);
        let (_, done) = pair_counters(1000, &s);
        assert!(done);
        assert_eq!(pair_state(4, 5), (4, false));
        assert_eq!(pair_state(7, 5), (5, true));
        assert!(pair_top(5, 5) && pair_top(6, 5) && !pair_top(7, 5) && !pair_top(4, 5));
        assert_eq!(pane_max(&s, 1), 5);
        assert_eq!(speed_sum(&s), 5);
    }
}
