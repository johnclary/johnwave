use nice_plug::prelude::*;
use std::sync::Arc;

mod env;
mod filter;
mod lfo;
mod notes;
mod osc;
mod voice;
use env::AdsrSettings;
use lfo::{Lfo, SyncDivision};
use notes::NoteStack;
use voice::{VoiceParams, VoicePool};

/// Wave-morphing synth: 8-voice polyphonic, or mono with legato.
pub struct Johnwave {
    params: Arc<SynthParams>,
    sample_rate: f32,

    voices: VoicePool,
    /// Held keys, most recent last. Only used in mono mode (last-note priority).
    notes: NoteStack,
    /// An LFO that always runs. With retrigger off, every voice follows it, so they all
    /// wobble in step (and, when synced, in time with the song).
    free_lfo: Lfo,
    /// The mode in effect last sample, to detect when the Voice Mode switch changes.
    mode: VoiceMode,
}

#[derive(Enum, PartialEq, Clone, Copy)]
enum VoiceMode {
    #[id = "poly"]
    Poly,
    #[id = "mono"]
    #[name = "Mono (legato)"]
    Mono,
}

#[derive(Params)]
struct SynthParams {
    #[id = "voice_mode"]
    pub voice_mode: EnumParam<VoiceMode>,

    /// 0 = sine, 33% = triangle, 67% = saw, 100% = square.
    #[id = "morph"]
    pub morph: FloatParam,

    /// LFO speed in Hz.
    #[id = "lfo_rate"]
    pub lfo_rate: FloatParam,
    /// How far the LFO swings Morph. At 100% it moves ±100%, i.e. the whole range.
    #[id = "lfo_depth"]
    pub lfo_depth: FloatParam,
    /// Restart the LFO cycle on each fresh (non-legato) note.
    #[id = "lfo_retrig"]
    pub lfo_retrigger: BoolParam,
    /// Follow the DAW's tempo instead of the LFO Rate knob.
    #[id = "lfo_sync"]
    pub lfo_sync: BoolParam,
    /// Cycle length when synced.
    #[id = "lfo_div"]
    pub lfo_division: EnumParam<SyncDivision>,

    /// Low-pass filter cutoff in Hz.
    #[id = "cutoff"]
    pub cutoff: FloatParam,
    /// Peak at the cutoff: 0 = none, 1 = strong ringing (stops short of self-oscillation).
    #[id = "resonance"]
    pub resonance: FloatParam,

    // The Mod Env is an attack/decay envelope that drives both Morph and Cutoff.
    // (The "menv_" ids date from when it only drove Morph. Changing an id would
    // break saved presets, so they stay.)
    /// How far the Mod Env pushes Morph at its peak. Negative pushes toward sine.
    #[id = "menv_amt"]
    pub mod_env_to_morph: FloatParam,
    /// How many octaves the Mod Env moves the cutoff at its peak. Negative closes it.
    #[id = "menv_cut"]
    pub mod_env_to_cutoff: FloatParam,
    #[id = "menv_att"]
    pub mod_env_attack: FloatParam,
    #[id = "menv_dec"]
    pub mod_env_decay: FloatParam,

    #[id = "attack"]
    pub attack: FloatParam,
    #[id = "decay"]
    pub decay: FloatParam,
    #[id = "sustain"]
    pub sustain: FloatParam,
    #[id = "release"]
    pub release: FloatParam,

    #[id = "gain"]
    pub gain: FloatParam,
}

/// An envelope time knob in milliseconds. It's skewed so the short times get more
/// of the knob's travel.
fn time_param(name: &str, default_ms: f32, max_ms: f32) -> FloatParam {
    FloatParam::new(
        name,
        default_ms,
        FloatRange::Skewed {
            min: 1.0,
            max: max_ms,
            factor: FloatRange::skew_factor(-2.0),
        },
    )
    .with_value_to_string(Arc::new(|ms| {
        if ms < 1000.0 {
            format!("{ms:.0} ms")
        } else {
            format!("{:.2} s", ms / 1000.0)
        }
    }))
    .with_string_to_value(Arc::new(|s| {
        let s = s.trim().to_lowercase();
        if let Some(n) = s.strip_suffix("ms") {
            n.trim().parse().ok()
        } else if let Some(n) = s.strip_suffix('s') {
            n.trim().parse::<f32>().ok().map(|secs| secs * 1000.0)
        } else {
            s.parse().ok()
        }
    }))
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            voice_mode: EnumParam::new("Voice Mode", VoiceMode::Poly),
            morph: FloatParam::new("Morph", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            lfo_rate: FloatParam::new(
                "LFO Rate",
                1.0,
                FloatRange::Skewed {
                    min: 0.05,
                    max: 20.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            lfo_depth: FloatParam::new("LFO Depth", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            lfo_retrigger: BoolParam::new("LFO Retrigger", true),
            lfo_sync: BoolParam::new("LFO Sync", false),
            lfo_division: EnumParam::new("LFO Sync Rate", SyncDivision::Quarter),

            cutoff: FloatParam::new(
                "Cutoff",
                20_000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20_000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_value_to_string(formatters::v2s_f32_hz_then_khz(0))
            .with_string_to_value(formatters::s2v_f32_hz_then_khz()),
            resonance: FloatParam::new("Resonance", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            mod_env_to_cutoff: FloatParam::new(
                "Mod Env to Cutoff",
                0.0,
                FloatRange::Linear { min: -6.0, max: 6.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" oct")
            .with_value_to_string(Arc::new(|oct| format!("{oct:+.1}"))),
            mod_env_to_morph: FloatParam::new(
                "Mod Env to Morph",
                0.0,
                FloatRange::Linear { min: -1.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            mod_env_attack: time_param("Mod Env Attack", 1.0, 5_000.0),
            mod_env_decay: time_param("Mod Env Decay", 500.0, 10_000.0),

            attack: time_param("Attack", 5.0, 5_000.0),
            decay: time_param("Decay", 300.0, 5_000.0),
            sustain: FloatParam::new("Sustain", 0.7, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            release: time_param("Release", 200.0, 10_000.0),

            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(-12.0),
                FloatRange::gain_range(-48.0, 0.0),
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
        }
    }
}

impl Default for Johnwave {
    fn default() -> Self {
        Self {
            params: Arc::new(SynthParams::default()),
            sample_rate: 44_100.0,
            voices: VoicePool::default(),
            notes: NoteStack::default(),
            free_lfo: Lfo::default(),
            mode: VoiceMode::Poly,
        }
    }
}

impl Johnwave {
    fn handle_event(&mut self, event: NoteEvent<()>) {
        match event {
            NoteEvent::NoteOn { key, velocity, .. } => {
                let note = key.number_or_middle_c();
                match self.mode {
                    VoiceMode::Poly => self.voices.note_on(note, velocity, self.sample_rate),
                    VoiceMode::Mono => {
                        let legato = self.notes.top().is_some();
                        self.notes.push(note);
                        // Another key still held: change pitch only. Fresh press: retrigger.
                        if legato {
                            self.voices.mono().glide_to(note);
                        } else {
                            self.voices
                                .mono()
                                .start(note, velocity, 0, self.sample_rate);
                        }
                    }
                }
            }
            NoteEvent::NoteOff { key, .. } => {
                let note = key.number_or_middle_c();
                match self.mode {
                    VoiceMode::Poly => self.voices.note_off(note),
                    VoiceMode::Mono => {
                        self.notes.remove(note);
                        match self.notes.top() {
                            // Fall back to the previously held key without retriggering.
                            Some(prev) => self.voices.mono().glide_to(prev),
                            None => self.voices.mono().release(),
                        }
                    }
                }
            }
            // The DAW wants these notes cut off immediately (e.g. a choke group).
            NoteEvent::Choke { .. } => {
                self.voices.reset();
                self.notes.clear();
            }
            _ => (),
        }
    }

    /// The LFO speed for this block: the Rate knob, or the note value at the DAW's
    /// tempo when synced. When synced, the playing song and Retrigger off, the shared LFO
    /// is also lined up with the song position, so its cycle always lands on the beat.
    fn update_lfo_rate(&mut self, transport: &Transport) -> f32 {
        if !self.params.lfo_sync.value() {
            return self.params.lfo_rate.value();
        }
        let division = self.params.lfo_division.value();
        let bpm = transport.tempo.unwrap_or(120.0);
        // Length of a bar in quarter notes: 4 in 4/4, 3 in 3/4, 3.5 in 7/8...
        let bar_beats = match (transport.time_sig_numerator, transport.time_sig_denominator) {
            (Some(num), Some(den)) if num > 0 && den > 0 => num as f64 * 4.0 / den as f64,
            _ => 4.0,
        };
        if transport.playing && !self.params.lfo_retrigger.value() {
            if let Some(pos) = transport.pos_beats() {
                self.free_lfo.set_phase(division.phase_at(pos, bar_beats));
            }
        }
        division.rate_hz(bpm, bar_beats)
    }

    fn adsr_settings(&self) -> AdsrSettings {
        AdsrSettings {
            attack_s: self.params.attack.value() / 1000.0,
            decay_s: self.params.decay.value() / 1000.0,
            sustain: self.params.sustain.value(),
            release_s: self.params.release.value() / 1000.0,
        }
    }

    /// Attack, then decay to 0 and stay there. Releasing the key mid-decay carries on at
    /// the same speed, because the release time is set equal to the decay time.
    fn mod_env_settings(&self) -> AdsrSettings {
        let decay_s = self.params.mod_env_decay.value() / 1000.0;
        AdsrSettings {
            attack_s: self.params.mod_env_attack.value() / 1000.0,
            decay_s,
            sustain: 0.0,
            release_s: decay_s,
        }
    }
}

impl Plugin for Johnwave {
    const NAME: &'static str = "johnwave";
    const VENDOR: &'static str = "john";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type Editor = ();
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn activate(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        true
    }

    fn reset(&mut self) {
        self.voices.reset();
        self.notes.clear();
        self.free_lfo.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let lfo_rate = self.update_lfo_rate(context.transport());
        let shared_lfo_on = !self.params.lfo_retrigger.value();
        let mut next_event = context.next_event();

        // Switching between Poly and Mono releases whatever was playing, so no voice
        // gets stuck on.
        let mode = self.params.voice_mode.value();
        if mode != self.mode {
            self.voices.release_all();
            self.notes.clear();
            self.mode = mode;
        }

        for (sample_id, channel_samples) in buffer.iter_samples().enumerate() {
            // Apply all events scheduled at or before this sample.
            while let Some(event) = next_event {
                if event.timing() > sample_id as u32 {
                    break;
                }
                self.handle_event(event);
                next_event = context.next_event();
            }

            // Read every knob once per sample, then share the values with all voices.
            let shared = self.free_lfo.next(lfo_rate, self.sample_rate);
            let p = VoiceParams {
                morph: self.params.morph.smoothed.next(),
                lfo_rate,
                shared_lfo: shared_lfo_on.then_some(shared),
                lfo_depth: self.params.lfo_depth.smoothed.next(),
                mod_env_to_morph: self.params.mod_env_to_morph.smoothed.next(),
                cutoff_hz: self.params.cutoff.smoothed.next(),
                resonance: self.params.resonance.smoothed.next(),
                mod_env_to_cutoff_oct: self.params.mod_env_to_cutoff.smoothed.next(),
                amp_env: self.adsr_settings(),
                mod_env: self.mod_env_settings(),
            };

            let out = self.voices.next(&p, self.sample_rate) * self.params.gain.smoothed.next();
            for sample in channel_samples {
                *sample = out;
            }
        }

        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for Johnwave {
    const CLAP_ID: &'static str = "dev.john.johnwave";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A simple wave-morphing synth");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for Johnwave {
    const VST3_CLASS_ID: [u8; 16] = *b"JohnwaveSynth001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nice_export_clap!(Johnwave);
nice_export_vst3!(Johnwave);
