#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/printvector.c -- vector printing.
//!
//! Provides printVector() and printNamedVector() for displaying R vectors,
//! along with type-specific vector printers (printIntegerVector, etc.).

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::mainutils::format::{compact_int_width, compact_real_field, integer_field_width};
use crate::mainutils::printutils::Rprt_adj;
use crate::sexp::accessors::{
    CHAR, COMPLEX_ELT, INTEGER_ELT, LOGICAL_ELT, RAW_ELT, REAL_ELT, STRING_ELT, TYPEOF, VECTOR_ELT,
    XLENGTH,
};
use crate::sexp::altseq::{CompactSeq, unexpanded_int, unexpanded_real};
use crate::sexp::attrib_core::getAttrib;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_REAL, R_IsNA, R_xlen_t, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::symbol::Rf_install;

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

macro_rules! Rprintf {
    ($($arg:tt)*) => {
        crate::mainutils::printutils::console_emit(format_args!($($arg)*))
    };
}

#[repr(transparent)]
pub struct MutPtr<T>(*mut T);

impl<T> std::ops::Deref for MutPtr<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.0 }
    }
}

impl<T> std::ops::DerefMut for MutPtr<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.0 }
    }
}

/// Get the global print data (full R_print parameters).
///
/// Named `get_R_PrintData` to avoid collision with `get_R_print` in
/// printutils.rs (which returns the smaller `RPrint` struct).
pub fn get_R_PrintData() -> MutPtr<R_PrintData> {
    crate::sexp::instance::with_required_current_instance(|inst|
        // SAFETY: `inst` is the live current instance for the closure call.
        unsafe { MutPtr(&mut (*inst).eval_state.printvector as *mut R_PrintData) })
}

// ---------------------------------------------------------------------------
// Checked-access boundary helpers
//
// The printers below run while the session is live. The few operations that
// still need unsafe — C ABI buffers, static encode buffers, raw SEXP data
// pointers and the format/encode entry points in sibling modules — are kept
// here as small audited boundaries so the print loops can stay safe.
// ---------------------------------------------------------------------------

/// Encode a logical value to owned text.
fn encode_logical(x: c_int, w: c_int) -> String {
    text_string(crate::mainutils::printutils::encode_logical_text(x, w))
}

/// Encode an integer value to owned text.
fn encode_integer(x: c_int, w: c_int) -> String {
    text_string(crate::mainutils::printutils::encode_integer_text(x, w))
}

/// Encode a real value to owned text (decimal mark `.`).
fn encode_real(x: std::os::raw::c_double, w: c_int, d: c_int, e: c_int) -> String {
    text_string(crate::mainutils::printutils::encode_real0_text(
        x, w, d, e, b".",
    ))
}

/// Encode a complex value to owned text.
fn encode_complex(
    z: Rcomplex,
    wr: c_int,
    dr: c_int,
    er: c_int,
    wi: c_int,
    di: c_int,
    ei: c_int,
) -> String {
    text_string(crate::mainutils::printutils::encode_complex_text(
        z, wr, dr, er, wi, di, ei,
    ))
}

/// Encode a raw byte to owned text (no prefix).
fn encode_raw(x: u8) -> String {
    text_string(crate::mainutils::printutils::encode_raw_text(x, b""))
}

/// Encoder bytes as text; invalid UTF-8 reads as empty, like `CStr::to_str`.
fn text_string(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_default()
}

fn encode_string_elt(s: SEXP, w: c_int, quote: c_int, justify: c_int) -> String {
    let justify = match justify {
        1 => Rprt_adj::right,
        2 => Rprt_adj::centre,
        3 => Rprt_adj::none,
        _ => Rprt_adj::left,
    };
    crate::mainutils::printutils::encode_string_bytes(s, w, quote, justify)
        .map(text_string)
        .unwrap_or_default()
}

/// formatLogicalS field width for a live vector.
fn format_logical_width(x: SEXP, n: R_xlen_t) -> c_int {
    crate::mainutils::format::logical_width_s(x, n)
}

/// formatIntegerS field width for a live vector.
fn format_integer_s_width(x: SEXP, n: R_xlen_t) -> c_int {
    crate::mainutils::format::integer_width_s(x, n)
}

/// formatStringS field width for a live vector.
fn format_string_width(x: SEXP, n: R_xlen_t, quote: c_int) -> c_int {
    crate::mainutils::format::string_width_s(x, n, quote)
}

/// formatRealS (w, d, e) for a live vector.
fn format_real_s_fields(x: SEXP, n: R_xlen_t) -> (c_int, c_int, c_int) {
    let f = crate::mainutils::format::real_field_s(x, n, 0);
    (f.w, f.d, f.e)
}

/// formatComplexS fields for a live vector.
fn format_complex_s_fields(x: SEXP, n: R_xlen_t) -> (c_int, c_int, c_int, c_int, c_int, c_int) {
    let f = crate::mainutils::format::complex_field_s(x, n, 0);
    (f.wr, f.dr, f.er, f.wi, f.di, f.ei)
}

/// formatRawS field width (always two hex digits).
fn format_raw_s_width(_x: SEXP, _n: R_xlen_t) -> c_int {
    2
}

/// Checked read handle for a live vector in the active session.
fn checked_vector(x: SEXP) -> crate::sexp::object::Sexp<'static> {
    crate::mainutils::essentials::owner_sexp(crate::mainutils::essentials::active_owner(), x)
}

fn checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// GNU NUMVECTOR_TIGHTLOOP: print `n` cells of width `w` (gap included),
/// wrapping at the print width and labelling lines when `indx` is set.
fn print_cells(n: R_xlen_t, indx: c_int, w: c_int, mut cell: impl FnMut(R_xlen_t) -> String) {
    let rp = get_R_PrintData();
    let mut labwidth: c_int = 0;
    let mut width: c_int;

    // DO_first_lab
    if indx != 0 {
        labwidth = index_width_xlen(n) + 2;
        vector_index(1, labwidth);
        width = labwidth;
    } else {
        width = 0;
    }

    for i in 0..n {
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
        let s = cell(i);
        Rprintf!("{}", s);
        width += w;
    }
    Rprintf!("\n");
}

/// Integer cells from `elt` (GNU printIntegerVector body).
fn print_integer_cells(n: R_xlen_t, indx: c_int, elt: impl Fn(R_xlen_t) -> c_int) {
    let w = if n <= 0 {
        1
    } else {
        integer_field_width(n, &elt)
    } + get_R_PrintData().gap;
    print_cells(n, indx, w, |i| encode_integer(elt(i), w));
}

/// Real cells from `elt` (GNU printRealVector body).
fn print_real_cells(n: R_xlen_t, indx: c_int, elt: impl Fn(R_xlen_t) -> f64) {
    warn_illegal_outdec();
    let f = if n <= 0 {
        crate::mainutils::format::RealField { w: 0, d: 0, e: 0 }
    } else {
        crate::mainutils::format::real_field(n, 0, &elt)
    };
    let w = f.w + get_R_PrintData().gap;
    print_cells(n, indx, w, |i| encode_real(elt(i), w, f.d, f.e));
}

/// Complex cells (GNU printComplexVector body).
fn print_complex_cells(data: &[Rcomplex], n: R_xlen_t, indx: c_int) {
    let rp = get_R_PrintData();
    let gap = rp.gap;
    let f = if data.is_empty() {
        crate::mainutils::format::ComplexField::default()
    } else {
        crate::mainutils::format::complex_field(data, 0)
    };
    let w = f.wr + f.wi + 2 + gap; // +2 for "+" and "i"
    print_cells(n, indx, w, |i| {
        let cx = data.get(i as usize).copied().unwrap_or(Rcomplex {
            r: NA_REAL,
            i: NA_REAL,
        });
        if R_IsNA(cx.r) || R_IsNA(cx.i) {
            encode_real(NA_REAL, w, 0, 0)
        } else {
            encode_complex(cx, f.wr + gap, f.dr, f.er, f.wi, f.di, f.ei)
        }
    });
}

/// Raw cells from `elt` (GNU printRawVector body).
fn print_raw_cells(n: R_xlen_t, indx: c_int, elt: impl Fn(R_xlen_t) -> u8) {
    let gap = get_R_PrintData().gap;
    let w = 2 + gap;
    print_cells(n, indx, w, |i| {
        format!("{:width$}{}", "", encode_raw(elt(i)), width = gap as usize)
    });
}

/// A live CHARSXP's byte length.
fn charsxp_len(x: SEXP) -> Option<usize> {
    Some(crate::mainutils::printutils::charsxp_bytes(x).len())
}

/// Option lookup under the active runtime.
fn get_option(name: &str) -> SEXP {
    crate::mainutils::options::GetOptionByName(name)
}

/// Audited boundary: copy a NUL-terminated title (or null) to Rust text.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn title_string(title: *const c_char) -> Option<String> {
    if title.is_null() {
        return None;
    }
    // SAFETY: callers pass a live NUL-terminated title or null.
    Some(
        unsafe { std::ffi::CStr::from_ptr(title) }
            .to_str()
            .unwrap_or("")
            .to_string(),
    )
}

/// Audited boundary: a live CHARSXP's raw pointer for an out-parameter.
fn charsxp_ptr(x: SEXP) -> *const c_char {
    // SAFETY: a live CHARSXP exposes NUL-terminated CHAR data; the pointer is
    // handed to a caller-owned out-parameter under the documented contract.
    unsafe { CHAR(x) }
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

/// Print a vector index label "[`i']", using total width `w` (left filling
/// blanks). GNU VectorIndex: `Rprintf("%*s[%ld]", w-IndexWidth(i)-2, "", i)`
/// — no trailing space; the element loops supply the gap.
fn vector_index(i: R_xlen_t, w: c_int) {
    let pad = (w - index_width_xlen(i) - 2).max(0) as usize;
    Rprintf!("{:>pad$}[{}]", "", i, pad = pad);
}

// ---------------------------------------------------------------------------
// printLogicalVectorS -- internal, not exported (static in C)
// ---------------------------------------------------------------------------

fn printLogicalVectorS(x: SEXP, n: R_xlen_t, indx: c_int) {
    let w = format_logical_width(x, n) + get_R_PrintData().gap;
    let src = checked_vector(x);
    print_cells(n, indx, w, |i| {
        encode_logical(checked(src.try_logical_elt(i)), w)
    });
}

// ---------------------------------------------------------------------------
// printIntegerVector -- exported
// ---------------------------------------------------------------------------

pub unsafe fn printIntegerVector(x: *const c_int, n: R_xlen_t, indx: c_int) {
    let data: &[c_int] = if x.is_null() || n <= 0 {
        &[]
    } else {
        // SAFETY: the caller passes a live buffer of `n` integers.
        unsafe { std::slice::from_raw_parts(x, n as usize) }
    };
    if data.is_empty() && n > 0 {
        // A null buffer formats as width 1 and prints NA cells.
        let w = 1 + get_R_PrintData().gap;
        print_cells(n, indx, w, |_| encode_integer(NA_INTEGER, w));
        return;
    }
    print_integer_cells(n, indx, |i| data[i as usize]);
}

pub(crate) fn warn_illegal_outdec() {
    {
        let opt = get_option("OutDec");
        if opt.is_null()
            || opt == crate::sexp::globals::R_NilValue()
            || crate::sexp::accessors::TYPEOF(opt) != crate::sexp::ffi::SEXPTYPE::STRSXP
            || crate::sexp::accessors::LENGTH(opt) < 1
        {
            return;
        }
        let ch = crate::sexp::accessors::STRING_ELT(opt, 0);
        if ch.is_null() {
            return;
        }
        let Some(nbytes) = charsxp_len(ch) else {
            return;
        };
        if nbytes == 1 {
            return;
        }
        let msg = if nbytes > 1 {
            "the decimal mark is more than one character wide; this will become an error"
        } else {
            "the decimal mark is less than one character wide; this will become an error"
        };
        if let Ok(c) = std::ffi::CString::new(msg) {
            crate::mainutils::errors::Rf_warning(c.as_ptr());
        }
    }
}

// ---------------------------------------------------------------------------
// printRealVector -- exported
// ---------------------------------------------------------------------------
pub unsafe fn printRealVector(x: *const f64, n: R_xlen_t, indx: c_int) {
    let data: &[f64] = if x.is_null() || n <= 0 {
        &[]
    } else {
        // SAFETY: the caller passes a live buffer of `n` doubles.
        unsafe { std::slice::from_raw_parts(x, n as usize) }
    };
    if data.is_empty() && n > 0 {
        // A null buffer formats as (0, 0, 0) and prints NA cells.
        warn_illegal_outdec();
        let w = get_R_PrintData().gap;
        print_cells(n, indx, w, |_| encode_real(NA_REAL, w, 0, 0));
        return;
    }
    print_real_cells(n, indx, |i| data[i as usize]);
}

// ---------------------------------------------------------------------------
// printComplexVector -- exported
// ---------------------------------------------------------------------------

pub unsafe fn printComplexVector(x: *const Rcomplex, n: R_xlen_t, indx: c_int) {
    let data: &[Rcomplex] = if x.is_null() || n <= 0 {
        &[]
    } else {
        // SAFETY: the caller passes a live buffer of `n` complex values.
        unsafe { std::slice::from_raw_parts(x, n as usize) }
    };
    print_complex_cells(data, n, indx);
}

// ---------------------------------------------------------------------------
// printRawVector -- exported
// ---------------------------------------------------------------------------

pub unsafe fn printRawVector(x: *const u8, n: R_xlen_t, indx: c_int) {
    let data: &[u8] = if x.is_null() || n <= 0 {
        &[]
    } else {
        // SAFETY: the caller passes a live buffer of `n` bytes.
        unsafe { std::slice::from_raw_parts(x, n as usize) }
    };
    print_raw_cells(n, indx, |i| data.get(i as usize).copied().unwrap_or(0));
}

// ---------------------------------------------------------------------------
// printStringVectorS -- internal (static in C)
// ---------------------------------------------------------------------------

fn printStringVectorS(x: SEXP, n: R_xlen_t, quote: c_int, indx: c_int) {
    {
        // Always use STRING_ELT path (DATAPTR_OR_NULL not available).
        let rp = get_R_PrintData();
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

        let w = format_string_width(x, n, quote);

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
            let s = encode_string_elt(si, w, quote, rp.right);
            Rprintf!("{:width$}{}", "", s, width = rp.gap as usize);
            width += w + rp.gap;
        }
        Rprintf!("\n");
    }
}

// ---------------------------------------------------------------------------
// printRawVectorS -- internal (static in C)
// ---------------------------------------------------------------------------

fn printRawVectorS(x: SEXP, n: R_xlen_t, indx: c_int) {
    let src = checked_vector(x);
    print_raw_cells(n, indx, |i| checked(src.try_raw_elt(i)));
}

// ---------------------------------------------------------------------------
// printRealVectorS -- internal (static in C)
// ---------------------------------------------------------------------------

fn printRealVectorS(x: SEXP, n: R_xlen_t, indx: c_int) {
    if let Some(seq) = unexpanded_real(x) {
        print_unexpanded_real_vector(seq, n, indx);
        return;
    }
    let src = checked_vector(x);
    print_real_cells(n, indx, |i| checked(src.try_real_elt(i)));
}

/// Same layout as [`printRealVector`], reading the formula instead of a buffer.
fn print_unexpanded_real_vector(seq: CompactSeq, n: R_xlen_t, indx: c_int) {
    {
        warn_illegal_outdec();
        let rp = get_R_PrintData();
        let mut labwidth: c_int = 0;
        let mut width: c_int = 0;

        if indx != 0 {
            labwidth = index_width_xlen(n) + 2;
            vector_index(1, labwidth);
            width = labwidth;
        } else {
            width = 0;
        }

        let fmt = compact_real_field(seq, 0, n, 0);
        let w = fmt.w + rp.gap;

        for i in 0..n {
            let xi = seq.real_or_na(i);
            if i > 0 && width + w > rp.width {
                Rprintf!("\n");
                if indx != 0 {
                    vector_index(i + 1, labwidth);
                    width = labwidth;
                } else {
                    width = 0;
                }
            }
            let s = encode_real(xi, w, fmt.d, fmt.e);
            Rprintf!("{}", s);
            width += w;
        }
        Rprintf!("\n");
    }
}

// ---------------------------------------------------------------------------
// printIntegerVectorS -- internal (static in C)
// ---------------------------------------------------------------------------

fn printIntegerVectorS(x: SEXP, n: R_xlen_t, indx: c_int) {
    if let Some(seq) = unexpanded_int(x) {
        print_unexpanded_integer_vector(seq, n, indx);
        return;
    }
    let src = checked_vector(x);
    print_integer_cells(n, indx, |i| checked(src.try_integer_elt(i)));
}

/// Same layout as [`printIntegerVector`], reading the formula instead of a buffer.
fn print_unexpanded_integer_vector(seq: CompactSeq, n: R_xlen_t, indx: c_int) {
    {
        let rp = get_R_PrintData();
        let mut labwidth: c_int = 0;
        let mut width: c_int = 0;

        if indx != 0 {
            labwidth = index_width_xlen(n) + 2;
            vector_index(1, labwidth);
            width = labwidth;
        } else {
            width = 0;
        }

        let mut w = compact_int_width(seq, 0, n);
        w += rp.gap;

        for i in 0..n {
            let xi = seq.int_or_na(i);
            if i > 0 && width + w > rp.width {
                Rprintf!("\n");
                if indx != 0 {
                    vector_index(i + 1, labwidth);
                    width = labwidth;
                } else {
                    width = 0;
                }
            }
            let s = encode_integer(xi, w);
            Rprintf!("{}", s);
            width += w;
        }
        Rprintf!("\n");
    }
}

// ---------------------------------------------------------------------------
// printComplexVectorS -- internal (static in C)
// ---------------------------------------------------------------------------

fn printComplexVectorS(x: SEXP, n: R_xlen_t, indx: c_int) {
    let src = checked_vector(x);
    let data: Vec<Rcomplex> = (0..n.max(0))
        .map(|i| checked(src.try_complex_elt(i)))
        .collect();
    print_complex_cells(&data, n, indx);
}

// ---------------------------------------------------------------------------
// printVector -- exported
// ---------------------------------------------------------------------------

pub fn printVector(x: SEXP, indx: c_int, quote: c_int) {
    {
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
            if t == SEXPTYPE::LGLSXP {
                printLogicalVectorS(x, n_pr, indx);
            } else if t == SEXPTYPE::INTSXP {
                printIntegerVectorS(x, n_pr, indx);
            } else if t == SEXPTYPE::REALSXP {
                printRealVectorS(x, n_pr, indx);
            } else if t == SEXPTYPE::STRSXP {
                if quote != 0 {
                    printStringVectorS(x, n_pr, '"' as c_int, indx);
                } else {
                    printStringVectorS(x, n_pr, 0, indx);
                }
            } else if t == SEXPTYPE::CPLXSXP {
                printComplexVectorS(x, n_pr, indx);
            } else if t == SEXPTYPE::RAWSXP {
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
                _ => {} // intentionally unhandled: unsupported type for empty vector printing
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Named vector printers (internal, static in C)
// ---------------------------------------------------------------------------

/// Helper: print the name row for named vectors.
fn print_name_row(names: SEXP, n: R_xlen_t, w: c_int, nperline: c_int, start: R_xlen_t) {
    {
        let rp = get_R_PrintData();
        let mut j: R_xlen_t = 0;
        while j < nperline as R_xlen_t {
            let k = start + j;
            if k >= n {
                break;
            }
            let s = encode_string_elt(STRING_ELT(names, k), w, 0, Rprt_adj_right);
            Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
            j += 1;
        }
        Rprintf!("\n");
    }
}

fn printNamedLogicalVectorS(x: SEXP, n: R_xlen_t, names: SEXP) {
    {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut wn: c_int = 0;

        w = format_logical_width(x, n);
        wn = format_string_width(names, n, 0);
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
                let s = encode_logical(LOGICAL_ELT(x, k as c_int), w);
                Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
                j += 1;
            }
            i += 1;
        }
        Rprintf!("\n");
    }
}

fn printNamedIntegerVectorS(x: SEXP, n: R_xlen_t, names: SEXP) {
    {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut wn: c_int = 0;

        w = format_integer_s_width(x, n);
        wn = format_string_width(names, n, 0);
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
                let s = encode_integer(INTEGER_ELT(x, k as c_int), w);
                Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
                j += 1;
            }
            i += 1;
        }
        Rprintf!("\n");
    }
}

fn printNamedRealVectorS(x: SEXP, n: R_xlen_t, names: SEXP) {
    {
        let rp = get_R_PrintData();
        let mut wn: c_int = 0;

        let (mut w, d, e) = format_real_s_fields(x, n);
        wn = format_string_width(names, n, 0);
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
                let s = encode_real(REAL_ELT(x, k as c_int), w, d, e);
                Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
                j += 1;
            }
            i += 1;
        }
        Rprintf!("\n");
    }
}

fn printNamedComplexVectorS(x: SEXP, n: R_xlen_t, names: SEXP) {
    {
        let mut w: c_int = 0;

        let rp = get_R_PrintData();

        let (wr, dr, er, wi, di, ei) = format_complex_s_fields(x, n);
        w = wr + wi + 2;

        let mut wn: c_int = 0;
        wn = format_string_width(names, n, 0);
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
            let mut j: R_xlen_t = 0;
            while j < nperline as R_xlen_t {
                let k = i * nperline as R_xlen_t + j;
                if k >= n {
                    break;
                }
                let s = encode_string_elt(STRING_ELT(names, k as i64), w, 0, Rprt_adj_right);
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
                    let s = encode_real(NA_REAL, w, 0, 0);
                    Rprintf!("{}", s);
                } else {
                    let s_re = encode_real(tmp.r, wr, dr, er);
                    if ISNAN(tmp.i) {
                        Rprintf!("{}+NaNi", s_re);
                    } else if tmp.i >= 0.0 {
                        let s_im = encode_real(tmp.i, wi, di, ei);
                        Rprintf!("{}+{}i", s_re, s_im);
                    } else {
                        let s_im = encode_real(-tmp.i, wi, di, ei);
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

fn printNamedStringVectorS(x: SEXP, n: R_xlen_t, quote: c_int, names: SEXP) {
    {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut wn: c_int = 0;

        w = format_string_width(x, n, quote);
        wn = format_string_width(names, n, 0);
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
                let s = encode_string_elt(STRING_ELT(x, k), w, quote, Rprt_adj_right);
                Rprintf!("{}{:width$}", s, "", width = rp.gap as usize);
                j += 1;
            }
            i += 1;
        }
        Rprintf!("\n");
    }
}

fn printNamedRawVectorS(x: SEXP, n: R_xlen_t, names: SEXP) {
    {
        let rp = get_R_PrintData();
        let mut w: c_int = 0;
        let mut wn: c_int = 0;

        w = format_raw_s_width(x, n);
        wn = format_string_width(names, n, 0);
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
            let mut j: R_xlen_t = 0;
            while j < nperline as R_xlen_t {
                let k = i * nperline as R_xlen_t + j;
                if k >= n {
                    break;
                }
                let s = encode_string_elt(STRING_ELT(names, k as i64), w, 0, Rprt_adj_right);
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
                let s = encode_raw(val);
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

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn printNamedVector(x: SEXP, names: SEXP, quote: c_int, title: *const c_char) {
    {
        // SAFETY: forwarded raw pointers satisfy `title_string`'s contract per this function's own `# Safety` requirements.
        if let Some(title) = unsafe { title_string(title) } {
            Rprintf!("{}\n", title);
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
            if t == SEXPTYPE::LGLSXP {
                printNamedLogicalVectorS(x, n_pr, names);
            } else if t == SEXPTYPE::INTSXP {
                printNamedIntegerVectorS(x, n_pr, names);
            } else if t == SEXPTYPE::REALSXP {
                printNamedRealVectorS(x, n_pr, names);
            } else if t == SEXPTYPE::CPLXSXP {
                printNamedComplexVectorS(x, n_pr, names);
            } else if t == SEXPTYPE::STRSXP {
                let q = if quote != 0 { '"' as c_int } else { 0 };
                printNamedStringVectorS(x, n_pr, q, names);
            } else if t == SEXPTYPE::RAWSXP {
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
                _ => {} // intentionally unhandled: unsupported type for empty vector printing
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PrintWarnings -- stub (depends on R_GetVar1 etc.)
// ---------------------------------------------------------------------------

pub fn PrintWarnings() {
    // the full implementation iterates over R's last.warning list
    // and prints each warning message.
}

// ---------------------------------------------------------------------------
// type2str_nowarn
// ---------------------------------------------------------------------------

pub fn type2str_nowarn(stype: c_int) -> *const c_char {
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

        // GNU GetMatrixDimnames: names(dimnames) is STRSXP; STRING_ELT
        // then CHAR. Empty strings stay non-NULL so printarray emits the
        // title row and R_MIN_LBLOFF indent.
        let names_sym = crate::sexp::attrib_core::R_NamesSymbol();
        let dn_names = getAttrib(dimnames, names_sym);

        if dn_names.is_null() || dn_names == R_NilValue() {
            return;
        }

        if !rn.is_null() && XLENGTH(dn_names) > 0 {
            let s = STRING_ELT(dn_names, 0);
            if !s.is_null() && s != R_NilValue() {
                *rn = CHAR(s);
            }
        }

        if !cn.is_null() && XLENGTH(dn_names) > 1 {
            let s = STRING_ELT(dn_names, 1);
            if !s.is_null() && s != R_NilValue() {
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
    use crate::sexp::session::RSession;

    #[test]
    fn test_print_vector_null() {
        printVector(ptr::null_mut(), 0, 0);
    }

    #[test]
    fn test_print_data_default() {
        let _session = RSession::new();
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
        let _session = RSession::new();
        unsafe {
            let vals: [c_int; 3] = [1, 2, 3];
            printIntegerVector(vals.as_ptr(), 3, 1);
        }
    }

    #[test]
    fn test_print_real_vector() {
        let _session = RSession::new();
        unsafe {
            let vals: [f64; 2] = [1.5, 2.5];
            printRealVector(vals.as_ptr(), 2, 1);
        }
    }

    #[test]
    fn test_print_complex_vector() {
        use crate::sexp::ffi::Rcomplex;
        let _session = RSession::new();
        unsafe {
            let vals = [Rcomplex { r: 1.0, i: 2.0 }, Rcomplex { r: 3.0, i: 4.0 }];
            printComplexVector(vals.as_ptr(), 2, 1);
        }
    }

    #[test]
    fn test_print_raw_vector() {
        let _session = RSession::new();
        unsafe {
            let vals: [u8; 4] = [0x00, 0x0a, 0xff, 0x10];
            printRawVector(vals.as_ptr(), 4, 1);
        }
    }

    #[test]
    fn test_print_warnings() {
        {
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
                .unwrap_or("");
            assert_eq!(s, "NULL");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(1))
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "symbol");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(10))
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "logical");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(13))
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "integer");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(14))
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "double");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(15))
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "complex");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(16))
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "character");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(19))
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "list");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(20))
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "expression");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(24))
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "raw");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(25))
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "S4");

            let s = std::ffi::CStr::from_ptr(type2str_nowarn(999))
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "unknown");
        }
    }

    #[test]
    fn test_print_named_vector_null() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { printNamedVector(ptr::null_mut(), ptr::null_mut(), 0, ptr::null()) };
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
        let _session = RSession::new();
        unsafe {
            let vals: [c_int; 0] = [];
            printIntegerVector(vals.as_ptr(), 0, 0);
        }
    }

    #[test]
    fn test_print_real_vector_empty() {
        let _session = RSession::new();
        unsafe {
            let vals: [f64; 0] = [];
            printRealVector(vals.as_ptr(), 0, 0);
        }
    }

    #[test]
    fn test_print_complex_vector_empty() {
        let _session = RSession::new();
        unsafe {
            let vals: [Rcomplex; 0] = [];
            printComplexVector(vals.as_ptr(), 0, 0);
        }
    }

    #[test]
    fn test_print_raw_vector_empty() {
        let _session = RSession::new();
        unsafe {
            let vals: [u8; 0] = [];
            printRawVector(vals.as_ptr(), 0, 0);
        }
    }

    #[test]
    fn test_print_integer_vector_null_ptr() {
        let _session = RSession::new();
        unsafe {
            printIntegerVector(ptr::null(), 3, 0);
        }
    }

    #[test]
    fn test_print_real_vector_null_ptr() {
        let _session = RSession::new();
        unsafe {
            printRealVector(ptr::null(), 2, 0);
        }
    }

    #[test]
    fn test_print_complex_vector_null_ptr() {
        let _session = RSession::new();
        unsafe {
            printComplexVector(ptr::null(), 2, 0);
        }
    }

    #[test]
    fn test_print_raw_vector_null_ptr() {
        let _session = RSession::new();
        unsafe {
            printRawVector(ptr::null(), 4, 0);
        }
    }
}
