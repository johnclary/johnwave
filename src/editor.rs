//! The plugin window: a deliberately minimal egui UI.
//!
//! egui is "immediate mode": every frame, `ui()` describes the whole window from scratch
//! ("a title, then three sliders"). egui works out the layout and handles the mouse. There's
//! no widget tree to keep in sync. The window only reads and sets parameters. No sound
//! logic lives here.

use crate::SynthParams;
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::dpi::LogicalSize;
use nice_plug_egui::widgets::ParamSlider;
use nice_plug_egui::{EguiEditorState, Frame, NiceEguiApp};
use std::sync::Arc;

/// Window size in points. On a Retina screen egui draws at twice this many pixels.
pub const WIDTH: f64 = 340.0;
pub const HEIGHT: f64 = 150.0;

pub fn default_state() -> Arc<EguiEditorState> {
    EguiEditorState::from_size(LogicalSize::new(WIDTH, HEIGHT), 1.0)
}

pub struct JohnwaveEditor {
    /// The same parameters the audio thread reads. `Arc` means both sides share one copy.
    params: Arc<SynthParams>,
    /// How the window talks to the host (to change parameters, with undo and automation).
    /// It only exists while the window is open, so it starts as `None`.
    gui_context: Option<GuiContext>,
}

impl JohnwaveEditor {
    pub fn new(params: Arc<SynthParams>) -> Self {
        Self {
            params,
            gui_context: None,
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
        // A `ParamSetter` turns slider drags into parameter changes the DAW can see and
        // record as automation.
        let setter = gui_context.param_setter();
        let params = &self.params;

        ui.heading("johnwave");
        ui.add_space(8.0);

        // A two-column grid: label on the left, slider on the right.
        egui::Grid::new("params").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Morph");
            ui.add(ParamSlider::for_param(&params.morph, &setter));
            ui.end_row();

            ui.label("Cutoff");
            ui.add(ParamSlider::for_param(&params.cutoff, &setter));
            ui.end_row();

            ui.label("LFO Shape");
            ui.add(ParamSlider::for_param(&params.lfo_shape, &setter));
            ui.end_row();
        });
    }
}
