//! Elementwise math: %in%, real_math1 table, sinpi/cospi family, trigonometric builtins — extracted verbatim from the former single-file module.
use super::*;

/// R's `%in%` operator — match operator, upstream
/// `function(x, table) match(x, table, nomatch = 0) > 0`: an empty table
/// yields FALSE for every element of `x` (never a zero-length result).
pub unsafe fn do_in_operator(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let table = CAR(CDR(args));

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = LOGICAL(result);

        let table_empty = table.is_null() || table == R_NilValue() || XLENGTH(table) == 0;
        for i in 0..n {
            let found = if table_empty {
                false
            } else {
                let elem = elt_to_string(x, i);
                let table_len = XLENGTH(table);
                let mut found = false;
                for j in 0..table_len {
                    let tbl_elem = elt_to_string(table, j);
                    if elem == tbl_elem {
                        found = true;
                        break;
                    }
                }
                found
            };
            *dst.add(i as usize) = if found { TRUE } else { FALSE };
        }
        result
    }
}

unsafe fn dispatch_math(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> Option<SEXP> {
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
            Some(dispatched)
        } else {
            None
        }
    }
}

pub unsafe fn real_math1(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
    f: impl Fn(f64) -> f64,
) -> SEXP {
    unsafe {
        if let Some(dispatched) = dispatch_math(call, op, args, rho) {
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
                if v == NA_INTEGER {
                    NA_REAL
                } else {
                    v as f64
                }
            } else {
                NA_REAL
            };

            let out = if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                NA_REAL
            } else {
                f(val)
            };
            if val.is_finite() && out.is_nan() {
                naflag = true;
            }
            *dst.add(i as usize) = out;
        }
        if naflag {
            crate::mainutils::errors::Rf_warningcall1(call, c"NaNs produced".as_ptr());
        }
        result
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe extern "C" {
    fn __sinpi(x: f64) -> f64;
    fn __cospi(x: f64) -> f64;
    fn __tanpi(x: f64) -> f64;
}

/// GNU `sinpi`: reduce modulo 2, then the platform's `sinpi` when it exists.
pub fn sinpi_value(x: f64) -> f64 {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        return unsafe { __sinpi(x) };
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        if x.is_nan() {
            return x;
        }
        if !x.is_finite() {
            return f64::NAN;
        }
        let mut x = x % 2.0;
        if x <= -1.0 {
            x += 2.0;
        } else if x > 1.0 {
            x -= 2.0;
        }
        if x == 0.0 || x == 1.0 {
            0.0
        } else if x == 0.5 {
            1.0
        } else if x == -0.5 {
            -1.0
        } else {
            (std::f64::consts::PI * x).sin()
        }
    }
}

/// GNU `cospi`.
pub fn cospi_value(x: f64) -> f64 {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        return unsafe { __cospi(x) };
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        if x.is_nan() {
            return x;
        }
        if !x.is_finite() {
            return f64::NAN;
        }
        let x = x.abs() % 2.0;
        if x % 1.0 == 0.5 {
            0.0
        } else if x == 1.0 {
            -1.0
        } else if x == 0.0 {
            1.0
        } else {
            (std::f64::consts::PI * x).cos()
        }
    }
}

/// GNU `tanpi`. Half-integers are NaN.
/// GNU `Rtanpi`. Half-integers are NaN, not the Inf `__tanpi` returns.
pub fn tanpi_value(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if !x.is_finite() {
        return f64::NAN;
    }
    let mut reduced = x % 1.0;
    if reduced <= -0.5 {
        reduced += 1.0;
    } else if reduced > 0.5 {
        reduced -= 1.0;
    }
    if reduced == 0.0 {
        return 0.0;
    }
    if reduced == 0.5 {
        return f64::NAN;
    }
    if reduced == 0.25 {
        return 1.0;
    }
    if reduced == -0.25 {
        return -1.0;
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        return unsafe { __tanpi(x) };
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        (std::f64::consts::PI * reduced).tan()
    }
}

/// R's `expm1(x)` — accurate exp(x)-1.
pub unsafe fn do_expm1(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { real_math1(call, op, args, rho, f64::exp_m1) }
}

/// R's `log1p(x)` — accurate log(1+x).
pub unsafe fn do_log1p(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { real_math1(call, op, args, rho, f64::ln_1p) }
}

/// R's `acosh(x)` — inverse hyperbolic cosine.
pub unsafe fn do_acosh(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(dispatched) = dispatch_math(call, op, args, rho) {
            return dispatched;
        }
        let x = CAR(args);
        if !x.is_null() && x != R_NilValue() && TYPEOF(x) == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_acosh,
            );
        }
        real_math1(call, op, args, rho, f64::acosh)
    }
}

/// R's `asinh(x)` — inverse hyperbolic sine.
pub unsafe fn do_asinh(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(dispatched) = dispatch_math(call, op, args, rho) {
            return dispatched;
        }
        let x = CAR(args);
        if !x.is_null() && x != R_NilValue() && TYPEOF(x) == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_asinh,
            );
        }
        real_math1(call, op, args, rho, f64::asinh)
    }
}

/// R's `atanh(x)` — inverse hyperbolic tangent.
pub unsafe fn do_atanh(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(dispatched) = dispatch_math(call, op, args, rho) {
            return dispatched;
        }
        let x = CAR(args);
        if !x.is_null() && x != R_NilValue() && TYPEOF(x) == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_atanh,
            );
        }
        real_math1(call, op, args, rho, f64::atanh)
    }
}

/// R's `sinpi(x)` — sin(pi*x), exact at integer arguments.
pub unsafe fn do_sinpi(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { real_math1(call, op, args, rho, sinpi_value) }
}

/// R's `cospi(x)` — cos(pi*x), exact at integer and half-integer arguments.
pub unsafe fn do_cospi(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { real_math1(call, op, args, rho, cospi_value) }
}

/// R's `tanpi(x)` — tan(pi*x).
pub unsafe fn do_tanpi(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { real_math1(call, op, args, rho, tanpi_value) }
}

/// R's `sin(x)` — sine function.
pub unsafe fn do_sin(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            crate::mainutils::errors::errorcall_str(
                call,
                "0 arguments passed to 'sin' which requires 1",
            );
        }
        if let Some(dispatched) = dispatch_math(call, op, args, rho) {
            return dispatched;
        }
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            crate::mainutils::errors::errorcall_str(
                call,
                "non-numeric argument to mathematical function",
            );
        }


        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_sin,
            );
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

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
                *dst.add(i as usize) = val.sin();
            }
        }
        result
    }
}

/// R's `cos(x)` — cosine function.
pub unsafe fn do_cos(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(dispatched) = dispatch_math(call, op, args, rho) {
            return dispatched;
        }
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {

            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_cos,
            );
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

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
                *dst.add(i as usize) = val.cos();
            }
        }
        result
    }
}

/// R's `tan(x)` — tangent function.
pub unsafe fn do_tan(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(dispatched) = dispatch_math(call, op, args, rho) {
            return dispatched;
        }
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_tan,
            );
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

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
                *dst.add(i as usize) = val.tan();
            }
        }
        result
    }
}

/// R's `asin(x)` — arc sine function.
pub unsafe fn do_asin(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(dispatched) = dispatch_math(call, op, args, rho) {
            return dispatched;
        }
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }


        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_asin,
            );
        }


        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

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
                *dst.add(i as usize) = val.asin();
            }
        }
        result
    }
}

/// R's `acos(x)` — arc cosine function.
pub unsafe fn do_acos(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(dispatched) = dispatch_math(call, op, args, rho) {
            return dispatched;
        }

        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_acos,
            );
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

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
                *dst.add(i as usize) = val.acos();
            }
        }
        result
    }
}

/// R's `atan(x)` — arc tangent function.
pub unsafe fn do_atan(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(dispatched) = dispatch_math(call, op, args, rho) {
            return dispatched;
        }

        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_atan,
            );
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

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
                *dst.add(i as usize) = val.atan();
            }
        }
        result
    }
}

/// R's `atan2(y, x)` — two-argument arc tangent function.
pub unsafe fn do_atan2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(args);
        let x = CAR(CDR(args));

        if y.is_null() || y == R_NilValue() || x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(y).max(XLENGTH(x));
        let ty = TYPEOF(y);
        let tx = TYPEOF(x);
        if ty == SEXPTYPE::CPLXSXP || tx == SEXPTYPE::CPLXSXP {
            let y_c = crate::eval::complex_arith::coerce_to_complex(y);
            let _y_guard = protect(y_c);
            let x_c = crate::eval::complex_arith::coerce_to_complex(x);
            let _x_guard = protect(x_c);
            let ny = XLENGTH(y_c);
            let nx = XLENGTH(x_c);
            let n = ny.max(nx);
            let result = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _r_guard = protect(result);
            let dst = crate::sexp::accessors::COMPLEX(result);
            let ys = crate::sexp::accessors::COMPLEX(y_c);
            let xs = crate::sexp::accessors::COMPLEX(x_c);
            for i in 0..n {
                let yi = if ny > 0 { i % ny } else { 0 };
                let xi = if nx > 0 { i % nx } else { 0 };
                crate::mainutils::complex_cmath::z_atan2(
                    &mut *dst.add(i as usize),
                    &*ys.add(yi as usize),
                    &*xs.add(xi as usize),
                );
            }
            return result;
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let y_len = XLENGTH(y);
            let x_len = XLENGTH(x);
            let yi = if y_len > 0 { i % y_len } else { 0 };
            let xi = if x_len > 0 { i % x_len } else { 0 };

            let val_y = if ty == SEXPTYPE::REALSXP {
                *REAL(y).add(yi as usize)
            } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
                let v = *INTEGER(y).add(yi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            let val_x = if tx == SEXPTYPE::REALSXP {
                *REAL(x).add(xi as usize)
            } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(xi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val_y.atan2(val_x);
            }
        }
        result
    }
}
