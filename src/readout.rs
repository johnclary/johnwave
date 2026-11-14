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

const HEIGHT: f32 = 48.0;

/// Draw the box, filling the available width. Call this after every control has been
/// drawn this frame, so it sees what they reported.
pub fn show(ui: &mut egui::Ui) {
    let entry = ui.data(|d| d.get_temp::<Entry>(note_id()));

    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();

    painter.rect_stroke(
        rect.shrink(0.5),
        4.0,
        egui::Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.5)),
        egui::StrokeKind::Inside,
    );

    let (name, value) = match &entry {
        Some(e) => (e.name.as_str(), e.value.as_str()),
        None => ("", "—"),
    };
    let left = rect.left() + 12.0;
    painter.text(
        egui::pos2(left, rect.top() + 8.0),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::proportional(11.0),
        visuals.weak_text_color(),
    );
    painter.text(
        egui::pos2(left, rect.bottom() - 7.0),
        egui::Align2::LEFT_BOTTOM,
        value,
        egui::FontId::proportional(17.0),
        visuals.strong_text_color(),
    );
}
