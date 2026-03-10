use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::pattern::{self, GenerateParams, Pattern};

struct PatternStateInner {
    params: GenerateParams,
    pattern: Pattern,
    rng: ChaCha8Rng,
}

pub struct SharedPatternState {
    revision: AtomicU64,
    inner: Mutex<PatternStateInner>,
}

impl SharedPatternState {
    pub fn new(initial_params: GenerateParams) -> Self {
        Self::with_rng(initial_params, ChaCha8Rng::from_entropy())
    }

    #[cfg(test)]
    pub fn new_with_seed(initial_params: GenerateParams, seed: u64) -> Self {
        Self::with_rng(initial_params, ChaCha8Rng::seed_from_u64(seed))
    }

    fn with_rng(initial_params: GenerateParams, mut rng: ChaCha8Rng) -> Self {
        let pattern = pattern::generate(&initial_params, &mut rng);

        Self {
            revision: AtomicU64::new(0),
            inner: Mutex::new(PatternStateInner {
                params: initial_params,
                pattern,
                rng,
            }),
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn current_pattern(&self) -> Pattern {
        self.inner
            .lock()
            .expect("pattern state poisoned")
            .pattern
            .clone()
    }

    pub fn sync_to_params(&self, params: GenerateParams) -> bool {
        let mut inner = self.inner.lock().expect("pattern state poisoned");
        if inner.params == params {
            return false;
        }

        inner.params = params;
        let current_params = inner.params;
        inner.pattern = pattern::generate(&current_params, &mut inner.rng);
        self.revision.fetch_add(1, Ordering::Release);
        true
    }

    pub fn generate_new_variation(&self, params: GenerateParams) {
        let mut inner = self.inner.lock().expect("pattern state poisoned");
        inner.params = params;
        let current_params = inner.params;
        inner.pattern = pattern::generate(&current_params, &mut inner.rng);
        self.revision.fetch_add(1, Ordering::Release);
    }

    pub fn try_pattern_if_newer(&self, last_seen_revision: &mut u64) -> Option<Pattern> {
        let candidate_revision = self.revision.load(Ordering::Acquire);
        if candidate_revision == *last_seen_revision {
            return None;
        }

        let pattern = self.inner.try_lock().ok()?.pattern.clone();
        *last_seen_revision = self.revision.load(Ordering::Acquire);
        Some(pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::PatternType;
    use crate::scale::{RootNote, Scale};

    fn params(root: RootNote, density: u8, pattern_type: PatternType) -> GenerateParams {
        GenerateParams {
            root,
            scale: Scale::Phrygian,
            pattern_type,
            density,
            octave: 2,
            velocity: 0.8,
            vel_range: 0.2,
            gate: 0.6,
        }
    }

    fn pattern_signature(pattern: &Pattern) -> Vec<(bool, u8, u8)> {
        pattern
            .steps
            .iter()
            .map(|step| (step.active, step.note, (step.velocity * 127.0).round() as u8))
            .collect()
    }

    #[test]
    fn sync_to_params_regenerates_when_generation_params_change() {
        let initial = params(RootNote::C, 4, PatternType::Syncopated);
        let updated = params(RootNote::DSharp, 7, PatternType::OctaveBounce);
        let state = SharedPatternState::new_with_seed(initial, 42);

        let before = pattern_signature(&state.current_pattern());

        assert!(state.sync_to_params(updated));

        let after = pattern_signature(&state.current_pattern());
        assert_ne!(before, after);
    }

    #[test]
    fn sync_to_params_is_a_noop_for_identical_generation_params() {
        let initial = params(RootNote::C, 6, PatternType::Syncopated);
        let state = SharedPatternState::new_with_seed(params(RootNote::C, 6, PatternType::Syncopated), 7);

        let before = pattern_signature(&state.current_pattern());

        assert!(!state.sync_to_params(initial));

        let after = pattern_signature(&state.current_pattern());
        assert_eq!(before, after);
    }

    #[test]
    fn generate_new_variation_changes_pattern_without_param_changes() {
        let initial = params(RootNote::FSharp, 8, PatternType::Syncopated);
        let state = SharedPatternState::new_with_seed(params(RootNote::FSharp, 8, PatternType::Syncopated), 99);

        let before = pattern_signature(&state.current_pattern());

        state.generate_new_variation(initial);

        let after = pattern_signature(&state.current_pattern());
        assert_ne!(before, after);
    }
}
