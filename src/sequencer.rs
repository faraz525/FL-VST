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

/// Free-running sequencer that uses an internal sample clock.
/// Syncs to host tempo but does NOT depend on transport.playing or pos_beats,
/// which FL Studio often doesn't provide reliably to VST3 plugins.
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

    /// Reset the sequencer state.
    pub fn reset(&mut self) {
        self.current_step = None;
        self.internal_beat_pos = 0.0;
        self.last_host_beat = -1.0;
    }

    /// Main process function. Uses host transport if it works,
    /// otherwise free-runs an internal clock synced to host tempo.
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

        // Determine effective playing state and beat position
        let is_playing = if self.host_transport_works {
            transport.playing
        } else {
            // Host transport broken — always run
            true
        };

        // Handle stop
        if !is_playing {
            if self.running {
                self.send_all_notes_off(0, context);
                self.reset();
            }
            self.running = false;
            self.last_host_beat = host_beat;
            self.samples_processed += buffer_len as u64;
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
