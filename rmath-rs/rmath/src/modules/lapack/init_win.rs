
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-2018 The R Core Team
 *
 *  Ported to Rust from R's src/modules/lapack/init_win.c
 *
 *  This appears not to currently be needed: in 2018-01 no Fortran I/O
 *  is done. But left as future-proofing and as an example (mentioned
 *  in 'Writing R Extensions').
 *
 *  On Windows, gfortran initialization sets stdout/stderr to _O_BINARY mode.
 *  This constructor resets them to _O_TEXT mode so that Fortran I/O works
 *  correctly with line ending translation.
 */

#[cfg(target_os = "windows")]
use libc::{_O_TEXT, _setmode, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO, c_int};

/// Windows-only constructor: resets stdout and stderr to text mode.
///
/// In C, this was implemented as `__attribute__((constructor))` which runs
/// automatically when the shared library is loaded. In Rust, we use the
/// same approach via a static initialization.
#[cfg(target_os = "windows")]
pub unsafe fn lapack_win_init() {
    // gfortran initialization sets these to _O_BINARY; reset to _O_TEXT
    unsafe {
        _setmode(STDOUT_FILENO, _O_TEXT);
        _setmode(STDERR_FILENO, _O_TEXT);
    }
}

/// Register the Windows init function as a constructor (runs at library load time).
///
/// This is equivalent to the C `__attribute__((constructor))` init() function.
/// The #[ctor] approach or #[link_section = ".CRT$XCA"] would be alternatives,
/// but the simplest portable approach is to have R_init_lapack call this.
#[cfg(target_os = "windows")]
pub(crate) fn init_win_stdio() {
    unsafe {
        _setmode(STDOUT_FILENO, _O_TEXT);
        _setmode(STDERR_FILENO, _O_TEXT);
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn init_win_stdio() {
    // No-op on non-Windows platforms
}
