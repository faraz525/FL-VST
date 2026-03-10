use midly::num::{u24, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use std::path::Path;

use crate::pattern::{Pattern, NUM_STEPS};

const TICKS_PER_QUARTER: u16 = 480;
const TICKS_PER_STEP: u32 = (TICKS_PER_QUARTER as u32) / 4; // 120 ticks per 16th note
const MIDI_CHANNEL: u4 = u4::new(0);

pub struct MidiExportParams {
    pub pattern: Pattern,
    pub tempo_bpm: f64,
}

#[derive(Debug)]
pub enum ExportError {
    Io(std::io::Error),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Io(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl From<std::io::Error> for ExportError {
    fn from(e: std::io::Error) -> Self {
        ExportError::Io(e)
    }
}

fn bpm_to_microseconds_per_beat(bpm: f64) -> u32 {
    let bpm = bpm.clamp(20.0, 300.0);
    (60_000_000.0 / bpm) as u32
}

pub fn export_to_file(params: &MidiExportParams, path: &Path) -> Result<(), ExportError> {
    let track = build_track(params);

    let smf = Smf {
        header: Header {
            format: Format::SingleTrack,
            timing: Timing::Metrical(TICKS_PER_QUARTER.into()),
        },
        tracks: vec![track],
    };

    smf.save(path).map_err(ExportError::Io)
}

fn build_track(params: &MidiExportParams) -> Vec<TrackEvent<'static>> {
    let mut events: Vec<TrackEvent<'static>> = Vec::new();

    // Tempo meta event at tick 0
    let tempo_us = bpm_to_microseconds_per_beat(params.tempo_bpm);
    events.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(tempo_us))),
    });

    let mut accumulated_delta: u32 = 0;

    for (i, step) in params.pattern.steps.iter().enumerate() {
        if step.active {
            let velocity = (step.velocity * 127.0).round().clamp(1.0, 127.0) as u8;
            let gate_ticks = ((step.gate * TICKS_PER_STEP as f32).round() as u32)
                .clamp(1, TICKS_PER_STEP);

            // NoteOn
            events.push(TrackEvent {
                delta: u28::new(accumulated_delta),
                kind: TrackEventKind::Midi {
                    channel: MIDI_CHANNEL,
                    message: MidiMessage::NoteOn {
                        key: u7::new(step.note),
                        vel: u7::new(velocity),
                    },
                },
            });

            // NoteOff after gate duration
            events.push(TrackEvent {
                delta: u28::new(gate_ticks),
                kind: TrackEventKind::Midi {
                    channel: MIDI_CHANNEL,
                    message: MidiMessage::NoteOff {
                        key: u7::new(step.note),
                        vel: u7::new(0),
                    },
                },
            });

            // Remaining silence in this step after the note ends
            let remaining = TICKS_PER_STEP.saturating_sub(gate_ticks);
            accumulated_delta = remaining;
        } else {
            accumulated_delta += TICKS_PER_STEP;
        }

        // If this is the last step, account for any remaining silence
        // by using it as delta on the end-of-track event
        if i == NUM_STEPS - 1 {
            // accumulated_delta carries forward to end-of-track
        }
    }

    // End-of-track meta event
    events.push(TrackEvent {
        delta: u28::new(accumulated_delta),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::{Pattern, Step};

    fn empty_pattern() -> Pattern {
        Pattern::default()
    }

    fn single_step_pattern() -> Pattern {
        let mut pattern = Pattern::default();
        pattern.steps[0] = Step {
            active: true,
            note: 36,
            velocity: 0.8,
            gate: 0.6,
        };
        pattern
    }

    fn all_active_pattern() -> Pattern {
        let mut pattern = Pattern::default();
        for step in pattern.steps.iter_mut() {
            step.active = true;
            step.note = 36;
            step.velocity = 0.8;
            step.gate = 0.5;
        }
        pattern
    }

    #[test]
    fn test_empty_pattern_produces_valid_track() {
        let params = MidiExportParams {
            pattern: empty_pattern(),
            tempo_bpm: 120.0,
        };
        let track = build_track(&params);

        // Should have tempo + end-of-track = 2 events
        assert_eq!(track.len(), 2);
        assert!(matches!(
            track[0].kind,
            TrackEventKind::Meta(MetaMessage::Tempo(_))
        ));
        assert!(matches!(
            track[1].kind,
            TrackEventKind::Meta(MetaMessage::EndOfTrack)
        ));

        // End-of-track delta should be 16 steps * 120 ticks = 1920
        assert_eq!(track[1].delta.as_int(), NUM_STEPS as u32 * TICKS_PER_STEP);
    }

    #[test]
    fn test_single_active_step() {
        let params = MidiExportParams {
            pattern: single_step_pattern(),
            tempo_bpm: 130.0,
        };
        let track = build_track(&params);

        // tempo + NoteOn + NoteOff + EndOfTrack = 4 events
        assert_eq!(track.len(), 4);

        // NoteOn at delta 0 (right after tempo)
        let note_on = &track[1];
        assert_eq!(note_on.delta.as_int(), 0);
        match note_on.kind {
            TrackEventKind::Midi { message, .. } => match message {
                MidiMessage::NoteOn { key, vel } => {
                    assert_eq!(key.as_int(), 36);
                    assert_eq!(vel.as_int(), 102); // 0.8 * 127 = 101.6 -> 102
                }
                _ => panic!("Expected NoteOn"),
            },
            _ => panic!("Expected Midi event"),
        }

        // NoteOff with gate=0.6 -> 0.6 * 120 = 72 ticks
        let note_off = &track[2];
        assert_eq!(note_off.delta.as_int(), 72);
    }

    #[test]
    fn test_all_active_steps() {
        let params = MidiExportParams {
            pattern: all_active_pattern(),
            tempo_bpm: 120.0,
        };
        let track = build_track(&params);

        // tempo + (NoteOn + NoteOff) * 16 + EndOfTrack = 1 + 32 + 1 = 34
        assert_eq!(track.len(), 34);

        // Verify total ticks sum to 16 * 120 = 1920
        let total_ticks: u32 = track.iter().map(|e| e.delta.as_int()).sum();
        assert_eq!(total_ticks, NUM_STEPS as u32 * TICKS_PER_STEP);
    }

    #[test]
    fn test_tempo_conversion() {
        assert_eq!(bpm_to_microseconds_per_beat(120.0), 500_000);
        assert_eq!(bpm_to_microseconds_per_beat(60.0), 1_000_000);
        assert_eq!(bpm_to_microseconds_per_beat(140.0), 428_571);
    }

    #[test]
    fn test_velocity_clamping() {
        let mut pattern = Pattern::default();
        pattern.steps[0] = Step {
            active: true,
            note: 48,
            velocity: 1.0, // max
            gate: 0.5,
        };
        pattern.steps[1] = Step {
            active: true,
            note: 48,
            velocity: 0.01, // very low
            gate: 0.5,
        };

        let params = MidiExportParams {
            pattern,
            tempo_bpm: 120.0,
        };
        let track = build_track(&params);

        // Check max velocity step
        match track[1].kind {
            TrackEventKind::Midi {
                message: MidiMessage::NoteOn { vel, .. },
                ..
            } => {
                assert_eq!(vel.as_int(), 127);
            }
            _ => panic!("Expected NoteOn"),
        }

        // Check min velocity step (0.01 * 127 = 1.27 -> clamped to 1)
        match track[3].kind {
            TrackEventKind::Midi {
                message: MidiMessage::NoteOn { vel, .. },
                ..
            } => {
                assert_eq!(vel.as_int(), 1);
            }
            _ => panic!("Expected NoteOn"),
        }
    }

    #[test]
    fn test_export_to_file_roundtrip() {
        let params = MidiExportParams {
            pattern: single_step_pattern(),
            tempo_bpm: 128.0,
        };

        let dir = std::env::temp_dir().join("dark_bassline_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_export.mid");

        export_to_file(&params, &path).expect("export should succeed");

        // Verify file exists and is valid MIDI
        let data = std::fs::read(&path).expect("should read file");
        let smf = Smf::parse(&data).expect("should parse as valid MIDI");
        assert_eq!(smf.tracks.len(), 1);

        // Cleanup
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
