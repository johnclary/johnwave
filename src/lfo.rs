//! The LFO (low-frequency oscillator) for modulating parameters, plus the note values
//! used to sync it to the DAW's tempo.
//!
//! It has two outputs:
//! - `wave`: a shape that morphs sine → triangle → saw → square, like the main
//!   oscillator. Every shape peaks at ±1, so a route's amount means the same for all.
//! - `random`: sample & hold. It jumps to a new random level at the start of each cycle,
//!   so it follows the same rate, sync and retrigger as the wave.
//!
//! Instant jumps (the square's edges, each new random level) would click when they move
//! the cutoff or pitch, so they're spread over a short ramp of `EDGE_S`.

use nice_plug::prelude::Enum;
use std::f32::consts::TAU;

/// The longest LFO Fade, either way: 10 s.
pub const MAX_FADE_MS: f32 = 10_000.0;

/// How much of the LFO a note gets, `since_start_s` seconds after it began, for an LFO
/// Fade of `fade_ms`. Positive fades in from nothing to full over that time, negative
/// starts at full and fades out to nothing, and under 1 ms either way is Off (always
/// full).
pub fn fade_gain(fade_ms: f32, since_start_s: f32) -> f32 {
    let length_s = fade_ms.abs() / 1000.0;
    if length_s < 0.001 {
        return 1.0;
    }
    let t = (since_start_s / length_s).min(1.0);
    if fade_ms > 0.0 { t } else { 1.0 - t }
}

/// How long a jump takes: 1.5 ms. Short enough to still sound like a hard step.
const EDGE_S: f32 = 0.0015;

/// Both LFO outputs for one sample, each in `-1..=1`.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct LfoOut {
    pub wave: f32,
    pub random: f32,
}

#[derive(Clone)]
pub struct Lfo {
    /// Position in the current cycle, always in `[0, 1)`.
    phase: f32,
    /// Random number generator state (never 0).
    rng: u32,
    /// The random output ramps from `random_from` to `random_to` at the start of a cycle.
    random_from: f32,
    random_to: f32,
}

impl Default for Lfo {
    fn default() -> Self {
        Self {
            phase: 0.0,
            rng: 0x9E37_79B9, // any non-zero starting value works
            random_from: 0.0,
            random_to: 0.0,
        }
    }
}

impl Lfo {
    /// Restart the cycle. The wave begins at 0 and rises, and the random output moves to
    /// a fresh level.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.new_random_level();
    }

    /// Give this LFO its own random sequence, so voices started together don't all pick
    /// the same random levels. Any number works as the seed.
    pub fn seed(&mut self, seed: u64) {
        // Mix the seed's bits so neighbouring seeds (1, 2, 3...) give unrelated sequences.
        let mixed = (seed ^ (seed >> 32)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.rng = (mixed >> 32) as u32 | 1;
    }

    /// Jump to a position in the cycle (0..1), e.g. to line up with the song.
    pub fn set_phase(&mut self, phase: f32) {
        let new_phase = phase - phase.floor();
        // How far the jump is, taking the short way around the cycle (−0.5..0.5).
        let step = new_phase - self.phase;
        let step = step - step.round();
        if self.phase + step >= 1.0 {
            // Moved forward past the start of a cycle: a new cycle began.
            self.new_random_level();
        } else if self.phase + step < 0.0 && step > -0.01 {
            // Nudged back a hair, to just before the start of the cycle we already began.
            // Stay put rather than starting this cycle (and picking a random level) a
            // second time. The next correction lines us up again. (A bigger jump back,
            // like moving the playhead, is followed as normal.)
            return;
        }
        self.phase = new_phase;
    }

    /// One sample of output, then advance by `rate_hz`.
    /// `shape` runs 0..=1: 0 = sine, ⅓ = triangle, ⅔ = saw, 1 = square.
    pub fn next(&mut self, rate_hz: f32, sample_rate: f32, shape: f32) -> LfoOut {
        // How much of one cycle a jump takes at this speed.
        let edge = (EDGE_S * rate_hz).clamp(1e-6, 0.5);
        let ramp = (self.phase / edge).min(1.0);
        let out = LfoOut {
            wave: wave(self.phase, shape, edge),
            random: self.random_from + (self.random_to - self.random_from) * ramp,
        };

        self.phase += rate_hz / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor(); // wrap into [0, 1)
            self.new_random_level();
        }
        out
    }

    /// Start ramping from wherever the random output is now to a new random level.
    fn new_random_level(&mut self) {
        self.random_from = self.random_to;
        self.random_to = self.next_random();
    }

    /// A random number in `-1..=1` ("xorshift": cheap and good enough for modulation).
    fn next_random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// The morphing wave at phase `p`. All shapes start at 0 and rise, like a sine, so the
/// sync and retrigger points don't move when the shape changes. `pub(crate)` lets the
/// window draw it too (the LFO's mini wave), but nothing outside this plugin.
pub(crate) fn wave(p: f32, shape: f32, edge: f32) -> f32 {
    let pos = shape.clamp(0.0, 1.0) * 3.0;
    let i = (pos as usize).min(2);
    let t = pos - i as f32;
    let (a, b) = match i {
        0 => (sine(p), triangle(p)),
        1 => (triangle(p), saw(p)),
        _ => (saw(p), square(p, edge)),
    };
    a + (b - a) * t
}

fn sine(p: f32) -> f32 {
    (p * TAU).sin()
}

/// Peaks at p = ¼, troughs at p = ¾.
fn triangle(p: f32) -> f32 {
    let q = wrap(p + 0.25);
    1.0 - 4.0 * (q - 0.5).abs()
}

/// Rises from 0 to +1, drops to −1 at p = ½, then rises back to 0.
fn saw(p: f32) -> f32 {
    2.0 * wrap(p + 0.5) - 1.0
}

/// +1 for the first half, −1 for the second. Made by steepening the triangle and
/// clipping it, so each edge becomes a short ramp `edge` cycles long instead of a jump.
fn square(p: f32, edge: f32) -> f32 {
    // The triangle crosses 0 at the same points the square flips, moving 4 per cycle.
    // Over a phase of `edge` it should cross from −1 to +1, a rise of 2.
    let steepness = (2.0 / (4.0 * edge)).max(1.0);
    (triangle(p) * steepness).clamp(-1.0, 1.0)
}

fn wrap(x: f32) -> f32 {
    x - x.floor()
}

/// How long one LFO cycle lasts when synced to tempo. Listed slowest to fastest.
#[derive(Enum, PartialEq, Clone, Copy, Debug)]
pub enum SyncDivision {
    #[id = "4bar"]
    #[name = "4 bars"]
    Bars4,
    #[id = "2bar"]
    #[name = "2 bars"]
    Bars2,
    #[id = "1bar"]
    #[name = "1 bar"]
    Bar1,
    #[id = "1_2"]
    #[name = "1/2"]
    Half,
    #[id = "1_4d"]
    #[name = "1/4 dotted"]
    QuarterDotted,
    #[id = "1_4"]
    #[name = "1/4"]
    Quarter,
    #[id = "1_4t"]
    #[name = "1/4 triplet"]
    QuarterTriplet,
    #[id = "1_8d"]
    #[name = "1/8 dotted"]
    EighthDotted,
    #[id = "1_8"]
    #[name = "1/8"]
    Eighth,
    #[id = "1_8t"]
    #[name = "1/8 triplet"]
    EighthTriplet,
    #[id = "1_16"]
    #[name = "1/16"]
    Sixteenth,
    #[id = "1_16t"]
    #[name = "1/16 triplet"]
    SixteenthTriplet,
    #[id = "1_32"]
    #[name = "1/32"]
    ThirtySecond,
}

impl SyncDivision {
    /// Length of one cycle in quarter notes (what DAWs call "beats").
    /// `bar_beats` is the length of a bar, e.g. 4 in 4/4 or 3 in 3/4.
    pub fn beats(self, bar_beats: f64) -> f64 {
        match self {
            Self::Bars4 => 4.0 * bar_beats,
            Self::Bars2 => 2.0 * bar_beats,
            Self::Bar1 => bar_beats,
            Self::Half => 2.0,
            Self::QuarterDotted => 1.5,
            Self::Quarter => 1.0,
            Self::QuarterTriplet => 2.0 / 3.0,
            Self::EighthDotted => 0.75,
            Self::Eighth => 0.5,
            Self::EighthTriplet => 1.0 / 3.0,
            Self::Sixteenth => 0.25,
            Self::SixteenthTriplet => 1.0 / 6.0,
            Self::ThirtySecond => 0.125,
        }
    }

    /// LFO speed in Hz at the given tempo.
    pub fn rate_hz(self, bpm: f64, bar_beats: f64) -> f32 {
        (bpm / 60.0 / self.beats(bar_beats)) as f32
    }

    /// Where the LFO should be in its cycle at song position `pos_beats`, so the cycle
    /// always starts on the beat (or on the bar, for the bar lengths).
    pub fn phase_at(self, pos_beats: f64, bar_beats: f64) -> f32 {
        (pos_beats / self.beats(bar_beats)).rem_euclid(1.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn synced_rates_match_the_tempo() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        // At 120 BPM a quarter note lasts 0.5 s, so a 1/4 LFO runs at 2 Hz.
        assert!(close(SyncDivision::Quarter.rate_hz(120.0, 4.0), 2.0));
        assert!(close(SyncDivision::Bar1.rate_hz(120.0, 4.0), 0.5));
        assert!(close(SyncDivision::EighthTriplet.rate_hz(120.0, 4.0), 6.0));
        assert!(close(SyncDivision::QuarterDotted.rate_hz(90.0, 4.0), 1.0));
        // In 3/4, one bar is three quarter notes long.
        assert!(close(SyncDivision::Bar1.rate_hz(120.0, 3.0), 2.0 / 3.0));
    }

    #[test]
    fn synced_phase_follows_the_song_position() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert!(close(SyncDivision::Bar1.phase_at(2.5, 4.0), 0.625));
        assert!(close(SyncDivision::Quarter.phase_at(7.25, 4.0), 0.25));
        assert!(close(SyncDivision::Bars2.phase_at(16.0, 4.0), 0.0)); // a new cycle every 8 beats
        assert!(close(SyncDivision::Bar1.phase_at(-1.0, 4.0), 0.75)); // pre-roll before bar 1
    }

    /// The wave output only, for the tests that just need that.
    fn wave_out(lfo: &mut Lfo, rate: f32, shape: f32) -> f32 {
        lfo.next(rate, SR, shape).wave
    }

    #[test]
    fn stays_in_range() {
        for step in 0..=12 {
            let shape = step as f32 / 12.0;
            let mut lfo = Lfo::default();
            for _ in 0..(SR as usize) {
                let out = lfo.next(7.3, SR, shape);
                assert!(out.wave.abs() <= 1.0, "shape {shape}: {}", out.wave);
                assert!(out.random.abs() <= 1.0);
            }
        }
    }

    #[test]
    fn shapes_at_known_points() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        let edge = 0.001;
        for &(p, sine_v, tri_v, saw_v, sq_v) in &[
            (0.0, 0.0, 0.0, 0.0, 0.0),
            (0.25, 1.0, 1.0, 0.5, 1.0),
            (0.5, 0.0, 0.0, -1.0, 0.0),
            (0.75, -1.0, -1.0, -0.5, -1.0),
        ] {
            assert!(close(wave(p, 0.0, edge), sine_v), "sine at {p}");
            assert!(close(wave(p, 1.0 / 3.0, edge), tri_v), "triangle at {p}");
            assert!(close(wave(p, 2.0 / 3.0, edge), saw_v), "saw at {p}");
            assert!(close(wave(p, 1.0, edge), sq_v), "square at {p}");
        }
        // Halfway between sine and triangle is the average of the two.
        assert!(close(wave(0.125, 1.0 / 6.0, edge), (sine(0.125) + triangle(0.125)) / 2.0));
    }

    #[test]
    fn square_edges_are_short_ramps_not_jumps() {
        // At 2 Hz the 1.5 ms ramp is 0.003 of a cycle (≈ 72 samples at 48 kHz), so no
        // single sample can move far.
        let mut lfo = Lfo::default();
        let out: Vec<f32> = (0..SR as usize).map(|_| wave_out(&mut lfo, 2.0, 1.0)).collect();
        let biggest_step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(biggest_step < 0.05, "step {biggest_step}");
        // ...but it still spends almost all its time at ±1.
        let at_full = out.iter().filter(|x| x.abs() > 0.999).count();
        assert!(at_full as f32 > 0.98 * SR);
    }

    #[test]
    fn random_holds_then_moves_once_per_cycle() {
        // At 4 Hz there are 4 cycles in a second. The random output settles on one level
        // per cycle, and the levels differ.
        let mut lfo = Lfo::default();
        lfo.reset();
        let out: Vec<f32> = (0..SR as usize).map(|_| lfo.next(4.0, SR, 0.0).random).collect();
        let cycle = (SR / 4.0) as usize;
        let levels: Vec<f32> = (0..4).map(|c| out[c * cycle + cycle / 2]).collect();
        for c in 0..4 {
            // Flat from just after the ramp to the end of the cycle.
            let held = &out[c * cycle + 200..(c + 1) * cycle];
            assert!(held.iter().all(|&x| x == levels[c]), "cycle {c} not held");
        }
        for pair in levels.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
        // And it ramps, never jumps.
        let biggest_step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(biggest_step < 0.05, "step {biggest_step}");
    }

    #[test]
    fn syncing_across_the_cycle_start_picks_one_random_level() {
        let mut lfo = Lfo::default();
        lfo.set_phase(0.5);
        lfo.set_phase(0.999);
        assert_eq!(lfo.random_to, 0.0); // no new cycle yet
        lfo.set_phase(0.001); // forward over the start: a new level
        let after_forward = lfo.random_to;
        assert_ne!(after_forward, 0.0);
        lfo.set_phase(0.998); // nudged back over the start: ignored
        assert_eq!(lfo.random_to, after_forward);
        assert!(lfo.phase < 0.5);
        lfo.set_phase(0.4); // an ordinary jump within the cycle
        assert_eq!(lfo.random_to, after_forward);
        assert_eq!(lfo.phase, 0.4);
        lfo.set_phase(0.1);
        lfo.set_phase(0.8); // a big jump back over the start (playhead moved): followed
        assert_eq!(lfo.phase, 0.8);
    }

    #[test]
    fn different_seeds_give_different_random_levels() {
        let first_level = |seed: u64| {
            let mut lfo = Lfo::default();
            lfo.seed(seed);
            lfo.reset();
            lfo.next(1.0, SR, 0.0); // the ramp starts at the previous level...
            lfo.random_to // ...and heads to this one
        };
        assert_ne!(first_level(1), first_level(2));
        assert_ne!(first_level(2), first_level(3));
    }

    #[test]
    fn runs_at_the_requested_rate() {
        // At 2 Hz the output crosses upward through 0 at 0.5 s and 1.0 s (the start at
        // t = 0 doesn't count). Render 1.25 s so neither crossing sits on the window edge.
        let mut lfo = Lfo::default();
        let n = (1.25 * SR) as usize;
        let out: Vec<f32> = (0..n).map(|_| wave_out(&mut lfo, 2.0, 0.0)).collect();
        let upward_crossings = out.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert_eq!(upward_crossings, 2);
    }

    #[test]
    fn reset_restarts_from_zero() {
        let mut lfo = Lfo::default();
        for _ in 0..1234 {
            wave_out(&mut lfo, 3.0, 0.0);
        }
        lfo.reset();
        assert_eq!(wave_out(&mut lfo, 3.0, 0.0), 0.0);
    }
}
