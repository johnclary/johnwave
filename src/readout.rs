//! The readout box: shows the name and value of the last control you were on, whether
//! you just hovered over it or changed it. It stays put when the mouse moves away.
//!
//! Controls don't need to know where the box is. Each one calls `report()` with its
//! parameter, which leaves a note in egui's memory ("Gain, -12.0 dB"). The box, drawn
//! after every control, reads that note. This only reads parameters.

use nice_plug::prelude::Param;

/// One parameter's name and value, already turned into text.
#[derive(Clone, Default)]
struct Entry {
    name: String,
    value: String,
}

impl Entry {
    fn of<P: Param>(param: &P) -> Self {
        Self {
            name: param.name().to_string(),
            value: param.normalized_value_to_string(param.modulated_normalized_value(), true),
        }
    }
}

/// Where the note lives in egui's memory: a fixed name that is the same in every frame.
fn note_id() -> egui::Id {
    egui::Id::unique("readout_last")
}

/// Call right after drawing a control, with the `Response` egui gave back for it.
pub fn report<P: Param>(ui: &egui::Ui, param: &P, response: &egui::Response) {
    // Rewritten every frame the mouse is on it, so the value stays live while you drag.
    if response.hovered() || response.dragged() {
        let entry = Entry::of(param);
        ui.data_mut(|d| d.insert_temp(note_id(), entry));
    }
}

/// Like `report`, but with the value already written out, for controls whose value
/// reads better in some other unit than the parameter's own.
pub fn report_text(ui: &egui::Ui, name: &str, value: String, response: &egui::Response) {
    if response.hovered() || response.dragged() {
        let entry = Entry { name: name.to_string(), value };
        ui.data_mut(|d| d.insert_temp(note_id(), entry));
    }
}

/// Space between the box's edges and its text.
const PADDING: f32 = 10.0;

/// Draw the box into `rect` (space the window set aside for it). Call this after every
/// control has been drawn this frame, so it shows what they reported.
pub fn draw(ui: &egui::Ui, rect: egui::Rect) {
    let entry = ui.data(|d| d.get_temp::<Entry>(note_id()));
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();

    crate::editor::screen_frame(
        &painter,
        rect,
        visuals.weak_text_color().gamma_multiply(0.5),
    );

    let (name, value) = match &entry {
        Some(e) => (e.name.to_uppercase(), e.value.as_str()),
        None => (String::new(), "—"),
    };
    // One line: the name small and faint on the left, the value big on the right.
    painter.text(
        egui::pos2(rect.left() + PADDING, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(11.0),
        visuals.weak_text_color(),
    );
    // Monospaced, so the digits don't shuffle sideways while you drag. Cyan, like the
    // knob arcs.
    painter.text(
        egui::pos2(rect.right() - PADDING, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        value,
        egui::FontId::monospace(18.0),
        crate::editor::cyan(),
    );
}
