use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use crate::pattern_state::SharedPatternState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportStatus {
    Idle,
    Pending,
    Cancelled,
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportDebugInfo {
    pub status: ExportStatus,
    pub path: Option<String>,
    pub error: Option<String>,
}

impl Default for ExportDebugInfo {
    fn default() -> Self {
        Self {
            status: ExportStatus::Idle,
            path: None,
            error: None,
        }
    }
}

pub struct SharedDebugState {
    transport_playing: AtomicBool,
    transport_tempo_bits: AtomicU64,
    has_pos_beats: AtomicBool,
    transport_pos_beats_bits: AtomicU64,
    export: Mutex<ExportDebugInfo>,
}

impl SharedDebugState {
    pub fn new() -> Self {
        Self {
            transport_playing: AtomicBool::new(false),
            transport_tempo_bits: AtomicU64::new(120.0f64.to_bits()),
            has_pos_beats: AtomicBool::new(false),
            transport_pos_beats_bits: AtomicU64::new(0),
            export: Mutex::new(ExportDebugInfo::default()),
        }
    }

    pub fn update_transport(&self, playing: bool, tempo: f64, pos_beats: Option<f64>) {
        self.transport_playing.store(playing, Ordering::Relaxed);
        self.transport_tempo_bits
            .store(tempo.to_bits(), Ordering::Relaxed);

        match pos_beats {
            Some(pos_beats) => {
                self.has_pos_beats.store(true, Ordering::Relaxed);
                self.transport_pos_beats_bits
                    .store(pos_beats.to_bits(), Ordering::Relaxed);
            }
            None => {
                self.has_pos_beats.store(false, Ordering::Relaxed);
            }
        }
    }

    pub fn transport_playing(&self) -> bool {
        self.transport_playing.load(Ordering::Relaxed)
    }

    pub fn transport_tempo(&self) -> f64 {
        f64::from_bits(self.transport_tempo_bits.load(Ordering::Relaxed))
    }

    pub fn transport_pos_beats(&self) -> Option<f64> {
        self.has_pos_beats
            .load(Ordering::Relaxed)
            .then(|| f64::from_bits(self.transport_pos_beats_bits.load(Ordering::Relaxed)))
    }

    pub fn export_info(&self) -> ExportDebugInfo {
        self.export.lock().expect("debug state poisoned").clone()
    }

    pub fn set_export_pending(&self) {
        let mut export = self.export.lock().expect("debug state poisoned");
        export.status = ExportStatus::Pending;
        export.path = None;
        export.error = None;
    }

    pub fn set_export_cancelled(&self) {
        let mut export = self.export.lock().expect("debug state poisoned");
        export.status = ExportStatus::Cancelled;
        export.path = None;
        export.error = None;
    }

    pub fn set_export_succeeded(&self, path: String) {
        let mut export = self.export.lock().expect("debug state poisoned");
        export.status = ExportStatus::Success;
        export.path = Some(path);
        export.error = None;
    }

    pub fn set_export_failed(&self, path: Option<String>, error: String) {
        let mut export = self.export.lock().expect("debug state poisoned");
        export.status = ExportStatus::Error;
        export.path = path;
        export.error = Some(error);
    }

    pub fn build_snapshot(
        &self,
        current_step: u8,
        sequencer_running: bool,
        pattern_state: &SharedPatternState,
    ) -> String {
        let transport_playing = self.transport_playing();
        let transport_tempo = self.transport_tempo();
        let transport_pos_beats = self.transport_pos_beats();
        let export = self.export_info();
        let pattern = pattern_state.current_pattern();
        let pattern_revision = pattern_state.revision();
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
        let pos_beats = transport_pos_beats
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "none".to_string());
        let export_path = export.path.as_deref().unwrap_or("none");
        let export_error = export.error.as_deref().unwrap_or("none");

        format!(
            "transport.playing={transport_playing}\n\
             transport.tempo={transport_tempo:.2}\n\
             transport.pos_beats={pos_beats}\n\
             sequencer.running={sequencer_running}\n\
             current_step={current_step}\n\
             pattern.revision={pattern_revision}\n\
             pattern.active_steps={active_steps}\n\
             pattern.preview={preview}\n\
             export.status={}\n\
             export.path={export_path}\n\
             export.error={export_error}",
            export.status.as_str()
        )
    }
}

impl ExportStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Pending => "pending",
            Self::Cancelled => "cancelled",
            Self::Success => "success",
            Self::Error => "error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::{GenerateParams, PatternType};
    use crate::scale::{RootNote, Scale};

    fn params() -> GenerateParams {
        GenerateParams {
            root: RootNote::C,
            scale: Scale::Phrygian,
            pattern_type: PatternType::Syncopated,
            density: 4,
            octave: 2,
            velocity: 0.8,
            vel_range: 0.1,
            gate: 0.6,
        }
    }

    #[test]
    fn export_status_roundtrip_keeps_latest_result() {
        let debug = SharedDebugState::new();

        debug.set_export_pending();
        debug.set_export_succeeded("C:/tmp/test.mid".to_string());

        let info = debug.export_info();
        assert_eq!(info.status, ExportStatus::Success);
        assert_eq!(info.path.as_deref(), Some("C:/tmp/test.mid"));
        assert_eq!(info.error, None);
    }

    #[test]
    fn snapshot_contains_transport_pattern_and_export_details() {
        let debug = SharedDebugState::new();
        let pattern_state = SharedPatternState::new_with_seed(params(), 42);

        debug.update_transport(true, 128.0, Some(12.5));
        debug.set_export_failed(Some("C:/tmp/fail.mid".to_string()), "disk full".to_string());

        let snapshot = debug.build_snapshot(7, true, &pattern_state);

        assert!(snapshot.contains("transport.playing=true"));
        assert!(snapshot.contains("transport.tempo=128.00"));
        assert!(snapshot.contains("transport.pos_beats=12.500"));
        assert!(snapshot.contains("sequencer.running=true"));
        assert!(snapshot.contains("current_step=7"));
        assert!(snapshot.contains("pattern.revision=0"));
        assert!(snapshot.contains("export.status=error"));
        assert!(snapshot.contains("export.path=C:/tmp/fail.mid"));
        assert!(snapshot.contains("export.error=disk full"));
        assert!(snapshot.contains("pattern.preview="));
    }
}
