#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/printvector.c -- vector printing.
//!
//! Provides printVector() and printNamedVector() for displaying R vectors,
//! along with type-specific vector printers (printIntegerVector, etc.).
//!
//! Functions that are already defined elsewhere (format.rs, printutils.rs,
//! print.rs, accessors.rs) are NOT redefined here to avoid duplicate
//! `#[unsafe(no_mangle)]` symbols.  Those include:
//!   formatInteger, formatReal, formatLogical, formatString, formatRaw,
//!   formatComplex (and their ALTREP *S variants) -- in format.rs
//!   EncodeLogical, EncodeInteger, EncodeReal0, EncodeComplex, EncodeRaw,
//!   EncodeString, Rstrwid, Rstrlen, IndexWidth, VectorIndex, Rprintf,
//!   get_R_print -- in printutils.rs
//!   PrintValue, PrintValueEnv, PrintValueRec -- in print.rs
//!   XLENGTH -- in sexp/accessors.rs

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::sexp::accessors::{
    COMPLEX, COMPLEX_ELT, INTEGER, INTEGER_ELT, LOGICAL, LOGICAL_ELT, RAW, RAW_ELT, REAL, REAL_ELT,
    STRING_ELT, TYPEOF, XLENGTH,
};
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_REAL, R_IsNA, R_xlen_t, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// R_PrintData struct -- full R_print parameters for printvector.c
//
// Note: printutils.rs defines a smaller RPrint struct and get_R_print().
// This module defines the full R_PrintData with all fields used by
// printvector.c.  We use a distinct name (R_PrintData / get_R_PrintData)
// to avoid symbol collisions.
// ---------------------------------------------------------------------------

/// Global print parameters matching R's `R_print` struct from Print.h.
///
/// This is the full set of fields used by printvector.c and related
/// printing code.  The smaller `RPrint` struct in printutils.rs covers
/// only the subset needed by encode functions.
pub struct R_PrintData {
    pub width: c_int,
    pub gap: c_int,
    pub digits: c_int,
    pub scipen: c_int,
    pub max: c_int,
    pub right: c_int,
    pub quote: c_int,
    pub na_width: c_int,
    pub na_width_noquote: c_int,
    pub na_string: SEXP,
    pub na_string_noquote: SEXP,
}

impl Default for R_PrintData {
    fn default() -> Self {
        R_PrintData {
            width: 80,
            gap: 1,
            digits: 4,
            scipen: 0,
            max: 99999,
            right: 0,
            quote: 1,
            na_width: 2,
            na_width_noquote: 4,
            na_string: ptr::null_mut(),
            na_string_noquote: ptr::null_mut(),
        }
    }
}

/// Module-level print parameter storage.
static mut R_PRINT_DATA: R_PrintData = R_PrintData {
    width: 80,
    gap: 1,
    digits: 4,
    scipen: 0,
    max: 99999,
    right: 0,
    quote: 1,
    na_width: 2,
    na_width_noquote: 4,
    na_string: ptr::null_mut(),
    na_string_noquote: ptr::null_mut(),
};

/// Get the global print data (full R_print parameters).
///
/// Named `get_R_PrintData` to avoid collision with `get_R_print` in
/// printutils.rs (which returns the smaller `RPrint` struct).
pub unsafe fn get_R_PrintData() -> &'static mut R_PrintData {
    unsafe { &mut *std::ptr::addr_of_mut!(R_PRINT_DATA) }
}

// ---------------------------------------------------------------------------
// Constants used by printvector.c
// ---------------------------------------------------------------------------

/// R's OutDec character (decimal separator, typically '.').
pub const OutDec: c_char = b'.' as c_char;

/// Minimum label offset for index printing.
pub const R_MIN_LBLOFF: c_int = 2;

/// Right adjustment constant.
pub const Rprt_adj_right: c_int = 1;

/// Left adjustment constant.
pub const Rprt_adj_left: c_int = 0;

// ---------------------------------------------------------------------------
// Helper: Rprintf via eprint!
//
// In the C source, Rprintf writes to the console. We use eprint! (stderr)
// as a stand-in.
// ---------------------------------------------------------------------------

/// Print formatted output to stderr (Rprintf equivalent).
macro_rules! Rprintf {
    ($($arg:tt)*) => {
        eprint!($($arg)*)
    };
}

// ---------------------------------------------------------------------------
// Helper: IndexWidth for R_xlen_t (printvector.c uses this on vector length)
//
// The IndexWidth in format.rs takes c_int. We need an R_xlen_t version.
// ---------------------------------------------------------------------------

/// Compute the number of decimal digits needed to display `x`.
fn index_width_xlen(x: R_xlen_t) -> c_int {
    if x <= 0 {
        return 1;
    }
    let mut n = x;
    let mut d = 0;
    loop {
        d += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    d
}

/// Print a vector index label at position `i` with field width `w`.
fn vector_index(i: R_xlen_t, w: c_int) {
    Rprintf!("[{:>width$}] ", i, width = (w - 3).max(0) as usize);
}

// ---------------------------------------------------------------------------
// Internal helpers: formatStringS, formatLogicalS, formatIntegerS, etc.
//
// These are wrappers around the format.rs functions which take SEXP and are
// already #[unsafe(no_mangle)] pub unsafe extern "C". We call them from Rust code.
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn formatString(x: *const SEXP, n: R_xlen_t, fieldwidth: *mut c_int, quote: c_int);
    fn formatStringS(x: SEXP, n: R_xlen_t, fieldwidth: *mut c_int, quote: c_int);
    fn formatLogical(x: *const c_int, n: R_xlen_t, fieldwidth: *mut c_int);
    fn formatLogicalS(x: SEXP, n: R_xlen_t, fieldwidth: *mut c_int);
    fn formatInteger(x: *const c_int, n: R_xlen_t, fieldwidth: *mut c_int);
    fn formatIntegerS(x: SEXP, n: R_xlen_t, fieldwidth: *mut c_int);
    fn formatReal(
        x: *const f64,
        n: R_xlen_t,
        w: *mut c_int,
        d: *mut c_int,
        e: *mut c_int,
        nsmall: c_int,
    );
    fn formatRealS(
        x: SEXP,
        n: R_xlen_t,
        w: *mut c_int,
        d: *mut c_int,
        e: *mut c_int,
        nsmall: c_int,
    );
    fn formatComplex(
        x: *const Rcomplex,
        n: R_xlen_t,
        wr: *mut c_int,
        dr: *mut c_int,
        er: *mut c_int,
        wi: *mut c_int,
        di: *mut c_int,
        ei: *mut c_int,
        nsmall: c_int,
    );
    fn formatComplexS(
        x: SEXP,
        n: R_xlen_t,
        wr: *mut c_int,
        dr: *mut c_int,
        er: *mut c_int,
        wi: *mut c_int,
        di: *mut c_int,
        ei: *mut c_int,
        nsmall: c_int,
    );
    fn formatRaw(x: *const std::os::raw::c_void, n: R_xlen_t, fieldwidth: *mut c_int);
    fn formatRawS(x: SEXP, n: R_xlen_t, fieldwidth: *mut c_int);
    fn EncodeLogical(x: c_int, w: c_int) -> *const c_char;
    fn EncodeInteger(x: c_int, w: c_int) -> *const c_char;
    fn EncodeReal0(x: f64, w: c_int, d: c_int, e: c_int, dec: *const c_char) -> *const c_char;
    fn EncodeComplex(
        x: Rcomplex,
        wr: c_int,
        dr: c_int,
        er: c_int,
        wi: c_int,
        di: c_int,
        ei: c_int,
        dec: *const c_char,
    ) -> *const c_char;
    fn EncodeRaw(x: u8, prefix: *const c_char) -> *const c_char;
    fn EncodeString(s: SEXP, w: c_int, quote: c_int, justify: c_int) -> *const c_char;
}

// ---------------------------------------------------------------------------
// printLogicalVectorS -- internal, not exported (static in C)
// ---------------------------------------------------------------------------

unsafe fn printLogicalVectorS(x: SEXP, n: R_xlen_t, indx: c_int) {
    unsafe {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut labwidth: c_int = 0;
        let mut width: c_int = 0;

        // DO_first_lab
        if indx != 0 {
            labwidth = index_width_xlen(n) + 2;
            vector_index(1, labwidth);
            width = labwidth;
        } else {
            width = 0;
        }

        formatLogicalS(x, n, &mut w);
        w += rp.gap;

        let px = LOGICAL(x);
        for i in 0..n {
            let xi = if px.is_null() {
                NA_INTEGER
            } else {
                *px.add(i as usize)
            };

            // NUMVECTOR_TIGHTLOOP
            if i > 0 && width + w > rp.width {
                // DO_newline
                Rprintf!("\n");
                if indx != 0 {
                    vector_index(i + 1, labwidth);
                    width = labwidth;
                } else {
                    width = 0;
                }
            }
            let enc = EncodeLogical(xi, w);
            let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
            Rprintf!("{}", s);
            width += w;
        }
        Rprintf!("\n");
    }
}

// ---------------------------------------------------------------------------
// printIntegerVector -- exported
// ---------------------------------------------------------------------------

pub unsafe fn printIntegerVector(x: *const c_int, n: R_xlen_t, indx: c_int) {
    unsafe {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut labwidth: c_int = 0;
        let mut width: c_int = 0;

        // DO_first_lab
        if indx != 0 {
            labwidth = index_width_xlen(n) + 2;
            vector_index(1, labwidth);
            width = labwidth;
        } else {
            width = 0;
        }

        formatInteger(x, n, &mut w);
        w += rp.gap;

        for i in 0..n {
            let xi = if x.is_null() {
                NA_INTEGER
            } else {
                *x.add(i as usize)
            };

            // NUMVECTOR_TIGHTLOOP
            if i > 0 && width + w > rp.width {
                Rprintf!("\n");
                if indx != 0 {
                    vector_index(i + 1, labwidth);
                    width = labwidth;
                } else {
                    width = 0;
                }
            }
            let enc = EncodeInteger(xi, w);
            let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
            Rprintf!("{}", s);
            width += w;
        }
        Rprintf!("\n");
    }
}

// ---------------------------------------------------------------------------
// printRealVector -- exported
// ---------------------------------------------------------------------------

pub unsafe fn printRealVector(x: *const f64, n: R_xlen_t, indx: c_int) {
    unsafe {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut d: c_int = 0;
        let mut e: c_int = 0;
        let mut labwidth: c_int = 0;
        let mut width: c_int = 0;

        // DO_first_lab
        if indx != 0 {
            labwidth = index_width_xlen(n) + 2;
            vector_index(1, labwidth);
            width = labwidth;
        } else {
            width = 0;
        }

        formatReal(x, n, &mut w, &mut d, &mut e, 0);
        w += rp.gap;

        let outdec = b".\0".as_ptr() as *const c_char;

        for i in 0..n {
            let xi = if x.is_null() {
                NA_REAL
            } else {
                *x.add(i as usize)
            };

            // NUMVECTOR_TIGHTLOOP
            if i > 0 && width + w > rp.width {
                Rprintf!("\n");
                if indx != 0 {
                    vector_index(i + 1, labwidth);
                    width = labwidth;
                } else {
                    width = 0;
                }
            }
            let enc = EncodeReal0(xi, w, d, e, outdec);
            let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
            Rprintf!("{}", s);
            width += w;
        }
        Rprintf!("\n");
    }
}

// ---------------------------------------------------------------------------
// printComplexVector -- exported
// ---------------------------------------------------------------------------

pub unsafe fn printComplexVector(x: *const Rcomplex, n: R_xlen_t, indx: c_int) {
    unsafe {
        let rp = get_R_PrintData();
        let mut wr: c_int = 0;
        let mut dr: c_int = 0;
        let mut er: c_int = 0;
        let mut wi: c_int = 0;
        let mut di: c_int = 0;
        let mut ei: c_int = 0;
        let mut labwidth: c_int = 0;
        let mut width: c_int = 0;

        // DO_first_lab
        if indx != 0 {
            labwidth = index_width_xlen(n) + 2;
            vector_index(1, labwidth);
            width = labwidth;
        } else {
            width = 0;
        }

        formatComplex(
            x, n, &mut wr, &mut dr, &mut er, &mut wi, &mut di, &mut ei, 0,
        );

        let w = wr + wi + 2; // +2 for "+" and "i"
        let w = w + rp.gap;

        let outdec = b".\0".as_ptr() as *const c_char;

        for i in 0..n {
            let cx = if x.is_null() {
                Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                }
            } else {
                *x.add(i as usize)
            };

            // NUMVECTOR_TIGHTLOOP (with NA complex check)
            if i > 0 && width + w > rp.width {
                Rprintf!("\n");
                if indx != 0 {
                    vector_index(i + 1, labwidth);
                    width = labwidth;
                } else {
                    width = 0;
                }
            }
            let enc = if R_IsNA(cx.r) || R_IsNA(cx.i) {
                EncodeReal0(NA_REAL, w, 0, 0, outdec)
            } else {
                EncodeComplex(cx, wr + rp.gap, dr, er, wi, di, ei, outdec)
            };
            let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
            Rprintf!("{}", s);
            width += w;
        }
        Rprintf!("\n");
    }
}

// ---------------------------------------------------------------------------
// printRawVector -- exported
// ---------------------------------------------------------------------------

pub unsafe fn printRawVector(x: *const u8, n: R_xlen_t, indx: c_int) {
    unsafe {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut labwidth: c_int = 0;
        let mut width: c_int = 0;

        // DO_first_lab
        if indx != 0 {
            labwidth = index_width_xlen(n) + 2;
            vector_index(1, labwidth);
            width = labwidth;
        } else {
            width = 0;
        }

        formatRaw(x as *const std::os::raw::c_void, n, &mut w);
        w += rp.gap;

        let empty_prefix: [c_char; 1] = [0];

        for i in 0..n {
            let xi = if x.is_null() { 0u8 } else { *x.add(i as usize) };

            // RAWVECTOR_TIGHTLOOP
            if i > 0 && width + w > rp.width {
                Rprintf!("\n");
                if indx != 0 {
                    vector_index(i + 1, labwidth);
                    width = labwidth;
                } else {
                    width = 0;
                }
            }
            let enc = EncodeRaw(xi, empty_prefix.as_ptr());
            let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
            Rprintf!("{:width$}{}", "", s, width = rp.gap as usize);
            width += w;
        }
        Rprintf!("\n");
    }
}

// ---------------------------------------------------------------------------
// printStringVector -- internal (static in C)
// ---------------------------------------------------------------------------

unsafe fn printStringVector(x: *const SEXP, n: R_xlen_t, quote: c_int, indx: c_int) {
    unsafe {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut labwidth: c_int = 0;
        let mut width: c_int = 0;

        // DO_first_lab
        if indx != 0 {
            labwidth = index_width_xlen(n) + 2;
            vector_index(1, labwidth);
            width = labwidth;
        } else {
            width = 0;
        }

        formatString(x, n, &mut w, quote);

        for i in 0..n {
            let si = if x.is_null() {
                ptr::null_mut()
            } else {
                *x.add(i as usize)
            };

            // CHARVECTOR_TIGHTLOOP
            if i > 0 && width + w + rp.gap > rp.width {
                Rprintf!("\n");
                if indx != 0 {
                    vector_index(i + 1, labwidth);
                    width = labwidth;
                } else {
                    width = 0;
                }
            }
            let enc = EncodeString(si, w, quote, rp.right);
            let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
            Rprintf!("{:width$}{}", "", s, width = rp.gap as usize);
            width += w + rp.gap;
        }
        Rprintf!("\n");
    }
}

// ---------------------------------------------------------------------------
// printStringVectorS -- internal (static in C)
// ---------------------------------------------------------------------------

unsafe fn printStringVectorS(x: SEXP, n: R_xlen_t, quote: c_int, indx: c_int) {
    unsafe {
        // Always use STRING_ELT path (DATAPTR_OR_NULL not available).
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut labwidth: c_int = 0;
        let mut width: c_int = 0;

        // DO_first_lab
        if indx != 0 {
            labwidth = index_width_xlen(n) + 2;
            vector_index(1, labwidth);
            width = labwidth;
        } else {
            width = 0;
        }

        formatStringS(x, n, &mut w, quote);

        for i in 0..n {
            let si = STRING_ELT(x, i);

            // CHARVECTOR_TIGHTLOOP
            if i > 0 && width + w + rp.gap > rp.width {
                Rprintf!("\n");
                if indx != 0 {
                    vector_index(i + 1, labwidth);
                    width = labwidth;
                } else {
                    width = 0;
                }
            }
            let enc = EncodeString(si, w, quote, rp.right);
            let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
            Rprintf!("{:width$}{}", "", s, width = rp.gap as usize);
            width += w + rp.gap;
        }
        Rprintf!("\n");
    }
}

// ---------------------------------------------------------------------------
// printRawVectorS -- internal (static in C)
// ---------------------------------------------------------------------------

unsafe fn printRawVectorS(x: SEXP, n: R_xlen_t, indx: c_int) {
    unsafe {
        let px = RAW(x);
        printRawVector(px, n, indx);
    }
}

// ---------------------------------------------------------------------------
// printRealVectorS -- internal (static in C)
// ---------------------------------------------------------------------------

unsafe fn printRealVectorS(x: SEXP, n: R_xlen_t, indx: c_int) {
    unsafe {
        let px = REAL(x);
        printRealVector(px, n, indx);
    }
}

// ---------------------------------------------------------------------------
// printIntegerVectorS -- internal (static in C)
// ---------------------------------------------------------------------------

unsafe fn printIntegerVectorS(x: SEXP, n: R_xlen_t, indx: c_int) {
    unsafe {
        let px = INTEGER(x);
        printIntegerVector(px, n, indx);
    }
}

// ---------------------------------------------------------------------------
// printComplexVectorS -- internal (static in C)
// ---------------------------------------------------------------------------

unsafe fn printComplexVectorS(x: SEXP, n: R_xlen_t, indx: c_int) {
    unsafe {
        let px = COMPLEX(x);
        printComplexVector(px, n, indx);
    }
}

// ---------------------------------------------------------------------------
// printVector -- exported
// ---------------------------------------------------------------------------

pub unsafe fn printVector(x: SEXP, indx: c_int, quote: c_int) {
    unsafe {
        if x.is_null() {
            return;
        }
        let rp = get_R_PrintData();
        let n = XLENGTH(x);

        if n != 0 {
            let n_pr = if n <= rp.max as R_xlen_t + 1 {
                n
            } else {
                rp.max as R_xlen_t
            };

            let t = TYPEOF(x);
            if t == SEXPTYPE::LGLSXP.0 {
                printLogicalVectorS(x, n_pr, indx);
            } else if t == SEXPTYPE::INTSXP.0 {
                printIntegerVectorS(x, n_pr, indx);
            } else if t == SEXPTYPE::REALSXP.0 {
                printRealVectorS(x, n_pr, indx);
            } else if t == SEXPTYPE::STRSXP.0 {
                if quote != 0 {
                    printStringVectorS(x, n_pr, '"' as c_int, indx);
                } else {
                    printStringVectorS(x, n_pr, 0, indx);
                }
            } else if t == SEXPTYPE::CPLXSXP.0 {
                printComplexVectorS(x, n_pr, indx);
            } else if t == SEXPTYPE::RAWSXP.0 {
                printRawVectorS(x, n_pr, indx);
            }

            if n_pr < n {
                let omitted = n - n_pr;
                Rprintf!(
                    " [ reached 'max' / getOption(\"max.print\") -- omitted {} entries ]\n",
                    omitted
                );
            }
        } else {
            // PRINT_V_0
            let t = TYPEOF(x);
            match t {
                10 => Rprintf!("logical(0)\n"),   // LGLSXP
                13 => Rprintf!("integer(0)\n"),   // INTSXP
                14 => Rprintf!("numeric(0)\n"),   // REALSXP
                15 => Rprintf!("complex(0)\n"),   // CPLXSXP
                16 => Rprintf!("character(0)\n"), // STRSXP
                24 => Rprintf!("raw(0)\n"),       // RAWSXP
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Named vector printers (internal, static in C)
// ---------------------------------------------------------------------------

/// Helper: print the name row for named vectors.
unsafe fn print_name_row(names: SEXP, n: R_xlen_t, w: c_int, nperline: c_int, start: R_xlen_t) {
    unsafe {
        let rp = get_R_PrintData();
        let mut j: R_xlen_t = 0;
        while j < nperline as R_xlen_t {
            let k = start + j;
            if k >= n {
                break;
            }
            let enc = EncodeString(STRING_ELT(names, k), w, 0, Rprt_adj_right);
            let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
            Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
            j += 1;
        }
        Rprintf!("\n");
    }
}

unsafe fn printNamedLogicalVectorS(x: SEXP, n: R_xlen_t, names: SEXP) {
    unsafe {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut wn: c_int = 0;

        formatLogicalS(x, n, &mut w);
        formatStringS(names, n, &mut wn, 0);
        if w < wn {
            w = wn;
        }

        let mut nperline = rp.width / (w + rp.gap);
        if nperline <= 0 {
            nperline = 1;
        }

        let mut nlines = n / nperline as R_xlen_t;
        if n % nperline as R_xlen_t != 0 {
            nlines += 1;
        }

        let mut i: R_xlen_t = 0;
        while i < nlines {
            if i > 0 {
                Rprintf!("\n");
            }
            // Name row
            print_name_row(names, n, w, nperline, i * nperline as R_xlen_t);
            // Value row
            let mut j: R_xlen_t = 0;
            while j < nperline as R_xlen_t {
                let k = i * nperline as R_xlen_t + j;
                if k >= n {
                    break;
                }
                let enc = EncodeLogical(LOGICAL_ELT(x, k as c_int), w);
                let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
                Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
                j += 1;
            }
            i += 1;
        }
        Rprintf!("\n");
    }
}

unsafe fn printNamedIntegerVectorS(x: SEXP, n: R_xlen_t, names: SEXP) {
    unsafe {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut wn: c_int = 0;

        formatIntegerS(x, n, &mut w);
        formatStringS(names, n, &mut wn, 0);
        if w < wn {
            w = wn;
        }

        let mut nperline = rp.width / (w + rp.gap);
        if nperline <= 0 {
            nperline = 1;
        }

        let mut nlines = n / nperline as R_xlen_t;
        if n % nperline as R_xlen_t != 0 {
            nlines += 1;
        }

        let mut i: R_xlen_t = 0;
        while i < nlines {
            if i > 0 {
                Rprintf!("\n");
            }
            print_name_row(names, n, w, nperline, i * nperline as R_xlen_t);
            let mut j: R_xlen_t = 0;
            while j < nperline as R_xlen_t {
                let k = i * nperline as R_xlen_t + j;
                if k >= n {
                    break;
                }
                let enc = EncodeInteger(INTEGER_ELT(x, k as c_int), w);
                let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
                Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
                j += 1;
            }
            i += 1;
        }
        Rprintf!("\n");
    }
}

unsafe fn printNamedRealVectorS(x: SEXP, n: R_xlen_t, names: SEXP) {
    unsafe {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut d: c_int = 0;
        let mut e: c_int = 0;
        let mut wn: c_int = 0;

        formatRealS(x, n, &mut w, &mut d, &mut e, 0);
        formatStringS(names, n, &mut wn, 0);
        if w < wn {
            w = wn;
        }

        let mut nperline = rp.width / (w + rp.gap);
        if nperline <= 0 {
            nperline = 1;
        }

        let mut nlines = n / nperline as R_xlen_t;
        if n % nperline as R_xlen_t != 0 {
            nlines += 1;
        }

        let outdec = b".\0".as_ptr() as *const c_char;

        let mut i: R_xlen_t = 0;
        while i < nlines {
            if i > 0 {
                Rprintf!("\n");
            }
            print_name_row(names, n, w, nperline, i * nperline as R_xlen_t);
            let mut j: R_xlen_t = 0;
            while j < nperline as R_xlen_t {
                let k = i * nperline as R_xlen_t + j;
                if k >= n {
                    break;
                }
                let enc = EncodeReal0(REAL_ELT(x, k as c_int), w, d, e, outdec);
                let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
                Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
                j += 1;
            }
            i += 1;
        }
        Rprintf!("\n");
    }
}

unsafe fn printNamedComplexVectorS(x: SEXP, n: R_xlen_t, names: SEXP) {
    unsafe {
        let mut w: c_int = 0;
        let mut wr: c_int = 0;
        let mut dr: c_int = 0;
        let mut er: c_int = 0;
        let mut wi: c_int = 0;
        let mut di: c_int = 0;
        let mut ei: c_int = 0;

        let rp = get_R_PrintData();

        formatComplexS(
            x, n, &mut wr, &mut dr, &mut er, &mut wi, &mut di, &mut ei, 0,
        );
        w = wr + wi + 2;

        let mut wn: c_int = 0;
        formatStringS(names, n, &mut wn, 0);
        if w < wn {
            w = wn;
        }

        let mut nperline = rp.width / (w + rp.gap);
        if nperline <= 0 {
            nperline = 1;
        }

        let mut nlines = n / nperline as R_xlen_t;
        if n % nperline as R_xlen_t != 0 {
            nlines += 1;
        }

        let outdec = b".\0".as_ptr() as *const c_char;

        let mut i: R_xlen_t = 0;
        while i < nlines {
            if i > 0 {
                Rprintf!("\n");
            }
            // Name row
            let mut j: R_xlen_t = 0;
            while j < nperline as R_xlen_t {
                let k = i * nperline as R_xlen_t + j;
                if k >= n {
                    break;
                }
                let enc = EncodeString(STRING_ELT(names, k as i64), w, 0, Rprt_adj_right);
                let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
                Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
                j += 1;
            }
            Rprintf!("\n");
            // Value row
            let mut j: R_xlen_t = 0;
            while j < nperline as R_xlen_t {
                let k = i * nperline as R_xlen_t + j;
                if k >= n {
                    break;
                }
                let tmp = COMPLEX_ELT(x, k as c_int);
                if j > 0 {
                    Rprintf!("{:width$}", "", width = rp.gap as usize);
                }
                if R_IsNA(tmp.r) || R_IsNA(tmp.i) {
                    let enc = EncodeReal0(NA_REAL, w, 0, 0, outdec);
                    let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
                    Rprintf!("{}", s);
                } else {
                    let enc_re = EncodeReal0(tmp.r, wr, dr, er, outdec);
                    let s_re = std::ffi::CStr::from_ptr(enc_re).to_str().unwrap_or("");
                    if ISNAN(tmp.i) {
                        Rprintf!("{}+NaNi", s_re);
                    } else if tmp.i >= 0.0 {
                        let enc_im = EncodeReal0(tmp.i, wi, di, ei, outdec);
                        let s_im = std::ffi::CStr::from_ptr(enc_im).to_str().unwrap_or("");
                        Rprintf!("{}+{}i", s_re, s_im);
                    } else {
                        let enc_im = EncodeReal0(-tmp.i, wi, di, ei, outdec);
                        let s_im = std::ffi::CStr::from_ptr(enc_im).to_str().unwrap_or("");
                        Rprintf!("{}-{}i", s_re, s_im);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Rprintf!("\n");
    }
}

unsafe fn printNamedStringVectorS(x: SEXP, n: R_xlen_t, quote: c_int, names: SEXP) {
    unsafe {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut wn: c_int = 0;

        formatStringS(x, n, &mut w, quote);
        formatStringS(names, n, &mut wn, 0);
        if w < wn {
            w = wn;
        }

        let mut nperline = rp.width / (w + rp.gap);
        if nperline <= 0 {
            nperline = 1;
        }

        let mut nlines = n / nperline as R_xlen_t;
        if n % nperline as R_xlen_t != 0 {
            nlines += 1;
        }

        let mut i: R_xlen_t = 0;
        while i < nlines {
            if i > 0 {
                Rprintf!("\n");
            }
            print_name_row(names, n, w, nperline, i * nperline as R_xlen_t);
            let mut j: R_xlen_t = 0;
            while j < nperline as R_xlen_t {
                let k = i * nperline as R_xlen_t + j;
                if k >= n {
                    break;
                }
                let enc = EncodeString(STRING_ELT(x, k), w, quote, Rprt_adj_right);
                let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
                Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
                j += 1;
            }
            i += 1;
        }
        Rprintf!("\n");
    }
}

unsafe fn printNamedRawVectorS(x: SEXP, n: R_xlen_t, names: SEXP) {
    unsafe {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut wn: c_int = 0;

        formatRawS(x, n, &mut w);
        formatStringS(names, n, &mut wn, 0);
        if w < wn {
            w = wn;
        }

        let mut nperline = rp.width / (w + rp.gap);
        if nperline <= 0 {
            nperline = 1;
        }

        let mut nlines = n / nperline as R_xlen_t;
        if n % nperline as R_xlen_t != 0 {
            nlines += 1;
        }

        let empty_prefix: [c_char; 1] = [0];

        let mut i: R_xlen_t = 0;
        while i < nlines {
            if i > 0 {
                Rprintf!("\n");
            }
            // Name row
            let mut j: R_xlen_t = 0;
            while j < nperline as R_xlen_t {
                let k = i * nperline as R_xlen_t + j;
                if k >= n {
                    break;
                }
                let enc = EncodeString(STRING_ELT(names, k as i64), w, 0, Rprt_adj_right);
                let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
                Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
                j += 1;
            }
            Rprintf!("\n");
            // Value row
            let mut j: R_xlen_t = 0;
            while j < nperline as R_xlen_t {
                let k = i * nperline as R_xlen_t + j;
                if k >= n {
                    break;
                }
                let val = RAW_ELT(x, k as c_int);
                let enc = EncodeRaw(val, empty_prefix.as_ptr());
                let s = std::ffi::CStr::from_ptr(enc).to_str().unwrap_or("");
                Rprintf!(
                    "{:pad$}{}{:gap$}",
                    "",
                    s,
                    "",
                    pad = (w - 2).max(0) as usize,
                    gap = rp.gap as usize
                );
                j += 1;
            }
            i += 1;
        }
        Rprintf!("\n");
    }
}

// ---------------------------------------------------------------------------
// printNamedVector -- exported
// ---------------------------------------------------------------------------

pub unsafe fn printNamedVector(
    x: SEXP,
    names: SEXP,
    quote: c_int,
    title: *const c_char,
) {
    unsafe {
        if !title.is_null() {
            let s = std::ffi::CStr::from_ptr(title).to_str().unwrap_or("");
            Rprintf!("{}\n", s);
        }

        if x.is_null() {
            return;
        }

        let rp = get_R_PrintData();
        let n = XLENGTH(x);

        if n != 0 {
            let n_pr = if n <= rp.max as R_xlen_t + 1 {
                n
            } else {
                rp.max as R_xlen_t
            };

            let t = TYPEOF(x);
            if t == SEXPTYPE::LGLSXP.0 {
                printNamedLogicalVectorS(x, n_pr, names);
            } else if t == SEXPTYPE::INTSXP.0 {
                printNamedIntegerVectorS(x, n_pr, names);
            } else if t == SEXPTYPE::REALSXP.0 {
                printNamedRealVectorS(x, n_pr, names);
            } else if t == SEXPTYPE::CPLXSXP.0 {
                printNamedComplexVectorS(x, n_pr, names);
            } else if t == SEXPTYPE::STRSXP.0 {
                let q = if quote != 0 { '"' as c_int } else { 0 };
                printNamedStringVectorS(x, n_pr, q, names);
            } else if t == SEXPTYPE::RAWSXP.0 {
                printNamedRawVectorS(x, n_pr, names);
            }

            if n_pr < n {
                let omitted = n - n_pr;
                Rprintf!(
                    " [ reached 'max' / getOption(\"max.print\") -- omitted {} entries ]\n",
                    omitted
                );
            }
        } else {
            Rprintf!("named ");
            let t = TYPEOF(x);
            match t {
                10 => Rprintf!("logical(0)\n"),   // LGLSXP
                13 => Rprintf!("integer(0)\n"),   // INTSXP
                14 => Rprintf!("numeric(0)\n"),   // REALSXP
                15 => Rprintf!("complex(0)\n"),   // CPLXSXP
                16 => Rprintf!("character(0)\n"), // STRSXP
                24 => Rprintf!("raw(0)\n"),       // RAWSXP
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PrintWarnings -- delegates to errors.rs real implementation.
// ---------------------------------------------------------------------------

pub unsafe fn PrintWarnings() {
    crate::main::errors::PrintWarnings();
}

// ---------------------------------------------------------------------------
// type2str_nowarn
// ---------------------------------------------------------------------------

pub unsafe fn type2str_nowarn(stype: c_int) -> *const c_char {
    match stype {
        0 => b"NULL\0".as_ptr() as *const c_char,
        1 => b"symbol\0".as_ptr() as *const c_char,
        2 => b"pairlist\0".as_ptr() as *const c_char,
        3 => b"closure\0".as_ptr() as *const c_char,
        4 => b"environment\0".as_ptr() as *const c_char,
        5 => b"promise\0".as_ptr() as *const c_char,
        6 => b"language\0".as_ptr() as *const c_char,
        7 => b"special\0".as_ptr() as *const c_char,
        8 => b"builtin\0".as_ptr() as *const c_char,
        9 => b"char\0".as_ptr() as *const c_char,
        10 => b"logical\0".as_ptr() as *const c_char,
        13 => b"integer\0".as_ptr() as *const c_char,
        14 => b"double\0".as_ptr() as *const c_char,
        15 => b"complex\0".as_ptr() as *const c_char,
        16 => b"character\0".as_ptr() as *const c_char,
        17 => b"...\0".as_ptr() as *const c_char,
        18 => b"any\0".as_ptr() as *const c_char,
        19 => b"list\0".as_ptr() as *const c_char,
        20 => b"expression\0".as_ptr() as *const c_char,
        21 => b"externalptr\0".as_ptr() as *const c_char,
        22 => b"bytecode\0".as_ptr() as *const c_char,
        23 => b"weakref\0".as_ptr() as *const c_char,
        24 => b"raw\0".as_ptr() as *const c_char,
        25 => b"S4\0".as_ptr() as *const c_char,
        _ => b"unknown\0".as_ptr() as *const c_char,
    }
}

// ---------------------------------------------------------------------------
// GetMatrixDimnames
// ---------------------------------------------------------------------------

pub unsafe fn GetMatrixDimnames(
    x: SEXP,
    rl: *mut SEXP,
    cl: *mut SEXP,
    rn: *mut *const c_char,
    cn: *mut *const c_char,
) {
    unsafe {
        // Extract dimnames[[1]], dimnames[[2]], names(dimnames)[1], names(dimnames)[2]
        // This requires getAttrib which is in attrib_core.rs
        unsafe extern "C" {
            fn getAttrib(x: SEXP, what: SEXP) -> SEXP;
            fn Rf_install(name: *const c_char) -> SEXP;
            fn VECTOR_ELT(x: SEXP, i: R_xlen_t) -> SEXP;
        }

        // Initialize outputs to NilValue/null
        if !rl.is_null() {
            *rl = R_NilValue();
        }
        if !cl.is_null() {
            *cl = R_NilValue();
        }
        if !rn.is_null() {
            *rn = ptr::null();
        }
        if !cn.is_null() {
            *cn = ptr::null();
        }

        if x.is_null() {
            return;
        }

        let dimnames_sym = Rf_install(b"dimnames\0".as_ptr() as *const c_char);
        let dimnames = getAttrib(x, dimnames_sym);

        if dimnames.is_null() || dimnames == R_NilValue() {
            return;
        }

        // dimnames[[1]] = row names
        if !rl.is_null() {
            *rl = VECTOR_ELT(dimnames, 0);
        }

        // dimnames[[2]] = column names
        if !cl.is_null() {
            *cl = VECTOR_ELT(dimnames, 1);
        }

        // names(dimnames)[1] and names(dimnames)[2]
        // names(dimnames) is the names attribute of dimnames
        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
        let dn_names = getAttrib(dimnames, names_sym);

        if dn_names.is_null() || dn_names == R_NilValue() {
            return;
        }

        // names(dimnames)[[1]] and [[2]] are CHARSXP values
        unsafe extern "C" {
            fn CHAR(x: SEXP) -> *const c_char;
            fn Rf_isNull(x: SEXP) -> c_int;
        }

        if !rn.is_null() {
            let s = VECTOR_ELT(dn_names, 0);
            if !s.is_null() && Rf_isNull(s) == 0 {
                *rn = CHAR(s);
            }
        }

        if !cn.is_null() {
            let s = VECTOR_ELT(dn_names, 1);
            if !s.is_null() && Rf_isNull(s) == 0 {
                *cn = CHAR(s);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_vector_null() {
        unsafe {
            printVector(ptr::null_mut(), 0, 0);
        }
    }

    #[test]
    fn test_print_data_default() {
        unsafe {
            let pd = get_R_PrintData();
            assert_eq!(pd.width, 80);
            assert_eq!(pd.digits, 4);
            assert_eq!(pd.gap, 1);
            assert_eq!(pd.max, 99999);
        }
    }

    #[test]
    fn test_print_data_impl_default() {
        let pd = R_PrintData::default();
        assert_eq!(pd.width, 80);
        assert_eq!(pd.quote, 1);
        assert_eq!(pd.scipen, 0);
    }

    #[test]
    fn test_print_integer_vector() {
        unsafe {
            let vals: [c_int; 3] = [1, 2, 3];
            printIntegerVector(vals.as_ptr(), 3, 1);
        }
    }

    #[test]
    fn test_print_real_vector() {
        unsafe {
            let vals: [f64; 2] = [1.5, 2.5];
            printRealVector(vals.as_ptr(), 2, 1);
        }
    }

    #[test]
    fn test_print_complex_vector() {
        use crate::sexp::ffi::Rcomplex;
        unsafe {
            let vals = [Rcomplex { r: 1.0, i: 2.0 }, Rcomplex { r: 3.0, i: 4.0 }];
            printComplexVector(vals.as_ptr(), 2, 1);
        }
    }

    #[test]
    fn test_print_raw_vector() {
        unsafe {
            let vals: [u8; 4] = [0x00, 0x0a, 0xff, 0x10];
            printRawVector(vals.as_ptr(), 4, 1);
        }
    }

    #[test]
    fn test_print_warnings() {
        unsafe {
            PrintWarnings();
        }
    }

    #[test]
    fn test_get_matrix_dimnames() {
        unsafe {
            let mut rl: SEXP = ptr::null_mut();
            let mut cl: SEXP = ptr::null_mut();
            let mut rn: *const c_char = ptr::null();
            let mut cn: *const c_char = ptr::null();
            GetMatrixDimnames(ptr::null_mut(), &mut rl, &mut cl, &mut rn, &mut cn);
            assert!(rl.is_null() || rl == R_NilValue());
        }
    }

    #[test]
    fn test_constants() {
        assert_eq!(OutDec, b'.' as c_char);
        assert_eq!(R_MIN_LBLOFF, 2);
        assert_eq!(Rprt_adj_right, 1);
        assert_eq!(Rprt_adj_left, 0);
    }

    #[test]
    fn test_index_width_xlen() {
        assert_eq!(index_width_xlen(0), 1);
        assert_eq!(index_width_xlen(1), 1);
        assert_eq!(index_width_xlen(9), 1);
        assert_eq!(index_width_xlen(10), 2);
        assert_eq!(index_width_xlen(99), 2);
        assert_eq!(index_width_xlen(100), 3);
        assert_eq!(index_width_xlen(999), 3);
        assert_eq!(index_width_xlen(1000), 4);
    }

    #[test]
    fn test_type2str_nowarn() {
        unsafe {
            let s = std::ffi::CStr::from_ptr(type2str_nowarn(0))
                .to_str()
                .unwrap();
            assert_eq!(s, "NULL");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(1))
                .to_str()
                .unwrap();
            assert_eq!(s, "symbol");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(10))
                .to_str()
                .unwrap();
            assert_eq!(s, "logical");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(13))
                .to_str()
                .unwrap();
            assert_eq!(s, "integer");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(14))
                .to_str()
                .unwrap();
            assert_eq!(s, "double");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(15))
                .to_str()
                .unwrap();
            assert_eq!(s, "complex");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(16))
                .to_str()
                .unwrap();
            assert_eq!(s, "character");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(19))
                .to_str()
                .unwrap();
            assert_eq!(s, "list");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(20))
                .to_str()
                .unwrap();
            assert_eq!(s, "expression");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(24))
                .to_str()
                .unwrap();
            assert_eq!(s, "raw");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(25))
                .to_str()
                .unwrap();
            assert_eq!(s, "S4");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(999))
                .to_str()
                .unwrap();
            assert_eq!(s, "unknown");
        }
    }

    #[test]
    fn test_print_named_vector_null() {
        unsafe {
            printNamedVector(ptr::null_mut(), ptr::null_mut(), 0, ptr::null());
        }
    }

    #[test]
    fn test_print_named_vector_with_title() {
        unsafe {
            let title = b"My Vector\0".as_ptr() as *const c_char;
            printNamedVector(ptr::null_mut(), ptr::null_mut(), 0, title);
        }
    }

    #[test]
    fn test_print_integer_vector_empty() {
        unsafe {
            let vals: [c_int; 0] = [];
            printIntegerVector(vals.as_ptr(), 0, 0);
        }
    }

    #[test]
    fn test_print_real_vector_empty() {
        unsafe {
            let vals: [f64; 0] = [];
            printRealVector(vals.as_ptr(), 0, 0);
        }
    }

    #[test]
    fn test_print_complex_vector_empty() {
        unsafe {
            let vals: [Rcomplex; 0] = [];
            printComplexVector(vals.as_ptr(), 0, 0);
        }
    }

    #[test]
    fn test_print_raw_vector_empty() {
        unsafe {
            let vals: [u8; 0] = [];
            printRawVector(vals.as_ptr(), 0, 0);
        }
    }

    #[test]
    fn test_print_integer_vector_null_ptr() {
        unsafe {
            printIntegerVector(ptr::null(), 3, 0);
        }
    }

    #[test]
    fn test_print_real_vector_null_ptr() {
        unsafe {
            printRealVector(ptr::null(), 2, 0);
        }
    }

    #[test]
    fn test_print_complex_vector_null_ptr() {
        unsafe {
            printComplexVector(ptr::null(), 2, 0);
        }
    }

    #[test]
    fn test_print_raw_vector_null_ptr() {
        unsafe {
            printRawVector(ptr::null(), 4, 0);
        }
    }
}
