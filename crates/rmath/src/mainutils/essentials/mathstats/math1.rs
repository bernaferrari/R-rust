//! Math1 builtins (abs..bessel), charToRaw/rawToChar — extracted verbatim from essentials/mathstats.rs.

use super::*;

/// Checked original-owner native boundary for GNU `charToRaw(x)`.
pub unsafe fn do_charToRaw(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use crate::sexp::owner::OwnerToken;
    let owner = unsafe { OwnerToken::current() }
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let _pin = owner
        .pin()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let arguments = owner
        .sexp(args)
        .and_then(crate::sexp::object::Sexp::into_owned)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let call = owner
        .sexp(call)
        .and_then(crate::sexp::object::Sexp::into_owned)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let weak = owner
        .weak_owner()
        .unwrap_or_else(|| crate::sexp::context::r_error("charToRaw requires a managed runtime"));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::sexp::owner::with_runtime(&weak, |access| {
            char_to_raw::evaluate(access, arguments, call)
        })
    }));
    owner
        .require_active()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    match outcome {
        Ok(Ok(Ok(value))) => value.as_raw(),
        Ok(Ok(Err(crate::sexp::object::SexpError::EvaluationFailed { message }))) => {
            crate::sexp::context::r_error(message)
        }
        Ok(Ok(Err(error))) | Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
#[path = "char_to_raw.rs"]
mod char_to_raw;

/// GNU `rawToChar(x)` copies raw bytes, stripping trailing nuls.
pub unsafe fn do_rawToChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use crate::sexp::{
        object::Sexp,
        owner::{OwnerToken, with_runtime},
    };
    let owner = unsafe { OwnerToken::current() }
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let _pin = owner
        .pin()
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let args = owner
        .sexp(args)
        .and_then(Sexp::into_owned)
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let weak = owner
        .weak_owner()
        .unwrap_or_else(|| crate::sexp::context::r_error("rawToChar requires a managed runtime"));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&weak, |access| {
            raw_to_char::evaluate(access, args, |value| {
                access.with_native(|original| {
                    let flag = unsafe { crate::mainutils::coerce::asLogical(value.as_raw()) };
                    original.require_active()?;
                    Ok(flag)
                })
            })
        })
    }));
    owner
        .require_active()
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    match outcome {
        Ok(Ok(Ok(value))) => value.as_raw(),
        Ok(Ok(Err(crate::sexp::diagnostic::EvaluationError::Diagnostic(diagnostic)))) => {
            crate::mainutils::errors::raise_owned_diagnostic(&owner, diagnostic)
        }
        Ok(Ok(Err(error))) => crate::sexp::context::r_error(error.to_string()),
        Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
#[path = "raw_to_char.rs"]
mod raw_to_char;

// ---------------------------------------------------------------------------
// do_abs — absolute value
// ---------------------------------------------------------------------------

/// R's `abs(x)` — absolute value of numeric vector.
///
/// Preserves integer/logical inputs as integer vectors and real inputs as
/// real vectors. Non-numeric arguments (and factors, via Math.factor)
/// error like stock Math1.
unsafe fn math_factor_error(call: SEXP, name: &str) -> ! {
    unsafe {
        let method_call = crate::sexp::constructors::Rf_lang2(
            Rf_install(c"Math.factor".as_ptr()),
            CAR(CDR(call)),
        );
        let _guard = protect(method_call);
        crate::mainutils::errors::errorcall_str(
            method_call,
            &format!("'{name}' not meaningful for factors"),
        );
    }
}

fn math_nonnum_error() -> ! {
    std::panic::panic_any(RError {
        message: "non-numeric argument to mathematical function".to_string(),
    })
}

/// GNU math1: SHALLOW_DUPLICATE_ATTRIB copies dim/dimnames/names.
unsafe fn math1_copy_dim_and_names(src: SEXP, dst: SEXP) {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(src, crate::sexp::attrib_core::R_DimSymbol());
        if !dim.is_null() && dim != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(dst, crate::sexp::attrib_core::R_DimSymbol(), dim);
            let dn = crate::sexp::attrib_core::getAttrib(
                src,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
            );
            if !dn.is_null() && dn != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    dst,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    dn,
                );
            }
        } else {
            let names =
                crate::sexp::attrib_core::getAttrib(src, crate::sexp::attrib_core::R_NamesSymbol());
            if !names.is_null() && names != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    dst,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
        }
    }
}

pub unsafe fn do_abs(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
        if x_arg.is_null() {
            return R_NilValue();
        }
        if x_arg == R_NilValue() {
            // stock Math1: NULL is not numeric and errors
            math_nonnum_error();
        }
        if sexp_has_class(x_arg, "factor") {
            math_factor_error(call, "abs");
        }
        let t = TYPEOF(x_arg);
        if t == SEXPTYPE::CPLXSXP {
            let result = crate::eval::complex_arith::complex_abs_vec(x_arg);
            if !result.is_null() && result != R_NilValue() {
                crate::mainutils::array::copyMostAttrib(x_arg, result);
                math1_copy_dim_and_names(x_arg, result);
            }
            return result;
        }
        if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
            math_nonnum_error();
        }
        let n = XLENGTH(x_arg);
        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = INTEGER(result);
            for i in 0..n {
                let value = *INTEGER(x_arg).add(i as usize);
                *dst.add(i as usize) = if value == NA_INTEGER || value == c_int::MIN {
                    NA_INTEGER
                } else {
                    value.abs()
                };
            }
            crate::mainutils::array::copyMostAttrib(x_arg, result);
            math1_copy_dim_and_names(x_arg, result);
            return result;
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = *REAL(x_arg).add(i as usize);
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                v
            } else {
                v.abs()
            };
        }
        crate::mainutils::array::copyMostAttrib(x_arg, result);
        math1_copy_dim_and_names(x_arg, result);
        result
    }
}

// ---------------------------------------------------------------------------
// do_sign — sign of values
// ---------------------------------------------------------------------------

/// R's `sign(x)` — sign of numeric vector (-1, 0, or 1).
///
/// Returns REALSXP. Preserves NA and NaN.
pub unsafe fn do_sign(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
        if x_arg.is_null() {
            return R_NilValue();
        }
        if x_arg == R_NilValue() {
            // stock Math1: NULL is not numeric and errors
            math_nonnum_error();
        }
        if sexp_has_class(x_arg, "factor") {
            math_factor_error(call, "sign");
        }
        let t = TYPEOF(x_arg);
        if t == SEXPTYPE::CPLXSXP {
            std::panic::panic_any(RError {
                message: "unimplemented complex function".to_string(),
            });
        }
        if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
            math_nonnum_error();
        }
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            };
            *dst.add(i as usize) = if v.is_nan() {
                v // preserve NaN/NA
            } else if v == 0.0 {
                0.0
            } else if v > 0.0 {
                1.0
            } else {
                -1.0
            };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete special functions for libRmath coverage
// ---------------------------------------------------------------------------

/// Helper to apply a scalar function to a numeric vector, preserving NA/NaN.
/// Returns REALSXP.
unsafe fn apply_unary_scalar_fn(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
    scalar_fn: impl Fn(f64) -> f64,
) -> SEXP {
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
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        let mut naflag = false;
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                let out = scalar_fn(val);
                *dst.add(i as usize) = out;
                if out.is_nan() && !val.is_nan() {
                    naflag = true;
                }
            }
        }
        if naflag {
            crate::mainutils::errors::Rf_warningcall1(call, c"NaNs produced".as_ptr());
        }
        result
    }
}

/// Helper to apply a binary scalar function to two numeric vectors with recycling.
/// Returns REALSXP.
unsafe fn apply_binary_scalar_fn(
    call: SEXP,
    x: SEXP,
    y: SEXP,
    scalar_fn: impl Fn(f64, f64) -> f64,
) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return R_NilValue();
        }
        let x_len = XLENGTH(x);
        let y_len = XLENGTH(y);
        let n = if x_len == 0 || y_len == 0 {
            0
        } else {
            x_len.max(y_len)
        };
        let tx = TYPEOF(x);
        let ty = TYPEOF(y);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        if x_len == 0 || y_len == 0 {
            if x_len == 0 {
                copy_all_attribs(result, x);
            }
            return result;
        }
        let dst = REAL(result);
        let mut naflag = false;
        for i in 0..n {
            let xi = if x_len > 0 { i % x_len } else { 0 };
            let yi = if y_len > 0 { i % y_len } else { 0 };
            let val_x = if tx == SEXPTYPE::REALSXP {
                *REAL(x).add(xi as usize)
            } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(xi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            let val_y = if ty == SEXPTYPE::REALSXP {
                *REAL(y).add(yi as usize)
            } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
                let v = *INTEGER(y).add(yi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if crate::sexp::ffi::R_IsNA(val_x) || crate::sexp::ffi::R_IsNA(val_y) {
                *dst.add(i as usize) = NA_REAL;
            } else if val_x.is_nan() || val_y.is_nan() {
                // GNU Math2 skips the scalar kernel for missing operands.
                // In particular a NaN psigamma derivative must not cast to0.
                *dst.add(i as usize) = f64::NAN;
            } else {
                let out = scalar_fn(val_x, val_y);
                *dst.add(i as usize) = out;
                if out.is_nan() && !val_x.is_nan() && !val_y.is_nan() {
                    naflag = true;
                }
            }
        }
        if naflag {
            crate::mainutils::errors::Rf_warningcall1(call, c"NaNs produced".as_ptr());
        }
        if n == x_len {
            copy_all_attribs(result, x);
        } else if n == y_len {
            copy_all_attribs(result, y);
        }
        result
    }
}

pub unsafe fn do_lgamma(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(call, op, args, rho, crate::special::gamma::lgammafn) }
}

/// R's `gamma(x)` — gamma function.
pub unsafe fn do_gamma(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(call, op, args, rho, crate::special::gamma::gammafn) }
}

/// R's `digamma(x)` — digamma (psi) function.
pub unsafe fn do_digamma(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(call, op, args, rho, crate::special::polygamma::digamma) }
}

/// R's `trigamma(x)` — trigamma function.
pub unsafe fn do_trigamma(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(call, op, args, rho, crate::special::polygamma::trigamma) }
}

/// R's `psigamma(x, deriv)` — polygamma function (deriv-th derivative of psi).
pub unsafe fn do_psigamma(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let deriv_arg = CAR(CDR(args));
        if let Err(message) = validate_math2_inputs(x, deriv_arg) {
            crate::mainutils::errors::errorcall_str(call, message);
        }
        checked_math2(call, x, deriv_arg, choose_checked::Operation::PsiGamma)
    }
}

/// R's `beta(a, b)` — beta function.
pub unsafe fn do_beta(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let a = CAR(args);
        let b = CAR(CDR(args));
        if a.is_null() || a == R_NilValue() || b.is_null() || b == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(a) == SEXPTYPE::CPLXSXP || TYPEOF(b) == SEXPTYPE::CPLXSXP {
            std::panic::panic_any(RError {
                message: "unimplemented complex function".to_string(),
            });
        }
        apply_binary_scalar_fn(call, a, b, |x, y| crate::special::lbeta::lbeta(x, y).exp())
    }
}

/// R's `lbeta(a, b)` — log beta function.
pub unsafe fn do_lbeta(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let a = CAR(args);
        let b = CAR(CDR(args));
        if a.is_null() || a == R_NilValue() || b.is_null() || b == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(a) == SEXPTYPE::CPLXSXP || TYPEOF(b) == SEXPTYPE::CPLXSXP {
            std::panic::panic_any(RError {
                message: "unimplemented complex function".to_string(),
            });
        }
        apply_binary_scalar_fn(call, a, b, crate::special::lbeta::lbeta)
    }
}

// GNU do_math2 dispatches complex first arguments before Math2 validation.
// A complex second argument instead fails the ordinary numeric-type check.
// Both checks precede the empty-vector shortcut.
unsafe fn validate_math2_inputs(a: SEXP, b: SEXP) -> Result<(), &'static str> {
    unsafe {
        if !a.is_null() && TYPEOF(a) == SEXPTYPE::CPLXSXP {
            return Err("unimplemented complex function");
        }
        for value in [a, b] {
            if value.is_null() {
                return Err("non-numeric argument to mathematical function");
            }
            let kind = TYPEOF(value);
            if !(kind == SEXPTYPE::LGLSXP || kind == SEXPTYPE::INTSXP || kind == SEXPTYPE::REALSXP)
            {
                return Err("non-numeric argument to mathematical function");
            }
        }
        Ok(())
    }
}

#[path = "bessel_scaling.rs"]
mod bessel_scaling;
#[path = "bessel_workspace.rs"]
mod bessel_workspace;
#[path = "choose_checked.rs"]
mod choose_checked;
#[path = "choose_factor.rs"]
mod choose_factor;

unsafe fn checked_math2(
    call: SEXP,
    a: SEXP,
    b: SEXP,
    operation: choose_checked::Operation,
) -> SEXP {
    unsafe { checked_math(call, a, b, None, operation, None) }
}
unsafe fn checked_math(
    call: SEXP,
    a: SEXP,
    b: SEXP,
    c: Option<SEXP>,
    operation: choose_checked::Operation,
    environment: Option<SEXP>,
) -> SEXP {
    use crate::sexp::{object::Sexp, owner::OwnerToken};
    let owner = unsafe { OwnerToken::current() }
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let active = || {
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
    };
    let _pin = owner
        .pin()
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let root = |raw| {
        owner
            .sexp(raw)
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
    };
    let call = root(call);
    let mut a = root(a);
    let mut b = root(b);
    let mut c = c.map(root);
    let environment = environment.map(root);
    active();
    if let Some(environment) = environment {
        let conversion = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            bessel_scaling::prepare(owner, c.take(), environment)
        }));
        active();
        c = Some(match conversion {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
            Err(payload) => std::panic::resume_unwind(payload),
        });
    }
    if matches!(
        operation,
        choose_checked::Operation::BesselK | choose_checked::Operation::BesselI
    ) {
        for value in [&a, &b].into_iter().chain(c.iter()) {
            if !matches!(
                value.typeof_(),
                SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
            ) {
                unsafe {
                    crate::mainutils::errors::errorcall_str(
                        call.as_raw(),
                        "non-numeric argument to mathematical function",
                    )
                }
            }
        }
    }
    let weak = owner
        .weak_owner()
        .unwrap_or_else(|| crate::sexp::context::r_error("choose requires a managed runtime"));
    let validation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::sexp::owner::with_runtime(&weak, |access| {
            if choose_factor::has_factor(access, &a, &b)? {
                return Ok(true);
            }
            if let Some(c) = &c {
                choose_factor::has_factor(access, c, c)
            } else {
                Ok(false)
            }
        })
    }));
    active();
    match validation {
        Ok(Ok(Ok(false))) => {}
        Ok(Ok(Ok(true))) => unsafe {
            crate::mainutils::errors::errorcall_str(
                call.as_raw(),
                "non-numeric argument to mathematical function",
            )
        },
        Ok(Ok(Err(error))) | Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
        Err(payload) => std::panic::resume_unwind(payload),
    }
    if !a.is_empty() && !b.is_empty() && c.as_ref().is_none_or(|c| !c.is_empty()) {
        let raw =
            unsafe { crate::main::coerce::coerceVector(a.as_raw(), SEXPTYPE::REALSXP.as_c_int()) };
        active();
        a = root(raw);
        let raw =
            unsafe { crate::main::coerce::coerceVector(b.as_raw(), SEXPTYPE::REALSXP.as_c_int()) };
        active();
        b = root(raw);
        if let Some(value) = &mut c {
            let raw = unsafe {
                crate::main::coerce::coerceVector(value.as_raw(), SEXPTYPE::REALSXP.as_c_int())
            };
            active();
            *value = root(raw);
        }
    }
    let _warning_call = crate::mainutils::errors::mathlib_warning_call_guard(call.as_raw());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::sexp::owner::with_runtime(&weak, |access| {
            choose_checked::evaluate(access, a, b, c, operation)
        })
    }));
    active();
    let (output, source) = match outcome {
        Ok(Ok(Ok(value))) => value,
        Ok(Ok(Err(error))) | Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
        Err(payload) => std::panic::resume_unwind(payload),
    };
    if let Some(source) = source {
        // Root the exact attribute graph before duplication can collect/reenter.
        let attributes = source
            .try_attrib()
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let copied = unsafe { crate::mainutils::duplicate::shallow_duplicate(attributes.as_raw()) };
        active();
        let copied = root(copied);
        unsafe {
            SET_ATTRIB(output.as_raw(), copied.as_raw());
            SET_OBJECT(output.as_raw(), OBJECT(source.as_raw()));
            if crate::mainutils::coerce::IS_S4_OBJECT(source.as_raw()) != 0 {
                SET_S4_OBJECT(output.as_raw());
            } else {
                UNSET_S4_OBJECT(output.as_raw());
            }
        }
        active();
    }
    output.as_raw()
}

/// R's `choose(n, k)` — binomial coefficient.
pub unsafe fn do_choose(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let k_arg = CAR(CDR(args));
        if let Err(message) = validate_math2_inputs(n_arg, k_arg) {
            crate::mainutils::errors::errorcall_str(call, message);
        }
        checked_math2(call, n_arg, k_arg, choose_checked::Operation::Choose)
    }
}

/// R's `lchoose(n, k)` — log of absolute value of binomial coefficient.
pub unsafe fn do_lchoose(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let k_arg = CAR(CDR(args));
        if let Err(message) = validate_math2_inputs(n_arg, k_arg) {
            crate::mainutils::errors::errorcall_str(call, message);
        }
        checked_math2(call, n_arg, k_arg, choose_checked::Operation::LogChoose)
    }
}

/// R's `factorial(n)` — factorial n!
pub unsafe fn do_factorial(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        apply_unary_scalar_fn(call, op, args, rho, |v| {
            crate::special::gamma::gammafn(v + 1.0)
        })
    }
}

/// R's `lfactorial(n)` — log factorial.
pub unsafe fn do_lfactorial(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        apply_unary_scalar_fn(call, op, args, rho, |v| {
            crate::special::gamma::lgammafn(v + 1.0)
        })
    }
}

/// R's `besselI(x, nu)` — modified Bessel function of the first kind.
pub unsafe fn do_besselI(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        bessel_scaled_entry(
            call,
            op,
            args,
            rho,
            true,
            choose_checked::Operation::BesselI,
        )
    }
}
pub unsafe fn do_besselI_internal(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        bessel_scaled_entry(
            call,
            op,
            args,
            rho,
            false,
            choose_checked::Operation::BesselI,
        )
    }
}

/// R's `besselJ(x, nu)` — Bessel function of the first kind.
pub unsafe fn do_besselJ(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let a = CAR(args);
        let b = CAR(CDR(args));
        if let Err(message) = validate_math2_inputs(a, b) {
            crate::mainutils::errors::errorcall_str(call, message);
        }
        checked_math2(call, a, b, choose_checked::Operation::BesselJ)
    }
}

/// R's `besselK(x, nu)` — modified Bessel function of the second kind.
pub unsafe fn do_besselK(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        bessel_scaled_entry(
            call,
            op,
            args,
            rho,
            true,
            choose_checked::Operation::BesselK,
        )
    }
}
pub unsafe fn do_besselK_internal(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        bessel_scaled_entry(
            call,
            op,
            args,
            rho,
            false,
            choose_checked::Operation::BesselK,
        )
    }
}
unsafe fn bessel_scaled_entry(
    call: SEXP,
    _op: SEXP,
    args: SEXP,
    rho: SEXP,
    public: bool,
    operation: choose_checked::Operation,
) -> SEXP {
    use crate::sexp::{object::Sexp, owner::OwnerToken};
    let owner = unsafe { OwnerToken::current() }
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let _pin = owner
        .pin()
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let root = |raw| {
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let value = owner
            .sexp(raw)
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        value
    };
    // Validate every entry object before traversing even the first argument.
    let arguments = root(args);
    let call = root(call);
    let environment = root(rho);
    owner
        .require_active()
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    let read = |value: Result<Sexp<'_>, crate::sexp::object::SexpError>| {
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let value = value
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        value
    };
    let a = read(arguments.try_car());
    let tail = read(arguments.try_cdr());
    let b = read(tail.try_car());
    let tail = read(tail.try_cdr());
    let c = if tail.is_nil() {
        None
    } else {
        Some(read(tail.try_car()))
    };
    if !public && c.is_none() {
        crate::sexp::context::r_error(match operation {
            choose_checked::Operation::BesselI => "missing scaling argument to internal besselI",
            _ => "missing scaling argument to internal besselK",
        })
    }
    unsafe {
        checked_math(
            call.as_raw(),
            a.as_raw(),
            b.as_raw(),
            c.as_ref().map(Sexp::as_raw),
            operation,
            public.then_some(environment.as_raw()),
        )
    }
}

/// R's `besselY(x, nu)` — Bessel function of the second kind.
pub unsafe fn do_besselY(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let a = CAR(args);
        let b = CAR(CDR(args));
        if let Err(message) = validate_math2_inputs(a, b) {
            crate::mainutils::errors::errorcall_str(call, message);
        }
        checked_math2(call, a, b, choose_checked::Operation::BesselY)
    }
}

// ---------------------------------------------------------------------------
// Final additions: commonly used missing functions
// ---------------------------------------------------------------------------

#[cfg(test)]
mod choose_validation_tests {
    use super::*;

    #[test]
    fn argument_validation_precedes_empty_and_complex_paths() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let numeric = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
            let _numeric = protect(numeric);
            let logical = Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
            let _logical = protect(logical);
            let complex = Rf_allocVector3(SEXPTYPE::CPLXSXP, 0);
            let _complex = protect(complex);
            let text = Rf_mkString(c"5".as_ptr());
            let _text = protect(text);
            crate::sexp::gengc::full_gc();
            assert_eq!(validate_math2_inputs(numeric, logical), Ok(()));
            for invalid in [R_NilValue(), text] {
                assert_eq!(
                    validate_math2_inputs(invalid, complex),
                    Err("non-numeric argument to mathematical function")
                );
                assert_eq!(
                    validate_math2_inputs(complex, invalid),
                    Err("unimplemented complex function")
                );
                assert_eq!(
                    validate_math2_inputs(numeric, invalid),
                    Err("non-numeric argument to mathematical function")
                );
            }
            assert_eq!(
                validate_math2_inputs(complex, numeric),
                Err("unimplemented complex function")
            );
        });
    }
}

#[cfg(test)]
#[path = "choose_checked_tests.rs"]
mod choose_checked_tests;

#[cfg(test)]
#[path = "choose_factor_tests.rs"]
mod choose_factor_tests;

#[cfg(test)]
#[path = "math2_nan_tests.rs"]
mod math2_nan_tests;
