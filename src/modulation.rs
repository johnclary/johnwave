//! Modulation routing: which source moves which destination, and by how much.
//!
//! A *source* is something that changes over time on its own (the LFO, the Mod Env,
//! the note's velocity, the LFO's random sample & hold). A *destination* is a sound setting it can move (Morph,
//! Cutoff, ...). A *route* connects one source to one destination with an amount.
//!
//! Each voice works out its source values every sample, runs them through the routes,
//! and gets back one offset per destination (`ModOffsets`). The voice then adds those
//! offsets to the knob values. The fixed knobs like "LFO to Cutoff" are just routes
//! too, so any new source or destination works with every existing one.
//!
//! Every destination has its own unit, which is what a route's amount means:
//! - Morph: morph units (1.0 = the whole sine → square range)
//! - Cutoff: octaves (+1 doubles the cutoff, −1 halves it)
//! - Resonance: resonance units (1.0 = the whole 0–100% range)
//! - Pitch: semitones

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

/// Settings that can be modulated.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ModDest {
    Morph,
    Cutoff,
    Resonance,
    Pitch,
}

/// One source → destination connection.
#[derive(Clone, Copy, Debug)]
pub struct Route {
    pub source: ModSource,
    pub dest: ModDest,
    /// In the destination's unit (see the module docs). Can be negative, which flips it.
    pub amount: f32,
}

/// Room for the fixed knobs plus a future mod matrix.
const MAX_ROUTES: usize = 16;

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
    pub cutoff_oct: f32,
    pub resonance: f32,
    pub pitch_st: f32,
}

impl ModOffsets {
    fn add(&mut self, dest: ModDest, value: f32) {
        match dest {
            ModDest::Morph => self.morph += value,
            ModDest::Cutoff => self.cutoff_oct += value,
            ModDest::Resonance => self.resonance += value,
            ModDest::Pitch => self.pitch_st += value,
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
