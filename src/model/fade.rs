//! Fade-mode math: hold at full alpha, then fade out over a fixed ramp.
//! Pure functions of the object's age.

/// Seconds the fade-out ramp lasts (after the configured hold).
pub const RAMP: f64 = 1.0;
/// Extra grace before a fully faded object is garbage-collected.
const GC_GRACE: f64 = 2.0;

/// Render alpha for an object `age` seconds old with `hold` seconds of
/// full visibility.
pub fn alpha(age: f64, hold: f64) -> f64 {
    if age <= hold {
        1.0
    } else {
        (1.0 - (age - hold) / RAMP).clamp(0.0, 1.0)
    }
}

/// True once the object should be removed from the scene entirely.
pub fn gc_due(age: f64, hold: f64) -> bool {
    age > hold + RAMP + GC_GRACE
}

/// True while the object still needs per-tick repaints.
pub fn is_fading(age: f64, hold: f64) -> bool {
    age > hold && alpha(age, hold) > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_curve_exact() {
        assert_eq!(alpha(0.0, 3.0), 1.0);
        assert_eq!(alpha(3.0, 3.0), 1.0);
        assert_eq!(alpha(3.5, 3.0), 0.5);
        assert_eq!(alpha(4.0, 3.0), 0.0);
        assert_eq!(alpha(10.0, 3.0), 0.0);
    }

    #[test]
    fn gc_after_grace() {
        assert!(!gc_due(4.0, 3.0));
        assert!(!gc_due(5.9, 3.0));
        assert!(gc_due(6.1, 3.0));
    }
}
