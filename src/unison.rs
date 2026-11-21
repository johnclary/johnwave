//! Unison: several copies of the oscillators per voice, spread in pitch and across the
//! speakers.
//!
//! Each copy sits at a *position* from −1 (furthest left, lowest) to +1 (furthest right,
//! highest), evenly spaced. Detune moves a copy by `position × Detune` cents, and Width
//! pans it to `position × Width`. With an odd number of copies, the middle one sits at
//! 0: on the note and in the centre, which keeps the pitch solid.
//!
//! The copies are added up and scaled by 1/√N. They drift in and out of phase, so they
//! add up by power (like the equal-power Osc Mix), and this keeps the level even
//! whatever the count: more copies sound wider, not louder.

/// The most copies a voice can play.
pub const MAX_VOICES: usize = 7;
/// The Detune knob's top, in cents.
pub const MAX_DETUNE_CT: f32 = 100.0;

/// Where copy `i` of `count` sits, from −1 to +1. A single copy sits at 0.
pub fn position(i: usize, count: usize) -> f32 {
    if count <= 1 {
        0.0
    } else {
        2.0 * i as f32 / (count - 1) as f32 - 1.0
    }
}

/// Left and right gains for a copy panned to `pan` (−1..=1). Equal-power, like the
/// main Pan, but scaled so the centre is 1 on both sides: one copy in the centre comes
/// out exactly as it would without unison. (The main Pan then does the −3 dB.)
pub fn copy_gains(pan: f32) -> (f32, f32) {
    // Exactly 1 in the centre. The formula gives 1 there too, but rounding can make it
    // 0.99999994, and the centre (no unison, or Width 0) should be bit-for-bit untouched.
    if pan == 0.0 {
        return (1.0, 1.0);
    }
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    let s = std::f32::consts::SQRT_2;
    (angle.cos() * s, angle.sin() * s)
}

/// The gains of every copy for `count` copies and a Width of `width` (0..=1). Copies
/// past `count` are left at (0, 0). Worked out once per sample and shared by all voices,
/// since Width can't be modulated.
pub fn pans(count: usize, width: f32) -> [(f32, f32); MAX_VOICES] {
    let count = count.clamp(1, MAX_VOICES);
    let mut gains = [(0.0, 0.0); MAX_VOICES];
    for (i, g) in gains.iter_mut().enumerate().take(count) {
        *g = copy_gains(position(i, count) * width);
    }
    gains
}

/// How much to scale the sum of `count` copies, so the level stays the same.
pub fn level(count: usize) -> f32 {
    1.0 / (count.max(1) as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_are_even_and_symmetric() {
        assert_eq!(position(0, 1), 0.0);
        assert_eq!((position(0, 2), position(1, 2)), (-1.0, 1.0));
        let five: Vec<f32> = (0..5).map(|i| position(i, 5)).collect();
        assert_eq!(five, vec![-1.0, -0.5, 0.0, 0.5, 1.0]);
        for n in 2..=MAX_VOICES {
            for i in 0..n {
                assert!((position(i, n) + position(n - 1 - i, n)).abs() < 1e-6);
            }
        }
        // Odd counts have a copy right on the note.
        assert_eq!(position(3, 7), 0.0);
    }

    #[test]
    fn centre_copy_is_untouched_and_power_stays_constant() {
        assert_eq!(copy_gains(0.0).0, copy_gains(0.0).1);
        assert_eq!(copy_gains(0.0), (1.0, 1.0));
        let (l, r) = copy_gains(-1.0);
        assert!((l - std::f32::consts::SQRT_2).abs() < 1e-6 && r.abs() < 1e-6);
        for step in 0..=10 {
            let (l, r) = copy_gains(step as f32 / 5.0 - 1.0);
            assert!((l * l + r * r - 2.0).abs() < 1e-5);
        }
    }

    #[test]
    fn width_zero_puts_every_copy_in_the_centre() {
        for (i, &(l, r)) in pans(5, 0.0).iter().enumerate() {
            if i < 5 {
                assert!((l - 1.0).abs() < 1e-6 && (r - 1.0).abs() < 1e-6);
            } else {
                assert_eq!((l, r), (0.0, 0.0));
            }
        }
        // A single copy stays in the centre at any width.
        assert_eq!(pans(1, 1.0)[0], copy_gains(0.0));
    }

    #[test]
    fn one_copy_is_at_full_level() {
        assert_eq!(level(1), 1.0);
        assert!((level(4) - 0.5).abs() < 1e-6);
    }
}
