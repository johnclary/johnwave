//! Voices: one complete copy of the sound engine per note that is playing.
//!
//! A `Voice` holds everything that differs from note to note: pitch, velocity, the
//! oscillator's phase, both envelopes and its LFO. A `VoicePool` holds a fixed set of
//! voices and decides which voice plays each new note.

use crate::env::{Adsr, AdsrSettings};
use crate::lfo::Lfo;
use crate::osc::MorphOsc;
use nice_plug::prelude::{Smoother, SmoothingStyle};
use nice_plug::util;

pub const NUM_VOICES: usize = 8;

/// Knob values for the current sample, shared by every voice.
pub struct VoiceParams {
    pub morph: f32,
    pub lfo_rate: f32,
    pub lfo_depth: f32,
    pub morph_env_amount: f32,
    pub amp_env: AdsrSettings,
    pub morph_env: AdsrSettings,
}

pub struct Voice {
    /// MIDI key this voice is playing (or last played).
    note: u8,
    /// True while the key is physically held down.
    held: bool,
    /// When this voice was started. Higher means newer. Used to pick a voice to steal.
    age: u64,
    freq: f32,
    velocity: Smoother<f32>,
    osc: MorphOsc,
    amp_env: Adsr,
    morph_env: Adsr,
    lfo: Lfo,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            note: 0,
            held: false,
            age: 0,
            freq: 440.0,
            velocity: Smoother::new(SmoothingStyle::Linear(5.0)),
            osc: MorphOsc::default(),
            amp_env: Adsr::default(),
            morph_env: Adsr::default(),
            lfo: Lfo::default(),
        }
    }
}

impl Voice {
    /// Still making sound (held, or ringing out in its release).
    pub fn is_active(&self) -> bool {
        !self.amp_env.is_idle()
    }

    /// Begin a note. `lfo` is the LFO state to start from: a fresh one to retrigger,
    /// or a copy of the free-running LFO to stay in step with it.
    pub fn start(&mut self, note: u8, velocity: f32, age: u64, lfo: Lfo, sample_rate: f32) {
        self.glide_to(note);
        self.held = true;
        self.age = age;
        self.velocity.set_target(sample_rate, velocity);
        self.amp_env.trigger();
        self.morph_env.trigger();
        self.lfo = lfo;
    }

    /// Change pitch without retriggering anything (mono legato).
    pub fn glide_to(&mut self, note: u8) {
        self.note = note;
        self.freq = util::midi_key_to_freq(note);
    }

    pub fn release(&mut self) {
        self.held = false;
        self.amp_env.release();
        self.morph_env.release();
    }

    pub fn reset(&mut self) {
        self.held = false;
        self.osc.reset();
        self.amp_env.reset();
        self.morph_env.reset();
        self.lfo.reset();
        self.velocity.reset(0.0);
    }

    /// One sample of this voice's output.
    pub fn next(&mut self, p: &VoiceParams, sample_rate: f32) -> f32 {
        if !self.is_active() {
            return 0.0;
        }
        // Morph = knob + LFO swing + envelope push. The oscillator clamps it to 0..=1.
        let lfo = self.lfo.next(p.lfo_rate, sample_rate);
        let morph_env = self.morph_env.next(&p.morph_env, sample_rate);
        let morph = p.morph + lfo * p.lfo_depth + morph_env * p.morph_env_amount;

        let osc = self.osc.next(self.freq, sample_rate, morph);
        let amp = self.amp_env.next(&p.amp_env, sample_rate);
        osc * amp * self.velocity.next()
    }
}

#[derive(Default)]
pub struct VoicePool {
    voices: [Voice; NUM_VOICES],
    /// Counts note starts, so each voice's `age` says how recent it is.
    started: u64,
}

impl VoicePool {
    /// Play a note on a free voice, taking one over if all 8 are busy.
    pub fn note_on(&mut self, note: u8, velocity: f32, lfo: Lfo, sample_rate: f32) {
        self.started += 1;
        let i = self.pick_voice(note);
        self.voices[i].start(note, velocity, self.started, lfo, sample_rate);
    }

    /// Which voice should play `note`, in order of preference:
    /// 1. a voice already playing this same key (so it isn't doubled up),
    /// 2. a silent voice,
    /// 3. the oldest voice that's already ringing out after release,
    /// 4. the oldest held voice.
    fn pick_voice(&self, note: u8) -> usize {
        let v = &self.voices;
        let oldest = |pred: &dyn Fn(&Voice) -> bool| {
            (0..NUM_VOICES)
                .filter(|&i| pred(&v[i]))
                .min_by_key(|&i| v[i].age)
        };
        oldest(&|x| x.is_active() && x.note == note)
            .or_else(|| oldest(&|x| !x.is_active()))
            .or_else(|| oldest(&|x| !x.held))
            .or_else(|| oldest(&|_| true))
            .unwrap()
    }

    pub fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut().filter(|v| v.held && v.note == note) {
            v.release();
        }
    }

    pub fn release_all(&mut self) {
        for v in self.voices.iter_mut().filter(|v| v.held) {
            v.release();
        }
    }

    /// Silence immediately (host reset, or a "choke" from the DAW).
    pub fn reset(&mut self) {
        self.voices.iter_mut().for_each(Voice::reset);
    }

    /// The single voice used in mono mode.
    pub fn mono(&mut self) -> &mut Voice {
        &mut self.voices[0]
    }

    /// The sum of all voices for one sample.
    pub fn next(&mut self, p: &VoiceParams, sample_rate: f32) -> f32 {
        self.voices.iter_mut().map(|v| v.next(p, sample_rate)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    const PARAMS: VoiceParams = VoiceParams {
        morph: 0.5,
        lfo_rate: 1.0,
        lfo_depth: 0.0,
        morph_env_amount: 0.0,
        amp_env: AdsrSettings {
            attack_s: 0.005,
            decay_s: 0.1,
            sustain: 0.7,
            release_s: 0.05,
        },
        morph_env: AdsrSettings {
            attack_s: 0.001,
            decay_s: 0.5,
            sustain: 0.0,
            release_s: 0.5,
        },
    };

    fn on(pool: &mut VoicePool, note: u8) {
        pool.note_on(note, 1.0, Lfo::default(), SR);
    }

    fn run(pool: &mut VoicePool, seconds: f32) -> Vec<f32> {
        (0..(seconds * SR) as usize).map(|_| pool.next(&PARAMS, SR)).collect()
    }

    /// The keys of all voices currently making sound, sorted.
    fn sounding(pool: &VoicePool) -> Vec<u8> {
        let mut notes: Vec<u8> = pool
            .voices
            .iter()
            .filter(|v| v.is_active())
            .map(|v| v.note)
            .collect();
        notes.sort();
        notes
    }

    fn held(pool: &VoicePool) -> Vec<u8> {
        let mut notes: Vec<u8> = pool.voices.iter().filter(|v| v.held).map(|v| v.note).collect();
        notes.sort();
        notes
    }

    #[test]
    fn chord_uses_separate_voices() {
        let mut pool = VoicePool::default();
        for n in [60, 64, 67] {
            on(&mut pool, n);
        }
        assert_eq!(sounding(&pool), vec![60, 64, 67]);
    }

    #[test]
    fn note_off_releases_only_that_key() {
        let mut pool = VoicePool::default();
        on(&mut pool, 60);
        on(&mut pool, 64);
        pool.note_off(60);
        assert_eq!(held(&pool), vec![64]);
        // After the 50 ms release, 60 has gone silent and 64 still plays.
        run(&mut pool, 0.5);
        assert_eq!(sounding(&pool), vec![64]);
    }

    #[test]
    fn repeating_a_key_reuses_its_voice() {
        let mut pool = VoicePool::default();
        on(&mut pool, 60);
        pool.note_off(60);
        on(&mut pool, 60); // pressed again while still ringing out
        assert_eq!(sounding(&pool), vec![60]);
    }

    #[test]
    fn steals_the_oldest_held_voice_when_full() {
        let mut pool = VoicePool::default();
        for n in 60..(60 + NUM_VOICES as u8) {
            on(&mut pool, n);
        }
        on(&mut pool, 100);
        // 60 was the oldest, so it was replaced.
        let expected: Vec<u8> = (61..(60 + NUM_VOICES as u8)).chain([100]).collect();
        assert_eq!(held(&pool), expected);
    }

    #[test]
    fn prefers_stealing_a_released_voice() {
        let mut pool = VoicePool::default();
        for n in 60..(60 + NUM_VOICES as u8) {
            on(&mut pool, n);
        }
        pool.note_off(63); // still ringing out, so not idle yet
        on(&mut pool, 100);
        // 63 was taken over instead of the older 60, and every held key survives.
        let expected: Vec<u8> = (60..(60 + NUM_VOICES as u8))
            .filter(|&n| n != 63)
            .chain([100])
            .collect();
        assert_eq!(held(&pool), expected);
        assert!(!sounding(&pool).contains(&63));
    }

    #[test]
    fn goes_silent_after_everything_is_released() {
        let mut pool = VoicePool::default();
        for n in [48, 55, 60, 64] {
            on(&mut pool, n);
        }
        run(&mut pool, 0.2);
        pool.release_all();
        run(&mut pool, 1.0);
        assert!(sounding(&pool).is_empty());
        assert!(run(&mut pool, 0.01).iter().all(|&x| x == 0.0));
    }
}
