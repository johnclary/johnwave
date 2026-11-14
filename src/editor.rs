//! The plugin window: a deliberately minimal egui UI.
//!
//! egui is "immediate mode": every frame, `ui()` describes the whole window from scratch
//! ("a title, then these rows of knobs"). egui works out the layout and handles the mouse. There's
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
pub const WIDTH: f64 = 640.0;
pub const HEIGHT: f64 = 440.0;

pub fn default_state() -> Arc<EguiEditorState> {
    EguiEditorState::from_size(LogicalSize::new(WIDTH, HEIGHT), 1.0)
}

pub struct JohnwaveEditor {
    /// The same parameters the audio thread reads. `Arc` means both sides share one copy.
    params: Arc<SynthParams>,
    /// How the window talks to the host (to change parameters, with undo and automation).
    /// It only exists while the window is open, so it starts as `None`.
    gui_context: Option<GuiContext>,
    /// The Shape being played right now (including LFO and Mod Env), set by the audio
    /// thread. The oscillator display draws this.
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
        ui.add_space(2.0);

        // Three rows, following the sound: the source, what shapes it, what moves it.

        // Row 1: the oscillator (with its display), tuning and voice.
        row(ui, |ui| {
            osc_display(ui, self.display_morph.load(Ordering::Relaxed));
            group(ui, |ui| {
                knob_section(ui, "Oscillator", |ui| {
                    ui.add(Knob::new(&p.morph, &setter, "Shape"));
                    // The shape the knob is set to (the screen on the left shows it
                    // after modulation).
                    let morph = p.morph.value();
                    knob_wave(ui, |phase| crate::osc::shape(phase, 0.0, morph));
                });
            });
            group(ui, |ui| {
                knob_section(ui, "Tuning", |ui| {
                    ui.add(Knob::new(&p.coarse, &setter, "Coarse").bipolar());
                    ui.add(Knob::new(&p.fine, &setter, "Fine").bipolar());
                    ui.add(Knob::new(&p.bend_range, &setter, "Bend"));
                });
            });
            group(ui, |ui| {
                // No title here: the Poly/Mono button sits where a title would, so the
                // Gain knob lines up with the knobs beside it.
                ui.add_space(6.0);
                voice_mode_button(ui, &p.voice_mode, &setter);
                ui.add_space(4.0);
                ui.add(Knob::new(&p.gain, &setter, "Gain"));
            });
        });

        // Row 2: filter and amp envelope, with the readout box at the right edge.
        let mut readout_rect = egui::Rect::NOTHING;
        row(ui, |ui| {
            group(ui, |ui| {
                knob_section(ui, "Filter", |ui| {
                    ui.add(Knob::new(&p.cutoff, &setter, "Cutoff"));
                    ui.add(Knob::new(&p.resonance, &setter, "Res"));
                });
            });
            group(ui, |ui| {
                knob_section(ui, "Amp Envelope", |ui| {
                    ui.add(Knob::new(&p.attack, &setter, "A"));
                    ui.add(Knob::new(&p.decay, &setter, "D"));
                    ui.add(Knob::new(&p.sustain, &setter, "S"));
                    ui.add(Knob::new(&p.release, &setter, "R"));
                });
            });
            // Only reserve the readout's space here. It's drawn at the very end, after
            // every control has said whether the mouse is on it.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                readout_rect = ui
                    .allocate_exact_size(egui::vec2(READOUT_WIDTH, SCREEN_SIZE), egui::Sense::hover())
                    .0;
            });
        });

        // Row 3: modulation.
        row(ui, |ui| {
            group(ui, |ui| {
                knob_section(ui, "Mod Envelope", |ui| {
                    ui.add(Knob::new(&p.mod_env_attack, &setter, "A"));
                    ui.add(Knob::new(&p.mod_env_decay, &setter, "D"));
                    // The amounts can push either way, so they're bipolar.
                    ui.add(Knob::new(&p.mod_env_to_morph, &setter, "To Shape").bipolar());
                    ui.add(Knob::new(&p.mod_env_to_cutoff, &setter, "To Cutoff").bipolar());
                });
            });
            group(ui, |ui| {
                // Only one of Rate / Sync Rate is in charge, so grey out the other.
                let synced = p.lfo_sync.value();
                let lfo_shape = p.lfo_shape.value();
                section_heading(ui, "LFO", |_| {});
                knob_row(ui, |ui| {
                    ui.add(Knob::new(&p.lfo_shape, &setter, "Shape"));
                    knob_wave(ui, |phase| crate::lfo::wave(phase, lfo_shape, KNOB_WAVE_EDGE));
                    ui.add_enabled(!synced, Knob::new(&p.lfo_rate, &setter, "Rate"));
                    ui.add(Knob::new(&p.lfo_to_morph, &setter, "To Shape"));
                    // Cutoff can push either way, so it's bipolar.
                    ui.add(Knob::new(&p.lfo_to_cutoff, &setter, "To Cutoff").bipolar());
                });
                ui.add_space(6.0);
                // The sync rate sits right after Sync, since that's what it belongs to.
                ui.horizontal(|ui| {
                    toggle_button(ui, "RETRIG", &p.lfo_retrigger, &setter);
                    toggle_button(ui, "SYNC", &p.lfo_sync, &setter);
                    ui.add_enabled_ui(synced, |ui| stepper(ui, &p.lfo_division, &setter));
                });
            });
        });

        crate::readout::draw(ui, readout_rect);
    }
}

/// Space between groups in a row, and between rows.
const GROUP_GAP: f32 = 32.0;
const ROW_GAP: f32 = 10.0;

/// One row of groups, side by side, lined up along their tops.
fn row(ui: &mut egui::Ui, groups: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(ROW_GAP);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = GROUP_GAP;
        groups(ui);
    });
}

/// One group in a row: its contents stacked top to bottom.
fn group(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        // Back to normal spacing inside the group (`row` widened it for between groups).
        ui.spacing_mut().item_spacing.x = 8.0;
        contents(ui);
    });
}

/// The oscillator display and the readout box are both this tall: about one section
/// (a title plus a row of knobs). The display is square.
const SCREEN_SIZE: f32 = 84.0;
const READOUT_WIDTH: f32 = 170.0;

/// The oscillator's current shape: one cycle in a small square "screen". It uses the
/// same `osc::shape` function the sound comes from, so it shows the real shape and its
/// real level (the square looks smaller because it's scaled to sound as loud as the others).
fn osc_display(ui: &mut egui::Ui, morph: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(SCREEN_SIZE), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let faint = ui.visuals().weak_text_color().gamma_multiply(0.5);

    screen_frame(&painter, rect, faint);
    // A faint centre line at 0.
    let inner = rect.shrink(SCREEN_PADDING);
    painter.line_segment(
        [inner.left_center(), inner.right_center()],
        egui::Stroke::new(1.0, faint.gamma_multiply(0.6)),
    );

    let wave = |phase| crate::osc::shape(phase, 0.0, morph);
    wave_line(&painter, inner, 1.0, wave, neon_color(morph), 3.0);
}

/// Space between a screen's border and what's drawn inside it.
const SCREEN_PADDING: f32 = 8.0;

/// The thin rounded border shared by the oscillator display and the readout box.
pub fn screen_frame(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    painter.rect_stroke(
        rect.shrink(0.5),
        4.0,
        egui::Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
}

/// The waves beside the Shape knobs are squares as wide as a knob's circle.
const KNOB_WAVE_SIZE: f32 = 44.0;
/// The LFO's wave draws its square with a tiny ramp on each edge, like the real one.
const KNOB_WAVE_EDGE: f32 = 0.001;

/// One cycle of a wave, in a knob-sized square, for beside a Shape knob. Always the same
/// cyan as the knob arcs: it shows what the knob is set to, not what's playing.
fn knob_wave(ui: &mut egui::Ui, wave: impl Fn(f32) -> f32) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(KNOB_WAVE_SIZE), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    // neon_color(0.0) is the knob arcs' cyan.
    wave_line(&painter, rect, 1.0, wave, neon_color(0.0), 2.0);
}

/// Draw `wave` (phase 0..1 in, -1..1 out) across `rect` as a neon line `thickness` wide.
/// Shared by the oscillator screen and the knob waves so they look alike.
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

/// Space between knobs in a row.
const KNOB_GAP: f32 = 14.0;

/// A titled block: a bold title, then its knobs side by side.
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
    let clicked = ui
        .scope(|ui| {
            cyan_selection(ui);
            ui.selectable_label(on, label).clicked()
        })
        .inner;
    if clicked {
        set(setter, param, !on);
    }
}

/// Make "selected" buttons in this `ui` a dim cyan fill with bright cyan text, to match
/// the knob arcs. Call it inside `ui.scope` so the change stays with those buttons.
fn cyan_selection(ui: &mut egui::Ui) {
    let cyan = neon_color(0.0);
    let selection = &mut ui.visuals_mut().selection;
    selection.bg_fill = cyan.gamma_multiply(0.25);
    selection.stroke.color = cyan;
}

/// A two-part switch, `POLY | MONO`, with the current mode lit up. Both are always
/// visible, so you can see what the other choice is. Hovering shows the full name
/// ("Mono (legato)") in the readout box.
fn voice_mode_button(ui: &mut egui::Ui, param: &EnumParam<VoiceMode>, setter: &ParamSetter) {
    let current = param.value();
    // A thin outline around both halves, so they read as one switch.
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.5)))
        .corner_radius(4.0)
        .inner_margin(2.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                cyan_selection(ui);
                for (mode, label) in [(VoiceMode::Poly, "POLY"), (VoiceMode::Mono, "MONO")] {
                    let response = ui.selectable_label(current == mode, label);
                    if response.clicked() && current != mode {
                        set(setter, param, mode);
                    }
                    crate::readout::report(ui, param, &response);
                }
            });
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
