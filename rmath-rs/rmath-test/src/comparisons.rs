/// Bitwise comparison utilities for f64 numerical equivalence testing.

/// Compare two f64 values bitwise. Returns true if identical bit patterns.
#[allow(dead_code)]
pub fn bits_equal(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

/// Check if both values are NaN (any NaN bit pattern).
#[allow(dead_code)]
pub fn both_nan(a: f64, b: f64) -> bool {
    a.is_nan() && b.is_nan()
}

/// Compare two f64 values, treating all NaN bit patterns as equal.
pub fn equiv(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    a.to_bits() == b.to_bits()
}

/// Assert bitwise equivalence, panicking on mismatch.
pub fn assert_equiv(a: f64, b: f64, label: &str) {
    if !equiv(a, b) {
        panic!(
            "{}: expected {:?} (0x{:016x}), got {:?} (0x{:016x})",
            label,
            b,
            b.to_bits(),
            a,
            a.to_bits()
        );
    }
}

/// Check that a value is NaN, panicking if not.
pub fn assert_nan(a: f64, label: &str) {
    if !a.is_nan() {
        panic!(
            "{}: expected NaN, got {:?} (0x{:016x})",
            label,
            a,
            a.to_bits()
        );
    }
}

/// Check that a value equals positive infinity, panicking if not.
pub fn assert_posinf(a: f64, label: &str) {
    if !(a.is_infinite() && a.is_sign_positive()) {
        panic!("{}: expected +Inf, got {:?}", label, a);
    }
}

/// Check that a value equals negative infinity, panicking if not.
pub fn assert_neginf(a: f64, label: &str) {
    if !(a.is_infinite() && a.is_sign_negative()) {
        panic!("{}: expected -Inf, got {:?}", label, a);
    }
}
