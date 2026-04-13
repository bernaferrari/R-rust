#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::{c_int, c_void};

/// R_EOF sentinel — on non-Windows platforms R defines this as `-1`.
const R_EOF: c_int = -1;

/// Port of `R_fgetc` from `src/main/gram-ex.c`.
///
/// Reads a single character from the C FILE stream `fp`, normalising CRLF
/// line termination to LF. Returns `R_EOF` (-1) when the stream is at EOF.
///
/// Non-Windows path only: the original C code branches on `#ifdef Win32`
/// with a two-phase EOF protocol (return `'\n'` on first EOF hit, then
/// `R_EOF` on the next call). We skip that branch — this port targets
/// Android / Unix where `R_EOF` is `-1`.
pub unsafe fn R_fgetc(fp: *mut c_void) -> c_int {
    // SAFETY: caller guarantees `fp` is a valid, non-null FILE pointer.
    let c = unsafe { libc::fgetc(fp as *mut libc::FILE) };
    if c == '\r' as c_int {
        // SAFETY: same fp guarantee.
        let next = unsafe { libc::fgetc(fp as *mut libc::FILE) };
        if next != '\n' as c_int {
            unsafe { libc::ungetc(next, fp as *mut libc::FILE) };
            return '\r' as c_int;
        }
        return '\n' as c_int;
    }
    if unsafe { libc::feof(fp as *mut libc::FILE) } != 0 {
        R_EOF
    } else {
        c
    }
}
