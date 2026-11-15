//! Voices: one complete copy of the sound engine per note that is playing.
//!
//! A `Voice` holds everything that differs from note to note: pitch, velocity, the
//! oscillator's phase, filter memory, both envelopes and its LFO. A `VoicePool` holds a
//! fixed set of voices and decides which voice plays each new note.
//!
//! Signal path for one voice: oscillator → low-pass filter → volume envelope × velocity.

use crate::env::{Adsr, AdsrSettings};
use crate::filter::{Filter, FilterType};
use crate::lfo::{Lfo, LfoOut};
use crate::modulation::{self, ModSources, Routes};
use crate::osc::MorphOsc;
use nice_plug::prelude::{Smoother, SmoothingStyle};
use nice_plug::util;

pub const NUM_VOICES: usize = 8;

/// Knob values for the current sample, shared by every voice.
pub struct VoiceParams {
    pub morph: f32,
    pub lfo_rate: f32,
    /// 0 = sine, ⅓ = triangle, ⅔ = saw, 1 = square.
    pub lfo_shape: f32,
    /// With LFO Retrigger off: the shared LFO's output this sample, used by every voice
    /// so they all move together. With retrigger on: `None`, and each voice runs its own.
    pub shared_lfo: Option<LfoOut>,
    /// Pitch shift for every voice in semitones (bend wheel + Coarse + Fine).
    pub pitch_st: f32,
    pub filter_type: FilterType,
    pub cutoff_hz: f32,
    pub resonance: f32,
    /// Which modulation source moves which setting (see `modulation.rs`).
    pub routes: Routes,
    pub amp_env: AdsrSettings,
    pub mod_env: AdsrSettings,
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
    filter: Filter,
    amp_env: Adsr,
    mod_env: Adsr,
    lfo: Lfo,
    /// Morph after modulation, from the latest sample. Read by the plugin window.
    morph: f32,
    /// How loud this voice is right now (amp envelope times velocity, 0..1), from the
    /// latest sample. Read by the plugin window.
    level: f32,
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
            filter: Filter::default(),
            amp_env: Adsr::default(),
            mod_env: Adsr::default(),
            lfo: Lfo::default(),
            level: 0.0,
            morph: 0.0,
        }
    }
}

impl Voice {
    /// Still making sound (held, or ringing out in its release).
    pub fn is_active(&self) -> bool {
        !self.amp_env.is_idle()
    }

    /// Begin a note. The voice's own LFO restarts (used when LFO Retrigger is on).
    pub fn start(&mut self, note: u8, velocity: f32, age: u64, sample_rate: f32) {
        // A silent voice starts with an empty filter. A voice that's still sounding keeps
        // its filter memory, so the takeover doesn't click.
        if !self.is_active() {
            self.filter.reset();
        }
        self.glide_to(note);
        self.held = true;
        self.age = age;
        self.velocity.set_target(sample_rate, velocity);
        self.amp_env.trigger();
        self.mod_env.trigger();
        // Seeding from `age` (different for every note) gives each voice its own random
        // levels, so a chord's voices don't all jump to the same place.
        self.lfo.seed(age);
        self.lfo.reset();
    }

    /// Change pitch without retriggering anything (mono legato).
    pub fn glide_to(&mut self, note: u8) {
        self.note = note;
        self.freq = util::midi_key_to_freq(note);
    }

    pub fn release(&mut self) {
        self.held = false;
        self.amp_env.release();
        self.mod_env.release();
    }

    pub fn reset(&mut self) {
        self.held = false;
        self.osc.reset();
        self.filter.reset();
        self.amp_env.reset();
        self.mod_env.reset();
        self.lfo.reset();
        self.velocity.reset(0.0);
    }

    /// One sample of this voice's output.
    pub fn next(&mut self, p: &VoiceParams, sample_rate: f32) -> f32 {
        if !self.is_active() {
            return 0.0;
        }
        // 1. This voice's modulation sources for this sample.
        let own_lfo = self.lfo.next(p.lfo_rate, sample_rate, p.lfo_shape);
        let lfo = p.shared_lfo.unwrap_or(own_lfo);
        let velocity = self.velocity.next();
        let sources = ModSources {
            lfo: lfo.wave,
            mod_env: self.mod_env.next(&p.mod_env, sample_rate),
            velocity,
            random: lfo.random,
        };

        // 2. Run them through the routes to get how far each setting is pushed.
        let off = modulation::apply(&p.routes, &sources);

        // 3. Knob value + offset. Pitch and cutoff work in semitones and octaves, then
        // turn into a frequency multiplier: +12 st or +1 oct doubles, −1 oct halves.
        // The oscillator clamps morph to 0..=1 and the filter clamps resonance.
        let freq = self.freq * ((p.pitch_st + off.pitch_st) / 12.0).exp2();
        self.morph = (p.morph + off.morph).clamp(0.0, 1.0);
        let osc = self.osc.next(freq, sample_rate, self.morph);

        let cutoff = p.cutoff_hz * off.cutoff_oct.exp2();
        let resonance = p.resonance + off.resonance;
        let filtered = self.filter.process(osc, p.filter_type, cutoff, resonance, sample_rate);

        let amp = self.amp_env.next(&p.amp_env, sample_rate);
        self.level = amp * velocity;
        filtered * self.level
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
    pub fn note_on(&mut self, note: u8, velocity: f32, sample_rate: f32) {
        self.started += 1;
        let i = self.pick_voice(note);
        self.voices[i].start(note, velocity, self.started, sample_rate);
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

    /// The modulated Morph and the level of the most recently started voice that's still
    /// sounding, as `(morph, level)`, or `None` when nothing is playing.
    pub fn newest_display(&self) -> Option<(f32, f32)> {
        self.voices
            .iter()
            .filter(|v| v.is_active())
            .max_by_key(|v| v.age)
            .map(|v| (v.morph, v.level))
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
    use crate::modulation::{ModDest, ModSource};

    const SR: f32 = 48_000.0;
    const PARAMS: VoiceParams = VoiceParams {
        morph: 0.5,
        lfo_rate: 1.0,
        lfo_shape: 0.0,
        shared_lfo: None,
        pitch_st: 0.0,
        filter_type: FilterType::LowPass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        routes: Routes::EMPTY,
        amp_env: AdsrSettings {
            attack_s: 0.005,
            decay_s: 0.1,
            sustain: 0.7,
            release_s: 0.05,
        },
        mod_env: AdsrSettings {
            attack_s: 0.001,
            decay_s: 0.5,
            sustain: 0.0,
            release_s: 0.5,
        },
    };

    fn on(pool: &mut VoicePool, note: u8) {
        pool.note_on(note, 1.0, SR);
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

    /// How bright a voice sounds: the RMS of the sample-to-sample change, which grows with
    /// high-frequency content.
    fn brightness(p: &VoiceParams) -> f32 {
        let mut v = Voice::default();
        v.start(45, 1.0, 1, SR);
        let out: Vec<f32> = (0..(0.3 * SR) as usize).map(|_| v.next(p, SR)).collect();
        let diffs: Vec<f32> = out.windows(2).map(|w| w[1] - w[0]).collect();
        (diffs.iter().map(|d| d * d).sum::<f32>() / diffs.len() as f32).sqrt()
    }

    #[test]
    fn lfo_moves_the_cutoff_both_ways() {
        // A square through a 300 Hz filter, with the shared LFO held at its peak (+1).
        let with = |octaves: f32| {
            let mut routes = Routes::EMPTY;
            routes.add(ModSource::Lfo, ModDest::Cutoff, octaves);
            VoiceParams {
                morph: 1.0,
                cutoff_hz: 300.0,
                shared_lfo: Some(LfoOut { wave: 1.0, random: 0.0 }),
                routes,
                ..PARAMS
            }
        };
        let opened = brightness(&with(4.0)); // cutoff pushed up to 4.8 kHz
        let plain = brightness(&with(0.0));
        let closed = brightness(&with(-4.0)); // pushed down to ~19 Hz
        assert!(opened > plain * 2.0, "opened {opened} vs plain {plain}");
        assert!(closed < plain * 0.5, "closed {closed} vs plain {plain}");
    }

    /// Pitch of a sine voice playing A3 (220 Hz), counted from upward zero crossings
    /// over one second.
    fn measured_freq(p: &VoiceParams) -> f32 {
        let mut v = Voice::default();
        v.start(57, 1.0, 1, SR);
        let out: Vec<f32> = (0..SR as usize).map(|_| v.next(p, SR)).collect();
        out.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32
    }

    #[test]
    fn pitch_offsets_shift_by_semitones() {
        let sine = VoiceParams { morph: 0.0, ..PARAMS };
        let at = |shared: f32, routed: f32| {
            // A route from velocity (1.0 here) adds `routed` semitones on top of the
            // shared bend/tuning offset.
            let mut routes = Routes::EMPTY;
            routes.add(ModSource::Velocity, ModDest::Pitch, routed);
            measured_freq(&VoiceParams { pitch_st: shared, routes, ..sine })
        };
        let near = |f: f32, hz: f32| (f - hz).abs() <= 2.0;
        assert!(near(at(0.0, 0.0), 220.0), "{}", at(0.0, 0.0));
        assert!(near(at(12.0, 0.0), 440.0), "{}", at(12.0, 0.0));
        assert!(near(at(-12.0, 0.0), 110.0), "{}", at(-12.0, 0.0));
        assert!(near(at(5.0, 7.0), 440.0), "{}", at(5.0, 7.0)); // the two add up
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
