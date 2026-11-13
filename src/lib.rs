use nice_plug::prelude::*;
use std::f32::consts::TAU;
use std::sync::Arc;

mod notes;
use notes::NoteStack;

/// Monophonic synth. Step 1: a plain sine oscillator driven by MIDI.
pub struct Johnwave {
    params: Arc<SynthParams>,
    sample_rate: f32,

    /// Oscillator phase in `[0, 1)`.
    phase: f32,
    /// Held keys, most recent last (last-note priority).
    notes: NoteStack,
    /// Frequency of the sounding note.
    freq: f32,
    /// Temporary declicking gate. Replaced by the ADSR in step 4.
    gate: Smoother<f32>,
}

#[derive(Params)]
struct SynthParams {
    #[id = "gain"]
    pub gain: FloatParam,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
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
            phase: 0.0,
            notes: NoteStack::default(),
            freq: 440.0,
            gate: Smoother::new(SmoothingStyle::Linear(5.0)),
        }
    }
}

impl Johnwave {
    fn handle_event(&mut self, event: NoteEvent<()>) {
        match event {
            NoteEvent::NoteOn { key, velocity, .. } => {
                let note = key.number_or_middle_c();
                self.notes.push(note);
                self.freq = util::midi_key_to_freq(note);
                self.gate.set_target(self.sample_rate, velocity);
            }
            NoteEvent::NoteOff { key, .. } => {
                self.notes.remove(key.number_or_middle_c());
                match self.notes.top() {
                    // Fall back to the previously held key without retriggering.
                    Some(prev) => self.freq = util::midi_key_to_freq(prev),
                    None => self.gate.set_target(self.sample_rate, 0.0),
                }
            }
            _ => (),
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
        self.phase = 0.0;
        self.notes.clear();
        self.gate.reset(0.0);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut next_event = context.next_event();

        for (sample_id, channel_samples) in buffer.iter_samples().enumerate() {
            // Apply all events scheduled at or before this sample.
            while let Some(event) = next_event {
                if event.timing() > sample_id as u32 {
                    break;
                }
                self.handle_event(event);
                next_event = context.next_event();
            }

            let osc = (self.phase * TAU).sin();
            self.phase += self.freq / self.sample_rate;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }

            let out = osc * self.gate.next() * self.params.gain.smoothed.next();
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
