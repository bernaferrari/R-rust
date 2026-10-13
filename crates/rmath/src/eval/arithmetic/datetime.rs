use super::*;

pub(crate) fn difftime_unit_scale(unit: &str) -> f64 {
    match unit {
        "secs" => 1.0,
        "mins" => 60.0,
        "hours" => 3_600.0,
        "days" => 86_400.0,
        "weeks" => 604_800.0,
        _ => 1.0,
    }
}

pub(crate) unsafe fn difftime_units(source: SEXP) -> String {
    unsafe { string_attribute_value(source, c"units").unwrap_or_else(|| "secs".to_string()) }
}

pub(crate) unsafe fn coerce_difftime_operand(
    source: SEXP,
    target_unit_seconds: f64,
    round_result: bool,
) -> SEXP {
    unsafe {
        if !crate::mainutils::essentials::sexp_has_class(source, "difftime") {
            return source;
        }
        let unit = string_attribute_value(source, c"units").unwrap_or_else(|| "secs".to_string());
        let scale = difftime_unit_scale(&unit) / target_unit_seconds;
        let Some(input) = NumericVector::from_raw(source) else {
            return source;
        };
        let result_raw = Rf_allocVector3(SEXPTYPE::REALSXP, input.clone().len());
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..input.clone().len() {
            let value = input.clone().real_at(i);
            let converted = if value.to_bits() == R_NA_BIT_PATTERN {
                NA_REAL
            } else if round_result {
                (value * scale).round_ties_even()
            } else {
                value * scale
            };
            result_mut.set_real_elt(i, converted);
        }
        let _ = result_mut.freeze();
        result_raw
    }
}

pub(crate) unsafe fn has_class_attribute(source: SEXP) -> bool {
    unsafe {
        let class = getAttrib(source, Rf_install(c"class".as_ptr()));
        !class.is_null() && class != R_NilValue()
    }
}

pub(crate) unsafe fn coerce_character_comparison_operand(
    source: SEXP,
    parser: fn(&str) -> Option<f64>,
) -> SEXP {
    unsafe {
        if TYPEOF(source) != SEXPTYPE::STRSXP {
            return source;
        }
        let Some(input) = Sexp::from_raw(source) else {
            return R_NilValue();
        };
        let result_raw = Rf_allocVector3(SEXPTYPE::REALSXP, input.clone().len());
        let Some(result) = Sexp::from_raw(result_raw) else {
            return R_NilValue();
        };
        let input_len = input.len();
        // SAFETY: result is freshly allocated; no payload reference was lent.
        let mut result_mut = SexpMut::from_owned(result);
        for i in 0..input_len {
            let value = STRING_ELT(source, i);
            let parsed = if value.is_null() || value == R_NaString() {
                NA_REAL
            } else {
                let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                parser(text).unwrap_or_else(|| {
                    arithmetic_error("character string is not in a standard unambiguous format");
                })
            };
            result_mut.set_real_elt(i, parsed);
        }
        let _ = result_mut.freeze();
        result_raw
    }
}

pub(crate) unsafe fn difftime_binary_comparison(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_difftime = crate::mainutils::essentials::sexp_has_class(a, "difftime");
        let b_is_difftime = crate::mainutils::essentials::sexp_has_class(b, "difftime");
        if !a_is_difftime && !b_is_difftime {
            return None;
        }

        // GNU's C comparison has no difftime unit logic: unit coercion
        // happens in the R-level Ops.difftime (coerceTimeUnit) before
        // NextMethod reaches this `.default` backend, so both operands'
        // raw values are already seconds. Rescaling by the (possibly
        // stale) units label here would apply the coercion twice.
        Some(binary_compare(op, a, b))
    }
}

pub(crate) fn parse_posixct_comparison_seconds(text: &str) -> Option<f64> {
    crate::mainutils::essentials::parse_iso_datetime_seconds(text)
}

pub(crate) unsafe fn is_numeric_version(x: SEXP) -> bool {
    unsafe {
        crate::mainutils::essentials::sexp_has_class(x, "numeric_version")
            || crate::mainutils::essentials::sexp_has_class(x, "package_version")
            || crate::mainutils::essentials::sexp_has_class(x, "R_system_version")
    }
}

pub(crate) unsafe fn version_components(x: SEXP) -> Option<Vec<i32>> {
    unsafe {
        if is_numeric_version(x) && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) >= 1 {
            let elt = VECTOR_ELT(x, 0);
            if TYPEOF(elt) == SEXPTYPE::INTSXP {
                let n = XLENGTH(elt);
                let mut out = Vec::with_capacity(n as usize);
                for i in 0..n {
                    out.push(INTEGER_ELT(elt, i as std::os::raw::c_int));
                }
                return Some(out);
            }
        }

        if TYPEOF(x) == SEXPTYPE::STRSXP && XLENGTH(x) >= 1 {
            let ch = STRING_ELT(x, 0);
            if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                return None;
            }
            let text = std::ffi::CStr::from_ptr(CHAR(ch)).to_string_lossy();
            let mut out = Vec::new();
            for part in text.split(['.', '-']) {
                if part.is_empty() {
                    continue;
                }
                out.push(part.parse::<i32>().ok()?);
            }
            return Some(out);
        }
        None
    }
}

pub(crate) unsafe fn numeric_version_comparison(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        if !is_numeric_version(a) && !is_numeric_version(b) {
            return None;
        }
        let av = version_components(a)?;
        let bv = version_components(b)?;
        let n = av.len().max(bv.len());
        let mut ord = 0i32;
        for i in 0..n {
            let x = av.get(i).copied().unwrap_or(0);
            let y = bv.get(i).copied().unwrap_or(0);
            if x != y {
                ord = if x < y { -1 } else { 1 };
                break;
            }
        }
        let yes = match op {
            "<" => ord < 0,
            ">" => ord > 0,
            "<=" => ord <= 0,
            ">=" => ord >= 0,
            "==" => ord == 0,
            "!=" => ord != 0,
            _ => return None,
        };
        Some(Rf_ScalarLogical(if yes { TRUE } else { FALSE }))
    }
}

pub(crate) unsafe fn date_binary_comparison(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_date = crate::mainutils::essentials::sexp_has_class(a, "Date");
        let b_is_date = crate::mainutils::essentials::sexp_has_class(b, "Date");
        if !a_is_date && !b_is_date {
            return None;
        }

        let a = coerce_character_comparison_operand(
            a,
            crate::mainutils::essentials::parse_iso_date_days,
        );
        let b = coerce_character_comparison_operand(
            b,
            crate::mainutils::essentials::parse_iso_date_days,
        );
        let _a_guard = protect(a);
        let _b_guard = protect(b);
        Some(binary_compare(op, a, b))
    }
}
pub(crate) unsafe fn posixlt_as_posixct_operand(call: SEXP, x: SEXP) -> SEXP {
    unsafe {
        if crate::mainutils::essentials::sexp_has_class(x, "POSIXlt") {
            let ct = crate::mainutils::essentials::do_as_POSIXct(
                call,
                R_NilValue(),
                Rf_cons(x, R_NilValue()),
                R_NilValue(),
            );
            let _ct = protect(ct);
            ct
        } else {
            x
        }
    }
}

pub(crate) unsafe fn posixct_binary_comparison(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_posixct = crate::mainutils::essentials::sexp_has_class(a, "POSIXct");
        let b_is_posixct = crate::mainutils::essentials::sexp_has_class(b, "POSIXct");
        if !a_is_posixct && !b_is_posixct {
            return None;
        }

        let a = coerce_character_comparison_operand(a, parse_posixct_comparison_seconds);
        let b = coerce_character_comparison_operand(b, parse_posixct_comparison_seconds);
        let _a_guard = protect(a);
        let _b_guard = protect(b);
        Some(binary_compare(op, a, b))
    }
}

pub(crate) fn auto_difftime_units(seconds: &[f64]) -> (&'static str, f64) {
    let min_abs = seconds
        .iter()
        .copied()
        .filter(|value| value.to_bits() != R_NA_BIT_PATTERN && value.is_finite())
        .map(f64::abs)
        .fold(f64::INFINITY, f64::min);
    if !min_abs.is_finite() || min_abs < 60.0 {
        ("secs", 1.0)
    } else if min_abs < 3_600.0 {
        ("mins", 60.0)
    } else if min_abs < 86_400.0 {
        ("hours", 3_600.0)
    } else {
        ("days", 86_400.0)
    }
}

pub(crate) unsafe fn set_difftime_attributes(result: SEXP, units: &str) {
    unsafe {
        set_string_attribute(result, "units", units);
        set_single_string_class(result, "difftime");
    }
}

pub(crate) unsafe fn date_binary_arithmetic(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_date = crate::mainutils::essentials::sexp_has_class(a, "Date");
        let b_is_date = crate::mainutils::essentials::sexp_has_class(b, "Date");
        if !a_is_date && !b_is_date {
            return None;
        }

        match op {
            "+" if a_is_date && b_is_date => {
                arithmetic_error("binary + is not defined for \"Date\" objects");
            }
            "+" if a_is_date || b_is_date => {
                let a = coerce_difftime_operand(a, 86_400.0, true);
                let b = coerce_difftime_operand(b, 86_400.0, true);
                let _a_guard = protect(a);
                let _b_guard = protect(b);
                let result = real_binary(op, a, b);
                set_single_string_class(result, "Date");
                Some(result)
            }
            "-" if a_is_date && b_is_date => {
                let result = real_binary(op, a, b);
                set_difftime_attributes(result, "days");
                Some(result)
            }
            "-" if a_is_date => {
                if has_class_attribute(b)
                    && !crate::mainutils::essentials::sexp_has_class(b, "difftime")
                {
                    arithmetic_error("can only subtract numbers from \"Date\" objects");
                }
                let b = coerce_difftime_operand(b, 86_400.0, true);
                let _b_guard = protect(b);
                let result = real_binary(op, a, b);
                set_single_string_class(result, "Date");
                Some(result)
            }
            "-" if b_is_date => {
                arithmetic_error("can only subtract from \"Date\" objects");
            }
            "*" | "/" | "^" | "%%" | "%/%" => {
                arithmetic_error(format!("{op} not defined for \"Date\" objects"));
            }
            _ => None,
        }
    }
}

pub(crate) unsafe fn difftime_binary_arithmetic(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_difftime = crate::mainutils::essentials::sexp_has_class(a, "difftime");
        let b_is_difftime = crate::mainutils::essentials::sexp_has_class(b, "difftime");
        if !a_is_difftime && !b_is_difftime {
            return None;
        }

        match op {
            // Operands reaching the arithmetic primitive with a difftime
            // class have already been unit-coerced by Ops.difftime (the
            // primitive is entered with dispatch suppressed via the
            // `.default` call name). GNU's R_binary therefore does raw
            // arithmetic and never rescales: converting here would apply
            // the scale twice (e.g. `dt + half` -> 3630 secs).
            "+" | "-" => {
                let result = real_binary(op, a, b);
                set_difftime_attributes(result, &difftime_units(if a_is_difftime { a } else { b }));
                Some(result)
            }
            "*" if a_is_difftime && b_is_difftime => {
                arithmetic_error("both arguments of * cannot be \"difftime\" objects");
            }
            "*" if a_is_difftime || b_is_difftime => {
                let result = real_binary(op, a, b);
                set_difftime_attributes(result, &difftime_units(if a_is_difftime { a } else { b }));
                Some(result)
            }
            "/" if b_is_difftime => {
                arithmetic_error("second argument of / cannot be a \"difftime\" object");
            }
            "/" if a_is_difftime => {
                let result = real_binary(op, a, b);
                set_difftime_attributes(result, &difftime_units(a));
                Some(result)
            }
            "^" | "%%" | "%/%" => {
                arithmetic_error(format!("'{op}' not defined for \"difftime\" objects"));
            }
            _ => None,
        }
    }
}

pub(crate) unsafe fn posixct_binary_arithmetic(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let a_is_posixct = crate::mainutils::essentials::sexp_has_class(a, "POSIXct");
        let b_is_posixct = crate::mainutils::essentials::sexp_has_class(b, "POSIXct");
        if !a_is_posixct && !b_is_posixct {
            return None;
        }

        match op {
            "+" if a_is_posixct && b_is_posixct => {
                arithmetic_error("binary '+' is not defined for \"POSIXt\" objects");
            }
            "+" if a_is_posixct || b_is_posixct => {
                let posixct_source = if a_is_posixct { a } else { b };
                let a = coerce_difftime_operand(a, 1.0, false);
                let b = coerce_difftime_operand(b, 1.0, false);
                let _a_guard = protect(a);
                let _b_guard = protect(b);
                let result = real_binary(op, a, b);
                set_posixct_attributes_from(result, posixct_source);
                Some(result)
            }
            "-" if a_is_posixct && b_is_posixct => {
                let result = real_binary(op, a, b);
                let Some(result_sexp) = Sexp::from_raw(result) else {
                    return Some(result);
                };
                let values_len = result_sexp.len();
                let values: Vec<f64> = (0..values_len)
                    .filter_map(|i| result_sexp.try_real_elt(i).ok())
                    .collect();
                let (units, scale) = auto_difftime_units(&values);
                // SAFETY: result is freshly allocated; no payload reference was lent.
                let mut result_mut = SexpMut::from_owned(result_sexp);
                for i in 0..result_mut.len() {
                    if let Ok(value) = result_mut.try_real_elt(i)
                        && value.to_bits() != R_NA_BIT_PATTERN
                    {
                        result_mut.set_real_elt(i, value / scale);
                    }
                }
                let _ = result_mut.freeze();
                set_difftime_attributes(result, units);
                Some(result)
            }
            "-" if a_is_posixct => {
                if has_class_attribute(b)
                    && !crate::mainutils::essentials::sexp_has_class(b, "difftime")
                {
                    arithmetic_error("can only subtract numbers from \"POSIXt\" objects");
                }
                let b = coerce_difftime_operand(b, 1.0, false);
                let _b_guard = protect(b);
                let result = real_binary(op, a, b);
                set_posixct_attributes_from(result, a);
                Some(result)
            }
            "-" if b_is_posixct => {
                arithmetic_error("can only subtract from \"POSIXt\" objects");
            }
            "*" | "/" | "^" | "%%" | "%/%" => {
                arithmetic_error(format!("'{op}' not defined for \"POSIXt\" objects"));
            }
            _ => None,
        }
    }
}

pub(crate) unsafe fn propagate_unary_vector_attributes(
    result: SEXP,
    source: SEXP,
    result_len: R_xlen_t,
) {
    unsafe {
        if copy_dims_if_present(result, source, result_len) {
            return;
        }
        if LENGTH(source) as R_xlen_t == result_len {
            copy_attr_if_present(result, source, R_NamesSymbol());
        }
        if has_class(source, "ts") || has_class(source, "mts") {
            copy_attr_if_present(result, source, crate::sexp::attrib_core::R_ClassSymbol());
            copy_attr_if_present(result, source, crate::sexp::attrib_core::R_TspSymbol());
        }
    }
}
