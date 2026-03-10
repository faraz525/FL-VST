use nih_plug::prelude::*;

use crate::pattern::{Pattern, Step, NUM_STEPS};

/// Owned snapshot of host transport state, extracted before passing context mutably.
#[derive(Clone, Copy)]
pub struct TransportState {
    pub playing: bool,
    pub sample_rate: f32,
    pub tempo: f64,
    pub pos_beats: Option<f64>,
}

impl TransportState {
    pub fn from_transport(transport: &Transport) -> Self {
        Self {
            playing: transport.playing,
            sample_rate: transport.sample_rate,
            tempo: transport.tempo.unwrap_or(120.0),
            pos_beats: transport.pos_beats(),
        }
    }
}

/// Tracks active notes that need note-off events.
struct ActiveNote {
    note: u8,
    channel: u8,
    /// Sample position (absolute) when note-off should fire.
    off_sample: u64,
}

/// Transport-driven sequencer that falls back to an internal clock when beat position
/// is unavailable, but never plays unless the host transport is running.
pub struct Sequencer {
    /// Current step index (0..15). None means no step has played yet.
    current_step: Option<usize>,
    /// Whether the sequencer is running (controlled by host transport OR free-run).
    running: bool,
    /// Internal beat position tracked by sample counting.
    internal_beat_pos: f64,
    /// Active notes waiting for note-off.
    active_notes: Vec<ActiveNote>,
    /// Accumulates total samples processed for note-off scheduling.
    samples_processed: u64,
    /// Previous host beat position for detecting movement.
    last_host_beat: f64,
    /// Whether host transport was detected as working.
    host_transport_works: bool,
    /// Number of process calls to evaluate host transport.
    eval_count: u32,
}

impl Sequencer {
    pub fn new() -> Self {
        Self {
            current_step: None,
            running: false,
            internal_beat_pos: 0.0,
            active_notes: Vec::with_capacity(16),
            samples_processed: 0,
            last_host_beat: -1.0,
            host_transport_works: false,
            eval_count: 0,
        }
    }

    /// Returns the current internal beat position (for UI step indicator).
    pub fn current_beat_pos(&self) -> f64 {
        self.internal_beat_pos
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Reset the sequencer state.
    pub fn reset(&mut self) {
        self.current_step = None;
        self.running = false;
        self.internal_beat_pos = 0.0;
        self.active_notes.clear();
        self.samples_processed = 0;
        self.last_host_beat = -1.0;
        self.host_transport_works = false;
        self.eval_count = 0;
    }

    /// Main process function. Uses host beat position when it advances, otherwise
    /// falls back to a tempo-synced internal clock while transport is playing.
    pub fn process<P: Plugin<SysExMessage = ()>>(
        &mut self,
        buffer_len: u32,
        pattern: &Pattern,
        swing_amount: f32,
        transport: &TransportState,
        context: &mut impl ProcessContext<P>,
    ) {
        let sample_rate = transport.sample_rate as f64;
        let bpm = transport.tempo;
        let samples_per_beat = sample_rate * 60.0 / bpm;
        let samples_per_16th = samples_per_beat / 4.0;

        // Determine if host transport is working by checking if pos_beats ever advances
        let host_beat = transport.pos_beats.unwrap_or(0.0);
        if self.eval_count < 2000 {
            self.eval_count += 1;
            if transport.playing && host_beat != self.last_host_beat && self.last_host_beat >= 0.0 {
                self.host_transport_works = true;
            }
        }

        let is_playing = transport.playing;

        // Handle stop
        if !is_playing {
            if self.running {
                self.send_all_notes_off(0, context);
            }
            self.reset();
            return;
        }

        // Handle start
        if !self.running {
            self.reset();
            if self.host_transport_works {
                self.internal_beat_pos = host_beat;
            }
        }
        self.running = true;

        // Detect transport jump when using host transport
        if self.host_transport_works && self.last_host_beat >= 0.0 {
            let jump = (host_beat - self.last_host_beat).abs();
            if jump > 0.5 {
                self.send_all_notes_off(0, context);
                self.current_step = None;
                self.internal_beat_pos = host_beat;
            }
        }

        let start_beat = if self.host_transport_works {
            host_beat
        } else {
            self.internal_beat_pos
        };

        // Process each sample in the buffer
        for sample_offset in 0..buffer_len {
            let current_beat = start_beat + (sample_offset as f64 / samples_per_beat);

            // Which 16th note are we on within the bar?
            let sixteenth_pos = (current_beat * 4.0).rem_euclid(NUM_STEPS as f64);
            let step_index = sixteenth_pos as usize;

            // Detect step transition
            let is_new_step = match self.current_step {
                None => true,
                Some(prev) => step_index != prev,
            };

            if is_new_step {
                self.current_step = Some(step_index);

                let step = &pattern.steps[step_index];
                if step.active {
                    // Apply swing: delay odd-numbered 16th notes
                    let swing_delay_samples = if step_index % 2 == 1 {
                        (samples_per_16th * swing_amount as f64 * 0.5) as u64
                    } else {
                        0
                    };

                    let timing = (sample_offset as u64 + swing_delay_samples)
                        .min(buffer_len as u64 - 1) as u32;

                    let gate_samples = (samples_per_16th * step.gate as f64) as u64;
                    let off_abs = self.samples_processed + timing as u64 + gate_samples;

                    self.trigger_note(step, timing, off_abs, context);
                }
            }

            // Check for pending note-offs
            self.process_note_offs(sample_offset, context);
        }

        // Advance internal clock
        self.internal_beat_pos = start_beat + (buffer_len as f64 / samples_per_beat);
        self.last_host_beat = host_beat;
        self.samples_processed += buffer_len as u64;
    }

    fn trigger_note<P: Plugin<SysExMessage = ()>>(
        &mut self,
        step: &Step,
        timing: u32,
        off_sample: u64,
        context: &mut impl ProcessContext<P>,
    ) {
        // Send note-off for any currently active note on the same channel
        self.send_note_off_for(step.note, 0, timing, context);

        context.send_event(NoteEvent::NoteOn {
            timing,
            voice_id: None,
            channel: 0,
            note: step.note,
            velocity: step.velocity,
        });

        self.active_notes.push(ActiveNote {
            note: step.note,
            channel: 0,
            off_sample,
        });
    }

    fn process_note_offs<P: Plugin<SysExMessage = ()>>(
        &mut self,
        sample_offset: u32,
        context: &mut impl ProcessContext<P>,
    ) {
        let abs_sample = self.samples_processed + sample_offset as u64;

        let mut i = 0;
        while i < self.active_notes.len() {
            if self.active_notes[i].off_sample <= abs_sample {
                let note = self.active_notes.swap_remove(i);
                context.send_event(NoteEvent::NoteOff {
                    timing: sample_offset,
                    voice_id: None,
                    channel: note.channel,
                    note: note.note,
                    velocity: 0.0,
                });
            } else {
                i += 1;
            }
        }
    }

    fn send_note_off_for<P: Plugin<SysExMessage = ()>>(
        &mut self,
        note: u8,
        channel: u8,
        timing: u32,
        context: &mut impl ProcessContext<P>,
    ) {
        let mut i = 0;
        while i < self.active_notes.len() {
            if self.active_notes[i].note == note && self.active_notes[i].channel == channel {
                let removed = self.active_notes.swap_remove(i);
                context.send_event(NoteEvent::NoteOff {
                    timing,
                    voice_id: None,
                    channel: removed.channel,
                    note: removed.note,
                    velocity: 0.0,
                });
            } else {
                i += 1;
            }
        }
    }

    fn send_all_notes_off<P: Plugin<SysExMessage = ()>>(
        &mut self,
        timing: u32,
        context: &mut impl ProcessContext<P>,
    ) {
        for note in self.active_notes.drain(..) {
            context.send_event(NoteEvent::NoteOff {
                timing,
                voice_id: None,
                channel: note.channel,
                note: note.note,
                velocity: 0.0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Params)]
    struct TestParams {
        #[id = "dummy"]
        dummy: FloatParam,
    }

    impl Default for TestParams {
        fn default() -> Self {
            Self {
                dummy: FloatParam::new("Dummy", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 }),
            }
        }
    }

    struct TestPlugin {
        params: Arc<TestParams>,
    }

    impl Default for TestPlugin {
        fn default() -> Self {
            Self {
                params: Arc::new(TestParams::default()),
            }
        }
    }

impl Plugin for TestPlugin {
        const NAME: &'static str = "Test";
        const VENDOR: &'static str = "Test";
        const URL: &'static str = "";
        const EMAIL: &'static str = "";
        const VERSION: &'static str = "0.0.0";
        const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[];

        type SysExMessage = ();
        type BackgroundTask = ();

        fn params(&self) -> Arc<dyn Params> {
            self.params.clone()
        }

        fn process(
            &mut self,
            _buffer: &mut Buffer,
            _aux: &mut AuxiliaryBuffers,
            _context: &mut impl ProcessContext<Self>,
        ) -> ProcessStatus {
            ProcessStatus::Normal
        }
    }

    #[derive(Default)]
    struct TestProcessContext {
        events: Vec<PluginNoteEvent<TestPlugin>>,
    }

    impl ProcessContext<TestPlugin> for TestProcessContext {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Standalone
        }

        fn execute_background(&self, _task: <TestPlugin as Plugin>::BackgroundTask) {}

        fn execute_gui(&self, _task: <TestPlugin as Plugin>::BackgroundTask) {}

        fn transport(&self) -> &Transport {
            panic!("transport() is not used in these tests")
        }

        fn next_event(&mut self) -> Option<PluginNoteEvent<TestPlugin>> {
            None
        }

        fn send_event(&mut self, event: PluginNoteEvent<TestPlugin>) {
            self.events.push(event);
        }

        fn set_latency_samples(&self, _samples: u32) {}

        fn set_current_voice_capacity(&self, _capacity: u32) {}
    }

    fn pattern_with_single_hit() -> Pattern {
        let mut pattern = Pattern::default();
        pattern.steps[0] = Step {
            active: true,
            note: 36,
            velocity: 0.8,
            gate: 0.75,
        };
        pattern
    }

    fn playing_transport(pos_beats: Option<f64>) -> TransportState {
        TransportState {
            playing: true,
            sample_rate: 48_000.0,
            tempo: 120.0,
            pos_beats,
        }
    }

    fn stopped_transport() -> TransportState {
        TransportState {
            playing: false,
            sample_rate: 48_000.0,
            tempo: 120.0,
            pos_beats: None,
        }
    }

    #[test]
    fn stopped_transport_does_not_emit_notes_by_default() {
        let mut sequencer = Sequencer::new();
        let mut context = TestProcessContext::default();

        sequencer.process::<TestPlugin>(
            512,
            &pattern_with_single_hit(),
            0.0,
            &stopped_transport(),
            &mut context,
        );

        assert!(context.events.is_empty());
        assert!(!sequencer.running);
    }

    #[test]
    fn reset_clears_active_note_and_transport_tracking_state() {
        let mut sequencer = Sequencer::new();
        let mut context = TestProcessContext::default();

        sequencer.process::<TestPlugin>(
            64,
            &pattern_with_single_hit(),
            0.0,
            &playing_transport(Some(0.0)),
            &mut context,
        );

        assert!(!sequencer.active_notes.is_empty());
        assert!(sequencer.running);
        assert!(sequencer.samples_processed > 0);

        sequencer.reset();

        assert!(sequencer.active_notes.is_empty());
        assert!(!sequencer.running);
        assert_eq!(sequencer.samples_processed, 0);
        assert_eq!(sequencer.last_host_beat, -1.0);
        assert!(!sequencer.host_transport_works);
        assert_eq!(sequencer.eval_count, 0);
    }

    #[test]
    fn restart_after_reset_does_not_emit_stale_note_offs() {
        let mut sequencer = Sequencer::new();
        let mut context = TestProcessContext::default();

        sequencer.process::<TestPlugin>(
            64,
            &pattern_with_single_hit(),
            0.0,
            &playing_transport(Some(0.0)),
            &mut context,
        );
        sequencer.reset();
        context.events.clear();

        sequencer.process::<TestPlugin>(
            64,
            &pattern_with_single_hit(),
            0.0,
            &playing_transport(Some(0.0)),
            &mut context,
        );

        assert!(matches!(
            context.events.as_slice(),
            [NoteEvent::NoteOn { .. }]
        ));
    }
}
