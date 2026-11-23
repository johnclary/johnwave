//! The plugin window: a deliberately minimal egui UI.
//!
//! egui is "immediate mode": every frame, `ui()` describes the whole window from scratch
//! ("a title, then these rows of knobs"). egui works out the layout and handles the mouse. There's
//! no widget tree to keep in sync. The window only reads and sets parameters. No sound
//! logic lives here.

use crate::filter::{FilterSlope, FilterType};
use crate::knob::Knob;
use crate::knob::ModTarget;
use crate::modulation::{mod_offset, mod_span, LiveSources, ModDest, ModSlotParams, ModSource, Routes, SlotDest, SlotSource};
use crate::screen::{OutputCapture, ScreenWave, SharedScreen};
use crate::trail::{Phosphor, Trail};
use crate::{SynthParams, VoiceMode};
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::{BoolParam, Enum, EnumParam, Param, ParamSetter};
use nice_plug_egui::{EguiEditorState, Frame, NiceEguiApp, RepaintNotifier};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Window size in points. On a Retina screen egui draws at twice this many pixels.
/// The width is just what the three columns need, so the right one ends at the edge.
pub const WIDTH: f64 =
    (2.0 * MARGIN_X + LEFT_COLUMN_WIDTH + MIDDLE_COLUMN_WIDTH + RIGHT_COLUMN_WIDTH + 2.0 * COLUMN_GAP) as f64;
/// A first guess, to be tuned once the layout's been seen.
pub const HEIGHT: f64 = 520.0;

pub fn default_state() -> Arc<EguiEditorState> {
    EguiEditorState::from_size(LogicalSize::new(WIDTH, HEIGHT), 1.0)
}

pub struct JohnwaveEditor {
    /// The same parameters the audio thread reads. `Arc` means both sides share one copy.
    params: Arc<SynthParams>,
    /// How the window talks to the host (to change parameters, with undo and automation).
    /// It only exists while the window is open, so it starts as `None`.
    gui_context: Option<GuiContext>,
    /// What the newest voice's oscillators are playing right now (Shapes including LFO
    /// and Mod Env, Osc 2's tuning and mix, and how loud it is), set by the audio thread.
    /// The oscillator screen draws this, so it swells and fades with the amp envelope.
    display: Arc<SharedScreen>,
    /// The last moment of real output, set by the audio thread, for the OUT line.
    capture: Arc<OutputCapture>,
    /// The newest voice's mod source values, set by the audio thread, for the live tick.
    live_sources: Arc<LiveSources>,
    /// Asks the host for another frame. `ctx.request_repaint()` doesn't get through in
    /// the plugin window (frames only came while the mouse moved), but this flag does: the
    /// window checks it 64 times a second.
    repaint: RepaintNotifier,
    /// The faint copies of recent shapes drawn behind the screen's wave.
    trail: Trail,
    /// The OUT line's afterglow.
    phosphor: Phosphor,
}

impl JohnwaveEditor {
    pub fn new(
        params: Arc<SynthParams>,
        display: Arc<SharedScreen>,
        capture: Arc<OutputCapture>,
        live_sources: Arc<LiveSources>,
        repaint: RepaintNotifier,
    ) -> Self {
        Self {
            params,
            gui_context: None,
            display,
            capture,
            live_sources,
            repaint,
            trail: Trail::default(),
            phosphor: Phosphor::default(),
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
        self.trail.update(now, self.display.load());
        // The OUT line's glow, only while it's showing a note.
        let out = self.params.screen_out.load(Ordering::Relaxed) && self.display.load().is_some();
        self.phosphor.update(now, out.then(|| self.capture.cycles(SCREEN_CYCLES, OUT_POINTS)).flatten());
        if self.trail.is_fading() {
            self.repaint.request_repaint();
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

        // Header: the title, Poly/Mono and Legato on the left, the readout box at the
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
            // LEGATO only matters while Porta is on, so it greys out at Off.
            let porta_on = p.porta.value() > 0.0;
            ui.add_enabled_ui(porta_on, |ui| toggle_button(ui, "LEGATO", &p.porta_legato, &setter));
            // Only reserve the readout's space here. It's drawn at the very end, after
            // every control has said whether the mouse is on it.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                readout_rect = ui
                    .allocate_exact_size(egui::vec2(READOUT_WIDTH, READOUT_HEIGHT), egui::Sense::hover())
                    .0;
            });
        });

        // Which chip is armed. The chips are in the middle column, so the left column's
        // knobs are already drawn by the time a chip click lands. The end of this
        // function asks for one more frame when that happens, so they catch up.
        let armed = armed_source(ui).map(|source| Armed {
            source,
            value: self.live_sources.get(source),
        });
        // While the live tick is moving, keep drawing frames (about 64 a second). When
        // the notes stop, the tick goes and so do the extra frames.
        if armed.is_some_and(|a| a.value.is_some()) {
            self.repaint.request_repaint();
        }

        // Three columns side by side, each a stack of panels.
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = COLUMN_GAP;

            // Left column: the sound's source, then the filter.
            column(ui, LEFT_COLUMN_WIDTH, |ui| {
                panel(ui, "Global", |ui| {
                    knob_row(ui, |ui| {
                        ui.add(Knob::new(&p.gain, &setter, "Gain"));
                        ui.add(Knob::new(&p.pan, &setter, "Pan").bipolar());
                        ui.add(Knob::new(&p.bend_range, &setter, "Bend"));
                        ui.add(Knob::new(&p.porta, &setter, "Porta"));
                        // Mix does nothing while Osc 2 is off.
                        ui.add(Knob::new(&p.osc_mix, &setter, "Mix").mod_target(mod_target(armed, &p.mod_slots,
                            &routes,
                            ModDest::OscMix,
                            |offset| p.osc_mix.modulated_normalized_value() + offset,
                        )).off(!p.osc2_on.value()));
                    });
                });

                panel(ui, "Oscillators", |ui| {
                    sub_label(ui, "Osc 1");
                    knob_row(ui, |ui| {
                        let morph = p.morph.value();
                        knob_wave(ui, |phase| crate::osc::shape(phase, 0.0, morph));
                        ui.add(Knob::new(&p.morph, &setter, "Shape").mod_target(mod_target(armed, &p.mod_slots,
                            &routes,
                            ModDest::Morph,
                            |offset| p.morph.modulated_normalized_value() + offset,
                        )));
                        // Pitch routes show here. Its range is 48 semitones (−24..+24), so
                        // one semitone is 1/48 of the way round.
                        ui.add(Knob::new(&p.coarse, &setter, "Coarse").bipolar().mod_target(mod_target(armed, &p.mod_slots,
                            &routes,
                            ModDest::Pitch,
                            |offset| p.coarse.modulated_normalized_value() + offset / 48.0,
                        )));
                        ui.add(Knob::new(&p.fine, &setter, "Fine").bipolar());
                        // DUAL switches Osc 2 on. It ends osc 1's row, so both rows' knobs
                        // line up.
                        row_toggle(ui, "DUAL", &p.osc2_on, &setter);
                    });
                    // Osc 2's knobs grey out while DUAL is off.
                    let osc2_on = p.osc2_on.value();
                    sub_label(ui, "Osc 2");
                    knob_row(ui, |ui| {
                        let morph2 = p.osc2_morph.value();
                        knob_wave(ui, |phase| crate::osc::shape(phase, 0.0, morph2));
                        ui.add(Knob::new(&p.osc2_morph, &setter, "Shape").mod_target(mod_target(armed, &p.mod_slots,
                            &routes,
                            ModDest::Osc2Morph,
                            |offset| p.osc2_morph.modulated_normalized_value() + offset,
                        )).off(!osc2_on));
                        // Pitch 2 routes show here. The range is 72 semitones (−36..+36).
                        ui.add(
                            Knob::new(&p.osc2_coarse, &setter, "Coarse").bipolar().mod_target(mod_target(armed, &p.mod_slots,
                                &routes,
                                ModDest::Osc2Pitch,
                                |offset| p.osc2_coarse.modulated_normalized_value() + offset / 72.0,
                            )).off(!osc2_on),
                        );
                        ui.add_enabled(osc2_on, Knob::new(&p.osc2_fine, &setter, "Fine").bipolar());
                    });
                });

                panel(ui, "Filter", |ui| {
                    knob_row(ui, |ui| {
                        // Cutoff routes are in octaves: each one doubles (or halves) the Hz.
                        ui.add(Knob::new(&p.cutoff, &setter, "Cutoff").mod_target(mod_target(armed, &p.mod_slots,
                            &routes,
                            ModDest::Cutoff,
                            |offset| {
                                let hz = p.cutoff.modulated_plain_value() * offset.exp2();
                                p.cutoff.preview_normalized(hz)
                            },
                        )));
                        ui.add(Knob::new(&p.resonance, &setter, "Res").mod_target(mod_target(armed, &p.mod_slots,
                            &routes,
                            ModDest::Resonance,
                            |offset| p.resonance.modulated_normalized_value() + offset,
                        )));
                        ui.add(Knob::new(&p.filter_drive, &setter, "Drive").mod_target(mod_target(armed, &p.mod_slots,
                            &routes,
                            ModDest::Drive,
                            |offset| p.filter_drive.modulated_normalized_value() + offset,
                        )));
                    });
                    // The type and slope sit under the knobs.
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
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
                        switch(
                            ui,
                            &p.filter_slope,
                            &setter,
                            &[(FilterSlope::Db12, "12"), (FilterSlope::Db24, "24")],
                        );
                    });
                });
            });

            // Middle column: the amp envelope, then everything that moves things.
            column(ui, MIDDLE_COLUMN_WIDTH, |ui| {
                panel(ui, "Amp Envelope", |ui| {
                    knob_row(ui, |ui| {
                        ui.add(Knob::new(&p.attack, &setter, "A"));
                        ui.add(Knob::new(&p.decay, &setter, "D"));
                        ui.add(Knob::new(&p.sustain, &setter, "S"));
                        ui.add(Knob::new(&p.release, &setter, "R"));
                    });
                });

                // The chips arm a source for mod mode. Two of them are the Mod Env and
                // LFO just below, so they share one panel.
                panel(ui, "Modulation", |ui| {
                    sub_label(ui, "Source Select");
                    mod_chips(ui, &p.mod_slots);

                    sub_label(ui, "Mod Env");
                    knob_row(ui, |ui| {
                        ui.add(Knob::new(&p.mod_env_attack, &setter, "A"));
                        ui.add(Knob::new(&p.mod_env_decay, &setter, "D"));
                        ui.add(Knob::new(&p.mod_env_sustain, &setter, "S"));
                        ui.add(Knob::new(&p.mod_env_release, &setter, "R"));
                    });

                    ui.add_space(4.0);
                    ui.separator();

                    // Only one of Rate / Sync Rate is in charge, so grey out the other.
                    let synced = p.lfo_sync.value();
                    let lfo_shape = p.lfo_shape.value();
                    sub_label(ui, "LFO");
                    knob_row(ui, |ui| {
                        ui.add(Knob::new(&p.lfo_shape, &setter, "Shape"));
                        knob_wave(ui, |phase| crate::lfo::wave(phase, lfo_shape, KNOB_WAVE_EDGE));
                        ui.add_enabled(!synced, Knob::new(&p.lfo_rate, &setter, "Rate"));
                        // Right of centre fades the LFO in on each note, left fades it out.
                        ui.add(Knob::new(&p.lfo_fade, &setter, "Fade").bipolar());
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

            // Right column: the scope, then the effects-ish extras.
            column(ui, RIGHT_COLUMN_WIDTH, |ui| {
                // The screen sits in a panel of its own, with a tight bezel, like a
                // monitor in its housing. It's shorter by whatever the title and bezel
                // take, so the column stays the same height.
                let title_height = panel_title(ui, "Scope").size().y;
                let height = SCOPE_HEIGHT - title_height - SCOPE_BEZEL;
                panel_with(ui, "Scope", SCOPE_BEZEL, SCOPE_BEZEL, 0.0, |ui| {
                    osc_display(ui, p, self.display.load(), &self.phosphor, &self.trail, height);
                });

                panel(ui, "Unison", |ui| {
                    // The knobs grey out while Unison is switched off.
                    let unison_on = p.unison_on.value();
                    knob_row(ui, |ui| {
                        ui.add_enabled(unison_on, Knob::new(&p.unison_voices, &setter, "Voices"));
                        // Detune and Width do nothing with a single voice.
                        let spread = unison_on && p.unison_voices.value() > 1;
                        // Detune routes are in cents, on a skewed knob.
                        ui.add(Knob::new(&p.unison_detune, &setter, "Detune").mod_target(mod_target(armed, &p.mod_slots,
                            &routes,
                            ModDest::UnisonDetune,
                            |offset| {
                                p.unison_detune
                                    .preview_normalized(p.unison_detune.modulated_plain_value() + offset)
                            },
                        )).off(!spread));
                        ui.add_enabled(spread, Knob::new(&p.unison_width, &setter, "Width"));
                        row_toggle(ui, "ON", &p.unison_on, &setter);
                    });
                });

                panel(ui, "Crush", |ui| {
                    // The knobs grey out while Crush is switched off.
                    let crush_on = p.crush_on.value();
                    knob_row(ui, |ui| {
                        // Bits is a straight line from 1 to 16, so one bit is 1/15 of the way.
                        ui.add(Knob::new(&p.crush_bits, &setter, "Bits").mod_target(mod_target(armed, &p.mod_slots,
                            &routes,
                            ModDest::CrushBits,
                            |offset| p.crush_bits.modulated_normalized_value() + offset / 15.0,
                        )).off(!crush_on));
                        // Steps routes are in octaves, like Cutoff.
                        ui.add(Knob::new(&p.crush_steps, &setter, "Steps").mod_target(mod_target(armed, &p.mod_slots,
                            &routes,
                            ModDest::CrushSteps,
                            |offset| {
                                let steps = p.crush_steps.modulated_plain_value() * offset.exp2();
                                p.crush_steps.preview_normalized(steps)
                            },
                        )).off(!crush_on));
                        ui.add(Knob::new(&p.crush_mix, &setter, "Mix").mod_target(mod_target(armed, &p.mod_slots,
                            &routes,
                            ModDest::CrushMix,
                            |offset| p.crush_mix.modulated_normalized_value() + offset,
                        )).off(!crush_on));
                        row_toggle(ui, "ON", &p.crush_on, &setter);
                    });
                });
            });
        });

        crate::readout::draw(ui, readout_rect);

        // A chip click or Esc changed the armed source partway through this frame, so
        // the knobs drawn before the chips still show the old one. Ask for another frame
        // to redraw them; without this they'd wait for the mouse to move.
        if armed_source(ui) != armed.map(|a| a.source) {
            self.repaint.request_repaint();
        }
    }
}

/// What makes a knob the mod destination `dest`: how far the routes push it, as knob
/// positions 0..1, plus the slots mod mode edits. With a source `armed`, only its routes
/// count, and `live` is where it's pushing the knob right now (the live tick).
/// `to_position` turns an offset (in the destination's unit) into the knob position its
/// value would land on.
fn mod_target<'a>(
    armed: Option<Armed>,
    slots: &'a [ModSlotParams],
    routes: &Routes,
    dest: ModDest,
    to_position: impl Fn(f32) -> f32,
) -> ModTarget<'a> {
    let position = |offset| to_position(offset).clamp(0.0, 1.0);
    let span = mod_span(routes, dest, armed.map(|a| a.source))
        .map(|(lo, hi)| (position(lo), position(hi)));
    let live = armed
        .and_then(|a| mod_offset(routes, dest, a.source, a.value?))
        .map(position);
    ModTarget { dest: SlotDest::of(dest), span, live, slots }
}

/// The source armed for mod mode, and where it is right now (`None` while nothing is
/// playing). `Clone, Copy` lets it be passed to every knob without fuss.
#[derive(Clone, Copy)]
struct Armed {
    source: ModSource,
    value: Option<f32>,
}

/// Where the armed source lives in egui's memory. It's window state, not a parameter:
/// it isn't saved with the patch, and closing the window ends mod mode.
fn armed_id() -> egui::Id {
    egui::Id::unique("armed_mod_source")
}

/// True while a source is armed. In mod mode everything that isn't a mod target (the
/// buttons, switches, steppers and the screen) fades and stops taking clicks, so the
/// knobs you can paint stand out. The mod chips stay live so you can leave.
fn in_mod_mode(ui: &egui::Ui) -> bool {
    armed_source(ui).is_some()
}

/// How visible the screen stays in mod mode (0..1).
const MOD_MODE_FADE: f32 = 0.3;

/// The source armed for mod mode, or `None` when it's off.
pub fn armed_source(ui: &egui::Ui) -> Option<ModSource> {
    ui.data(|d| d.get_temp::<Option<ModSource>>(armed_id())).flatten()
}

fn set_armed(ui: &egui::Ui, source: Option<ModSource>) {
    ui.data_mut(|d| d.insert_temp(armed_id(), source));
}

/// The mod mode chips: `LFO ENV VEL RAND`. Click one to arm it, and every knob shows
/// just what that source does; drag a knob to set its amount. Click it again, or press
/// Esc, to leave mod mode.
///
/// A chip has three looks: plain (like the other buttons) when the source goes nowhere,
/// dim pink when it's routed to at least one knob, and lit pink while it's armed.
fn mod_chips(ui: &mut egui::Ui, slots: &[ModSlotParams]) {
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        set_armed(ui, None);
    }
    let armed = armed_source(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (source, label) in [
            (ModSource::Lfo, "LFO"),
            (ModSource::ModEnv, "ENV"),
            (ModSource::Velocity, "VEL"),
            (ModSource::Random, "RAND"),
        ] {
            let color = mod_pink();
            let on = armed == Some(source);
            let routed = is_routed(slots, source);
            let response = ui
                .scope(|ui| {
                    let selection = &mut ui.visuals_mut().selection;
                    selection.bg_fill = color.gamma_multiply(0.3);
                    selection.stroke.color = color;
                    let dim = color.gamma_multiply(0.75);
                    let mut text = egui::RichText::new(label);
                    if on {
                        text = text.color(color);
                    } else if routed {
                        text = text.color(dim);
                    }
                    let response = ui.selectable_label(on, text);
                    if !on {
                        chip_outline(ui, &response, routed.then_some(dim));
                    }
                    response
                })
                .inner
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                set_armed(ui, if on { None } else { Some(source) });
            }
            let name = SlotSource::of(source).name();
            let hint = if on { "drag a knob · Esc to exit".to_string() } else { "click to show & paint".to_string() };
            crate::readout::report_text(ui, &format!("{name} mod"), hint, &response);
        }
    });
}

/// True if any mod slot sends `source` somewhere with a non-zero amount.
fn is_routed(slots: &[ModSlotParams], source: ModSource) -> bool {
    let wanted = SlotSource::of(source);
    slots
        .iter()
        .any(|s| s.source.value() == wanted && s.dest.value() != SlotDest::Off && s.amount.value() != 0.0)
}

/// The mod colour: a dusky pink, far enough round the colour wheel from the cyan to
/// stand apart. #C95DA7, which is 319°, 54%, 79% in a normal colour picker. Used for the
/// knobs' mod arcs and the mod mode chips.
pub fn mod_pink() -> egui::Color32 {
    egui::Color32::from_rgb(0xC9, 0x5D, 0xA7)
}

/// The readout box is a flat strip in the header.
const READOUT_WIDTH: f32 = 240.0;
const READOUT_HEIGHT: f32 = 30.0;

/// What the oscillators are playing right now, in a wide "screen" under the readout. It
/// uses the same `osc::shape` function the sound comes from, so it shows the real shapes
/// and their real level (the square looks smaller because it's scaled to sound as loud as
/// the others). `wave` is `None` when nothing is playing. Chips along the top switch
/// each line on and off.
fn osc_display(
    ui: &mut egui::Ui,
    p: &SynthParams,
    wave: Option<ScreenWave>,
    phosphor: &Phosphor,
    trail: &Trail,
    height: f32,
) {
    let size = egui::vec2(ui.available_width(), height);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let mut painter = ui.painter_at(rect);
    // Faded in mod mode, like everything else that isn't a mod target.
    if in_mod_mode(ui) {
        painter.multiply_opacity(MOD_MODE_FADE);
    }
    let faint = ui.visuals().weak_text_color().gamma_multiply(0.5);
    screen_frame(&painter, rect, faint);

    // The chips across the top, inside the frame, and the waves in the space below.
    let inner = rect.shrink(SCREEN_PADDING);
    let (chip_row, waves) = inner.split_top_bottom_at_y(inner.top() + SCREEN_CHIP_ROW);
    screen_chips(ui, chip_row, p);
    let show = |flag: &AtomicBool| flag.load(Ordering::Relaxed);
    let (show1, show2, show_mix) = (show(&p.screen_osc1), show(&p.screen_osc2), show(&p.screen_mix));
    let show_out = show(&p.screen_out);

    // A faint centre line at 0.
    painter.line_segment(
        [waves.left_center(), waves.right_center()],
        egui::Stroke::new(1.0, faint.gamma_multiply(0.6)),
    );

    // The trail first, so the main wave draws on top. Each ghost is just the mix, in
    // its own colour, and gets fainter as it ages (squared, so it fades quickly at
    // first, then lingers).
    if show_mix {
        let now = ui.input(|i| i.time);
        for (ghost, fade) in trail.ghosts(now) {
            let w = ghost.wave;
            let color = neon_color(w.morph).gamma_multiply(TRAIL_STRENGTH * fade * fade);
            screen_line(&painter, waves, |x| w.mix(x), color, 1.0);
        }
    }

    // Nothing playing: leave just the centre line (and any trail still fading), so it
    // doesn't show a sound that isn't there.
    let Some(w) = wave else {
        return;
    };
    // Everything in Osc 1's colour: thickness and brightness tell the lines apart.
    let color = neon_color(w.morph);
    // Each oscillator at its share of the mix and the voice's level. Behind the mix they're
    // thin and faint. On their own they're drawn brighter, so they're easy to follow.
    let (part_color, part_width) = if show_mix {
        (color.gamma_multiply(COMPONENT_STRENGTH), 1.0)
    } else {
        (color, 1.5)
    };
    // Without Osc 2, Osc 1 alone *is* the mix, so it's only drawn when the mix isn't.
    if show1 && (w.has_osc2() || !show_mix) {
        screen_line(&painter, waves, |x| w.osc1(x) * w.gain1 * w.level, part_color, part_width);
    }
    if show2 && w.has_osc2() {
        screen_line(&painter, waves, |x| w.osc2(x) * w.gain2 * w.level, part_color, part_width);
    }
    // Then the two added up, bold on top. Scaled by the level, so it swells on attack
    // and shrinks away on release.
    if show_mix {
        screen_line(&painter, waves, |x| w.mix(x), color, 2.0);
    }
    // Last and on top, the real output: what the voices actually played, after Crush,
    // the filter, unison and any other notes, lined up with Osc 1's cycles. Without any
    // of those it lands right on the MIX line. A thin soft white with a light glow, like
    // a real scope trace next to the drawn neon waves.
    if show_out {
        if let Some(out) = phosphor.line() {
            let last = (out.len() - 1) as f32;
            let curve = |x: f32| out[(x * last).round() as usize].clamp(-1.0, 1.0);
            neon_line_glow(&painter, waves, curve, OUT_COLOR, 1.5, OUT_GLOW);
        }
    }
}

/// How many points the OUT line is made of. About one per screen point across.
const OUT_POINTS: usize = 240;
/// The OUT line's soft white, a touch cool so it sits with the cyan.
const OUT_COLOR: egui::Color32 = egui::Color32::from_rgb(0xE6, 0xEE, 0xF2);
/// How strong the OUT line's glow is, compared with the neon lines'.
const OUT_GLOW: f32 = 0.5;

/// The `OSC 1  OSC 2  MIX  OUT` chips along the top of the screen. Each switches its line on
/// and off. Osc 2's greys out while Osc 2 is switched off. They're window settings, not
/// sound, so they're saved with the patch but aren't parameters.
fn screen_chips(ui: &mut egui::Ui, row: egui::Rect, p: &SynthParams) {
    let mut chips = ui.new_child(egui::UiBuilder::new().max_rect(row));
    // Like the other buttons, they fade and stop taking clicks in mod mode.
    chips.add_enabled_ui(!in_mod_mode(ui), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.spacing_mut().button_padding = egui::vec2(4.0, 0.0);
            cyan_selection(ui);
            screen_chip(ui, "OSC 1", &p.screen_osc1, true);
            screen_chip(ui, "OSC 2", &p.screen_osc2, p.osc2_on.value());
            screen_chip(ui, "MIX", &p.screen_mix, true);
            screen_chip(ui, "OUT", &p.screen_out, true);
        });
    });
}

fn screen_chip(ui: &mut egui::Ui, label: &str, flag: &AtomicBool, enabled: bool) {
    let on = flag.load(Ordering::Relaxed);
    let text = egui::RichText::new(label).size(SCREEN_CHIP_TEXT);
    let response = ui.add_enabled(enabled, egui::Button::selectable(on, text));
    if !on {
        chip_outline(ui, &response, None);
    }
    if response.clicked() {
        flag.store(!on, Ordering::Relaxed);
    }
}

/// How tall the strip of chips at the top of the screen is, and their text size.
const SCREEN_CHIP_ROW: f32 = 20.0;
const SCREEN_CHIP_TEXT: f32 = 10.0;

/// One line on the oscillator screen. `wave` takes how many cycles of Osc 1 from the
/// left edge and gives -1..1.
fn screen_line(
    painter: &egui::Painter,
    rect: egui::Rect,
    wave: impl Fn(f32) -> f32,
    color: egui::Color32,
    thickness: f32,
) {
    neon_line(painter, rect, |x| wave(x * SCREEN_CYCLES), color, thickness);
}

/// How bright the thin Osc 1 and Osc 2 lines are, behind the bold mix.
const COMPONENT_STRENGTH: f32 = 0.5;

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

/// Draw `cycles` cycles of `wave` (phase 0..1 in, -1..1 out) across `rect` as a neon
/// line `thickness` wide.
fn wave_line(
    painter: &egui::Painter,
    rect: egui::Rect,
    cycles: f32,
    wave: impl Fn(f32) -> f32,
    color: egui::Color32,
    thickness: f32,
) {
    neon_line(painter, rect, |x| wave((x * cycles).fract()), color, thickness);
}

/// How many segments a line across `rect` is made of: one per 2 screen points across is
/// plenty for a smooth line.
fn line_steps(rect: egui::Rect) -> usize {
    ((rect.width() / 2.0) as usize).max(8)
}

/// Draw `curve` (0..1 across `rect` in, -1..1 out) as a neon line `thickness` wide.
/// Shared by the oscillator screen and the knob waves so they look alike.
fn neon_line(
    painter: &egui::Painter,
    rect: egui::Rect,
    curve: impl Fn(f32) -> f32,
    color: egui::Color32,
    thickness: f32,
) {
    neon_line_glow(painter, rect, curve, color, thickness, 1.0);
}

/// The thickest line ever drawn, which sets how much room lines leave for their glow.
const THICKEST_LINE: f32 = 2.0;

/// `neon_line` with its glow scaled by `glow` (1 = the usual).
fn neon_line_glow(
    painter: &egui::Painter,
    rect: egui::Rect,
    curve: impl Fn(f32) -> f32,
    color: egui::Color32,
    thickness: f32,
    glow: f32,
) {
    // Two wide, faint copies of the line underneath make the glow.
    let glow = [(thickness * 2.2, 0.10 * glow), (thickness * 1.5, 0.25 * glow)];
    // Leave room top and bottom so the glow isn't cut off. The room is the same for
    // every line, however thick, so lines of different thicknesses share one scale and
    // land on top of each other when they match.
    let half_height = rect.height() / 2.0 - (THICKEST_LINE * 2.2 / 2.0 + 1.5);

    let steps = line_steps(rect);
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let x = i as f32 / steps as f32;
            let y = curve(x);
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

/// The gray of the panel outlines and their titles.
fn panel_gray(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().weak_text_color().gamma_multiply(0.6)
}

/// How round a panel's corners are, in points.
const PANEL_RADIUS: f32 = 6.0;
/// Space inside a panel's outline, around its contents (left/right, and bottom).
const PANEL_PADDING_X: f32 = 10.0;
const PANEL_PADDING_BOTTOM: f32 = 8.0;
/// Extra space below the title, before the contents start.
const PANEL_TITLE_GAP: f32 = 4.0;
/// How far the outline stops short of the title on each side.
const PANEL_TITLE_PADDING: f32 = 6.0;
/// The title's text size.
const PANEL_TITLE_SIZE: f32 = 12.5;
/// How thick the outline is, in points.
const PANEL_STROKE: f32 = 2.0;

/// A cluster of controls inside a gray rounded outline, with `title` in capitals sitting
/// on the top edge, centred. The line breaks around the title so it never crosses it.
fn panel<R>(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    panel_with(ui, title, PANEL_PADDING_X, PANEL_PADDING_BOTTOM, PANEL_TITLE_GAP, contents)
}

/// A panel's title, laid out ready to draw.
fn panel_title(ui: &egui::Ui, title: &str) -> std::sync::Arc<egui::Galley> {
    let font = egui::FontId::proportional(PANEL_TITLE_SIZE);
    ui.painter().layout_no_wrap(title.to_uppercase(), font, panel_gray(ui))
}

/// A `panel` with its own spacing inside the outline: `sides`, `bottom`, and `title_gap`
/// extra below the title.
fn panel_with<R>(
    ui: &mut egui::Ui,
    title: &str,
    sides: f32,
    bottom: f32,
    title_gap: f32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let gray = panel_gray(ui);
    let galley = panel_title(ui, title);
    let title_size = galley.size();

    // Space from whatever's above, plus room for the half of the title that sticks up
    // above the outline.
    ui.add_space(PANEL_GAP + title_size.y / 2.0);
    // Save a spot in the paint order for the outline now, so it draws under the
    // contents. Its shape is filled in once we know how big the contents are.
    let outline = ui.painter().add(egui::Shape::Noop);

    let margin = egui::Margin {
        left: sides as i8,
        right: sides as i8,
        top: (title_size.y / 2.0 + title_gap) as i8,
        bottom: bottom as i8,
    };
    let inner = egui::Frame::new().inner_margin(margin).show(ui, |ui| {
        // As wide as its column, so a column's panels line up on both sides. And never
        // narrower than the title plus its gap and the corners.
        let title_width = title_size.x + 2.0 * (PANEL_TITLE_PADDING + PANEL_RADIUS);
        ui.set_min_width(ui.available_width().max(title_width));
        contents(ui)
    });

    let rect = inner.response.rect;
    let gap = title_size.x / 2.0 + PANEL_TITLE_PADDING;
    let points = outline_with_gap(rect, PANEL_RADIUS, rect.center().x - gap, rect.center().x + gap);
    ui.painter().set(outline, egui::Shape::line(points, egui::Stroke::new(PANEL_STROKE, gray)));
    let title_pos = egui::pos2(rect.center().x, rect.top()) - title_size / 2.0;
    ui.painter().galley(title_pos, galley, gray);

    inner.inner
}

/// The points of a rounded rectangle's outline, going clockwise from `gap_right` on the
/// top edge all the way round to `gap_left`, so the top edge has a gap between them.
fn outline_with_gap(rect: egui::Rect, radius: f32, gap_left: f32, gap_right: f32) -> Vec<egui::Pos2> {
    // Half the line's width in, so the whole line sits inside the panel.
    let rect = rect.shrink(PANEL_STROKE / 2.0);
    let (left, right, top, bottom) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    // Each corner: its centre and the angle its quarter-circle starts at (screen y
    // points down, so -90° is straight up).
    let corners = [
        (egui::pos2(right - radius, top + radius), -90.0_f32),
        (egui::pos2(right - radius, bottom - radius), 0.0),
        (egui::pos2(left + radius, bottom - radius), 90.0),
        (egui::pos2(left + radius, top + radius), 180.0),
    ];
    let mut points = vec![egui::pos2(gap_right, top)];
    for (centre, start) in corners {
        for step in 0..=CORNER_STEPS {
            let angle = (start + 90.0 * step as f32 / CORNER_STEPS as f32).to_radians();
            points.push(centre + radius * egui::vec2(angle.cos(), angle.sin()));
        }
    }
    points.push(egui::pos2(gap_left, top));
    points
}

/// Straight pieces per rounded corner. Enough to look round at this size.
const CORNER_STEPS: usize = 6;

/// Space above each panel (between panels in a column, and below the header).
const PANEL_GAP: f32 = 10.0;

/// The three columns' widths, and the space between them. The right one is as wide as
/// the readout above it, so the screen lines up under it.
const LEFT_COLUMN_WIDTH: f32 = 310.0;
const MIDDLE_COLUMN_WIDTH: f32 = 290.0;
const RIGHT_COLUMN_WIDTH: f32 = READOUT_WIDTH;
const COLUMN_GAP: f32 = 16.0;

/// How tall the Scope panel is, outline included: what the bare screen and the space
/// above it took before it had a panel, so the column below didn't move.
const SCOPE_HEIGHT: f32 = 170.0 + PANEL_TITLE_SIZE / 2.0;
/// The gap between the Scope's outline and its screen, at the sides and bottom.
const SCOPE_BEZEL: f32 = 5.0;

/// One column: a fixed-width stack of panels, top to bottom.
fn column(ui: &mut egui::Ui, width: f32, contents: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.set_width(width);
        // Back to normal spacing inside (the row of columns widened it).
        ui.spacing_mut().item_spacing.x = 8.0;
        contents(ui);
    });
}

/// An on/off button at the end of a row of knobs, level with the knobs' circles.
fn row_toggle(ui: &mut egui::Ui, label: &str, param: &BoolParam, setter: &ParamSetter) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), KNOB_WAVE_SIZE),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| toggle_button(ui, label, param, setter),
    );
}

/// A small muted label naming a part of a panel, like "LFO" inside Modulation.
fn sub_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(PANEL_TITLE_SIZE - 1.0)
            .color(panel_gray(ui)),
    );
    ui.add_space(2.0);
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
        .add_enabled_ui(!in_mod_mode(ui), |ui| {
            cyan_selection(ui);
            let response = ui.selectable_label(on, label);
            if !on {
                chip_outline(ui, &response, None);
            }
            response.clicked()
        })
        .inner;
    if clicked {
        set(setter, param, !on);
    }
}

/// A faint outline around an unselected chip, so it reads as a button and not a floating
/// label. egui only draws a selectable's frame while it's hovered or selected, so this
/// paints one on top. `color: None` uses the gray of the `switch` frame.
fn chip_outline(ui: &egui::Ui, response: &egui::Response, color: Option<egui::Color32>) {
    let color = color.unwrap_or_else(|| ui.visuals().weak_text_color().gamma_multiply(0.5));
    let radius = ui.visuals().widgets.inactive.corner_radius;
    ui.painter().rect_stroke(response.rect, radius, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
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
    ui.add_enabled_ui(!in_mod_mode(ui), |ui| {
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
    ui.add_enabled_ui(!in_mod_mode(ui), |ui| ui.horizontal(|ui| {
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
    }));
}
