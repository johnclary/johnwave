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

/// The DEPTH chip's history: the waves of the last moment, lined up going back toward
/// the horizon, so the sound seems to roll in toward you. Unlike the trail it keeps
/// adding rows on a steady note, so the ridges keep marching forward while it's held.
#[derive(Default)]
pub struct Depth {
    /// Oldest first, like the trail.
    rows: VecDeque<Row>,
}

/// One wave in the DEPTH stack.
pub struct Row {
    /// When it was taken, in seconds (egui's clock).
    time: f64,
    pub wave: ScreenWave,
    /// The OUT line at that moment, left to right, if it was showing.
    pub out: Option<Vec<f32>>,
}

/// How often a new row is added, in seconds.
pub const DEPTH_STEP: f64 = 1.0 / 20.0;
/// How many rows fit between the front and the horizon.
pub const DEPTH_ROWS: usize = 16;
/// How long a row takes to travel from the front to the horizon, in seconds.
const DEPTH_LIFETIME: f64 = DEPTH_STEP * DEPTH_ROWS as f64;

impl Depth {
    /// Call once per frame with the time, what the screen shows now (`None` when
    /// nothing is playing) and the OUT line if it's showing.
    pub fn update(&mut self, now: f64, wave: Option<ScreenWave>, out: Option<&[f32]>) {
        while self.rows.front().is_some_and(|r| now - r.time >= DEPTH_LIFETIME) {
            self.rows.pop_front();
        }
        // Silent: add nothing, and let the rows already there drift off.
        let Some(wave) = wave else {
            return;
        };
        // Rows sit on an even beat of DEPTH_STEP, whenever the frames happen to come,
        // so the gaps between them stay the same as they roll forward.
        let time = (now / DEPTH_STEP).floor() * DEPTH_STEP;
        let add = self.rows.back().is_none_or(|last| time > last.time + DEPTH_STEP / 2.0);
        if add {
            self.rows.push_back(Row { time, wave, out: out.map(|o| o.to_vec()) });
        }
    }

    /// Forget everything, for when DEPTH is switched off.
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// Whether any rows are still on their way to the horizon. While there are, the
    /// window has to keep redrawing.
    pub fn is_active(&self) -> bool {
        !self.rows.is_empty()
    }

    /// Each row, oldest (furthest back) first, with how far back it is: 0 at the front,
    /// 1 at the horizon. Worked out from its age, so the rows glide smoothly between
    /// one being added and the next.
    pub fn rows(&self, now: f64) -> impl Iterator<Item = (&Row, f32)> {
        self.rows.iter().map(move |r| {
            let z = ((now - r.time) / DEPTH_LIFETIME).clamp(0.0, 1.0) as f32;
            (r, z)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_keeps_rolling_on_a_steady_note_then_clears() {
        let mut d = Depth::default();
        let wave = ScreenWave::default();
        let frames = 60;
        for frame in 0..frames {
            d.update(frame as f64 / 60.0, Some(wave), None);
        }
        let now = (frames - 1) as f64 / 60.0;
        // A steady note still adds rows, up to about a horizon's worth.
        let count = d.rows(now).count();
        assert!(count >= DEPTH_ROWS - 1 && count <= DEPTH_ROWS + 1);
        // Oldest first, so the depth goes from far back to the front.
        let depths: Vec<f32> = d.rows(now).map(|(_, z)| z).collect();
        assert!(depths.windows(2).all(|p| p[0] >= p[1]));
        // After silence they all drift off.
        d.update(now + DEPTH_LIFETIME + 0.01, None, None);
        assert!(!d.is_active());
    }

    #[test]
    fn phosphor_settles_on_a_steady_shape() {
        let mut p = Phosphor::default();
        p.update(0.0, Some(vec![0.0; 4]));
        // A new shape that then stays put: the glow slides over to it.
        // Ten times PHOSPHOR_TIME of frames is long enough whatever it's tuned to.
        let frames = (PHOSPHOR_TIME * 10.0 * 60.0) as usize;
        for frame in 1..=frames {
            p.update(frame as f64 / 60.0, Some(vec![1.0; 4]));
        }
        assert!(p.line().unwrap().iter().all(|&y| (y - 1.0).abs() < 1e-3));
        // Silence clears it.
        p.update(frames as f64 / 60.0 + 0.1, None);
        assert_eq!(p.line(), None);
    }
}
