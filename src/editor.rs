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
use crate::spectrum::{Spectrum, FFT_SIZE};
use crate::trail::{Phosphor, Trail};
use crate::{SynthParams, VoiceMode};
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::{BoolParam, Enum, EnumParam, Param, ParamSetter};
use nice_plug_egui::{EguiEditorState, Frame, NiceEguiApp, RepaintNotifier};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

/// Window size in points. On a Retina screen egui draws at twice this many pixels.
/// The width is just what the three columns need, so the right one ends at the edge.
pub const WIDTH: f64 =
    (2.0 * MARGIN_X + LEFT_COLUMN_WIDTH + MIDDLE_COLUMN_WIDTH + RIGHT_COLUMN_WIDTH + 2.0 * COLUMN_GAP) as f64;
/// Just tall enough for the tallest column (the left one, ending with the filter), with
/// the same 12-point margin at the bottom as at the top. Measured in the window: if the
/// panels grow, this needs measuring again.
pub const HEIGHT: f64 = 481.0;

pub fn default_state() -> Arc<EguiEditorState> {
    EguiEditorState::from_size(LogicalSize::new(WIDTH, HEIGHT), 1.0)
}

// ---- Palette ---------------------------------------------------------------------------
// The window's base colours, all in one place. The accent colours live further down:
// `accent()`, `ACCENT_HUE`, `neon_color()`, `MOD_COLOR`, `OUT_COLOR`.

/// The window background.
pub const BACKGROUND: egui::Color32 = egui::Color32::BLACK;
/// Normal text: knob names.
pub const TEXT: egui::Color32 = egui::Color32::from_gray(140);
/// Muted text: switched-off knob names, the readout's parameter name.
pub const MUTED_TEXT: egui::Color32 = egui::Color32::from_gray(84);
/// Panel outlines, panel titles and sub-labels ("LFO").
pub const PANEL_GRAY: egui::Color32 = egui::Color32::from_gray(65);
/// Faint outlines: the scope and readout frames, unselected chips, two-way switches.
/// Also the FREQ view's labels.
pub const FAINT: egui::Color32 = egui::Color32::from_gray(42);
/// The scope's centre line and the FREQ view's gridlines.
pub const GRID: egui::Color32 = egui::Color32::from_gray(25);
/// The dim ring a knob's arc fills along.
pub const KNOB_TRACK: egui::Color32 = egui::Color32::from_gray(60);

/// Chip and button text: resting, under the mouse, and while pressed.
pub const CHIP_TEXT: egui::Color32 = egui::Color32::from_gray(215);
pub const CHIP_TEXT_HOVER: egui::Color32 = egui::Color32::from_gray(245);
pub const CHIP_TEXT_PRESSED: egui::Color32 = egui::Color32::WHITE;
/// The fill behind a chip under the mouse, and while pressed.
pub const CHIP_HOVER_FILL: egui::Color32 = egui::Color32::from_gray(64);
pub const CHIP_PRESSED_FILL: egui::Color32 = egui::Color32::from_gray(40);
/// The fill of the ‹ › arrow buttons.
pub const BUTTON_FILL: egui::Color32 = egui::Color32::from_gray(50);

// How bright the accent colours are drawn in each state, 0..1 (1 = full colour).
/// A knob's arc at rest (it goes to full while hovered or dragged).
pub const ARC_RESTING: f32 = 0.85;
/// A switched-off knob's arc, and its mod arc.
pub const ARC_OFF: f32 = 0.3;
/// The mod arc of a switched-off knob that mod mode can still paint.
pub const MOD_ARC_OFF_TARGET: f32 = 0.55;
/// The mod-colour outline round a knob's name in mod mode.
pub const MOD_OUTLINE: f32 = 0.6;
/// The fill behind a selected accent chip, and a selected mod chip.
pub const SELECTED_FILL: f32 = 0.25;
pub const MOD_SELECTED_FILL: f32 = 0.3;
/// A mod source chip that's routed somewhere but not armed.
pub const MOD_ROUTED: f32 = 0.75;
/// The name in the readout box, dimmer than its value so the value leads.
pub const READOUT_NAME: f32 = 0.6;

/// Hand the palette's base colours to egui, so its own text and buttons use them.
fn apply_palette(ctx: &egui::Context, frame: &mut Frame) {
    frame.set_clear_color(BACKGROUND.into());
    ctx.all_styles_mut(|style| {
        let widgets = &mut style.visuals.widgets;
        widgets.noninteractive.fg_stroke.color = TEXT;
        widgets.inactive.fg_stroke.color = CHIP_TEXT;
        widgets.hovered.fg_stroke.color = CHIP_TEXT_HOVER;
        widgets.active.fg_stroke.color = CHIP_TEXT_PRESSED;
        widgets.inactive.weak_bg_fill = BUTTON_FILL;
        widgets.hovered.weak_bg_fill = CHIP_HOVER_FILL;
        widgets.active.weak_bg_fill = CHIP_PRESSED_FILL;
        style.visuals.weak_text_color = Some(MUTED_TEXT);
    });
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
    /// The FREQ view's live spectrum of the output.
    spectrum: Spectrum,
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
            spectrum: Spectrum::default(),
        }
    }
}

impl NiceEguiApp for JohnwaveEditor {
    /// Called each time the window opens.
    fn build(
        &mut self,
        egui_ctx: egui::Context,
        nice_gui_ctx: GuiContext,
        frame: &mut Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        apply_palette(&egui_ctx, frame);
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
        // The OUT line's glow in WAVE, or the spectrum in FREQ, only while it's showing
        // a note.
        let playing = self.display.load().is_some();
        let freq = self.params.screen_view.load(Ordering::Relaxed) == SCREEN_VIEW_FREQ;
        let out = !freq && playing && self.params.screen_out.load(Ordering::Relaxed);
        self.phosphor.update(now, out.then(|| self.capture.cycles(SCREEN_CYCLES, OUT_POINTS)).flatten());
        let spectrum = freq && playing && self.params.screen_spectrum.load(Ordering::Relaxed);
        let latest = spectrum.then(|| self.capture.latest(FFT_SIZE)).flatten();
        self.spectrum.update(now, latest.as_deref(), self.display.sample_rate(), FREQ_LOW, FREQ_HIGH);
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
                        knob_wave(ui, false, |phase| crate::osc::shape(phase, 0.0, morph));
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
                        knob_wave(ui, !osc2_on, |phase| crate::osc::shape(phase, 0.0, morph2));
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
                        knob_wave(ui, false, |phase| crate::lfo::wave(phase, lfo_shape, KNOB_WAVE_EDGE));
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
                // monitor in its housing, and the WAVE | FREQ switch under it.
                let title_height = panel_title(ui, "Scope").size().y;
                let height = SCOPE_HEIGHT - title_height - SCOPE_BEZEL;
                panel_with(ui, "Scope", SCOPE_BEZEL, SCOPE_BEZEL, 0.0, |ui| {
                    let lines = ScreenLines { phosphor: &self.phosphor, trail: &self.trail, spectrum: &self.spectrum };
                    osc_display(ui, p, self.display.load(), lines, self.display.sample_rate(), height);
                    ui.add_space(SCOPE_BEZEL);
                    view_switch(ui, &p.screen_view, &[(SCREEN_VIEW_WAVE, "WAVE"), (SCREEN_VIEW_FREQ, "FREQ")]);
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
/// dimmed in the mod colour when it's routed to at least one knob, and lit while it's armed.
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
            let color = MOD_COLOR;
            let on = armed == Some(source);
            let routed = is_routed(slots, source);
            let response = ui
                .scope(|ui| {
                    let selection = &mut ui.visuals_mut().selection;
                    selection.bg_fill = color.gamma_multiply(MOD_SELECTED_FILL);
                    selection.stroke.color = color;
                    let dim = color.gamma_multiply(MOD_ROUTED);
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
            let hint = if on { "Drag a target knob".to_string() } else { "Select modulation source".to_string() };
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

/// The readout box is a flat strip in the header, as wide as the middle and right
/// columns together, so long mod mode text fits.
const READOUT_WIDTH: f32 = MIDDLE_COLUMN_WIDTH + COLUMN_GAP + RIGHT_COLUMN_WIDTH;
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
    lines: ScreenLines,
    sample_rate: f32,
    height: f32,
) {
    let size = egui::vec2(ui.available_width(), height);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let mut painter = ui.painter_at(rect);
    // Faded in mod mode, like everything else that isn't a mod target.
    if in_mod_mode(ui) {
        painter.multiply_opacity(MOD_MODE_FADE);
    }

    screen_frame(&painter, rect, FAINT);

    // The chips across the top, inside the frame, and the view in the space below.
    let inner = rect.shrink(SCREEN_PADDING);
    let (chip_row, view) = inner.split_top_bottom_at_y(inner.top() + SCREEN_CHIP_ROW);
    screen_chips(ui, chip_row, p);
    if p.screen_view.load(Ordering::Relaxed) == SCREEN_VIEW_FREQ {
        freq_view(&painter, view, p, wave, lines.spectrum, sample_rate);
    } else {
        wave_view(ui, &painter, view, p, wave, lines.phosphor, lines.trail);
    }
}

/// What the window remembers between frames for the screen's live lines.
#[derive(Clone, Copy)]
struct ScreenLines<'a> {
    phosphor: &'a Phosphor,
    trail: &'a Trail,
    spectrum: &'a Spectrum,
}

/// The WAVE view: two cycles of Osc 1 across, with each line the chips switch on.
#[allow(clippy::too_many_arguments)]
fn wave_view(
    ui: &egui::Ui,
    painter: &egui::Painter,
    waves: egui::Rect,
    p: &SynthParams,
    wave: Option<ScreenWave>,
    phosphor: &Phosphor,
    trail: &Trail,
) {
    let show = |flag: &AtomicBool| flag.load(Ordering::Relaxed);
    let (show1, show2, show_mix) = (show(&p.screen_osc1), show(&p.screen_osc2), show(&p.screen_mix));
    let show_out = show(&p.screen_out);

    // A faint centre line at 0.
    painter.line_segment(
        [waves.left_center(), waves.right_center()],
        egui::Stroke::new(1.0, GRID),
    );

    // The trail first, so the main wave draws on top. Each ghost is just the mix, in
    // its own colour, and gets fainter as it ages (squared, so it fades quickly at
    // first, then lingers).
    if show_mix {
        let now = ui.input(|i| i.time);
        for (ghost, fade) in trail.ghosts(now) {
            let w = ghost.wave;
            let color = neon_color(w.morph).gamma_multiply(TRAIL_STRENGTH * fade * fade);
            screen_line(painter, waves, |x| w.mix(x), color, 1.0);
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
        screen_line(painter, waves, |x| w.osc1(x) * w.gain1 * w.level, part_color, part_width);
    }
    if show2 && w.has_osc2() {
        screen_line(painter, waves, |x| w.osc2(x) * w.gain2 * w.level, part_color, part_width);
    }
    // Then the two added up, bold on top. Scaled by the level, so it swells on attack
    // and shrinks away on release.
    if show_mix {
        screen_line(painter, waves, |x| w.mix(x), color, 2.0);
    }
    // Last and on top, the real output: what the voices actually played, after Crush,
    // the filter, unison and any other notes, lined up with Osc 1's cycles. Without any
    // of those it lands right on the MIX line. A thin soft white with a light glow, like
    // a real scope trace next to the drawn neon waves.
    if show_out {
        if let Some(out) = phosphor.line() {
            let last = (out.len() - 1) as f32;
            let curve = |x: f32| out[(x * last).round() as usize].clamp(-1.0, 1.0);
            neon_line_glow(painter, waves, curve, OUT_COLOR, 1.5, OUT_GLOW);
        }
    }
}

/// The FREQ view: low notes on the left, high on the right (20 Hz to 20 kHz, each
/// octave the same width, the way pitch is heard), loudness up the side in decibels.
/// It shows the filter's curve: what it lets through at each frequency. While a note
/// plays the curve follows that note's cutoff and resonance, mod routes and all;
/// otherwise it shows the knobs. Behind it, the live spectrum of the real output.
fn freq_view(
    painter: &egui::Painter,
    rect: egui::Rect,
    p: &SynthParams,
    wave: Option<ScreenWave>,
    spectrum: &Spectrum,
    sample_rate: f32,
) {
    // Where a frequency sits across, 0..1, and how a level in dB maps to a line value.
    let across = |hz: f32| (hz / FREQ_LOW).log2() / (FREQ_HIGH / FREQ_LOW).log2();
    let v_of_db = |db: f32| {
        let middle = (DB_TOP + DB_BOTTOM) / 2.0;
        ((db - middle) / ((DB_TOP - DB_BOTTOM) / 2.0)).clamp(-1.0, 1.0)
    };

    // Faint gridlines: 0 dB (unchanged) across, and 100 Hz, 1 kHz and 10 kHz up, each
    // with a tiny label at the bottom.
    let grid = egui::Stroke::new(1.0, GRID);
    let zero_y = line_y(rect, v_of_db(0.0));
    painter.line_segment([egui::pos2(rect.left(), zero_y), egui::pos2(rect.right(), zero_y)], grid);
    let label_font = egui::FontId::proportional(FREQ_LABEL_SIZE);
    for (hz, label) in [(100.0, "100"), (1_000.0, "1k"), (10_000.0, "10k")] {
        let x = rect.left() + across(hz) * rect.width();
        painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], grid);
        painter.text(
            egui::pos2(x + 2.0, rect.bottom()),
            egui::Align2::LEFT_BOTTOM,
            label,
            label_font.clone(),
            FAINT,
        );
    }

    // The spectrum, behind the curve. Its levels are of the sound itself (0 dBFS is a
    // full-scale sine), not how much the filter changes it, so it has its own scale:
    // 0 dBFS at the top, SPECTRUM_BOTTOM at the bottom. A soft filled hill in OUT's
    // white, with a thin glowing edge along the top.
    if let Some(levels) = spectrum.levels().filter(|_| p.screen_spectrum.load(Ordering::Relaxed)) {
        let v_of = |db: f32| (2.0 * (db - SPECTRUM_BOTTOM) / -SPECTRUM_BOTTOM - 1.0).clamp(-1.0, 1.0);
        let level_at = |x: f32| levels[((x * levels.len() as f32) as usize).min(levels.len() - 1)];
        // The fill: a strip of triangles between the top edge and the bottom, each
        // column's two corners meeting the next column's.
        let fill = OUT_COLOR.gamma_multiply(SPECTRUM_FILL);
        let mut mesh = egui::Mesh::default();
        for (c, &db) in levels.iter().enumerate() {
            let x = rect.left() + (c as f32 + 0.5) / levels.len() as f32 * rect.width();
            mesh.colored_vertex(egui::pos2(x, line_y(rect, v_of(db))), fill);
            mesh.colored_vertex(egui::pos2(x, rect.bottom()), fill);
            if c > 0 {
                let i = 2 * c as u32;
                mesh.add_triangle(i - 2, i - 1, i);
                mesh.add_triangle(i - 1, i + 1, i);
            }
        }
        painter.add(mesh);
        neon_curve(painter, rect, |x| v_of(level_at(x)), levels.len(), OUT_COLOR.gamma_multiply(SPECTRUM_EDGE), 1.0, OUT_GLOW);
    }

    if p.screen_filter.load(Ordering::Relaxed) {
        let (cutoff, resonance) = match wave {
            Some(w) => (w.cutoff_hz, w.resonance),
            None => (p.cutoff.value(), p.resonance.value()),
        };
        let (kind, slope) = (p.filter_type.value(), p.filter_slope.value());
        let curve = |x: f32| {
            let hz = FREQ_LOW * (FREQ_HIGH / FREQ_LOW).powf(x);
            let gain = crate::filter::response(kind, slope, cutoff, resonance, hz, sample_rate);
            // 20·log10 turns a gain into decibels. The tiny floor keeps a notch's
            // silent middle from being minus infinity.
            v_of_db(20.0 * gain.max(1e-6).log10())
        };
        neon_curve(painter, rect, curve, FREQ_STEPS, accent(), 2.0, 1.0);
    }
}

/// The FREQ view's range across, in Hz.
const FREQ_LOW: f32 = 20.0;
const FREQ_HIGH: f32 = 20_000.0;
/// Its range up the side, in dB. 0 dB (unchanged) sits 40% of the way down, leaving
/// room above for a resonant peak (up to +20 dB) and below for the cut.
const DB_TOP: f32 = 24.0;
const DB_BOTTOM: f32 = -36.0;
/// Segments in the filter curve: enough to catch a sharp resonant peak.
const FREQ_STEPS: usize = 400;
/// The spectrum's scale up the side: 0 dBFS at the top down to this, in dBFS.
const SPECTRUM_BOTTOM: f32 = -90.0;
/// How bright the spectrum's fill and its top edge are, so the filter curve stands out.
const SPECTRUM_FILL: f32 = 0.15;
const SPECTRUM_EDGE: f32 = 0.6;
/// The frequency labels' text size.
const FREQ_LABEL_SIZE: f32 = 9.0;

/// How many points the OUT line is made of. About one per screen point across.
const OUT_POINTS: usize = 240;
/// The OUT line's colour, also used for the FREQ view's spectrum. Set to the mod colour
/// for now; it used to be a soft, cool white: `from_rgb(0xE6, 0xEE, 0xF2)`.
const OUT_COLOR: egui::Color32 = MOD_COLOR;
/// How strong the OUT line's glow is, compared with the neon lines'.
const OUT_GLOW: f32 = 0.5;

/// Which view the screen shows (`SynthParams::screen_view`).
pub const SCREEN_VIEW_WAVE: u8 = 0;
pub const SCREEN_VIEW_FREQ: u8 = 1;

/// The chips along the top of the screen. In WAVE, `OSC 1  OSC 2  MIX  OUT` switch each line on and off (Osc 2's greys out
/// while Osc 2 is switched off). In FREQ, `FILTER  OUT` do the same for the filter's curve
/// and the output's spectrum.
/// They're window settings, not sound, so they're saved with the patch but aren't
/// parameters.
fn screen_chips(ui: &mut egui::Ui, row: egui::Rect, p: &SynthParams) {
    let mut chips = ui.new_child(egui::UiBuilder::new().max_rect(row));
    // Like the other buttons, they fade and stop taking clicks in mod mode.
    chips.add_enabled_ui(!in_mod_mode(ui), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.spacing_mut().button_padding = egui::vec2(4.0, 0.0);
            accent_selection(ui);
            let view = p.screen_view.load(Ordering::Relaxed);
            if view == SCREEN_VIEW_FREQ {
                screen_chip(ui, "FILTER", &p.screen_filter, true);
                screen_chip(ui, "OUT", &p.screen_spectrum, true);
            } else {
                screen_chip(ui, "OSC 1", &p.screen_osc1, true);
                screen_chip(ui, "OSC 2", &p.screen_osc2, p.osc2_on.value());
                screen_chip(ui, "MIX", &p.screen_mix, true);
                screen_chip(ui, "OUT", &p.screen_out, true);
            }
        });
    });
}

/// The `WAVE  FREQ` chips under the screen: which view it shows. The screen's own
/// setting, not a parameter. They look like the screen's other chips, with the current
/// one lit.
fn view_switch(ui: &mut egui::Ui, setting: &AtomicU8, choices: &[(u8, &str)]) {
    let current = setting.load(Ordering::Relaxed);
    ui.add_enabled_ui(!in_mod_mode(ui), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.spacing_mut().button_padding = egui::vec2(4.0, 0.0);
            accent_selection(ui);
            for &(choice, label) in choices {
                let text = egui::RichText::new(label).size(SCREEN_CHIP_TEXT);
                let response = ui.add(egui::Button::selectable(current == choice, text));
                if current != choice {
                    chip_outline(ui, &response, None);
                }
                if response.clicked() {
                    setting.store(choice, Ordering::Relaxed);
                }
            }
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
/// Space between a knob wave's frame and its line: a small `SCREEN_PADDING`.
const KNOB_WAVE_PADDING: f32 = 5.0;

/// One cycle of a wave, in a knob-sized square, for beside a Shape knob. Always the same
/// accent colour as the knob arcs: it shows what the knob is set to, not what's playing.
/// It's framed like a tiny copy of the oscillator screen, centre line and all. `off`
/// dims it like an off knob, for when its section is switched off.
fn knob_wave(ui: &mut egui::Ui, off: bool, wave: impl Fn(f32) -> f32) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(KNOB_WAVE_SIZE), egui::Sense::hover());
    let mut painter = ui.painter_at(rect);
    if off {
        painter.multiply_opacity(ARC_OFF);
    }
    screen_frame(&painter, rect, FAINT);
    let inner = rect.shrink(KNOB_WAVE_PADDING);
    painter.line_segment(
        [inner.left_center(), inner.right_center()],
        egui::Stroke::new(1.0, GRID),
    );
    wave_line(&painter, inner, 1.0, wave, accent(), 2.0);
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

/// Where a line's value `v` (-1 at the bottom, 1 at the top) lands in `rect`. It leaves
/// room top and bottom so the glow isn't cut off. The room is the same for every line,
/// however thick, so lines of different thicknesses share one scale and land on top of
/// each other when they match.
fn line_y(rect: egui::Rect, v: f32) -> f32 {
    let half_height = rect.height() / 2.0 - (THICKEST_LINE * 2.2 / 2.0 + 1.5);
    rect.center().y - v * half_height
}

/// `neon_line` with its glow scaled by `glow` (1 = the usual).
fn neon_line_glow(
    painter: &egui::Painter,
    rect: egui::Rect,
    curve: impl Fn(f32) -> f32,
    color: egui::Color32,
    thickness: f32,
    glow: f32,
) {
    neon_curve(painter, rect, curve, line_steps(rect), color, thickness, glow);
}

/// `neon_line_glow` made of `steps` segments, for curves with detail finer than the
/// usual spacing (like a sharp resonant peak).
fn neon_curve(
    painter: &egui::Painter,
    rect: egui::Rect,
    curve: impl Fn(f32) -> f32,
    steps: usize,
    color: egui::Color32,
    thickness: f32,
    glow: f32,
) {
    // Two wide, faint copies of the line underneath make the glow.
    let glow = [(thickness * 2.2, 0.10 * glow), (thickness * 1.5, 0.25 * glow)];
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let x = i as f32 / steps as f32;
            egui::pos2(rect.left() + x * rect.width(), line_y(rect, curve(x)))
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

/// The window title in the accent colour with a soft glow, like the knob arcs and waves.
fn glow_title(ui: &mut egui::Ui, text: &str) {
    let font = egui::TextStyle::Heading.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), accent());
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
                accent().gamma_multiply(strength),
            );
        }
    }
    // The sharp text on top.
    painter.galley(rect.min, galley, accent());
}

/// The title's glow rings: (how far out, in points; strength 0..1).
const TITLE_GLOW: [(f32, f32); 2] = [(3.0, 0.05), (1.5, 0.12)];

/// The accent colour: the knob arcs, toggles, readout value, title and the small waves.
pub fn accent() -> egui::Color32 {
    egui::ecolor::Hsva::new(ACCENT_HUE, 0.9, 1.0, 1.0).into()
}

/// The mod colour: the knobs' mod arcs and live ticks, the mod chips, and mod mode.
/// Pick something far round the colour wheel from the accent, so the two stand apart.
pub const MOD_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 251, 0);

/// Hue is a position on the colour wheel, 0..1 (0 = red, ⅓ = green, ½ = cyan, ⅔ = blue).
const ACCENT_HUE: f32 = 0.9;
/// How far the screen's colour drifts round the wheel as Shape goes from sine to square.
/// Keep it small so it still reads as the accent. Negative drifts the other way.
const HUE_DRIFT: f32 = 0.08;
/// Saturation at sine and at square. Less saturated looks paler and icier.
const SATURATION: (f32, f32) = (0.9, 0.65);

/// The screen wave's colour: the accent at sine, drifting gently round the wheel (and
/// paler) as Shape goes to square.
pub fn neon_color(morph: f32) -> egui::Color32 {
    let t = morph.clamp(0.0, 1.0);
    let hue = ACCENT_HUE + HUE_DRIFT * t;
    let saturation = SATURATION.0 + (SATURATION.1 - SATURATION.0) * t;
    // Full brightness is what keeps it reading as neon.
    egui::ecolor::Hsva::new(hue, saturation, 1.0, 1.0).into()
}

/// Space around the window's edges, left and right.
const MARGIN_X: f32 = 16.0;

/// Space between knobs in a row.
const KNOB_GAP: f32 = 14.0;

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
    ui.painter().layout_no_wrap(title.to_uppercase(), font, PANEL_GRAY)
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
    let gray = PANEL_GRAY;
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
const RIGHT_COLUMN_WIDTH: f32 = 240.0;
const COLUMN_GAP: f32 = 16.0;

/// How tall the Scope panel is down to the bottom of its screen, outline included. The
/// row of view chips under the screen adds to this.
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
            .color(PANEL_GRAY),
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
            accent_selection(ui);
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
    let color = color.unwrap_or(FAINT);
    let radius = ui.visuals().widgets.inactive.corner_radius;
    ui.painter().rect_stroke(response.rect, radius, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
}

/// Make "selected" buttons in this `ui` a dim accent fill with bright accent text, to match
/// the knob arcs. Call it inside `ui.scope` so the change stays with those buttons.
fn accent_selection(ui: &mut egui::Ui) {
    let accent = accent();
    let selection = &mut ui.visuals_mut().selection;
    selection.bg_fill = accent.gamma_multiply(SELECTED_FILL);
    selection.stroke.color = accent;
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
            .stroke(egui::Stroke::new(1.0, FAINT))
            .corner_radius(4.0)
            .inner_margin(2.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    accent_selection(ui);
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
