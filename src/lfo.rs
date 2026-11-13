//! A sine LFO (low-frequency oscillator) for modulating parameters such as Morph.

use std::f32::consts::TAU;

#[derive(Default)]
pub struct Lfo {
    /// Position in the current cycle, always in `[0, 1)`.
    phase: f32,
}

impl Lfo {
    /// Restart the cycle. Output then begins at 0 and rises.
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// One sample of output in `-1..=1`, then advance by `rate_hz`.
    pub fn next(&mut self, rate_hz: f32, sample_rate: f32) -> f32 {
        let out = (self.phase * TAU).sin();
        self.phase += rate_hz / sample_rate;
        self.phase -= self.phase.floor(); // wrap into [0, 1)
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

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
