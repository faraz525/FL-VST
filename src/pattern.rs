use nih_plug::prelude::Enum;
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::rhythm;
use crate::scale::{self, RootNote, Scale};

pub const NUM_STEPS: usize = 16;

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternType {
    #[name = "Root Pulse"]
    RootPulse,
    Syncopated,
    #[name = "Rolling 16th"]
    Rolling16th,
    #[name = "Phrygian Drop"]
    PhrygianDrop,
    #[name = "Octave Bounce"]
    OctaveBounce,
}

#[derive(Debug, Clone, Copy)]
pub struct Step {
    pub active: bool,
    pub note: u8,
    pub velocity: f32,
    pub gate: f32,
}

impl Default for Step {
    fn default() -> Self {
        Self {
            active: false,
            note: 36,
            velocity: 0.8,
            gate: 0.6,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub steps: [Step; NUM_STEPS],
}

impl Default for Pattern {
    fn default() -> Self {
        Self {
            steps: [Step::default(); NUM_STEPS],
        }
    }
}

/// All inputs needed to generate a pattern.
pub struct GenerateParams {
    pub root: RootNote,
    pub scale: Scale,
    pub pattern_type: PatternType,
    pub density: u8,
    pub octave: u8,
    pub velocity: f32,
    pub vel_range: f32,
    pub gate: f32,
}

/// Generates a new 16-step pattern based on the given parameters.
pub fn generate(params: &GenerateParams, rng: &mut ChaCha8Rng) -> Pattern {
    let rhythm = rhythm::euclidean(params.density, NUM_STEPS as u8);

    let pitches = generate_pitches(params, &rhythm, rng);
    let velocities = generate_velocities(params, &rhythm, rng);

    let mut pattern = Pattern::default();
    for (i, step) in pattern.steps.iter_mut().enumerate() {
        step.active = rhythm[i];
        step.note = pitches[i];
        step.velocity = velocities[i];
        step.gate = params.gate;
    }

    pattern
}

fn generate_pitches(
    params: &GenerateParams,
    rhythm: &[bool; NUM_STEPS],
    rng: &mut ChaCha8Rng,
) -> [u8; NUM_STEPS] {
    let root = scale::root_midi_note(params.root, params.octave);

    match params.pattern_type {
        PatternType::RootPulse => pitch_root_pulse(params, root, rhythm, rng),
        PatternType::Syncopated => pitch_syncopated(params, root, rhythm, rng),
        PatternType::Rolling16th => pitch_rolling(params, root, rhythm, rng),
        PatternType::PhrygianDrop => pitch_phrygian_drop(params, root, rhythm),
        PatternType::OctaveBounce => pitch_octave_bounce(params, root, rhythm, rng),
    }
}

/// Root Pulse: Mostly root note, occasional 5th or semitone drop on beat 4.
fn pitch_root_pulse(
    params: &GenerateParams,
    root: u8,
    _rhythm: &[bool; NUM_STEPS],
    rng: &mut ChaCha8Rng,
) -> [u8; NUM_STEPS] {
    let fifth = scale::fifth_midi_note(params.root, params.scale, params.octave);
    let flat2 = scale::degree_to_midi(params.root, params.scale, params.octave, 1);

    let mut pitches = [root; NUM_STEPS];

    // Occasional variation on beats 3-4 area (steps 8-15)
    for i in 8..NUM_STEPS {
        let roll: f32 = rng.gen();
        if roll < 0.15 {
            pitches[i] = fifth;
        } else if roll < 0.25 {
            pitches[i] = flat2;
        }
    }

    pitches
}

/// Syncopated: Root on downbeats, scale tones on offbeats with ghost note feel.
fn pitch_syncopated(
    params: &GenerateParams,
    root: u8,
    rhythm: &[bool; NUM_STEPS],
    rng: &mut ChaCha8Rng,
) -> [u8; NUM_STEPS] {
    let intervals = params.scale.intervals();
    let mut pitches = [root; NUM_STEPS];

    for (i, &active) in rhythm.iter().enumerate() {
        if !active {
            continue;
        }

        let is_downbeat = i % 4 == 0;
        if is_downbeat {
            pitches[i] = root;
        } else {
            // Pick a scale tone, weighted toward root and fifth
            let (weights, len) = build_pitch_weights(intervals);
            let degree = weighted_pick(&weights, len, rng);
            pitches[i] = scale::degree_to_midi(
                params.root,
                params.scale,
                params.octave,
                degree as i8,
            );
        }
    }

    pitches
}

/// Rolling 16th: Root-heavy with subtle chromatic movement.
fn pitch_rolling(
    params: &GenerateParams,
    root: u8,
    _rhythm: &[bool; NUM_STEPS],
    rng: &mut ChaCha8Rng,
) -> [u8; NUM_STEPS] {
    let mut pitches = [root; NUM_STEPS];
    let fifth = scale::fifth_midi_note(params.root, params.scale, params.octave);

    for (i, pitch) in pitches.iter_mut().enumerate() {
        let roll: f32 = rng.gen();
        if i % 4 == 0 {
            *pitch = root; // Anchor downbeats
        } else if roll < 0.1 {
            *pitch = fifth;
        } else if roll < 0.15 {
            // One semitone below root for tension
            *pitch = root.saturating_sub(1);
        }
    }

    pitches
}

/// Phrygian Drop: Alternates between root and flat 2nd for signature dark tension.
fn pitch_phrygian_drop(
    params: &GenerateParams,
    root: u8,
    rhythm: &[bool; NUM_STEPS],
) -> [u8; NUM_STEPS] {
    let flat2 = scale::degree_to_midi(params.root, params.scale, params.octave, 1);
    let mut pitches = [root; NUM_STEPS];

    // Place flat-2nd on specific beats for tension-release
    for (i, &active) in rhythm.iter().enumerate() {
        if !active {
            continue;
        }
        // Flat-2nd on beats 2 and 4 area (steps 4-7 and 12-15)
        let in_tension_zone = (4..8).contains(&i) || (12..16).contains(&i);
        if in_tension_zone {
            pitches[i] = flat2;
        }
    }

    pitches
}

/// Octave Bounce: Alternates between root octave and one octave up.
fn pitch_octave_bounce(
    _params: &GenerateParams,
    root: u8,
    rhythm: &[bool; NUM_STEPS],
    rng: &mut ChaCha8Rng,
) -> [u8; NUM_STEPS] {
    let root_high = (root as u16 + 12).min(127) as u8;
    let mut pitches = [root; NUM_STEPS];
    let mut use_high = false;

    for (i, &active) in rhythm.iter().enumerate() {
        if !active {
            continue;
        }

        pitches[i] = if use_high { root_high } else { root };

        // Usually alternate, occasionally double-hit the same octave
        if rng.gen::<f32>() < 0.8 {
            use_high = !use_high;
        }
    }

    pitches
}

fn generate_velocities(
    params: &GenerateParams,
    rhythm: &[bool; NUM_STEPS],
    rng: &mut ChaCha8Rng,
) -> [f32; NUM_STEPS] {
    let mut velocities = [params.velocity; NUM_STEPS];

    for (i, vel) in velocities.iter_mut().enumerate() {
        if !rhythm[i] {
            continue;
        }

        let is_downbeat = i % 4 == 0;
        let is_backbeat = i % 4 == 2;

        // Accent pattern: strong on 1, medium on 3, softer elsewhere
        let accent = if is_downbeat {
            1.0
        } else if is_backbeat {
            0.9
        } else {
            0.7
        };

        // Apply randomization within range
        let jitter = if params.vel_range > 0.0 {
            rng.gen_range(-params.vel_range..params.vel_range)
        } else {
            0.0
        };

        *vel = (params.velocity * accent + jitter).clamp(0.05, 1.0);
    }

    velocities
}

/// Builds pitch weights for scale degrees on the stack.
/// Root = heaviest, perfect fifth (7 semitones) = heavy, others lighter.
fn build_pitch_weights(intervals: &[u8]) -> ([f32; 12], usize) {
    let len = intervals.len().min(12);
    let mut weights = [1.0f32; 12];

    if len > 0 {
        weights[0] = 5.0; // Root
    }
    // Find the perfect fifth (7 semitones) by interval value, not index
    for (i, &interval) in intervals.iter().enumerate() {
        if interval == 7 {
            weights[i] = 3.0;
            break;
        }
    }
    (weights, len)
}

/// Picks an index from a weighted distribution (up to `len` entries).
fn weighted_pick(weights: &[f32; 12], len: usize, rng: &mut ChaCha8Rng) -> usize {
    let total: f32 = weights[..len].iter().sum();
    if total <= 0.0 {
        return 0;
    }
    let mut roll = rng.gen_range(0.0..total);
    for (i, &w) in weights[..len].iter().enumerate() {
        roll -= w;
        if roll <= 0.0 {
            return i;
        }
    }
    len.saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn test_rng() -> ChaCha8Rng {
        ChaCha8Rng::seed_from_u64(42)
    }

    #[test]
    fn test_generate_produces_correct_density() {
        let params = GenerateParams {
            root: RootNote::C,
            scale: Scale::Phrygian,
            pattern_type: PatternType::RootPulse,
            density: 4,
            octave: 2,
            velocity: 0.8,
            vel_range: 0.1,
            gate: 0.6,
        };
        let pattern = generate(&params, &mut test_rng());
        let active_count = pattern.steps.iter().filter(|s| s.active).count();
        assert_eq!(active_count, 4);
    }

    #[test]
    fn test_all_pattern_types_generate() {
        let types = [
            PatternType::RootPulse,
            PatternType::Syncopated,
            PatternType::Rolling16th,
            PatternType::PhrygianDrop,
            PatternType::OctaveBounce,
        ];
        for pt in types {
            let params = GenerateParams {
                root: RootNote::C,
                scale: Scale::Phrygian,
                pattern_type: pt,
                density: 4,
                octave: 2,
                velocity: 0.8,
                vel_range: 0.1,
                gate: 0.6,
            };
            let pattern = generate(&params, &mut test_rng());
            assert!(pattern.steps.iter().any(|s| s.active), "{pt:?} produced no active steps");
        }
    }

    #[test]
    fn test_velocities_in_range() {
        let params = GenerateParams {
            root: RootNote::C,
            scale: Scale::NaturalMinor,
            pattern_type: PatternType::Syncopated,
            density: 8,
            octave: 2,
            velocity: 0.8,
            vel_range: 0.2,
            gate: 0.6,
        };
        let pattern = generate(&params, &mut test_rng());
        for step in &pattern.steps {
            if step.active {
                assert!(step.velocity >= 0.05 && step.velocity <= 1.0);
            }
        }
    }
}
