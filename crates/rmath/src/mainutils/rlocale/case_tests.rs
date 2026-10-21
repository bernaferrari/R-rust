#![forbid(unsafe_code)]
//! GNU simple scalar mappings, not full Unicode string expansions.
use super::{Ri18n_towlower, Ri18n_towupper};
#[test]
fn simple_case_upper_ascii_and_gnu_unicode_scalars() {
    for (from, to) in [
        ('a', 'A'),
        ('é', 'É'),
        ('ß', 'ß'),
        ('ᾀ', 'ᾈ'),
        ('𐐨', '𐐀'),
        ('ȿ', 'Ȿ'),
    ] {
        assert_eq!(Ri18n_towupper(from as i32), to as i32);
    }
}
#[test]
fn simple_case_lower_gnu_unicode_scalars() {
    for (from, to) in [('A', 'a'), ('İ', 'i'), ('Σ', 'σ'), ('ẞ', 'ß'), ('𐐀', '𐐨')] {
        assert_eq!(Ri18n_towlower(from as i32), to as i32);
    }
}
#[test]
fn simple_case_invalid_integer_and_nonletter_control() {
    for value in [-1, 0, 0xd800, 0x110000, i32::MAX, '1' as i32, '🙂' as i32] {
        assert_eq!(Ri18n_towupper(value), value);
        assert_eq!(Ri18n_towlower(value), value);
    }
}
