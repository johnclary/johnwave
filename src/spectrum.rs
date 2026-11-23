//! The FREQ view's live spectrum: how loud the output is at each frequency, worked out
//! in the window from the OUT capture (`screen::OutputCapture`).
//!
//! A Fourier transform turns a stretch of sound (a level at each moment) into its
//! ingredients (a level at each frequency). The FFT, the "fast" Fourier transform, is a
//! quick way to do it for a power-of-two number of samples. Each output slot is a
//! "bin": bin `k` holds how much there is of `k × sample_rate / FFT_SIZE` Hz.
//!
//! Like `trail.rs`, this is window-only state. The audio thread never runs any of it.

use std::f32::consts::TAU;

/// How many samples go into each analysis: about 85 ms at 48 kHz, with bins 11.7 Hz
/// apart. Longer would show more detail in the bass but react more slowly.
pub const FFT_SIZE: usize = 4096;

/// How many columns the spectrum is drawn as, across the view. About one per screen
/// point, like the OUT line.
pub const COLUMNS: usize = 240;

/// The quietest level kept, in dBFS. Silence reads as this, not minus infinity.
pub const FLOOR_DB: f32 = -120.0;

/// How fast a level falls once the sound at that frequency goes, in dB per second. A
/// rise shows at once, but a fall slides down at this speed, like a hardware
/// analyser, so the picture doesn't flicker.
const FALL_DB_PER_SEC: f32 = 20.0;

/// Turns `re` and `im` (the real and imaginary halves of each value, all `im` 0 for
/// sound) into their Fourier transform, in place. The length must be a power of two.
///
/// This is the textbook "radix-2" FFT. First the values are shuffled into bit-reversed
/// order (slot 0b001 swaps with 0b100, and so on). Then pairs are combined into
/// transforms of 2, those into transforms of 4, and so on up to the whole length.
/// Each step is one "butterfly": a sum and a difference of two values, one turned by
/// an angle (the "twiddle").
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    assert!(n >= 2 && n.is_power_of_two() && im.len() == n);

    let bits = n.trailing_zeros();
    for i in 0..n {
        let j = i.reverse_bits() >> (usize::BITS - bits);
        // Only swap once per pair.
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let step = -TAU / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..half {
                let (sin, cos) = (step * k as f32).sin_cos();
                let (a, b) = (start + k, start + k + half);
                // The second value, turned by the twiddle angle.
                let tr = re[b] * cos - im[b] * sin;
                let ti = re[b] * sin + im[b] * cos;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len *= 2;
    }
}

/// The level of each bin of `samples`, in dBFS (0 dB is a full-scale sine), from 0 Hz
/// up to half the sample rate. `samples.len()` must be a power of two.
///
/// The samples are first faded in and out with a Hann window (a smooth hump). Cutting
/// a stretch out of a sound leaves sharp edges, and without the fade those edges would
/// smear every note across the whole spectrum.
pub fn analyse(samples: &[f32]) -> Vec<f32> {
    let n = samples.len();
    let hann = |i: usize| 0.5 - 0.5 * (TAU * i as f32 / n as f32).cos();
    let mut re: Vec<f32> = samples.iter().enumerate().map(|(i, &x)| x * hann(i)).collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    // A sine of size A comes out as A × (the window's total) / 2. The Hann window adds
    // up to n / 2, so multiplying by 4 / n reads the sine's size back.
    let scale = 4.0 / n as f32;
    (0..=n / 2)
        .map(|k| {
            let size = (re[k] * re[k] + im[k] * im[k]).sqrt() * scale;
            (20.0 * size.max(1e-9).log10()).max(FLOOR_DB)
        })
        .collect()
}

/// `bins` (from `analyse`) shared out between `COLUMNS` columns from `low` to `high`
/// Hz, each octave the same width. A column that covers several bins takes the loudest,
/// so a narrow peak isn't lost. Down in the bass, where one bin is wider than a column,
/// it's blended from the two nearest bins so it doesn't come out as steps.
pub fn columns(bins: &[f32], sample_rate: f32, low: f32, high: f32) -> Vec<f32> {
    let fft_size = (bins.len() - 1) * 2;
    let bin_of = |hz: f32| hz * fft_size as f32 / sample_rate;
    let last = (bins.len() - 1) as f32;
    let hz_at = |c: f32| low * (high / low).powf(c / COLUMNS as f32);
    (0..COLUMNS)
        .map(|c| {
            let (from, to) = (bin_of(hz_at(c as f32)), bin_of(hz_at(c as f32 + 1.0)));
            if from > last {
                // Above half the sample rate there's nothing.
                FLOOR_DB
            } else if to - from < 1.0 {
                let at = bin_of(hz_at(c as f32 + 0.5)).min(last);
                let below = at.floor() as usize;
                let above = (below + 1).min(bins.len() - 1);
                let frac = at - below as f32;
                bins[below] * (1.0 - frac) + bins[above] * frac
            } else {
                let first = from.ceil() as usize;
                let end = (to.floor() as usize).min(bins.len() - 1);
                bins[first..=end].iter().copied().fold(FLOOR_DB, f32::max)
            }
        })
        .collect()
}

/// What the spectrum shows: one level per column, in dBFS. It jumps up at once and
/// slides down slowly.
#[derive(Default)]
pub struct Spectrum {
    /// Left to right. Empty when there's nothing to show.
    shown: Vec<f32>,
    /// When it was last updated, in seconds (egui's clock).
    time: f64,
}

impl Spectrum {
    /// Call once per frame with the time and the newest `FFT_SIZE` samples of output
    /// (`None` when there are none, which clears it so the next note starts fresh).
    /// `low` and `high` are the view's range in Hz.
    pub fn update(&mut self, now: f64, samples: Option<&[f32]>, sample_rate: f32, low: f32, high: f32) {
        let Some(samples) = samples else {
            self.shown.clear();
            return;
        };
        let levels = columns(&analyse(samples), sample_rate, low, high);
        if self.shown.len() != levels.len() {
            self.shown = levels;
        } else {
            // How far a level may fall since the last frame. It's worked out from the
            // time that has passed, so it looks the same however often frames come.
            let fall = FALL_DB_PER_SEC * (now - self.time).max(0.0) as f32;
            for (shown, new) in self.shown.iter_mut().zip(levels) {
                *shown = new.max(*shown - fall);
            }
        }
        self.time = now;
    }

    /// The levels to draw, left to right, or `None` when there's nothing to show.
    pub fn levels(&self) -> Option<&[f32]> {
        (!self.shown.is_empty()).then_some(&self.shown[..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    const LOW: f32 = 20.0;
    const HIGH: f32 = 20_000.0;

    fn sine(freq: f32, size: f32) -> Vec<f32> {
        (0..FFT_SIZE).map(|i| size * (TAU * freq * i as f32 / SR).sin()).collect()
    }

    /// The column whose band holds `hz`.
    fn column_of(hz: f32) -> usize {
        ((hz / LOW).log2() / (HIGH / LOW).log2() * COLUMNS as f32) as usize
    }

    #[test]
    fn fft_matches_a_plain_fourier_sum() {
        // Made-up but repeatable values (a simple random-number formula).
        let mut seed = 1u32;
        let input: Vec<f32> = (0..64)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                seed as f32 / u32::MAX as f32 - 0.5
            })
            .collect();
        let (mut re, mut im) = (input.clone(), vec![0.0; 64]);
        fft(&mut re, &mut im);
        // The slow way: for every bin, add up every sample turned by its angle.
        for k in 0..64 {
            let (mut sr, mut si) = (0.0, 0.0);
            for (i, &x) in input.iter().enumerate() {
                let angle = -TAU * (k * i) as f32 / 64.0;
                sr += x * angle.cos();
                si += x * angle.sin();
            }
            assert!((re[k] - sr).abs() < 1e-4 && (im[k] - si).abs() < 1e-4, "bin {k}");
        }
    }

    #[test]
    fn full_scale_sine_reads_0_db_at_its_frequency() {
        let levels = columns(&analyse(&sine(1_000.0, 1.0)), SR, LOW, HIGH);
        let loudest = (0..COLUMNS).max_by(|&a, &b| levels[a].total_cmp(&levels[b])).unwrap();
        assert!(loudest.abs_diff(column_of(1_000.0)) <= 1, "peak at column {loudest}");
        assert!(levels[loudest] <= 0.1 && levels[loudest] > -SCALLOP_DB, "peak is {} dB", levels[loudest]);
        // Well away from it, it's far quieter.
        assert!(levels[column_of(100.0)] < -60.0);
        assert!(levels[column_of(10_000.0)] < -60.0);
    }

    /// A sine that falls between two bins reads a little low, up to about 1.4 dB with a
    /// Hann window at exactly halfway ("scalloping"). Too small to see on the screen.
    const SCALLOP_DB: f32 = 1.5;

    #[test]
    fn half_size_sine_reads_6_db_down() {
        // 440 Hz is bin 37.5 at this size: as far from a bin as a sine can be.
        let levels = columns(&analyse(&sine(440.0, 0.5)), SR, LOW, HIGH);
        let peak = levels.iter().copied().fold(FLOOR_DB, f32::max);
        assert!(peak <= -6.0 + 0.1 && peak > -6.0 - SCALLOP_DB, "peak is {peak} dB");
    }

    #[test]
    fn silence_is_the_floor() {
        let levels = columns(&analyse(&[0.0; FFT_SIZE]), SR, LOW, HIGH);
        assert!(levels.iter().all(|&db| db == FLOOR_DB));
    }

    #[test]
    fn rises_at_once_and_falls_slowly() {
        let mut s = Spectrum::default();
        let (tone, quiet) = (sine(1_000.0, 1.0), vec![0.0; FFT_SIZE]);
        let col = column_of(1_000.0);
        s.update(0.0, Some(&tone), SR, LOW, HIGH);
        // Half a second of silence later, it has only slid part of the way down.
        s.update(0.5, Some(&quiet), SR, LOW, HIGH);
        let level = s.levels().unwrap()[col];
        assert!((level + 0.5 * FALL_DB_PER_SEC).abs() < 1.5, "fell to {level} dB");
        // The tone again: straight back up.
        s.update(0.51, Some(&tone), SR, LOW, HIGH);
        assert!(s.levels().unwrap()[col] > -1.0);
        // No output at all clears it.
        s.update(0.52, None, SR, LOW, HIGH);
        assert_eq!(s.levels(), None);
    }
}
