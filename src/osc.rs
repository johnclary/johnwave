//! The morphing oscillator: sine → triangle → saw → square.
//!
//! The waves are phase-aligned, so they all start at 0 and rise together and their
//! fundamentals line up. That way a crossfade between two neighbouring shapes never
//! partially cancels and sounds thin. Saw and square use PolyBLEP to cut aliasing at
//! their jumps. Every shape is scaled to the same RMS level, so turning Morph changes
//! the timbre, not the volume.

use std::f32::consts::TAU;

/// RMS of the triangle and saw (1/√3). Every shape is scaled to this level, which also
/// keeps all peaks within ±1.
const TARGET_RMS: f32 = 0.577_350_3;
const SINE_GAIN: f32 = TARGET_RMS / std::f32::consts::FRAC_1_SQRT_2; // ≈ 0.816
const SQUARE_GAIN: f32 = TARGET_RMS; // a square's RMS is 1

#[derive(Default)]
pub struct MorphOsc {
    /// Position in the current cycle, always in `[0, 1)`.
    phase: f32,
}

impl MorphOsc {
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Produce one sample and advance the phase.
    ///
    /// `morph` runs 0..=1: 0 = sine, ⅓ = triangle, ⅔ = saw, 1 = square, crossfading in between.
    pub fn next(&mut self, freq: f32, sample_rate: f32, morph: f32) -> f32 {
        // Phase advance per sample. Clamped below Nyquist so PolyBLEP stays valid.
        let dt = (freq / sample_rate).clamp(0.0, 0.49);
        let out = shape(self.phase, dt, morph);

        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// The morph blend at phase `p`. A pure function, so it's easy to test, and the plugin
/// window uses it to draw the wave (with `dt = 0`, which skips the PolyBLEP smoothing).
pub fn shape(p: f32, dt: f32, morph: f32) -> f32 {
    // Split 0..=1 into three segments: [sine→tri], [tri→saw], [saw→square].
    let pos = morph.clamp(0.0, 1.0) * 3.0;
    let i = (pos as usize).min(2);
    let t = pos - i as f32;

    // Only calculate the two shapes being blended.
    let (a, b) = match i {
        0 => (sine(p), triangle(p)),
        1 => (triangle(p), saw(p, dt)),
        _ => (saw(p, dt), square(p, dt)),
    };
    a + (b - a) * t
}

fn sine(p: f32) -> f32 {
    (p * TAU).sin() * SINE_GAIN
}

/// Peaks at p = ¼ and troughs at p = ¾, like the sine. Its kinks alias only a little
/// (harmonics fall off as 1/n²), so it's left naive for now.
fn triangle(p: f32) -> f32 {
    let q = wrap(p + 0.25);
    1.0 - 4.0 * (q - 0.5).abs()
}

/// Rising saw, shifted so it crosses 0 at p = 0 and drops at p = ½.
fn saw(p: f32, dt: f32) -> f32 {
    let t = wrap(p + 0.5);
    let naive = 2.0 * t - 1.0;
    naive - poly_blep(t, dt)
}

/// +1 for the first half of the cycle, −1 for the second half.
fn square(p: f32, dt: f32) -> f32 {
    let naive = if p < 0.5 { 1.0 } else { -1.0 };
    (naive + poly_blep(p, dt) - poly_blep(wrap(p + 0.5), dt)) * SQUARE_GAIN
}

/// PolyBLEP correction for a jump at t = 0 (t is phase in 0..1, dt is the phase step).
/// Smooths the jump over one sample on each side, which removes most aliasing cheaply.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        0.0
    } else if t < dt {
        let x = t / dt;
        2.0 * x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + 2.0 * x + 1.0
    } else {
        0.0
    }
}

fn wrap(x: f32) -> f32 {
    x - x.floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn render(freq: f32, morph: f32, n: usize) -> Vec<f32> {
        let mut osc = MorphOsc::default();
        (0..n).map(|_| osc.next(freq, SR, morph)).collect()
    }

    fn rms(xs: &[f32]) -> f32 {
        (xs.iter().map(|x| x * x).sum::<f32>() / xs.len() as f32).sqrt()
    }

    #[test]
    fn stays_in_range_for_all_morphs_and_pitches() {
        for &freq in &[20.0, 110.0, 440.0, 2_000.0, 8_000.0, 20_000.0] {
            for step in 0..=20 {
                let morph = step as f32 / 20.0;
                for x in render(freq, morph, 4_800) {
                    assert!(x.abs() <= 1.0 + 1e-4, "freq {freq} morph {morph}: {x}");
                }
            }
        }
    }

    #[test]
    fn endpoints_are_sine_and_square() {
        for (i, &x) in render(100.0, 0.0, 480).iter().enumerate() {
            let expected = (i as f32 * 100.0 / SR * TAU).sin() * SINE_GAIN;
            assert!((x - expected).abs() < 1e-3);
        }
        // At a low pitch, the square sits at ±level except right at the edges.
        let sq = render(100.0, 1.0, 480);
        assert!((sq[120] - SQUARE_GAIN).abs() < 1e-4);
        assert!((sq[360] + SQUARE_GAIN).abs() < 1e-4);
    }

    #[test]
    fn loudness_is_even_across_the_morph_range() {
        // One full second at 100 Hz = exactly 100 cycles.
        let n = SR as usize;
        for step in 0..=12 {
            let morph = step as f32 / 12.0;
            let level = rms(&render(100.0, morph, n));
            assert!(
                (level - TARGET_RMS).abs() / TARGET_RMS < 0.12,
                "morph {morph}: rms {level}"
            );
        }
    }

    #[test]
    fn phase_wraps_and_stays_in_range() {
        let mut osc = MorphOsc::default();
        for _ in 0..100_000 {
            osc.next(13_000.0, SR, 0.5);
            assert!((0.0..1.0).contains(&osc.phase));
        }
    }

    #[test]
    fn poly_blep_reduces_the_jump() {
        // Naive saw jumps by 2.0 between two samples. The BLEP version should step less.
        let dt = 1_000.0 / SR;
        let before = saw(0.5 - dt * 0.5, dt);
        let after = saw(0.5 + dt * 0.5, dt);
        assert!((before - after).abs() < 1.5);
    }
}
