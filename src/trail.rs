//! The fading trail behind the oscillator screen's wave, like the glow an analog
//! oscilloscope leaves behind. The window keeps the last fraction of a second of shapes
//! it has shown, and draws them faint and thin behind the main wave.
//!
//! This is window-only state: it remembers what the screen showed, not anything about
//! the sound.

use std::collections::VecDeque;

/// How long a ghost takes to fade away completely, in seconds.
const LIFETIME: f64 = 0.35;
/// The shortest time between two ghosts, in seconds. Keeps the count to about a dozen.
const STEP: f64 = 1.0 / 30.0;

/// One shape the screen showed earlier.
pub struct Ghost {
    /// When it was shown, in seconds (egui's clock).
    time: f64,
    pub morph: f32,
    pub level: f32,
}

#[derive(Default)]
pub struct Trail {
    /// Oldest first. A `VecDeque` is a list that's quick to add to at one end and remove
    /// from at the other.
    ghosts: VecDeque<Ghost>,
}

impl Trail {
    /// Call once per frame with the time and what the screen shows now (`morph` is NaN
    /// when nothing is playing).
    pub fn update(&mut self, now: f64, morph: f32, level: f32) {
        // Drop ghosts that have fully faded. They're oldest first, so look at the front.
        while self.ghosts.front().is_some_and(|g| now - g.time >= LIFETIME) {
            self.ghosts.pop_front();
        }
        // Silent: add nothing, and let the ones already there fade out.
        if morph.is_nan() {
            return;
        }
        // Only add a ghost when the shape has visibly moved, so a steady note shows just
        // the one clean wave.
        let add = match self.ghosts.back() {
            None => true,
            Some(last) => {
                let moved = (last.morph - morph).abs() > 0.002 || (last.level - level).abs() > 0.005;
                moved && now - last.time >= STEP
            }
        };
        if add {
            self.ghosts.push_back(Ghost { time: now, morph, level });
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
