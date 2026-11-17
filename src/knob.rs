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
//! It only reads and sets the parameter. Nothing else.

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
    /// Colour of the mod arc: the thin stripe along the value arc's ring showing how far
    /// the mod matrix can push this knob.
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
            // A dusky pink, far enough round the colour wheel from the cyan to stand
            // apart: #C95DA7, which is 319°, 54%, 79% in a normal colour picker.
            mod_color: egui::Color32::from_rgb(0xC9, 0x5D, 0xA7),
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

/// A knob for one parameter. The `'a` says it borrows the parameter and setter, which only
/// has to last for this one frame (egui rebuilds the knob every frame).
pub struct Knob<'a, P: Param> {
    param: &'a P,
    setter: &'a ParamSetter<'a>,
    label: &'a str,
    style: KnobStyle,
    /// Whether the arc grows out from the top centre instead of from the bottom-left.
    bipolar: bool,
    /// How far the mod matrix can push this knob, as `(lowest, highest)` positions 0..1.
    mod_span: Option<(f32, f32)>,
}

impl<'a, P: Param> Knob<'a, P> {
    pub fn new(param: &'a P, setter: &'a ParamSetter<'a>, label: &'a str) -> Self {
        Self {
            param,
            setter,
            label,
            style: KnobStyle::default(),
            bipolar: false,
            mod_span: None,
        }
    }

    /// For parameters centred on zero, like Coarse (-24 to +24). The arc starts at the
    /// top (the middle of the range) and grows left for negative values and right for
    /// positive ones, so zero shows as no arc at all.
    pub fn bipolar(mut self) -> Self {
        self.bipolar = true;
        self
    }

    /// Show how far the mod matrix can push this knob: a thin arc between two positions
    /// 0..1 on the knob. `None` (nothing routed here) draws no arc.
    pub fn mod_span(mut self, span: Option<(f32, f32)>) -> Self {
        self.mod_span = span;
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
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        let mut response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);

        // --- Mouse ---

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

        // --- Drawing ---

        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect.expand(16.0)); // room for the glow
            let center = egui::pos2(rect.center().x, rect.top() + style.diameter / 2.0);
            let radius = style.radius();
            let value = self.param.modulated_normalized_value();

            // Light up a little while the mouse is on it.
            let active = response.hovered() || response.dragged();
            // A greyed-out knob (like Rate while the LFO is synced) is drawn much dimmer.
            let enabled = ui.is_enabled();
            let arc_color = if !enabled {
                style.arc_color.gamma_multiply(0.3)
            } else if active {
                style.arc_color
            } else {
                style.arc_color.gamma_multiply(0.85)
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
            if let Some((lo, hi)) = self.mod_span {
                let min_len = 0.02;
                let (lo, hi) = if hi - lo < min_len {
                    let mid = ((lo + hi) / 2.0).clamp(min_len / 2.0, 1.0 - min_len / 2.0);
                    (mid - min_len / 2.0, mid + min_len / 2.0)
                } else {
                    (lo, hi)
                };
                let mod_color = if enabled {
                    style.mod_color
                } else {
                    style.mod_color.gamma_multiply(0.3)
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
            }

            let visuals = ui.visuals();
            // The name underneath. (The value shows in the readout box instead.)
            painter.text(
                egui::pos2(center.x, center.y + style.arc_bottom() + LABEL_GAP),
                egui::Align2::CENTER_TOP,
                self.label,
                egui::FontId::proportional(style.label_text_size),
                if enabled {
                    visuals.text_color()
                } else {
                    visuals.weak_text_color()
                },
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
