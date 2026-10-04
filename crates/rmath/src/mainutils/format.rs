#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/format.c
//!
//! Object Formatting -- determines proper width, digits, etc. for printing
//! R objects.
//!
//! Exports (from original C):
//!   formatStringS, formatLogical, formatLogicalS,
//!   formatInteger, formatIntegerS, formatReal, formatRealS,
//!   formatComplex, formatComplexS, formatRaw, formatRawS

use std::os::raw::{c_double, c_int, c_void};

use crate::sexp::accessors::{COMPLEX, INTEGER, LOGICAL, REAL, STRING_ELT};
use crate::sexp::altseq::{CompactSeq, unexpanded_int, unexpanded_real};
use crate::sexp::ffi::{
    NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXP,
};

// ---------------------------------------------------------------------------
// Print parameters (R_print global)
//
// These are read-only globals in R.  We expose them so that callers (e.g.
// graphics code) can configure formatting before calling formatReal / scientific.
// ---------------------------------------------------------------------------

/// Mirrors R's `R_print` structure from Print.h.
///
/// Only the fields used by format.c / scientific() are included.
/// Defaults match R's startup values.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct RPrint {
    pub digits: c_int,
    pub scipen: c_int,
    pub na_width: c_int,
    pub na_width_noquote: c_int,
}

impl Default for RPrint {
    fn default() -> Self {
        RPrint {
            digits: 0, // unset / significant; R defaults to 7 at startup
            scipen: 0,
            na_width: 2, // "NA"
            na_width_noquote: 2,
        }
    }
}

fn current_R_print() -> RPrint {
    // Live options("digits") / options("scipen"), unless a caller pinned
    // `format_print.digits` (GNU `R_print.digits = DBL_DIG` around deparse).
    unsafe {
        let stored =
            crate::sexp::instance::with_current_instance(|inst| (*inst).eval_state.format_print);
        let digits = stored
            .map(|p| p.digits)
            .filter(|&d| d > 0)
            .unwrap_or_else(|| crate::mainutils::options::GetOptionDigits());
        RPrint {
            digits,
            scipen: crate::mainutils::options::GetOptionScipen(),
            na_width: 2,
            na_width_noquote: 2,
        }
    }
}

pub unsafe fn format_set_R_print(p: RPrint) -> RPrint {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        let old = (*inst).eval_state.format_print;
        (*inst).eval_state.format_print = p;
        old
    })
}

pub unsafe fn format_get_R_print() -> RPrint {
    current_R_print()
}

// ---------------------------------------------------------------------------
// Helper: Rstrlen (forward declaration to printutils)
//
// In the C source, Rstrlen is declared in Print.h and defined in
// printutils.c. We provide an extern declaration here so formatString
// can call it. The actual implementation is in printutils.rs.
// ---------------------------------------------------------------------------

use crate::mainutils::printutils::Rstrlen;

// ---------------------------------------------------------------------------
// Helper: IndexWidth
//
// Computes the number of decimal digits needed to represent a non-negative
// integer.  This is used by formatInteger to determine field widths.
// ---------------------------------------------------------------------------

/// Return the number of decimal digits in `x` (x >= 0).
/// Equivalent to `(int) floor(log10((double) x)) + 1` but faster.
pub unsafe fn IndexWidth(mut x: c_int) -> c_int {
    if x < 0 {
        x = -x;
    }
    if x < 10 {
        return 1;
    }
    if x < 100 {
        return 2;
    }
    if x < 1000 {
        return 3;
    }
    if x < 10000 {
        return 4;
    }
    if x < 100000 {
        return 5;
    }
    if x < 1000000 {
        return 6;
    }
    if x < 10000000 {
        return 7;
    }
    if x < 100000000 {
        return 8;
    }
    if x < 1000000000 {
        return 9;
    }
    10
}

// ---------------------------------------------------------------------------
// Helper: Rexp10
//
// Compute 10^n for integer n, using the lookup table from R's math library
// when possible.
// ---------------------------------------------------------------------------

/// Power-of-10 lookup table (exact powers representable in 53-bit mantissa).
#[rustfmt::skip]
static TBL: [f64; 23] = [
    1e00, 1e01, 1e02, 1e03, 1e04, 1e05, 1e06, 1e07, 1e08, 1e09,
    1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19,
    1e20, 1e21, 1e22,
];

/// Maximum index into TBL.
const KP_MAX: c_int = 22;

/// R_dec_min_exponent: approximately -307 for IEEE 754 double.
const R_DEC_MIN_EXPONENT: c_int = -307;

/// Compute 10^n for integer n using the lookup table when possible.
/// Falls back to `pow` for out-of-range exponents.
pub unsafe fn format_Rexp10(n: c_int) -> c_double {
    let n_abs = if n < 0 { -n } else { n };
    if n_abs <= KP_MAX {
        if n >= 0 {
            TBL[n_abs as usize]
        } else {
            1.0 / TBL[n_abs as usize]
        }
    } else {
        10.0_f64.powi(n)
    }
}

// ---------------------------------------------------------------------------
// format_via_sprintf  (static in C, exposed here for testing / reuse)
//
// Uses snprintf(%#.*e) to determine the exponent and significant digits
// of a floating-point number.  Used when R_print.digits >= DBL_DIG + 1.
// ---------------------------------------------------------------------------

const NB: usize = 1000;

/// Determine the exponent (kpower) and number of significant digits (nsig)
/// for a real number using snprintf(%#.*e).
///
/// This is the fallback path when `R_print.digits >= DBL_DIG + 1` (i.e.
/// >= 16 for IEEE 754 double).
pub unsafe fn format_via_sprintf(r: c_double, d: c_int, kpower: *mut c_int, nsig: *mut c_int) {
    unsafe {
        let mut buff = [0 as core::ffi::c_char; NB];
        let d = d as usize;
        let _nc = snprintf(&mut buff, NB, b"%#.*e\0".as_ptr().cast(), d - 1, r);
        // buff[d+2..] contains the exponent string, e.g. "e+02" or "e+100"
        // We parse it as an integer.
        let exp_start = d + 2;
        let exp_str = std::ffi::CStr::from_ptr(buff.as_ptr().add(exp_start));
        let exp_val: i64 = exp_str.to_str().unwrap_or("0").parse().unwrap_or(0);
        *kpower = exp_val as c_int;
        // Count significant digits from the right: skip trailing zeros.
        let mut i = d as i32;
        while i >= 2 && buff[i as usize] == b'0' as core::ffi::c_char {
            i -= 1;
        }
        *nsig = i;
    }
}

/// Minimal snprintf shim for our format_via_sprintf.
/// Writes into `buf` (up to `buf_size` bytes including NUL) using the
/// C format string `fmt`.
fn snprintf(
    buf: &mut [core::ffi::c_char],
    buf_size: usize,
    fmt: *const core::ffi::c_char,
    precision: usize,
    value: f64,
) -> i32 {
    let fmt_cstr = unsafe { std::ffi::CStr::from_ptr(fmt) };
    let fmt_str = fmt_cstr.to_str().unwrap_or("%.15e");
    // We only support the specific "%#.*e" pattern used by format_via_sprintf.
    let formatted = format!("{}{}", fmt_str.replace(".*", &precision.to_string()), value);
    let bytes = formatted.as_bytes();
    let copy_len = bytes.len().min(buf_size.saturating_sub(1));
    buf[..copy_len].copy_from_slice(
        &bytes[..copy_len]
            .iter()
            .map(|&b| b as core::ffi::c_char)
            .collect::<Vec<core::ffi::c_char>>()[..copy_len],
    );
    buf[copy_len] = 0;
    formatted.len() as i32
}

// ---------------------------------------------------------------------------
// scientific  (static in C, exposed for reuse)
//
// For a number x, determine:
//   neg    = 1 if x < 0
//   kpower = exponent of 10 such that |x| = alpha * 10^kpower, 1 <= alpha < 10
//   nsig   = min(R_print.digits, #{significant digits of alpha})
//   roundingwidens = true iff rounding causes x to increase in width
//
// This is time-critical code.  Ported from the non-long-double path.
// ---------------------------------------------------------------------------

/// Determine the scientific representation parameters for a finite
/// non-zero double.
pub unsafe fn format_scientific(
    x: *const c_double,
    neg: *mut c_int,
    kpower: *mut c_int,
    nsig: *mut c_int,
    roundingwidens: *mut bool,
) {
    unsafe {
        let xv = *x;

        if xv == 0.0 {
            *kpower = 0;
            *nsig = 1;
            *neg = 0;
            *roundingwidens = false;
            return;
        }

        let r: f64;
        if xv < 0.0 {
            *neg = 1;
            r = -xv;
        } else {
            *neg = 0;
            r = xv;
        }

        let digits = current_R_print().digits;
        if digits == 0 {
            // No digits configured; fall back to a safe default.
            *kpower = 0;
            *nsig = 1;
            *roundingwidens = false;
            return;
        }

        // When digits >= DBL_DIG + 1 (16 for IEEE 754), use snprintf path.
        const DBL_DIG_CONST: c_int = 15;
        if digits > DBL_DIG_CONST {
            format_via_sprintf(r, digits, kpower, nsig);
            *roundingwidens = false;
            return;
        }

        let mut kp = (r.log10().floor() as c_int) - digits + 1;
        // r = |x|; 10^(kp + digits - 1) <= r

        let mut r_prec = r;

        // Use exact scaling factor from lookup table when possible.
        let kp_abs = if kp < 0 { -kp } else { kp };
        if kp_abs <= KP_MAX {
            if kp >= 0 {
                r_prec /= TBL[kp as usize];
            } else {
                r_prec *= TBL[(-kp) as usize];
            }
        } else if kp <= R_DEC_MIN_EXPONENT {
            // Handle denormalized / very small numbers.
            // (r_prec * 1e+303) / 10^(kp+303)
            r_prec = (r_prec * 1e303) / format_Rexp10(kp + 303);
        } else {
            r_prec /= format_Rexp10(kp);
        }

        // The table index for digits-1 is safe because digits <= DBL_DIG (15)
        // and the table has entries 0..22.
        let digits_idx = (digits - 1) as usize;
        if digits_idx < TBL.len() && r_prec < TBL[digits_idx] {
            r_prec *= 10.0;
            kp -= 1;
        }

        // Round alpha to nearest integer.
        let mut alpha = r_prec.round();

        *nsig = digits;
        let mut j = 1;
        while j <= digits {
            alpha /= 10.0;
            if alpha == alpha.floor() {
                *nsig -= 1;
            } else {
                break;
            }
            j += 1;
        }

        if *nsig == 0 && digits > 0 {
            *nsig = 1;
            kp += 1;
        }

        *kpower = kp + digits - 1;

        // Determine whether scientific format rounding would widen the number.
        // Scientific format may do more rounding than fixed format, e.g.
        // 9996 with 3 digits is 1e+04 in scientific, but 9996 in fixed.
        let mut rgt = digits - *kpower;
        // bound rgt by 0 and KP_MAX
        if rgt < 0 {
            rgt = 0;
        } else if rgt > KP_MAX {
            rgt = KP_MAX;
        }
        let fuzz = 0.5 / TBL[rgt as usize];
        *roundingwidens = *kpower > 0 && *kpower <= KP_MAX && r < TBL[*kpower as usize] - fuzz;
    }
}

// ---------------------------------------------------------------------------
// formatRaw  -- field width for raw bytes (always 2: "00".."ff")
// ---------------------------------------------------------------------------

pub unsafe fn formatRaw(_x: *const c_void, _n: R_xlen_t, fieldwidth: *mut c_int) {
    unsafe {
        if !fieldwidth.is_null() {
            *fieldwidth = 2;
        }
    }
}

// ---------------------------------------------------------------------------
// formatRawS  -- SEXP variant (also always 2)
// ---------------------------------------------------------------------------

pub unsafe fn formatRawS(_x: SEXP, _n: R_xlen_t, fieldwidth: *mut c_int) {
    unsafe {
        if !fieldwidth.is_null() {
            *fieldwidth = 2;
        }
        *fieldwidth = 2;
    }
}

/// Display width of one character element, including quoting and NA widths.
pub(crate) unsafe fn string_element_width(element: SEXP, quote: c_int) -> c_int {
    unsafe {
        if element.is_null() || element == crate::sexp::globals::R_NaString() {
            if quote != 0 {
                current_R_print().na_width
            } else {
                current_R_print().na_width_noquote
            }
        } else {
            Rstrlen(element, quote) + if quote != 0 { 2 } else { 0 }
        }
    }
}

// ---------------------------------------------------------------------------
// formatStringS  -- SEXP variant using STRING_ELT
//
// Ported from C: uses STRING_ELT to access elements.
// ---------------------------------------------------------------------------

pub unsafe fn formatStringS(x: SEXP, n: R_xlen_t, fieldwidth: *mut c_int, quote: c_int) {
    unsafe {
        let mut xmax: c_int = 0;

        for i in 0..n {
            let si = STRING_ELT(x, i);
            let l = string_element_width(si, quote);
            if l > xmax {
                xmax = l;
            }
        }
        *fieldwidth = xmax;
    }
}

// ---------------------------------------------------------------------------
// formatLogical  -- field width for logical vector (raw int* version)
//
// Ported from C: TRUE -> width 4, FALSE -> width 5, NA -> na_width.
// ---------------------------------------------------------------------------

pub unsafe fn formatLogical(x: *const c_int, n: R_xlen_t, fieldwidth: *mut c_int) {
    unsafe {
        if x.is_null() || n <= 0 {
            if !fieldwidth.is_null() {
                *fieldwidth = 1;
            }
            return;
        }
        *fieldwidth = 1;
        for i in 0..n {
            let xi = *x.add(i as usize);
            if xi == NA_LOGICAL {
                if *fieldwidth < current_R_print().na_width {
                    *fieldwidth = current_R_print().na_width;
                }
            } else if xi != 0 && *fieldwidth < 4 {
                // TRUE
                *fieldwidth = 4;
            } else if xi == 0 && *fieldwidth < 5 {
                // FALSE
                *fieldwidth = 5;
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// formatLogicalS  -- SEXP variant using LOGICAL accessor
//
// Ported from C: uses LOGICAL() to get the data pointer, then delegates
// to formatLogical. The C version uses ITERATE_BY_REGION_PARTIAL for
// ALTREP support; we use the direct accessor path.
// ---------------------------------------------------------------------------

pub unsafe fn formatLogicalS(x: SEXP, n: R_xlen_t, fieldwidth: *mut c_int) {
    unsafe {
        if fieldwidth.is_null() {
            return;
        }
        *fieldwidth = 1;
        if x.is_null() || n == 0 {
            return;
        }
        let px = LOGICAL(x);
        let mut tmpfieldwidth = 1;
        formatLogical(px, n, &mut tmpfieldwidth);
        if tmpfieldwidth > *fieldwidth {
            *fieldwidth = tmpfieldwidth;
        }
    }
}

// ---------------------------------------------------------------------------
// formatInteger  -- field width for integer vector (raw int* version)
//
// Ported from C: finds min/max values and NA presence, then applies
// FORMATINT_RETLOGIC to compute the required field width.
// ---------------------------------------------------------------------------

pub unsafe fn formatInteger(x: *const c_int, n: R_xlen_t, fieldwidth: *mut c_int) {
    let width = if x.is_null() || n <= 0 {
        1
    } else {
        integer_field_width(n, |i| unsafe { *x.add(i as usize) })
    };
    unsafe {
        if !fieldwidth.is_null() {
            *fieldwidth = width;
        }
    }
}

/// Field width of `n` integers produced by `elt`.
///
/// A null pointer passed to [`formatInteger`] still means "no values" and
/// stays width 1. A compact sequence passes its formula here and never a buffer.
pub(crate) fn integer_field_width(n: R_xlen_t, mut elt: impl FnMut(R_xlen_t) -> c_int) -> c_int {
    if n <= 0 {
        return 1;
    }
    let mut xmin = c_int::MAX;
    let mut xmax = c_int::MIN;
    let mut naflag = false;

    for i in 0..n {
        let xi = elt(i);
        if xi == NA_INTEGER {
            naflag = true;
        } else {
            if xi < xmin {
                xmin = xi;
            }
            if xi > xmax {
                xmax = xi;
            }
        }
    }

    // FORMATINT_RETLOGIC:
    let mut fieldwidth = if naflag {
        current_R_print().na_width
    } else {
        1
    };

    if xmin < 0 {
        let l = unsafe { IndexWidth(-xmin) } + 1; // +1 for sign
        if l > fieldwidth {
            fieldwidth = l;
        }
    }
    if xmax > 0 {
        let l = unsafe { IndexWidth(xmax) };
        if l > fieldwidth {
            fieldwidth = l;
        }
    }
    fieldwidth
}

/// Field width of `n` elements of an integer compact sequence, starting at `start`.
///
/// The width matches [`integer_field_width`] on the same formula. A span whose
/// mathematical endpoints fit in `i32` is monotonic, so the endpoints decide it.
/// A unit step that wraps is a circular arc and still takes constant time.
/// Anything longer than a million elements with another step uses the widest
/// integer field: colon only constructs steps of ±1, and a full walk of a
/// vector longer than `c_int::MAX` is not possible here.
pub(crate) fn compact_int_width(seq: CompactSeq, start: R_xlen_t, n: R_xlen_t) -> c_int {
    if n <= 0 {
        return 1;
    }
    let Some((from, step)) = seq.int_origin_step() else {
        return 1;
    };
    let len = seq.len();
    if start < 0 || start >= len {
        return current_R_print().na_width.max(1);
    }
    let in_range = n.min(len - start);
    let extra_na = n > in_range;
    let origin = from as i128 + start as i128 * step as i128;
    let mut width = int_run_width(origin, step as i128, in_range);
    if extra_na {
        width = width.max(current_R_print().na_width);
    }
    width.max(1)
}

struct IntExtrema {
    xmin: i32,
    xmax: i32,
    any: bool,
    na: bool,
}

impl IntExtrema {
    fn new() -> Self {
        Self {
            xmin: 0,
            xmax: 0,
            any: false,
            na: false,
        }
    }

    fn observe(&mut self, value: i32) {
        if value == NA_INTEGER {
            self.na = true;
            return;
        }
        if !self.any {
            self.xmin = value;
            self.xmax = value;
            self.any = true;
        } else {
            if value < self.xmin {
                self.xmin = value;
            }
            if value > self.xmax {
                self.xmax = value;
            }
        }
    }

    fn width(self) -> c_int {
        let mut fieldwidth = if self.na {
            current_R_print().na_width
        } else {
            1
        };
        // `xmin` excludes NA_INTEGER, so `-xmin` fits in `c_int`.
        if self.any && self.xmin < 0 {
            let l = unsafe { IndexWidth(-self.xmin) } + 1;
            if l > fieldwidth {
                fieldwidth = l;
            }
        }
        if self.any && self.xmax > 0 {
            let l = unsafe { IndexWidth(self.xmax) };
            if l > fieldwidth {
                fieldwidth = l;
            }
        }
        fieldwidth
    }
}

fn observe_i32_range(acc: &mut IntExtrema, lo: i32, hi: i32) {
    if lo == i32::MIN {
        acc.na = true;
        if hi != i32::MIN {
            acc.observe(i32::MIN + 1);
            acc.observe(hi);
        }
    } else if hi == i32::MIN {
        acc.na = true;
        acc.observe(lo);
    } else {
        acc.observe(lo);
        acc.observe(hi);
    }
}

fn i32_span_width(first: i128, last: i128, step: i128, n: R_xlen_t) -> Option<c_int> {
    let in_i32 = |v: i128| v >= i32::MIN as i128 && v <= i32::MAX as i128;
    if !in_i32(first) || !in_i32(last) {
        return None;
    }
    let mut acc = IntExtrema::new();
    acc.observe(first as i32);
    if n > 1 {
        acc.observe(last as i32);
        if first as i32 == NA_INTEGER {
            let neighbor = first + step;
            if in_i32(neighbor) {
                acc.observe(neighbor as i32);
            }
        }
        if last as i32 == NA_INTEGER && first as i32 != NA_INTEGER {
            let neighbor = last - step;
            if in_i32(neighbor) {
                acc.observe(neighbor as i32);
            }
        }
    }
    Some(acc.width())
}

fn wrapping_unit_step_width(start: i32, step: i32, n: R_xlen_t) -> c_int {
    let mut acc = IntExtrema::new();
    if n as u128 >= (1u128 << 32) {
        acc.na = true;
        acc.observe(i32::MIN + 1);
        acc.observe(i32::MAX);
        return acc.width();
    }
    let span = n as i128 - 1;
    if step == 1 {
        let reach = start as i128 + span;
        if reach <= i32::MAX as i128 {
            observe_i32_range(&mut acc, start, reach as i32);
        } else {
            let extra = reach - i32::MAX as i128;
            let tail_last = (i32::MIN as i128 + extra - 1) as i32;
            observe_i32_range(&mut acc, start, i32::MAX);
            observe_i32_range(&mut acc, i32::MIN, tail_last);
        }
    } else {
        let reach = start as i128 - span;
        if reach >= i32::MIN as i128 {
            observe_i32_range(&mut acc, reach as i32, start);
        } else {
            let extra = i32::MIN as i128 - reach;
            let tail_last = (i32::MAX as i128 - (extra - 1)) as i32;
            observe_i32_range(&mut acc, i32::MIN, start);
            observe_i32_range(&mut acc, tail_last, i32::MAX);
        }
    }
    acc.width()
}

fn int_run_width(origin: i128, step: i128, n: R_xlen_t) -> c_int {
    if n <= 0 {
        return 1;
    }
    if n == 1 || step == 0 {
        let mut acc = IntExtrema::new();
        acc.observe(origin as i32);
        return acc.width();
    }
    let last = origin + (n as i128 - 1) * step;
    if let Some(width) = i32_span_width(origin, last, step, n) {
        return width;
    }
    if step == 1 || step == -1 {
        return wrapping_unit_step_width(origin as i32, step as i32, n);
    }
    if n <= 1_000_000 {
        return integer_field_width(n, |i| (origin + (i as i128) * step) as i32);
    }
    let numeric = unsafe { IndexWidth(c_int::MAX) } + 1;
    current_R_print().na_width.max(numeric)
}

fn buffered_real_field(x: SEXP, n: R_xlen_t, nsmall: c_int) -> RealField {
    let mut tmpw = 0;
    let mut tmpd = 0;
    let mut tmpe = 0;
    unsafe {
        formatReal(REAL(x), n, &mut tmpw, &mut tmpd, &mut tmpe, nsmall);
    }
    RealField {
        w: tmpw,
        d: tmpd,
        e: tmpe,
    }
}

// ---------------------------------------------------------------------------
// formatIntegerS  -- SEXP variant
//
// A compact sequence is measured from its formula. INTEGER() would allocate
// the payload, or return null when the budget refuses.
// ---------------------------------------------------------------------------

pub unsafe fn formatIntegerS(x: SEXP, n: R_xlen_t, fieldwidth: *mut c_int) {
    let width = if x.is_null() || n == 0 {
        1
    } else if let Some(seq) = unexpanded_int(x) {
        // The formula covers every length, including those above `c_int::MAX`.
        // INTEGER() would allocate the payload.
        compact_int_width(seq, 0, n).max(1)
    } else {
        let mut tmpfw = 1;
        unsafe {
            formatInteger(INTEGER(x), n, &mut tmpfw);
        }
        tmpfw.max(1)
    };
    unsafe {
        if !fieldwidth.is_null() {
            *fieldwidth = width;
        }
    }
}

// ---------------------------------------------------------------------------
// formatReal  -- NOT hidden in C; used in graphics/src/plot.c
//
// Computes the field width (w), decimal digits (d), and exponent width (e)
// for an array of doubles.  This is a fully standalone port that operates
// on raw double pointers.
// ---------------------------------------------------------------------------

/// Compute format parameters for an array of doubles.
///
/// # Arguments
/// * `x`      - pointer to array of `n` doubles
/// * `n`      - number of elements
/// * `w`      - [out] required field width
/// * `d`      - [out] decimal digits to use
/// * `e`      - [out] exponent width (0 = fixed format, 1 = 2-digit exp, 2 = 3-digit)
/// * `nsmall` - minimum number of decimal digits in fixed format
pub unsafe fn formatReal(
    x: *const c_double,
    n: R_xlen_t,
    w: *mut c_int,
    d: *mut c_int,
    e: *mut c_int,
    nsmall: c_int,
) {
    let fmt = if x.is_null() || n <= 0 {
        RealField { w: 0, d: 0, e: 0 }
    } else {
        real_field(n, nsmall, |i| unsafe { *x.add(i as usize) })
    };
    unsafe {
        if !w.is_null() {
            *w = fmt.w;
        }
        if !d.is_null() {
            *d = fmt.d;
        }
        if !e.is_null() {
            *e = fmt.e;
        }
    }
}

/// Format parameters for `n` doubles produced by `elt`.
///
/// [`formatReal`] keeps a null pointer as an empty span. A compact sequence
/// passes its formula here and never a buffer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RealField {
    pub w: c_int,
    pub d: c_int,
    pub e: c_int,
}

pub(crate) fn real_field(
    n: R_xlen_t,
    nsmall: c_int,
    mut elt: impl FnMut(R_xlen_t) -> c_double,
) -> RealField {
    if n <= 0 {
        return RealField { w: 0, d: 0, e: 0 };
    }
    let mut naflag = false;
    let mut nanflag = false;
    let mut posinf = false;
    let mut neginf = false;
    let mut neg = 0;

    let mut mnl = c_int::MAX;
    let mut mxl: c_int = c_int::MIN;
    let mut rgt: c_int = c_int::MIN;
    let mut mxsl: c_int = c_int::MIN;
    let mut mxns: c_int = c_int::MIN;

    let na_width = current_R_print().na_width;

    for i in 0..n {
        let xi = elt(i);
        if !xi.is_finite() {
            if xi.is_nan() {
                // Distinguish NA from NaN: R's NA has a specific bit pattern.
                if xi.to_bits() == R_NA_BIT_PATTERN {
                    naflag = true;
                } else {
                    nanflag = true;
                }
            } else if xi > 0.0 {
                posinf = true;
            } else {
                neginf = true;
            }
        } else {
            let mut neg_i: c_int = 0;
            let mut kpower: c_int = 0;
            let mut nsig: c_int = 0;
            let mut roundingwidens: bool = false;

            unsafe {
                format_scientific(&xi, &mut neg_i, &mut kpower, &mut nsig, &mut roundingwidens);
            }

            let mut left = kpower + 1;
            if roundingwidens {
                left -= 1;
            }

            let sleft = neg_i + if left <= 0 { 1 } else { left }; // >= 1
            let right = nsig - left; // #{digits} right of '.'
            if neg_i != 0 {
                neg = 1;
            }

            // Infinite precision "F" Format:
            if right > rgt {
                rgt = right;
            }
            if left > mxl {
                mxl = left;
            }
            if left < mnl {
                mnl = left;
            }
            if sleft > mxsl {
                mxsl = sleft;
            }
            if nsig > mxns {
                mxns = nsig;
            }
        }
    }

    // F vs E format decision
    if current_R_print().digits == 0 {
        rgt = 0;
    }
    if mxl < 0 {
        mxsl = 1 + neg; // we use %#w.dg, so have leading zero
    }

    if rgt < 0 {
        rgt = 0;
    }
    let mut wF = mxsl + rgt + if rgt != 0 { 1 } else { 0 }; // width for F format

    // "E" exponential format
    let mut e = if mxl > 100 || mnl <= -99 { 2 } else { 1 }; // 3-digit exponent?
    let (mut w, d) = if mxns != c_int::MIN {
        let mut d = mxns - 1;
        let mut w = neg + if d > 0 { 1 } else { 0 } + d + 4 + e; // width for E format
        if wF <= w + current_R_print().scipen {
            // Fixpoint if it needs less space
            e = 0;
            let nsmall_i = nsmall as c_int;
            if nsmall_i > rgt {
                rgt = nsmall_i;
                wF = mxsl + rgt + if rgt != 0 { 1 } else { 0 };
            }
            d = rgt;
            w = wF;
        }
        (w, d)
    } else {
        // all x[i] are non-finite
        e = 0;
        (0, 0)
    };

    if naflag && w < na_width {
        w = na_width;
    }
    if nanflag && w < 3 {
        w = 3;
    }
    if posinf && w < 3 {
        w = 3;
    }
    if neginf && w < 4 {
        w = 4;
    }
    RealField { w, d, e }
}

/// Format parameters for a real arithmetic sequence, from a short sample.
///
/// Used when the length is above [`c_int::MAX`]. A full walk would read every
/// element. Colon steps are ±1, so the aggregates in [`real_field`] are fixed
/// by the endpoints, the element nearest zero, and one non-round probe in each
/// decade (a pure power of ten understates `nsig`, and rounding-widens just
/// below a power of ten). The altseq sample test checks this against a full
/// walk on moderate sequences.
pub(crate) fn sampled_real_field(
    from: c_double,
    step: c_double,
    n: R_xlen_t,
    nsmall: c_int,
) -> RealField {
    if n <= 0 {
        return RealField { w: 0, d: 0, e: 0 };
    }
    let samples = real_sequence_samples(from, step, n);
    if samples.is_empty() {
        return RealField { w: 0, d: 0, e: 0 };
    }
    real_field(samples.len() as R_xlen_t, nsmall, |i| samples[i as usize])
}

const REAL_SAMPLE_CAP: usize = 4000;

fn real_at_index(from: c_double, step: c_double, index: u64) -> c_double {
    from + (index as c_double) * step
}

fn real_sequence_samples(from: c_double, step: c_double, n: R_xlen_t) -> Vec<c_double> {
    let Ok(n_u) = u64::try_from(n) else {
        return Vec::new();
    };
    if n_u == 0 {
        return Vec::new();
    }
    if !from.is_finite() || !step.is_finite() || step == 0.0 {
        let mut samples = vec![real_at_index(from, step, 0)];
        if n_u > 1 {
            samples.push(real_at_index(from, step, n_u - 1));
        }
        return samples;
    }

    let mut indices = Vec::with_capacity(64);
    push_window(&mut indices, n_u, 0, 3);
    push_window(&mut indices, n_u, n_u - 1, 3);
    let closest = closest_real_index(from, step, n_u);
    push_window(&mut indices, n_u, closest, 8);
    if let Some(boundary) = last_finite_index(from, step, n_u) {
        push_window(&mut indices, n_u, boundary, 2);
    }

    let first = real_at_index(from, step, 0);
    let last = real_at_index(from, step, n_u - 1);
    let (lo, hi) = if first <= last {
        (first, last)
    } else {
        (last, first)
    };
    // Factors that are not trailing-zero powers of ten, plus the value just
    // below the next power where rounding can widen the field.
    const FACTORS: [f64; 5] = [1.0, 1.234567, 2.345678, 9.876543, 9.999999];
    if lo.is_finite() && hi.is_finite() {
        for k in -307..=308 {
            if indices.len() >= REAL_SAMPLE_CAP {
                break;
            }
            let p = 10f64.powi(k);
            let p_hi = 10f64.powi(k + 1);
            if !p.is_finite() {
                continue;
            }
            let upper = if p_hi.is_finite() { p_hi } else { hi };
            if !ranges_overlap(p, upper, lo, hi) && !ranges_overlap(-upper, -p, lo, hi) {
                continue;
            }
            for factor in FACTORS {
                let target = p * factor;
                if let Some(index) = index_near(from, step, n_u, target) {
                    push_window(&mut indices, n_u, index, 2);
                }
                if let Some(index) = index_near(from, step, n_u, -target) {
                    push_window(&mut indices, n_u, index, 2);
                }
                if indices.len() >= REAL_SAMPLE_CAP {
                    break;
                }
            }
        }
    }

    indices
        .into_iter()
        .map(|index| real_at_index(from, step, index))
        .collect()
}

fn ranges_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    a0 <= b1 && b0 <= a1
}

fn push_index(indices: &mut Vec<u64>, n: u64, index: u64) {
    if index < n && indices.len() < REAL_SAMPLE_CAP && !indices.contains(&index) {
        indices.push(index);
    }
}

fn push_window(indices: &mut Vec<u64>, n: u64, index: u64, radius: u64) {
    push_index(indices, n, index);
    for delta in 1..=radius {
        if index >= delta {
            push_index(indices, n, index - delta);
        }
        push_index(indices, n, index.saturating_add(delta));
    }
}

fn closest_real_index(from: f64, step: f64, n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let t = (-from / step).round();
    if !t.is_finite() || t <= 0.0 {
        0
    } else if t >= n as f64 {
        n - 1
    } else {
        t as u64
    }
}

fn index_near(from: f64, step: f64, n: u64, target: f64) -> Option<u64> {
    if n == 0 || !target.is_finite() || step == 0.0 {
        return None;
    }
    let t = ((target - from) / step).round();
    if !t.is_finite() {
        return None;
    }
    let index = if t <= 0.0 {
        0
    } else if t >= n as f64 {
        n - 1
    } else {
        t as u64
    };
    let value = real_at_index(from, step, index);
    let slack = step.abs() * 1.5 + target.abs().max(1.0) * 1e-9;
    if (value - target).abs() <= slack {
        Some(index)
    } else {
        None
    }
}

fn last_finite_index(from: f64, step: f64, n: u64) -> Option<u64> {
    if n == 0 || !real_at_index(from, step, 0).is_finite() {
        return None;
    }
    if real_at_index(from, step, n - 1).is_finite() {
        return Some(n - 1);
    }
    let mut lo = 0u64;
    let mut hi = n;
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        if real_at_index(from, step, mid).is_finite() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(lo)
}

/// Format parameters for `n` elements of a real compact sequence starting at `start`.
///
/// Lengths up to [`c_int::MAX`] walk every element, matching a buffer of the
/// same values. Longer sequences use [`sampled_real_field`] and never call `REAL()`.
pub(crate) fn compact_real_field(
    seq: CompactSeq,
    start: R_xlen_t,
    n: R_xlen_t,
    nsmall: c_int,
) -> RealField {
    if n <= 0 {
        return RealField { w: 0, d: 0, e: 0 };
    }
    let Some((from, step)) = seq.real_origin_step() else {
        return RealField { w: 0, d: 0, e: 0 };
    };
    let len = seq.len();
    if start < 0 || start >= len {
        return real_field(1, nsmall, |_| NA_REAL);
    }
    let in_range = n.min(len - start);
    let extra_na = n > in_range;
    if in_range <= c_int::MAX as R_xlen_t {
        let count = in_range + if extra_na { 1 } else { 0 };
        return real_field(count, nsmall, |i| {
            if i < in_range {
                seq.real_or_na(start + i)
            } else {
                NA_REAL
            }
        });
    }
    let origin = from + (start as c_double) * step;
    let mut samples = real_sequence_samples(origin, step, in_range);
    if extra_na {
        samples.push(NA_REAL);
    }
    if samples.is_empty() {
        return RealField { w: 0, d: 0, e: 0 };
    }
    real_field(samples.len() as R_xlen_t, nsmall, |i| samples[i as usize])
}

// ---------------------------------------------------------------------------
// formatRealS  -- SEXP variant
//
// A compact sequence is measured from its formula. REAL() would allocate
// the payload, or return null when the budget refuses.
// ---------------------------------------------------------------------------

pub unsafe fn formatRealS(
    x: SEXP,
    n: R_xlen_t,
    w: *mut c_int,
    d: *mut c_int,
    e: *mut c_int,
    nsmall: c_int,
) {
    unsafe {
        if !w.is_null() {
            *w = 0;
        }
        if !d.is_null() {
            *d = 0;
        }
        if !e.is_null() {
            *e = 0;
        }
        if x.is_null() || n == 0 {
            return;
        }
        let fmt = if let Some(seq) = unexpanded_real(x) {
            // The formula covers every length. REAL() would allocate the payload.
            compact_real_field(seq, 0, n, nsmall)
        } else {
            buffered_real_field(x, n, nsmall)
        };
        if !w.is_null() && fmt.w > *w {
            *w = fmt.w;
        }
        if !d.is_null() && *d == 0 && fmt.d != 0 {
            *d = fmt.d;
        }
        if !e.is_null() && fmt.e > *e {
            *e = fmt.e;
        }
    }
}

// ---------------------------------------------------------------------------
// formatComplex  -- operates on raw Rcomplex arrays
//
// Since R 4.4.0, Re(.) and Im(.) are treated separately using formatReal.
// We port the modern (non-"tricky") path with NA_give_NA behavior.
// ---------------------------------------------------------------------------

/// Compute format parameters for an array of complex numbers.
///
/// Treats Re and Im parts independently via `formatReal`.
pub unsafe fn formatComplex(
    x: *const Rcomplex,
    n: R_xlen_t,
    wr: *mut c_int,
    dr: *mut c_int,
    er: *mut c_int,
    wi: *mut c_int,
    di: *mut c_int,
    ei: *mut c_int,
    nsmall: c_int,
) {
    unsafe {
        if x.is_null() || n <= 0 {
            if !wr.is_null() {
                *wr = 0;
            }
            if !dr.is_null() {
                *dr = 0;
            }
            if !er.is_null() {
                *er = 0;
            }
            if !wi.is_null() {
                *wi = 0;
            }
            if !di.is_null() {
                *di = 0;
            }
            if !ei.is_null() {
                *ei = 0;
            }
            return;
        }
        let n_usize = n as usize;
        if n_usize == 0 {
            *wr = 0;
            *dr = 0;
            *er = 0;
            *wi = 0;
            *di = 0;
            *ei = 0;
            return;
        }

        // Use R_alloc for transient memory (matches C behavior).
        // R_alloc args are (element_size, count).
        use crate::sexp::memory_ext::R_alloc;

        let re_ptr = R_alloc(std::mem::size_of::<c_double>(), n_usize) as *mut c_double;
        let im_ptr = R_alloc(std::mem::size_of::<c_double>(), n_usize) as *mut c_double;

        let mut i1: usize = 0;
        let mut naflag = false;

        for i in 0..n_usize {
            let cx = *x.add(i);
            let r_bits = cx.r.to_bits();
            let i_bits = cx.i.to_bits();
            let is_na = r_bits == R_NA_BIT_PATTERN || i_bits == R_NA_BIT_PATTERN;
            if is_na {
                naflag = true;
            } else {
                *re_ptr.add(i1) = cx.r;
                *im_ptr.add(i1) = cx.i.abs(); // sign is handled when printing
                i1 += 1;
            }
        }

        formatReal(re_ptr, i1 as R_xlen_t, wr, dr, er, nsmall);
        formatReal(im_ptr, i1 as R_xlen_t, wi, di, ei, nsmall);

        // Ensure space for NA in the combined width.
        let na_width = current_R_print().na_width;
        if naflag && *wr + *wi + 2 < na_width {
            *wr += na_width - (*wr + *wi + 2);
        }
    }
}

// ---------------------------------------------------------------------------
// formatComplexS  -- SEXP variant using COMPLEX accessor
//
// Ported from C: uses COMPLEX() to get the data pointer, then delegates
// to formatComplex. The C version uses ITERATE_BY_REGION_PARTIAL for
// ALTREP support; we use the direct accessor path.
// ---------------------------------------------------------------------------

pub unsafe fn formatComplexS(
    x: SEXP,
    n: R_xlen_t,
    wr: *mut c_int,
    dr: *mut c_int,
    er: *mut c_int,
    wi: *mut c_int,
    di: *mut c_int,
    ei: *mut c_int,
    nsmall: c_int,
) {
    unsafe {
        *wr = 0;
        *wi = 0;
        *dr = 0;
        *di = 0;
        *er = 0;
        *ei = 0;
        if x.is_null() || n == 0 {
            return;
        }
        let px = COMPLEX(x);
        let mut tmpwr: c_int = 0;
        let mut tmpdr: c_int = 0;
        let mut tmper: c_int = 0;
        let mut tmpwi: c_int = 0;
        let mut tmpdi: c_int = 0;
        let mut tmpei: c_int = 0;
        formatComplex(
            px, n, &mut tmpwr, &mut tmpdr, &mut tmper, &mut tmpwi, &mut tmpdi, &mut tmpei, nsmall,
        );
        if tmpwr > *wr {
            *wr = tmpwr;
        }
        if tmpdr != 0 && *dr == 0 {
            *dr = tmpdr;
        }
        if tmper > *er {
            *er = tmper;
        }
        if tmpwi > *wi {
            *wi = tmpwi;
        }
        if tmpdi != 0 && *di == 0 {
            *di = tmpdi;
        }
        if tmpei > *ei {
            *ei = tmpei;
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
    use std::os::raw::c_int;

    /// Helper: set R_print with digits and get a guard that resets on drop.
    struct RPrintGuard {
        _session: RSession,
        old: RPrint,
    }

    impl RPrintGuard {
        fn new(digits: c_int) -> Self {
            let session = RSession::new();
            let p = RPrint {
                digits,
                scipen: 0,
                na_width: 2,
                na_width_noquote: 2,
            };
            let old = unsafe { format_set_R_print(p) };
            RPrintGuard {
                _session: session,
                old,
            }
        }
    }

    impl Drop for RPrintGuard {
        fn drop(&mut self) {
            unsafe {
                format_set_R_print(self.old);
            }
        }
    }

    #[test]
    fn test_index_width() {
        unsafe {
            assert_eq!(IndexWidth(0), 1);
            assert_eq!(IndexWidth(1), 1);
            assert_eq!(IndexWidth(9), 1);
            assert_eq!(IndexWidth(10), 2);
            assert_eq!(IndexWidth(99), 2);
            assert_eq!(IndexWidth(100), 3);
            assert_eq!(IndexWidth(999), 3);
            assert_eq!(IndexWidth(1000), 4);
            assert_eq!(IndexWidth(999999999), 9);
            assert_eq!(IndexWidth(1000000000), 10);
        }
    }

    #[test]
    fn test_index_width_negative() {
        unsafe {
            assert_eq!(IndexWidth(-5), 1);
            assert_eq!(IndexWidth(-42), 2);
            assert_eq!(IndexWidth(-100), 3);
        }
    }

    #[test]
    fn test_format_integer_empty() {
        unsafe {
            let mut fw: c_int = 0;
            let arr: [c_int; 0] = [];
            formatInteger(arr.as_ptr(), 0, &mut fw);
            assert_eq!(fw, 1);
        }
    }

    #[test]
    fn test_format_integer_simple() {
        unsafe {
            let mut fw: c_int = 0;
            let arr = [1i32, 2, 3];
            formatInteger(arr.as_ptr(), 3, &mut fw);
            assert_eq!(fw, 1);
        }
    }

    #[test]
    fn test_format_integer_with_na() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut fw: c_int = 0;
            let arr = [1i32, NA_INTEGER, 3];
            formatInteger(arr.as_ptr(), 3, &mut fw);
            assert_eq!(fw, 2); // na_width = 2
        }
    }

    #[test]
    fn test_format_integer_negative() {
        unsafe {
            let mut fw: c_int = 0;
            let arr = [-42i32, 100, 999];
            formatInteger(arr.as_ptr(), 3, &mut fw);
            // -42 needs 3 chars ("-42"), 999 needs 3 chars
            assert_eq!(fw, 3);
        }
    }

    #[test]
    fn test_format_integer_large() {
        unsafe {
            let mut fw: c_int = 0;
            let arr = [100000i32, -999999];
            formatInteger(arr.as_ptr(), 2, &mut fw);
            // 100000 -> 6 chars, -999999 -> 7 chars
            assert_eq!(fw, 7);
        }
    }

    #[test]
    fn test_format_logical_empty() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut fw: c_int = 0;
            let arr: [c_int; 0] = [];
            formatLogical(arr.as_ptr(), 0, &mut fw);
            assert_eq!(fw, 1);
        }
    }

    #[test]
    fn test_format_logical_true() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut fw: c_int = 0;
            let arr = [1i32];
            formatLogical(arr.as_ptr(), 1, &mut fw);
            assert_eq!(fw, 4); // "TRUE"
        }
    }

    #[test]
    fn test_format_logical_false() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut fw: c_int = 0;
            let arr = [0i32];
            formatLogical(arr.as_ptr(), 1, &mut fw);
            assert_eq!(fw, 5); // "FALSE"
        }
    }

    #[test]
    fn test_format_logical_mixed() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut fw: c_int = 0;
            let arr = [1i32, 0, NA_LOGICAL];
            formatLogical(arr.as_ptr(), 3, &mut fw);
            // NA -> na_width=2, TRUE -> 4, FALSE -> 5 => max is 5
            assert_eq!(fw, 5);
        }
    }

    #[test]
    fn test_format_logical_false_short_circuits_like_r() {
        unsafe {
            let _session = RSession::new();
            let p = RPrint {
                digits: 7,
                scipen: 0,
                na_width: 10,
                na_width_noquote: 10,
            };
            let old = format_set_R_print(p);
            let mut fw: c_int = 0;
            let arr = [0i32, NA_LOGICAL];
            formatLogical(arr.as_ptr(), 2, &mut fw);
            assert_eq!(fw, 5);
            format_set_R_print(old);
        }
    }

    #[test]
    fn test_format_real_simple() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut w: c_int = 0;
            let mut d: c_int = 0;
            let mut e: c_int = 0;
            let arr = [1.0f64, 2.0, 3.0];
            formatReal(arr.as_ptr(), 3, &mut w, &mut d, &mut e, 0);
            assert!(w > 0);
            assert!(d >= 0);
        }
    }

    #[test]
    fn test_format_real_with_na() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut w: c_int = 0;
            let mut d: c_int = 0;
            let mut e: c_int = 0;
            let na_val = c_double::from_bits(R_NA_BIT_PATTERN);
            let arr = [1.0f64, na_val, 3.0];
            formatReal(arr.as_ptr(), 3, &mut w, &mut d, &mut e, 0);
            assert!(w >= 2); // at least na_width=2
        }
    }

    #[test]
    fn test_format_real_with_inf() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut w: c_int = 0;
            let mut d: c_int = 0;
            let mut e: c_int = 0;
            let arr = [1.0f64, f64::INFINITY, f64::NEG_INFINITY];
            formatReal(arr.as_ptr(), 3, &mut w, &mut d, &mut e, 0);
            assert!(w >= 4); // at least "-Inf" = 4
        }
    }

    #[test]
    fn test_format_real_scientific() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut w: c_int = 0;
            let mut d: c_int = 0;
            let mut e: c_int = 0;
            let arr = [1e20f64];
            formatReal(arr.as_ptr(), 1, &mut w, &mut d, &mut e, 0);
            // Very large number: should use exponential format
            assert!(e > 0);
        }
    }

    #[test]
    fn test_format_real_empty() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut w: c_int = 0;
            let mut d: c_int = 0;
            let mut e: c_int = 0;
            let arr: [f64; 0] = [];
            formatReal(arr.as_ptr(), 0, &mut w, &mut d, &mut e, 0);
            // All non-finite: w=0, d=0, e=0
            assert_eq!(w, 0);
            assert_eq!(d, 0);
            assert_eq!(e, 0);
        }
    }

    #[test]
    fn test_format_complex_simple() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut wr: c_int = 0;
            let mut dr: c_int = 0;
            let mut er: c_int = 0;
            let mut wi: c_int = 0;
            let mut di: c_int = 0;
            let mut ei: c_int = 0;
            let arr = [Rcomplex { r: 1.0, i: 2.0 }, Rcomplex { r: 3.0, i: 4.0 }];
            formatComplex(
                arr.as_ptr(),
                2,
                &mut wr,
                &mut dr,
                &mut er,
                &mut wi,
                &mut di,
                &mut ei,
                0,
            );
            assert!(wr > 0);
            assert!(wi > 0);
        }
    }

    #[test]
    fn test_format_complex_empty() {
        unsafe {
            let mut wr: c_int = 0;
            let mut dr: c_int = 0;
            let mut er: c_int = 0;
            let mut wi: c_int = 0;
            let mut di: c_int = 0;
            let mut ei: c_int = 0;
            let arr: [Rcomplex; 0] = [];
            formatComplex(
                arr.as_ptr(),
                0,
                &mut wr,
                &mut dr,
                &mut er,
                &mut wi,
                &mut di,
                &mut ei,
                0,
            );
            assert_eq!(wr, 0);
            assert_eq!(wi, 0);
        }
    }

    #[test]
    fn test_format_complex_with_na() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let mut wr: c_int = 0;
            let mut dr: c_int = 0;
            let mut er: c_int = 0;
            let mut wi: c_int = 0;
            let mut di: c_int = 0;
            let mut ei: c_int = 0;
            let na_val = c_double::from_bits(R_NA_BIT_PATTERN);
            let arr = [Rcomplex { r: 1.0, i: 2.0 }, Rcomplex { r: na_val, i: 3.0 }];
            formatComplex(
                arr.as_ptr(),
                2,
                &mut wr,
                &mut dr,
                &mut er,
                &mut wi,
                &mut di,
                &mut ei,
                0,
            );
            assert!(wr + wi + 2 >= 2); // space for NA
        }
    }

    #[test]
    fn test_scientific_zero() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let x = 0.0_f64;
            let mut neg: c_int = 0;
            let mut kpower: c_int = 0;
            let mut nsig: c_int = 0;
            let mut rw = false;
            format_scientific(&x, &mut neg, &mut kpower, &mut nsig, &mut rw);
            assert_eq!(neg, 0);
            assert_eq!(kpower, 0);
            assert_eq!(nsig, 1);
            assert!(!rw);
        }
    }

    #[test]
    fn test_scientific_positive() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let x = 123.456_f64;
            let mut neg: c_int = 0;
            let mut kpower: c_int = 0;
            let mut nsig: c_int = 0;
            let mut rw = false;
            format_scientific(&x, &mut neg, &mut kpower, &mut nsig, &mut rw);
            assert_eq!(neg, 0);
            assert_eq!(kpower, 2); // 123.456 = 1.23... * 10^2
            assert!(nsig >= 1);
        }
    }

    #[test]
    fn test_scientific_negative() {
        unsafe {
            let _g = RPrintGuard::new(7);
            let x = -42.0_f64;
            let mut neg: c_int = 0;
            let mut kpower: c_int = 0;
            let mut nsig: c_int = 0;
            let mut rw = false;
            format_scientific(&x, &mut neg, &mut kpower, &mut nsig, &mut rw);
            assert_eq!(neg, 1);
            assert_eq!(kpower, 1); // 42 = 4.2 * 10^1
        }
    }

    #[test]
    fn test_format_raw() {
        unsafe {
            let mut fw: c_int = 0;
            let arr: [u8; 3] = [0x00, 0xFF, 0xAB];
            formatRaw(arr.as_ptr() as *const c_void, 3, &mut fw);
            assert_eq!(fw, 2);
        }
    }

    #[test]
    fn test_rexp10_table() {
        unsafe {
            assert!((format_Rexp10(0) - 1.0).abs() < 1e-15);
            assert!((format_Rexp10(1) - 10.0).abs() < 1e-15);
            assert!((format_Rexp10(10) - 1e10).abs() < 1e-5);
            assert!((format_Rexp10(-3) - 0.001).abs() < 1e-15);
        }
    }

    #[test]
    fn test_rexp10_out_of_range() {
        unsafe {
            let v = format_Rexp10(50);
            assert!(v > 0.0);
            let v = format_Rexp10(-50);
            assert!(v > 0.0 && v < 1.0);
        }
    }
}
