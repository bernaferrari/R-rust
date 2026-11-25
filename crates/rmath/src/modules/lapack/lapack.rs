//! Shared LAPACK helper types and validation routines.
//!
//! The runtime uses the pure-Rust faer backend exclusively. This module keeps
//! only the LAPACK-shaped validation and conversion helpers needed by that
//! implementation; it has no Fortran or system-BLAS FFI surface.

use crate::main::errors::Rf_error1;

/// Rcomplex: a pair of doubles for complex numbers (matches R's Rcomplex)
#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct Rcomplex {
    pub r: f64,
    pub i: f64,
}

// ============================================================
// Helper functions (module-private)
// ============================================================

/// Validate and normalize a LAPACK norm type string.
///
/// Converts single-character norm specification to uppercase LAPACK convention:
/// - '1' -> 'O' (one-norm alias)
/// - 'E' -> 'F' (Euclidean/Frobenius alias)
/// - 'M', 'O', 'I', 'F' pass through
///
/// Returns the normalized character or calls Rf_error on invalid input.
pub(crate) fn La_norm_type(typstr: &str) -> u8 {
    if typstr.len() != 1 {
        let msg = format!(
            "argument type[1]='{}' must be a character string of string length 1",
            typstr
        );
        let cmsg = std::ffi::CString::new(msg).unwrap_or_default();

        Rf_error1(c"invalid argument", &cmsg);
        unreachable!()
    }
    let typup = typstr.as_bytes()[0].to_ascii_uppercase();
    match typup {
        b'1' => b'O', // alias
        b'E' => b'F', // alias
        b'M' | b'O' | b'I' | b'F' => typup,
        _ => {
            let msg = format!(
                "argument type[1]='{}' must be one of 'M','1','O','I','F' or 'E'",
                typstr
            );
            let cmsg = std::ffi::CString::new(msg).unwrap_or_default();

            Rf_error1(c"invalid argument", &cmsg);
            unreachable!()
        }
    }
}

/// Validate and normalize a LAPACK condition number norm type string.
///
/// Currently only supports '1' (one-norm) or 'I' (infinity-norm):
/// - '1' -> 'O'
/// - 'O' or 'I' pass through
///
/// Returns 'O' or 'I', or calls Rf_error on invalid input.
pub(crate) fn La_rcond_type(typstr: &str) -> u8 {
    if typstr.len() != 1 {
        let msg = format!(
            "argument type[1]='{}' must be a character string of string length 1",
            typstr
        );
        let cmsg = std::ffi::CString::new(msg).unwrap_or_default();

        Rf_error1(c"invalid argument", &cmsg);
        unreachable!()
    }
    let typup = typstr.as_bytes()[0].to_ascii_uppercase();
    match typup {
        b'1' => b'O', // alias
        b'O' | b'I' => typup,
        _ => {
            let msg = format!(
                "argument type[1]='{}' must be one of '1','O', or 'I'",
                typstr
            );
            let cmsg = std::ffi::CString::new(msg).unwrap_or_default();

            Rf_error1(c"invalid argument", &cmsg);
            unreachable!()
        }
    }
}

/// Validate and normalize an uplo (upper/lower triangular) string.
///
/// Converts single-character uplo specification to uppercase:
/// - 'U' or 'L' pass through
///
/// Returns 'U' or 'L', or calls Rf_error on invalid input.
pub(crate) fn La_valid_uplo(uplostr: &str) -> u8 {
    if uplostr.len() != 1 {
        let msg = format!(
            "argument type[1]='{}' must be a character string of string length 1",
            uplostr
        );
        let cmsg = std::ffi::CString::new(msg).unwrap_or_default();

        Rf_error1(c"invalid argument", &cmsg);
        unreachable!()
    }
    let uplo = uplostr.as_bytes()[0].to_ascii_uppercase();
    match uplo {
        b'U' | b'L' => uplo,
        _ => {
            let msg = format!("argument type[1]='{}' must be 'U' or 'L'", uplostr);
            let cmsg = std::ffi::CString::new(msg).unwrap_or_default();

            Rf_error1(c"invalid argument", &cmsg);
            unreachable!()
        }
    }
}

/// Reconstruct DGEEV's packed real eigenvectors into admitted output storage.
/// No allocation or callback occurs; malformed dimensions or an orphaned
/// conjugate column reject before any output element is written.
pub(crate) fn unscramble_into(
    imaginary: &[f64],
    n: usize,
    vecs: &[f64],
    output: &mut [crate::sexp::ffi::Rcomplex],
) -> Option<()> {
    use crate::sexp::ffi::Rcomplex as ManagedComplex;
    let len = n.checked_mul(n)?;
    if imaginary.len() < n || vecs.len() < len || output.len() < len {
        return None;
    }
    let mut j = 0;
    while j < n {
        if imaginary[j] != 0.0 {
            if j + 1 == n {
                return None;
            }
            j += 2;
        } else {
            j += 1;
        }
    }
    let mut j = 0;
    while j < n {
        if imaginary[j] != 0.0 {
            let partner = j + 1;
            for i in 0..n {
                let real = vecs[i + j * n];
                let imaginary = vecs[i + partner * n];
                output[i + j * n] = ManagedComplex {
                    r: real,
                    i: imaginary,
                };
                output[i + partner * n] = ManagedComplex {
                    r: real,
                    i: -imaginary,
                };
            }
            j += 2;
        } else {
            for i in 0..n {
                output[i + j * n] = ManagedComplex {
                    r: vecs[i + j * n],
                    i: 0.0,
                };
            }
            j += 1;
        }
    }
    Some(())
}

// ============================================================
// Fortran character argument helpers
// ============================================================

/// Convert a Rust char to a null-terminated byte array suitable for passing
/// to Fortran LAPACK routines.
#[inline]
pub(crate) fn fort_char(c: u8) -> [u8; 2] {
    [c, 0]
}

/// Convert a Rust &str to a null-terminated byte array suitable for passing
/// to Fortran LAPACK routines.
#[inline]
pub(crate) fn fort_str(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.bytes().collect();
    v.push(0);
    v
}
