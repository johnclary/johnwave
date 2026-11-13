//! A resonant 12 dB/octave low-pass filter.
//!
//! This is a state-variable filter (SVF) in the "topology-preserving transform" form
//! described by Andrew Simper (Cytomic). It stays stable and smooth even when the cutoff
//! moves every sample, which matters because envelopes sweep it.

use std::f32::consts::PI;

/// Damping at 0% resonance: the flattest response with no peak (Q ≈ 0.707).
const K_MAX: f32 = std::f32::consts::SQRT_2;
/// Damping at 100% resonance (Q = 10): a strong, ringing peak. It stays just short of
/// self-oscillation, so the filter can't whistle on its own.
const K_MIN: f32 = 0.1;

#[derive(Default)]
pub struct LowPass {
    // The filter's memory: two integrator states.
    ic1eq: f32,
    ic2eq: f32,
}

impl LowPass {
    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    /// Filter one sample. `resonance` runs 0..=1.
    pub fn process(&mut self, input: f32, cutoff_hz: f32, resonance: f32, sample_rate: f32) -> f32 {
        // Keep the cutoff in a safe range. tan() blows up approaching Nyquist.
        let cutoff = cutoff_hz.clamp(10.0, 0.45 * sample_rate);
        let g = (PI * cutoff / sample_rate).tan();
        let k = K_MAX + (K_MIN - K_MAX) * resonance.clamp(0.0, 1.0);

        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = input - self.ic2eq;
        let v1 = a1 * self.ic1eq + a2 * v3;
        let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        v2 // the low-pass output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const SR: f32 = 48_000.0;

    /// Gain of the filter for a sine at `freq`, measured after it has settled.
    fn gain_at(freq: f32, cutoff: f32, resonance: f32) -> f32 {
        let mut f = LowPass::default();
        let n = SR as usize;
        let mut peak: f32 = 0.0;
        for i in 0..n {
            let x = (i as f32 * freq / SR * TAU).sin();
            let y = f.process(x, cutoff, resonance, SR);
            if i > n / 2 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn passes_low_frequencies_unchanged() {
        let g = gain_at(50.0, 5_000.0, 0.0);
        assert!((g - 1.0).abs() < 0.01, "gain {g}");
    }

    #[test]
    fn cuts_high_frequencies() {
        // 10 kHz through a 500 Hz cutoff is over 4 octaves above, so about −50 dB.
        let g = gain_at(10_000.0, 500.0, 0.0);
        assert!(g < 0.01, "gain {g}");
    }

    #[test]
    fn resonance_boosts_the_cutoff_frequency() {
        let flat = gain_at(1_000.0, 1_000.0, 0.0);
        let peaky = gain_at(1_000.0, 1_000.0, 1.0);
        assert!((flat - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.02, "flat {flat}");
        assert!(peaky > 8.0, "peaky {peaky}");
    }

    #[test]
    fn stays_stable_under_wild_cutoff_sweeps() {
        // A full-scale square, with the cutoff sweeping 20 Hz ↔ 20 kHz 50 times a second
        // at full resonance. The output must stay finite and bounded.
        let mut f = LowPass::default();
        for i in 0..(2.0 * SR) as usize {
            let t = i as f32 / SR;
            let x = if (t * 110.0).fract() < 0.5 { 1.0 } else { -1.0 };
            let sweep = 0.5 + 0.5 * (t * 50.0 * TAU).sin();
            let cutoff = 20.0 * 1000f32.powf(sweep);
            let y = f.process(x, cutoff, 1.0, SR);
            assert!(y.is_finite() && y.abs() < 30.0, "sample {i}: {y}");
        }
    }
}
