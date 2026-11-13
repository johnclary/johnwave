//! A simple ADSR envelope.
//!
//! - **Attack** rises in a straight line from the *current* level to full. A retriggered
//!   note therefore never snaps to zero, so it can't click.
//! - **Decay** falls exponentially toward the sustain level and stays there. If you move
//!   the Sustain knob while a note is held, the level glides smoothly to the new value.
//! - **Release** falls exponentially to silence.
//!
//! "Time" for decay and release means roughly how long it takes to get 99% of the way.

/// Shortest allowed stage time. Anything quicker would click.
const MIN_TIME_S: f32 = 0.001;
/// How many time constants make up a "stage time": e^-4.6 ≈ 1%.
const TIME_CONSTANTS: f32 = 4.6;
/// Below this the envelope is treated as silent (about −100 dB).
const SILENCE: f32 = 1e-5;

#[derive(Clone, Copy)]
pub struct AdsrSettings {
    pub attack_s: f32,
    pub decay_s: f32,
    pub sustain: f32,
    pub release_s: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    Idle,
    Attack,
    /// Decay toward sustain, then hold there for as long as the key is down.
    DecaySustain,
    Release,
}

pub struct Adsr {
    stage: Stage,
    level: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
        }
    }
}

impl Adsr {
    /// Key pressed: start (or restart) the attack from wherever the level is now.
    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    /// Key released: fade out from wherever the level is now.
    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// True once the note has fully faded out (or never started).
    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    /// Advance one sample and return the level (0..=1).
    pub fn next(&mut self, s: &AdsrSettings, sample_rate: f32) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += 1.0 / (s.attack_s.max(MIN_TIME_S) * sample_rate);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::DecaySustain;
                }
            }
            Stage::DecaySustain => {
                let target = s.sustain.clamp(0.0, 1.0);
                self.level = approach(self.level, target, s.decay_s, sample_rate);
            }
            Stage::Release => {
                self.level = approach(self.level, 0.0, s.release_s, sample_rate);
                if self.level < SILENCE {
                    self.reset();
                }
            }
        }
        self.level
    }
}

/// One step of an exponential glide from `level` toward `target`.
fn approach(level: f32, target: f32, time_s: f32, sample_rate: f32) -> f32 {
    let samples = time_s.max(MIN_TIME_S) * sample_rate / TIME_CONSTANTS;
    let coef = (-1.0 / samples).exp();
    target + (level - target) * coef
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    const SETTINGS: AdsrSettings = AdsrSettings {
        attack_s: 0.01,
        decay_s: 0.1,
        sustain: 0.5,
        release_s: 0.2,
    };

    fn run(env: &mut Adsr, seconds: f32) -> Vec<f32> {
        (0..(seconds * SR) as usize)
            .map(|_| env.next(&SETTINGS, SR))
            .collect()
    }

    #[test]
    fn idle_is_silent() {
        let mut env = Adsr::default();
        assert!(run(&mut env, 0.1).iter().all(|&x| x == 0.0));
    }

    #[test]
    fn attack_reaches_full_level_on_time() {
        let mut env = Adsr::default();
        env.trigger();
        let out = run(&mut env, 0.02);
        let peak_at = out.iter().position(|&x| x >= 1.0).unwrap();
        let expected = (SETTINGS.attack_s * SR) as usize;
        assert!(peak_at.abs_diff(expected) <= 1, "peaked at sample {peak_at}");
    }

    #[test]
    fn decays_to_sustain_and_holds() {
        let mut env = Adsr::default();
        env.trigger();
        run(&mut env, SETTINGS.attack_s + SETTINGS.decay_s);
        // After the decay time it should be within ~1% of the way to sustain.
        let level = env.next(&SETTINGS, SR);
        assert!((level - 0.5).abs() < 0.01, "level {level}");
        // ...and it holds there.
        assert!(run(&mut env, 1.0).iter().all(|&x| (x - 0.5).abs() < 0.01));
    }

    #[test]
    fn release_fades_to_silence_and_goes_idle() {
        let mut env = Adsr::default();
        env.trigger();
        run(&mut env, 0.5);
        env.release();
        run(&mut env, 2.0);
        assert_eq!(env.stage, Stage::Idle);
        assert_eq!(env.next(&SETTINGS, SR), 0.0);
    }

    #[test]
    fn zero_sustain_works_as_an_attack_decay_envelope() {
        // How the Mod Env uses it: peak at 1, decay to 0, hold at 0 while the
        // key is down, and stay smooth when released mid-decay.
        let ad = AdsrSettings {
            attack_s: 0.005,
            decay_s: 0.2,
            sustain: 0.0,
            release_s: 0.2,
        };
        let mut env = Adsr::default();
        env.trigger();
        let mut out: Vec<f32> = (0..(0.1 * SR) as usize).map(|_| env.next(&ad, SR)).collect();
        assert!(out.iter().any(|&x| x == 1.0), "never reached the peak");
        env.release(); // let go halfway through the decay
        out.extend((0..(0.5 * SR) as usize).map(|_| env.next(&ad, SR)));

        let worst = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(worst <= 1.0 / (ad.attack_s * SR) + 1e-6, "step of {worst}");
        assert_eq!(env.stage, Stage::Idle);

        // Held down instead: decays to ~0 and holds there.
        env.trigger();
        for _ in 0..SR as usize {
            env.next(&ad, SR);
        }
        assert!(env.next(&ad, SR) < 1e-4);
    }

    #[test]
    fn never_jumps_even_when_retriggered_mid_release() {
        // Any stage boundary or retrigger should move smoothly, never in a step.
        // The fastest legal movement is the attack slope.
        let max_step = 1.0 / (SETTINGS.attack_s * SR) + 1e-6;
        let mut env = Adsr::default();
        let mut out = Vec::new();
        env.trigger();
        out.extend(run(&mut env, 0.3));
        env.release();
        out.extend(run(&mut env, 0.05));
        env.trigger(); // press again while still ringing out
        out.extend(run(&mut env, 0.3));

        let worst = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(worst <= max_step, "step of {worst}");
        assert!(out.iter().all(|&x| (0.0..=1.0).contains(&x)));
    }
}
