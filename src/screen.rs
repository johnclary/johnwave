//! What the oscillator screen draws: the oscillators of the newest voice, as shapes.
//!
//! The screen doesn't record any audio. The audio thread hands over a few numbers
//! (each oscillator's Shape, Osc 2's tuning, the mix, how loud the voice is) and the
//! window redraws the waves from them with the same `osc::shape` the sound uses.

use nice_plug::prelude::AtomicF32;
use std::sync::atomic::Ordering;

/// One voice's oscillators, as the screen draws them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenWave {
    /// Osc 1's Shape, after modulation (0..1).
    pub morph: f32,
    /// How loud the voice is right now (amp envelope × velocity, 0..1).
    pub level: f32,
    /// Osc 2's Shape, after modulation (0..1).
    pub morph2: f32,
    /// Osc 2's frequency as a multiple of osc 1's: 2.0 is an octave up.
    pub ratio2: f32,
    /// How much of each oscillator is in the mix. With Osc 2 off, 1 and 0.
    pub gain1: f32,
    pub gain2: f32,
}

impl Default for ScreenWave {
    fn default() -> Self {
        Self { morph: 0.0, level: 0.0, morph2: 0.0, ratio2: 1.0, gain1: 1.0, gain2: 0.0 }
    }
}

impl ScreenWave {
    /// Whether Osc 2 is in the sound at all (switched on with some Mix).
    pub fn has_osc2(&self) -> bool {
        self.gain2 > 0.0
    }

    /// Osc 1 at `x` cycles of osc 1 from the left edge, at full size (-1..1).
    pub fn osc1(&self, x: f32) -> f32 {
        crate::osc::shape(x.fract(), 0.0, self.morph)
    }

    /// Osc 2 at the same point. It's tuned against osc 1, so an octave up fits twice
    /// as many cycles in.
    pub fn osc2(&self, x: f32) -> f32 {
        crate::osc::shape((x * self.ratio2).fract(), 0.0, self.morph2)
    }

    /// The two added up the way the voice adds them, at the voice's level. Kept within
    /// -1..1 so it stays on the screen.
    pub fn mix(&self, x: f32) -> f32 {
        let s = self.osc1(x) * self.gain1 + self.osc2(x) * self.gain2;
        (s * self.level).clamp(-1.0, 1.0)
    }

    /// Whether `other` would look different on screen. Tiny changes don't count, so a
    /// steady note doesn't keep redrawing.
    pub fn moved(&self, other: &ScreenWave) -> bool {
        (self.morph - other.morph).abs() > 0.002
            || (self.level - other.level).abs() > 0.005
            || (self.morph2 - other.morph2).abs() > 0.002
            || (self.ratio2 - other.ratio2).abs() > 0.001
            || (self.gain2 - other.gain2).abs() > 0.005
    }
}

/// A `ScreenWave` the audio thread writes and the window reads, one atomic number per
/// field, so neither side ever waits for the other. A Shape of NaN means "nothing is
/// playing".
pub struct SharedScreen {
    morph: AtomicF32,
    level: AtomicF32,
    morph2: AtomicF32,
    ratio2: AtomicF32,
    gain1: AtomicF32,
    gain2: AtomicF32,
}

impl Default for SharedScreen {
    fn default() -> Self {
        let w = ScreenWave::default();
        Self {
            morph: AtomicF32::new(f32::NAN),
            level: AtomicF32::new(w.level),
            morph2: AtomicF32::new(w.morph2),
            ratio2: AtomicF32::new(w.ratio2),
            gain1: AtomicF32::new(w.gain1),
            gain2: AtomicF32::new(w.gain2),
        }
    }
}

impl SharedScreen {
    /// Called by the audio thread: the newest voice, or `None` when nothing is playing.
    pub fn store(&self, wave: Option<ScreenWave>) {
        let Some(w) = wave else {
            self.morph.store(f32::NAN, Ordering::Relaxed);
            return;
        };
        // Shape last: the window checks it first, so the others are already in place.
        self.level.store(w.level, Ordering::Relaxed);
        self.morph2.store(w.morph2, Ordering::Relaxed);
        self.ratio2.store(w.ratio2, Ordering::Relaxed);
        self.gain1.store(w.gain1, Ordering::Relaxed);
        self.gain2.store(w.gain2, Ordering::Relaxed);
        self.morph.store(w.morph, Ordering::Relaxed);
    }

    /// Called by either side: what the screen shows now, or `None` when silent.
    pub fn load(&self) -> Option<ScreenWave> {
        let morph = self.morph.load(Ordering::Relaxed);
        if morph.is_nan() {
            return None;
        }
        Some(ScreenWave {
            morph,
            level: self.level.load(Ordering::Relaxed),
            morph2: self.morph2.load(Ordering::Relaxed),
            ratio2: self.ratio2.load(Ordering::Relaxed),
            gain1: self.gain1.load(Ordering::Relaxed),
            gain2: self.gain2.load(Ordering::Relaxed),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_screen_round_trips() {
        let shared = SharedScreen::default();
        assert_eq!(shared.load(), None);
        let w = ScreenWave { morph: 0.3, level: 0.8, morph2: 0.6, ratio2: 2.0, gain1: 0.7, gain2: 0.7 };
        shared.store(Some(w));
        assert_eq!(shared.load(), Some(w));
        shared.store(None);
        assert_eq!(shared.load(), None);
    }

    #[test]
    fn mix_without_osc2_is_just_osc1() {
        let w = ScreenWave { morph: 0.4, level: 0.5, ..ScreenWave::default() };
        for i in 0..20 {
            let x = i as f32 / 10.0;
            assert!((w.mix(x) - w.osc1(x) * 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn octave_up_repeats_twice_as_often() {
        let w = ScreenWave { morph2: 0.3, ratio2: 2.0, ..ScreenWave::default() };
        // Half a cycle of osc 1 is a whole cycle of osc 2.
        assert!((w.osc2(0.1) - w.osc2(0.6)).abs() < 1e-5);
    }

    #[test]
    fn mix_stays_on_screen() {
        let w = ScreenWave { morph: 1.0, morph2: 1.0, level: 1.0, gain1: 0.71, gain2: 0.71, ..ScreenWave::default() };
        assert!((0..100).all(|i| w.mix(i as f32 / 50.0).abs() <= 1.0));
    }
}
