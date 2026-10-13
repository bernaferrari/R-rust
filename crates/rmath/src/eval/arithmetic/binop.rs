use super::*;

// ---------------------------------------------------------------------------
// Vectorized binary arithmetic
// ---------------------------------------------------------------------------

pub(crate) const VECTOR_CANCELLATION_POLL_INTERVAL: R_xlen_t = 1024;

#[inline]
pub(crate) fn poll_vector_cancellation(i: R_xlen_t) {
    if i % VECTOR_CANCELLATION_POLL_INTERVAL == 0 {
        crate::sexp::instance::check_cancellation();
    }
}

/// Apply a real-valued binary operation with recycling.
///
/// Returns REALSXP if either operand is REALSXP, otherwise returns INTSXP/LGLSXP.
/// Integer overflow produces NA_INTEGER.
pub unsafe fn real_binary(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        if sa.is_null() || sb.is_null() {
            return R_NilValue();
        }
        if sa == R_NilValue() || sb == R_NilValue() {
            let complex = (!sa.is_null() && sa != R_NilValue() && TYPEOF(sa) == SEXPTYPE::CPLXSXP)
                || (!sb.is_null() && sb != R_NilValue() && TYPEOF(sb) == SEXPTYPE::CPLXSXP);
            let other_real =
                (!sa.is_null() && sa != R_NilValue() && TYPEOF(sa) == SEXPTYPE::REALSXP)
                    || (!sb.is_null() && sb != R_NilValue() && TYPEOF(sb) == SEXPTYPE::REALSXP);
            let kind = if complex {
                SEXPTYPE::CPLXSXP
            } else if op == "/" || op == "^" || other_real {
                SEXPTYPE::REALSXP
            } else {
                SEXPTYPE::INTSXP
            };
            let empty = Rf_allocVector3(kind, 0);
            let _empty_guard = protect(empty);
            let src_a = if sa == R_NilValue() { empty } else { sa };
            let src_b = if sb == R_NilValue() { empty } else { sb };
            propagate_arithmetic_attributes(empty, src_a, src_b, 0);
            return empty;
        }
        if let Some(frame) = data_frame_arith(op, sa, sb) {
            return frame;
        }
        if !is_numeric_operand(sa) || !is_numeric_operand(sb) {
            arithmetic_error("non-numeric argument to binary operator");
        }
        let Some(a) = NumericVector::from_raw(sa) else {
            return R_NilValue();
        };
        let Some(b) = NumericVector::from_raw(sb) else {
            return R_NilValue();
        };
        let n = a.clone().recycled_len_with(b.clone());
        let use_real = op == "/" || op == "^" || a.clone().needs_real_with(b.clone());
        if n == 0 {
            let kind = if use_real {
                SEXPTYPE::REALSXP
            } else {
                SEXPTYPE::INTSXP
            };
            let empty = Rf_allocVector3(kind, 0);
            let _empty_guard = protect(empty);
            propagate_arithmetic_attributes(empty, sa, sb, 0);
            return empty;
        }
        let integer_overflow_can_warn = matches!(op, "+" | "-" | "*");
        let sa_dim =
            crate::sexp::attrib_core::getAttrib(sa, crate::sexp::attrib_core::R_DimSymbol());
        let sb_dim =
            crate::sexp::attrib_core::getAttrib(sb, crate::sexp::attrib_core::R_DimSymbol());
        let sa_arr = !sa_dim.is_null() && sa_dim != R_NilValue();
        let sb_arr = !sb_dim.is_null() && sb_dim != R_NilValue();
        let al = XLENGTH(sa);
        let bl = XLENGTH(sb);
        if sa_arr && !sb_arr && al == 1 && bl > 1 {
            warn_simple(
                "Recycling array of length 1 in array-vector arithmetic is deprecated.\n  Use c() or as.vector() instead.",
            );
        } else if sb_arr && !sa_arr && bl == 1 && al > 1 {
            warn_simple(
                "Recycling array of length 1 in vector-array arithmetic is deprecated.\n  Use c() or as.vector() instead.",
            );
        }
        let result_raw = if use_real {
            Rf_allocVector3(SEXPTYPE::REALSXP, n)
        } else {
            Rf_allocVector3(SEXPTYPE::INTSXP, n)
        };
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);
        warn_if_non_multiple_recycling(a.clone().len(), b.clone().len());
        let mut integer_overflow = false;

        for i in 0..n {
            poll_vector_cancellation(i);
            let x = a.clone().real_at(i);
            let y = b.clone().real_at(i);
            let x_na = x.to_bits() == R_NA_BIT_PATTERN;
            let y_na = y.to_bits() == R_NA_BIT_PATTERN;

            if op == "^" && ((!y_na && y == 0.0) || (!x_na && x == 1.0)) {
                result_mut.set_real_elt(i, 1.0);
                continue;
            }

            if x_na || y_na {
                if use_real {
                    result_mut.set_real_elt(i, NA_REAL);
                } else {
                    result_mut.set_integer_elt(i, NA_INTEGER);
                }
                continue;
            }

            let val = binary_arithmetic_value(op, x, y);

            if use_real {
                result_mut.set_real_elt(i, val);
            } else {
                // Integer path: check for overflow
                if val.is_finite()
                    && val == val.floor()
                    && val >= i32::MIN as f64
                    && val <= i32::MAX as f64
                {
                    let ival = val as i32;
                    result_mut.set_integer_elt(i, ival);
                } else {
                    integer_overflow |= integer_overflow_can_warn;
                    result_mut.set_integer_elt(i, NA_INTEGER);
                }
            }
        }

        if integer_overflow {
            warn_simple("NAs produced by integer overflow");
        }
        let _ = result_mut.freeze();
        propagate_arithmetic_attributes(result_raw, sa, sb, n);
        result_raw
    }
}

#[inline]
pub(crate) fn binary_arithmetic_value(op: &str, x: f64, y: f64) -> f64 {
    match op {
        "+" => x + y,
        "-" => x - y,
        "*" => x * y,
        "/" => x / y,
        "^" => crate::nmath::special::mlutils::R_pow(x, y),
        "%%" => crate::mainutils::arithmetic::myfmod(x, y),
        "%/%" => crate::mainutils::arithmetic::myfloor(x, y),
        _ => NA_REAL,
    }
}

/// Vectorized binary comparison with recycling.
///
/// Always returns LGLSXP of the result length.
pub(crate) unsafe fn binary_compare(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        let Some(a) = NumericVector::from_raw(sa) else {
            arithmetic_error("comparison of these types is not implemented");
        };
        let Some(b) = NumericVector::from_raw(sb) else {
            arithmetic_error("comparison of these types is not implemented");
        };
        let sa_dim =
            crate::sexp::attrib_core::getAttrib(sa, crate::sexp::attrib_core::R_DimSymbol());
        let sb_dim =
            crate::sexp::attrib_core::getAttrib(sb, crate::sexp::attrib_core::R_DimSymbol());
        let sa_arr = !sa_dim.is_null() && sa_dim != R_NilValue();
        let sb_arr = !sb_dim.is_null() && sb_dim != R_NilValue();
        let al = XLENGTH(sa);
        let bl = XLENGTH(sb);
        if (sa_arr && !sb_arr && al > 0 && bl > 1 && al % bl != 0)
            || (sb_arr && !sa_arr && bl > 0 && al > 1 && bl % al != 0)
        {
            let (prod, obj) = if sa_arr { (al, bl) } else { (bl, al) };
            arithmetic_error(format!(
                "dims [product {prod}] do not match the length of object [{obj}]"
            ));
        }
        let n = a.clone().recycled_len_with(b.clone());
        if n == 0 {
            let empty = Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
            let _empty_guard = protect(empty);
            propagate_binary_vector_attributes(empty, sa, sb, 0);
            return empty;
        }
        let result_raw = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let _result_guard = protect(result_raw);
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);
        warn_if_non_multiple_recycling(a.clone().len(), b.clone().len());

        let use_real = a.clone().needs_real_with(b.clone());

        for i in 0..n {
            poll_vector_cancellation(i);
            let (missing, cmp): (bool, bool) = if use_real {
                let x = a.clone().real_at(i);
                let y = b.clone().real_at(i);
                // GNU relop ISNAN: NA and NaN both yield NA_LOGICAL.
                let missing = x.is_nan() || y.is_nan();
                let c = match op {
                    "<" => x < y,
                    ">" => x > y,
                    "<=" => x <= y,
                    ">=" => x >= y,
                    "==" => x == y,
                    "!=" => x != y,
                    _ => false,
                };
                (missing, c)
            } else {
                let x = a.clone().int_at(i);
                let y = b.clone().int_at(i);
                let missing = x == NA_INTEGER || y == NA_INTEGER;
                let c = match op {
                    "<" => x < y,
                    ">" => x > y,
                    "<=" => x <= y,
                    ">=" => x >= y,
                    "==" => x == y,
                    "!=" => x != y,
                    _ => false,
                };
                (missing, c)
            };

            let value = if missing {
                NA_LOGICAL
            } else if cmp {
                TRUE
            } else {
                FALSE
            };

            result_mut.set_logical_elt(i, value);
        }

        let _ = result_mut.freeze();
        propagate_binary_vector_attributes(result_raw, sa, sb, n);
        result_raw
    }
}

pub(crate) fn warn_if_non_multiple_recycling(a_len: R_xlen_t, b_len: R_xlen_t) {
    let shorter = a_len.min(b_len);
    let longer = a_len.max(b_len);
    if shorter > 0 && longer % shorter != 0 {
        warn_simple("longer object length is not a multiple of shorter object length");
    }
}

pub(crate) fn warn_simple(message: &str) {
    let cmsg = std::ffi::CString::new(message).unwrap_or_default();
    unsafe { crate::mainutils::errors::Rf_warning(cmsg.as_ptr()) }
}

pub(crate) fn arithmetic_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

pub(crate) unsafe fn copy_attr_if_present(result: SEXP, source: SEXP, symbol: SEXP) -> bool {
    unsafe {
        let attr = getAttrib(source, symbol);
        if attr.is_null() || attr == R_NilValue() {
            return false;
        }
        let attr = crate::mainutils::duplicate::duplicate(attr);
        setAttrib(result, symbol, attr);
        true
    }
}

pub(crate) unsafe fn copy_dims_if_present(
    result: SEXP,
    source: SEXP,
    result_len: R_xlen_t,
) -> bool {
    unsafe {
        if source.is_null() || LENGTH(source) as R_xlen_t != result_len {
            return false;
        }
        let dim = getAttrib(source, R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() {
            return false;
        }

        let dim = crate::mainutils::duplicate::duplicate(dim);
        setAttrib(result, R_DimSymbol(), dim);
        let dimnames = getAttrib(source, R_DimNamesSymbol());
        if !dimnames.is_null() && dimnames != R_NilValue() {
            let dimnames = crate::mainutils::duplicate::duplicate(dimnames);
            setAttrib(result, R_DimNamesSymbol(), dimnames);
        }
        true
    }
}

/// Propagate attributes onto a binary-operation result, mirroring upstream
/// `R_binary` (r-source/src/main/arithmetic.c, attribute preservation block).
///
/// Decision table:
/// - If both operands carry `dims` they must be conformable, otherwise a
///   'non-conformable arrays' error is raised; the result takes dims from `x`
///   and dimnames from `x` (falling back to `y`).
///   special case does not apply, nor is the partner a length-0 non-array),
///   the result inherits that operand's dims/dimnames.
/// - Otherwise (plain vectors) only `names` are propagated, from whichever
///   operand matches the result length (`x` wins).
/// Arithmetic preserves non-shape attributes from a full-length operand;
/// when lengths tie, the left operand overwrites matching right attributes.
/// Comparisons use the separate shape-only path below (GNU arithmetic.c).
pub(crate) unsafe fn propagate_arithmetic_attributes(result: SEXP, a: SEXP, b: SEXP, n: R_xlen_t) {
    unsafe {
        if XLENGTH(b) == n {
            crate::mainutils::array::copyMostAttrib(b, result);
        }
        if XLENGTH(a) == n {
            crate::mainutils::array::copyMostAttrib(a, result);
        }
        propagate_binary_vector_attributes(result, a, b, n);
    }
}

pub(super) unsafe fn propagate_binary_vector_attributes(
    result: SEXP,
    a: SEXP,
    b: SEXP,
    result_len: R_xlen_t,
) {
    unsafe {
        let nil = R_NilValue();
        let a_dims = getAttrib(a, R_DimSymbol());
        let b_dims = getAttrib(b, R_DimSymbol());
        let a_is_array = !a_dims.is_null() && a_dims != nil;
        let b_is_array = !b_dims.is_null() && b_dims != nil;

        if a_is_array || b_is_array {
            let a_len = LENGTH(a) as R_xlen_t;
            let b_len = LENGTH(b) as R_xlen_t;
            // Upstream strips the dims of a length-1 array whose partner is a
            // vector of a different length (deprecated recycling), so such a
            // combination falls through to the plain-vector treatment below.
            let a_counts = !(a_is_array && a_len == 1 && b_len != 1);
            let b_counts = !(b_is_array && b_len == 1 && a_len != 1);
            // An array paired with a length-0 non-array skips array treatment.
            let a_applies = a_is_array && a_counts && (b_len != 0 || a_len == 0);
            let b_applies = b_is_array && b_counts && (a_len != 0 || b_len == 0);

            if a_applies && b_applies {
                if crate::mainutils::relop::conformable(a, b) == 0 {
                    arithmetic_error("non-conformable arrays");
                }
                let dims = crate::mainutils::duplicate::duplicate(a_dims);
                setAttrib(result, R_DimSymbol(), dims);
                if copy_attr_if_present(result, a, R_DimNamesSymbol()) {
                    return;
                }
                copy_attr_if_present(result, b, R_DimNamesSymbol());
            } else if a_applies {
                copy_dims_if_present(result, a, result_len);
            } else if b_applies {
                copy_dims_if_present(result, b, result_len);
            } else {
                // Array treatment skipped: plain-vector names below.
                propagate_plain_names(result, a, b, result_len);
            }
            return;
        }

        propagate_plain_names(result, a, b, result_len);
    }
}

/// Plain-vector tail of the upstream decision table: names only, `x` wins.
pub(crate) unsafe fn propagate_plain_names(result: SEXP, a: SEXP, b: SEXP, result_len: R_xlen_t) {
    unsafe {
        if LENGTH(a) as R_xlen_t == result_len && copy_attr_if_present(result, a, R_NamesSymbol()) {
            return;
        }
        if LENGTH(b) as R_xlen_t == result_len {
            copy_attr_if_present(result, b, R_NamesSymbol());
        }
    }
}

pub(crate) unsafe fn set_single_string_class(result: SEXP, class_name: &str) {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class.is_null() {
            return;
        }
        let _class_guard = protect(class);
        let cstr = CString::new(class_name).unwrap_or_default();
        SET_STRING_ELT(class, 0, Rf_mkChar(cstr.as_ptr()));
        setAttrib(result, Rf_install(c"class".as_ptr()), class);
    }
}

pub(crate) unsafe fn set_string_attribute(result: SEXP, name: &str, value: &str) {
    unsafe {
        let attr = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if attr.is_null() {
            return;
        }
        let _attr_guard = protect(attr);
        let cvalue = CString::new(value).unwrap_or_default();
        SET_STRING_ELT(attr, 0, Rf_mkChar(cvalue.as_ptr()));
        let cname = CString::new(name).unwrap_or_default();
        setAttrib(result, Rf_install(cname.as_ptr()), attr);
    }
}

pub(crate) unsafe fn set_posixct_attributes_from(result: SEXP, source: SEXP) {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !class.is_null() {
            let _class_guard = protect(class);
            SET_STRING_ELT(class, 0, Rf_mkChar(c"POSIXct".as_ptr()));
            SET_STRING_ELT(class, 1, Rf_mkChar(c"POSIXt".as_ptr()));
            setAttrib(result, Rf_install(c"class".as_ptr()), class);
        }

        let tzone = getAttrib(source, Rf_install(c"tzone".as_ptr()));
        if !tzone.is_null() && tzone != R_NilValue() {
            setAttrib(
                result,
                Rf_install(c"tzone".as_ptr()),
                crate::mainutils::duplicate::duplicate(tzone),
            );
        }
    }
}

pub(crate) unsafe fn first_summary_data_arg(args: SEXP) -> SEXP {
    unsafe {
        let na_rm_sym = Rf_install(c"na.rm".as_ptr());
        let finite_sym = Rf_install(c"finite".as_ptr());
        let mut p = args;
        while !p.is_null() && p != R_NilValue() {
            if TAG(p) != na_rm_sym && TAG(p) != finite_sym {
                return CAR(p);
            }

            p = CDR(p);
        }
        R_NilValue()
    }
}

pub(crate) unsafe fn restore_datetime_summary_class(source: SEXP, result: SEXP) {
    unsafe {
        if source.is_null() || source == R_NilValue() || result.is_null() || result == R_NilValue()
        {
            return;
        }
        if crate::mainutils::essentials::sexp_has_class(source, "Date") {
            set_single_string_class(result, "Date");
        } else if crate::mainutils::essentials::sexp_has_class(source, "POSIXct")
            || crate::mainutils::essentials::sexp_has_class(source, "POSIXt")
        {
            set_posixct_attributes_from(result, source);
        }
    }
}

pub(crate) unsafe fn coerce_summary_posixlt_args(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let mut out = R_NilValue();
        let mut tail: SEXP = std::ptr::null_mut();
        let mut guards: Vec<_> = Vec::new();
        let mut p = args;
        while !p.is_null() && p != R_NilValue() {
            let mut val = CAR(p);
            if crate::mainutils::essentials::sexp_has_class(val, "POSIXlt")
                && TYPEOF(val) == SEXPTYPE::VECSXP
            {
                let converted = crate::mainutils::essentials::do_as_POSIXct(
                    call,
                    op,
                    Rf_cons(val, R_NilValue()),
                    rho,
                );
                guards.push(protect(converted));
                val = converted;
            }
            let cell = Rf_cons(val, R_NilValue());
            guards.push(protect(cell));
            if !TAG(p).is_null() && TAG(p) != R_NilValue() {
                SETTAG(cell, TAG(p));
            }
            if out == R_NilValue() {
                out = cell;
            } else {
                SETCDR(tail, cell);
            }
            tail = cell;
            p = CDR(p);
        }
        let _guards = guards;
        out
    }
}

pub(crate) unsafe fn string_attribute_value(source: SEXP, name: &CStr) -> Option<String> {
    unsafe {
        let attr = getAttrib(source, Rf_install(name.as_ptr()));
        let attr = Sexp::from_raw(attr)?;
        if attr.clone().typeof_() != SEXPTYPE::STRSXP || attr.clone().len() == 0 {
            return None;
        }
        let value = STRING_ELT(attr.as_raw(), 0);
        if value.is_null() || value == R_NaString() {
            return None;
        }
        CStr::from_ptr(CHAR(value))
            .to_str()
            .ok()
            .map(str::to_string)
    }
}
