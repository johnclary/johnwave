//! A neon arc knob for any parameter.
//!
//! neither egui nor nice-plug-egui comes with a knob, so this is our own widget. It works
//! like the waveform: claim a square of the window, then draw circles and lines into it.
//! On top of that it listens for the mouse:
//! - drag up/down to change the value (hold Shift for fine control)
//! - scroll wheel to nudge it
//! - double-click to reset it to its default
//!
//! Its name and value show in the readout box (`readout.rs`) while you're on it.
//!
//! In mod mode (a source armed in the header), a knob that's a mod destination edits
//! that source's route instead: dragging sets the amount (adding a route in a free
//! matrix slot if there isn't one) and double-click removes it. Knobs that can't be
//! modulated dim and ignore the mouse until mod mode ends.
//!
//! It only reads and sets parameters: its own, and in mod mode the matrix slots.

use crate::modulation::{ModSlotParams, ModSource, SlotDest, SlotSource};
use nice_plug::prelude::{Param, ParamSetter};

/// Everything about how the knob looks, in one place so it's easy to tweak.
#[derive(Clone)]
pub struct KnobStyle {
    /// Width of the knob circle, in points.
    pub diameter: f32,
    /// Thickness of the bright value arc.
    pub arc_width: f32,
    /// Glow layers under the arc: (extra width beyond `arc_width`, strength 0..1).
    /// Wider, fainter copies of the arc underneath make it look like neon.
    pub glow: Vec<(f32, f32)>,
    /// Colour of the value arc.
    pub arc_color: egui::Color32,
    /// Colour of the faint track the arc fills along.
    pub track_color: egui::Color32,
    /// Size of the name underneath ("Gain").
    pub label_text_size: f32,
    /// Colour of the mod arc: the stripe along the value arc's ring showing how far the
    /// mod matrix can push this knob.
    pub mod_color: egui::Color32,
    /// Thickness of the mod arc. At `arc_width` it covers the value arc completely (its
    /// glow still shows around the edges); thinner leaves a cyan edge either side.
    pub mod_arc_width: f32,
    /// Thickness of the value tick: the short line across the ring marking where the
    /// knob is set, drawn on top of the mod arc so the setting stays readable.
    pub tick_width: f32,
}

impl Default for KnobStyle {
    fn default() -> Self {
        Self {
            diameter: 44.0,
            arc_width: 5.0,
            glow: vec![(8.0, 0.10), (4.0, 0.25)],
            // The accent cyan, shared with the rest of the window.
            arc_color: crate::editor::cyan(),
            track_color: egui::Color32::from_gray(60),
            label_text_size: 12.0,
            mod_color: crate::editor::mod_pink(),
            mod_arc_width: 5.0,
            tick_width: 2.0,
        }
    }
}

impl KnobStyle {
    /// The arc's radius: just inside the knob's circle, so the arc and its glow fit.
    fn radius(&self) -> f32 {
        self.diameter / 2.0 - self.arc_width / 2.0 - 2.0
    }

    /// How far below the knob's centre the arc reaches. Because of the gap at the
    /// bottom, the arc's lowest points are its two ends, not the bottom of the circle.
    /// So the name goes under those ends instead of under the whole circle.
    fn arc_bottom(&self) -> f32 {
        self.radius() * (START_ANGLE.sin()) + self.arc_width / 2.0
    }
}

/// The arc's start angle. Angles are measured from "3 o'clock" going clockwise (because
/// screen y points down), so 135° is the bottom-left, roughly 7:30 on a clock.
const START_ANGLE: f32 = 135.0_f32 * std::f32::consts::PI / 180.0;
/// How far round the arc goes: 270°, ending at the bottom-right (4:30). That leaves the
/// usual gap at the bottom of the knob.
const SWEEP: f32 = 270.0_f32 * std::f32::consts::PI / 180.0;
/// Dragging this many points covers the whole range.
const DRAG_RANGE: f32 = 200.0;
/// Holding Shift makes dragging this many times finer.
const FINE_FACTOR: f32 = 10.0;
/// Space between the bottom of the arc (its two ends) and the top of the name.
const LABEL_GAP: f32 = 7.0;
/// How bright the pink is (0..1 of the full mod pink) on a mod target whose section is
/// switched off, in mod mode. Dim enough to read as off, bright enough to see it's usable.
const OFF_TARGET_STRENGTH: f32 = 0.55;

/// A knob for one parameter. The `'a` says it borrows the parameter and setter, which only
/// has to last for this one frame (egui rebuilds the knob every frame).
pub struct Knob<'a, P: Param> {
    param: &'a P,
    setter: &'a ParamSetter<'a>,
    label: &'a str,
    style: KnobStyle,
    /// Whether the arc grows out from the top centre instead of from the bottom-left.
    bipolar: bool,
    /// Set for knobs that are mod destinations: their arc, and what mod mode edits.
    mod_target: Option<ModTarget<'a>>,
    /// Its section is switched off (like Osc 2's knobs while Osc 2 is off). It's greyed
    /// out and can't be turned, but mod mode can still paint routes onto it.
    off: bool,
}

/// What makes a knob a mod destination.
pub struct ModTarget<'a> {
    /// Which matrix destination this knob is.
    pub dest: SlotDest,
    /// How far the routes can push it, as knob positions `(lowest, highest)` 0..1, or
    /// `None` if nothing reaches it. In mod mode, only the armed source's routes count.
    pub span: Option<(f32, f32)>,
    /// In mod mode, while a note plays: the knob position the armed source is pushing
    /// it to right now (0..1). Drawn as a pink tick riding the arc.
    pub live: Option<f32>,
    /// The matrix slots, which mod mode reads and writes.
    pub slots: &'a [ModSlotParams],
}

impl<'a, P: Param> Knob<'a, P> {
    pub fn new(param: &'a P, setter: &'a ParamSetter<'a>, label: &'a str) -> Self {
        Self {
            param,
            setter,
            label,
            style: KnobStyle::default(),
            bipolar: false,
            mod_target: None,
            off: false,
        }
    }

    /// For parameters centred on zero, like Coarse (-24 to +24). The arc starts at the
    /// top (the middle of the range) and grows left for negative values and right for
    /// positive ones, so zero shows as no arc at all.
    pub fn bipolar(mut self) -> Self {
        self.bipolar = true;
        self
    }

    /// Make this knob a mod destination: it shows its routes' arc, and mod mode can
    /// paint routes onto it.
    pub fn mod_target(mut self, target: ModTarget<'a>) -> Self {
        self.mod_target = Some(target);
        self
    }

    /// Grey the knob out while `off` is true, like `ui.add_enabled(false, ..)`, except
    /// that mod mode can still route to it. So you can set up mod on a section that's
    /// switched off, then switch it on.
    pub fn off(mut self, off: bool) -> Self {
        self.off = off;
        self
    }

    /// Use a different look than the default.
    #[allow(dead_code)] // not used yet. Here for when knobs need different looks
    pub fn with_style(mut self, style: KnobStyle) -> Self {
        self.style = style;
        self
    }

    /// Set the parameter from a 0..1 position. The DAW is told about it, so it can record
    /// automation. This must happen between a begin and end call.
    fn set_normalized(&self, normalized: f32) {
        let value = self.param.preview_plain(normalized.clamp(0.0, 1.0));
        if value != self.param.modulated_plain_value() {
            self.setter.set_parameter(self.param, value);
        }
    }

    /// The usual mouse handling: drag, scroll and double-click change the value.
    fn value_input(&self, ui: &egui::Ui, response: &mut egui::Response) {
        // While dragging, we keep our own copy of the position (0..1) in egui's memory
        // instead of re-reading the parameter each frame. The parameter may round the value
        // (e.g. to whole semitones), and re-reading it would make slow drags get stuck.
        let drag_id = response.id.with("drag_value");
        if response.drag_started() {
            self.setter.begin_set_parameter(self.param);
            let start = self.param.unmodulated_normalized_value();
            ui.data_mut(|d| d.insert_temp(drag_id, start));
        }
        if response.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let range = if fine { DRAG_RANGE * FINE_FACTOR } else { DRAG_RANGE };
            // Screen y grows downwards, so dragging up (negative y) should increase the value.
            let delta = -response.drag_delta().y / range;
            let current = ui
                .data(|d| d.get_temp::<f32>(drag_id))
                .unwrap_or_else(|| self.param.unmodulated_normalized_value());
            let next = (current + delta).clamp(0.0, 1.0);
            ui.data_mut(|d| d.insert_temp(drag_id, next));
            self.set_normalized(next);
            response.mark_changed();
        }
        if response.drag_stopped() {
            self.setter.end_set_parameter(self.param);
        }

        if response.double_clicked() {
            self.setter.begin_set_parameter(self.param);
            self.set_normalized(self.param.default_normalized_value());
            self.setter.end_set_parameter(self.param);
            response.mark_changed();
        }

        if response.hovered() && !response.dragged() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.setter.begin_set_parameter(self.param);
                let now = self.param.unmodulated_normalized_value();
                self.set_normalized(now + scroll / (DRAG_RANGE * 2.0));
                self.setter.end_set_parameter(self.param);
                response.mark_changed();
            }
        }
    }

    /// Mod mode: drag sets `source`'s route amount to this knob, adding the route in a
    /// free slot on the first move if there isn't one. Double-click removes it.
    fn paint_route(
        &self,
        ui: &egui::Ui,
        response: &mut egui::Response,
        source: ModSource,
        target: &ModTarget,
    ) {
        let slot_source = SlotSource::of(source);
        let slots = target.slots;
        let routed = |i: &usize| slots[*i].source.value() == slot_source && slots[*i].dest.value() == target.dest;
        let existing = (0..slots.len()).find(routed);
        let free = || (0..slots.len()).find(|&i| slots[i].source.value() == SlotSource::Off || slots[i].dest.value() == SlotDest::Off);

        // Like the value drag: our own copy of (slot, amount) while dragging.
        let drag_id = response.id.with("mod_paint");
        let mut drag = ui.data(|d| d.get_temp::<(usize, f32)>(drag_id));
        if response.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let range = if fine { DRAG_RANGE * FINE_FACTOR } else { DRAG_RANGE };
            let delta = -response.drag_delta().y / range;
            // Start editing on the first real movement, so a click doesn't add a route.
            if drag.is_none() && delta != 0.0 {
                drag = match existing {
                    Some(i) => {
                        self.setter.begin_set_parameter(&slots[i].amount);
                        Some((i, slots[i].amount.value()))
                    }
                    None => free().map(|i| {
                        set_once(self.setter, &slots[i].source, slot_source);
                        set_once(self.setter, &slots[i].dest, target.dest);
                        self.setter.begin_set_parameter(&slots[i].amount);
                        self.setter.set_parameter(&slots[i].amount, 0.0);
                        (i, 0.0)
                    }),
                };
            }
            if let Some((i, amount)) = drag {
                let next = (amount + delta).clamp(-1.0, 1.0);
                self.setter.set_parameter(&slots[i].amount, next);
                drag = Some((i, next));
                ui.data_mut(|d| d.insert_temp(drag_id, (i, next)));
                response.mark_changed();
            }
        }
        if response.drag_stopped() {
            if let Some((i, _)) = drag.take() {
                self.setter.end_set_parameter(&slots[i].amount);
            }
            ui.data_mut(|d| d.remove::<(usize, f32)>(drag_id));
        }

        if response.double_clicked() {
            for i in (0..slots.len()).filter(routed) {
                set_once(self.setter, &slots[i].source, SlotSource::Off);
                set_once(self.setter, &slots[i].dest, SlotDest::Off);
                set_once(self.setter, &slots[i].amount, 0.0);
            }
            response.mark_changed();
        }

        // The readout names the route, like "LFO → Cutoff  +2.0 oct".
        let amount = drag.map(|(_, a)| a).or_else(|| existing.map(|i| slots[i].amount.value()));
        let value = match amount {
            Some(a) => target.dest.format_amount(a),
            None if free().is_none() => "Matrix full".into(),
            None => "drag to add".into(),
        };
        let name = format!("{} → {}", slot_source.name(), target.dest.name());
        crate::readout::report_text(ui, &name, value, response);
    }
}

/// Change a parameter in one go, as one undo step.
fn set_once<P: Param>(setter: &ParamSetter, param: &P, value: P::Plain) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

/// Implementing egui's `Widget` trait is what lets us write `ui.add(Knob::new(...))`.
impl<P: Param> egui::Widget for Knob<'_, P> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let style = &self.style;
        // As wide as the knob or its name, whichever is wider, so long names like
        // "Bend Range" don't run into the next knob in a row.
        let label_width = ui
            .painter()
            .layout_no_wrap(
                self.label.to_string(),
                egui::FontId::proportional(style.label_text_size),
                egui::Color32::WHITE,
            )
            .size()
            .x;
        let size = egui::vec2(
            style.diameter.max(label_width),
            style.diameter / 2.0 + style.arc_bottom() + LABEL_GAP + style.label_text_size,
        );
        // In mod mode, a knob that can't be modulated sits out: dimmed, and deaf to the
        // mouse, so it's clear what dragging does right now. Outside mod mode, a knob
        // whose section is off sits out the same way.
        let armed = crate::editor::armed_source(ui);
        let inert = if armed.is_some() { self.mod_target.is_none() } else { self.off };
        let sense = if inert { egui::Sense::hover() } else { egui::Sense::click_and_drag() };
        let (rect, response) = ui.allocate_exact_size(size, sense);
        let mut response = if inert {
            response
        } else {
            response.on_hover_cursor(egui::CursorIcon::ResizeVertical)
        };

        // --- Mouse ---

        match (armed, &self.mod_target) {
            (None, _) if !inert => self.value_input(ui, &mut response),
            (None, _) => {}
            (Some(source), Some(target)) => self.paint_route(ui, &mut response, source, target),
            (Some(_), None) => {}
        }

        // --- Drawing ---

        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect.expand(16.0)); // room for the glow
            let center = egui::pos2(rect.center().x, rect.top() + style.diameter / 2.0);
            let radius = style.radius();
            let value = self.param.modulated_normalized_value();

            // Light up a little while the mouse is on it.
            let active = response.hovered() || response.dragged();
            // A greyed-out knob (like Rate while the LFO is synced), one whose section is
            // off, or one sitting out mod mode, is drawn much dimmer.
            let enabled = ui.is_enabled() && !inert && !self.off;
            let arc_color = if !enabled {
                style.arc_color.gamma_multiply(0.3)
            } else if active {
                style.arc_color
            } else {
                style.arc_color.gamma_multiply(0.85)
            };

            // Pink for the mod arc and, in mod mode, the label. A switched-off target that
            // mod mode can still paint gets a dimmer pink, so it reads as off but usable.
            let mod_color = if enabled {
                style.mod_color
            } else if armed.is_some() && !inert {
                style.mod_color.gamma_multiply(OFF_TARGET_STRENGTH)
            } else {
                style.mod_color.gamma_multiply(0.3)
            };

            // The faint full track.
            painter.add(egui::Shape::line(
                arc_points(center, radius, 0.0, 1.0),
                egui::Stroke::new(style.arc_width, style.track_color),
            ));

            // The value arc: glow layers first, the bright arc on top.
            // Where the arc starts: the beginning of the sweep, or the top for bipolar.
            let origin = if self.bipolar { 0.5 } else { 0.0 };
            if value != origin {
                let points = arc_points(center, radius, origin, value);
                for &(extra, strength) in &style.glow {
                    painter.add(egui::Shape::line(
                        points.clone(),
                        egui::Stroke::new(style.arc_width + extra, arc_color.gamma_multiply(strength)),
                    ));
                }
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(style.arc_width, arc_color),
                ));
            }

            // The mod arc: drawn along the value arc's ring, after it so it sits on top
            // (see `mod_arc_width` for how much cyan it leaves showing). Even a tiny span
            // gets a few points of arc, so a route with a small amount is still visible.
            // In mod mode it covers only the armed source (the x-ray view).
            let span = self.mod_target.as_ref().and_then(|t| t.span);
            if let Some((lo, hi)) = span {
                let min_len = 0.02;
                let (lo, hi) = if hi - lo < min_len {
                    let mid = ((lo + hi) / 2.0).clamp(min_len / 2.0, 1.0 - min_len / 2.0);
                    (mid - min_len / 2.0, mid + min_len / 2.0)
                } else {
                    (lo, hi)
                };
                painter.add(egui::Shape::line(
                    arc_points(center, radius, lo, hi),
                    egui::Stroke::new(style.mod_arc_width, mod_color),
                ));

                // The value tick: a short line straight across the ring at the knob's
                // value, a little longer than the ring is wide so it pokes out both
                // sides. Only drawn with a mod arc, since otherwise the cyan arc's end
                // already shows the value.
                let angle = START_ANGLE + SWEEP * value;
                let direction = egui::vec2(angle.cos(), angle.sin());
                let half = style.arc_width / 2.0 + 1.5;
                painter.line_segment(
                    [center + direction * (radius - half), center + direction * (radius + half)],
                    egui::Stroke::new(style.tick_width, arc_color),
                );

                // The live tick: where the armed source has the knob right now. Pink, as
                // thick as the value tick but a little longer, since on the pink arc only
                // the ends poking out past the ring show.
                let live = self.mod_target.as_ref().and_then(|t| t.live);
                if let (Some(live), true) = (live, enabled) {
                    let angle = START_ANGLE + SWEEP * live;
                    let direction = egui::vec2(angle.cos(), angle.sin());
                    let half = half + 1.5;
                    painter.line_segment(
                        [center + direction * (radius - half), center + direction * (radius + half)],
                        egui::Stroke::new(style.tick_width, mod_color),
                    );
                }
            }

            let visuals = ui.visuals();
            // The name underneath. (The value shows in the readout box instead.) In mod
            // mode a target's name turns pink, so the targets stand out across the window.
            let label_color = if armed.is_some() && !inert && ui.is_enabled() {
                mod_color
            } else if !enabled {
                visuals.weak_text_color()
            } else {
                visuals.text_color()
            };
            painter.text(
                egui::pos2(center.x, center.y + style.arc_bottom() + LABEL_GAP),
                egui::Align2::CENTER_TOP,
                self.label,
                egui::FontId::proportional(style.label_text_size),
                label_color,
            );
        }

        crate::readout::report(ui, self.param, &response);
        response
    }
}

/// Points along the arc between two positions, each 0..1 of the way round the sweep
/// (0 = bottom-left, 0.5 = top, 1 = bottom-right). `to` may be before `from`.
fn arc_points(center: egui::Pos2, radius: f32, from: f32, to: f32) -> Vec<egui::Pos2> {
    // About one point every 3° keeps the curve smooth.
    let steps = ((90.0 * (to - from).abs()).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let t = from + (to - from) * (i as f32 / steps as f32);
            let angle = START_ANGLE + SWEEP * t;
            center + radius * egui::vec2(angle.cos(), angle.sin())
        })
        .collect()
}
