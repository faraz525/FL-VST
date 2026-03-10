use nih_plug::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use pattern_state::SharedPatternState;

/// Log to /tmp/dark_bassline.log for non-realtime diagnostics.
pub(crate) fn debug_log(msg: &str) {
    use std::io::Write;

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/dark_bassline.log")
    {
        let _ = writeln!(file, "{}", msg);
    }
}

mod editor;
mod midi_export;
mod pattern;
mod pattern_state;
mod rhythm;
mod scale;
mod sequencer;

use pattern::{GenerateParams, Pattern, PatternType, NUM_STEPS};
use scale::{RootNote, Scale};
use sequencer::{Sequencer, TransportState};

struct DarkBassline {
    params: Arc<DarkBasslineParams>,
    sequencer: Sequencer,
    current_pattern: Pattern,
    current_pattern_revision: u64,
    pattern_state: Arc<SharedPatternState>,

    // Shared state for UI
    ui_current_step: Arc<AtomicU8>,
    ui_is_playing: Arc<AtomicBool>,
    ui_tempo: Arc<Mutex<f64>>,
}

#[derive(Params)]
pub struct DarkBasslineParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<nih_plug_egui::EguiState>,

    #[id = "root"]
    pub root: EnumParam<RootNote>,

    #[id = "scale"]
    pub scale: EnumParam<Scale>,

    #[id = "ptype"]
    pub pattern_type: EnumParam<PatternType>,

    #[id = "dens"]
    pub density: IntParam,

    #[id = "oct"]
    pub octave: IntParam,

    #[id = "gate"]
    pub gate: FloatParam,

    #[id = "swing"]
    pub swing: FloatParam,

    #[id = "vel"]
    pub velocity: FloatParam,

    #[id = "vrng"]
    pub vel_range: FloatParam,
}

impl Default for DarkBasslineParams {
    fn default() -> Self {
        Self {
            editor_state: editor::default_editor_state(),

            root: EnumParam::new("Root", RootNote::C),

            scale: EnumParam::new("Scale", Scale::Phrygian),

            pattern_type: EnumParam::new("Pattern", PatternType::RootPulse),

            density: IntParam::new("Density", 4, IntRange::Linear { min: 1, max: 16 }),

            octave: IntParam::new("Octave", 2, IntRange::Linear { min: 1, max: 4 }),

            gate: FloatParam::new("Gate", 0.6, FloatRange::Linear { min: 0.1, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
                .with_string_to_value(Arc::new(|s| s.parse::<f32>().ok().map(|v| v / 100.0))),

            swing: FloatParam::new(
                "Swing",
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 0.25,
                },
            )
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
            .with_string_to_value(Arc::new(|s| s.parse::<f32>().ok().map(|v| v / 100.0))),

            velocity: FloatParam::new("Velocity", 0.8, FloatRange::Linear { min: 0.1, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
                .with_string_to_value(Arc::new(|s| s.parse::<f32>().ok().map(|v| v / 100.0))),

            vel_range: FloatParam::new("Vel Range", 0.1, FloatRange::Linear { min: 0.0, max: 0.5 })
                .with_unit("%")
                .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
                .with_string_to_value(Arc::new(|s| s.parse::<f32>().ok().map(|v| v / 100.0))),
        }
    }
}

impl Default for DarkBassline {
    fn default() -> Self {
        let params = Arc::new(DarkBasslineParams::default());
        let pattern_state = Arc::new(SharedPatternState::new(default_generate_params()));
        let current_pattern = pattern_state.current_pattern();
        let current_pattern_revision = pattern_state.revision();

        Self {
            params,
            sequencer: Sequencer::new(),
            current_pattern,
            current_pattern_revision,
            pattern_state,
            ui_current_step: Arc::new(AtomicU8::new(0)),
            ui_is_playing: Arc::new(AtomicBool::new(false)),
            ui_tempo: Arc::new(Mutex::new(120.0)),
        }
    }
}

pub(crate) fn default_generate_params() -> GenerateParams {
    GenerateParams {
        root: RootNote::C,
        scale: Scale::Phrygian,
        pattern_type: PatternType::RootPulse,
        density: 4,
        octave: 2,
        velocity: 0.8,
        vel_range: 0.1,
        gate: 0.6,
    }
}

pub(crate) fn current_generate_params(params: &DarkBasslineParams) -> GenerateParams {
    GenerateParams {
        root: params.root.value(),
        scale: params.scale.value(),
        pattern_type: params.pattern_type.value(),
        density: params.density.value() as u8,
        octave: params.octave.value() as u8,
        velocity: params.velocity.value(),
        vel_range: params.vel_range.value(),
        gate: params.gate.value(),
    }
}

impl DarkBassline {
    fn sync_pattern_state(&mut self) {
        self.pattern_state
            .sync_to_params(current_generate_params(self.params.as_ref()));

        if let Some(pattern) = self
            .pattern_state
            .try_pattern_if_newer(&mut self.current_pattern_revision)
        {
            self.current_pattern = pattern;
        }
    }
}

impl Plugin for DarkBassline {
    const NAME: &'static str = "Dark Bassline";
    const VENDOR: &'static str = "Faraz";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // Dummy stereo I/O for host compatibility (FL Studio, etc.)
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::MidiCCs;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(
            self.params.clone(),
            editor::UiState {
                current_step: self.ui_current_step.clone(),
                is_playing: self.ui_is_playing.clone(),
                pattern_state: self.pattern_state.clone(),
                tempo: self.ui_tempo.clone(),
            },
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        _buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sync_pattern_state();
        true
    }

    fn reset(&mut self) {
        self.sequencer.reset();
        self.ui_current_step.store(0, Ordering::Relaxed);
        self.ui_is_playing.store(false, Ordering::Relaxed);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.sync_pattern_state();

        let transport = TransportState::from_transport(context.transport());
        let buffer_len = buffer.samples() as u32;

        if let Ok(mut tempo) = self.ui_tempo.try_lock() {
            *tempo = transport.tempo;
        }

        self.sequencer.process::<Self>(
            buffer_len,
            &self.current_pattern,
            self.params.swing.value(),
            &transport,
            context,
        );

        self.ui_is_playing
            .store(self.sequencer.is_running(), Ordering::Relaxed);

        let beat_pos = self.sequencer.current_beat_pos();
        let step = ((beat_pos * 4.0).rem_euclid(NUM_STEPS as f64)) as u8;
        self.ui_current_step.store(step, Ordering::Relaxed);

        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for DarkBassline {
    const CLAP_ID: &'static str = "com.faraz.dark-bassline";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Generative MIDI bassline plugin for dark tech house");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::NoteEffect, ClapFeature::Utility];
}

impl Vst3Plugin for DarkBassline {
    const VST3_CLASS_ID: [u8; 16] = *b"DrkBassLn_Faraz!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Tools];
}

nih_export_clap!(DarkBassline);
nih_export_vst3!(DarkBassline);
