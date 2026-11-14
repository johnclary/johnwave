//! The plugin window: a deliberately minimal egui UI.
//!
//! egui is "immediate mode": every frame, `ui()` describes the whole window from scratch
//! ("a title, then these sections of sliders"). egui works out the layout and handles the mouse. There's
//! no widget tree to keep in sync. The window only reads and sets parameters. No sound
//! logic lives here.

use crate::SynthParams;
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::{AtomicF32, BoolParam, Enum, EnumParam, Param, ParamSetter};
use std::sync::atomic::Ordering;
use nice_plug_egui::widgets::ParamSlider;
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

        // Two fixed-width columns side by side. Fixed, because each slider's value box
        // changes width with its value ("5 ms" vs "10.00 s"), and the columns shouldn't
        // shift or overlap when that happens.
        ui.horizontal_top(|ui| {
            column(ui, |ui| {
                section(ui, "Voice", |ui| {
                    ui.label("Mode");
                    choice_buttons(ui, &p.voice_mode, &setter);
                    ui.end_row();
                    slider_row(ui, "Gain", &p.gain, &setter);
                });
                section(ui, "Oscillator", |ui| {
                    slider_row(ui, "Morph", &p.morph, &setter);
                });
                section(ui, "Tuning", |ui| {
                    slider_row(ui, "Coarse", &p.coarse, &setter);
                    slider_row(ui, "Fine", &p.fine, &setter);
                    slider_row(ui, "Bend Range", &p.bend_range, &setter);
                });
                section(ui, "Filter", |ui| {
                    slider_row(ui, "Cutoff", &p.cutoff, &setter);
                    slider_row(ui, "Resonance", &p.resonance, &setter);
                });
                section(ui, "Amp Envelope", |ui| {
                    slider_row(ui, "Attack", &p.attack, &setter);
                    slider_row(ui, "Decay", &p.decay, &setter);
                    slider_row(ui, "Sustain", &p.sustain, &setter);
                    slider_row(ui, "Release", &p.release, &setter);
                });
            });
            ui.add_space(COLUMN_GAP);
            column(ui, |ui| {
                section(ui, "Mod Envelope", |ui| {
                    slider_row(ui, "Attack", &p.mod_env_attack, &setter);
                    slider_row(ui, "Decay", &p.mod_env_decay, &setter);
                    slider_row(ui, "To Morph", &p.mod_env_to_morph, &setter);
                    slider_row(ui, "To Cutoff", &p.mod_env_to_cutoff, &setter);
                });
                section(ui, "LFO", |ui| {
                    let synced = p.lfo_sync.value();
                    slider_row(ui, "Shape", &p.lfo_shape, &setter);

                    // Only one of Rate / Sync Rate is in charge, so grey out the other.
                    ui.label("Rate");
                    ui.add_enabled(!synced, slider(&p.lfo_rate, &setter));
                    ui.end_row();
                    ui.label("Sync Rate");
                    ui.add_enabled_ui(synced, |ui| stepper(ui, &p.lfo_division, &setter));
                    ui.end_row();

                    ui.label("");
                    ui.horizontal(|ui| {
                        toggle_button(ui, "Sync", &p.lfo_sync, &setter);
                        toggle_button(ui, "Retrigger", &p.lfo_retrigger, &setter);
                    });
                    ui.end_row();

                    slider_row(ui, "To Morph", &p.lfo_to_morph, &setter);
                    slider_row(ui, "To Cutoff", &p.lfo_to_cutoff, &setter);
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
    let visuals = ui.visuals();

    // A faint centre line at 0.
    painter.line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.4)),
    );

    // One point per 2 screen points across is plenty for a smooth line.
    let steps = (width / 2.0) as usize;
    let half_height = WAVE_HEIGHT / 2.0 - 8.0; // room top and bottom for the glow
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let x = i as f32 / steps as f32;
            let phase = (x * WAVE_CYCLES).fract();
            let y = crate::osc::shape(phase, 0.0, morph);
            egui::pos2(rect.left() + x * width, rect.center().y - y * half_height)
        })
        .collect();

    // Neon look: two wide, faint copies of the line underneath make a glow, then the
    // bright line on top.
    let color = neon_color(morph);
    for (width, strength) in [(9.0, 0.10), (5.0, 0.25)] {
        painter.add(egui::Shape::line(
            points.clone(),
            egui::Stroke::new(width, color.gamma_multiply(strength)),
        ));
    }
    painter.add(egui::Shape::line(points, egui::Stroke::new(2.5, color)));
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
/// Each column: label (72) + gap + slider + value box, with room for the widest value.
const COLUMN_WIDTH: f32 = 280.0;
const COLUMN_GAP: f32 = 24.0;

/// A column of sections that is always exactly `COLUMN_WIDTH` wide.
fn column(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.set_width(COLUMN_WIDTH);
        contents(ui);
    });
}

/// How wide every slider is, so the columns line up.
const SLIDER_WIDTH: f32 = 110.0;

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

fn slider<'a, P: Param>(param: &'a P, setter: &'a ParamSetter<'a>) -> ParamSlider<'a, P> {
    ParamSlider::for_param(param, setter).with_width(SLIDER_WIDTH)
}

/// One grid row: label, then a slider.
fn slider_row<P: Param>(ui: &mut egui::Ui, label: &str, param: &P, setter: &ParamSetter) {
    ui.label(label);
    ui.add(slider(param, setter));
    ui.end_row();
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

/// One button per option, side by side, for enums with only a few options.
fn choice_buttons<T: Enum + PartialEq + 'static>(
    ui: &mut egui::Ui,
    param: &EnumParam<T>,
    setter: &ParamSetter,
) {
    let current = param.value().to_index();
    ui.horizontal(|ui| {
        for (index, name) in T::variants().iter().enumerate() {
            if ui.selectable_label(index == current, *name).clicked() && index != current {
                set(setter, param, T::from_index(index));
            }
        }
    });
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
