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
//! matrix slot if there isn't one). To remove it, click the knob's name (it reads
//! "Remove" while the mouse is on the knob), then click again to confirm. Knobs that
//! can't be modulated dim and ignore the mouse until mod mode ends.
//!
//! It only reads and sets parameters: its own, and in mod mode the matrix slots.

use crate::modulation::{ModSlotParams, ModSource, SlotDest, SlotSource};
use crate::editor::{ARC_OFF, ARC_RESTING, MOD_ARC_OFF_TARGET, MOD_OUTLINE, SELECTED_FILL};
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
    /// Colour of the mod arc: the outer ring showing how far the mod matrix can push
    /// this knob.
    pub mod_color: egui::Color32,
    /// Thickness of the mod arc. It's its own ring just outside the value arc, so it
    /// never covers it.
    pub mod_arc_width: f32,
    /// Space between the value arc and the mod arc's ring.
    pub mod_ring_gap: f32,
    /// Radius of the mod dot in the middle of a knob that's a mod target.
    pub mod_dot_radius: f32,
    /// How strong the mod slice is (0..1 of the mod colour): the faint wedge from the
    /// mod dot out to the mod ring, covering the same range. `.0` at the centre, `.1`
    /// at the ring, so it brightens toward the ring like light spilling inward.
    pub mod_fill: (f32, f32),
    /// Thickness of the live tick: the short mod-colour line marking where the armed
    /// source is pushing the knob right now.
    pub tick_width: f32,
}

impl Default for KnobStyle {
    fn default() -> Self {
        Self {
            diameter: 44.0,
            arc_width: 5.0,
            glow: vec![(8.0, 0.10), (4.0, 0.25)],
            // The accent colour, shared with the rest of the window.
            arc_color: crate::editor::accent(),
            track_color: crate::editor::KNOB_TRACK,
            label_text_size: 12.0,
            mod_color: crate::editor::MOD_COLOR,
            mod_arc_width: 2.5,
            mod_ring_gap: 1.0,
            mod_dot_radius: 1.0,
            mod_fill: (0.1, 0.22),
            tick_width: 1.0,
        }
    }
}

impl KnobStyle {
    /// The value arc's radius. Worked out from the outside in: the mod ring's outer
    /// edge reaches `RING_OVERHANG` past the knob's circle, and the value arc sits
    /// inside that.
    fn radius(&self) -> f32 {
        self.diameter / 2.0 + RING_OVERHANG - self.mod_arc_width - self.mod_ring_gap - self.arc_width / 2.0
    }

    /// The mod ring's radius: just outside the value arc, past the gap.
    fn mod_radius(&self) -> f32 {
        self.radius() + self.arc_width / 2.0 + self.mod_ring_gap + self.mod_arc_width / 2.0
    }

    /// How far below the knob's centre the rings reach. Because of the gap at the
    /// bottom, their lowest points are their ends, not the bottom of the circle. So the
    /// name goes under the mod ring's ends instead of under the whole circle.
    fn arc_bottom(&self) -> f32 {
        self.mod_radius() * (START_ANGLE.sin()) + self.mod_arc_width / 2.0
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
const LABEL_GAP: f32 = 5.25;
/// How far the mod ring may poke past the knob's square, in points. A little overhang
/// lets the value arc stay near its old size without making the knob bigger.
const RING_OVERHANG: f32 = 1.0;
/// How long the remove button waits at "Sure?" for the second click, in seconds.
const CONFIRM_TIME: f64 = 2.0;
/// Room between a mod target's name and its outline in mod mode (x, y).
const LABEL_OUTLINE_PAD: egui::Vec2 = egui::vec2(4.0, 1.0);

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
    /// it to right now (0..1). Drawn as a mod-colour tick riding the arc.
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
    ) -> LabelMode {
        let slot_source = SlotSource::of(source);
        let slots = target.slots;
        let routed = |i: &usize| slots[*i].source.value() == slot_source && slots[*i].dest.value() == target.dest;
        let existing = (0..slots.len()).find(routed);
        // A slot is free if it's switched off, or its amount was dragged back to zero
        // (it does nothing, and there's no remove button for it since it shows no arc).
        let free = || {
            (0..slots.len()).find(|&i| {
                slots[i].source.value() == SlotSource::Off
                    || slots[i].dest.value() == SlotDest::Off
                    || slots[i].amount.value() == 0.0
            })
        };

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

        // Removing takes two clicks on the name, so a stray click mid-performance can't
        // lose a route. While the mouse is on a knob this source reaches, its name reads
        // "Remove". Clicking that lights it up as "Sure?", and clicking again removes the
        // route. Moving off the knob, dragging it, or waiting `CONFIRM_TIME` cancels.
        let removable = existing.is_some() && target.span.is_some();
        let confirm_id = response.id.with("remove_confirm");
        let now = ui.input(|i| i.time);
        let confirm_since = ui
            .data(|d| d.get_temp::<(ModSource, f64)>(confirm_id))
            .filter(|&(s, since)| s == source && now - since < CONFIRM_TIME)
            .map(|(_, since)| since);
        let on_label = removable
            && response.hover_pos().is_some_and(|p| label_row(&self.style, response.rect).contains(p));
        let mut mode = LabelMode::Name;
        if removable && response.hovered() && !response.dragged() {
            mode = if confirm_since.is_some() { LabelMode::Confirm } else { LabelMode::Remove };
            if on_label {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
        if response.clicked() && on_label {
            if confirm_since.is_some() {
                for i in (0..slots.len()).filter(routed) {
                    set_once(self.setter, &slots[i].source, SlotSource::Off);
                    set_once(self.setter, &slots[i].dest, SlotDest::Off);
                    set_once(self.setter, &slots[i].amount, 0.0);
                }
                ui.data_mut(|d| d.remove::<(ModSource, f64)>(confirm_id));
                response.mark_changed();
                mode = LabelMode::Name;
            } else {
                ui.data_mut(|d| d.insert_temp(confirm_id, (source, now)));
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(CONFIRM_TIME));
                mode = LabelMode::Confirm;
            }
        } else if confirm_since.is_some() && (!response.hovered() || response.dragged() || response.clicked()) {
            ui.data_mut(|d| d.remove::<(ModSource, f64)>(confirm_id));
            mode = LabelMode::Name;
        } else if let Some(since) = confirm_since {
            // Draw a frame when the time runs out, so "Sure?" goes away by itself.
            let left = (CONFIRM_TIME - (now - since)).max(0.0);
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(left));
        }

        // The readout names the route, like "LFO > Cutoff  +2.0 oct".
        let amount = drag.map(|(_, a)| a).or_else(|| existing.map(|i| slots[i].amount.value()));
        let value = match amount {
            Some(a) => target.dest.format_amount(a),
            None if free().is_none() => "Matrix full".into(),
            None => "drag to add".into(),
        };
        let value = if mode == LabelMode::Confirm { "Click again to remove".into() } else { value };
        let name = format!("{} > {}", slot_source.name(), target.dest.name());
        crate::readout::report_text(ui, &name, value, response);
        mode
    }
}

/// What a knob's name shows. Only mod mode's remove button changes it.
#[derive(Clone, Copy, PartialEq)]
enum LabelMode {
    /// The knob's own name.
    Name,
    /// "Remove": the mouse is on a knob the armed source reaches.
    Remove,
    /// "Sure?": clicked once; click again to remove the route.
    Confirm,
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

        let label_mode = match (armed, &self.mod_target) {
            (None, _) if !inert => {
                self.value_input(ui, &mut response);
                LabelMode::Name
            }
            (Some(source), Some(target)) => self.paint_route(ui, &mut response, source, target),
            _ => LabelMode::Name,
        };

        // --- Drawing ---

        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect.expand(16.0)); // room for the glow
            let center = knob_center(style, rect);
            let radius = style.radius();
            let value = self.param.modulated_normalized_value();

            // Light up a little while the mouse is on it.
            let active = response.hovered() || response.dragged();
            // A greyed-out knob (like Rate while the LFO is synced), one whose section is
            // off, or one sitting out mod mode, is drawn much dimmer.
            let enabled = ui.is_enabled() && !inert && !self.off;
            let arc_color = if !enabled {
                style.arc_color.gamma_multiply(ARC_OFF)
            } else if active {
                style.arc_color
            } else {
                style.arc_color.gamma_multiply(ARC_RESTING)
            };

            // The mod colour for the mod arc and, in mod mode, the label. A switched-off target
            // that mod mode can still paint gets it dimmer, so it reads as off but usable.
            let mod_color = if enabled {
                style.mod_color
            } else if armed.is_some() && !inert {
                style.mod_color.gamma_multiply(MOD_ARC_OFF_TARGET)
            } else {
                style.mod_color.gamma_multiply(ARC_OFF)
            };

            // The mod range: the routes' span, widened to a few points of arc if it's
            // tiny, so a route with a small amount is still visible. In mod mode it
            // covers only the armed source (the x-ray view).
            let span = self.mod_target.as_ref().and_then(|t| t.span).map(|(lo, hi)| {
                let min_len = 0.02;
                if hi - lo < min_len {
                    let mid = ((lo + hi) / 2.0).clamp(min_len / 2.0, 1.0 - min_len / 2.0);
                    (mid - min_len / 2.0, mid + min_len / 2.0)
                } else {
                    (lo, hi)
                }
            });

            // The mod slice first, underneath everything: a faint wedge from the centre
            // out to the mod ring over the same range.
            if let Some((lo, hi)) = span {
                let (inner, outer) = style.mod_fill;
                painter.add(slice_mesh(
                    center,
                    style.mod_radius(),
                    lo,
                    hi,
                    mod_color.gamma_multiply(inner),
                    mod_color.gamma_multiply(outer),
                ));
            }

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

            // The mod arc: its own ring just outside the value arc, drawn after the value
            // arc's glow so it sits cleanly on top of it.
            if let Some((lo, hi)) = span {
                painter.add(egui::Shape::line(
                    arc_points(center, style.mod_radius(), lo, hi),
                    egui::Stroke::new(style.mod_arc_width, mod_color),
                ));

                // The mod dot: a mod-colour dot in the middle, so a targeted knob is easy to
                // spot even when its amount (and so its mod arc) is tiny.
                painter.circle_filled(center, style.mod_dot_radius, mod_color);

                // The live tick: where the armed source has the knob right now. A thin
                // mod-colour line like a clock hand, from the edge of the mod dot out to
                // the outside of the value arc, so it never cuts across the mod ring.
                let live = self.mod_target.as_ref().and_then(|t| t.live);
                if let (Some(live), true) = (live, enabled) {
                    let angle = START_ANGLE + SWEEP * live;
                    let direction = egui::vec2(angle.cos(), angle.sin());
                    let outer = radius + style.arc_width / 2.0;
                    let inner = style.mod_dot_radius;
                    painter.line_segment(
                        [center + direction * inner, center + direction * outer],
                        egui::Stroke::new(style.tick_width, mod_color),
                    );
                }
            }

            let visuals = ui.visuals();
            // The name underneath. (The value shows in the readout box instead.) In mod
            // mode a target's name turns the mod colour, so the targets stand out across the window.
            // The remove button (see `paint_route`) swaps the name for "Remove", then for
            // a lit "Sure?" chip in dark text.
            let (label_text, label_color) = match label_mode {
                LabelMode::Confirm => ("Sure?", crate::editor::BACKGROUND),
                LabelMode::Remove => ("Remove", style.mod_color),
                LabelMode::Name if armed.is_some() && !inert && ui.is_enabled() => (self.label, mod_color),
                LabelMode::Name if !enabled => (self.label, visuals.weak_text_color()),
                LabelMode::Name => (self.label, visuals.text_color()),
            };
            let galley = painter.layout_no_wrap(
                label_text.to_string(),
                egui::FontId::proportional(style.label_text_size),
                label_color,
            );
            let label_pos = egui::pos2(center.x, center.y + style.arc_bottom() + LABEL_GAP);
            let label_rect = egui::Align2::CENTER_TOP.anchor_size(label_pos, galley.size());
            // The chip round the name: at least as wide as "Remove", so every mod mode
            // target's chip lines up, and it doesn't jump between the name, "Remove" and
            // "Sure?".
            let remove_width = painter
                .layout_no_wrap("Remove".into(), egui::FontId::proportional(style.label_text_size), label_color)
                .size()
                .x;
            let chip = egui::Align2::CENTER_TOP
                .anchor_size(label_pos, egui::vec2(galley.size().x.max(remove_width), galley.size().y))
                .expand2(LABEL_OUTLINE_PAD);
            match label_mode {
                LabelMode::Confirm => {
                    painter.rect_filled(chip, 3.0, style.mod_color);
                }
                // A faint fill while the mouse is right on it, like the chips in the header.
                LabelMode::Remove if response.hover_pos().is_some_and(|p| label_row(style, rect).contains(p)) => {
                    painter.rect_filled(chip, 3.0, style.mod_color.gamma_multiply(SELECTED_FILL));
                }
                _ => {}
            }
            painter.galley(label_rect.min, galley, label_color);
            // In mod mode, every target gets a thin mod-colour outline round its name, like an
            // unpressed chip, so you can see at a glance where the armed source can go.
            if armed.is_some() && !inert && ui.is_enabled() && label_mode != LabelMode::Confirm {
                painter.rect_stroke(
                    chip,
                    3.0,
                    egui::Stroke::new(1.0, mod_color.gamma_multiply(MOD_OUTLINE)),
                    egui::StrokeKind::Outside,
                );
            }
        }

        crate::readout::report(ui, self.param, &response);
        response
    }
}

/// The middle of the knob's circle, for a knob drawn in `rect`.
fn knob_center(style: &KnobStyle, rect: egui::Rect) -> egui::Pos2 {
    egui::pos2(rect.center().x, rect.top() + style.diameter / 2.0)
}

/// The strip at the bottom of a knob drawn in `rect` where its name sits: what mod
/// mode's remove button listens to.
fn label_row(style: &KnobStyle, rect: egui::Rect) -> egui::Rect {
    let top = knob_center(style, rect).y + style.arc_bottom() + LABEL_GAP - LABEL_OUTLINE_PAD.y;
    egui::Rect::from_min_max(egui::pos2(rect.left(), top), rect.max)
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

/// A filled wedge from `center` out to `radius`, between two positions round the sweep
/// (like `arc_points`), coloured `inner` at the centre blending to `outer` at the edge.
/// It's a fan of triangles that all share the centre, so it works for any width, even
/// past half the circle where egui's own polygon fill would go wrong.
fn slice_mesh(
    center: egui::Pos2,
    radius: f32,
    from: f32,
    to: f32,
    inner: egui::Color32,
    outer: egui::Color32,
) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, inner);
    for point in arc_points(center, radius, from, to) {
        mesh.colored_vertex(point, outer);
    }
    // Vertex 0 is the centre; each triangle joins it to two neighbours along the edge.
    for i in 1..mesh.vertices.len() as u32 - 1 {
        mesh.add_triangle(0, i, i + 1);
    }
    mesh
}
