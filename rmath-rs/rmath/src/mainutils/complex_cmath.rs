#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/complex.c (lines 1-793) -- complex arithmetic and math functions.
//!
//! This module ports the complex number operations from R's complex.c, excluding
//! the cpolyroot functions (already in `polyroot.rs`).
//!
//! Ported standalone functions:
//!   R_cpow_n, mycpow, z_prec_r,
//!   clog, csqrt, cexp, ccos, csin, ctan, casin, cacos, catan, ccosh, csinh, ctanh,
//!   z_tan, z_asin, z_acos, z_atan, z_acosh, z_asinh, z_atanh,
//!   cmath1, z_rround, z_prec, z_logbase, z_atan2
//!
//! Ported FFI entry points:
//!   complex_unary, complex_binary, do_cmathfuns,
//!   complex_math1, complex_math2, do_complex

use std::os::raw::c_int;

use num::Complex;

use crate::fprec::fround;
use crate::sexp::accessors::{
    ATTRIB, CAR, CDR, COMPLEX, INTEGER, REAL, SET_ATTRIB, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::Rf_allocVector3;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_REAL, R_FINITE, R_xlen_t, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::special::mlutils::R_pow;

// ---------------------------------------------------------------------------
// Helper conversions between Rcomplex and num::Complex
// ---------------------------------------------------------------------------

/// Convert an Rcomplex to a num::Complex<f64>.
#[inline]
fn to_complex(x: &Rcomplex) -> Complex<f64> {
    Complex::new(x.r, x.i)
}

/// Convert a num::Complex<f64> to an Rcomplex.
#[inline]
fn from_complex(z: Complex<f64>) -> Rcomplex {
    Rcomplex { r: z.re, i: z.im }
}

// ---------------------------------------------------------------------------
// R_cpow_n -- complex power by integer (fast exponentiation by squaring)
// ---------------------------------------------------------------------------

/// Compute X^k for a complex number X and integer k, using exponentiation by squaring.
///
/// Ported from lines 100-115 of complex.c.
pub fn R_cpow_n(x: Complex<f64>, k: i32) -> Complex<f64> {
    if k == 0 {
        return Complex::new(1.0, 0.0);
    } else if k == 1 {
        return x;
    } else if k < 0 {
        return 1.0 / R_cpow_n(x, -k);
    } else {
        // k > 0
        let mut z = Complex::new(1.0, 0.0);
        let mut x_val = x;
        let mut k_val = k;
        while k_val > 0 {
            if (k_val & 1) != 0 {
                z *= x_val;
            }
            if k_val == 1 {
                break;
            }
            k_val >>= 1;
            x_val = x_val * x_val;
        }
        z
    }
}

// ---------------------------------------------------------------------------
// mycpow -- complex power with special cases
// ---------------------------------------------------------------------------

/// Complex power X^Y with special-case handling for integer exponents and zero base.
///
/// Ported from lines 132-168 of complex.c.
pub fn mycpow(x: Complex<f64>, y: Complex<f64>) -> Complex<f64> {
    let yr = y.re;
    let yi = y.im;

    if x == Complex::new(0.0, 0.0) {
        if yi == 0.0 {
            Complex::new(R_pow(0.0, yr), 0.0)
        } else {
            Complex::new(f64::NAN, f64::NAN)
        }
    } else if yi == 0.0 {
        let k = yr as i32;
        if yr == (k as f64) && k.abs() <= 65536 {
            R_cpow_n(x, k)
        } else {
            x.powc(y)
        }
    } else {
        x.powc(y)
    }
}

// ---------------------------------------------------------------------------
// Complex math fallback implementations
// ---------------------------------------------------------------------------

/// Complex logarithm: log(|z|) + i*arg(z).
///
/// Ported from lines 397-405 of complex.c.
pub fn clog(x: Complex<f64>) -> Complex<f64> {
    let xr = x.re;
    let xi = x.im;
    Complex::new(xr.hypot(xi).ln(), xi.atan2(xr))
}

/// Complex square root via mycpow(z, 0.5).
///
/// Ported from lines 407-414 of complex.c.
pub fn csqrt(x: Complex<f64>) -> Complex<f64> {
    mycpow(x, Complex::new(0.5, 0.0))
}

/// Complex exponential: exp(xr)*(cos(y) + i*sin(y)).
///
/// Ported from lines 416-424 of complex.c.
pub fn cexp(x: Complex<f64>) -> Complex<f64> {
    let expx = x.re.exp();
    let y = x.im;
    Complex::new(expx * y.cos(), expx * y.sin())
}

/// Complex cosine (A&S 4.3.56): cos(xr)*cosh(xi) - i*sin(xr)*sinh(xi).
///
/// Ported from lines 426-433 of complex.c.
pub fn ccos(x: Complex<f64>) -> Complex<f64> {
    let xr = x.re;
    let xi = x.im;
    Complex::new(xr.cos() * xi.cosh(), -(xr.sin() * xi.sinh()))
}

/// Complex sine (A&S 4.3.55): sin(xr)*cosh(xi) + i*cos(xr)*sinh(xi).
///
/// Ported from lines 435-442 of complex.c.
pub fn csin(x: Complex<f64>) -> Complex<f64> {
    let xr = x.re;
    let xi = x.im;
    Complex::new(xr.sin() * xi.cosh(), xr.cos() * xi.sinh())
}

/// Complex tangent (A&S 4.3.57).
///
/// Ported from lines 444-458 of complex.c.
pub fn ctan(z: Complex<f64>) -> Complex<f64> {
    let x2 = 2.0 * z.re;
    let y2 = 2.0 * z.im;
    let den = x2.cos() + y2.cosh();
    let ri = if y2.is_nan() || y2.abs() < 50.0 {
        y2.sinh() / den
    } else {
        if y2 < 0.0 { -1.0 } else { 1.0 }
    };
    Complex::new(x2.sin() / den, ri)
}

/// Complex arcsine (A&S 4.4.37).
///
/// Ported from lines 460-477 of complex.c.
pub fn casin(z: Complex<f64>) -> Complex<f64> {
    let x = z.re;
    let y = z.im;
    let t1 = 0.5 * (x + 1.0_f64).hypot(y);
    let t2 = 0.5 * (x - 1.0_f64).hypot(y);
    let alpha = t1 + t2;
    let mut ri = (alpha + (alpha * alpha - 1.0_f64).sqrt()).ln();
    // z_asin() is continuous from below if x >= 1, continuous from above if x <= -1
    if y < 0.0 || (y == 0.0 && x > 1.0) {
        ri *= -1.0;
    }
    Complex::new((t1 - t2).asin(), ri)
}

/// Complex arccosine: pi/2 - asin(z).
///
/// Ported from lines 479-485 of complex.c.
pub fn cacos(z: Complex<f64>) -> Complex<f64> {
    std::f64::consts::FRAC_PI_2 - casin(z)
}

/// Complex arctangent.
///
/// Ported from lines 487-497 of complex.c.
pub fn catan(z: Complex<f64>) -> Complex<f64> {
    let x = z.re;
    let y = z.im;
    let rr = 0.5 * (2.0 * x).atan2(1.0 - x * x - y * y);
    let ri = 0.25 * ((x * x + (y + 1.0).powi(2)) / (x * x + (y - 1.0).powi(2))).ln();
    Complex::new(rr, ri)
}

/// Complex hyperbolic cosine: cos(z*i) (A&S 4.5.8).
///
/// Ported from lines 499-505 of complex.c.
pub fn ccosh(z: Complex<f64>) -> Complex<f64> {
    ccos(z * Complex::new(0.0, 1.0))
}

/// Complex hyperbolic sine: -i*sin(z*i) (A&S 4.5.7).
///
/// Ported from lines 507-513 of complex.c.
pub fn csinh(z: Complex<f64>) -> Complex<f64> {
    let result = csin(z * Complex::new(0.0, 1.0));
    Complex::new(result.im, -result.re)
}

// ---------------------------------------------------------------------------
// Branch-cut aware versions
// ---------------------------------------------------------------------------

/// Branch-cut aware complex tangent.
///
/// Ported from lines 515-529 of complex.c.
pub fn z_tan(z: Complex<f64>) -> Complex<f64> {
    let y = z.im;
    let mut r = ctan(z);
    if R_FINITE(y) && y.abs() > 25.0 {
        // At this point the real part is nearly zero, and the
        // imaginary part is one: but some OSes get the imag as NaN.
        r = Complex::new(r.re, if y < 0.0 { -1.0 } else { 1.0 });
    }
    r
}

/// Complex hyperbolic tangent: -i*tan(z*i) (A&S 4.5.9).
///
/// Ported from lines 531-537 of complex.c.
pub fn ctanh(z: Complex<f64>) -> Complex<f64> {
    let result = z_tan(z * Complex::new(0.0, 1.0));
    Complex::new(result.im, -result.re)
}

/// Branch-cut aware complex arcsine.
///
/// Ported from lines 542-554 of complex.c.
pub fn z_asin(z: Complex<f64>) -> Complex<f64> {
    if z.im == 0.0 && z.re.abs() > 1.0 {
        let x = z.re;
        let t1 = 0.5 * (x + 1.0_f64).abs();
        let t2 = 0.5 * (x - 1.0_f64).abs();
        let alpha = t1 + t2;
        let mut ri = (alpha + (alpha * alpha - 1.0_f64).sqrt()).ln();
        if x > 1.0 {
            ri *= -1.0;
        }
        Complex::new((t1 - t2).asin(), ri)
    } else {
        casin(z)
    }
}

/// Branch-cut aware complex arccosine.
///
/// Ported from lines 556-560 of complex.c.
pub fn z_acos(z: Complex<f64>) -> Complex<f64> {
    if z.im == 0.0 && z.re.abs() > 1.0 {
        std::f64::consts::FRAC_PI_2 - z_asin(z)
    } else {
        cacos(z)
    }
}

/// Branch-cut aware complex arctangent.
///
/// Ported from lines 562-571 of complex.c.
pub fn z_atan(z: Complex<f64>) -> Complex<f64> {
    if z.re == 0.0 && z.im.abs() > 1.0 {
        let y = z.im;
        let rr = if y > 0.0 {
            std::f64::consts::FRAC_PI_2
        } else {
            -std::f64::consts::FRAC_PI_2
        };
        let ri = 0.25 * (((y + 1.0) * (y + 1.0)) / ((y - 1.0) * (y - 1.0))).ln();
        Complex::new(rr, ri)
    } else {
        catan(z)
    }
}

/// Complex inverse hyperbolic cosine: acos(z) * i.
///
/// Ported from lines 573-576 of complex.c.
pub fn z_acosh(z: Complex<f64>) -> Complex<f64> {
    let result = z_acos(z);
    Complex::new(-result.im, result.re)
}

/// Complex inverse hyperbolic sine: -i * asin(z * i).
///
/// Ported from lines 578-581 of complex.c.
pub fn z_asinh(z: Complex<f64>) -> Complex<f64> {
    let result = z_asin(z * Complex::new(0.0, 1.0));
    Complex::new(result.im, -result.re)
}

/// Complex inverse hyperbolic tangent: -i * atan(z * i).
///
/// Ported from lines 583-586 of complex.c.
pub fn z_atanh(z: Complex<f64>) -> Complex<f64> {
    let result = z_atan(z * Complex::new(0.0, 1.0));
    Complex::new(result.im, -result.re)
}

// ---------------------------------------------------------------------------
// z_prec_r -- signif for complex numbers
// ---------------------------------------------------------------------------

/// Apply signif() to a complex number, rounding both real and imaginary parts
/// to the given number of significant digits.
///
/// Ported from lines 358-388 of complex.c.
pub fn z_prec_r(r: &mut Rcomplex, x: &Rcomplex, digits: f64) {
    const MAX_DIGITS: i32 = 22;

    r.r = x.r;
    r.i = x.i;

    let mut m = 0.0_f64;
    let m1 = x.r.abs();
    let m2 = x.i.abs();
    if R_FINITE(m1) {
        m = m1;
    }
    if R_FINITE(m2) && m2 > m {
        m = m2;
    }
    if m == 0.0 {
        return;
    }
    if !R_FINITE(digits) {
        if digits > 0.0 {
            return;
        } else {
            r.r = 0.0;
            r.i = 0.0;
            return;
        }
    }

    let mut dig = (digits + 0.5).floor() as i32;
    if dig > MAX_DIGITS {
        return;
    } else if dig < 1 {
        dig = 1;
    }

    let mag = m.log10().floor() as i32;
    dig = dig - mag - 1;

    if dig > 306 {
        let pow10 = 1.0e4;
        let digits_f = (dig - 4) as f64;
        r.r = fround(pow10 * x.r, digits_f) / pow10;
        r.i = fround(pow10 * x.i, digits_f) / pow10;
    } else {
        let digits_f = dig as f64;
        r.r = fround(x.r, digits_f);
        r.i = fround(x.i, digits_f);
    }
}

// ---------------------------------------------------------------------------
// cmath1 -- apply complex function to array with NA handling
// ---------------------------------------------------------------------------

/// Apply a complex unary function `f` to each element of the input array,
/// writing results to the output array. Returns true if any NaN was produced
/// from non-NaN input.
///
/// Ported from lines 595-610 of complex.c.
pub fn cmath1(
    f: fn(Complex<f64>) -> Complex<f64>,
    x: &[Rcomplex],
    y: &mut [Rcomplex],
    n: R_xlen_t,
) -> bool {
    let mut naflag = false;
    for i in 0..n as usize {
        let xi = &x[i];
        if xi.r.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            || xi.i.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        {
            // R_IsNA
            y[i].r = NA_REAL;
            y[i].i = NA_REAL;
        } else {
            let z = f(to_complex(xi));
            y[i] = from_complex(z);
            if (ISNAN(y[i].r) || ISNAN(y[i].i)) && !(ISNAN(x[i].r) || ISNAN(x[i].i)) {
                naflag = true;
            }
        }
    }
    naflag
}

// ---------------------------------------------------------------------------
// z_rround, z_prec, z_logbase, z_atan2 -- complex utility functions
// ---------------------------------------------------------------------------

/// Round a complex number to `p.r` decimal places.
///
/// Ported from lines 659-663 of complex.c.
pub fn z_rround(r: &mut Rcomplex, x: &Rcomplex, p: &Rcomplex) {
    r.r = fround(x.r, p.r);
    r.i = fround(x.i, p.r);
}

/// Apply signif() to a complex number using z_prec_r.
///
/// Ported from lines 665-668 of complex.c.
pub fn z_prec(r: &mut Rcomplex, x: &Rcomplex, p: &Rcomplex) {
    z_prec_r(r, x, p.r);
}

/// Complex logarithm in a given base: log(z) / log(base).
///
/// Ported from lines 670-674 of complex.c.
pub fn z_logbase(r: &mut Rcomplex, z: &Rcomplex, base: &Rcomplex) {
    let dz = to_complex(z);
    let dbase = to_complex(base);
    let result = clog(dz) / clog(dbase);
    *r = from_complex(result);
}

/// Complex atan2(y, x).
///
/// Ported from lines 676-694 of complex.c.
pub fn z_atan2(r: &mut Rcomplex, csn: &Rcomplex, ccs: &Rcomplex) {
    let dcsn = to_complex(csn);
    let dccs = to_complex(ccs);

    if dccs == Complex::new(0.0, 0.0) {
        if dcsn == Complex::new(0.0, 0.0) {
            r.r = NA_REAL;
            r.i = NA_REAL;
            return;
        }
        let y = dcsn.re;
        let real_part = if ISNAN(y) {
            y
        } else if y >= 0.0 {
            std::f64::consts::FRAC_PI_2
        } else {
            -std::f64::consts::FRAC_PI_2
        };
        *r = from_complex(Complex::new(real_part, 0.0));
        return;
    }

    let mut dr = catan(dcsn / dccs);
    if dccs.re < 0.0 {
        dr += std::f64::consts::PI;
    }
    if dr.re > std::f64::consts::PI {
        dr -= 2.0 * std::f64::consts::PI;
    }
    *r = from_complex(dr);
}

// ---------------------------------------------------------------------------
// FFI entry points (SEXP-dependent)
// ---------------------------------------------------------------------------

/// Unary + and - on complex vectors.
///
/// Ported from lines 75-98 of complex.c.
pub unsafe fn complex_unary(code: c_int, s1: SEXP, _call: SEXP) -> SEXP {
    unsafe {
        match code {
            0 => {
                // PLUSOP
                s1
            }
            1 => {
                // MINUSOP
                // NO_REFERENCES(x) -> always return new copy in our stub
                let n = XLENGTH(s1);
                let ans = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, n);
                if ans.is_null() || s1.is_null() {
                    return R_NilValue();
                }
                let ps1 = COMPLEX(s1);
                let pans = COMPLEX(ans);
                for i in 0..n as usize {
                    let x = *ps1.add(i);
                    (*pans.add(i)).r = -x.r;
                    (*pans.add(i)).i = -x.i;
                }
                ans
            }
            _ => R_NilValue(),
        }
    }
}

/// Binary +, -, *, /, ^ on complex vectors.
///
/// Ported from lines 172-243 of complex.c.
pub unsafe fn complex_binary(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1);
        let n2 = XLENGTH(s2);

        // S4-compatibility: if n1 or n2 is 0, result is length 0
        if n1 == 0 || n2 == 0 {
            return Rf_allocVector3(SEXPTYPE::CPLXSXP.0, 0);
        }

        let n = if n1 > n2 { n1 } else { n2 };
        let ans = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, n);
        if ans.is_null() || s1.is_null() || s2.is_null() {
            return R_NilValue();
        }

        let ps1 = COMPLEX(s1);
        let ps2 = COMPLEX(s2);
        let pans = COMPLEX(ans);

        match code {
            0 => {
                // PLUSOP
                let mut i1 = 0usize;
                let mut i2 = 0usize;
                for i in 0..n as usize {
                    if n1 > 0 {
                        i1 = i % n1 as usize;
                    }
                    if n2 > 0 {
                        i2 = i % n2 as usize;
                    }
                    let x1 = *ps1.add(i1);
                    let x2 = *ps2.add(i2);
                    (*pans.add(i)).r = x1.r + x2.r;
                    (*pans.add(i)).i = x1.i + x2.i;
                }
            }
            1 => {
                // MINUSOP
                let mut i1 = 0usize;
                let mut i2 = 0usize;
                for i in 0..n as usize {
                    if n1 > 0 {
                        i1 = i % n1 as usize;
                    }
                    if n2 > 0 {
                        i2 = i % n2 as usize;
                    }
                    let x1 = *ps1.add(i1);
                    let x2 = *ps2.add(i2);
                    (*pans.add(i)).r = x1.r - x2.r;
                    (*pans.add(i)).i = x1.i - x2.i;
                }
            }
            2 => {
                // TIMESOP
                let mut i1 = 0usize;
                let mut i2 = 0usize;
                for i in 0..n as usize {
                    if n1 > 0 {
                        i1 = i % n1 as usize;
                    }
                    if n2 > 0 {
                        i2 = i % n2 as usize;
                    }
                    let val = to_complex(&*ps1.add(i1)) * to_complex(&*ps2.add(i2));
                    *pans.add(i) = from_complex(val);
                }
            }
            3 => {
                // DIVOP
                let mut i1 = 0usize;
                let mut i2 = 0usize;
                for i in 0..n as usize {
                    if n1 > 0 {
                        i1 = i % n1 as usize;
                    }
                    if n2 > 0 {
                        i2 = i % n2 as usize;
                    }
                    let val = to_complex(&*ps1.add(i1)) / to_complex(&*ps2.add(i2));
                    *pans.add(i) = from_complex(val);
                }
            }
            4 => {
                // POWOP
                let mut i1 = 0usize;
                let mut i2 = 0usize;
                for i in 0..n as usize {
                    if n1 > 0 {
                        i1 = i % n1 as usize;
                    }
                    if n2 > 0 {
                        i2 = i % n2 as usize;
                    }
                    let val = mycpow(to_complex(&*ps1.add(i1)), to_complex(&*ps2.add(i2)));
                    *pans.add(i) = from_complex(val);
                }
            }
            _ => {}
        }

        // Copy attributes from longer argument (stub: only if non-nil)
        let attr_s2 = ATTRIB(s2);
        let attr_s1 = ATTRIB(s1);
        if !attr_s2.is_null() && attr_s2 != R_NilValue() && n == n2 {
            SET_ATTRIB(ans, attr_s2);
        }
        if !attr_s1.is_null() && attr_s1 != R_NilValue() && n == n1 {
            SET_ATTRIB(ans, attr_s1);
        }

        ans
    }
}

/// Re, Im, Mod, Arg, Conj functions.
///
/// Ported from lines 245-356 of complex.c.
pub unsafe fn do_cmathfuns(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() {
            return R_NilValue();
        }

        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }

        let xtype = TYPEOF(x);
        let n = XLENGTH(x);

        if xtype == SEXPTYPE::CPLXSXP.0 {
            // Complex input
            let px = COMPLEX(x);
            // Default to case 1 (Re) since PRIMVAL(op) is stubbed to 0
            let primval = 0; // stub: PRIMVAL(op) returns 0

            match primval {
                1 => {
                    // Re
                    let y = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
                    if y.is_null() {
                        return R_NilValue();
                    }
                    let py = REAL(y);
                    for i in 0..n as usize {
                        *py.add(i) = (*px.add(i)).r;
                    }
                    return y;
                }
                2 => {
                    // Im
                    let y = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
                    if y.is_null() {
                        return R_NilValue();
                    }
                    let py = REAL(y);
                    for i in 0..n as usize {
                        *py.add(i) = (*px.add(i)).i;
                    }
                    return y;
                }
                3 | 6 => {
                    // Mod / abs
                    let y = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
                    if y.is_null() {
                        return R_NilValue();
                    }
                    let py = REAL(y);
                    for i in 0..n as usize {
                        let xi = *px.add(i);
                        *py.add(i) = xi.r.hypot(xi.i);
                    }
                    return y;
                }
                4 => {
                    // Arg
                    let y = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
                    if y.is_null() {
                        return R_NilValue();
                    }
                    let py = REAL(y);
                    for i in 0..n as usize {
                        let xi = *px.add(i);
                        *py.add(i) = xi.i.atan2(xi.r);
                    }
                    return y;
                }
                5 => {
                    // Conj
                    let y = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, n);
                    if y.is_null() {
                        return R_NilValue();
                    }
                    let py = COMPLEX(y);
                    for i in 0..n as usize {
                        let xi = *px.add(i);
                        (*py.add(i)).r = xi.r;
                        (*py.add(i)).i = -xi.i;
                    }
                    return y;
                }
                _ => {
                    // Default: treat as Re
                    let y = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
                    if y.is_null() {
                        return R_NilValue();
                    }
                    let py = REAL(y);
                    for i in 0..n as usize {
                        *py.add(i) = (*px.add(i)).r;
                    }
                    return y;
                }
            }
        } else if xtype == SEXPTYPE::REALSXP.0
            || xtype == SEXPTYPE::INTSXP.0
            || xtype == SEXPTYPE::LGLSXP.0
        {
            // Numeric (non-complex) input
            let px = REAL(x);
            let y = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
            if y.is_null() || px.is_null() {
                return R_NilValue();
            }
            let py = REAL(y);
            let primval = 0; // stub

            match primval {
                1 | 5 => {
                    // Re / Conj
                    for i in 0..n as usize {
                        *py.add(i) = *px.add(i);
                    }
                }
                2 => {
                    // Im
                    for i in 0..n as usize {
                        *py.add(i) = 0.0;
                    }
                }
                4 => {
                    // Arg
                    for i in 0..n as usize {
                        let v = *px.add(i);
                        if ISNAN(v) {
                            *py.add(i) = v;
                        } else if v >= 0.0 {
                            *py.add(i) = 0.0;
                        } else {
                            *py.add(i) = std::f64::consts::PI;
                        }
                    }
                }
                3 | 6 => {
                    // Mod / abs
                    for i in 0..n as usize {
                        *py.add(i) = (*px.add(i)).abs();
                    }
                }
                _ => {
                    for i in 0..n as usize {
                        *py.add(i) = *px.add(i);
                    }
                }
            }
            y
        } else {
            R_NilValue()
        }
    }
}

/// Complex math functions of one argument: log, sqrt, exp, cos, sin, tan, etc.
///
/// Ported from lines 612-657 of complex.c.
pub unsafe fn complex_math1(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() {
            return R_NilValue();
        }
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let y = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, n);
        if y.is_null() || x.is_null() {
            return R_NilValue();
        }

        let px = COMPLEX(x);
        let py = COMPLEX(y);

        let primval = 0; // stub: PRIMVAL(op)

        // Create slices for cmath1
        let x_slice = std::slice::from_raw_parts(px, n as usize);
        let y_slice = std::slice::from_raw_parts_mut(py, n as usize);

        let _naflag = match primval {
            10003 => cmath1(clog, x_slice, y_slice, n),
            3 => cmath1(csqrt, x_slice, y_slice, n),
            10 => cmath1(cexp, x_slice, y_slice, n),
            20 => cmath1(ccos, x_slice, y_slice, n),
            21 => cmath1(csin, x_slice, y_slice, n),
            22 => cmath1(z_tan, x_slice, y_slice, n),
            23 => cmath1(z_acos, x_slice, y_slice, n),
            24 => cmath1(z_asin, x_slice, y_slice, n),
            25 => cmath1(z_atan, x_slice, y_slice, n),
            30 => cmath1(ccosh, x_slice, y_slice, n),
            31 => cmath1(csinh, x_slice, y_slice, n),
            32 => cmath1(ctanh, x_slice, y_slice, n),
            33 => cmath1(z_acosh, x_slice, y_slice, n),
            34 => cmath1(z_asinh, x_slice, y_slice, n),
            35 => cmath1(z_atanh, x_slice, y_slice, n),
            _ => {
                // Default: log (case 10003 equivalent)
                cmath1(clog, x_slice, y_slice, n)
            }
        };

        // SHALLOW_DUPLICATE_ATTRIB stub
        let attr_x = ATTRIB(x);
        if !attr_x.is_null() && attr_x != R_NilValue() {
            SET_ATTRIB(y, attr_x);
        }

        y
    }
}

/// Complex math functions of two arguments: atan2, round, log, signif.
///
/// Ported from lines 700-755 of complex.c.
pub unsafe fn complex_math2(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() {
            return R_NilValue();
        }

        let sa = CAR(args);
        let sb = if !CDR(args).is_null() {
            CAR(CDR(args))
        } else {
            R_NilValue()
        };

        if sa.is_null() || sb.is_null() {
            return R_NilValue();
        }

        // coerceVector stub: only handle CPLXSXP
        let na = XLENGTH(sa);
        let nb = XLENGTH(sb);

        if na == 0 || nb == 0 {
            return Rf_allocVector3(SEXPTYPE::CPLXSXP.0, 0);
        }

        let n = if na < nb { nb } else { na };
        let sy = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, n);
        if sy.is_null() {
            return R_NilValue();
        }

        let a = COMPLEX(sa);
        let b = COMPLEX(sb);
        let y = COMPLEX(sy);

        let primval = 0; // stub

        let mut i1 = 0usize;
        let mut i2 = 0usize;
        for i in 0..n as usize {
            if na > 0 {
                i1 = i % na as usize;
            }
            if nb > 0 {
                i2 = i % nb as usize;
            }
            let ai = *a.add(i1);
            let bi = *b.add(i2);

            if ai.r.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                && ai.i.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                && bi.r.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                && bi.i.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                (*y.add(i)).r = NA_REAL;
                (*y.add(i)).i = NA_REAL;
            } else {
                match primval {
                    0 => {
                        // atan2
                        z_atan2(&mut *y.add(i), &ai, &bi);
                    }
                    10001 => {
                        // round
                        z_rround(&mut *y.add(i), &ai, &bi);
                    }
                    10002 | 10010 | 10003 => {
                        // log base
                        z_logbase(&mut *y.add(i), &ai, &bi);
                    }
                    10004 => {
                        // signif
                        z_prec(&mut *y.add(i), &ai, &bi);
                    }
                    _ => {
                        // default: log base
                        z_logbase(&mut *y.add(i), &ai, &bi);
                    }
                }
            }
        }

        // SHALLOW_DUPLICATE_ATTRIB
        if n == na {
            let attr = ATTRIB(sa);
            if !attr.is_null() && attr != R_NilValue() {
                SET_ATTRIB(sy, attr);
            }
        } else if n == nb {
            let attr = ATTRIB(sb);
            if !attr.is_null() && attr != R_NilValue() {
                SET_ATTRIB(sy, attr);
            }
        }

        sy
    }
}

/// complex(length, real, imag) constructor.
///
/// Ported from lines 757-793 of complex.c.
pub unsafe fn do_complex(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() {
            return R_NilValue();
        }

        // First arg: length (should be integer)
        let length_arg = CAR(args);
        let mut na: R_xlen_t = 0;
        if !length_arg.is_null() && TYPEOF(length_arg) == SEXPTYPE::INTSXP.0 {
            let pv = INTEGER(length_arg);
            if !pv.is_null() {
                na = *pv as R_xlen_t;
            }
        }

        if na == NA_INTEGER as R_xlen_t || na < 0 {
            return R_NilValue();
        }

        // Second arg: real part
        let re_arg = if !CDR(args).is_null() {
            CAR(CDR(args))
        } else {
            R_NilValue()
        };

        // Third arg: imaginary part
        let im_arg = if !CDR(args).is_null() && !CDR(CDR(args)).is_null() {
            CAR(CDR(CDR(args)))
        } else {
            R_NilValue()
        };

        let mut nr: R_xlen_t = 0;
        let mut ni: R_xlen_t = 0;

        // Get real part pointer
        let pre = if !re_arg.is_null() && TYPEOF(re_arg) == SEXPTYPE::REALSXP.0 {
            nr = XLENGTH(re_arg);
            REAL(re_arg)
        } else {
            std::ptr::null_mut()
        };

        // Get imaginary part pointer
        let pim = if !im_arg.is_null() && TYPEOF(im_arg) == SEXPTYPE::REALSXP.0 {
            ni = XLENGTH(im_arg);
            REAL(im_arg)
        } else {
            std::ptr::null_mut()
        };

        // Recycle lengths
        if nr > na {
            na = nr;
        }
        if ni > na {
            na = ni;
        }

        let ans = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, na);
        if ans.is_null() {
            return R_NilValue();
        }

        let pans = COMPLEX(ans);
        // Initialize to zero
        for i in 0..na as usize {
            (*pans.add(i)).r = 0.0;
            (*pans.add(i)).i = 0.0;
        }

        // Fill in real parts
        if na > 0 && nr > 0 && !pre.is_null() {
            for i in 0..na as usize {
                (*pans.add(i)).r = *pre.add(i % nr as usize);
            }
        }

        // Fill in imaginary parts
        if na > 0 && ni > 0 && !pim.is_null() {
            for i in 0..na as usize {
                (*pans.add(i)).i = *pim.add(i % ni as usize);
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_R_cpow_n_zero() {
        let z = Complex::new(2.0, 3.0);
        let result = R_cpow_n(z, 0);
        assert!((result.re - 1.0).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_R_cpow_n_one() {
        let z = Complex::new(2.0, 3.0);
        let result = R_cpow_n(z, 1);
        assert!((result.re - 2.0).abs() < 1e-10);
        assert!((result.im - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_R_cpow_n_positive() {
        let z = Complex::new(2.0, 0.0);
        let result = R_cpow_n(z, 3);
        assert!((result.re - 8.0).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_R_cpow_n_negative() {
        let z = Complex::new(2.0, 0.0);
        let result = R_cpow_n(z, -2);
        assert!((result.re - 0.25).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_R_cpow_n_complex() {
        // (1+i)^2 = 2i
        let z = Complex::new(1.0, 1.0);
        let result = R_cpow_n(z, 2);
        assert!(result.re.abs() < 1e-10);
        assert!((result.im - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_mycpow_zero_base_real_exp() {
        let x = Complex::new(0.0, 0.0);
        let y = Complex::new(2.0, 0.0);
        let result = mycpow(x, y);
        assert!((result.re - 0.0).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_mycpow_zero_base_complex_exp() {
        let x = Complex::new(0.0, 0.0);
        let y = Complex::new(1.0, 1.0);
        let result = mycpow(x, y);
        assert!(result.re.is_nan());
        assert!(result.im.is_nan());
    }

    #[test]
    fn test_mycpow_integer_exp() {
        let x = Complex::new(2.0, 3.0);
        let y = Complex::new(3.0, 0.0);
        let result = mycpow(x, y);
        // Compare with direct R_cpow_n
        let expected = R_cpow_n(x, 3);
        assert!((result.re - expected.re).abs() < 1e-10);
        assert!((result.im - expected.im).abs() < 1e-10);
    }

    #[test]
    fn test_mycpow_general() {
        let x = Complex::new(2.0, 0.0);
        let y = Complex::new(0.5, 0.0);
        let result = mycpow(x, y);
        assert!((result.re - 2.0_f64.sqrt()).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    // -- Complex math fallbacks --

    #[test]
    fn test_clog_real_positive() {
        let z = Complex::new(1.0, 0.0);
        let result = clog(z);
        assert!(result.re.abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_clog_real_negative() {
        let z = Complex::new(-1.0, 0.0);
        let result = clog(z);
        assert!(result.re.abs() < 1e-10);
        assert!((result.im - std::f64::consts::PI).abs() < 1e-10);
    }

    #[test]
    fn test_csqrt_positive_real() {
        let z = Complex::new(4.0, 0.0);
        let result = csqrt(z);
        assert!((result.re - 2.0).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_csqrt_negative_real() {
        let z = Complex::new(-4.0, 0.0);
        let result = csqrt(z);
        assert!(result.re.abs() < 1e-10);
        assert!((result.im - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_cexp_real() {
        let z = Complex::new(1.0, 0.0);
        let result = cexp(z);
        assert!((result.re - 1.0_f64.exp()).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_cexp_imaginary() {
        let z = Complex::new(0.0, std::f64::consts::PI);
        let result = cexp(z);
        assert!((result.re - (-1.0)).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_ccos_real() {
        let z = Complex::new(0.0, 0.0);
        let result = ccos(z);
        assert!((result.re - 1.0).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_ccos_imaginary() {
        // cos(i) = cosh(1) ~ 1.5431
        let z = Complex::new(0.0, 1.0);
        let result = ccos(z);
        assert!((result.re - 1.0_f64.cosh()).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_csin_real() {
        let z = Complex::new(std::f64::consts::FRAC_PI_2, 0.0);
        let result = csin(z);
        assert!((result.re - 1.0).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_ctan_real() {
        let z = Complex::new(0.0, 0.0);
        let result = ctan(z);
        assert!(result.re.abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_ctan_pi_over_4() {
        let z = Complex::new(std::f64::consts::FRAC_PI_4, 0.0);
        let result = ctan(z);
        assert!((result.re - 1.0).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_casin_real_unit() {
        let z = Complex::new(1.0, 0.0);
        let result = casin(z);
        assert!((result.re - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_casin_zero() {
        let z = Complex::new(0.0, 0.0);
        let result = casin(z);
        assert!(result.re.abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_cacos_real_unit() {
        let z = Complex::new(1.0, 0.0);
        let result = cacos(z);
        assert!(result.re.abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_catan_zero() {
        let z = Complex::new(0.0, 0.0);
        let result = catan(z);
        assert!(result.re.abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_catan_real() {
        let z = Complex::new(1.0, 0.0);
        let result = catan(z);
        assert!((result.re - std::f64::consts::FRAC_PI_4).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_ccosh_zero() {
        let z = Complex::new(0.0, 0.0);
        let result = ccosh(z);
        assert!((result.re - 1.0).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_csinh_zero() {
        let z = Complex::new(0.0, 0.0);
        let result = csinh(z);
        assert!(result.re.abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_ctanh_zero() {
        let z = Complex::new(0.0, 0.0);
        let result = ctanh(z);
        assert!(result.re.abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    // -- z_prec_r tests --

    #[test]
    fn test_z_prec_r_basic() {
        let x = Rcomplex {
            r: 1.234567,
            i: 9.876543,
        };
        let mut r = Rcomplex { r: 0.0, i: 0.0 };
        z_prec_r(&mut r, &x, 4.0);
        // 1.235, 9.877
        assert!((r.r - 1.235).abs() < 1e-3);
        assert!((r.i - 9.877).abs() < 1e-3);
    }

    #[test]
    fn test_z_prec_r_zero() {
        let x = Rcomplex { r: 0.0, i: 0.0 };
        let mut r = Rcomplex { r: 0.0, i: 0.0 };
        z_prec_r(&mut r, &x, 4.0);
        assert_eq!(r.r, 0.0);
        assert_eq!(r.i, 0.0);
    }

    #[test]
    fn test_z_prec_r_negative_digits() {
        let x = Rcomplex {
            r: 1.234567,
            i: 9.876543,
        };
        let mut r = Rcomplex { r: 0.0, i: 0.0 };
        z_prec_r(&mut r, &x, -1.0);
        // dig becomes 1 (clamped from -1), then mag=0, dig=0,
        // so fround(x, 0) rounds to 0 decimal places: 1 and 10
        assert!((r.r - 1.0).abs() < 1e-10);
        assert!((r.i - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_z_prec_r_inf_digits() {
        let x = Rcomplex {
            r: 1.234567,
            i: 9.876543,
        };
        let mut r = Rcomplex { r: 0.0, i: 0.0 };
        z_prec_r(&mut r, &x, f64::INFINITY);
        // digits is infinite and positive => no change
        assert!((r.r - 1.234567).abs() < 1e-10);
        assert!((r.i - 9.876543).abs() < 1e-10);
    }

    // -- Euler's identity test --

    #[test]
    fn test_euler_identity() {
        // e^(i*pi) = -1
        let z = Complex::new(0.0, std::f64::consts::PI);
        let result = cexp(z);
        assert!((result.re - (-1.0)).abs() < 1e-10);
        assert!(result.im.abs() < 1e-10);
    }

    #[test]
    fn test_cexp_clog_inverse() {
        let z = Complex::new(2.0, 3.0);
        let roundtrip = cexp(clog(z));
        assert!((roundtrip.re - z.re).abs() < 1e-10);
        assert!((roundtrip.im - z.im).abs() < 1e-10);
    }
}
