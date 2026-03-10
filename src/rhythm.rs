/// Generates a Euclidean rhythm pattern using the Bjorklund algorithm.
/// Distributes `hits` onsets as evenly as possible across `steps` positions.
///
/// Returns a boolean array where `true` = active step.
///
/// Examples:
///   euclidean(3, 8)  = [true, false, false, true, false, false, true, false]
///   euclidean(4, 16) = [true, false, false, false, true, false, false, false, ...]
pub fn euclidean(hits: u8, steps: u8) -> [bool; 16] {
    let mut pattern = [false; 16];
    let steps = steps.min(16) as usize;
    let hits = (hits as usize).min(steps);

    if hits == 0 || steps == 0 {
        return pattern;
    }

    if hits >= steps {
        for slot in pattern.iter_mut().take(steps) {
            *slot = true;
        }
        return pattern;
    }

    // Bresenham-style Euclidean rhythm — zero allocations
    for (i, slot) in pattern.iter_mut().enumerate().take(steps) {
        if (i * hits) % steps < hits {
            *slot = true;
        }
    }

    pattern
}

#[cfg(test)]
/// Rotates a pattern by `offset` steps to the right.
/// Rotation shifts the downbeat, creating syncopation.
pub fn rotate(pattern: &[bool; 16], offset: u8) -> [bool; 16] {
    let mut rotated = [false; 16];
    let offset = offset as usize % 16;
    for (i, slot) in rotated.iter_mut().enumerate() {
        *slot = pattern[(i + 16 - offset) % 16];
    }
    rotated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_euclidean_4_16() {
        let p = euclidean(4, 16);
        assert_eq!(p.iter().filter(|&&x| x).count(), 4);
        // Should be evenly spaced: every 4th step
        assert!(p[0]);
        assert!(p[4]);
        assert!(p[8]);
        assert!(p[12]);
    }

    #[test]
    fn test_euclidean_3_8() {
        let p = euclidean(3, 8);
        assert_eq!(p.iter().take(8).filter(|&&x| x).count(), 3);
    }

    #[test]
    fn test_euclidean_0() {
        let p = euclidean(0, 16);
        assert!(p.iter().all(|&x| !x));
    }

    #[test]
    fn test_euclidean_full() {
        let p = euclidean(16, 16);
        assert!(p.iter().all(|&x| x));
    }

    #[test]
    fn test_rotate() {
        let p = euclidean(4, 16);
        let r = rotate(&p, 2);
        // Original: [1,0,0,0,1,0,0,0,1,0,0,0,1,0,0,0]
        // Rotated 2: [0,0,1,0,0,0,1,0,0,0,1,0,0,0,1,0]
        assert!(r[2]);
        assert!(r[6]);
        assert!(r[10]);
        assert!(r[14]);
    }
}
