use super::*;

/// stock `math2(x, base, logbase)`: `log(x)/log(base)` with Math2
/// recycling, zero-length handling, attributes, and NaN warnings.
pub(crate) unsafe fn log_with_base(call: SEXP, sx: SEXP, sbase: SEXP) -> SEXP {
    unsafe {
        let x = NumericVector::from_raw(sx);
        let base = NumericVector::from_raw(sbase);
        let (Some(x), Some(base)) = (x, base) else {
            return R_NilValue();
        };
        let nx = x.clone().len();
        let nb = base.clone().len();
        if nx == 0 {
            // SETUP_Math2: empty x gives an empty result with x's attributes
            let empty = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
            if empty.is_null() {
                return R_NilValue();
            }
            copy_all_attrib(empty, sx);
            return empty;
        }
        let n = nx.max(nb);
        let result_raw = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        warn_if_non_multiple_recycling(nx, nb);
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);
        let mut naflag = false;
        for i in 0..n {
            let xv = x.clone().real_at(i % nx);
            let bv = base.clone().real_at(i % nb);
            // if_NA_Math2_set: NA in -> NA, NaN in -> NaN
            let out = if xv.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || bv.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                crate::sexp::ffi::NA_REAL
            } else if xv.is_nan() || bv.is_nan() {
                f64::NAN
            } else {
                let v = libm::log(xv) / libm::log(bv);
                if v.is_nan() {
                    naflag = true;
                }
                v
            };
            result_mut.set_real_elt(i, out);
        }
        let _ = result_mut.freeze();
        if naflag {
            crate::mainutils::errors::Rf_warningcall1(call, c"NaNs produced".as_ptr());
        }
        // FINISH_Math2: attributes from the first operand of length n
        if n == nx {
            copy_all_attrib(result_raw, sx);
        } else if n == nb {
            copy_all_attrib(result_raw, sbase);
        }
        result_raw
    }
}

/// stock complex_math2 `logbase`: `Clog(x) / Clog(base)` element-wise.
pub unsafe fn complex_log_with_base(sx: SEXP, sbase: SEXP) -> SEXP {
    unsafe {
        let as_cplx = |s: SEXP| -> SEXP {
            if TYPEOF(s) == SEXPTYPE::CPLXSXP {
                return s;
            }
            let n = XLENGTH(s);
            let out = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
            let dst = crate::sexp::accessors::COMPLEX(out);
            for i in 0..n {
                let r = if TYPEOF(s) == SEXPTYPE::REALSXP {
                    *crate::sexp::accessors::REAL(s).add(i as usize)
                } else if TYPEOF(s) == SEXPTYPE::INTSXP || TYPEOF(s) == SEXPTYPE::LGLSXP {
                    let iv = *crate::sexp::accessors::INTEGER(s).add(i as usize);
                    if iv == crate::sexp::ffi::NA_INTEGER {
                        crate::sexp::ffi::NA_REAL
                    } else {
                        iv as f64
                    }
                } else {
                    crate::sexp::ffi::NA_REAL
                };
                *dst.add(i as usize) = Rcomplex { r, i: 0.0 };
            }
            out
        };
        let cx = as_cplx(sx);
        let _cx = protect(cx);
        let cb = as_cplx(sbase);
        let _cb = protect(cb);
        let lx = crate::eval::complex_arith::complex_unary_vec(
            cx,
            crate::eval::complex_arith::complex_log,
        );
        let _lx_guard = protect(lx);
        let lb = crate::eval::complex_arith::complex_unary_vec(
            cb,
            crate::eval::complex_arith::complex_log,
        );
        let _lb_guard = protect(lb);
        crate::eval::complex_arith::complex_binary("/", lx, lb)
    }
}

/// SHALLOW_DUPLICATE_ATTRIB: copy every attribute of `src` onto `dst`.
pub(crate) unsafe fn copy_all_attrib(dst: SEXP, src: SEXP) {
    unsafe {
        if src.is_null() || src == R_NilValue() {
            return;
        }
        let mut attr = crate::sexp::accessors::ATTRIB(src);
        while !attr.is_null() && attr != R_NilValue() {
            setAttrib(dst, TAG(attr), CAR(attr));
            attr = CDR(attr);
        }
    }
}

/// Handle unary math functions: abs, sqrt, log, log2, log10, exp,
/// ceiling, floor, trunc, round, sign.
pub unsafe fn do_math1(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            let op_name = get_op_name(op, call);
            arithmetic_error(format!(
                "0 arguments passed to '{op_name}' which requires 1"
            ));
        }
        crate::mainutils::seq::check1arg(args, call, c"x".as_ptr());
        if let Some(result) = try_group_dispatch(b"Math\0", call, op, args, rho) {
            return result;
        }
        let op_name = get_op_name(op, call);
        let x = CAR(args);
        if x == R_NilValue() {
            // stock Math1: NULL is not numeric and errors
            arithmetic_error("non-numeric argument to mathematical function");
        }
        if has_class(x, "factor") {
            // stock Math.factor stops, attributed to the method call
            let f = Rf_install(c"Math.factor".as_ptr());
            let method_call = Rf_lang2(f, CAR(CDR(call)));
            let _method_guard = protect(method_call);
            crate::mainutils::errors::errorcall_str(
                method_call,
                &format!("'{op_name}' not meaningful for factors"),
            );
        }
        // stock do_log: `log(x, base)` recycles both arguments
        if op_name == "log" {
            let rest = CDR(args);
            if !rest.is_null() && rest != R_NilValue() {
                let mut base = CAR(rest);
                if base == crate::sexp::globals::R_MissingArg() {
                    base = Rf_ScalarReal(std::f64::consts::E);
                }
                if x == crate::sexp::globals::R_MissingArg() {
                    arithmetic_error("argument \"x\" is missing, with no default");
                }
                if XLENGTH(base) == 0 {
                    arithmetic_error("invalid argument 'base' of length 0");
                }
                if TYPEOF(x) == SEXPTYPE::CPLXSXP || TYPEOF(base) == SEXPTYPE::CPLXSXP {
                    return complex_log_with_base(x, base);
                }
                if !is_numeric_operand(x) || !is_numeric_operand(base) {
                    crate::mainutils::errors::record_error_call(call, true);
                    arithmetic_error("non-numeric argument to mathematical function");
                }
                return log_with_base(call, x, base);
            }
        }
        if TYPEOF(x) == SEXPTYPE::CPLXSXP {
            return match op_name {
                "sqrt" => crate::eval::complex_arith::complex_unary_vec(
                    x,
                    crate::eval::complex_arith::complex_sqrt,
                ),
                "log" => crate::eval::complex_arith::complex_unary_vec(
                    x,
                    crate::eval::complex_arith::complex_log,
                ),
                "exp" => crate::eval::complex_arith::complex_unary_vec(
                    x,
                    crate::eval::complex_arith::complex_exp,
                ),
                "sinh" => crate::eval::complex_arith::complex_unary_vec(
                    x,
                    crate::eval::complex_arith::complex_sinh,
                ),
                "cosh" => crate::eval::complex_arith::complex_unary_vec(
                    x,
                    crate::eval::complex_arith::complex_cosh,
                ),
                "tanh" => crate::eval::complex_arith::complex_unary_vec(
                    x,
                    crate::eval::complex_arith::complex_tanh,
                ),
                "abs" => {
                    // stock Math1: complex abs is Mod, returned as double
                    let n = XLENGTH(x);
                    let out = Rf_allocVector3(SEXPTYPE::REALSXP, n);
                    let Some(result) = Sexp::from_raw(out) else {
                        return R_NilValue();
                    };
                    let _out_guard = protect(out);
                    // SAFETY: result is freshly allocated; no payload reference was lent.
                    let mut result_mut = SexpMut::from_owned(result);
                    for i in 0..n {
                        let c = crate::sexp::accessors::COMPLEX_ELT(x, i as i32);
                        let value = if c.r.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                            || c.i.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                        {
                            crate::sexp::ffi::NA_REAL
                        } else {
                            libm::sqrt(c.r * c.r + c.i * c.i)
                        };
                        result_mut.set_real_elt(i, value);
                    }
                    let _ = result_mut.freeze();
                    return out;
                }
                "log10" => {
                    let base = crate::sexp::constructors::Rf_ScalarReal(10.0);
                    let _g = protect(base);
                    complex_log_with_base(x, base)
                }
                "log2" => {
                    let base = crate::sexp::constructors::Rf_ScalarReal(2.0);
                    let _g = protect(base);
                    complex_log_with_base(x, base)
                }
                _ => arithmetic_error("unimplemented complex function"),
            };
        }

        // stock Math1: non-numeric arguments error (lowercase message from
        // arithmetic.c, unlike the capital-N distn.c variant).
        if !is_numeric_operand(x) {
            crate::mainutils::errors::record_error_call(call, true);
            arithmetic_error("non-numeric argument to mathematical function");
        }

        let f: fn(f64) -> f64 = match op_name {
            "abs" => f64::abs,
            "sqrt" => |v: f64| {
                if v.is_nan() {
                    v
                } else if v < 0.0 {
                    f64::NAN
                } else {
                    libm::sqrt(v)
                }
            },
            "log" => |v: f64| libm::log(v),
            "log2" => |v: f64| libm::log2(v),
            "log10" => |v: f64| libm::log10(v),
            "exp" => |v: f64| libm::exp(v),
            "sinh" => f64::sinh,
            "cosh" => f64::cosh,
            "tanh" => f64::tanh,
            "ceiling" => f64::ceil,
            "floor" => f64::floor,
            "trunc" => f64::trunc,
            "round" => f64::round,
            "sign" => |v: f64| {
                if v > 0.0 {
                    1.0
                } else if v < 0.0 {
                    -1.0
                } else {
                    0.0
                }
            },
            _ => return R_NilValue(),
        };

        let result = math1_vec(call, x, f);
        if result.is_null() {
            return R_NilValue();
        }

        // For integer-friendly operations, try to return INTSXP
        let prefer_int =
            op_name == "ceiling" || op_name == "floor" || op_name == "trunc" || op_name == "round";
        let input_is_int = Sexp::from_raw(x)
            .is_some_and(|input| matches!(input.typeof_(), SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP));

        if prefer_int || input_is_int {
            let _result_guard = protect(result);
            let Some(result_vec) = NumericVector::from_raw(result) else {
                return result;
            };
            let n = result_vec.clone().len();
            let mut all_int = true;
            for i in 0..n {
                let v = result_vec.clone().real_at(i);
                if v.to_bits() == R_NA_BIT_PATTERN {
                    continue;
                }
                if !v.is_finite() || v != v.floor() || v < i32::MIN as f64 || v > i32::MAX as f64 {
                    all_int = false;
                    break;
                }
            }
            if all_int {
                let iresult_raw = Rf_allocVector3(SEXPTYPE::INTSXP, n);
                let Some(iresult) = Sexp::from_raw(iresult_raw) else {
                    return result;
                };
                let _iresult_guard = protect(iresult_raw);
                // SAFETY: result is freshly allocated; no payload reference was lent.
                let mut iresult_mut = SexpMut::from_owned(iresult);
                for i in 0..n {
                    let v = result_vec.clone().real_at(i);
                    let value = if v.to_bits() == R_NA_BIT_PATTERN {
                        NA_INTEGER
                    } else {
                        v as i32
                    };
                    iresult_mut.set_integer_elt(i, value);
                }
                let _ = iresult_mut.freeze();
                propagate_unary_vector_attributes(iresult_raw, x, n);
                return iresult_raw;
            }
        }

        result
    }
}

/// Handle `length(x)`.
pub unsafe fn do_length(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchOrEval(
            call,
            op,
            c"length".as_ptr(),
            args,
            rho,
            &mut ans,
            0,
            1,
        ) != 0
        {
            return ans;
        }
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) == SEXPTYPE::NILSXP {
            return Rf_ScalarInteger(0);
        }

        #[cfg(feature = "renderplot-device")]
        if crate::mainutils::essentials::sexp_has_class(x, "unit") {
            return crate::mainutils::portable_grid::unit_value_length(x);
        }
        Rf_ScalarInteger(crate::sexp::constructors::Rf_length(x))
    }
}
