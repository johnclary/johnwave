//! The fading trail behind the oscillator screen's wave, like the glow an analog
//! oscilloscope leaves behind. The window keeps the last fraction of a second of shapes
//! it has shown, and draws them faint and thin behind the main wave.
//!
//! This is window-only state: it remembers what the screen showed, not anything about
//! the sound.

use crate::screen::ScreenWave;
use std::collections::VecDeque;

/// How long a ghost takes to fade away completely, in seconds.
const LIFETIME: f64 = 0.35;
/// The shortest time between two ghosts, in seconds. Keeps the count to about a dozen.
const STEP: f64 = 1.0 / 30.0;

/// One shape the screen showed earlier.
pub struct Ghost {
    /// When it was shown, in seconds (egui's clock).
    time: f64,
    pub wave: ScreenWave,
}

#[derive(Default)]
pub struct Trail {
    /// Oldest first. A `VecDeque` is a list that's quick to add to at one end and remove
    /// from at the other.
    ghosts: VecDeque<Ghost>,
}

impl Trail {
    /// Call once per frame with the time and what the screen shows now (`None` when
    /// nothing is playing).
    pub fn update(&mut self, now: f64, wave: Option<ScreenWave>) {
        // Drop ghosts that have fully faded. They're oldest first, so look at the front.
        while self.ghosts.front().is_some_and(|g| now - g.time >= LIFETIME) {
            self.ghosts.pop_front();
        }
        // Silent: add nothing, and let the ones already there fade out.
        let Some(wave) = wave else {
            return;
        };
        // Only add a ghost when the shape has visibly moved, so a steady note shows just
        // the one clean wave.
        let add = match self.ghosts.back() {
            None => true,
            Some(last) => last.wave.moved(&wave) && now - last.time >= STEP,
        };
        if add {
            self.ghosts.push_back(Ghost { time: now, wave });
        }
    }

    /// Whether there's anything left to fade out. While there is, the window has to keep
    /// redrawing even if nothing else changes.
    pub fn is_fading(&self) -> bool {
        !self.ghosts.is_empty()
    }

    /// Each ghost, oldest first, with how strongly to draw it: 1 when brand new, falling
    /// to 0 as it fades.
    pub fn ghosts(&self, now: f64) -> impl Iterator<Item = (&Ghost, f32)> {
        self.ghosts.iter().map(move |g| {
            let fade = 1.0 - ((now - g.time) / LIFETIME).clamp(0.0, 1.0) as f32;
            (g, fade)
        })
    }
}

/// The OUT line's afterglow, like the phosphor on an analog scope's screen. Each frame
/// the newest stretch of output is blended into what's already showing instead of
/// replacing it, so the parts of the sound that stay the same build up into a steady
/// line and fast changes (detune, chords) soften into a blur instead of flickering.
#[derive(Default)]
pub struct Phosphor {
    /// What the OUT line shows, left to right. Empty when there's nothing to show.
    shown: Vec<f32>,
    /// When it was last updated, in seconds (egui's clock).
    time: f64,
}

/// How quickly the glow catches up with the sound, in seconds: after this long it has
/// gone about two thirds of the way to a new shape. Longer is calmer but lags behind
/// knob moves.
const PHOSPHOR_TIME: f64 = 0.2;

impl Phosphor {
    /// Call once per frame with the time and the newest stretch of output (`None` when
    /// there's none, which clears it so the next note starts fresh).
    pub fn update(&mut self, now: f64, latest: Option<Vec<f32>>) {
        let Some(latest) = latest else {
            self.shown.clear();
            return;
        };
        if self.shown.len() != latest.len() {
            self.shown = latest;
        } else {
            // How far to move towards the new shape. Worked out from the time since
            // the last frame, so it looks the same however often frames come.
            let elapsed = (now - self.time).max(0.0);
            let amount = (1.0 - (-elapsed / PHOSPHOR_TIME).exp()) as f32;
            for (shown, new) in self.shown.iter_mut().zip(latest) {
                *shown += (new - *shown) * amount;
            }
        }
        self.time = now;
    }

    /// The line to draw, left to right, or `None` when there's nothing to show.
    pub fn line(&self) -> Option<&[f32]> {
        (!self.shown.is_empty()).then_some(&self.shown[..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phosphor_settles_on_a_steady_shape() {
        let mut p = Phosphor::default();
        p.update(0.0, Some(vec![0.0; 4]));
        // A new shape that then stays put: the glow slides over to it.
        for frame in 1..=60 {
            p.update(frame as f64 / 60.0, Some(vec![1.0; 4]));
        }
        assert!(p.line().unwrap().iter().all(|&y| (y - 1.0).abs() < 1e-3));
        // Silence clears it.
        p.update(1.1, None);
        assert_eq!(p.line(), None);
    }
}
