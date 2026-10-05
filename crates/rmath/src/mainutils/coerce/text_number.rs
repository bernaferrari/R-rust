#![forbid(unsafe_code)]

//! Scalar text coercion over bounded bytes, with no runtime pointers or callbacks.
use super::{WARN_INT_NA, WARN_NA};
use crate::mainutils::number_parse::parse_number;
use crate::sexp::ffi::{NA_INTEGER, NA_REAL, Rcomplex};

fn blank(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 11 | 12))
}

pub(super) fn real(bytes: &[u8]) -> (f64, i32) {
    if blank(bytes) {
        return (NA_REAL, 0);
    }
    let parsed = parse_number(bytes, b'.', false, 0);
    if blank(&bytes[parsed.consumed..]) {
        (parsed.value, 0)
    } else {
        (NA_REAL, WARN_NA)
    }
}

pub(super) fn integer(bytes: &[u8]) -> (i32, i32) {
    if blank(bytes) {
        return (NA_INTEGER, 0);
    }
    let parsed = parse_number(bytes, b'.', false, 0);
    if !blank(&bytes[parsed.consumed..]) {
        return (NA_INTEGER, WARN_NA);
    }
    if parsed.value >= f64::from(i32::MAX) + 1.0 || parsed.value <= f64::from(i32::MIN) {
        (NA_INTEGER, WARN_INT_NA)
    } else {
        // GNU's text branch admits NaN before the integer cast. Rust defines
        // its saturating cast (NaN -> 0), matching the pinned aarch64 oracle.
        // This is deliberate defined behavior, not C float-to-int undefined behavior.
        (parsed.value as i32, 0)
    }
}

pub(super) fn complex(bytes: &[u8]) -> (Rcomplex, i32) {
    let missing = Rcomplex {
        r: NA_REAL,
        i: NA_REAL,
    };
    if blank(bytes) {
        return (missing, 0);
    }
    let real = parse_number(bytes, b'.', false, 0);
    let rest = &bytes[real.consumed..];
    if blank(rest) {
        return (
            Rcomplex {
                r: real.value,
                i: 0.0,
            },
            0,
        );
    }
    if rest.first() == Some(&b'i') && blank(&rest[1..]) {
        return (
            Rcomplex {
                r: 0.0,
                i: real.value,
            },
            0,
        );
    }
    if matches!(rest.first(), Some(b'+') | Some(b'-')) {
        let imaginary = parse_number(rest, b'.', false, 0);
        let suffix = &rest[imaginary.consumed..];
        if suffix.first() == Some(&b'i') && blank(&suffix[1..]) {
            return (
                Rcomplex {
                    r: real.value,
                    i: imaginary.value,
                },
                0,
            );
        }
    }
    (missing, WARN_NA)
}
