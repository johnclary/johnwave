//! Voices: one complete copy of the sound engine per note that is playing.
//!
//! A `Voice` holds everything that differs from note to note: pitch, velocity, the
//! oscillator's phase, filter memory, both envelopes and its LFO. A `VoicePool` holds a
//! fixed set of voices and decides which voice plays each new note.
//!
//! Signal path for one voice: oscillators 1 + 2 (blended by Osc Mix) → bitcrusher →
//! filter → volume envelope × velocity.

use crate::crush::{self, Crusher};
use crate::env::{Adsr, AdsrSettings};
use crate::filter::{Filter, FilterType};
use crate::lfo::{Lfo, LfoOut};
use crate::modulation::{self, ModSources, Routes};
use crate::osc::MorphOsc;
use nice_plug::prelude::{Smoother, SmoothingStyle};
use nice_plug::util;
use std::f32::consts::FRAC_PI_2;

pub const NUM_VOICES: usize = 8;

/// Knob values for the current sample, shared by every voice.
pub struct VoiceParams {
    pub morph: f32,
    /// The Osc 2 switch: 0 = off, 1 = on, fading in between.
    pub osc2_amount: f32,
    pub osc2_morph: f32,
    /// Osc 2's tuning relative to osc 1, in semitones (Osc 2 Coarse + Fine).
    pub osc2_pitch_st: f32,
    /// 0 = only osc 1, 1 = only osc 2.
    pub osc_mix: f32,
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
    /// The Crush switch: 0 = bypassed, 1 = on, fading in between.
    pub crush_amount: f32,
    /// Bitcrusher bit depth (`crush::MIN_BITS..=MAX_BITS`).
    pub crush_bits: f32,
    /// Bitcrusher stairs per wave cycle (`crush::MIN_STEPS..=MAX_STEPS`, top = off).
    pub crush_steps: f32,
    /// 0 = clean, 1 = fully crushed.
    pub crush_mix: f32,
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
    osc2: MorphOsc,
    /// State of a tiny random number generator, for osc 2's start phase.
    rng: u32,
    filter: Filter,
    crusher: Crusher,
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
            osc2: MorphOsc::default(),
            rng: 0x2545_F491, // any non-zero starting value works
            filter: Filter::default(),
            crusher: Crusher::default(),
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
        // A silent voice starts with an empty filter and crusher. A voice that's still
        // sounding keeps their memory, so the takeover doesn't click.
        if !self.is_active() {
            self.filter.reset();
            self.crusher.reset();
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
        // Osc 2 starts each note at a random point in its cycle. If it always started
        // in step with osc 1, the same tuning could sound full on one note and thin
        // (partly cancelled) on the next, depending on where osc 1 happened to be.
        let phase = self.random_phase(age);
        self.osc2.set_phase(phase);
    }

    /// A random number in 0..1. Mixing in `age` stops a chord's voices all picking the
    /// same number. Mono notes all have age 0, but the generator moves on every call,
    /// so they still differ.
    fn random_phase(&mut self, age: u64) -> f32 {
        let mut x = self.rng ^ (age as u32).wrapping_mul(0x9E37_79B9);
        if x == 0 {
            x = 1; // xorshift gets stuck at 0
        }
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        // The top 24 bits, as a fraction (an f32 holds 24 bits exactly).
        (x >> 8) as f32 / (1u32 << 24) as f32
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
        self.osc2.reset();
        self.filter.reset();
        self.crusher.reset();
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
        let osc1 = self.osc.next(freq, sample_rate, self.morph);

        // Osc 2 is tuned relative to osc 1, so bend, Coarse, Fine and Pitch routes move
        // both together. With the switch fully off it isn't run at all.
        let osc = if p.osc2_amount > 0.0 {
            let freq2 = freq * ((p.osc2_pitch_st + off.osc2_pitch_st) / 12.0).exp2();
            let morph2 = (p.osc2_morph + off.osc2_morph).clamp(0.0, 1.0);
            let osc2 = self.osc2.next(freq2, sample_rate, morph2);
            // Equal-power crossfade: detuned oscillators add like noise (by power), so a
            // plain 50/50 blend would dip about 3 dB in the middle. Fading the switch
            // off slides the mix back to pure osc 1.
            let mix = (p.osc_mix + off.osc_mix).clamp(0.0, 1.0) * p.osc2_amount;
            let angle = mix * FRAC_PI_2;
            osc1 * angle.cos() + osc2 * angle.sin()
        } else {
            osc1
        };

        // Crush straight after the oscillator, so the filter can smooth its harsh edges,
        // and before the volume envelope, so the grit stays the same as a note fades.
        // The stairs follow osc 1's `freq` (bend and pitch mod included) to stay in tune.
        // Steps are moved in octaves, like cutoff: −1 oct halves them. With the switch
        // off the crusher is skipped entirely, routes and all.
        let mixed = if p.crush_amount > 0.0 {
            let bits = (p.crush_bits + off.crush_bits).clamp(crush::MIN_BITS, crush::MAX_BITS);
            let steps = (p.crush_steps * off.crush_steps_oct.exp2())
                .clamp(crush::MIN_STEPS, crush::MAX_STEPS);
            let crush_mix = (p.crush_mix + off.crush_mix).clamp(0.0, 1.0) * p.crush_amount;
            let hold = crush::hold_length(steps, freq, sample_rate);
            let crushed = self.crusher.process(osc, bits, hold);
            osc + (crushed - osc) * crush_mix
        } else {
            osc
        };

        let cutoff = p.cutoff_hz * off.cutoff_oct.exp2();
        let resonance = p.resonance + off.resonance;
        let filtered = self.filter.process(mixed, p.filter_type, cutoff, resonance, sample_rate);

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
        // Osc 2 off, with its knobs where you'd find them on first switching it on.
        osc2_amount: 0.0,
        osc2_morph: 0.0,
        osc2_pitch_st: 0.07,
        osc_mix: 0.5,
        lfo_rate: 1.0,
        lfo_shape: 0.0,
        shared_lfo: None,
        pitch_st: 0.0,
        filter_type: FilterType::LowPass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        // Switched on, but with both knobs at their top ("Off"), so the tests start
        // from the clean sound and turn on only the crush they're testing.
        crush_amount: 1.0,
        crush_bits: crate::crush::MAX_BITS,
        crush_steps: crate::crush::MAX_STEPS,
        crush_mix: 1.0,
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

    fn render(p: &VoiceParams) -> Vec<f32> {
        let mut v = Voice::default();
        v.start(57, 1.0, 1, SR);
        (0..(0.1 * SR) as usize).map(|_| v.next(p, SR)).collect()
    }

    #[test]
    fn crusher_defaults_leave_the_sound_alone() {
        // Mix 0 skips the crusher entirely, so it's the true clean sound.
        let clean = render(&VoiceParams { crush_mix: 0.0, ..PARAMS });
        let default = render(&PARAMS);
        for (i, (a, b)) in clean.iter().zip(&default).enumerate() {
            assert!((a - b).abs() < 1e-4, "sample {i}: {a} vs {b}");
        }
    }

    #[test]
    fn crushing_adds_high_end_and_mix_blends_it() {
        // A sine is smooth, so its sample-to-sample changes are small. Steps from the
        // crusher make big jumps, which `brightness` picks up.
        let sine = VoiceParams { morph: 0.0, ..PARAMS };
        let clean = brightness(&sine);
        let bits = brightness(&VoiceParams { crush_bits: 3.0, ..sine });
        let rate = brightness(&VoiceParams { crush_steps: 8.0, ..sine });
        let half = brightness(&VoiceParams { crush_bits: 3.0, crush_mix: 0.5, ..sine });
        assert!(bits > clean * 2.0, "bits {bits} vs clean {clean}");
        assert!(rate > clean * 2.0, "rate {rate} vs clean {clean}");
        assert!(half > clean && half < bits, "half {half}, clean {clean}, full {bits}");
    }

    #[test]
    fn modulation_can_bring_in_the_crusher_from_off() {
        // Both crush knobs at Off. A full-velocity note with a velocity route pushing
        // Bits or Steps down should sound rougher than with no route.
        let sine = VoiceParams { morph: 0.0, ..PARAMS };
        let clean = brightness(&sine);
        let routed = |dest: ModDest, amount: f32| {
            let mut routes = Routes::EMPTY;
            routes.add(ModSource::Velocity, dest, amount);
            brightness(&VoiceParams { routes, ..sine })
        };
        let bits = routed(ModDest::CrushBits, -13.0); // 16 → 3 bits
        let steps = routed(ModDest::CrushSteps, -5.0); // 256 → 8 steps
        assert!(bits > clean * 2.0, "bits {bits} vs clean {clean}");
        assert!(steps > clean * 2.0, "steps {steps} vs clean {clean}");
        // Pushing Mix to 0 turns a fully crushed sound clean again.
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Velocity, ModDest::CrushMix, -1.0);
        let unmixed = brightness(&VoiceParams { crush_bits: 3.0, routes, ..sine });
        assert!((unmixed - clean).abs() < clean * 0.01, "unmixed {unmixed} vs clean {clean}");
    }

    #[test]
    fn switched_off_crusher_is_bypassed_even_with_routes() {
        let sine = VoiceParams { morph: 0.0, ..PARAMS };
        let clean = render(&VoiceParams { crush_mix: 0.0, ..sine }); // exactly the oscillator
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Velocity, ModDest::CrushBits, -13.0);
        let off = VoiceParams { crush_amount: 0.0, crush_bits: 3.0, crush_steps: 4.0, routes, ..sine };
        assert_eq!(render(&off), clean);
    }

    /// Osc 2 on at full Mix, so only osc 2 is heard. Both oscillators are sines.
    const OSC2_ONLY: VoiceParams = VoiceParams {
        morph: 0.0,
        osc2_amount: 1.0,
        osc2_morph: 0.0,
        osc2_pitch_st: 0.0,
        osc_mix: 1.0,
        ..PARAMS
    };

    #[test]
    fn osc2_switched_off_changes_nothing() {
        let plain = render(&PARAMS);
        // Osc 2's knobs and routes do nothing while it's off...
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Velocity, ModDest::OscMix, 1.0);
        routes.add(ModSource::Velocity, ModDest::Osc2Pitch, 7.0);
        let off = VoiceParams { osc2_morph: 1.0, osc_mix: 1.0, routes, ..PARAMS };
        assert_eq!(render(&off), plain);
        // ...and switched on at Mix 0 it's exactly osc 1 too.
        let mix_zero = VoiceParams { osc2_amount: 1.0, osc_mix: 0.0, ..PARAMS };
        assert_eq!(render(&mix_zero), plain);
    }

    #[test]
    fn osc2_is_tuned_relative_to_osc1() {
        let near = |f: f32, hz: f32| (f - hz).abs() <= 2.0;
        let up = measured_freq(&VoiceParams { osc2_pitch_st: 12.0, ..OSC2_ONLY });
        assert!(near(up, 440.0), "{up}");
        let down = measured_freq(&VoiceParams { osc2_pitch_st: -36.0, ..OSC2_ONLY });
        assert!(near(down, 27.5), "{down}");
        // Shared pitch (bend, Coarse) moves osc 2 along with osc 1.
        let shared = measured_freq(&VoiceParams { pitch_st: 12.0, ..OSC2_ONLY });
        assert!(near(shared, 440.0), "{shared}");
        // A Pitch 2 route adds on top of the knob.
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Velocity, ModDest::Osc2Pitch, 5.0);
        let routed = measured_freq(&VoiceParams { osc2_pitch_st: 7.0, routes, ..OSC2_ONLY });
        assert!(near(routed, 440.0), "{routed}");
    }

    #[test]
    fn osc2_starts_each_note_somewhere_new() {
        // The same voice, retriggered (as in mono, where every note has age 0).
        let mut v = Voice::default();
        let mut phases = Vec::new();
        for _ in 0..4 {
            v.start(60, 1.0, 0, SR);
            phases.push(v.osc2.phase());
        }
        // A chord: fresh voices, which all start with the same generator state.
        let mut pool = VoicePool::default();
        for n in [60, 64, 67] {
            on(&mut pool, n);
        }
        phases.extend(pool.voices[..3].iter().map(|v| v.osc2.phase()));
        for (i, a) in phases.iter().enumerate() {
            assert!((0.0..1.0).contains(a));
            for b in &phases[i + 1..] {
                assert!((a - b).abs() > 1e-3, "repeated phase {a} in {phases:?}");
            }
        }
    }

    #[test]
    fn mix_keeps_the_level_even() {
        // Osc 2 a semitone up, so the two drift in and out of phase like a real detune.
        // Level is measured once the envelope has settled, over enough beats to average.
        let rms_at = |mix: f32| {
            let p = VoiceParams { osc2_pitch_st: 1.0, osc_mix: mix, ..OSC2_ONLY };
            let mut v = Voice::default();
            v.start(57, 1.0, 1, SR);
            let out: Vec<f32> = (0..(1.5 * SR) as usize).map(|_| v.next(&p, SR)).collect();
            let tail = &out[(0.5 * SR) as usize..];
            (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let solo = rms_at(0.0);
        for mix in [0.25, 0.5, 0.75, 1.0] {
            let level = rms_at(mix);
            assert!((level - solo).abs() / solo < 0.05, "mix {mix}: {level} vs {solo}");
        }
    }
}
