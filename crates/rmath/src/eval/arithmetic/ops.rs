use super::*;

// ---------------------------------------------------------------------------
// Unary operations (vectorized)
// ---------------------------------------------------------------------------

/// Apply a unary real function element-wise to a numeric vector.
pub(crate) unsafe fn math1_vec(call: SEXP, sa: SEXP, f: fn(f64) -> f64) -> SEXP {
    unsafe {
        let Some(x) = NumericVector::from_raw(sa) else {
            return R_NilValue();
        };
        let n = x.clone().len();
        let result_raw = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);
        let mut naflag = false;
        for i in 0..n {
            poll_vector_cancellation(i);
            let value = x.clone().real_at(i);
            if value.to_bits() == R_NA_BIT_PATTERN {
                result_mut.set_real_elt(i, NA_REAL);
            } else {
                let r = f(value);
                if r.is_nan() && !value.is_nan() {
                    naflag = true;
                }
                result_mut.set_real_elt(i, r);
            }
        }
        let _ = result_mut.freeze();
        if naflag {
            crate::mainutils::errors::Rf_warningcall1(call, c"NaNs produced".as_ptr());
        }
        propagate_unary_vector_attributes(result_raw, sa, n);
        result_raw
    }
}

// ---------------------------------------------------------------------------
// Factor operands and stock group-generic behavior
// ---------------------------------------------------------------------------

/// True for logical/integer/real vectors — stock's `isNumeric` minus NULL.
pub(crate) fn is_numeric_operand(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP
    }
}

/// An unordered factor: carries the "factor" class but not "ordered".
pub(crate) fn is_plain_factor(x: SEXP) -> bool {
    unsafe { has_class(x, "factor") && !has_class(x, "ordered") }
}

/// Emit a group-generic warning attributed to the S3 method call the way
/// stock dispatch does (`In Ops.factor(e1, e2) : ...`). `e1`/`e2` are the
/// unevaluated operand expressions of the applied call.
pub(crate) unsafe fn factor_not_meaningful_warning(
    method: &'static str,
    message: &str,
    e1: SEXP,
    e2: SEXP,
) {
    unsafe {
        // method is a fixed interned name; Rf_install needs a NUL suffix
        let f = if method == "Ops.ordered" {
            Rf_install(c"Ops.ordered".as_ptr())
        } else {
            Rf_install(c"Ops.factor".as_ptr())
        };
        let method_call = if e2.is_null() || e2 == R_NilValue() {
            Rf_lang2(f, e1)
        } else {
            Rf_lang3(f, e1, e2)
        };
        let _call_guard = protect(method_call);
        let c_msg = std::ffi::CString::new(message).unwrap_or_default();
        crate::mainutils::errors::Rf_warningcall1(method_call, c_msg.as_ptr());
    }
}

/// `rep.int(NA, max(length(e1), if (!missing(e2)) length(e2)))` — the
/// logical NA vector stock Ops.factor/Ops.ordered return for operators
/// that are not meaningful for factors.
pub(crate) unsafe fn factor_na_result(e1: SEXP, e2: SEXP) -> SEXP {
    unsafe {
        let n1 = if e1.is_null() || e1 == R_NilValue() {
            0
        } else {
            XLENGTH(e1)
        };
        let n2 = if e2.is_null() || e2 == R_NilValue() {
            0
        } else {
            XLENGTH(e2)
        };
        let n = n1.max(n2);
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..n {
            result_mut.set_logical_elt(i, NA_LOGICAL);
        }
        let _ = result_mut.freeze();
        result_raw
    }
}

/// Handle factor/ordered operands for arithmetic the way stock's Ops group
/// dispatch does: warn via the method call and return logical NAs. The
/// unevaluated operand expressions come from `call` so the warning is
/// attributed exactly like stock. Returns None when neither operand is a
/// factor.
pub(crate) unsafe fn factor_arith(op: &str, call: SEXP, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_factor = has_class(a, "factor");
        let b_factor = has_class(b, "factor");
        if !a_factor && !b_factor {
            return None;
        }
        let ordered = has_class(a, "ordered") || has_class(b, "ordered");
        let message = if ordered {
            format!("'{op}' is not meaningful for ordered factors")
        } else {
            format!("'{op}' not meaningful for factors")
        };
        let method = if ordered { "Ops.ordered" } else { "Ops.factor" };
        let e1_expr = CAR(CDR(call));
        let e2_expr = CAR(CDR(CDR(call)));
        factor_not_meaningful_warning(method, &message, e1_expr, e2_expr);
        Some(factor_na_result(a, b))
    }
}

/// as.character(<factor>): level strings by 1-based code; NA and 0 codes
/// stay NA.
pub(crate) unsafe fn factor_as_character(f: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(f);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if out.is_null() {
            return R_NilValue();
        }
        let _out_guard = protect(out);
        let levels = getAttrib(f, Rf_install(c"levels".as_ptr()));
        let levels_ok =
            !levels.is_null() && levels != R_NilValue() && TYPEOF(levels) == SEXPTYPE::STRSXP;
        let n_levels = if levels_ok { LENGTH(levels) } else { 0 };
        for i in 0..n {
            let code = INTEGER_ELT(f, i as i32);
            let s = if code == NA_INTEGER || code <= 0 || code as R_xlen_t > n_levels as R_xlen_t {
                R_NaString()
            } else {
                let level = STRING_ELT(levels, (code - 1) as i64);
                if level.is_null() || level == R_NaString() {
                    let sentinel = na_level_sentinel(levels, n_levels);
                    let c = std::ffi::CString::new(sentinel).unwrap_or_default();
                    Rf_mkChar(c.as_ptr())
                } else {
                    level
                }
            };
            SET_STRING_ELT(out, i as i64, s);
        }
        out
    }
}
pub(crate) unsafe fn na_level_sentinel(levels: SEXP, n_levels: i32) -> String {
    unsafe {
        let mut sentinel = "  NA ".to_string();
        loop {
            let mut taken = false;
            for i in 0..n_levels {
                let level = STRING_ELT(levels, i as i64);
                if level.is_null() || level == R_NaString() {
                    continue;
                }
                if charsxp_to_string(level).as_deref() == Some(sentinel.as_str()) {
                    taken = true;
                    break;
                }
            }
            if !taken {
                return sentinel;
            }
            sentinel.push_str(" .");
        }
    }
}

pub(crate) unsafe fn rewritten_level_labels(f: SEXP) -> Vec<String> {
    unsafe {
        let levels = getAttrib(f, Rf_install(c"levels".as_ptr()));
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return Vec::new();
        }
        let n_levels = LENGTH(levels);
        let sentinel = na_level_sentinel(levels, n_levels);
        let mut out = Vec::with_capacity(n_levels as usize);
        for i in 0..n_levels {
            let level = STRING_ELT(levels, i as i64);
            if level.is_null() || level == R_NaString() {
                out.push(sentinel.clone());
            } else {
                out.push(charsxp_to_string(level).unwrap_or_default());
            }
        }
        out
    }
}

/// Coerce a relop operand to character following the stock ladder; only
/// logical/integer/real/complex/raw make it (the rest error like stock's
/// final else).
pub(crate) unsafe fn relop_operand_to_character(x: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            return x;
        }
        if is_numeric_operand(x) || TYPEOF(x) == SEXPTYPE::CPLXSXP || TYPEOF(x) == SEXPTYPE::RAWSXP
        {
            return crate::mainutils::coerce::coerceVector(x, SEXPTYPE::STRSXP.into());
        }
        arithmetic_error("comparison of these types is not implemented")
    }
}

/// stock Ops.factor for unordered factors: `==`/`!=` compare level strings
/// (with the scalar-character fast paths and the level-set check stock
/// applies when both sides are converted), any other operator warns and
/// returns logical NAs. Returns None when neither operand is an unordered
/// factor.
pub(crate) unsafe fn unordered_factor_compare(
    op: &str,
    call: SEXP,
    sa: SEXP,
    sb: SEXP,
) -> Option<SEXP> {
    unsafe {
        let a_factor = is_plain_factor(sa);
        let b_factor = is_plain_factor(sb);
        if !a_factor && !b_factor {
            return None;
        }
        if op != "==" && op != "!=" {
            let e1_expr = CAR(CDR(call));
            let e2_expr = CAR(CDR(CDR(call)));
            factor_not_meaningful_warning(
                "Ops.factor",
                &format!("'{op}' not meaningful for factors"),
                e1_expr,
                e2_expr,
            );
            return Some(factor_na_result(sa, sb));
        }

        // stock converts the left factor to its level strings first; when
        // the right side is then a factor with no NA levels and the
        // converted left is a scalar string, the levels table is compared
        // directly (bypassing the level-set check, like stock).
        let scalar_string = |s: SEXP| -> Option<String> {
            if TYPEOF(s) == SEXPTYPE::STRSXP && LENGTH(s) == 1 {
                charsxp_to_string(STRING_ELT(s, 0))
            } else {
                None
            }
        };
        let levels_of = |f: SEXP| -> SEXP { getAttrib(f, Rf_install(c"levels".as_ptr())) };
        let levels_have_no_na = |l: SEXP| -> bool {
            (0..LENGTH(l) as R_xlen_t).all(|i| {
                let s = STRING_ELT(l, i as i64);
                !s.is_null() && s != R_NaString()
            })
        };
        let levels_vec = |l: SEXP| -> Option<Vec<String>> {
            let mut out = Vec::with_capacity(LENGTH(l) as usize);
            for i in 0..LENGTH(l) as R_xlen_t {
                out.push(charsxp_to_string(STRING_ELT(l, i as i64))?);
            }
            Some(out)
        };

        let a_cmp = if a_factor {
            factor_as_character(sa)
        } else {
            relop_operand_to_character(sa)
        };
        if b_factor {
            let b_levels = levels_of(sb);
            if TYPEOF(b_levels) == SEXPTYPE::STRSXP && levels_have_no_na(b_levels) {
                if let Some(other) = scalar_string(a_cmp) {
                    if let Some(levels) = levels_vec(b_levels) {
                        return Some(factor_scalar_string_compare(op, sb, &levels, &other));
                    }
                }
            }
        }
        let b_cmp = if b_factor {
            factor_as_character(sb)
        } else {
            relop_operand_to_character(sb)
        };
        if a_factor && b_factor {
            let mut la = rewritten_level_labels(sa);
            let mut lb = rewritten_level_labels(sb);
            la.sort();
            lb.sort();
            if la != lb {
                arithmetic_error("level sets of factors are different");
            }
        }
        Some(character_compare(op, a_cmp, b_cmp))
    }
}

/// stock fastpath: `leq <- (levels == other)` indexed by the factor codes.
pub(crate) unsafe fn factor_scalar_string_compare(
    op: &str,
    f: SEXP,
    levels: &[String],
    other: &str,
) -> SEXP {
    unsafe {
        let n = XLENGTH(f);
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..n {
            let code = INTEGER_ELT(f, i as i32);
            let value = if code == NA_INTEGER || code <= 0 || code as usize > levels.len() {
                NA_LOGICAL
            } else {
                let eq = levels[(code - 1) as usize] == other;
                if (op == "==") == eq { TRUE } else { FALSE }
            };
            result_mut.set_logical_elt(i, value);
        }
        let _ = result_mut.freeze();
        result_raw
    }
}

/// Unary minus for complex values (stock `-(z)`).
pub(crate) fn complex_negate(c: Rcomplex) -> Rcomplex {
    Rcomplex { r: -c.r, i: -c.i }
}

/// stock complex_relop: only `==`/`!=` are defined; ordering comparisons
/// error. Equality compares component-wise with NA propagation.
pub(crate) unsafe fn complex_relop(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        if op != "==" && op != "!=" {
            arithmetic_error("invalid comparison with complex values");
        }
        let a = crate::mainutils::coerce::coerceVector(sa, SEXPTYPE::CPLXSXP.into());
        let b = crate::mainutils::coerce::coerceVector(sb, SEXPTYPE::CPLXSXP.into());
        let _a_guard = protect(a);
        let _b_guard = protect(b);
        let n1 = XLENGTH(a);
        let n2 = XLENGTH(b);
        let n = n1.max(n2);
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..n {
            let x = crate::sexp::accessors::COMPLEX_ELT(a, i as i32);
            let y = crate::sexp::accessors::COMPLEX_ELT(b, i as i32);
            // stock ISNAN covers both NA and NaN components
            let value = if x.r.is_nan() || x.i.is_nan() || y.r.is_nan() || y.i.is_nan() {
                NA_LOGICAL
            } else {
                let eq = x.r == y.r && x.i == y.i;
                if (op == "==") == eq { TRUE } else { FALSE }
            };
            result_mut.set_logical_elt(i, value);
        }
        let _ = result_mut.freeze();
        result_raw
    }
}

/// Attempt S3 group-generic dispatch (Ops / Math / Summary / Complex).
/// Returns Some(result) when a method was found and applied.
pub(crate) unsafe fn try_group_dispatch(
    group: &[u8],
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> Option<SEXP> {
    unsafe {
        if args.is_null() || args == R_NilValue() || op.is_null() || op == R_NilValue() {
            return None;
        }
        let mut ans: SEXP = R_NilValue();
        let dispatched = crate::eval::dispatch::DispatchGroup(
            group.as_ptr() as *const std::os::raw::c_char,
            call,
            op,
            args,
            rho,
            &mut ans,
        );
        if dispatched != 0 { Some(ans) } else { None }
    }
}

/// Shared Ops dispatch used by GNU AND/OR/NOT bytecode.
pub(crate) unsafe fn try_ops_group_dispatch(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> Option<SEXP> {
    unsafe { try_group_dispatch(b"Ops\0", call, op, args, rho) }
}

// ---------------------------------------------------------------------------
// Top-level dispatch functions (called by the evaluator)
// ---------------------------------------------------------------------------
/// Handle binary arithmetic: +, -, *, /, ^, %%, %/%
pub unsafe fn do_arith(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(result) = try_group_dispatch(b"Ops\0", call, op, args, rho) {
            return result;
        }
        let op_name = get_op_name(op, call);
        #[cfg(feature = "renderplot-device")]
        if let Some(result) = crate::mainutils::portable_grid::unit_binary(
            op_name,
            CAR(args),
            if CDR(args).is_null() || CDR(args) == R_NilValue() {
                R_NilValue()
            } else {
                CAR(CDR(args))
            },
        ) {
            return result;
        }
        match op_name {
            "+" | "-" | "*" | "/" | "^" | "%%" | "%/%" => {
                let a = CAR(args);
                if a.is_null() {
                    return R_NilValue();
                }
                let b_cdr = CDR(args);
                if b_cdr.is_null() || b_cdr == R_NilValue() {
                    // Unary +/-
                    if op_name == "-" || op_name == "+" {
                        if let Some(result) = factor_arith(op_name, call, a, R_NilValue()) {
                            return result;
                        }
                        if TYPEOF(a) == SEXPTYPE::CPLXSXP {
                            if op_name == "-" {
                                return crate::eval::complex_arith::complex_unary_vec(
                                    a,
                                    complex_negate,
                                );
                            }
                            return a;
                        }
                        // stock: unary +/- require a numeric argument
                        // (NULL included); anything else errors.
                        if !is_numeric_operand(a) {
                            arithmetic_error("invalid argument to unary operator");
                        }
                        if op_name == "+" {
                            if TYPEOF(a) == SEXPTYPE::LGLSXP {
                                return unary_plus_logical(a);
                            }
                            return a;
                        }
                        let result = unary_minus(a);
                        if crate::mainutils::essentials::sexp_has_class(a, "difftime") {
                            set_difftime_attributes(result, &difftime_units(a));
                        }
                        return result;
                    }
                    return R_NilValue();
                }
                let b = CAR(b_cdr);
                if let Some(result) = date_binary_arithmetic(op_name, a, b) {
                    return result;
                }
                let a = posixlt_as_posixct_operand(call, a);
                let b = posixlt_as_posixct_operand(call, b);
                if let Some(result) = posixct_binary_arithmetic(op_name, a, b) {
                    return result;
                }

                if let Some(result) = difftime_binary_arithmetic(op_name, a, b) {
                    return result;
                }
                if let Some(result) = factor_arith(op_name, call, a, b) {
                    return result;
                }
                if TYPEOF(a) == SEXPTYPE::CPLXSXP || TYPEOF(b) == SEXPTYPE::CPLXSXP {
                    return crate::eval::complex_arith::complex_binary(op_name, a, b);
                }
                real_binary(op_name, a, b)
            }
            _ => R_NilValue(),
        }
    }
}
