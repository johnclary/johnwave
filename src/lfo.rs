//! A sine LFO (low-frequency oscillator) for modulating parameters such as Morph,
//! plus the note values used to sync it to the DAW's tempo.

use nice_plug::prelude::Enum;
use std::f32::consts::TAU;

#[derive(Default, Clone)]
pub struct Lfo {
    /// Position in the current cycle, always in `[0, 1)`.
    phase: f32,
}

impl Lfo {
    /// Restart the cycle. Output then begins at 0 and rises.
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Jump to a position in the cycle (0..1), e.g. to line up with the song.
    pub fn set_phase(&mut self, phase: f32) {
        self.phase = phase - phase.floor();
    }

    /// One sample of output in `-1..=1`, then advance by `rate_hz`.
    pub fn next(&mut self, rate_hz: f32, sample_rate: f32) -> f32 {
        let out = (self.phase * TAU).sin();
        self.phase += rate_hz / sample_rate;
        self.phase -= self.phase.floor(); // wrap into [0, 1)
        out
    }
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

    #[test]
    fn stays_in_range() {
        let mut lfo = Lfo::default();
        for _ in 0..(SR as usize) {
            assert!(lfo.next(7.3, SR).abs() <= 1.0);
        }
    }

    #[test]
    fn runs_at_the_requested_rate() {
        // At 2 Hz the output crosses upward through 0 at 0.5 s and 1.0 s (the start at
        // t = 0 doesn't count). Render 1.25 s so neither crossing sits on the window edge.
        let mut lfo = Lfo::default();
        let n = (1.25 * SR) as usize;
        let out: Vec<f32> = (0..n).map(|_| lfo.next(2.0, SR)).collect();
        let upward_crossings = out.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert_eq!(upward_crossings, 2);
    }

    #[test]
    fn reset_restarts_from_zero() {
        let mut lfo = Lfo::default();
        for _ in 0..1234 {
            lfo.next(3.0, SR);
        }
        lfo.reset();
        assert_eq!(lfo.next(3.0, SR), 0.0);
    }
}
