//! A thin horizontal neon slider for any parameter: the knob's look, laid flat.
//!
//! - drag left/right to change the value (hold Shift for fine control)
//! - double-click to reset it to its default
//!
//! It has no label or value text of its own. Its name and value show in the readout box
//! while you're on it. It only reads and sets the parameter.

use nice_plug::prelude::{Param, ParamSetter};

/// Height of the whole slider (the part you can grab), and of its bright bar.
const HEIGHT: f32 = 18.0;
const BAR_WIDTH: f32 = 4.0;
/// Glow layers under the bar: (extra thickness, strength 0..1), like the knob arcs.
const GLOW: [(f32, f32); 2] = [(6.0, 0.10), (3.0, 0.25)];
/// Holding Shift makes dragging this many times finer.
const FINE_FACTOR: f32 = 10.0;

pub struct Slider<'a, P: Param> {
    param: &'a P,
    setter: &'a ParamSetter<'a>,
    width: f32,
    /// Whether the bar grows out from the middle instead of from the left.
    bipolar: bool,
    /// What the readout shows as the value, instead of the parameter's own text.
    readout_value: Option<String>,
}

impl<'a, P: Param> Slider<'a, P> {
    pub fn new(param: &'a P, setter: &'a ParamSetter<'a>, width: f32) -> Self {
        Self {
            param,
            setter,
            width,
            bipolar: false,
            readout_value: None,
        }
    }

    /// For parameters centred on zero: the bar grows left or right from the middle.
    pub fn bipolar(mut self) -> Self {
        self.bipolar = true;
        self
    }

    /// Show `text` as the value in the readout (e.g. "+2.0 oct" for a mod amount whose
    /// unit depends on another setting).
    pub fn readout_value(mut self, text: String) -> Self {
        self.readout_value = Some(text);
        self
    }

    /// Set the parameter from a 0..1 position. Must happen between a begin and end call.
    fn set_normalized(&self, normalized: f32) {
        let value = self.param.preview_plain(normalized.clamp(0.0, 1.0));
        if value != self.param.modulated_plain_value() {
            self.setter.set_parameter(self.param, value);
        }
    }
}

impl<P: Param> egui::Widget for Slider<'_, P> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(self.width, HEIGHT), egui::Sense::click_and_drag());
        let mut response = response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);

        // --- Mouse --- (the same approach as the knob: keep our own 0..1 copy while
        // dragging, so values the parameter rounds don't make slow drags stick)
        let drag_id = response.id.with("drag_value");
        if response.drag_started() {
            self.setter.begin_set_parameter(self.param);
            let start = self.param.unmodulated_normalized_value();
            ui.data_mut(|d| d.insert_temp(drag_id, start));
        }
        if response.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let range = if fine { self.width * FINE_FACTOR } else { self.width };
            let current = ui
                .data(|d| d.get_temp::<f32>(drag_id))
                .unwrap_or_else(|| self.param.unmodulated_normalized_value());
            let next = (current + response.drag_delta().x / range).clamp(0.0, 1.0);
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

        // --- Drawing ---
        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect.expand(8.0)); // room for the glow
            let active = response.hovered() || response.dragged();
            let cyan = crate::editor::cyan();
            let color = if !ui.is_enabled() {
                cyan.gamma_multiply(0.3)
            } else if active {
                cyan
            } else {
                cyan.gamma_multiply(0.85)
            };

            // Leave room at the ends so the round caps of the bar fit.
            let left = rect.left() + BAR_WIDTH;
            let right = rect.right() - BAR_WIDTH;
            let y = rect.center().y;
            let x_at = |t: f32| left + (right - left) * t;

            // The faint full track, and a small tick in the middle for bipolar.
            let track = egui::Color32::from_gray(60);
            painter.line_segment(
                [egui::pos2(left, y), egui::pos2(right, y)],
                egui::Stroke::new(BAR_WIDTH, track),
            );
            if self.bipolar {
                let mid = x_at(0.5);
                painter.line_segment(
                    [egui::pos2(mid, y - 5.0), egui::pos2(mid, y + 5.0)],
                    egui::Stroke::new(1.0, track),
                );
            }

            // The value bar, glow first.
            let origin = if self.bipolar { 0.5 } else { 0.0 };
            let value = self.param.modulated_normalized_value();
            if value != origin {
                let ends = [egui::pos2(x_at(origin), y), egui::pos2(x_at(value), y)];
                for (extra, strength) in GLOW {
                    painter.line_segment(
                        ends,
                        egui::Stroke::new(BAR_WIDTH + extra, color.gamma_multiply(strength)),
                    );
                }
                painter.line_segment(ends, egui::Stroke::new(BAR_WIDTH, color));
            }
        }

        match self.readout_value {
            Some(text) => crate::readout::report_text(ui, self.param.name(), text, &response),
            None => crate::readout::report(ui, self.param, &response),
        }
        response
    }
}
