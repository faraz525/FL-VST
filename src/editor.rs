use nih_plug::prelude::*;
use nih_plug_egui::egui;
use nih_plug_egui::{create_egui_editor, EguiState};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use crate::DarkBasslineParams;

/// Shared state from the DSP thread for UI display.
pub struct UiState {
    pub current_step: Arc<AtomicU8>,
    pub is_playing: Arc<AtomicBool>,
    /// Atomic trigger: UI sets to true, audio thread consumes.
    pub generate_trigger: Arc<AtomicBool>,
}

pub fn default_editor_state() -> Arc<EguiState> {
    EguiState::from_size(560, 360)
}

pub fn create(
    params: Arc<DarkBasslineParams>,
    ui_state: UiState,
) -> Option<Box<dyn Editor>> {
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

        // Generate button
        ui.vertical_centered(|ui| {
            let btn = ui.add_sized(
                [180.0, 32.0],
                egui::Button::new(
                    egui::RichText::new("GENERATE")
                        .size(13.0)
                        .color(egui::Color32::from_rgb(200, 200, 220)),
                )
                .fill(egui::Color32::from_rgb(60, 40, 80)),
            );
            if btn.clicked() {
                state.generate_trigger.store(true, Ordering::Relaxed);
            }
        });

        ui.add_space(8.0);

        // Step indicator
        draw_step_indicator(ui, state);
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

fn param_with_label<'a, P: Param>(
    ui: &mut egui::Ui,
    label: &str,
    param: &P,
    setter: &ParamSetter,
) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(9.0)
                .color(egui::Color32::from_rgb(120, 120, 140)),
        );
        ui.add(nih_plug_egui::widgets::ParamSlider::for_param(param, setter).with_width(90.0));
    });
}
