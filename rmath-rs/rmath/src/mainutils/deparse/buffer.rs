#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;

// print2buff — append a string to the deparse buffer
// ---------------------------------------------------------------------------

/// Append a string to the deparse buffer, handling indentation at line start.
pub unsafe fn print2buff(strng: *const c_char, d: *mut LocalParseData) {
    unsafe {
        if strng.is_null() {
            return;
        }
        let d = &mut *d;

        if d.startline {
            d.startline = false;
            printtab2buff(d.indent, d);
        }
        let tlen = libc::strlen(strng);
        // Allocate buffer
        R_AllocStringBuffer(0, &mut d.buffer);
        let bufflen = libc::strlen(d.buffer.data);
        R_AllocStringBuffer(bufflen + tlen, &mut d.buffer);
        // Append string
        libc::strcat(d.buffer.data, strng);
        d.len += tlen as c_int;
    }
}

// ---------------------------------------------------------------------------
// printtab2buff — write indentation tabs to the buffer
// ---------------------------------------------------------------------------

/// Write indentation to the buffer. First 4 levels use 4 spaces each,
/// subsequent levels use 2 spaces each (emacs-style).
pub unsafe fn printtab2buff(ntab: c_int, d: *mut LocalParseData) {
    unsafe {
        for i in 1..=ntab {
            if i <= 4 {
                print2buff(b"    \0".as_ptr() as *const c_char, d);
            } else {
                print2buff(b"  \0".as_ptr() as *const c_char, d);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// writeline — flush current buffer line to the string vector
// ---------------------------------------------------------------------------

/// Flush the current buffer line to the output string vector.
///
/// If strvec is R_NilValue (counting pass), just increments linenumber.
/// Otherwise, stores the buffer content in strvec[linenumber].
pub unsafe fn writeline(d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        if !isNull(d.strvec) && d.linenumber < d.maxlines {
            let chars = Rf_mkChar(d.buffer.data);
            SET_STRING_ELT(d.strvec, d.linenumber as R_xlen_t, chars);
        }
        d.linenumber += 1;
        if d.linenumber >= d.maxlines {
            d.active = false;
        }
        // Reset
        d.len = 0;
        if !d.buffer.data.is_null() {
            *d.buffer.data = 0;
        }
        d.startline = true;
    }
}

// ---------------------------------------------------------------------------
// linebreak — break line if current line exceeds cutoff
// ---------------------------------------------------------------------------

/// Break the current line if it exceeds the cutoff width.
pub unsafe fn linebreak(lbreak: *mut bool, d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        if d.len > d.cutoff {
            if !*lbreak {
                *lbreak = true;
                d.indent += 1;
            }
            writeline(d);
        }
    }
}
