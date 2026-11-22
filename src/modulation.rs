//! Modulation routing: which source moves which destination, and by how much.
//!
//! A *source* is something that changes over time on its own (the LFO, the Mod Env,
//! the note's velocity, the LFO's random sample & hold). A *destination* is a sound setting it can move (Morph,
//! Cutoff, ...). A *route* connects one source to one destination with an amount.
//!
//! Each voice works out its source values every sample, runs them through the routes,
//! and gets back one offset per destination (`ModOffsets`). The voice then adds those
//! offsets to the knob values. The routes come from the mod matrix: a few slots, each
//! picking a source, a destination and an amount (`ModSlotParams`). Any new source or
//! destination works with every existing one.
//!
//! Every destination has its own unit, which is what a route's amount means:
//! - Morph, Osc 2 Morph: morph units (1.0 = the whole sine → square range)
//! - Osc Mix: mix units (1.0 = the whole osc 1 → osc 2 range)
//! - Cutoff: octaves (+1 doubles the cutoff, −1 halves it)
//! - Resonance: resonance units (1.0 = the whole 0–100% range)
//! - Pitch (both oscillators), Osc 2 Pitch (osc 2 only): semitones
//! - Detune (unison spread): cents
//! - Bits (crusher bit depth): bits
//! - Steps (crusher stairs per cycle): octaves (+1 doubles the steps, −1 halves them)
//! - Mix (crusher blend): mix units (1.0 = the whole 0–100% range)
//!
//! As with every knob, a positive amount turns it clockwise. For Bits and Steps that
//! means cleaner, so a negative amount is what adds crush.

use nice_plug::prelude::*;
use std::sync::Arc;

/// Things that can modulate. Values are −1..=1 (bipolar) or 0..=1 (unipolar).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ModSource {
    /// −1..=1
    Lfo,
    /// 0..=1
    ModEnv,
    /// 0..=1, how hard the key was hit.
    Velocity,
    /// −1..=1, the LFO's sample & hold output: a new random level each LFO cycle.
    Random,
}

impl ModSource {
    /// Every source, in the order the window lists them.
    pub const ALL: [ModSource; 4] = [ModSource::Lfo, ModSource::ModEnv, ModSource::Velocity, ModSource::Random];

    /// The lowest and highest values this source can reach.
    pub fn range(self) -> (f32, f32) {
        match self {
            ModSource::Lfo | ModSource::Random => (-1.0, 1.0),
            ModSource::ModEnv | ModSource::Velocity => (0.0, 1.0),
        }
    }
}

/// Settings that can be modulated.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ModDest {
    Morph,
    Osc2Morph,
    Osc2Pitch,
    OscMix,
    UnisonDetune,
    Cutoff,
    Resonance,
    Pitch,
    CrushBits,
    CrushSteps,
    CrushMix,
}

impl ModDest {
    /// How far a matrix slot at 100% pushes this destination, in its own unit.
    pub fn max_amount(self) -> f32 {
        match self {
            ModDest::Morph => 1.0,
            ModDest::Osc2Morph => 1.0,
            // The same reach as the Osc 2 Coarse knob.
            ModDest::Osc2Pitch => 36.0,
            ModDest::OscMix => 1.0,
            ModDest::UnisonDetune => crate::unison::MAX_DETUNE_CT,
            ModDest::Cutoff => 6.0,
            ModDest::Resonance => 1.0,
            ModDest::Pitch => 24.0,
            ModDest::CrushBits => crate::crush::MAX_BITS - crate::crush::MIN_BITS,
            // From the top (256) to the bottom (2) is 7 halvings.
            ModDest::CrushSteps => (crate::crush::MAX_STEPS / crate::crush::MIN_STEPS).log2(),
            ModDest::CrushMix => 1.0,
        }
    }

    /// A slot amount (−1..=1) written in this destination's unit, like "+2.0 oct".
    pub fn format_amount(self, amount: f32) -> String {
        let value = amount * self.max_amount();
        match self {
            ModDest::Morph
            | ModDest::Osc2Morph
            | ModDest::OscMix
            | ModDest::Resonance
            | ModDest::CrushMix => {
                format!("{:+.0}%", value * 100.0)
            }
            ModDest::Cutoff | ModDest::CrushSteps => format!("{value:+.1} oct"),
            ModDest::Pitch | ModDest::Osc2Pitch => format!("{value:+.1} st"),
            ModDest::CrushBits => format!("{value:+.1} bits"),
            ModDest::UnisonDetune => format!("{value:+.0} ct"),
        }
    }
}

/// A matrix slot's source choice: one of the sources, or Off (blank).
///
/// `Enum` lets it be a parameter. The `#[id]`s are what saved patches store, so they
/// must never change. The variants can be renamed or reordered freely.
#[derive(Enum, Clone, Copy, PartialEq, Debug)]
pub enum SlotSource {
    #[id = "off"]
    Off,
    #[id = "lfo"]
    #[name = "LFO"]
    Lfo,
    #[id = "menv"]
    #[name = "Mod Env"]
    ModEnv,
    #[id = "vel"]
    Velocity,
    #[id = "random"]
    Random,
}

impl SlotSource {
    /// The matrix choice for `source`.
    pub fn of(source: ModSource) -> SlotSource {
        match source {
            ModSource::Lfo => SlotSource::Lfo,
            ModSource::ModEnv => SlotSource::ModEnv,
            ModSource::Velocity => SlotSource::Velocity,
            ModSource::Random => SlotSource::Random,
        }
    }

    /// Its name as the window shows it ("Mod Env").
    pub fn name(self) -> &'static str {
        Self::variants()[self.to_index()]
    }

    /// The source, or `None` for Off.
    pub fn source(self) -> Option<ModSource> {
        match self {
            SlotSource::Off => None,
            SlotSource::Lfo => Some(ModSource::Lfo),
            SlotSource::ModEnv => Some(ModSource::ModEnv),
            SlotSource::Velocity => Some(ModSource::Velocity),
            SlotSource::Random => Some(ModSource::Random),
        }
    }
}

/// A matrix slot's destination choice: one of the destinations, or Off (blank).
/// Same rules as `SlotSource`: never change an `#[id]`.
#[derive(Enum, Clone, Copy, PartialEq, Debug)]
pub enum SlotDest {
    #[id = "off"]
    Off,
    #[id = "shape"]
    Shape,
    #[id = "shape2"]
    #[name = "Shape 2"]
    Shape2,
    #[id = "osc_mix"]
    #[name = "Osc Mix"]
    OscMix,
    #[id = "cutoff"]
    Cutoff,
    #[id = "res"]
    #[name = "Res"]
    Resonance,
    #[id = "pitch"]
    Pitch,
    #[id = "pitch2"]
    #[name = "Pitch 2"]
    Pitch2,
    #[id = "detune"]
    Detune,
    #[id = "crush_bits"]
    #[name = "Bits"]
    CrushBits,
    #[id = "crush_steps"]
    #[name = "Steps"]
    CrushSteps,
    #[id = "crush_mix"]
    #[name = "Crush Mix"]
    CrushMix,
}

impl SlotDest {
    /// The matrix choice for `dest`. The reverse of `dest()`.
    pub fn of(dest: ModDest) -> SlotDest {
        match dest {
            ModDest::Morph => SlotDest::Shape,
            ModDest::Osc2Morph => SlotDest::Shape2,
            ModDest::Osc2Pitch => SlotDest::Pitch2,
            ModDest::OscMix => SlotDest::OscMix,
            ModDest::UnisonDetune => SlotDest::Detune,
            ModDest::Cutoff => SlotDest::Cutoff,
            ModDest::Resonance => SlotDest::Resonance,
            ModDest::Pitch => SlotDest::Pitch,
            ModDest::CrushBits => SlotDest::CrushBits,
            ModDest::CrushSteps => SlotDest::CrushSteps,
            ModDest::CrushMix => SlotDest::CrushMix,
        }
    }

    /// Its name as the window shows it ("Crush Mix").
    pub fn name(self) -> &'static str {
        Self::variants()[self.to_index()]
    }

    /// The destination, or `None` for Off.
    pub fn dest(self) -> Option<ModDest> {
        match self {
            SlotDest::Off => None,
            SlotDest::Shape => Some(ModDest::Morph),
            SlotDest::Shape2 => Some(ModDest::Osc2Morph),
            SlotDest::OscMix => Some(ModDest::OscMix),
            SlotDest::Cutoff => Some(ModDest::Cutoff),
            SlotDest::Resonance => Some(ModDest::Resonance),
            SlotDest::Pitch => Some(ModDest::Pitch),
            SlotDest::Pitch2 => Some(ModDest::Osc2Pitch),
            SlotDest::Detune => Some(ModDest::UnisonDetune),
            SlotDest::CrushBits => Some(ModDest::CrushBits),
            SlotDest::CrushSteps => Some(ModDest::CrushSteps),
            SlotDest::CrushMix => Some(ModDest::CrushMix),
        }
    }

    /// A slot amount (−1..=1) as text: in the destination's unit, or a plain
    /// percentage while the destination is blank.
    pub fn format_amount(self, amount: f32) -> String {
        match self.dest() {
            Some(dest) => dest.format_amount(amount),
            None => format!("{:+.0}%", amount * 100.0),
        }
    }
}

/// The id an enum choice is saved under in patches (its `#[id = "..."]`).
pub fn enum_id<T: Enum>(choice: T) -> &'static str {
    T::ids().expect("every choice has an #[id]")[choice.to_index()]
}

/// How many slots the mod matrix has.
pub const MATRIX_SLOTS: usize = 16;

/// One mod matrix slot: source → destination, by `amount`.
///
/// When these sit in an array in `SynthParams`, nice-plug adds the slot number to each id,
/// so slot 1 saves as `mod_src_1`, `mod_dst_1` and `mod_amt_1`.
#[derive(Params)]
pub struct ModSlotParams {
    #[id = "mod_src"]
    pub source: EnumParam<SlotSource>,
    #[id = "mod_dst"]
    pub dest: EnumParam<SlotDest>,
    /// −1..=1, a fraction of the destination's `max_amount`. Negative flips it.
    #[id = "mod_amt"]
    pub amount: FloatParam,
}

impl ModSlotParams {
    /// Slot `number` (counting from 1), named so the DAW can tell the slots apart.
    pub fn new(number: usize) -> Self {
        Self {
            source: EnumParam::new(format!("Mod {number} Source"), SlotSource::Off),
            dest: EnumParam::new(format!("Mod {number} Dest"), SlotDest::Off),
            amount: FloatParam::new(
                format!("Mod {number} Amount"),
                0.0,
                FloatRange::Linear { min: -1.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:+.0}", v * 100.0)))
            .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

/// One source → destination connection.
#[derive(Clone, Copy, Debug)]
pub struct Route {
    pub source: ModSource,
    pub dest: ModDest,
    /// In the destination's unit (see the module docs). Can be negative, which flips it.
    pub amount: f32,
}

/// Room for one route per matrix slot.
const MAX_ROUTES: usize = MATRIX_SLOTS;

/// A fixed-size list of routes. It uses a plain array instead of a `Vec`, so building
/// it every sample never allocates memory (allocating on the audio thread can glitch).
#[derive(Clone, Copy, Debug)]
pub struct Routes {
    list: [Route; MAX_ROUTES],
    len: usize,
}

impl Routes {
    /// No routes. `const` means it can be used to build constants, like in the tests.
    pub const EMPTY: Routes = Routes {
        list: [Route {
            source: ModSource::Lfo,
            dest: ModDest::Morph,
            amount: 0.0,
        }; MAX_ROUTES],
        len: 0,
    };

    /// Add a route. Ignored if the list is full, or if `amount` is 0 (it would do nothing).
    pub fn add(&mut self, source: ModSource, dest: ModDest, amount: f32) {
        if amount != 0.0 && self.len < MAX_ROUTES {
            self.list[self.len] = Route { source, dest, amount };
            self.len += 1;
        }
    }

    /// The routes added so far.
    pub fn iter(&self) -> impl Iterator<Item = &Route> {
        self.list[..self.len].iter()
    }
}

/// One voice's source values for the current sample.
#[derive(Clone, Copy, Default, Debug)]
pub struct ModSources {
    pub lfo: f32,
    pub mod_env: f32,
    pub velocity: f32,
    pub random: f32,
}

impl ModSources {
    pub fn get(&self, source: ModSource) -> f32 {
        match source {
            ModSource::Lfo => self.lfo,
            ModSource::ModEnv => self.mod_env,
            ModSource::Velocity => self.velocity,
            ModSource::Random => self.random,
        }
    }
}

/// How far each destination is pushed this sample, in that destination's unit.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct ModOffsets {
    pub morph: f32,
    pub osc2_morph: f32,
    pub osc2_pitch_st: f32,
    pub osc_mix: f32,
    pub unison_detune_ct: f32,
    pub cutoff_oct: f32,
    pub resonance: f32,
    pub pitch_st: f32,
    pub crush_bits: f32,
    pub crush_steps_oct: f32,
    pub crush_mix: f32,
}

impl ModOffsets {
    fn add(&mut self, dest: ModDest, value: f32) {
        match dest {
            ModDest::Morph => self.morph += value,
            ModDest::Osc2Morph => self.osc2_morph += value,
            ModDest::Osc2Pitch => self.osc2_pitch_st += value,
            ModDest::OscMix => self.osc_mix += value,
            ModDest::UnisonDetune => self.unison_detune_ct += value,
            ModDest::Cutoff => self.cutoff_oct += value,
            ModDest::Resonance => self.resonance += value,
            ModDest::Pitch => self.pitch_st += value,
            ModDest::CrushBits => self.crush_bits += value,
            ModDest::CrushSteps => self.crush_steps_oct += value,
            ModDest::CrushMix => self.crush_mix += value,
        }
    }
}

/// Run every route: each one adds `source value × amount` to its destination.
/// Several routes to the same destination add up.
pub fn apply(routes: &Routes, sources: &ModSources) -> ModOffsets {
    let mut offsets = ModOffsets::default();
    for route in routes.iter() {
        offsets.add(route.dest, sources.get(route.source) * route.amount);
    }
    offsets
}

/// How far the routes can push `dest`, as `(lowest, highest)` offsets in its unit, or
/// `None` if nothing is routed there. With `only` set, just that source's routes count
/// (mod mode's view). The knobs draw this as their mod arc.
///
/// Each route reaches from `amount × source's lowest` to `amount × source's highest`
/// (swapped when the amount is negative). Routes to the same destination add up, so
/// their lowest points add and their highest points add.
pub fn mod_span(routes: &Routes, dest: ModDest, only: Option<ModSource>) -> Option<(f32, f32)> {
    let mut span = None;
    for route in routes.iter().filter(|r| r.dest == dest && only.is_none_or(|s| r.source == s)) {
        let (lo, hi) = route.source.range();
        let (a, b) = (lo * route.amount, hi * route.amount);
        let (total_lo, total_hi) = span.unwrap_or((0.0, 0.0));
        span = Some((total_lo + a.min(b), total_hi + a.max(b)));
    }
    span
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCES: ModSources = ModSources {
        lfo: -0.5,
        mod_env: 0.8,
        velocity: 1.0,
        random: 0.3,
    };

    #[test]
    fn slot_choices_map_both_ways() {
        // Every choice but Off (index 0) round-trips.
        for i in 1..SlotDest::variants().len() {
            let choice = SlotDest::from_index(i);
            assert_eq!(SlotDest::of(choice.dest().unwrap()), choice);
        }
        for source in ModSource::ALL {
            assert_eq!(SlotSource::of(source).source(), Some(source));
        }
    }

    #[test]
    fn no_routes_means_no_change() {
        assert_eq!(apply(&Routes::EMPTY, &SOURCES), ModOffsets::default());
    }

    #[test]
    fn a_route_scales_its_source() {
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Lfo, ModDest::Cutoff, 2.0);
        let off = apply(&routes, &SOURCES);
        assert_eq!(off.cutoff_oct, -1.0); // −0.5 × 2 octaves
        assert_eq!(off.morph, 0.0);
    }

    #[test]
    fn routes_to_the_same_destination_add_up() {
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Lfo, ModDest::Morph, 0.4); // −0.2
        routes.add(ModSource::ModEnv, ModDest::Morph, 0.5); // +0.4
        let off = apply(&routes, &SOURCES);
        assert!((off.morph - 0.2).abs() < 1e-6);
    }

    #[test]
    fn one_source_can_drive_several_destinations() {
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Velocity, ModDest::Pitch, 12.0);
        routes.add(ModSource::Velocity, ModDest::Resonance, -0.25);
        let off = apply(&routes, &SOURCES);
        assert_eq!(off.pitch_st, 12.0);
        assert_eq!(off.resonance, -0.25);
    }

    #[test]
    fn span_of_an_unrouted_destination_is_none() {
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Lfo, ModDest::Cutoff, 2.0);
        assert_eq!(mod_span(&routes, ModDest::Morph, Some(ModSource::Lfo)), None);
        // Routed there, but from a different source.
        assert_eq!(mod_span(&routes, ModDest::Cutoff, Some(ModSource::ModEnv)), None);
    }

    #[test]
    fn span_follows_the_sources_range_and_sign() {
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Lfo, ModDest::Cutoff, 2.0); // bipolar: both ways
        routes.add(ModSource::ModEnv, ModDest::Pitch, -12.0); // unipolar, flipped
        assert_eq!(mod_span(&routes, ModDest::Cutoff, Some(ModSource::Lfo)), Some((-2.0, 2.0)));
        assert_eq!(mod_span(&routes, ModDest::Pitch, Some(ModSource::ModEnv)), Some((-12.0, 0.0)));
    }

    #[test]
    fn spans_of_routes_to_the_same_destination_add_up() {
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Lfo, ModDest::Morph, 0.25); // −0.25..+0.25
        routes.add(ModSource::Lfo, ModDest::Morph, -0.5); // −0.5..+0.5
        routes.add(ModSource::Velocity, ModDest::Morph, 0.5); // a different source
        assert_eq!(mod_span(&routes, ModDest::Morph, Some(ModSource::Lfo)), Some((-0.75, 0.75)));
        assert_eq!(mod_span(&routes, ModDest::Morph, Some(ModSource::Velocity)), Some((0.0, 0.5)));
        // All sources together.
        assert_eq!(mod_span(&routes, ModDest::Morph, None), Some((-0.75, 1.25)));
    }

    #[test]
    fn amounts_read_in_the_destinations_unit() {
        assert_eq!(ModDest::Cutoff.format_amount(0.5), "+3.0 oct");
        assert_eq!(ModDest::Morph.format_amount(-0.35), "-35%");
        assert_eq!(ModDest::Pitch.format_amount(0.5), "+12.0 st");
        assert_eq!(ModDest::Osc2Pitch.format_amount(-1.0), "-36.0 st");
        assert_eq!(ModDest::OscMix.format_amount(0.25), "+25%");
        assert_eq!(ModDest::UnisonDetune.format_amount(0.5), "+50 ct");
    }

    #[test]
    fn zero_amounts_are_skipped_and_a_full_list_ignores_extras() {
        let mut routes = Routes::EMPTY;
        routes.add(ModSource::Lfo, ModDest::Morph, 0.0);
        assert_eq!(routes.iter().count(), 0);
        for _ in 0..(MAX_ROUTES + 3) {
            routes.add(ModSource::Velocity, ModDest::Pitch, 1.0);
        }
        assert_eq!(routes.iter().count(), MAX_ROUTES);
    }
}
