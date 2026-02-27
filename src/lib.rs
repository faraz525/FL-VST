use nih_plug::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use std::io::Write;

/// Log to /tmp/dark_bassline.log for debugging. Only call on rare events, not per-sample.
fn debug_log(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/dark_bassline.log")
    {
        let _ = writeln!(f, "{}", msg);
    }
}

mod editor;
mod pattern;
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
    rng: ChaCha8Rng,

    // Shared state for UI
    ui_current_step: Arc<AtomicU8>,
    ui_is_playing: Arc<AtomicBool>,

    /// Atomic trigger for pattern regeneration (set by UI, consumed by audio thread).
    generate_trigger: Arc<AtomicBool>,

    /// Counter to throttle diagnostic logging (log every N process calls).
    debug_process_count: u64,
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

            gate: FloatParam::new(
                "Gate",
                0.6,
                FloatRange::Linear { min: 0.1, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
            .with_string_to_value(Arc::new(|s| s.parse::<f32>().ok().map(|v| v / 100.0))),

            swing: FloatParam::new(
                "Swing",
                0.0,
                FloatRange::Linear { min: 0.0, max: 0.25 },
            )
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
            .with_string_to_value(Arc::new(|s| s.parse::<f32>().ok().map(|v| v / 100.0))),

            velocity: FloatParam::new(
                "Velocity",
                0.8,
                FloatRange::Linear { min: 0.1, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
            .with_string_to_value(Arc::new(|s| s.parse::<f32>().ok().map(|v| v / 100.0))),

            vel_range: FloatParam::new(
                "Vel Range",
                0.1,
                FloatRange::Linear { min: 0.0, max: 0.5 },
            )
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
            .with_string_to_value(Arc::new(|s| s.parse::<f32>().ok().map(|v| v / 100.0))),
        }
    }
}

impl Default for DarkBassline {
    fn default() -> Self {
        let params = Arc::new(DarkBasslineParams::default());
        let mut rng = ChaCha8Rng::from_entropy();

        let initial_pattern = pattern::generate(
            &GenerateParams {
                root: RootNote::C,
                scale: Scale::Phrygian,
                pattern_type: PatternType::RootPulse,
                density: 4,
                octave: 2,
                velocity: 0.8,
                vel_range: 0.1,
                gate: 0.6,
            },
            &mut rng,
        );

        Self {
            params,
            sequencer: Sequencer::new(),
            current_pattern: initial_pattern,
            rng,
            ui_current_step: Arc::new(AtomicU8::new(0)),
            ui_is_playing: Arc::new(AtomicBool::new(false)),
            generate_trigger: Arc::new(AtomicBool::new(false)),
            debug_process_count: 0,
        }
    }
}

impl DarkBassline {
    fn regenerate_pattern(&mut self) {
        let gen_params = GenerateParams {
            root: self.params.root.value(),
            scale: self.params.scale.value(),
            pattern_type: self.params.pattern_type.value(),
            density: self.params.density.value() as u8,
            octave: self.params.octave.value() as u8,
            velocity: self.params.velocity.value(),
            vel_range: self.params.vel_range.value(),
            gate: self.params.gate.value(),
        };
        self.current_pattern = pattern::generate(&gen_params, &mut self.rng);
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
                generate_trigger: self.generate_trigger.clone(),
            },
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        debug_log(&format!(
            "Dark Bassline initialized: sample_rate={}, max_buffer_size={}",
            buffer_config.sample_rate, buffer_config.max_buffer_size
        ));
        self.regenerate_pattern();
        let active = self.current_pattern.steps.iter().filter(|s| s.active).count();
        debug_log(&format!("Initial pattern: {} active steps", active));
        true
    }

    fn reset(&mut self) {
        self.sequencer.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Check atomic trigger from UI (compare-exchange consumes the trigger)
        if self
            .generate_trigger
            .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            debug_log("Generate triggered — regenerating pattern");
            self.regenerate_pattern();
            let active = self.current_pattern.steps.iter().filter(|s| s.active).count();
            debug_log(&format!("New pattern: {} active steps", active));
        }

        // Snapshot transport state into owned struct to avoid borrow conflict with context
        let transport = TransportState::from_transport(context.transport());
        let buffer_len = buffer.samples() as u32;

        // Log transport diagnostics every ~1 second (44100/192 ≈ 230 calls/sec)
        self.debug_process_count += 1;
        if self.debug_process_count % 500 == 1 {
            debug_log(&format!(
                "process() call #{}: playing={}, pos_beats={:?}, tempo={:.1}, buf_len={}",
                self.debug_process_count, transport.playing, transport.pos_beats, transport.tempo, buffer_len
            ));
        }

        // Log transport state once when play starts
        if transport.playing && !self.ui_is_playing.load(Ordering::Relaxed) {
            debug_log(&format!(
                "Transport started: bpm={:.1}, pos_beats={:?}, sample_rate={}",
                transport.tempo, transport.pos_beats, transport.sample_rate
            ));
            let active = self.current_pattern.steps.iter().filter(|s| s.active).count();
            debug_log(&format!("Current pattern has {} active steps", active));
        }

        // The sequencer always runs (free-running if host transport is broken)
        self.ui_is_playing.store(true, Ordering::Relaxed);

        // Run the sequencer
        self.sequencer.process::<Self>(
            buffer_len,
            &self.current_pattern,
            self.params.swing.value(),
            &transport,
            context,
        );

        // Update UI step indicator from sequencer's internal beat position
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
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::NoteEffect,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for DarkBassline {
    const VST3_CLASS_ID: [u8; 16] = *b"DrkBassLn_Faraz!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Instrument,
        Vst3SubCategory::Tools,
    ];
}

nih_export_clap!(DarkBassline);
nih_export_vst3!(DarkBassline);
