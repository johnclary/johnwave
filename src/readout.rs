//! The readout box: shows the name and value of the last control you were on, whether
//! you just hovered over it or changed it. It stays put when the mouse moves away. It
//! also follows changes from outside the plugin (a MIDI controller through an Ableton
//! macro, or automation), so you can see what the knob is being turned to.
//!
//! Controls don't need to know where the box is. Each one calls `report()` with its
//! parameter, which leaves a note in egui's memory ("Gain, -12.0 dB"). The box, drawn
//! after every control, reads that note. This only reads parameters.
//!
//! Who gets the box, strongest first: the control you're dragging, then a parameter that
//! changed from outside, then the control the mouse just landed on.

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

fn write_note(ui: &egui::Ui, entry: Entry) {
    ui.data_mut(|d| d.insert_temp(note_id(), entry));
}

/// A parameter that changed this frame without the mouse on it, waiting for `draw()` to
/// decide whether to show it. `count` is how many changed this frame.
#[derive(Clone, Default)]
struct Pending {
    entry: Option<Entry>,
    count: u32,
}

fn pending_id() -> egui::Id {
    egui::Id::unique("readout_pending")
}

/// True on the frame the mouse lands on a control (not on every frame it rests there),
/// so a resting mouse doesn't keep taking the box back from an outside change.
fn hover_began(ui: &egui::Ui, response: &egui::Response) -> bool {
    let id = response.id.with("readout_hovered");
    let hovered = response.hovered();
    let was = ui.data_mut(|d| {
        let was = d.get_temp::<bool>(id).unwrap_or(false);
        d.insert_temp(id, hovered);
        was
    });
    hovered && !was
}

/// Call right after drawing a control, with the `Response` egui gave back for it.
pub fn report<P: Param>(ui: &egui::Ui, param: &P, response: &egui::Response) {
    // The value last frame, kept under the parameter's own address so it stays the same
    // even if the layout shifts. Unmodulated, so host modulation doesn't count as a move.
    let seen_id = egui::Id::unique(("readout_seen", param as *const P as usize));
    let value = param.unmodulated_normalized_value();
    let last = ui.data_mut(|d| {
        let last = d.get_temp::<f32>(seen_id);
        d.insert_temp(seen_id, value);
        last
    });
    // Nothing to compare with on the first frame, so opening the window isn't a change.
    let changed = last.is_some_and(|l| l != value);

    let began = hover_began(ui, response);
    if response.dragged() || began || (changed && response.hovered()) {
        write_note(ui, Entry::of(param));
    } else if changed {
        // Moved from outside. `draw()` shows it if it was the only one.
        let entry = Entry::of(param);
        ui.data_mut(|d| {
            let pending = d.get_temp_mut_or_default::<Pending>(pending_id());
            pending.entry = Some(entry);
            pending.count += 1;
        });
    }
}

/// Like `report`, but with the value already written out, for controls whose value
/// reads better in some other unit than the parameter's own.
pub fn report_text(ui: &egui::Ui, name: &str, value: String, response: &egui::Response) {
    // Rewritten while hovered only when the text changes (a click flips a chip's hint).
    let text_id = response.id.with("readout_text");
    let changed = ui.data_mut(|d| {
        let changed = d.get_temp::<String>(text_id).as_ref() != Some(&value);
        d.insert_temp(text_id, value.clone());
        changed
    });
    let began = hover_began(ui, response);
    if response.dragged() || began || (changed && response.hovered()) {
        write_note(ui, Entry { name: name.to_string(), value });
    }
}

/// Space between the box's edges and its text.
const PADDING: f32 = 10.0;

/// Draw the box into `rect` (space the window set aside for it). Call this after every
/// control has been drawn this frame, so it shows what they reported.
pub fn draw(ui: &egui::Ui, rect: egui::Rect) {
    // One parameter moved from outside: show it. Many at once is a preset or project
    // loading, so leave the box alone rather than jump to whichever came last.
    let pending = ui.data_mut(|d| d.remove_temp::<Pending>(pending_id())).unwrap_or_default();
    if let (Some(entry), 1) = (pending.entry, pending.count) {
        write_note(ui, entry);
    }

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
