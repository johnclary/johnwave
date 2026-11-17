//! The bitcrusher: makes the sound lo-fi in two separate ways.
//!
//! - **Bit depth** snaps every sample to a coarse grid of levels, like an old sampler
//!   with few bits. Fewer bits = bigger steps = more grit and hiss.
//! - **Sample-rate reduction** ("downsample") keeps one sample and repeats it for a
//!   while before taking the next, turning the wave into a staircase.
//!
//! A fixed downsample adds tones that have nothing to do with the note, which sounds
//! metallic. So the synth sets it from the note instead (`hold_length`): Steps says how
//! many stairs each wave cycle gets. The staircase then repeats exactly once per cycle,
//! every added tone is a harmonic of the note, and it sounds like lo-fi or chiptune.
//!
//! Both settings are floats, so modulation can sweep them smoothly: 7.5 bits sits
//! between 7 and 8, and a downsample of 2.5 takes a new sample every 2.5 samples on
//! average.

/// The most bits the knob goes up to. At 16 the steps are too small to hear.
pub const MAX_BITS: f32 = 16.0;
/// The fewest bits: just the levels −1, 0 and +1.
pub const MIN_BITS: f32 = 1.0;
/// The fewest stairs per wave cycle. Two gives a square-ish wave.
pub const MIN_STEPS: f32 = 2.0;
/// The most stairs per cycle. This top setting turns sample-rate reduction off.
pub const MAX_STEPS: f32 = 256.0;

/// How many samples each stair lasts so that a cycle at `freq` gets `steps` stairs:
/// the cycle's length in samples (`sample_rate / freq`) shared out between them.
/// `MAX_STEPS` means off (every sample is its own stair).
pub fn hold_length(steps: f32, freq: f32, sample_rate: f32) -> f32 {
    if steps >= MAX_STEPS {
        1.0
    } else {
        sample_rate / (freq * steps.max(MIN_STEPS))
    }
}

#[derive(Default)]
pub struct Crusher {
    /// The sample being repeated.
    held: f32,
    /// How much of the current hold is left. A new sample is taken when it hits 0.
    /// Starts at 0, so the very first sample is always taken.
    countdown: f32,
}

impl Crusher {
    pub fn reset(&mut self) {
        self.held = 0.0;
        self.countdown = 0.0;
    }

    /// Crush one sample.
    ///
    /// `bits` runs `MIN_BITS..=MAX_BITS`. `downsample` is how many samples each kept
    /// sample lasts: 1 = every sample (off), 4 = a quarter of the sample rate.
    pub fn process(&mut self, x: f32, bits: f32, downsample: f32) -> f32 {
        // Sample-rate reduction: take a new sample only when the countdown runs out.
        if self.countdown <= 0.0 {
            self.held = x;
            self.countdown += 1.0;
        }
        self.countdown -= 1.0 / downsample.max(1.0);

        quantize(self.held, bits)
    }
}

/// Snap `x` to the nearest step on a grid with `2^(bits−1)` steps per unit. A pure
/// function, so it's easy to test.
fn quantize(x: f32, bits: f32) -> f32 {
    let levels = (bits.clamp(MIN_BITS, MAX_BITS) - 1.0).exp2();
    let snapped = (x * levels).round() / levels;
    // With fractional bits the grid doesn't land exactly on ±1, so a sample near the
    // top can round past it. Don't let it go further out than 1, or than the input
    // itself if that was already louder (no extra clipping on a hot signal).
    let limit = x.abs().max(1.0);
    snapped.clamp(-limit, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test signal: a sine with a period that isn't a whole number of samples, so
    /// it hits lots of different values.
    fn sine(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.0137 * std::f32::consts::TAU).sin()).collect()
    }

    #[test]
    fn max_bits_and_no_downsample_is_nearly_unchanged() {
        let mut crusher = Crusher::default();
        for x in sine(4_800) {
            let y = crusher.process(x, MAX_BITS, 1.0);
            assert!((y - x).abs() <= 1.0 / 65_536.0 + 1e-7, "{x} became {y}");
        }
    }

    #[test]
    fn downsample_of_one_takes_every_sample() {
        let mut crusher = Crusher::default();
        let input = sine(1_000);
        let mut previous = f32::NAN;
        for x in input {
            let y = crusher.process(x, MAX_BITS, 1.0);
            assert_ne!(y, previous, "a sample was repeated");
            previous = y;
        }
    }

    #[test]
    fn downsample_of_four_holds_each_sample_for_four() {
        let mut crusher = Crusher::default();
        let input: Vec<f32> = (0..16).map(|i| i as f32 / 16.0).collect();
        let output: Vec<f32> = input.iter().map(|&x| crusher.process(x, MAX_BITS, 4.0)).collect();
        for (i, y) in output.iter().enumerate() {
            let kept = input[i - i % 4]; // samples 0, 4, 8, 12 are the ones kept
            assert!((y - kept).abs() < 1e-4, "sample {i}: expected {kept}, got {y}");
        }
    }

    #[test]
    fn fractional_downsample_takes_the_right_number_of_samples() {
        // At 2.5, 1000 samples in should keep about 400 of them.
        let mut crusher = Crusher::default();
        let mut taken = 0;
        let mut previous = f32::NAN;
        for i in 0..1_000 {
            let y = crusher.process(i as f32 / 1_000.0, MAX_BITS, 2.5);
            if y != previous {
                taken += 1;
            }
            previous = y;
        }
        assert!((399..=401).contains(&taken), "took {taken}");
    }

    #[test]
    fn fewer_bits_means_fewer_distinct_levels() {
        for bits in [1.0, 2.0, 3.0, 4.0] {
            let mut levels: Vec<f32> = sine(4_800).into_iter().map(|x| quantize(x, bits)).collect();
            levels.sort_by(f32::total_cmp);
            levels.dedup();
            // 2^(bits−1) steps on each side of zero, plus zero itself.
            let expected = 2 * (bits - 1.0).exp2() as usize + 1;
            assert_eq!(levels.len(), expected, "{bits} bits");
        }
    }

    #[test]
    fn stays_in_range_for_every_setting() {
        for bits in [1.0, 1.5, 4.0, 8.3, 16.0] {
            for downsample in [1.0, 2.5, 7.0, 64.0] {
                let mut crusher = Crusher::default();
                for x in sine(4_800) {
                    let y = crusher.process(x, bits, downsample);
                    assert!(y.abs() <= 1.0, "bits {bits} downsample {downsample}: {y}");
                }
            }
        }
    }

    #[test]
    fn hold_length_gives_each_cycle_the_asked_number_of_steps() {
        for freq in [55.0, 440.0, 1_234.5] {
            for steps in [2.0, 8.0, 33.3] {
                let cycle = 48_000.0 / freq; // samples per wave cycle
                let hold = hold_length(steps, freq, 48_000.0);
                assert!((cycle / hold - steps).abs() < 1e-3, "{freq} Hz, {steps} steps");
            }
        }
        assert_eq!(hold_length(MAX_STEPS, 55.0, 48_000.0), 1.0);
    }

    #[test]
    fn steps_follow_the_note() {
        // At 8 steps, a 480 Hz note (100 samples per cycle) holds each stair for 12.5
        // samples, so 1000 samples (10 cycles) keep 80.
        let mut crusher = Crusher::default();
        let hold = hold_length(8.0, 480.0, 48_000.0);
        let mut taken = 0;
        let mut previous = f32::NAN;
        for i in 0..1_000 {
            let y = crusher.process(i as f32 / 1_000.0, MAX_BITS, hold);
            if y != previous {
                taken += 1;
            }
            previous = y;
        }
        assert!((79..=81).contains(&taken), "took {taken}");
    }

    #[test]
    fn reset_takes_a_fresh_sample_straight_away() {
        let mut crusher = Crusher::default();
        crusher.process(0.5, MAX_BITS, 64.0);
        crusher.reset();
        let y = crusher.process(-0.25, MAX_BITS, 64.0);
        assert!((y + 0.25).abs() < 1e-4, "got {y}");
    }
}
