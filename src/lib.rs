use nice_plug::prelude::*;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

mod crush;
mod editor;
mod env;
mod filter;
mod knob;
mod lfo;
mod modulation;
mod notes;
mod osc;
mod readout;
mod slider;
mod trail;
mod unison;
mod voice;
use env::AdsrSettings;
use filter::FilterType;
use lfo::{Lfo, SyncDivision};
use modulation::{enum_id, ModSlotParams, Routes, SlotDest, SlotSource, MATRIX_SLOTS};
use nice_plug::plugin::ParamValue;
use notes::NoteStack;
use voice::{Porta, VoiceParams, VoicePool};
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
    /// The Crush switch as a 0..1 fade, so flipping it mid-note doesn't click.
    crush_fade: SwitchFade,
    /// The same for the Osc 2 switch.
    osc2_fade: SwitchFade,
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

    // The second oscillator. Off by default, so older patches sound the same.
    #[id = "osc2_on"]
    pub osc2_on: BoolParam,
    /// Osc 2's wave, like Shape.
    #[id = "osc2_morph"]
    pub osc2_morph: FloatParam,
    /// Osc 2's tuning against osc 1, in whole semitones.
    #[id = "osc2_coarse"]
    pub osc2_coarse: IntParam,
    /// Osc 2's tuning against osc 1, in cents.
    #[id = "osc2_fine"]
    pub osc2_fine: FloatParam,
    /// The blend: 0 = only osc 1, 100% = only osc 2.
    #[id = "osc_mix"]
    pub osc_mix: FloatParam,

    // Unison (see `unison.rs`). One voice, the default, is no unison.
    /// How many copies of the oscillators each note plays.
    #[id = "uni_voices"]
    pub unison_voices: IntParam,
    /// How far the outer copies are detuned, in cents.
    #[id = "uni_detune"]
    pub unison_detune: FloatParam,
    /// How far the copies spread across the speakers.
    #[id = "uni_width"]
    pub unison_width: FloatParam,

    /// LFO speed in Hz.
    #[id = "lfo_rate"]
    pub lfo_rate: FloatParam,
    /// The LFO's wave: 0 = sine, 33% = triangle, 67% = saw, 100% = square.
    #[id = "lfo_shape"]
    pub lfo_shape: FloatParam,
    /// Restart the LFO cycle on each fresh (non-legato) note.
    #[id = "lfo_retrig"]
    pub lfo_retrigger: BoolParam,
    /// Follow the DAW's tempo instead of the LFO Rate knob.
    #[id = "lfo_sync"]
    pub lfo_sync: BoolParam,
    /// Cycle length when synced.
    #[id = "lfo_div"]
    pub lfo_division: EnumParam<SyncDivision>,

    /// Which part of the sound the filter keeps (low-pass, high-pass, ...).
    #[id = "filter_type"]
    pub filter_type: EnumParam<FilterType>,
    /// Filter cutoff in Hz.
    #[id = "cutoff"]
    pub cutoff: FloatParam,
    /// Peak at the cutoff: 0 = none, 1 = strong ringing (stops short of self-oscillation).
    #[id = "resonance"]
    pub resonance: FloatParam,

    // Bitcrusher, applied in each voice before the filter (see `crush.rs`).
    /// The on/off switch. While off, the knobs (and routes to them) do nothing.
    #[id = "crush_on"]
    pub crush_on: BoolParam,
    /// Bit depth: 16 = clean, 1 = harshest.
    #[id = "crush_bits"]
    pub crush_bits: FloatParam,
    /// Sample-rate reduction, following the note: how many stairs each wave cycle is
    /// cut into. Fewer = rougher. The top setting is off.
    #[id = "crush_steps"]
    pub crush_steps: FloatParam,
    /// Blend between the clean (0) and crushed (1) sound.
    #[id = "crush_mix"]
    pub crush_mix: FloatParam,

    // The Mod Env is a second ADSR. What it moves is set in the mod matrix.
    #[id = "menv_att"]
    pub mod_env_attack: FloatParam,
    #[id = "menv_dec"]
    pub mod_env_decay: FloatParam,
    /// Level held while the key is down. 0 (the default) makes it an attack/decay envelope.
    #[id = "menv_sus"]
    pub mod_env_sustain: FloatParam,
    #[id = "menv_rel"]
    pub mod_env_release: FloatParam,

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
    /// Where the sound sits between the speakers: −1 = left, 0 = centre, 1 = right.
    #[id = "pan"]
    pub pan: FloatParam,

    /// How far the pitch bend wheel goes, in semitones each way.
    #[id = "bend_range"]
    pub bend_range: IntParam,
    /// Transpose in whole semitones.
    #[id = "coarse"]
    pub coarse: IntParam,
    /// Fine tune in cents (100 cents = 1 semitone).
    #[id = "fine"]
    pub fine: FloatParam,
    /// Portamento: how long each new note takes to slide in from the last, in ms.
    /// 0 = off.
    #[id = "porta"]
    pub porta: FloatParam,
    /// Only slide when the new key is pressed while another is still held.
    #[id = "porta_legato"]
    pub porta_legato: BoolParam,

    /// The mod matrix: each slot routes one source to one destination.
    /// `nested(array)` saves slot 1 as `mod_src_1`, `mod_dst_1`, `mod_amt_1`, and so on.
    #[nested(array, group = "Mod Matrix")]
    pub mod_slots: [ModSlotParams; MATRIX_SLOTS],
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
    .with_value_to_string(Arc::new(ms_text))
    .with_string_to_value(Arc::new(ms_from_text))
}

/// A time in milliseconds as text: "250 ms", or "1.50 s" from a second up.
fn ms_text(ms: f32) -> String {
    if ms < 1000.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// Read a typed time: "250 ms", "1.5 s", or a plain number of milliseconds.
fn ms_from_text(text: &str) -> Option<f32> {
    let s = text.trim().to_lowercase();
    if let Some(n) = s.strip_suffix("ms") {
        n.trim().parse().ok()
    } else if let Some(n) = s.strip_suffix('s') {
        n.trim().parse::<f32>().ok().map(|secs| secs * 1000.0)
    } else {
        s.parse().ok()
    }
}

/// The Porta knob: 0–2 s, skewed like the envelope times, with 0 shown as Off.
fn porta_param() -> FloatParam {
    FloatParam::new(
        "Porta",
        0.0,
        FloatRange::Skewed {
            min: 0.0,
            max: 2_000.0,
            factor: FloatRange::skew_factor(-2.0),
        },
    )
    .with_value_to_string(Arc::new(|ms| if ms < 0.5 { "Off".into() } else { ms_text(ms) }))
    .with_string_to_value(Arc::new(|s| {
        if s.trim().eq_ignore_ascii_case("off") { Some(0.0) } else { ms_from_text(s) }
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

            osc2_on: BoolParam::new("Osc 2", false),
            osc2_morph: FloatParam::new("Osc 2 Shape", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            osc2_coarse: IntParam::new("Osc 2 Coarse", 0, IntRange::Linear { min: -36, max: 36 })
                .with_unit(" st"),
            // A few cents off by default, so switching osc 2 on thickens the sound
            // straight away instead of just doubling it.
            osc2_fine: FloatParam::new("Osc 2 Fine", 7.0, FloatRange::Linear { min: -100.0, max: 100.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit(" ct")
                .with_value_to_string(Arc::new(|ct| format!("{ct:+.0}"))),
            osc_mix: FloatParam::new("Osc Mix", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            unison_voices: IntParam::new(
                "Unison Voices",
                1,
                IntRange::Linear { min: 1, max: unison::MAX_VOICES as i32 },
            ),
            // Skewed so the small detunes, where most of the sweet spots are, get more
            // of the knob's travel.
            unison_detune: FloatParam::new(
                "Unison Detune",
                25.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: unison::MAX_DETUNE_CT,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" ct")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            unison_width: FloatParam::new("Unison Width", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
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
            filter_type: EnumParam::new("Filter Type", FilterType::LowPass),
            resonance: FloatParam::new("Resonance", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            // Off by default, so older patches sound the same. The knobs start somewhere
            // you can hear, so switching Crush on does something straight away.
            crush_on: BoolParam::new("Crush", false),
            crush_bits: FloatParam::new(
                "Crush Bits",
                8.0,
                FloatRange::Linear { min: crush::MIN_BITS, max: crush::MAX_BITS },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(Arc::new(|bits| {
                if bits >= crush::MAX_BITS { "Off".into() } else { format!("{bits:.1} bit") }
            })),
            crush_steps: FloatParam::new(
                "Crush Steps",
                16.0,
                // Skewed so the low end, where each step is most audible, gets more of
                // the knob's travel.
                FloatRange::Skewed {
                    min: crush::MIN_STEPS,
                    max: crush::MAX_STEPS,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(Arc::new(|steps| {
                if steps >= crush::MAX_STEPS { "Off".into() } else { format!("{steps:.0} steps") }
            })),
            crush_mix: FloatParam::new("Crush Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),

            mod_env_attack: time_param("Mod Env Attack", 1.0, 5_000.0),
            mod_env_decay: time_param("Mod Env Decay", 500.0, 10_000.0),
            mod_env_sustain: FloatParam::new(
                "Mod Env Sustain",
                0.0,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(formatters::v2s_f32_percentage(0))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            mod_env_release: time_param("Mod Env Release", 500.0, 10_000.0),

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
            pan: FloatParam::new("Pan", 0.0, FloatRange::Linear { min: -1.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(Arc::new(|v| pan_name(v)))
                .with_string_to_value(Arc::new(|s| pan_from_text(s))),

            bend_range: IntParam::new("Bend Range", 2, IntRange::Linear { min: 0, max: 24 })
                .with_unit(" st"),
            coarse: IntParam::new("Coarse", 0, IntRange::Linear { min: -24, max: 24 })
                .with_unit(" st"),
            fine: FloatParam::new("Fine", 0.0, FloatRange::Linear { min: -100.0, max: 100.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit(" ct")
                .with_value_to_string(Arc::new(|ct| format!("{ct:+.0}"))),
            porta: porta_param(),
            porta_legato: BoolParam::new("Porta Legato", false),

            // `from_fn` builds the array by calling this once per slot (i = 0, 1, ...).
            mod_slots: std::array::from_fn(|i| ModSlotParams::new(i + 1)),
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

/// Show Pan as "L50", "C" or "R50" (percent of the way to that side).
fn pan_name(value: f32) -> String {
    let percent = (value * 100.0).round() as i32;
    match percent {
        0 => "C".to_string(),
        p if p < 0 => format!("L{}", -p),
        p => format!("R{p}"),
    }
}

/// Read typed-in Pan text: "C", "L30", "r 100", or a plain number (−100..100).
fn pan_from_text(text: &str) -> Option<f32> {
    let t = text.trim().to_ascii_uppercase();
    let percent = if t == "C" {
        0.0
    } else if let Some(rest) = t.strip_prefix('L') {
        -rest.trim().parse::<f32>().ok()?
    } else if let Some(rest) = t.strip_prefix('R') {
        rest.trim().parse::<f32>().ok()?
    } else {
        t.parse::<f32>().ok()?
    };
    Some((percent / 100.0).clamp(-1.0, 1.0))
}

/// Left and right levels for a pan position (−1..=1), using the equal-power law: the
/// two levels trace a quarter circle, so left² + right² is always 1 and the sound stays
/// equally loud as it moves. At centre each side is √½ ≈ 0.707 (−3 dB).
fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4; // 0 (left) to π/2 (right)
    (angle.cos(), angle.sin())
}

/// The pitch shift shared by every voice, in semitones: the bend wheel plus tuning.
/// `bend` is 0..1 with 0.5 = centred, so `(bend - 0.5) * 2` runs −1..=1.
fn pitch_offset_st(bend: f32, bend_range_st: f32, coarse_st: f32, fine_cents: f32) -> f32 {
    (bend - 0.5) * 2.0 * bend_range_st + coarse_st + fine_cents / 100.0
}

/// Turn matrix slots (source, destination, amount −1..=1) into routes, scaling each
/// amount to its destination's unit. A slot with a blank source or destination is
/// skipped.
pub(crate) fn matrix_routes(slots: impl Iterator<Item = (SlotSource, SlotDest, f32)>) -> Routes {
    let mut routes = Routes::EMPTY;
    for (source, dest, amount) in slots {
        if let (Some(source), Some(dest)) = (source.source(), dest.dest()) {
            routes.add(source, dest, amount * dest.max_amount());
        }
    }
    routes
}

/// The four knobs that came before the mod matrix: (old id, source, destination). Their
/// values were already in the destination's unit.
const OLD_MOD_KNOBS: [(&str, SlotSource, SlotDest); 4] = [
    ("lfo_depth", SlotSource::Lfo, SlotDest::Shape),
    ("lfo_cut", SlotSource::Lfo, SlotDest::Cutoff),
    ("menv_amt", SlotSource::ModEnv, SlotDest::Shape),
    ("menv_cut", SlotSource::ModEnv, SlotDest::Cutoff),
];

/// Move an old patch's "LFO/Mod Env to Shape/Cutoff" knobs into matrix slots, so it
/// sounds the same. Each knob that was turned up takes the next slot. Patches saved with
/// the matrix are left alone.
fn migrate_fixed_knobs(params: &mut BTreeMap<String, ParamValue>) {
    if params.contains_key("mod_src_1") {
        return;
    }
    let mut slot = 1;
    for (old_id, source, dest) in OLD_MOD_KNOBS {
        // `remove` takes the old value out of the patch (nothing uses that id any more).
        let Some(ParamValue::F32(value)) = params.remove(old_id) else {
            continue;
        };
        // Every destination in the table is a real one, so `dest()` is never `None`.
        let (Some(scale), true) = (dest.dest().map(|d| d.max_amount()), value != 0.0) else {
            continue;
        };
        params.insert(format!("mod_src_{slot}"), ParamValue::String(enum_id(source).into()));
        params.insert(format!("mod_dst_{slot}"), ParamValue::String(enum_id(dest).into()));
        params.insert(format!("mod_amt_{slot}"), ParamValue::F32(value / scale));
        slot += 1;
    }
}

/// The Mod Env used to have no Release knob: its release time was its decay time.
/// Patches from before then get Release = Decay, so they sound the same. (Their Sustain
/// gets the default of 0, which is what it used to be.)
fn migrate_mod_env_release(params: &mut BTreeMap<String, ParamValue>) {
    if params.contains_key("menv_rel") {
        return;
    }
    if let Some(&ParamValue::F32(decay)) = params.get("menv_dec") {
        params.insert("menv_rel".into(), ParamValue::F32(decay));
    }
}

/// An on/off switch as a 0..1 fade (10 ms), so flipping it mid-note doesn't click.
struct SwitchFade {
    fade: Smoother<f32>,
    /// Where the fade is heading, to spot when the switch flips.
    on: bool,
}

impl Default for SwitchFade {
    fn default() -> Self {
        Self {
            fade: Smoother::new(SmoothingStyle::Linear(10.0)),
            on: false,
        }
    }
}

impl SwitchFade {
    /// Jump straight to the switch's position, with no fade.
    fn reset(&mut self, on: bool) {
        self.on = on;
        self.fade.reset(if on { 1.0 } else { 0.0 });
    }

    /// Once per block. Only starts a new fade when the switch flips, since setting the
    /// target restarts the fade from wherever it is.
    fn update(&mut self, on: bool, sample_rate: f32) {
        if on != self.on {
            self.on = on;
            self.fade.set_target(sample_rate, if on { 1.0 } else { 0.0 });
        }
    }

    /// The fade for this sample.
    fn next(&mut self) -> f32 {
        self.fade.next()
    }
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
            crush_fade: SwitchFade::default(),
            osc2_fade: SwitchFade::default(),
            editor_state: editor::default_state(),
            display_morph: Arc::new(AtomicF32::new(f32::NAN)),
            display_level: Arc::new(AtomicF32::new(0.0)),
            repaint: RepaintNotifier::new(),
        }
    }
}

impl Johnwave {
    fn handle_event(&mut self, event: NoteEvent<()>) {
        let porta = Porta {
            seconds: self.params.porta.value() / 1000.0,
            legato_only: self.params.porta_legato.value(),
        };
        match event {
            NoteEvent::NoteOn { key, velocity, .. } => {
                let note = key.number_or_middle_c();
                match self.mode {
                    VoiceMode::Poly => self.voices.note_on(note, velocity, self.sample_rate, porta),
                    VoiceMode::Mono => {
                        let legato = self.notes.top().is_some();
                        self.notes.push(note);
                        // Another key still held: change pitch only. Fresh press: retrigger.
                        if legato {
                            self.voices.mono_glide(note, self.sample_rate, porta);
                        } else {
                            self.voices.mono_start(note, velocity, self.sample_rate, porta);
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
                            // Fall back to the previously held key without retriggering
                            // (gliding back, with Porta on).
                            Some(prev) => self.voices.mono_glide(prev, self.sample_rate, porta),
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

    /// The mod matrix slots, as routes. Reading `.smoothed.next()` advances each amount's
    /// smoother by one sample, so this is called exactly once per sample.

    fn matrix_routes(&self) -> Routes {
        matrix_routes(self.params.mod_slots.iter().map(|slot| {
            // Every amount smoother advances, even for an Off slot, so turning the slot
            // on later doesn't start from a stale value.
            let amount = slot.amount.smoothed.next();
            (slot.source.value(), slot.dest.value(), amount)
        }))
    }

    fn adsr_settings(&self) -> AdsrSettings {
        AdsrSettings {
            attack_s: self.params.attack.value() / 1000.0,
            decay_s: self.params.decay.value() / 1000.0,
            sustain: self.params.sustain.value(),
            release_s: self.params.release.value() / 1000.0,
        }
    }

    fn mod_env_settings(&self) -> AdsrSettings {
        AdsrSettings {
            attack_s: self.params.mod_env_attack.value() / 1000.0,
            decay_s: self.params.mod_env_decay.value() / 1000.0,
            sustain: self.params.mod_env_sustain.value(),
            release_s: self.params.mod_env_release.value() / 1000.0,
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

    /// Runs on every saved patch just before it loads, so old patches can be updated.
    fn filter_state(state: &mut PluginState) {
        migrate_fixed_knobs(&mut state.params);
        migrate_mod_env_release(&mut state.params);
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
        // Start at the switches' positions, with no fade.
        self.crush_fade.reset(self.params.crush_on.value());
        self.osc2_fade.reset(self.params.osc2_on.value());
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

        // The Crush and Osc 2 switches fade rather than jump.
        self.crush_fade.update(self.params.crush_on.value(), self.sample_rate);
        self.osc2_fade.update(self.params.osc2_on.value(), self.sample_rate);

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
            let unison_voices = self.params.unison_voices.value() as usize;
            let p = VoiceParams {
                morph: self.params.morph.smoothed.next(),
                osc2_amount: self.osc2_fade.next(),
                osc2_morph: self.params.osc2_morph.smoothed.next(),
                osc2_pitch_st: self.params.osc2_coarse.value() as f32
                    + self.params.osc2_fine.smoothed.next() / 100.0,
                osc_mix: self.params.osc_mix.smoothed.next(),
                unison_voices,
                unison_detune_ct: self.params.unison_detune.smoothed.next(),
                unison_pan: unison::pans(unison_voices, self.params.unison_width.smoothed.next()),
                lfo_rate,
                lfo_shape,
                shared_lfo: shared_lfo_on.then_some(shared),
                pitch_st: pitch_offset_st(
                    self.bend.next(),
                    self.params.bend_range.value() as f32,
                    self.params.coarse.value() as f32,
                    self.params.fine.smoothed.next(),
                ),
                filter_type: self.params.filter_type.value(),
                cutoff_hz: self.params.cutoff.smoothed.next(),
                resonance: self.params.resonance.smoothed.next(),
                crush_amount: self.crush_fade.next(),
                crush_bits: self.params.crush_bits.smoothed.next(),
                crush_steps: self.params.crush_steps.smoothed.next(),
                crush_mix: self.params.crush_mix.smoothed.next(),
                routes: self.matrix_routes(),
                amp_env: self.adsr_settings(),
                mod_env: self.mod_env_settings(),
            };

            let (out_l, out_r) = self.voices.next(&p, self.sample_rate);
            let gain = self.params.gain.smoothed.next();
            // Pan works as a balance on the stereo voices. Without unison both sides are
            // the same, so it's the same equal-power pan as ever.
            let (left, right) = pan_gains(self.params.pan.smoothed.next());
            // The layout is always stereo, so there are exactly two channels.
            for (channel, sample) in channel_samples.into_iter().enumerate() {
                *sample = if channel == 0 { out_l * gain * left } else { out_r * gain * right };
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
    fn pan_centre_is_equal_and_hard_sides_are_silent_opposite() {
        let (l, r) = pan_gains(0.0);
        assert!(close(l, r) && close(l, std::f32::consts::FRAC_1_SQRT_2));
        let (l, r) = pan_gains(-1.0);
        assert!(close(l, 1.0) && close(r, 0.0));
        let (l, r) = pan_gains(1.0);
        assert!(close(l, 0.0) && close(r, 1.0));
    }

    #[test]
    fn pan_keeps_total_power_constant() {
        for i in -10..=10 {
            let (l, r) = pan_gains(i as f32 / 10.0);
            assert!(close(l * l + r * r, 1.0));
        }
    }

    #[test]
    fn pan_text_round_trips() {
        assert_eq!(pan_name(0.0), "C");
        assert_eq!(pan_name(-0.5), "L50");
        assert_eq!(pan_name(1.0), "R100");
        assert_eq!(pan_from_text("c"), Some(0.0));
        assert_eq!(pan_from_text("L50"), Some(-0.5));
        assert_eq!(pan_from_text("r 100"), Some(1.0));
        assert_eq!(pan_from_text("-25"), Some(-0.25));
        assert_eq!(pan_from_text("nonsense"), None);
    }

    fn old_patch(values: &[(&str, f32)]) -> BTreeMap<String, ParamValue> {
        values
            .iter()
            .map(|&(id, v)| (id.to_string(), ParamValue::F32(v)))
            .collect()
    }

    fn text(params: &BTreeMap<String, ParamValue>, id: &str) -> String {
        match params.get(id) {
            Some(ParamValue::String(s)) => s.clone(),
            other => panic!("{id}: {other:?}"),
        }
    }

    fn number(params: &BTreeMap<String, ParamValue>, id: &str) -> f32 {
        match params.get(id) {
            Some(ParamValue::F32(v)) => *v,
            other => panic!("{id}: {other:?}"),
        }
    }

    #[test]
    fn matrix_ids_are_what_migration_writes() {
        // If nice-plug ever named the slots differently, migrated patches would load into
        // nothing. This pins the ids.
        let params = SynthParams::default();
        let ids: Vec<String> = params.param_map().into_iter().map(|(id, _, _)| id).collect();
        for slot in 1..=MATRIX_SLOTS {
            for name in ["mod_src", "mod_dst", "mod_amt"] {
                assert!(ids.contains(&format!("{name}_{slot}")), "{name}_{slot}");
            }
        }
        assert!(!ids.iter().any(|id| OLD_MOD_KNOBS.iter().any(|k| k.0 == id)));
    }

    #[test]
    fn old_knobs_move_into_slots_in_order() {
        let mut params = old_patch(&[
            ("lfo_depth", 0.5),
            ("lfo_cut", -3.0),
            ("menv_amt", -0.25),
            ("menv_cut", 6.0),
            ("cutoff", 800.0),
        ]);
        migrate_fixed_knobs(&mut params);
        assert_eq!(text(&params, "mod_src_1"), "lfo");
        assert_eq!(text(&params, "mod_dst_1"), "shape");
        assert_eq!(number(&params, "mod_amt_1"), 0.5);
        assert_eq!(text(&params, "mod_dst_2"), "cutoff");
        assert_eq!(number(&params, "mod_amt_2"), -0.5); // −3 oct of ±6
        assert_eq!(text(&params, "mod_src_3"), "menv");
        assert_eq!(number(&params, "mod_amt_3"), -0.25);
        assert_eq!(number(&params, "mod_amt_4"), 1.0);
        assert!(!params.contains_key("lfo_depth") && !params.contains_key("menv_cut"));
        assert_eq!(number(&params, "cutoff"), 800.0); // other params untouched
    }

    #[test]
    fn knobs_left_at_zero_take_no_slot() {
        let mut params = old_patch(&[("lfo_depth", 0.0), ("lfo_cut", 0.0), ("menv_cut", 2.0)]);
        migrate_fixed_knobs(&mut params);
        assert_eq!(text(&params, "mod_dst_1"), "cutoff");
        assert!(!params.contains_key("mod_src_2"));
        assert!(!params.contains_key("lfo_depth"));
    }

    #[test]
    fn patches_saved_with_the_matrix_are_left_alone() {
        let mut params = old_patch(&[("mod_amt_1", 0.3)]);
        params.insert("mod_src_1".into(), ParamValue::String("vel".into()));
        params.insert("lfo_cut".into(), ParamValue::F32(2.0)); // a stray old value
        migrate_fixed_knobs(&mut params);
        // (`ParamValue` can't be compared with `==`, so check the pieces.)
        assert_eq!(params.len(), 3);
        assert_eq!(text(&params, "mod_src_1"), "vel");
        assert_eq!(number(&params, "mod_amt_1"), 0.3);
        assert!(!params.contains_key("mod_dst_1"));
    }

    #[test]
    fn old_mod_env_release_follows_decay() {
        let mut params = old_patch(&[("menv_dec", 3950.0)]);
        migrate_mod_env_release(&mut params);
        assert_eq!(number(&params, "menv_rel"), 3950.0);

        // A patch that already has a release keeps it.
        let mut params = old_patch(&[("menv_dec", 3950.0), ("menv_rel", 80.0)]);
        migrate_mod_env_release(&mut params);
        assert_eq!(number(&params, "menv_rel"), 80.0);
    }

    #[test]
    fn matrix_slots_scale_to_the_destination() {
        let slots = [
            (SlotSource::Lfo, SlotDest::Cutoff, 0.5),
            (SlotSource::Off, SlotDest::Pitch, 1.0),
            (SlotSource::Random, SlotDest::Off, 1.0),
            (SlotSource::Velocity, SlotDest::Pitch, -0.25),
        ];
        let routes = matrix_routes(slots.into_iter());
        let all: Vec<_> = routes.iter().collect();
        assert_eq!(all.len(), 2); // a blank source or destination adds nothing
        assert_eq!(all[0].amount, 3.0); // half of ±6 oct
        assert_eq!(all[1].amount, -6.0); // a quarter of ±24 st, flipped
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
