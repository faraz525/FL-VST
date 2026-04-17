use nih_plug::prelude::*;
use nih_plug_egui::egui;
use nih_plug_egui::{create_egui_editor, EguiState};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use crate::debug_state::{ExportStatus, SharedDebugState};
use crate::midi_export::{self, MidiExportParams};
use crate::pattern_state::SharedPatternState;
use crate::{current_generate_params, DarkBasslineParams};

/// Shared state from the DSP thread for UI display.
pub struct UiState {
    pub current_step: Arc<AtomicU8>,
    pub is_playing: Arc<AtomicBool>,
    /// Shared generated pattern state for live preview and MIDI export.
    pub pattern_state: Arc<SharedPatternState>,
    /// Shared debug state for transport/export diagnostics.
    pub debug_state: Arc<SharedDebugState>,
    /// Current tempo from host transport for MIDI export.
    pub tempo: Arc<Mutex<f64>>,
}

pub fn default_editor_state() -> Arc<EguiState> {
    EguiState::from_size(560, 400)
}

pub fn create(params: Arc<DarkBasslineParams>, ui_state: UiState) -> Option<Box<dyn Editor>> {
    create_egui_editor(
        params.editor_state.clone(),
        ui_state,
        |ctx, _state| {
            setup_style(ctx);
        },
        move |ctx, setter, state| {
            draw_ui(ctx, setter, &params, state);
        },
    )
}

fn setup_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Dark theme
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(18, 18, 22);
    visuals.window_fill = egui::Color32::from_rgb(18, 18, 22);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(35, 35, 42);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 50, 60);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(70, 50, 90);
    visuals.override_text_color = Some(egui::Color32::from_rgb(200, 200, 210));

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    ctx.set_style(style);
}

fn draw_ui(
    ctx: &egui::Context,
    setter: &ParamSetter,
    params: &DarkBasslineParams,
    state: &mut UiState,
) {
    state
        .pattern_state
        .sync_to_params(current_generate_params(params));

    egui::CentralPanel::default().show(ctx, |ui| {
        // Header
        ui.vertical_centered(|ui| {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("DARK BASSLINE")
                    .size(16.0)
                    .color(egui::Color32::from_rgb(160, 120, 200))
                    .strong(),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("generative midi")
                    .size(10.0)
                    .color(egui::Color32::from_rgb(100, 100, 120)),
            );
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        // Row 1: Root, Scale, Pattern Type
        ui.horizontal(|ui| {
            param_with_label(ui, "ROOT", &params.root, setter);
            ui.add_space(8.0);
            param_with_label(ui, "SCALE", &params.scale, setter);
            ui.add_space(8.0);
            param_with_label(ui, "PATTERN", &params.pattern_type, setter);
        });

        ui.add_space(8.0);

        // Row 2: Density, Octave, Gate, Swing
        ui.horizontal(|ui| {
            param_with_label(ui, "DENSITY", &params.density, setter);
            ui.add_space(8.0);
            param_with_label(ui, "OCTAVE", &params.octave, setter);
            ui.add_space(8.0);
            param_with_label(ui, "GATE", &params.gate, setter);
            ui.add_space(8.0);
            param_with_label(ui, "SWING", &params.swing, setter);
        });

        ui.add_space(8.0);

        // Row 3: Velocity, Vel Range
        ui.horizontal(|ui| {
            param_with_label(ui, "VELOCITY", &params.velocity, setter);
            ui.add_space(8.0);
            param_with_label(ui, "VEL RNG", &params.vel_range, setter);
        });

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        // Action buttons
        ui.vertical_centered(|ui| {
            // Generate button
            let gen_btn = ui.add_sized(
                [180.0, 32.0],
                egui::Button::new(
                    egui::RichText::new("GENERATE")
                        .size(13.0)
                        .color(egui::Color32::from_rgb(200, 200, 220)),
                )
                .fill(egui::Color32::from_rgb(60, 40, 80)),
            );
            if gen_btn.clicked() {
                state
                    .pattern_state
                    .generate_new_variation(current_generate_params(params));
            }

            ui.add_space(4.0);

            // Export MIDI button
            let export_btn = ui.add_sized(
                [180.0, 32.0],
                egui::Button::new(
                    egui::RichText::new("EXPORT MIDI")
                        .size(13.0)
                        .color(egui::Color32::from_rgb(200, 210, 230)),
                )
                .fill(egui::Color32::from_rgb(35, 50, 80)),
            );
            if export_btn.clicked() {
                let pattern = state.pattern_state.current_pattern();
                let tempo = state.tempo.lock().map(|g| *g).unwrap_or(120.0);
                let debug_state = state.debug_state.clone();
                debug_state.set_export_pending();

                std::thread::spawn(move || {
                    let dialog = rfd::FileDialog::new()
                        .add_filter("MIDI", &["mid"])
                        .set_file_name("dark-bassline.mid")
                        .save_file();

                    if let Some(path) = dialog {
                        let path_string = path.display().to_string();
                        let params = MidiExportParams {
                            pattern,
                            tempo_bpm: tempo,
                        };
                        if let Err(e) = midi_export::export_to_file(&params, &path) {
                            debug_state.set_export_failed(Some(path_string), e.to_string());
                        } else {
                            debug_state.set_export_succeeded(path_string);
                        }
                    } else {
                        debug_state.set_export_cancelled();
                    }
                });
            }
        });

        ui.add_space(8.0);

        // Step indicator
        draw_step_indicator(ui, state);

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);
        draw_debug_panel(ctx, ui, state);
    });

    // Request continuous repaint while playing (for step indicator)
    if state.is_playing.load(Ordering::Relaxed) {
        ctx.request_repaint();
    }
}

fn draw_step_indicator(ui: &mut egui::Ui, state: &UiState) {
    let current = state.current_step.load(Ordering::Relaxed) as usize;
    let playing = state.is_playing.load(Ordering::Relaxed);

    ui.vertical_centered(|ui| {
        let available_width = ui.available_width().min(420.0);
        let step_width = (available_width - 15.0 * 2.0) / 16.0;
        let step_height = 6.0;

        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(available_width, step_height),
            egui::Sense::hover(),
        );

        let painter = ui.painter_at(rect);

        for i in 0..16 {
            let x = rect.min.x + i as f32 * (step_width + 2.0);
            let step_rect = egui::Rect::from_min_size(
                egui::pos2(x, rect.min.y),
                egui::vec2(step_width, step_height),
            );

            let color = if playing && i == current {
                egui::Color32::from_rgb(160, 120, 200) // Active step: purple
            } else if i % 4 == 0 {
                egui::Color32::from_rgb(55, 55, 65) // Downbeat marker
            } else {
                egui::Color32::from_rgb(35, 35, 42) // Inactive
            };

            painter.rect_filled(step_rect, 1.0, color);
        }
    });
}

fn draw_debug_panel(ctx: &egui::Context, ui: &mut egui::Ui, state: &UiState) {
    let export_info = state.debug_state.export_info();
    let pattern = state.pattern_state.current_pattern();
    let active_steps = pattern.steps.iter().filter(|step| step.active).count();
    let preview = pattern
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.active)
        .take(8)
        .map(|(index, step)| format!("{index}:{}@{:.0}", step.note, step.velocity * 127.0))
        .collect::<Vec<_>>()
        .join(", ");
    let current_step = state.current_step.load(Ordering::Relaxed);
    let sequencer_running = state.is_playing.load(Ordering::Relaxed);
    let snapshot =
        state
            .debug_state
            .build_snapshot(current_step, sequencer_running, &state.pattern_state);

    egui::CollapsingHeader::new("DEBUG")
        .default_open(false)
        .show(ui, |ui| {
            ui.monospace(format!(
                "transport.playing={}",
                state.debug_state.transport_playing()
            ));
            ui.monospace(format!(
                "transport.tempo={:.2}",
                state.debug_state.transport_tempo()
            ));

            let pos_beats = state
                .debug_state
                .transport_pos_beats()
                .map(|value| format!("{value:.3}"))
                .unwrap_or_else(|| "none".to_string());
            ui.monospace(format!("transport.pos_beats={pos_beats}"));

            ui.monospace(format!("sequencer.running={sequencer_running}"));
            ui.monospace(format!("current_step={current_step}"));
            ui.monospace(format!(
                "pattern.revision={}",
                state.pattern_state.revision()
            ));
            ui.monospace(format!("pattern.active_steps={active_steps}"));
            ui.monospace(format!("pattern.preview={preview}"));
            ui.monospace(format!("export.status={}", export_info.status.as_str()));
            ui.monospace(format!(
                "export.path={}",
                export_info.path.as_deref().unwrap_or("none")
            ));
            ui.monospace(format!(
                "export.error={}",
                export_info.error.as_deref().unwrap_or("none")
            ));

            ui.add_space(6.0);
            if ui.button("COPY DEBUG SNAPSHOT").clicked() {
                ctx.copy_text(snapshot);
            }

            if matches!(export_info.status, ExportStatus::Error) {
                ui.add_space(4.0);
                ui.colored_label(
                    egui::Color32::from_rgb(220, 120, 120),
                    "Last export failed. Copy the debug snapshot and include it with the repro.",
                );
            }
        });
}

fn param_with_label<P: Param>(ui: &mut egui::Ui, label: &str, param: &P, setter: &ParamSetter) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(9.0)
                .color(egui::Color32::from_rgb(120, 120, 140)),
        );
        ui.add(nih_plug_egui::widgets::ParamSlider::for_param(param, setter).with_width(90.0));
    });
}
