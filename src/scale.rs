use nih_plug::prelude::Enum;

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootNote {
    C,
    #[name = "C#"]
    CSharp,
    D,
    #[name = "D#"]
    DSharp,
    E,
    F,
    #[name = "F#"]
    FSharp,
    G,
    #[name = "G#"]
    GSharp,
    A,
    #[name = "A#"]
    ASharp,
    B,
}

impl RootNote {
    pub fn midi_offset(self) -> u8 {
        match self {
            RootNote::C => 0,
            RootNote::CSharp => 1,
            RootNote::D => 2,
            RootNote::DSharp => 3,
            RootNote::E => 4,
            RootNote::F => 5,
            RootNote::FSharp => 6,
            RootNote::G => 7,
            RootNote::GSharp => 8,
            RootNote::A => 9,
            RootNote::ASharp => 10,
            RootNote::B => 11,
        }
    }
}

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Phrygian,
    #[name = "Natural Minor"]
    NaturalMinor,
    #[name = "Harmonic Minor"]
    HarmonicMinor,
    #[name = "Minor Pentatonic"]
    MinorPentatonic,
    Dorian,
}

impl Scale {
    /// Semitone intervals from root for this scale.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Scale::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Scale::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Scale::MinorPentatonic => &[0, 3, 5, 7, 10],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
        }
    }
}

/// Resolves a scale degree to a MIDI note number.
/// `degree` is an index into the scale intervals (can exceed scale length for octave wrapping).
/// Returns a MIDI note clamped to 0..=127.
pub fn degree_to_midi(root: RootNote, scale: Scale, octave: u8, degree: i8) -> u8 {
    let intervals = scale.intervals();
    let len = intervals.len() as i8;

    let octave_offset = if degree >= 0 {
        degree / len
    } else {
        (degree - len + 1) / len
    };
    let index = degree.rem_euclid(len) as usize;

    let base = (octave as i16 + 1) * 12 + root.midi_offset() as i16;
    let note = base + intervals[index] as i16 + octave_offset as i16 * 12;

    note.clamp(0, 127) as u8
}

/// Returns the root MIDI note for a given root + octave.
pub fn root_midi_note(root: RootNote, octave: u8) -> u8 {
    let note = (octave as u16 + 1) * 12 + root.midi_offset() as u16;
    note.min(127) as u8
}

/// Returns the perfect fifth (7 semitones) MIDI note for the given scale.
/// Finds the fifth by interval value rather than assuming a fixed degree index.
pub fn fifth_midi_note(root: RootNote, scale: Scale, octave: u8) -> u8 {
    let intervals = scale.intervals();
    let degree = intervals
        .iter()
        .position(|&i| i == 7)
        .unwrap_or(4) as i8;
    degree_to_midi(root, scale, octave, degree)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_midi_note() {
        assert_eq!(root_midi_note(RootNote::C, 2), 36); // C2
        assert_eq!(root_midi_note(RootNote::A, 3), 57); // A3
    }

    #[test]
    fn test_degree_to_midi_phrygian() {
        // C Phrygian: C, Db, Eb, F, G, Ab, Bb
        assert_eq!(degree_to_midi(RootNote::C, Scale::Phrygian, 2, 0), 36); // C2
        assert_eq!(degree_to_midi(RootNote::C, Scale::Phrygian, 2, 1), 37); // Db2
        assert_eq!(degree_to_midi(RootNote::C, Scale::Phrygian, 2, 4), 43); // G2 (fifth)
    }

    #[test]
    fn test_negative_degree() {
        // Going below root should wrap to lower octave
        let note = degree_to_midi(RootNote::C, Scale::Phrygian, 2, -1);
        assert_eq!(note, 34); // Bb1
    }

    #[test]
    fn test_clamp() {
        assert_eq!(degree_to_midi(RootNote::C, Scale::Phrygian, 0, -20), 0);
        assert_eq!(degree_to_midi(RootNote::C, Scale::Phrygian, 9, 20), 127);
    }
}
