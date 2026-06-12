#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/format.c
//!
//! Object Formatting -- determines proper width, digits, etc. for printing
//! R objects.
//!
//! Exports (from original C):
//!   formatString, formatStringS, formatLogical, formatLogicalS,
//!   formatInteger, formatIntegerS, formatReal, formatRealS,
//!   formatComplex, formatComplexS, formatRaw, formatRawS

use std::os::raw::{c_double, c_int, c_void};

use crate::sexp::accessors::{COMPLEX, INTEGER, LOGICAL, REAL, STRING_ELT};
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXP};

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
    crate::sexp::instance::with_current_instance(|inst| inst.eval_state.format_print)
        .unwrap_or_default()
}

pub unsafe fn format_set_R_print(p: RPrint) -> RPrint {
    crate::sexp::instance::with_required_current_instance(|inst| {
        let old = inst.eval_state.format_print;
        inst.eval_state.format_print = p;
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
        let mut buff = [0 as libc::c_char; NB];
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
        while i >= 2 && buff[i as usize] == b'0' as libc::c_char {
            i -= 1;
        }
        *nsig = i;
    }
}

/// Minimal snprintf shim for our format_via_sprintf.
/// Writes into `buf` (up to `buf_size` bytes including NUL) using the
/// C format string `fmt`.
fn snprintf(
    buf: &mut [libc::c_char],
    buf_size: usize,
    fmt: *const libc::c_char,
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
            .map(|&b| b as libc::c_char)
            .collect::<Vec<libc::c_char>>()[..copy_len],
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

// ---------------------------------------------------------------------------
// formatString  -- field width for character strings
//
// Ported from C: iterates SEXP array, calls Rstrlen for display width.
// ---------------------------------------------------------------------------

pub unsafe fn formatString(x: *const SEXP, n: R_xlen_t, fieldwidth: *mut c_int, quote: c_int) {
    unsafe {
        let mut xmax: c_int = 0;

        for i in 0..n {
            let si = *x.add(i as usize);
            let l;
            if si.is_null() {
                // NA_STRING
                l = if quote != 0 {
                    current_R_print().na_width
                } else {
                    current_R_print().na_width_noquote
                };
            } else {
                l = Rstrlen(si, quote) + if quote != 0 { 2 } else { 0 };
            }
            if l > xmax {
                xmax = l;
            }
        }
        *fieldwidth = xmax;
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
            let l;
            if si.is_null() {
                // NA_STRING
                l = if quote != 0 {
                    current_R_print().na_width
                } else {
                    current_R_print().na_width_noquote
                };
            } else {
                l = Rstrlen(si, quote) + if quote != 0 { 2 } else { 0 };
            }
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
    unsafe {
        if x.is_null() || n <= 0 {
            if !fieldwidth.is_null() {
                *fieldwidth = 1;
            }
            return;
        }
        let mut xmin = c_int::MAX;
        let mut xmax = c_int::MIN;
        let mut naflag = false;

        for i in 0..n {
            let xi = *x.add(i as usize);
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
        if naflag {
            *fieldwidth = current_R_print().na_width;
        } else {
            *fieldwidth = 1;
        }

        if xmin < 0 {
            let l = IndexWidth(-xmin) + 1; // +1 for sign
            if l > *fieldwidth {
                *fieldwidth = l;
            }
        }
        if xmax > 0 {
            let l = IndexWidth(xmax);
            if l > *fieldwidth {
                *fieldwidth = l;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// formatIntegerS  -- SEXP variant using INTEGER accessor
//
// Ported from C: the C version uses ALTREP fast-paths (INTEGER_IS_SORTED,
// ALTINTEGER_MIN/MAX). We use the simpler direct path via INTEGER().
// ---------------------------------------------------------------------------

pub unsafe fn formatIntegerS(x: SEXP, n: R_xlen_t, fieldwidth: *mut c_int) {
    unsafe {
        *fieldwidth = 1;
        if x.is_null() || n == 0 {
            return;
        }
        let px = INTEGER(x);
        let mut tmpfw = 1;
        formatInteger(px, n, &mut tmpfw);
        if tmpfw > *fieldwidth {
            *fieldwidth = tmpfw;
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
    unsafe {
        if x.is_null() || n <= 0 {
            if !w.is_null() {
                *w = 0;
            }
            if !d.is_null() {
                *d = 0;
            }
            if !e.is_null() {
                *e = 0;
            }
            return;
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
            let xi = *x.add(i as usize);
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

                format_scientific(&xi, &mut neg_i, &mut kpower, &mut nsig, &mut roundingwidens);

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
        *e = if mxl > 100 || mnl <= -99 { 2 } else { 1 }; // 3-digit exponent?
        if mxns != c_int::MIN {
            *d = mxns - 1;
            *w = neg + if *d > 0 { 1 } else { 0 } + *d + 4 + *e; // width for E format
            if wF <= *w + current_R_print().scipen {
                // Fixpoint if it needs less space
                *e = 0;
                let nsmall_i = nsmall as c_int;
                if nsmall_i > rgt {
                    rgt = nsmall_i;
                    wF = mxsl + rgt + if rgt != 0 { 1 } else { 0 };
                }
                *d = rgt;
                *w = wF;
            }
        } else {
            // all x[i] are non-finite
            *w = 0;
            *d = 0;
            *e = 0;
        }

        if naflag && *w < na_width {
            *w = na_width;
        }
        if nanflag && *w < 3 {
            *w = 3;
        }
        if posinf && *w < 3 {
            *w = 3;
        }
        if neginf && *w < 4 {
            *w = 4;
        }
    }
}

// ---------------------------------------------------------------------------
// formatRealS  -- SEXP variant using REAL accessor
//
// Ported from C: uses REAL() to get the data pointer, then delegates
// to formatReal. The C version uses ITERATE_BY_REGION_PARTIAL for
// ALTREP support; we use the direct accessor path.
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
        let px = REAL(x);
        let mut tmpw: c_int = 0;
        let mut tmpd: c_int = 0;
        let mut tmpe: c_int = 0;
        formatReal(px, n, &mut tmpw, &mut tmpd, &mut tmpe, nsmall);
        if tmpw > *w {
            *w = tmpw;
        }
        if *d == 0 && tmpd != 0 {
            *d = tmpd;
        }
        if tmpe > *e {
            *e = tmpe;
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
