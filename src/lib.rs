use nice_plug::prelude::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;

mod editor;
mod env;
mod filter;
mod knob;
mod lfo;
mod modulation;
mod notes;
mod osc;
mod readout;
mod trail;
mod voice;
use env::AdsrSettings;
use lfo::{Lfo, SyncDivision};
use modulation::{ModDest, ModSource, Routes};
use notes::NoteStack;
use voice::{VoiceParams, VoicePool};
use editor::JohnwaveEditor;
use nice_plug_egui::{EguiEditor, EguiEditorState, EguiNiceSettings, RepaintNotifier};

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
    /// The pitch bend wheel, 0..1 with 0.5 = centred. Smoothed, because MIDI sends the
    /// wheel as a series of small jumps that would otherwise sound steppy.
    bend: Smoother<f32>,
    /// The plugin window's size and whether it's open.
    editor_state: Arc<EguiEditorState>,
    /// The Morph the window's waveform shows, or NaN when nothing is playing. The audio
    /// thread writes it and the window reads it. An atomic number can be shared like this
    /// safely without locking.
    display_morph: Arc<AtomicF32>,
    /// How loud that voice is (0..1). The waveform is drawn this tall.
    display_level: Arc<AtomicF32>,
    /// Lets the audio thread tell the window to redraw (the waveform moved).
    repaint: RepaintNotifier,
}

#[derive(Enum, PartialEq, Clone, Copy)]
pub enum VoiceMode {
    #[id = "poly"]
    Poly,
    #[id = "mono"]
    #[name = "Mono (legato)"]
    Mono,
}

#[derive(Params)]
pub struct SynthParams {
    #[id = "voice_mode"]
    pub voice_mode: EnumParam<VoiceMode>,

    /// 0 = sine, 33% = triangle, 67% = saw, 100% = square.
    #[id = "morph"]
    pub morph: FloatParam,

    /// LFO speed in Hz.
    #[id = "lfo_rate"]
    pub lfo_rate: FloatParam,
    /// The LFO's wave: 0 = sine, 33% = triangle, 67% = saw, 100% = square.
    #[id = "lfo_shape"]
    pub lfo_shape: FloatParam,
    /// How far the LFO swings Morph. At 100% it moves ±100%, i.e. the whole range.
    /// (Shown as "LFO to Shape". The id is from when it was called "LFO Depth".)
    #[id = "lfo_depth"]
    pub lfo_to_morph: FloatParam,
    /// How many octaves the LFO swings the cutoff up and down.
    #[id = "lfo_cut"]
    pub lfo_to_cutoff: FloatParam,
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

    /// How far the pitch bend wheel goes, in semitones each way.
    #[id = "bend_range"]
    pub bend_range: IntParam,
    /// Transpose in whole semitones.
    #[id = "coarse"]
    pub coarse: IntParam,
    /// Fine tune in cents (100 cents = 1 semitone).
    #[id = "fine"]
    pub fine: FloatParam,
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
            // Shown as "Shape" (it used to be "Morph"). The code still calls it `morph`,
            // and the id stays "morph" so saved projects and automation keep working.
            morph: FloatParam::new("Shape", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
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
            lfo_shape: FloatParam::new("LFO Shape", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(Arc::new(shape_name))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            lfo_to_morph: FloatParam::new("LFO to Shape", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
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

            lfo_to_cutoff: FloatParam::new(
                "LFO to Cutoff",
                0.0,
                FloatRange::Linear { min: -4.0, max: 4.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" oct")
            .with_value_to_string(Arc::new(|oct| format!("{oct:+.1}"))),
            mod_env_to_cutoff: FloatParam::new(
                "Mod Env to Cutoff",
                0.0,
                FloatRange::Linear { min: -6.0, max: 6.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" oct")
            .with_value_to_string(Arc::new(|oct| format!("{oct:+.1}"))),
            mod_env_to_morph: FloatParam::new(
                "Mod Env to Shape",
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

            bend_range: IntParam::new("Bend Range", 2, IntRange::Linear { min: 0, max: 24 })
                .with_unit(" st"),
            coarse: IntParam::new("Coarse", 0, IntRange::Linear { min: -24, max: 24 })
                .with_unit(" st"),
            fine: FloatParam::new("Fine", 0.0, FloatRange::Linear { min: -100.0, max: 100.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit(" ct")
                .with_value_to_string(Arc::new(|ct| format!("{ct:+.0}"))),
        }
    }
}

/// Show the LFO Shape knob as a percentage, naming the pure shapes at 0/33/67/100%.
fn shape_name(value: f32) -> String {
    let percent = (value * 100.0).round();
    match percent as i32 {
        0 => "Sine".to_string(),
        33 => "Triangle".to_string(),
        67 => "Saw".to_string(),
        100 => "Square".to_string(),
        _ => format!("{percent:.0}%"),
    }
}

/// The pitch shift shared by every voice, in semitones: the bend wheel plus tuning.
/// `bend` is 0..1 with 0.5 = centred, so `(bend - 0.5) * 2` runs −1..=1.
fn pitch_offset_st(bend: f32, bend_range_st: f32, coarse_st: f32, fine_cents: f32) -> f32 {
    (bend - 0.5) * 2.0 * bend_range_st + coarse_st + fine_cents / 100.0
}

/// A bend smoother that starts with the wheel centred. 5 ms is quick enough to feel
/// immediate but smooths out the jumps between MIDI bend messages.
fn centred_bend() -> Smoother<f32> {
    let bend = Smoother::new(SmoothingStyle::Linear(5.0));
    bend.reset(0.5);
    bend
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
            bend: centred_bend(),
            editor_state: editor::default_state(),
            display_morph: Arc::new(AtomicF32::new(f32::NAN)),
            display_level: Arc::new(AtomicF32::new(0.0)),
            repaint: RepaintNotifier::new(),
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
            // Bend affects every voice. `channel` is ignored: all 16 MIDI channels bend
            // the whole synth.
            NoteEvent::MidiPitchBend { value, .. } => {
                self.bend.set_target(self.sample_rate, value);
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

    /// Once per block: pass the Morph and level currently being played to the window, and
    /// ask it to redraw if the wave visibly changed. Skipped while the window is closed.
    fn update_display(&self) {
        if !self.editor_state.is_open() {
            return;
        }
        // NaN ("not a number") is the signal for "nothing is playing": the screen goes flat.
        let (morph, level) = self.voices.newest_display().unwrap_or((f32::NAN, 0.0));
        let shown = self.display_morph.load(Ordering::Relaxed);
        let shown_level = self.display_level.load(Ordering::Relaxed);
        // NaN never equals anything, so check "silent vs. playing" before comparing values.
        let changed = if morph.is_nan() || shown.is_nan() {
            morph.is_nan() != shown.is_nan()
        } else {
            (morph - shown).abs() > 0.002 || (level - shown_level).abs() > 0.005
        };
        if changed {
            self.display_morph.store(morph, Ordering::Relaxed);
            self.display_level.store(level, Ordering::Relaxed);
            self.repaint.request_repaint();
        }
    }

    /// The modulation knobs on the panel, as routes. Each knob is a fixed source →
    /// destination pair. Reading `.smoothed.next()` advances each knob's smoother by one
    /// sample, so this is called exactly once per sample.
    fn fixed_routes(&self) -> Routes {
        let p = &self.params;
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Lfo, ModDest::Morph, p.lfo_to_morph.smoothed.next());
        routes.add(ModSource::Lfo, ModDest::Cutoff, p.lfo_to_cutoff.smoothed.next());
        routes.add(ModSource::ModEnv, ModDest::Morph, p.mod_env_to_morph.smoothed.next());
        routes.add(ModSource::ModEnv, ModDest::Cutoff, p.mod_env_to_cutoff.smoothed.next());
        routes
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

    // `MidiCCs` (not `Basic`) so the DAW also sends pitch bend, not just notes.
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type Editor = EguiEditor<JohnwaveEditor>;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    /// The DAW asks for the window when you open the plugin.
    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        nice_plug_egui::create_egui_editor(
            self.editor_state.clone(),
            self.repaint.clone(),
            EguiNiceSettings::new().with_tile("johnwave"),
            JohnwaveEditor::new(
                self.params.clone(),
                self.display_morph.clone(),
                self.display_level.clone(),
            ),
        )
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
        self.bend.reset(0.5);
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
            let lfo_shape = self.params.lfo_shape.smoothed.next();
            let shared = self.free_lfo.next(lfo_rate, self.sample_rate, lfo_shape);
            let p = VoiceParams {
                morph: self.params.morph.smoothed.next(),
                lfo_rate,
                lfo_shape,
                shared_lfo: shared_lfo_on.then_some(shared),
                pitch_st: pitch_offset_st(
                    self.bend.next(),
                    self.params.bend_range.value() as f32,
                    self.params.coarse.value() as f32,
                    self.params.fine.smoothed.next(),
                ),
                cutoff_hz: self.params.cutoff.smoothed.next(),
                resonance: self.params.resonance.smoothed.next(),
                routes: self.fixed_routes(),
                amp_env: self.adsr_settings(),
                mod_env: self.mod_env_settings(),
            };

            let out = self.voices.next(&p, self.sample_rate) * self.params.gain.smoothed.next();
            for sample in channel_samples {
                *sample = out;
            }
        }

        self.update_display();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn centred_wheel_and_no_tuning_leave_pitch_alone() {
        assert_eq!(pitch_offset_st(0.5, 2.0, 0.0, 0.0), 0.0);
        assert_eq!(pitch_offset_st(0.5, 24.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn full_bend_reaches_the_bend_range() {
        assert!(close(pitch_offset_st(1.0, 2.0, 0.0, 0.0), 2.0));
        assert!(close(pitch_offset_st(0.0, 2.0, 0.0, 0.0), -2.0));
        assert!(close(pitch_offset_st(0.75, 12.0, 0.0, 0.0), 6.0));
        assert!(close(pitch_offset_st(1.0, 0.0, 0.0, 0.0), 0.0)); // range 0 = bend off
    }

    #[test]
    fn coarse_and_fine_add_on_top() {
        assert!(close(pitch_offset_st(0.5, 2.0, -12.0, 0.0), -12.0));
        assert!(close(pitch_offset_st(0.5, 2.0, 0.0, 100.0), 1.0)); // 100 cents = 1 st
        assert!(close(pitch_offset_st(1.0, 2.0, 7.0, -50.0), 8.5));
    }
}
