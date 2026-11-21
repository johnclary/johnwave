//! The plugin window: a deliberately minimal egui UI.
//!
//! egui is "immediate mode": every frame, `ui()` describes the whole window from scratch
//! ("a title, then these rows of knobs"). egui works out the layout and handles the mouse. There's
//! no widget tree to keep in sync. The window only reads and sets parameters. No sound
//! logic lives here.

use crate::filter::FilterType;
use crate::knob::Knob;
use crate::slider::Slider;
use crate::modulation::{dest_span, ModDest, ModSlotParams, Routes, SlotDest, SlotSource};
use crate::trail::Trail;
use crate::{SynthParams, VoiceMode};
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::{AtomicF32, BoolParam, Enum, EnumParam, Param, ParamSetter};
use std::sync::atomic::Ordering;
use nice_plug_egui::{EguiEditorState, Frame, NiceEguiApp};
use std::sync::Arc;

/// Window size in points. On a Retina screen egui draws at twice this many pixels.
pub const WIDTH: f64 = 760.0;
pub const HEIGHT: f64 = 710.0;

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
    /// How loud that voice is right now (0..1), set by the audio thread. The display
    /// draws the wave this tall, so it swells and fades with the amp envelope.
    display_level: Arc<AtomicF32>,
    /// The faint copies of recent shapes drawn behind the screen's wave.
    trail: Trail,
}

impl JohnwaveEditor {
    pub fn new(
        params: Arc<SynthParams>,
        display_morph: Arc<AtomicF32>,
        display_level: Arc<AtomicF32>,
    ) -> Self {
        Self {
            params,
            gui_context: None,
            display_morph,
            display_level,
            trail: Trail::default(),
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

        // Remember what the screen shows this frame, for its trail. While the trail is
        // still fading, ask for another frame so it keeps fading even if nothing else
        // changes.
        let now = ui.input(|i| i.time);
        let morph = self.display_morph.load(Ordering::Relaxed);
        let level = self.display_level.load(Ordering::Relaxed);
        self.trail.update(now, morph, level);
        if self.trail.is_fading() {
            ui.ctx().request_repaint();
        }

        // Padding around the edges of the window.
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(MARGIN_X as i8, 12))
            .show(ui, |ui| self.contents(ui, &setter));
    }
}

impl JohnwaveEditor {
    fn contents(&self, ui: &mut egui::Ui, setter: &ParamSetter) {
        let p = &self.params;
        // The mod matrix as routes, so the knobs can show how far they're being pushed.
        let routes = crate::matrix_routes(
            p.mod_slots
                .iter()
                .map(|slot| (slot.source.value(), slot.dest.value(), slot.amount.value())),
        );

        // Header: the title and the Poly/Mono switch on the left, the readout box at the
        // right edge.
        let mut readout_rect = egui::Rect::NOTHING;
        ui.horizontal(|ui| {
            glow_title(ui, "johnwave");
            ui.add_space(12.0);
            switch(
                ui,
                &p.voice_mode,
                &setter,
                &[(VoiceMode::Poly, "POLY"), (VoiceMode::Mono, "MONO")],
            );
            // Only reserve the readout's space here. It's drawn at the very end, after
            // every control has said whether the mouse is on it.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                readout_rect = ui
                    .allocate_exact_size(egui::vec2(READOUT_WIDTH, READOUT_HEIGHT), egui::Sense::hover())
                    .0;
            });
        });

        // Three rows, following the sound: the source, what shapes it, what moves it.

        // Row 1: the oscillator (with its display) and tuning.
        row(ui, |ui| {
            group(ui, |ui| {
                knob_section(ui, "Oscillator", |ui| {
                    ui.add(Knob::new(&p.morph, &setter, "Shape").mod_span(mod_span(
                        &routes,
                        ModDest::Morph,
                        |offset| p.morph.modulated_normalized_value() + offset,
                    )));
                    // The shape the knob is set to (the screen on the right shows what's
                    // actually playing, after modulation).
                    let morph = p.morph.value();
                    knob_wave(ui, |phase| crate::osc::shape(phase, 0.0, morph));
                });
            });
            group(ui, |ui| {
                knob_section(ui, "Tuning", |ui| {
                    // Pitch routes show here, since Coarse is in semitones too. Its range is
                    // 48 semitones (−24..+24), so one semitone is 1/48 of the way round.
                    ui.add(Knob::new(&p.coarse, &setter, "Coarse").bipolar().mod_span(mod_span(
                        &routes,
                        ModDest::Pitch,
                        |offset| p.coarse.modulated_normalized_value() + offset / 48.0,
                    )));
                    ui.add(Knob::new(&p.fine, &setter, "Fine").bipolar());
                    ui.add(Knob::new(&p.bend_range, &setter, "Bend"));
                });
            });
            // The screen sits at the right edge, under the readout, as wide as it. It's
            // as tall as the row so far, so its bottom lines up with the knob names.
            let height = ui.min_rect().height();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let morph = self.display_morph.load(Ordering::Relaxed);
                let level = self.display_level.load(Ordering::Relaxed);
                osc_display(ui, morph, level, &self.trail, height);
            });
        });

        // Row 1b: the second oscillator. Its own row for now, since row 1 is full; the
        // layout rework will find it a proper home.
        row(ui, |ui| {
            group(ui, |ui| {
                // The knobs grey out while Osc 2 is switched off.
                let osc2_on = p.osc2_on.value();
                section_heading(ui, "Osc 2", |ui| toggle_button(ui, "ON", &p.osc2_on, &setter));
                knob_row(ui, |ui| {
                    ui.add_enabled(osc2_on, Knob::new(&p.osc2_morph, &setter, "Shape").mod_span(mod_span(
                        &routes,
                        ModDest::Osc2Morph,
                        |offset| p.osc2_morph.modulated_normalized_value() + offset,
                    )));
                    let morph2 = p.osc2_morph.value();
                    knob_wave(ui, |phase| crate::osc::shape(phase, 0.0, morph2));
                    // Pitch 2 routes show here. The range is 72 semitones (−36..+36).
                    ui.add_enabled(
                        osc2_on,
                        Knob::new(&p.osc2_coarse, &setter, "Coarse").bipolar().mod_span(mod_span(
                            &routes,
                            ModDest::Osc2Pitch,
                            |offset| p.osc2_coarse.modulated_normalized_value() + offset / 72.0,
                        )),
                    );
                    ui.add_enabled(osc2_on, Knob::new(&p.osc2_fine, &setter, "Fine").bipolar());
                    ui.add_enabled(osc2_on, Knob::new(&p.osc_mix, &setter, "Mix").mod_span(mod_span(
                        &routes,
                        ModDest::OscMix,
                        |offset| p.osc_mix.modulated_normalized_value() + offset,
                    )));
                });
            });
        });

        // Row 2: filter and amp (its envelope plus the output level).
        row(ui, |ui| {
            group(ui, |ui| {
                knob_section(ui, "Filter", |ui| {
                    // Cutoff routes are in octaves: each one doubles (or halves) the Hz.
                    ui.add(Knob::new(&p.cutoff, &setter, "Cutoff").mod_span(mod_span(
                        &routes,
                        ModDest::Cutoff,
                        |offset| {
                            let hz = p.cutoff.modulated_plain_value() * offset.exp2();
                            p.cutoff.preview_normalized(hz)
                        },
                    )));
                    ui.add(Knob::new(&p.resonance, &setter, "Res").mod_span(mod_span(
                        &routes,
                        ModDest::Resonance,
                        |offset| p.resonance.modulated_normalized_value() + offset,
                    )));
                });
                // The type sits under its knobs, like the LFO's buttons under its own.
                ui.add_space(6.0);
                switch(
                    ui,
                    &p.filter_type,
                    &setter,
                    &[
                        (FilterType::LowPass, "LP"),
                        (FilterType::HighPass, "HP"),
                        (FilterType::BandPass, "BP"),
                        (FilterType::Notch, "NOTCH"),
                    ],
                );
            });
            group(ui, |ui| {
                knob_section(ui, "Amp", |ui| {
                    ui.add(Knob::new(&p.attack, &setter, "A"));
                    ui.add(Knob::new(&p.decay, &setter, "D"));
                    ui.add(Knob::new(&p.sustain, &setter, "S"));
                    ui.add(Knob::new(&p.release, &setter, "R"));
                    ui.add(Knob::new(&p.gain, &setter, "Gain"));
                    ui.add(Knob::new(&p.pan, &setter, "Pan").bipolar());
                });
            });
        });

        // Row 3: modulation, plus the bitcrusher (row 2, where it sits in the sound,
        // has no room left).
        row(ui, |ui| {
            group(ui, |ui| {
                knob_section(ui, "Mod Envelope", |ui| {
                    ui.add(Knob::new(&p.mod_env_attack, &setter, "A"));
                    ui.add(Knob::new(&p.mod_env_decay, &setter, "D"));
                    ui.add(Knob::new(&p.mod_env_sustain, &setter, "S"));
                    ui.add(Knob::new(&p.mod_env_release, &setter, "R"));
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
                });
                ui.add_space(6.0);
                // The sync rate sits right after Sync, since that's what it belongs to.
                ui.horizontal(|ui| {
                    toggle_button(ui, "RETRIG", &p.lfo_retrigger, &setter);
                    toggle_button(ui, "SYNC", &p.lfo_sync, &setter);
                    ui.add_enabled_ui(synced, |ui| stepper(ui, &p.lfo_division, &setter));
                });
            });
            group(ui, |ui| {
                // The knobs grey out while Crush is switched off.
                let crush_on = p.crush_on.value();
                section_heading(ui, "Crush", |ui| toggle_button(ui, "ON", &p.crush_on, &setter));
                knob_row(ui, |ui| {
                    // Bits is a straight line from 1 to 16, so one bit is 1/15 of the way.
                    ui.add_enabled(crush_on, Knob::new(&p.crush_bits, &setter, "Bits").mod_span(mod_span(
                        &routes,
                        ModDest::CrushBits,
                        |offset| p.crush_bits.modulated_normalized_value() + offset / 15.0,
                    )));
                    // Steps routes are in octaves, like Cutoff.
                    ui.add_enabled(crush_on, Knob::new(&p.crush_steps, &setter, "Steps").mod_span(mod_span(
                        &routes,
                        ModDest::CrushSteps,
                        |offset| {
                            let steps = p.crush_steps.modulated_plain_value() * offset.exp2();
                            p.crush_steps.preview_normalized(steps)
                        },
                    )));
                    ui.add_enabled(crush_on, Knob::new(&p.crush_mix, &setter, "Mix").mod_span(mod_span(
                        &routes,
                        ModDest::CrushMix,
                        |offset| p.crush_mix.modulated_normalized_value() + offset,
                    )));
                });
            });
        });

        // Row 4: the mod matrix. A list of the routes in use, and one editor for the
        // route picked in the list.
        row(ui, |ui| {
            group(ui, |ui| {
                section_heading(ui, "Mod Matrix", |_| {});
                mod_matrix(ui, &p.mod_slots, setter);
            });
        });

        crate::readout::draw(ui, readout_rect);
    }
}

/// Width of a route's amount slider.
const AMOUNT_SLIDER_WIDTH: f32 = 160.0;

/// The mod matrix: every slot as a row, `[Source] [Dest] [amount]`. A new slot is
/// blank (both Off), and a slot only does something once both are set.
/// Where a knob's mod arc reaches, as knob positions 0..1, or `None` if nothing is routed
/// to `dest`. `to_position` turns an offset (in the destination's unit) into the knob
/// position its value would land on.
fn mod_span(routes: &Routes, dest: ModDest, to_position: impl Fn(f32) -> f32) -> Option<(f32, f32)> {
    dest_span(routes, dest)
        .map(|(lo, hi)| (to_position(lo).clamp(0.0, 1.0), to_position(hi).clamp(0.0, 1.0)))
}

fn mod_matrix(ui: &mut egui::Ui, slots: &[ModSlotParams], setter: &ParamSetter) {
    ui.spacing_mut().item_spacing.y = 4.0;
    for slot in slots {
        route_row(ui, slot, setter);
    }
}

/// One slot: its source, destination and amount. The slider is greyed out until both
/// are set, since it does nothing before then.
fn route_row(ui: &mut egui::Ui, slot: &ModSlotParams, setter: &ParamSetter) {
    let dest = slot.dest.value();
    let routed = slot.source.value() != SlotSource::Off && dest != SlotDest::Off;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        cycler(ui, &slot.source, setter, SlotSource::Off);
        cycler(ui, &slot.dest, setter, SlotDest::Off);
        let amount = dest.format_amount(slot.amount.value());
        ui.add_enabled(
            routed,
            Slider::new(&slot.amount, setter, AMOUNT_SLIDER_WIDTH)
                .bipolar()
                .readout_value(amount),
        );
    });
}

/// A button that steps through an enum's choices: click for the next one, right-click
/// for the previous one. It wraps around at the ends. The `blank` choice (Off) shows
/// as an empty outline, and any other choice lights up. Its name and value show in the
/// readout box.
fn cycler<T: Enum + PartialEq + Copy + 'static>(
    ui: &mut egui::Ui,
    param: &EnumParam<T>,
    setter: &ParamSetter,
    blank: T,
) {
    let value = param.value();
    let current = value.to_index();
    let count = T::variants().len();
    let is_blank = value == blank;
    let text = if is_blank { "" } else { T::variants()[current] };
    // A fixed width, so the things after it don't jump around as the name changes.
    let response = ui
        .scope(|ui| {
            cyan_selection(ui);
            ui.add_sized([CYCLER_WIDTH, 20.0], egui::Button::new(text).selected(!is_blank))
        })
        .inner
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    let step: isize = if response.clicked() {
        1
    } else if response.secondary_clicked() {
        -1
    } else {
        0
    };
    if step != 0 {
        let next = (current as isize + step).rem_euclid(count as isize) as usize;
        set(setter, param, T::from_index(next));
    }
    crate::readout::report(ui, param, &response);
}

/// Wide enough for the longest choice ("Crush Mix").
const CYCLER_WIDTH: f32 = 80.0;

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

/// The readout box is a flat strip in the header.
const READOUT_WIDTH: f32 = 240.0;
const READOUT_HEIGHT: f32 = 30.0;

/// What the oscillator is playing right now, in a wide "screen" under the readout. It uses the
/// same `osc::shape` function the sound comes from, so it shows the real shape and its
/// real level (the square looks smaller because it's scaled to sound as loud as the others).
fn osc_display(ui: &mut egui::Ui, morph: f32, level: f32, trail: &Trail, height: f32) {
    let size = egui::vec2(READOUT_WIDTH, height);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let faint = ui.visuals().weak_text_color().gamma_multiply(0.5);

    screen_frame(&painter, rect, faint);
    // A faint centre line at 0.
    let inner = rect.shrink(SCREEN_PADDING);
    painter.line_segment(
        [inner.left_center(), inner.right_center()],
        egui::Stroke::new(1.0, faint.gamma_multiply(0.6)),
    );

    // The trail first, so the main wave draws on top. Each ghost keeps its own colour
    // and gets fainter as it ages (squared, so it fades quickly at first, then lingers).
    let now = ui.input(|i| i.time);
    for (ghost, fade) in trail.ghosts(now) {
        let wave = |phase| crate::osc::shape(phase, 0.0, ghost.morph) * ghost.level;
        let color = neon_color(ghost.morph).gamma_multiply(TRAIL_STRENGTH * fade * fade);
        wave_line(&painter, inner, SCREEN_CYCLES, wave, color, 1.0);
    }

    // Nothing playing (NaN): leave just the centre line (and any trail still fading),
    // so it doesn't show a sound that isn't there.
    if morph.is_nan() {
        return;
    }
    // Scaled by the level, so the wave swells on attack and shrinks away on release.
    let wave = |phase| crate::osc::shape(phase, 0.0, morph) * level;
    wave_line(&painter, inner, SCREEN_CYCLES, wave, neon_color(morph), 2.0);
}

/// How bright the newest ghost in the trail is, compared with the main wave.
const TRAIL_STRENGTH: f32 = 0.5;

/// How many cycles the wide screen shows.
const SCREEN_CYCLES: f32 = 2.0;

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
    wave_line(&painter, rect, 1.0, wave, cyan(), 2.0);
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

/// The window title in neon cyan with a soft glow, like the knob arcs and waves.
fn glow_title(ui: &mut egui::Ui, text: &str) {
    let font = egui::TextStyle::Heading.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), cyan());
    let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    let painter = ui.painter();

    // The glow: faint copies of the text nudged out in a ring around it, a wide faint
    // ring and a closer, stronger one. Where they overlap, near the letters, it's
    // brightest.
    for (radius, strength) in TITLE_GLOW {
        for i in 0..8 {
            let angle = i as f32 * std::f32::consts::TAU / 8.0;
            let offset = radius * egui::vec2(angle.cos(), angle.sin());
            painter.text(
                rect.min + offset,
                egui::Align2::LEFT_TOP,
                text,
                font.clone(),
                cyan().gamma_multiply(strength),
            );
        }
    }
    // The sharp text on top.
    painter.galley(rect.min, galley, cyan());
}

/// The title's glow rings: (how far out, in points; strength 0..1).
const TITLE_GLOW: [(f32, f32); 2] = [(3.0, 0.05), (1.5, 0.12)];

/// The accent colour: the neon cyan of the knob arcs, toggles, readout value and the
/// small waves.
pub fn cyan() -> egui::Color32 {
    egui::ecolor::Hsva::new(CYAN_HUE, 0.9, 1.0, 1.0).into()
}

/// Hue is a position on the colour wheel, 0..1 (0 = red, ⅓ = green, ½ = cyan, ⅔ = blue).
const CYAN_HUE: f32 = 0.5;
/// How far the screen's colour drifts as Shape goes from sine to square. Small, so it
/// stays a cyan: 0.08 ends at a light azure blue. Try negative to drift toward green.
const HUE_DRIFT: f32 = 0.08;
/// Saturation at sine and at square. Less saturated looks paler and icier.
const SATURATION: (f32, f32) = (0.9, 0.65);

/// The screen wave's colour: the accent cyan at sine, drifting gently toward a paler,
/// icier blue as Shape goes to square.
pub fn neon_color(morph: f32) -> egui::Color32 {
    let t = morph.clamp(0.0, 1.0);
    let hue = CYAN_HUE + HUE_DRIFT * t;
    let saturation = SATURATION.0 + (SATURATION.1 - SATURATION.0) * t;
    // Full brightness is what keeps it reading as neon.
    egui::ecolor::Hsva::new(hue, saturation, 1.0, 1.0).into()
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
    let cyan = cyan();
    let selection = &mut ui.visuals_mut().selection;
    selection.bg_fill = cyan.gamma_multiply(0.25);
    selection.stroke.color = cyan;
}

/// A switch with one button per choice, like `POLY | MONO`, with the current one lit
/// up. Every choice is always visible, so you can see what else there is. Hovering
/// shows the full name ("Mono (legato)") in the readout box.
fn switch<T: Enum + PartialEq + Copy + 'static>(
    ui: &mut egui::Ui,
    param: &EnumParam<T>,
    setter: &ParamSetter,
    choices: &[(T, &str)],
) {
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
                for &(choice, label) in choices {
                    let response = ui.selectable_label(current == choice, label);
                    if response.clicked() && current != choice {
                        set(setter, param, choice);
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
