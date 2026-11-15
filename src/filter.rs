//! A resonant 12 dB/octave filter: low-pass, high-pass, band-pass or notch.
//!
//! This is a state-variable filter (SVF) in the "topology-preserving transform" form
//! described by Andrew Simper (Cytomic). It stays stable and smooth even when the cutoff
//! moves every sample, which matters because envelopes sweep it.
//!
//! An SVF works out its low-pass and band-pass outputs together each sample, and the
//! other types are just those mixed with the input. So all four cost the same, and you
//! can switch type while a note plays without the filter losing its memory.

use nice_plug::prelude::Enum;
use std::f32::consts::PI;

/// Which part of the sound the filter keeps.
#[derive(Enum, PartialEq, Clone, Copy, Debug)]
pub enum FilterType {
    /// Keeps the lows, cuts above the cutoff.
    #[id = "lp"]
    #[name = "Low-pass"]
    LowPass,
    /// Keeps the highs, cuts below the cutoff.
    #[id = "hp"]
    #[name = "High-pass"]
    HighPass,
    /// Keeps a band around the cutoff. More resonance makes the band narrower.
    #[id = "bp"]
    #[name = "Band-pass"]
    BandPass,
    /// Cuts a band around the cutoff, keeps the rest. More resonance makes the cut narrower.
    #[id = "notch"]
    #[name = "Notch"]
    Notch,
}

/// Damping at 0% resonance: the flattest response with no peak (Q ≈ 0.707).
const K_MAX: f32 = std::f32::consts::SQRT_2;
/// Damping at 100% resonance (Q = 10): a strong, ringing peak. It stays just short of
/// self-oscillation, so the filter can't whistle on its own.
const K_MIN: f32 = 0.1;

#[derive(Default)]
pub struct Filter {
    // The filter's memory: two integrator states.
    ic1eq: f32,
    ic2eq: f32,
}

impl Filter {
    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    /// Filter one sample. `resonance` runs 0..=1.
    pub fn process(
        &mut self,
        input: f32,
        kind: FilterType,
        cutoff_hz: f32,
        resonance: f32,
        sample_rate: f32,
    ) -> f32 {
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

        // v1 is the band-pass output and v2 the low-pass. Band-pass is scaled by k so
        // it peaks at full level whatever the resonance (unscaled it would jump to 10× at
        // full resonance). Notch is the input minus that band, and high-pass is the input
        // minus both band and lows.
        let band = k * v1;
        match kind {
            FilterType::LowPass => v2,
            FilterType::HighPass => input - band - v2,
            FilterType::BandPass => band,
            FilterType::Notch => input - band,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const SR: f32 = 48_000.0;

    /// Gain of the filter for a sine at `freq`, measured after it has settled.
    fn gain_at(freq: f32, cutoff: f32, resonance: f32) -> f32 {
        gain_of(FilterType::LowPass, freq, cutoff, resonance)
    }

    fn gain_of(kind: FilterType, freq: f32, cutoff: f32, resonance: f32) -> f32 {
        let mut f = Filter::default();
        let n = SR as usize;
        let mut peak: f32 = 0.0;
        for i in 0..n {
            let x = (i as f32 * freq / SR * TAU).sin();
            let y = f.process(x, kind, cutoff, resonance, SR);
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
        // at full resonance, through every type. The output must stay finite and bounded.
        let mut filters: [Filter; 4] = Default::default();
        for i in 0..(2.0 * SR) as usize {
            let t = i as f32 / SR;
            let x = if (t * 110.0).fract() < 0.5 { 1.0 } else { -1.0 };
            let sweep = 0.5 + 0.5 * (t * 50.0 * TAU).sin();
            let cutoff = 20.0 * 1000f32.powf(sweep);
            for (n, f) in filters.iter_mut().enumerate() {
                let kind = FilterType::from_index(n);
                let y = f.process(x, kind, cutoff, 1.0, SR);
                assert!(y.is_finite() && y.abs() < 30.0, "{kind:?} sample {i}: {y}");
            }
        }
    }

    #[test]
    fn high_pass_keeps_highs_and_cuts_lows() {
        let high = gain_of(FilterType::HighPass, 10_000.0, 500.0, 0.0);
        let low = gain_of(FilterType::HighPass, 50.0, 5_000.0, 0.0);
        assert!((high - 1.0).abs() < 0.02, "high {high}");
        assert!(low < 0.01, "low {low}");
    }

    #[test]
    fn band_pass_peaks_at_full_level_whatever_the_resonance() {
        for res in [0.0, 0.5, 1.0] {
            let g = gain_of(FilterType::BandPass, 1_000.0, 1_000.0, res);
            assert!((g - 1.0).abs() < 0.02, "res {res}: gain {g}");
        }
        // Three octaves away, it's well down.
        let far = gain_of(FilterType::BandPass, 8_000.0, 1_000.0, 0.5);
        assert!(far < 0.1, "far {far}");
    }

    #[test]
    fn notch_cuts_the_cutoff_and_keeps_the_rest() {
        let at = gain_of(FilterType::Notch, 1_000.0, 1_000.0, 0.5);
        // 7 kHz, not 8: at 48 kHz an 8 kHz sine is exactly 6 samples per cycle, so the
        // samples always land on the same points and can miss the wave's true peak.
        let away = gain_of(FilterType::Notch, 7_000.0, 1_000.0, 0.5);
        assert!(at < 0.02, "at {at}");
        assert!((away - 1.0).abs() < 0.05, "away {away}");
    }
}
