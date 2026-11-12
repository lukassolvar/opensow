//! The ambient clouds (class `CCloud`, vtable `0x4878e4`, 20-byte objects in the list `[0x49a3fc]`; docs/clouds.md).
//!
//! A cloud is pure decoration: `x`, `y` (map pixels), a speed class `10..=39` and a sprite `0..=4` of `extras\bigexplo\clouds`
//! (array `0x5e95b0`). Each frame it moves right by the speed-table steps of the frame's ticks (the same 60-entry rows as the
//! bullets, [`steps_at`]); once `x` passes `(map width + 25) * 32` it is re-rolled at the left edge. Every random number comes from
//! the visual generator `0x437cd0` (not the lockstep one), so the functions here take it as a closure `rand(n) -> 0..n`.
//!
//! Drawing (`0x420082` / `0x420422`): the shadow with the masked darkening blit `0x45f4d0` at `(x + 100, y)` before the shots, the
//! cloud itself with the additive blit `0x45cde0` at `(x, y - 200)` after the aircraft and bullets.

use crate::projectile::steps_at;

/// Sprite array of the cloud pictures (`extras\bigexplo\clouds`, five used frames).
pub const ARRAY: u32 = 0x5e95b0;
/// Number of different cloud pictures: `rand(5)`.
pub const PICTURES: u32 = 5;
/// The shadow is drawn this far right of `(x, y)`.
pub const SHADOW_DX: i32 = 100;
/// The cloud is drawn this far above its ground position `y`.
pub const BODY_DY: i32 = -200;

/// Cloud count of an Options CLOUDS detail `0..=4` (`0x453060`: the jump table `0x453298`; anything else 0).
pub fn count_for_detail(detail: u32) -> usize {
    [0, 13, 32, 45, 70].get(detail as usize).copied().unwrap_or(0)
}

/// One cloud (`+4` x, `+8` y, `+0xc` speed, `+0x10` picture).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cloud {
    /// `+4`: x position.
    pub x: i32,
    /// `+8`: y position.
    pub y: i32,
    /// `+0xc`: speed.
    pub speed: u32,
    /// `+0x10`: which cloud picture is drawn.
    pub picture: u32,
}

impl Cloud {
    /// Constructor `0x4145a0` / re-roll `0x414770` (same four draws in this order): `x = rand(400) - 800`, `y =
    /// rand(height * 32 - 200) + 100`, `speed = rand(30) + 10`, `picture = rand(5)`. `height` is the map height in cells (`[0x71cec4]`).
    pub fn roll(height: i32, rand: &mut impl FnMut(u32) -> u32) -> Cloud {
        let x = rand(400) as i32 - 800;
        let y = rand((height * 32 - 200).max(0) as u32) as i32 + 100;
        let speed = rand(30) + 10;
        let picture = rand(PICTURES);
        Cloud { x, y, speed, picture }
    }

    /// A cloud created when the list grows (`0x453060`): the constructor, then `0x4149d0` replaces the position so the sky is full
    /// from the start: `x = rand((width + 25) * 32) - 400`, `y = rand(height * 32)` (`width`, `height` in cells).
    pub fn scatter(width: i32, height: i32, rand: &mut impl FnMut(u32) -> u32) -> Cloud {
        let mut c = Cloud::roll(height, rand);
        c.x = rand(((width + 25) * 32) as u32) as i32 - 400;
        c.y = rand((height * 32) as u32) as i32;
        c
    }

    /// `0x4147d0` for one rendered frame that covers `ticks` (the in-second tick indices, `[0x517b10]`): `x` grows by the row of
    /// `speed` for each tick. Returns `false` when `x > (width + 25) * 32` (the caller then re-rolls it with [`Cloud::roll`]).
    pub fn advance(&mut self, ticks: impl IntoIterator<Item = u32>, width: i32) -> bool {
        for t in ticks {
            self.x += steps_at(self.speed, t % 60) as i32;
        }
        self.x <= (width + 25) * 32
    }

    /// Where the shadow sprite's hot spot goes (map pixels).
    pub fn shadow_pos(&self) -> (i32, i32) {
        (self.x + SHADOW_DX, self.y)
    }

    /// Where the cloud sprite's hot spot goes (map pixels).
    pub fn body_pos(&self) -> (i32, i32) {
        (self.x, self.y + BODY_DY)
    }
}

/// The cloud list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sky {
    /// The clouds, in list order.
    pub clouds: Vec<Cloud>,
}

impl Sky {
    /// `0x453060` with the options' detail: detail 0 destroys every cloud; otherwise it adds [`Cloud::scatter`]ed ones up to the
    /// target or removes random ones (`rand(count)`, one at a time) down to it.
    pub fn resize(&mut self, detail: u32, width: i32, height: i32, rand: &mut impl FnMut(u32) -> u32) {
        if detail == 0 {
            self.clouds.clear();
            return;
        }
        let target = count_for_detail(detail);
        while self.clouds.len() < target {
            self.clouds.push(Cloud::scatter(width, height, rand));
        }
        while self.clouds.len() > target {
            let i = rand(self.clouds.len() as u32) as usize;
            self.clouds.remove(i);
        }
    }

    /// The per-frame update `0x42e290`: advance each cloud, re-roll the ones that left (`0x414770`).
    pub fn step(&mut self, ticks: &[u32], width: i32, height: i32, rand: &mut impl FnMut(u32) -> u32) {
        for c in &mut self.clouds {
            if !c.advance(ticks.iter().copied(), width) {
                *c = Cloud::roll(height, rand);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq(v: Vec<u32>) -> impl FnMut(u32) -> u32 {
        let mut i = 0;
        move |n| {
            let r = v[i % v.len()] % n.max(1);
            i += 1;
            r
        }
    }

    #[test]
    fn counts_follow_the_detail_table() {
        let c: Vec<_> = (0..6).map(count_for_detail).collect();
        assert_eq!(c, [0, 13, 32, 45, 70, 0]);
    }

    #[test]
    fn roll_draws_x_y_speed_picture_in_order() {
        // 64-cell map: y range 2048 - 200 = 1848
        let c = Cloud::roll(64, &mut seq(vec![399, 1847, 29, 4]));
        assert_eq!(c, Cloud { x: -401, y: 1947, speed: 39, picture: 4 });
        let c = Cloud::roll(64, &mut seq(vec![0, 0, 0, 0]));
        assert_eq!(c, Cloud { x: -800, y: 100, speed: 10, picture: 0 });
    }

    #[test]
    fn scatter_overrides_the_position() {
        let c = Cloud::scatter(64, 64, &mut seq(vec![1, 2, 3, 4, 2847, 2047]));
        assert_eq!((c.x, c.y, c.speed, c.picture), (2847 - 400, 2047, 13, 4));
    }

    #[test]
    fn advance_adds_the_speed_row_and_reports_leaving() {
        let mut c = Cloud { x: 0, y: 0, speed: 30, picture: 0 };
        assert!(c.advance(0..60, 64));
        assert_eq!(c.x, 30, "speed 30 = 30 px per second");
        let mut c = Cloud { x: (64 + 25) * 32, y: 0, speed: 39, picture: 0 };
        assert!(c.advance(std::iter::empty(), 64), "exactly at the limit is still inside");
        assert!(!c.advance(0..60, 64));
    }

    #[test]
    fn resize_grows_shrinks_and_clears() {
        let mut sky = Sky::default();
        let mut r = seq((1..400).collect());
        sky.resize(2, 64, 64, &mut r);
        assert_eq!(sky.clouds.len(), 32);
        sky.resize(1, 64, 64, &mut r);
        assert_eq!(sky.clouds.len(), 13);
        sky.resize(4, 64, 64, &mut r);
        assert_eq!(sky.clouds.len(), 70);
        sky.resize(0, 64, 64, &mut r);
        assert!(sky.clouds.is_empty());
    }

    #[test]
    fn step_rerolls_cloud_that_left() {
        let mut sky = Sky { clouds: vec![Cloud { x: (64 + 25) * 32, y: 5, speed: 39, picture: 1 }] };
        sky.step(&[0, 1, 2], 64, 64, &mut seq(vec![0, 0, 0, 3]));
        assert_eq!(sky.clouds[0], Cloud { x: -800, y: 100, speed: 10, picture: 3 });
    }
}
