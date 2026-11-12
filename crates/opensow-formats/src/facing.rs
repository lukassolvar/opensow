//! Unit facings, as computed by `State of War.exe`.
//!
//! Sprites come in `steps` facings numbered **clockwise from north**: with 32 steps, 0 = north, 8 = east, 16 = south,
//! 24 = west (chassis, weapon tops and fire points); some tops and walker bodies have 16.
//!
//! - Heading to facing: `0x46b180(ax, ay, bx, by, steps)`. `dx = bx - ax`, `dy = ay - by` (y flipped to point up),
//!   `theta = atan(|dy| / |dx|)` in degrees, fixed to a counter-clockwise angle from east in 0..360
//!   (`dx >= 0, dy >= 0`: theta; `dx >= 0, dy < 0`: 360 - theta; `dx < 0, dy >= 0`: 180 - theta; else 180 + theta),
//!   then `sector = trunc((theta + 5.625) / 11.25)` for 32 steps (`+ 11.25`, `/ 22.5` for 16) mapped by a jump table.
//!   Faithful quirks are kept: a 16-step heading within 11.25 degrees below east returns 5 instead of 4; sectors outside
//!   the table (including the NaN from `a == b`) return 8 (32 steps), 0 (16 steps) or 2 (8 steps); other step counts return 0.
//!   Eight facings use the same convention (`(2 - round(theta/45)) mod 8`: 0 north, 2 east, 4 south, 6 west).
//!   Every branch was checked against the exe's own code in an emulator (see `docs/projectiles.md`, Verification).
//! - One turn step toward a target: `0x46a190(&current, target, steps)` takes the shorter way round.
//! - Chassis frame (`0x46fe80`): `frame = phase * steps + facing`; phases are the tread/leg animation (3 for
//!   `chA/chB/chC`, 5 for `chW`, 1 for `chI1`).
//! - A new unit's facing is not set by its constructors, so it is probably 0 (north); this is not verified.

/// Faithful port of `0x46b180`. Points are in pixels (y down), `steps` is the number of facings.
pub fn facing(ax: i32, ay: i32, bx: i32, by: i32, steps: u32) -> u32 {
    let dx = bx.wrapping_sub(ax);
    let dy = ay.wrapping_sub(by);
    // The pi literal is kept exactly as ported: the emulator checks of this function were made with this value, and
    // `std::f64::consts::PI` differs from it in the last digits.
    #[allow(clippy::approx_constant)]
    let t = ((dy.unsigned_abs() as f64) / (dx.unsigned_abs() as f64)).atan() * 180.0 / 3.1415926535;
    let theta = match (dx >= 0, dy >= 0) {
        (true, true) => t,
        (true, false) => 360.0 - t,
        (false, true) => 180.0 - t,
        (false, false) => t + 180.0,
    };
    // cvttsd2si: NaN and out-of-range values become i32::MIN, which the table bounds checks treat as out of range.
    let trunc = |v: f64| if v.is_nan() || v.abs() >= 2_147_483_648.0 { i32::MIN } else { v as i32 };
    match steps {
        32 => {
            let s = trunc((theta + 5.625) / 11.25);
            if (0..32).contains(&s) {
                (8 - s).rem_euclid(32) as u32
            } else {
                8
            }
        }
        16 => {
            let s = trunc((theta + 11.25) / 22.5);
            match s {
                0..=15 => (4 - s).rem_euclid(16) as u32,
                16 => 5, // original table quirk: the wrap-around sector repeats sector 15's value
                _ => 0,
            }
        }
        8 => {
            let s = trunc((theta + 22.5) / 45.0);
            if (0..=8).contains(&s) {
                (2 - s).rem_euclid(8) as u32
            } else {
                2
            }
        }
        _ => 0,
    }
}

/// Port of `0x46a190`: moves `current` one step toward `target` the short way round (wrapping in `0..steps`).
pub fn turn_step(current: u32, target: u32, steps: u32) -> u32 {
    if current == target {
        return current;
    }
    let (c, t, n) = (current as i32, target as i32, steps as i32);
    let d = (t - c).abs();
    let step = if d < n - d {
        if t > c {
            1
        } else {
            -1
        }
    } else if t <= c {
        1
    } else {
        -1
    };
    let mut next = c + step;
    if next < 0 {
        next = n - 1;
    }
    if next > n - 1 {
        next = 0;
    }
    next as u32
}

/// Chassis sprite frame for an animation `phase` and `facing` (`0x46fe80`).
pub fn frame(phase: u32, facing: u32, steps: u32) -> u32 {
    phase * steps + facing
}

/// Compass heading of a facing in degrees clockwise from north.
pub fn heading_degrees(facing: u32, steps: u32) -> f64 {
    facing as f64 * 360.0 / steps as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cardinal_directions_with_32_steps() {
        let o = (100, 100);
        assert_eq!(facing(o.0, o.1, 200, 100, 32), 8); // east
        assert_eq!(facing(o.0, o.1, 100, 0, 32), 0); // north (y decreases upward on screen)
        assert_eq!(facing(o.0, o.1, 0, 100, 32), 24); // west
        assert_eq!(facing(o.0, o.1, 100, 200, 32), 16); // south
        assert_eq!(facing(o.0, o.1, 200, 0, 32), 4); // north-east
        assert_eq!(facing(o.0, o.1, 0, 200, 32), 20); // south-west
        assert_eq!(heading_degrees(8, 32), 90.0);
    }

    #[test]
    fn sixteen_steps_and_the_wraparound_quirk() {
        let o = (0, 0);
        assert_eq!(facing(o.0, o.1, 10, 0, 16), 4); // east
        assert_eq!(facing(o.0, o.1, 0, -10, 16), 0); // north
        assert_eq!(facing(o.0, o.1, 0, 10, 16), 8); // south
        assert_eq!(facing(o.0, o.1, -10, 0, 16), 12); // west
                                                      // a hair south of east lands in the sector that the original maps to 5 instead of 4
        assert_eq!(facing(o.0, o.1, 100, 1, 16), 5);
        assert_eq!(facing(o.0, o.1, 100, 1, 32), 8); // the 32-step path handles the same case correctly
        assert_eq!(facing(o.0, o.1, 100, -1, 16), 4);
    }

    #[test]
    fn degenerate_and_odd_inputs() {
        assert_eq!(facing(5, 5, 5, 5, 32), 8); // 0/0 = NaN -> default
        assert_eq!(facing(5, 5, 5, 5, 16), 0);
        assert_eq!(facing(0, 0, 10, 0, 7), 0);
        assert_eq!(facing(0, 0, 10, 0, 8), 2); // east
        assert_eq!(facing(0, 0, 0, -10, 8), 0); // north
        assert_eq!(facing(0, 0, 0, 10, 8), 4); // south
        assert_eq!(facing(0, 0, -10, 0, 8), 6); // west
        assert_eq!(facing(5, 5, 5, 5, 8), 2); // NaN -> default
    }

    #[test]
    fn turning_takes_the_short_way() {
        assert_eq!(turn_step(5, 5, 32), 5);
        assert_eq!(turn_step(5, 9, 32), 6);
        assert_eq!(turn_step(9, 5, 32), 8);
        assert_eq!(turn_step(30, 2, 32), 31); // through 31, 0, 1
        assert_eq!(turn_step(31, 2, 32), 0);
        assert_eq!(turn_step(2, 30, 32), 1);
        assert_eq!(turn_step(0, 31, 32), 31);
        // from 0 to 16 the two ways are equally long; the original goes backwards
        assert_eq!(turn_step(0, 16, 32), 31);
        let mut f = 3;
        for _ in 0..13 {
            f = turn_step(f, 16, 32);
        }
        assert_eq!(f, 16);
    }

    #[test]
    fn frame_layout() {
        assert_eq!(frame(0, 8, 32), 8);
        assert_eq!(frame(2, 8, 32), 72);
        assert_eq!(frame(4, 31, 32), 159);
    }
}
