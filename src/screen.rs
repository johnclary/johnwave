//! What the oscillator screen draws: the oscillators of the newest voice, as shapes.
//!
//! The screen doesn't record any audio. The audio thread hands over a few numbers
//! (each oscillator's Shape, Osc 2's tuning, the mix, how loud the voice is) and the
//! window redraws the waves from them with the same `osc::shape` the sound uses.
//!
//! The OUT line is the one exception: `OutputCapture` keeps the last fraction of a
//! second of what the voices really played, so the screen can show it next to them.

use nice_plug::prelude::AtomicF32;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    /// The filter's cutoff (Hz) and resonance (0..1), after modulation, for the FREQ
    /// view's curve.
    pub cutoff_hz: f32,
    pub resonance: f32,
}

impl Default for ScreenWave {
    fn default() -> Self {
        Self {
            morph: 0.0,
            level: 0.0,
            morph2: 0.0,
            ratio2: 1.0,
            gain1: 1.0,
            gain2: 0.0,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
        }
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
            // Cutoff by how far it moved in octaves, since it's heard that way.
            || (self.cutoff_hz / other.cutoff_hz).log2().abs() > 0.005
            || (self.resonance - other.resonance).abs() > 0.002
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
    cutoff_hz: AtomicF32,
    resonance: AtomicF32,
    /// The host's sample rate, which the filter curve needs. Kept while silent too.
    sample_rate: AtomicF32,
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
            cutoff_hz: AtomicF32::new(w.cutoff_hz),
            resonance: AtomicF32::new(w.resonance),
            sample_rate: AtomicF32::new(48_000.0),
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
        self.cutoff_hz.store(w.cutoff_hz, Ordering::Relaxed);
        self.resonance.store(w.resonance, Ordering::Relaxed);
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
            cutoff_hz: self.cutoff_hz.load(Ordering::Relaxed),
            resonance: self.resonance.load(Ordering::Relaxed),
        })
    }

    /// Called by the audio thread when the sample rate is known.
    pub fn set_sample_rate(&self, sample_rate: f32) {
        self.sample_rate.store(sample_rate, Ordering::Relaxed);
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate.load(Ordering::Relaxed)
    }
}

/// How many samples of output the screen keeps: about 0.34 s at 48 kHz, enough for two
/// cycles of the lowest notes. A power of two, so wrapping round is cheap.
const CAPTURE_LEN: usize = 16_384;

/// The last `CAPTURE_LEN` samples of output, for the OUT line. The audio thread writes
/// one sample at a time round a ring (like a clock hand going round and overwriting the
/// oldest), and the window reads whichever stretch it needs. Every slot is its own atomic
/// number, so neither side waits for the other. A read that overlaps a write can catch
/// a sample or two mid-update, which can't be seen at 60 frames a second.
pub struct OutputCapture {
    samples: Box<[AtomicF32]>,
    /// How many samples have been written, ever. `written % CAPTURE_LEN` is where the
    /// next one goes.
    written: AtomicUsize,
    /// The value of `written` when osc 1's cycle was last marked, how many samples
    /// before that the cycle began, and how long osc 1's cycles are, in samples (with
    /// fractions, since a cycle rarely lasts a whole number of samples). The screen
    /// starts the OUT line where a cycle began, so it holds still instead of scrolling.
    marked_at: AtomicUsize,
    /// The value of `written` when capturing last started. Anything before it is old.
    started_at: AtomicUsize,
    cycle_began: AtomicF32,
    cycle_len: AtomicF32,
}

impl Default for OutputCapture {
    fn default() -> Self {
        Self {
            samples: (0..CAPTURE_LEN).map(|_| AtomicF32::new(0.0)).collect(),
            written: AtomicUsize::new(0),
            marked_at: AtomicUsize::new(0),
            started_at: AtomicUsize::new(0),
            cycle_began: AtomicF32::new(0.0),
            cycle_len: AtomicF32::new(0.0),
        }
    }
}

impl OutputCapture {
    /// Called by the audio thread when capturing starts again after a pause: what's in
    /// the ring is from before, so the window shouldn't use it.
    pub fn restart(&self) {
        let written = self.written.load(Ordering::Relaxed);
        self.started_at.store(written, Ordering::Relaxed);
        // No cycle marked yet, so the window shows nothing until there is one.
        self.cycle_len.store(0.0, Ordering::Relaxed);
    }

    /// Called by the audio thread for every sample, while the OUT line is showing.
    pub fn push(&self, sample: f32) {
        let n = self.written.load(Ordering::Relaxed);
        self.samples[n % CAPTURE_LEN].store(sample, Ordering::Relaxed);
        self.written.store(n.wrapping_add(1), Ordering::Release);
    }

    /// Called by the audio thread at the end of each block: where osc 1 of the newest
    /// voice is in its cycle now (0..1), and its frequency. Its current cycle began
    /// `phase` cycles ago.
    pub fn mark_cycle(&self, phase: f32, freq: f32, sample_rate: f32) {
        if freq <= 0.0 {
            return;
        }
        let len = sample_rate / freq;
        self.cycle_began.store(phase * len, Ordering::Relaxed);
        self.cycle_len.store(len, Ordering::Relaxed);
        self.marked_at.store(self.written.load(Ordering::Relaxed), Ordering::Release);
    }

    /// Called by the window: `cycles` cycles of osc 1 from the output, ending where the
    /// latest marked cycle began, as `points` evenly spaced values (in-between values
    /// are blended from their neighbours). `None` if the ring doesn't hold all of it.
    pub fn cycles(&self, cycles: f32, points: usize) -> Option<Vec<f32>> {
        let marked_at = self.marked_at.load(Ordering::Acquire);
        let written = self.written.load(Ordering::Acquire);
        let len = self.cycle_len.load(Ordering::Relaxed);
        if len <= 0.0 || points < 2 {
            return None;
        }
        // How far back from the newest sample the stretch ends and starts, in samples.
        // The newest sample is 1 back, the one before it 2, and so on.
        let end = written.wrapping_sub(marked_at) as f32 + self.cycle_began.load(Ordering::Relaxed);
        let start = end + cycles * len;
        // All of it must be written since capturing started, and not yet overwritten.
        let fresh = written.wrapping_sub(self.started_at.load(Ordering::Relaxed)).min(CAPTURE_LEN);
        if start + 1.0 >= fresh as f32 {
            return None;
        }
        // The value `back` samples before the newest one is written. With a fraction,
        // it's blended from the samples either side.
        let at = |back: f32| {
            let whole = back.floor();
            let frac = back - whole;
            let slot = |b: usize| self.samples[written.wrapping_sub(b) % CAPTURE_LEN].load(Ordering::Relaxed);
            let b = whole as usize;
            slot(b) * (1.0 - frac) + slot(b + 1) * frac
        };
        // Left to right is oldest to newest.
        Some((0..points).map(|k| at(start - (start - end) * k as f32 / (points - 1) as f32)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_screen_round_trips() {
        let shared = SharedScreen::default();
        assert_eq!(shared.load(), None);
        let w = ScreenWave {
            morph: 0.3,
            level: 0.8,
            morph2: 0.6,
            ratio2: 2.0,
            gain1: 0.7,
            gain2: 0.7,
            cutoff_hz: 800.0,
            resonance: 0.4,
        };
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
    fn capture_lines_up_with_the_cycle() {
        // A ramp from 0 to 1 every 100 samples, like a saw that starts each cycle at 0.
        let out = OutputCapture::default();
        let len = 100;
        for n in 0..1_050 {
            out.push((n % len) as f32 / len as f32);
        }
        // 1,050 samples in, the 11th cycle is half done.
        out.mark_cycle(0.5, 480.0, 48_000.0);
        let got = out.cycles(2.0, 201).expect("enough is written");
        // Two whole cycles, each starting at 0 and climbing.
        for (k, &y) in got.iter().enumerate().take(200) {
            let expected = (k % 100) as f32 / 100.0;
            assert!((y - expected).abs() < 1e-3, "point {k}: {y} vs {expected}");
        }
    }

    #[test]
    fn capture_drops_old_output_on_restart() {
        let out = OutputCapture::default();
        for _ in 0..1_000 {
            out.push(0.5);
        }
        out.restart();
        out.push(0.0);
        out.mark_cycle(0.0, 480.0, 48_000.0);
        // Plenty in the ring, but almost all of it from before the restart.
        assert_eq!(out.cycles(2.0, 10), None);
    }

    #[test]
    fn capture_waits_until_it_has_enough() {
        let out = OutputCapture::default();
        out.push(0.0);
        out.mark_cycle(0.0, 480.0, 48_000.0);
        assert_eq!(out.cycles(2.0, 10), None);
    }

    #[test]
    fn mix_stays_on_screen() {
        let w = ScreenWave { morph: 1.0, morph2: 1.0, level: 1.0, gain1: 0.71, gain2: 0.71, ..ScreenWave::default() };
        assert!((0..100).all(|i| w.mix(i as f32 / 50.0).abs() <= 1.0));
    }
}
