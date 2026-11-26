//! A resonant filter: low-pass, high-pass, band-pass or notch, at 12 or 24 dB/octave.
//!
//! This is a state-variable filter (SVF) in the "topology-preserving transform" form
//! described by Andrew Simper (Cytomic). It stays stable and smooth even when the cutoff
//! moves every sample, which matters because envelopes sweep it.
//!
//! An SVF works out its low-pass and band-pass outputs together each sample, and the
//! other types are just those mixed with the input. So all four cost the same, and you
//! can switch type while a note plays without the filter losing its memory.
//!
//! Drive pushes the sound into the filter harder, in two ways. Before the filter, a soft
//! clipper (tanh) rounds off the peaks and adds harmonics for the filter to shape. Inside
//! it, the band-pass state is soft-clipped too, the way an analog filter's parts saturate:
//! at high resonance the peak gets squashed and grinds instead of ringing cleanly. At 0%
//! drive both are skipped and the filter is exactly the clean one.
//!
//! The 24 dB slope runs two of these stages one after the other. Only the second one
//! gets the resonance: the first is fixed at the damping of a 4-pole Butterworth filter,
//! so at 0% resonance the pair is flat, and at 100% there's one peak, not two stacked up.

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

/// How steeply the filter cuts past the cutoff.
#[derive(Enum, PartialEq, Clone, Copy, Debug)]
pub enum FilterSlope {
    /// One stage: gentle, open.
    #[id = "12"]
    #[name = "12 dB"]
    Db12,
    /// Two stages: steeper, darker and rounder.
    #[id = "24"]
    #[name = "24 dB"]
    Db24,
}

/// Damping at 0% resonance: the flattest response with no peak (Q ≈ 0.707).
const K_MAX: f32 = std::f32::consts::SQRT_2;
/// Damping at 100% resonance (Q = 10): a strong, ringing peak. It stays just short of
/// self-oscillation, so the filter can't whistle on its own.
const K_MIN: f32 = 0.1;

/// 4-pole Butterworth damping for the two stages of the 24 dB slope (2·cos(π/8) and
/// 2·cos(3π/8)). Together they give a flat response at 0% resonance.
const K_24_FIRST: f32 = 1.847_759;
const K_24_SECOND: f32 = 0.765_367;

/// Pre-filter gain at 100% drive: 8× (about +18 dB) for quiet signals.
const MAX_DRIVE_GAIN: f32 = 8.0;

/// One 12 dB/octave SVF stage.
#[derive(Default)]
struct Stage {
    // The stage's memory: two integrator states.
    ic1eq: f32,
    ic2eq: f32,
}

impl Stage {
    /// Filter one sample. `g` sets the cutoff and `k` the damping (less = more resonance).
    /// `sat` (0..=1) blends the band-pass state toward its soft-clipped version.
    fn process(&mut self, input: f32, kind: FilterType, g: f32, k: f32, sat: f32) -> f32 {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = input - self.ic2eq;
        let mut v1 = a1 * self.ic1eq + a2 * v3;
        let mut v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;
        if sat > 0.0 {
            // Saturate the band state, then work the low-pass out from that (v2 is
            // always ic2eq + g·v1), so the two stay consistent. tanh can only shrink v1,
            // which also keeps the resonance from running away.
            v1 += (v1.tanh() - v1) * sat;
            v2 = self.ic2eq + g * v1;
        }
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

#[derive(Default)]
pub struct Filter {
    stages: [Stage; 2],
}

impl Filter {
    pub fn reset(&mut self) {
        self.stages = Default::default();
    }

    /// Filter one sample. `resonance` and `drive` run 0..=1.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        input: f32,
        kind: FilterType,
        slope: FilterSlope,
        cutoff_hz: f32,
        resonance: f32,
        drive: f32,
        sample_rate: f32,
    ) -> f32 {
        // Keep the cutoff in a safe range. tan() blows up approaching Nyquist.
        let cutoff = cutoff_hz.clamp(10.0, 0.45 * sample_rate);
        let g = (PI * cutoff / sample_rate).tan();
        let res = resonance.clamp(0.0, 1.0);
        let drive = drive.clamp(0.0, 1.0);

        // tanh(gain·x) / tanh(gain): a full-scale peak stays at full scale, quieter parts
        // are boosted and rounded. As the gain shrinks toward 0 this becomes plain x, so
        // the sound changes smoothly as Drive leaves 0%.
        let gain = MAX_DRIVE_GAIN * drive;
        let input = if gain > 1e-3 {
            (gain * input).tanh() / gain.tanh()
        } else {
            input
        };

        match slope {
            FilterSlope::Db12 => {
                // Keep the unused second stage empty, so switching to 24 dB mid-note
                // starts it fresh instead of from a leftover old state.
                self.stages[1] = Stage::default();
                let k = K_MAX + (K_MIN - K_MAX) * res;
                self.stages[0].process(input, kind, g, k, drive)
            }
            FilterSlope::Db24 => {
                let k = K_24_SECOND + (K_MIN - K_24_SECOND) * res;
                let first = self.stages[0].process(input, kind, g, K_24_FIRST, drive);
                self.stages[1].process(first, kind, g, k, drive)
            }
        }
    }
}

/// One filter's settings, for drawing its curve.
#[derive(Clone, Copy, Debug)]
pub struct CurveSettings {
    pub kind: FilterType,
    pub slope: FilterSlope,
    pub cutoff_hz: f32,
    pub resonance: f32,
}

/// How much the clean filters (Drive at 0%) let through at `freq`: 1 is unchanged,
/// 0.5 is half as loud (about −6 dB), above 1 is boosted (a resonant peak). For drawing
/// the curve, so the window doesn't have to run the filters to find out.
///
/// Filter 1 alone, or with Filter 2 (`Some`, with
/// `true` for parallel). In serial the sound goes through both, so their gains multiply.
/// In parallel the two outputs are mixed 50/50, and how they add depends on their
/// timing (phase) as well as their levels: a low-pass and a high-pass at the same
/// cutoff partly cancel there, leaving a dip. So this works with the full complex
/// response, not just the levels.
pub fn combined_response(
    filter1: CurveSettings,
    filter2: Option<(CurveSettings, bool)>,
    freq: f32,
    sample_rate: f32,
) -> f32 {
    let h1 = complex_response(filter1, freq, sample_rate);
    let h = match filter2 {
        None => h1,
        Some((settings, parallel)) => {
            let h2 = complex_response(settings, freq, sample_rate);
            if parallel {
                ((h1.0 + h2.0) / 2.0, (h1.1 + h2.1) / 2.0)
            } else {
                mul(h1, h2)
            }
        }
    };
    magnitude(h)
}

/// A complex number as (real, imaginary). It holds both how loud a frequency comes out
/// and how far it's delayed (its phase), which is what adding two filters needs.
type Complex = (f32, f32);

fn mul(a: Complex, b: Complex) -> Complex {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}

fn div(a: Complex, b: Complex) -> Complex {
    let d = b.0 * b.0 + b.1 * b.1;
    ((a.0 * b.0 + a.1 * b.1) / d, (a.1 * b.0 - a.0 * b.1) / d)
}

fn magnitude(a: Complex) -> f32 {
    a.0.hypot(a.1)
}

/// The filter's complex response at `freq`. It's worked out from the filter's own
/// formulas: the SVF behaves exactly like a classic analog filter whose frequencies have
/// been squeezed to fit below the top of the sample rate, and `w` undoes that squeeze.
/// At `w` = 1 you're at the cutoff.
fn complex_response(settings: CurveSettings, freq: f32, sample_rate: f32) -> Complex {
    let CurveSettings { kind, slope, cutoff_hz, resonance } = settings;
    // The same limits `process` uses.
    let cutoff = cutoff_hz.clamp(10.0, 0.45 * sample_rate);
    let g = (PI * cutoff / sample_rate).tan();
    let res = resonance.clamp(0.0, 1.0);
    let w = (PI * freq.min(0.5 * sample_rate) / sample_rate).tan() / g;
    // One stage with damping `k`. Each type is a different top line over the same
    // bottom line, which is where the peak comes from: it's smallest at the cutoff, and
    // smaller still with less damping.
    let stage = |k: f32| {
        let bottom = (1.0 - w * w, k * w);
        let top = match kind {
            FilterType::LowPass => (1.0, 0.0),
            FilterType::HighPass => (-w * w, 0.0),
            FilterType::BandPass => (0.0, k * w),
            FilterType::Notch => (1.0 - w * w, 0.0),
        };
        div(top, bottom)
    };
    match slope {
        FilterSlope::Db12 => stage(K_MAX + (K_MIN - K_MAX) * res),
        FilterSlope::Db24 => mul(stage(K_24_FIRST), stage(K_24_SECOND + (K_MIN - K_24_SECOND) * res)),
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
        gain_sloped(kind, FilterSlope::Db12, freq, cutoff, resonance)
    }

    fn gain_sloped(
        kind: FilterType,
        slope: FilterSlope,
        freq: f32,
        cutoff: f32,
        resonance: f32,
    ) -> f32 {
        let mut f = Filter::default();
        let n = SR as usize;
        let mut peak: f32 = 0.0;
        for i in 0..n {
            let x = (i as f32 * freq / SR * TAU).sin();
            let y = f.process(x, kind, slope, cutoff, resonance, 0.0, SR);
            if i > n / 2 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn response_matches_the_real_filter() {
        // Every type and slope, gentle and resonant, below, at and above the cutoff.
        for kind in [FilterType::LowPass, FilterType::HighPass, FilterType::BandPass, FilterType::Notch] {
            for slope in [FilterSlope::Db12, FilterSlope::Db24] {
                for res in [0.0, 0.7] {
                    for freq in [200.0, 1_000.0, 4_000.0] {
                        let measured = gain_sloped(kind, slope, freq, 1_000.0, res);
                        let settings = CurveSettings { kind, slope, cutoff_hz: 1_000.0, resonance: res };
                        let worked_out = combined_response(settings, None, freq, SR);
                        // A notch right on its cutoff is (nearly) silent either way.
                        let close = (measured - worked_out).abs() < 0.02 * worked_out.max(0.05);
                        assert!(close, "{kind:?} {slope:?} res {res} at {freq} Hz: {measured} vs {worked_out}");
                    }
                }
            }
        }
    }

    #[test]
    fn combined_response_matches_the_real_filters() {
        // Filter 1 a resonant low-pass at 1 kHz, Filter 2 a high-pass at 1 kHz: run both
        // ways on sines and compare with the worked-out curve. Parallel at the cutoff is
        // where the phase matters most.
        let f1 = CurveSettings { kind: FilterType::LowPass, slope: FilterSlope::Db12, cutoff_hz: 1_000.0, resonance: 0.3 };
        let f2 = CurveSettings { kind: FilterType::HighPass, slope: FilterSlope::Db24, cutoff_hz: 1_000.0, resonance: 0.0 };
        for parallel in [false, true] {
            for freq in [200.0, 1_000.0, 4_000.0] {
                let (mut a, mut b) = (Filter::default(), Filter::default());
                let n = SR as usize;
                let mut peak: f32 = 0.0;
                for i in 0..n {
                    let x = (i as f32 * freq / SR * TAU).sin();
                    let y1 = a.process(x, f1.kind, f1.slope, f1.cutoff_hz, f1.resonance, 0.0, SR);
                    let input2 = if parallel { x } else { y1 };
                    let y2 = b.process(input2, f2.kind, f2.slope, f2.cutoff_hz, f2.resonance, 0.0, SR);
                    let y = if parallel { (y1 + y2) / 2.0 } else { y2 };
                    if i > n / 2 {
                        peak = peak.max(y.abs());
                    }
                }
                let worked_out = combined_response(f1, Some((f2, parallel)), freq, SR);
                let close = (peak - worked_out).abs() < 0.02 * worked_out.max(0.05);
                assert!(close, "parallel {parallel} at {freq} Hz: {peak} vs {worked_out}");
            }
        }
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
        // Both slopes, clean and fully driven.
        let mut filters: [Filter; 16] = Default::default();
        for i in 0..(2.0 * SR) as usize {
            let t = i as f32 / SR;
            let x = if (t * 110.0).fract() < 0.5 { 1.0 } else { -1.0 };
            let sweep = 0.5 + 0.5 * (t * 50.0 * TAU).sin();
            let cutoff = 20.0 * 1000f32.powf(sweep);
            for (n, f) in filters.iter_mut().enumerate() {
                let kind = FilterType::from_index(n % 4);
                let slope = FilterSlope::from_index(n / 4 % 2);
                let drive = (n / 8) as f32;
                let y = f.process(x, kind, slope, cutoff, 1.0, drive, SR);
                assert!(y.is_finite() && y.abs() < 30.0, "{kind:?} {slope:?} sample {i}: {y}");
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

    #[test]
    fn slope_24_cuts_twice_as_hard() {
        // Two octaves above a 1 kHz cutoff: 12 dB/oct gives about −24 dB, 24 dB/oct
        // about −48 dB.
        let db = |g: f32| 20.0 * g.log10();
        let g12 = db(gain_sloped(FilterType::LowPass, FilterSlope::Db12, 4_000.0, 1_000.0, 0.0));
        let g24 = db(gain_sloped(FilterType::LowPass, FilterSlope::Db24, 4_000.0, 1_000.0, 0.0));
        assert!((g12 + 24.0).abs() < 2.0, "12 dB: {g12}");
        assert!((g24 + 48.0).abs() < 3.0, "24 dB: {g24}");
    }

    #[test]
    fn slope_24_is_flat_without_resonance_and_has_one_peak_with_it() {
        let at = |res| gain_sloped(FilterType::LowPass, FilterSlope::Db24, 1_000.0, 1_000.0, res);
        let low = gain_sloped(FilterType::LowPass, FilterSlope::Db24, 50.0, 1_000.0, 0.0);
        assert!((low - 1.0).abs() < 0.01, "low {low}");
        let flat = at(0.0);
        assert!((flat - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.02, "flat {flat}");
        // Only one stage resonates: a clear peak, nowhere near two stacked (~100×).
        let peaky = at(1.0);
        assert!(peaky > 4.0 && peaky < 8.0, "peaky {peaky}");
    }

    /// Run a 200 Hz sine through a wide-open low-pass with this drive and resonance.
    fn driven_sine(drive: f32, cutoff: f32, resonance: f32) -> Vec<f32> {
        let mut f = Filter::default();
        (0..SR as usize)
            .map(|i| {
                let x = (i as f32 * 200.0 / SR * TAU).sin();
                f.process(x, FilterType::LowPass, FilterSlope::Db12, cutoff, resonance, drive, SR)
            })
            .collect()
    }

    /// How much of `freq` is in the second half of `signal` (a one-bin Fourier sum).
    fn amount_of(signal: &[f32], freq: f32) -> f32 {
        let half = &signal[signal.len() / 2..];
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, &y) in half.iter().enumerate() {
            let phase = i as f32 * freq / SR * TAU;
            re += y * phase.cos();
            im += y * phase.sin();
        }
        2.0 * (re * re + im * im).sqrt() / half.len() as f32
    }

    #[test]
    fn drive_leaves_zero_smoothly() {
        // A hair of drive sounds the same as none: no jump as the knob leaves 0%.
        let clean = driven_sine(0.0, 20_000.0, 0.0);
        let hair = driven_sine(0.001, 20_000.0, 0.0);
        let worst = clean.iter().zip(&hair).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst < 0.01, "worst {worst}");
    }

    #[test]
    fn drive_adds_odd_harmonics() {
        let clean = amount_of(&driven_sine(0.0, 20_000.0, 0.0), 600.0);
        let driven = amount_of(&driven_sine(1.0, 20_000.0, 0.0), 600.0);
        assert!(clean < 0.001, "clean {clean}");
        assert!(driven > 0.05, "driven {driven}");
    }

    #[test]
    fn drive_squashes_the_resonant_peak() {
        let peak = |drive| {
            let out = driven_sine(drive, 200.0, 1.0);
            out[out.len() / 2..].iter().fold(0.0f32, |m, y| m.max(y.abs()))
        };
        let clean = peak(0.0);
        let driven = peak(1.0);
        assert!(clean > 8.0, "clean {clean}");
        assert!(driven < clean / 2.0, "driven {driven}");
    }
}
