//! The plugin window: a deliberately minimal egui UI.
//!
//! egui is "immediate mode": every frame, `ui()` describes the whole window from scratch
//! ("a title, then these sections of knobs"). egui works out the layout and handles the mouse. There's
//! no widget tree to keep in sync. The window only reads and sets parameters. No sound
//! logic lives here.

use crate::knob::Knob;
use crate::{SynthParams, VoiceMode};
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::{AtomicF32, BoolParam, Enum, EnumParam, Param, ParamSetter};
use std::sync::atomic::Ordering;
use nice_plug_egui::{EguiEditorState, Frame, NiceEguiApp};
use std::sync::Arc;

/// Window size in points. On a Retina screen egui draws at twice this many pixels.
pub const WIDTH: f64 = MARGIN_X as f64 * 2.0 + COLUMN_WIDTH as f64 * 2.0 + COLUMN_GAP as f64;
pub const HEIGHT: f64 = 640.0;

pub fn default_state() -> Arc<EguiEditorState> {
    EguiEditorState::from_size(LogicalSize::new(WIDTH, HEIGHT), 1.0)
}

pub struct JohnwaveEditor {
    /// The same parameters the audio thread reads. `Arc` means both sides share one copy.
    params: Arc<SynthParams>,
    /// How the window talks to the host (to change parameters, with undo and automation).
    /// It only exists while the window is open, so it starts as `None`.
    gui_context: Option<GuiContext>,
    /// The Morph being played right now (including LFO and Mod Env), set by the audio
    /// thread. The waveform draws this.
    display_morph: Arc<AtomicF32>,
}

impl JohnwaveEditor {
    pub fn new(params: Arc<SynthParams>, display_morph: Arc<AtomicF32>) -> Self {
        Self {
            params,
            gui_context: None,
            display_morph,
        }
    }
}

impl NiceEguiApp for JohnwaveEditor {
    /// Called each time the window opens.
    fn build(
        &mut self,
        _egui_ctx: egui::Context,
        nice_gui_ctx: GuiContext,
        _frame: &mut Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        self.gui_context = Some(nice_gui_ctx);
        Ok(())
    }

    fn editor_closed(&mut self) {
        self.gui_context = None;
    }

    /// Called every frame: describe the whole window.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        let Some(gui_context) = &self.gui_context else {
            return;
        };
        // A `ParamSetter` turns clicks and drags into parameter changes the DAW can see
        // and record as automation.
        let setter = gui_context.param_setter();

        // Padding around the edges of the window.
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(MARGIN_X as i8, 12))
            .show(ui, |ui| self.contents(ui, &setter));
    }
}

impl JohnwaveEditor {
    fn contents(&self, ui: &mut egui::Ui, setter: &ParamSetter) {
        let p = &self.params;

        ui.heading("johnwave");
        ui.add_space(6.0);
        waveform(ui, self.display_morph.load(Ordering::Relaxed));
        ui.add_space(2.0);

        // Two fixed-width columns side by side, so the layout never shifts as things
        // change.
        ui.horizontal_top(|ui| {
            column(ui, |ui| {
                section(ui, "Voice", |ui| {
                    ui.label("Mode");
                    voice_mode_button(ui, &p.voice_mode, &setter);
                    ui.end_row();
                });
                ui.add_space(6.0);
                ui.add(Knob::new(&p.gain, &setter, "Gain"));
                knob_section(ui, "Oscillator", |ui| {
                    ui.add(Knob::new(&p.morph, &setter, "Shape"));
                });
                knob_section(ui, "Tuning", |ui| {
                    ui.add(Knob::new(&p.coarse, &setter, "Coarse").bipolar());
                    ui.add(Knob::new(&p.fine, &setter, "Fine").bipolar());
                    ui.add(Knob::new(&p.bend_range, &setter, "Bend"));
                });
                knob_section(ui, "Filter", |ui| {
                    ui.add(Knob::new(&p.cutoff, &setter, "Cutoff"));
                    ui.add(Knob::new(&p.resonance, &setter, "Res"));
                });
                knob_section(ui, "Amp Envelope", |ui| {
                    ui.add(Knob::new(&p.attack, &setter, "A"));
                    ui.add(Knob::new(&p.decay, &setter, "D"));
                    ui.add(Knob::new(&p.sustain, &setter, "S"));
                    ui.add(Knob::new(&p.release, &setter, "R"));
                });
            });
            ui.add_space(COLUMN_GAP);
            column(ui, |ui| {
                knob_section(ui, "Mod Envelope", |ui| {
                    ui.add(Knob::new(&p.mod_env_attack, &setter, "A"));
                    ui.add(Knob::new(&p.mod_env_decay, &setter, "D"));
                    // The amounts can push either way, so they're bipolar.
                    ui.add(Knob::new(&p.mod_env_to_morph, &setter, "To Shape").bipolar());
                    ui.add(Knob::new(&p.mod_env_to_cutoff, &setter, "To Cutoff").bipolar());
                });
                // Only one of Rate / Sync Rate is in charge, so grey out the other.
                let synced = p.lfo_sync.value();
                let lfo_shape = p.lfo_shape.value();
                section_heading(ui, "LFO", |ui| {
                    // A tiny copy of the LFO's current shape, next to the title.
                    let wave = |phase| crate::lfo::wave(phase, lfo_shape, MINI_WAVE_EDGE);
                    mini_wave(ui, wave, neon_color(lfo_shape));
                });
                knob_row(ui, |ui| {
                    ui.add(Knob::new(&p.lfo_shape, &setter, "Shape"));
                    ui.add_enabled(!synced, Knob::new(&p.lfo_rate, &setter, "Rate"));
                    ui.add(Knob::new(&p.lfo_to_morph, &setter, "To Shape"));
                    // Cutoff can push either way, so it's bipolar.
                    ui.add(Knob::new(&p.lfo_to_cutoff, &setter, "To Cutoff").bipolar());
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    toggle_button(ui, "Sync", &p.lfo_sync, &setter);
                    toggle_button(ui, "Retrigger", &p.lfo_retrigger, &setter);
                });
                ui.horizontal(|ui| {
                    ui.label("Sync Rate");
                    ui.add_enabled_ui(synced, |ui| stepper(ui, &p.lfo_division, &setter));
                });
                // The readout box sits at the bottom of this column. It's drawn last so
                // it sees which control the mouse is on this frame.
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                    crate::readout::show(ui);
                });
            });
        });
    }
}

/// How tall the waveform strip is.
const WAVE_HEIGHT: f32 = 64.0;
/// How many cycles of the wave to draw.
const WAVE_CYCLES: f32 = 2.0;

/// The oscillator's current shape, drawn across the full width. It uses the same
/// `osc::shape` function the sound comes from, so it shows the real shape and its real
/// level (the square looks smaller because it's scaled to sound as loud as the others).
fn waveform(ui: &mut egui::Ui, morph: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, WAVE_HEIGHT), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // A faint centre line at 0.
    painter.line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.4)),
    );

    let wave = |phase| crate::osc::shape(phase, 0.0, morph);
    wave_line(&painter, rect, WAVE_CYCLES, wave, neon_color(morph), 6.0);
}

/// Size of the little waves next to section titles.
const MINI_WAVE_SIZE: egui::Vec2 = egui::vec2(48.0, 22.0);
/// The mini LFO wave draws its square with a tiny ramp on each edge, like the real one.
const MINI_WAVE_EDGE: f32 = 0.001;

/// One cycle of a wave, small, for beside a section title.
fn mini_wave(ui: &mut egui::Ui, wave: impl Fn(f32) -> f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(MINI_WAVE_SIZE, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    wave_line(&painter, rect, 1.0, wave, color, 1.5);
}

/// Draw `wave` (phase 0..1 in, -1..1 out) across `rect` as a neon line `thickness` wide.
/// Shared by the big waveform and the mini ones so they look alike.
fn wave_line(
    painter: &egui::Painter,
    rect: egui::Rect,
    cycles: f32,
    wave: impl Fn(f32) -> f32,
    color: egui::Color32,
    thickness: f32,
) {
    // Two wide, faint copies of the line underneath make the glow.
    let glow = [(thickness * 2.2, 0.10), (thickness * 1.5, 0.25)];
    // Leave room top and bottom so the glow isn't cut off.
    let half_height = rect.height() / 2.0 - (glow[0].0 / 2.0 + 1.5);

    // One point per 2 screen points across is plenty for a smooth line.
    let steps = ((rect.width() / 2.0) as usize).max(8);
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let x = i as f32 / steps as f32;
            let phase = (x * cycles).fract();
            let y = wave(phase);
            egui::pos2(rect.left() + x * rect.width(), rect.center().y - y * half_height)
        })
        .collect();

    for (width, strength) in glow {
        painter.add(egui::Shape::line(
            points.clone(),
            egui::Stroke::new(width, color.gamma_multiply(strength)),
        ));
    }
    painter.add(egui::Shape::line(points, egui::Stroke::new(thickness, color)));
}

/// The wave's colour, sliding around the colour wheel as Morph goes from sine to square:
/// cyan (sine) → blue → violet (triangle) → magenta → hot pink (saw) → orange (square).
fn neon_color(morph: f32) -> egui::Color32 {
    // Hue is a position on the colour wheel, 0..1 (0 = red, ⅓ = green, ⅔ = blue).
    // Start at cyan (0.5) and go 0.55 of the way around, ending at orange (1.05 → 0.05).
    let hue = (0.5 + 0.55 * morph.clamp(0.0, 1.0)).fract();
    // Full saturation and brightness is what makes it read as neon.
    egui::ecolor::Hsva::new(hue, 0.9, 1.0, 1.0).into()
}

/// Space around the window's edges, left and right.
const MARGIN_X: f32 = 16.0;
/// Each column's width. Fits a row of four knobs with room to spare.
const COLUMN_WIDTH: f32 = 280.0;
const COLUMN_GAP: f32 = 24.0;

/// A column of sections that is always exactly `COLUMN_WIDTH` wide.
fn column(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.set_width(COLUMN_WIDTH);
        contents(ui);
    });
}


/// A titled block of rows: a small heading, then a label/control grid.
fn section(ui: &mut egui::Ui, title: &str, rows: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title).strong());
    egui::Grid::new(title)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(72.0)
        .show(ui, rows);
}

/// Space between knobs in a row.
const KNOB_GAP: f32 = 14.0;

/// A titled block like `section`, but with its knobs side by side in a row.
fn knob_section(ui: &mut egui::Ui, title: &str, knobs: impl FnOnce(&mut egui::Ui)) {
    section_heading(ui, title, |_| {});
    knob_row(ui, knobs);
}

/// A section's bold title, with room for something small beside it (`extra`).
fn section_heading(ui: &mut egui::Ui, title: &str, extra: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong());
        ui.add_space(6.0);
        extra(ui);
    });
    ui.add_space(4.0);
}

/// Knobs side by side.
fn knob_row(ui: &mut egui::Ui, knobs: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = KNOB_GAP;
        knobs(ui);
    });
}

/// Change a parameter the way the DAW expects: begin, set, end. The begin/end pair is
/// what makes it one undo step and records it as automation.
fn set<P: Param>(setter: &ParamSetter, param: &P, value: P::Plain) {
    setter.begin_set_parameter(param);
    setter.set_parameter(param, value);
    setter.end_set_parameter(param);
}

/// An on/off button for a `BoolParam`. It looks pressed while on.
fn toggle_button(ui: &mut egui::Ui, label: &str, param: &BoolParam, setter: &ParamSetter) {
    let on = param.value();
    if ui.selectable_label(on, label).clicked() {
        set(setter, param, !on);
    }
}

/// One button that flips between Poly and Mono. Its label shows the mode you're in.
/// Hovering it shows the full name ("Mono (legato)") in the readout box.
fn voice_mode_button(ui: &mut egui::Ui, param: &EnumParam<VoiceMode>, setter: &ParamSetter) {
    let (label, other) = match param.value() {
        VoiceMode::Poly => ("POLY", VoiceMode::Mono),
        VoiceMode::Mono => ("MONO", VoiceMode::Poly),
    };
    // A fixed width, so the button doesn't change size when the label does.
    let button = egui::Button::new(label).min_size(egui::vec2(56.0, 0.0));
    let response = ui.add(button);
    if response.clicked() {
        set(setter, param, other);
    }
    crate::readout::report(ui, param, &response);
}

/// `‹ value ›` buttons that step through an enum's options in order. Stops at the
/// first and last option instead of wrapping around.
fn stepper<T: Enum + PartialEq + 'static>(
    ui: &mut egui::Ui,
    param: &EnumParam<T>,
    setter: &ParamSetter,
) {
    let current = param.value().to_index();
    let last = T::variants().len() - 1;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(current > 0, egui::Button::new("‹"))
            .clicked()
        {
            set(setter, param, T::from_index(current - 1));
        }
        // A fixed-width name, so the › button doesn't jump around as the text changes.
        ui.add_sized([80.0, 18.0], egui::Label::new(T::variants()[current]));
        if ui
            .add_enabled(current < last, egui::Button::new("›"))
            .clicked()
        {
            set(setter, param, T::from_index(current + 1));
        }
    });
}
