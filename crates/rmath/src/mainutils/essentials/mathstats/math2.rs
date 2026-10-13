//! Math2 builtins (log2, round, signif, trunc) and shared helpers — extracted verbatim from essentials/mathstats.rs.

use super::*;

// ---------------------------------------------------------------------------
// R's math2 builtins (2-arg math): log2, round, signif, trunc
// ---------------------------------------------------------------------------

/// R's `log2(x)` — log base 2 with optional explicit base override.
pub unsafe fn do_log2(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut dispatched = R_NilValue();
        if crate::eval::dispatch::DispatchGroup(
            c"Math".as_ptr(),
            call,
            op,
            args,
            rho,
            &mut dispatched,
        ) != 0
        {
            return dispatched;
        }
        let x_arg = CAR(args);
        let base_arg = CAR(CDR(args));

        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let base = if base_arg.is_null() || base_arg == R_NilValue() {
            2.0
        } else {
            real_or_default(base_arg, std::f64::consts::E)
        };
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);
        if t == SEXPTYPE::CPLXSXP {
            let base_sexp = if base_arg.is_null() || base_arg == R_NilValue() {
                crate::sexp::constructors::Rf_ScalarReal(2.0)
            } else {
                base_arg
            };
            let _g = protect(base_sexp);
            return crate::eval::arithmetic::complex_log_with_base(x_arg, base_sexp);
        }
        if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
            crate::mainutils::errors::errorcall_str(
                call,
                "non-numeric argument to mathematical function",
            );
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        let log_base = base.ln();
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                NA_REAL
            } else if v == 0.0 {
                f64::NEG_INFINITY
            } else if v < 0.0 {
                f64::NAN
            } else {
                v.ln() / log_base
            };
        }
        result
    }
}

/// complex_math2's round/signif driver (r-source/src/main/complex.c):
/// coerce both operands to complex, recycle to max(na, nb), and apply `f`
/// part-wise with the digits scalar `b.r`. The all-NA rule yields
/// NA_complex_ only when every input part is NA; a NaN result part with no
/// NaN/NA input part raises stock's "NaNs produced in function \"%s\"".
unsafe fn math2_complex(
    call: SEXP,
    x_arg: SEXP,
    digits_arg: SEXP,
    dflt_digits: f64,
    name: &str,
    f: fn(re: f64, im: f64, digits: f64) -> (f64, f64),
) -> SEXP {
    unsafe {
        // Trunk coerces CAR(args) (x) to complex first and CADR(args)
        // (digits) second; x is already complex at this dispatch, so its
        // coercion is the identity. `sa` stays the value operand and `sb`
        // the digits operand, matching complex_math2's a/b roles.
        let sa = x_arg;
        let sb =
            if digits_arg.is_null() || digits_arg == R_NilValue() || digits_arg == R_MissingArg() {
                // do_Math2 fills the digits default upstream.
                let s = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
                *COMPLEX(s) = Rcomplex {
                    r: dflt_digits,
                    i: 0.0,
                };
                s
            } else {
                crate::mainutils::coerce::coerceVector(digits_arg, SEXPTYPE::CPLXSXP.as_c_int())
            };
        let _sa_guard = protect(sa);
        let _sb_guard = protect(sb);

        let na = XLENGTH(sa);
        let nb = XLENGTH(sb);
        if na == 0 || nb == 0 {
            return Rf_allocVector3(SEXPTYPE::CPLXSXP, 0);
        }
        let n = if na < nb { nb } else { na };
        let sy = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        let _sy_guard = protect(sy);

        let a = std::slice::from_raw_parts(COMPLEX(sa), na as usize);
        let b = std::slice::from_raw_parts(COMPLEX(sb), nb as usize);
        let y = std::slice::from_raw_parts_mut(COMPLEX(sy), n as usize);
        let na_bit = crate::sexp::ffi::R_NA_BIT_PATTERN;
        let mut naflag = false;
        // MOD_ITERATE2 recycling of x and digits.
        for i in 0..n as usize {
            let ai = a[i % na as usize];
            let bi = b[i % nb as usize];
            if ai.r.to_bits() == na_bit
                && ai.i.to_bits() == na_bit
                && bi.r.to_bits() == na_bit
                && bi.i.to_bits() == na_bit
            {
                y[i].r = NA_REAL;
                y[i].i = NA_REAL;
            } else {
                let (r, im) = f(ai.r, ai.i, bi.r);
                y[i].r = r;
                y[i].i = im;
                if (r.is_nan() || im.is_nan())
                    && !(ISNAN(ai.r) || ISNAN(ai.i) || ISNAN(bi.r) || ISNAN(bi.i))
                {
                    naflag = true;
                }
            }
        }
        if naflag {
            let msg =
                CString::new(format!("NaNs produced in function \"{name}\"")).unwrap_or_default();
            crate::mainutils::errors::Rf_warningcall1(call, msg.as_ptr());
        }
        if n == na {
            copy_all_attribs(sy, sa);
        } else if n == nb {
            copy_all_attribs(sy, sb);
        }
        sy
    }
}

/// Port of complex.c's `z_prec_r`: `signif()` for complex parts scales both
/// parts by the magnitude of the larger one (`m = max(|re|, |im|)`), so
/// they share a single exponent scale before `fround` applies.
fn z_prec_r(re: f64, im: f64, digits: f64) -> (f64, f64) {
    const MAX_DIGITS: i32 = 22; // complex.c's local MAX_DIGITS
    let m1 = re.abs();
    let m2 = im.abs();
    let mut m = 0.0f64;
    if m1.is_finite() {
        m = m1;
    }
    if m2.is_finite() && m2 > m {
        m = m2;
    }
    if m == 0.0 {
        return (re, im);
    }
    if !digits.is_finite() {
        if digits > 0.0 {
            return (re, im);
        }
        return (0.0, 0.0);
    }
    let mut dig = (digits + 0.5).floor() as i32;
    if dig > MAX_DIGITS {
        return (re, im);
    } else if dig < 1 {
        dig = 1;
    }
    let mag = m.log10().floor() as i32;
    dig = dig - mag - 1;
    if dig > 306 {
        let pow10 = 1.0e4f64;
        let digits = (dig - 4) as f64;
        (
            fround(pow10 * re, digits) / pow10,
            fround(pow10 * im, digits) / pow10,
        )
    } else {
        let digits = dig as f64;
        (fround(re, digits), fround(im, digits))
    }
}

/// `setAttrib` rechecks `tsp` before `dim` is installed and rejects a valid mts.
pub(super) unsafe fn copy_all_attribs(dst: SEXP, src: SEXP) {
    unsafe {
        if dst.is_null() || dst == R_NilValue() || src.is_null() || src == R_NilValue() {
            return;
        }
        let attrs = ATTRIB(src);
        let copied = if attrs.is_null() || attrs == R_NilValue() {
            R_NilValue()
        } else {
            crate::mainutils::duplicate::shallow_duplicate(attrs)
        };
        let _copied = protect(copied);
        SET_ATTRIB(dst, copied);
        SET_OBJECT(dst, OBJECT(src));
        if crate::mainutils::coerce::IS_S4_OBJECT(src) != 0 {
            SET_S4_OBJECT(dst);
        } else {
            UNSET_S4_OBJECT(dst);
        }
    }
}

/// Exact names, then a unique partial name, then leftover positionals.
/// `round(d = 2, x = pi)` binds `x` exactly and `d` to `digits`.
unsafe fn match_x_digits(args: SEXP) -> (SEXP, SEXP) {
    unsafe {
        let formals = ["x", "digits"];
        let mut bound = [R_NilValue(), R_NilValue()];
        let mut seen = [false, false];
        let mut untagged = Vec::new();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            if tag.is_null() || tag == R_NilValue() {
                untagged.push(CAR(cell));
            } else {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned();
                if let Some(i) = formals.iter().position(|f| *f == name) {
                    bound[i] = CAR(cell);
                    seen[i] = true;
                }
            }
            cell = CDR(cell);
        }
        cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            if !tag.is_null() && tag != R_NilValue() {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned();
                if !formals.iter().any(|f| *f == name) {
                    let hits: Vec<usize> = formals
                        .iter()
                        .enumerate()
                        .filter(|(i, f)| !seen[*i] && f.starts_with(&name))
                        .map(|(i, _)| i)
                        .collect();
                    if hits.len() == 1 {
                        bound[hits[0]] = CAR(cell);
                        seen[hits[0]] = true;
                    } else {
                        let msg = format!("unused argument ({name} = ...)\0");
                        crate::mainutils::errors::Rf_error(msg.as_ptr() as *const _);
                    }
                }
            }
            cell = CDR(cell);
        }
        for value in untagged {
            if let Some(i) = seen.iter().position(|s| !s) {
                bound[i] = value;
                seen[i] = true;
            }
        }
        if !seen[0] {
            crate::mainutils::errors::Rf_error(
                b"argument \"x\" is missing, with no default\0".as_ptr() as *const _,
            );
        }
        (bound[0], bound[1])
    }
}

/// R's `round(x, digits=0)` — round to specified decimal digits.
pub unsafe fn do_round(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut dispatched = R_NilValue();
        if crate::eval::dispatch::DispatchGroup(
            c"Math".as_ptr(),
            call,
            op,
            args,
            rho,
            &mut dispatched,
        ) != 0
        {
            return dispatched;
        }
        let (x_arg, digits_arg) = match_x_digits(args);
        if crate::mainutils::essentials::sexp_has_class(x_arg, "POSIXt") {
            let paired = Rf_cons(digits_arg, R_NilValue());
            let _p = protect(paired);
            let call_args = Rf_cons(x_arg, paired);
            let _c = protect(call_args);
            return crate::mainutils::essentials::do_round_POSIXt(call, op, call_args, rho);
        }

        if x_arg.is_null() || x_arg == R_NilValue() || x_arg == R_MissingArg() {
            crate::mainutils::errors::Rf_error(
                b"argument \"x\" is missing, with no default\0".as_ptr() as *const _,
            );
        }
        // Stock routes complex x to complex_math2 (main/complex.c): round
        // each part with the same ties-even fround as the real path.
        if TYPEOF(x_arg) == SEXPTYPE::CPLXSXP {
            return math2_complex(call, x_arg, digits_arg, 0.0, "round", |re, im, digits| {
                (fround(re, digits), fround(im, digits))
            });
        }
        let digits =
            if digits_arg.is_null() || digits_arg == R_NilValue() || digits_arg == R_MissingArg() {
                0.0
            } else {
                real_or_default(digits_arg, 0.0)
            };
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                NA_REAL
            } else {
                fround(v, digits)
            };
        }
        let dim =
            crate::sexp::attrib_core::getAttrib(x_arg, crate::sexp::attrib_core::R_DimSymbol());
        if !dim.is_null() && dim != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimSymbol(),
                dim,
            );
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x_arg,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        if !dimnames.is_null() && dimnames != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dimnames,
            );
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x_arg, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        result
    }
}

/// GNU `zapsmall(x, digits = getOption("digits"))`.
pub unsafe fn do_zapsmall(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let digits_arg = CAR(CDR(args));
        let digits = if digits_arg.is_null()
            || digits_arg == R_NilValue()
            || digits_arg == crate::sexp::globals::R_MissingArg()
        {
            crate::mainutils::options::GetOptionDigits() as f64
        } else if XLENGTH(digits_arg) == 0 {
            crate::mainutils::errors::errorcall_str(call, "invalid 'digits'");
        } else {
            real_or_default(digits_arg, 7.0)
        };
        let n = XLENGTH(x);
        if n == 0 {
            return x;
        }
        let rest = CDR(CDR(args));
        let mfun = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            CAR(rest)
        };
        let min_cell = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            CDR(rest)
        };
        let min_d = if min_cell.is_null()
            || min_cell == R_NilValue()
            || CAR(min_cell) == crate::sexp::globals::R_MissingArg()
        {
            0.0
        } else {
            real_or_default(CAR(min_cell), 0.0)
        };
        let ina = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let _ina = protect(ina);
        let mut any_observed = false;
        let t = TYPEOF(x);
        for i in 0..n {
            let missing = if t == SEXPTYPE::CPLXSXP {
                let z = *COMPLEX(x).add(i as usize);
                z.r.is_nan() || z.i.is_nan()
            } else if t == SEXPTYPE::REALSXP {
                (*REAL(x).add(i as usize)).is_nan()
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                *INTEGER(x).add(i as usize) == NA_INTEGER
            } else {
                false
            };
            *INTEGER(ina).add(i as usize) = if missing { 1 } else { 0 };
            if !missing {
                any_observed = true;
            }
        }
        if !any_observed {
            return x;
        }
        let mx = if TYPEOF(mfun) == SEXPTYPE::CLOSXP {
            let mc = crate::sexp::constructors::Rf_lang3(mfun, x, ina);
            let _mc = protect(mc);
            let got = crate::eval::eval::Rf_eval(mc, rho);
            real_or_default(got, 0.0)
        } else {
            let mut mx = 0.0f64;
            for i in 0..n {
                if *INTEGER(ina).add(i as usize) != 0 {
                    continue;
                }
                let mag = if t == SEXPTYPE::CPLXSXP {
                    let z = *COMPLEX(x).add(i as usize);
                    (z.r * z.r + z.i * z.i).sqrt()
                } else if t == SEXPTYPE::REALSXP {
                    (*REAL(x).add(i as usize)).abs()
                } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    (*INTEGER(x).add(i as usize) as f64).abs()
                } else {
                    0.0
                };
                if mag > mx {
                    mx = mag;
                }
            }
            mx
        };
        if mx.is_infinite() && digits.is_infinite() {
            return x;
        }
        let adj = if mx > 0.0 {
            (digits - mx.log10()).max(min_d)
        } else {
            digits
        };
        let d = Rf_ScalarReal(adj);
        let _d = protect(d);
        let packed = Rf_cons(x, Rf_cons(d, R_NilValue()));
        let _p = protect(packed);
        do_round(call, op, packed, rho)
    }
}

/// Port of R's `fround` (r-source/src/nmath/fround.c): round `x` to `digits`
/// decimal digits with ties-to-even. Instead of a naive multiply-round-divide
/// (which double-rounds when `x * 10^dig` is itself inexact), it compares the
/// exact decimal candidates `floor(x*10^dig)/10^dig` and `ceil(x*10^dig)/10^dig`
/// and picks the nearer, breaking ties toward the even candidate.
fn fround(x: f64, digits: f64) -> f64 {
    const MAX_DIGITS: f64 = 323.0; // DBL_MAX_10_EXP + DBL_DIG
    const MAX10E: i32 = 308; // DBL_MAX_10_EXP
    const DBL_DIG: f64 = 15.0;

    if x.is_nan() || digits.is_nan() {
        return x + digits;
    }
    if !x.is_finite() {
        return x;
    }
    if digits > MAX_DIGITS || x == 0.0 {
        return x;
    }
    if digits < -(MAX10E as f64) {
        return 0.0;
    }
    if digits == 0.0 {
        return x.round_ties_even();
    }

    let dig = (digits + 0.5).floor() as i32;
    // 10^k for k>0 is an integer. Dividing by the inexact 10^{-k}
    // makes round(100000/3, -2) miss 33300 by an ulp.
    if dig < 0 {
        let scale = r_pow_di(10.0, -dig);
        let sgn = if x < 0.0 { -1.0 } else { 1.0 };
        return sgn * (x.abs() / scale).round_ties_even() * scale;
    }
    let sgn = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let l10x = std::f64::consts::LOG10_2 * (0.5 + logb(x));
    if l10x + dig as f64 > DBL_DIG {
        // Rounding to so many digits that no rounding is needed.
        return sgn * x;
    }
    let (pow10, p10): (f64, f64);
    let (xd, xu): (f64, f64);
    let i10: f64;
    if dig <= MAX10E {
        pow10 = r_pow_di(10.0, dig);
        p10 = 1.0;
    } else {
        p10 = r_pow_di(10.0, dig - MAX10E);
        pow10 = r_pow_di(10.0, MAX10E);
    }
    let x10 = if dig <= MAX10E {
        x * pow10
    } else {
        (x * pow10) * p10
    };
    i10 = x10.floor();
    if dig <= MAX10E {
        xd = i10 / pow10;
        xu = (i10 + 1.0) / pow10;
    } else {
        xd = i10 / pow10 / p10;
        xu = (i10 + 1.0) / pow10 / p10;
    }
    let du = xu - x;
    let dd = x - xd;
    sgn * (if du < dd || (i10 % 2.0 == 1.0 && du == dd) {
        xu
    } else {
        xd
    })
}

/// Port of R's `R_pow_di`: 10^n by binary exponentiation (matches C exactly).
fn r_pow_di(x: f64, n: i32) -> f64 {
    let mut n = n;
    let mut x = x;
    let mut dev = 1.0;
    if n == 0 {
        return 1.0;
    }
    if n < 0 {
        n = -n;
        x = 1.0 / x;
    }
    while n != 0 {
        if n & 1 != 0 {
            dev *= x;
        }
        x *= x;
        n >>= 1;
    }
    dev
}

/// logb(3): the integral binary exponent of `x` as a double.
fn logb(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return f64::NAN;
    }
    let bits = x.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    if raw_exp == 0 {
        // subnormal: normalize
        let mut m = bits & 0x000f_ffff_ffff_ffff;
        let mut e = -1022;
        while m & 0x0010_0000_0000_0000 == 0 {
            m <<= 1;
            e -= 1;
        }
        e as f64
    } else {
        (raw_exp - 1023) as f64
    }
}

/// R's `signif(x, digits=6)` — round to significant digits.
///
/// Wires through the faithful `fprec` port (r-source/src/nmath/fprec.c),
/// mirroring how `do_round` routes through `fround`.
pub unsafe fn do_signif(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut dispatched = R_NilValue();
        if crate::eval::dispatch::DispatchGroup(
            c"Math".as_ptr(),
            call,
            op,
            args,
            rho,
            &mut dispatched,
        ) != 0
        {
            return dispatched;
        }
        let m = crate::mainutils::match_mod::match_formal_slots(call, args, &["x", "digits"]);
        let missing = crate::sexp::globals::R_MissingArg();
        let force = |v: SEXP| -> SEXP {
            if !v.is_null() && TYPEOF(v) == SEXPTYPE::PROMSXP {
                crate::sexp::envir::forcePromise(v)
            } else {
                v
            }
        };
        let x_arg = force(*m.first().unwrap_or(&R_NilValue()));
        let digits_arg = force(m.get(1).copied().unwrap_or(missing));

        if x_arg.is_null() || x_arg == R_NilValue() || x_arg == missing {
            crate::mainutils::errors::Rf_error(
                b"argument \"x\" is missing, with no default\0".as_ptr() as *const _,
            );
        }
        if TYPEOF(x_arg) == SEXPTYPE::CPLXSXP {
            return math2_complex(call, x_arg, digits_arg, 6.0, "signif", z_prec_r);
        }
        let nx = XLENGTH(x_arg);
        let digits_missing =
            digits_arg.is_null() || digits_arg == R_NilValue() || digits_arg == missing;
        if !digits_missing && XLENGTH(digits_arg) == 0 {
            crate::mainutils::errors::errorcall_str(call, "invalid second argument of length 0");
        }
        if nx == 0 {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
            let _result_guard = protect(result);
            // Empty math2 still keeps x's attributes.
            copy_all_attribs(result, x_arg);
            return result;
        }
        let nd = if digits_missing {
            0
        } else {
            XLENGTH(digits_arg)
        };
        let n = if nd == 0 { nx } else { nx.max(nd) };

        let t = TYPEOF(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let xi = if nx == 0 { 0 } else { i % nx };
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(xi as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(xi as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            let digits = if nd == 0 {
                6.0
            } else if TYPEOF(digits_arg) == SEXPTYPE::INTSXP
                || TYPEOF(digits_arg) == SEXPTYPE::LGLSXP
            {
                // GNU coerces the digits argument with asReal: integer or
                // logical NA becomes NA_real_, which propagates as NA —
                // not the numeric default.
                let iv = *INTEGER(digits_arg).add((i % nd) as usize);
                if iv == crate::sexp::ffi::NA_INTEGER {
                    NA_REAL
                } else {
                    iv as f64
                }
            } else {
                real_elt_or_default(digits_arg, i, 6.0)
            };
            let digits_is_na = digits.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN;
            *dst.add(i as usize) =
                if digits_is_na || v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    NA_REAL
                } else {
                    crate::fprec::fprec(v, digits)
                };
        }
        // GNU FINISH_Math2: SHALLOW_DUPLICATE_ATTRIB from x when the
        // result length matches x, otherwise from digits.
        if n == nx {
            copy_all_attribs(result, x_arg);
        } else if n == nd {
            copy_all_attribs(result, digits_arg);
        }
        result
    }
}

/// R's `trunc(x, ...)` — truncate toward zero with digits support.
pub unsafe fn do_trunc(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut dispatched = R_NilValue();
        if crate::eval::dispatch::DispatchGroup(
            c"Math".as_ptr(),
            call,
            op,
            args,
            rho,
            &mut dispatched,
        ) != 0
        {
            return dispatched;
        }
        if crate::mainutils::objects::inherits2(CAR(args), c"Date".as_ptr()) != 0 {
            return crate::mainutils::essentials::do_trunc_Date(call, op, args, rho);
        }
        let x_arg = CAR(args);
        let _digits_arg = CAR(CDR(args));

        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                NA_REAL
            } else {
                v.trunc()
            };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Math/Statistics
// ---------------------------------------------------------------------------
