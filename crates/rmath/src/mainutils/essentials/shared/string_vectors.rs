//! String vector construction helpers.

use super::*;

pub(crate) fn datetime_c_value(source: SEXP, index: R_xlen_t, class: DatetimeVectorClass) -> f64 {
    match class {
        DatetimeVectorClass::Date => {
            if TYPEOF(source) == SEXPTYPE::STRSXP {
                let value = STRING_ELT(source, index);
                if value.is_null() || value == crate::sexp::globals::R_NaString() {
                    return NA_REAL;
                }
                let text_cstr = charsxp_cstring(value);
                let text = text_cstr.to_str().unwrap_or("");
                if text.is_empty() {
                    return NA_REAL;
                }
                return parse_iso_date_days(text).unwrap_or_else(|| {
                    base_error("character string is not in a standard unambiguous format");
                });
            }
            let value = real_elt_or_default(source, index, NA_REAL);
            if sexp_has_class(source, "POSIXct") && value.to_bits() != R_NA_BIT_PATTERN {
                (value / 86_400.0).floor()
            } else {
                value
            }
        }
        DatetimeVectorClass::Posixct => {
            if TYPEOF(source) == SEXPTYPE::STRSXP {
                let value = STRING_ELT(source, index);
                if value.is_null() || value == crate::sexp::globals::R_NaString() {
                    return NA_REAL;
                }
                let text_cstr = charsxp_cstring(value);
                let text = text_cstr.to_str().unwrap_or("");
                return parse_iso_datetime_seconds(text).unwrap_or_else(|| {
                    base_error("character string is not in a standard unambiguous format");
                });
            }
            let value = real_elt_or_default(source, index, NA_REAL);
            if sexp_has_class(source, "Date") && value.to_bits() != R_NA_BIT_PATTERN {
                value.floor() * 86_400.0
            } else {
                value
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn string_vector(values: &[String]) -> SEXP {
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    for (i, value) in values.iter().enumerate() {
        SET_STRING_ELT(
            result,
            i as R_xlen_t,
            mk_char_cstr(&CString::new(value.as_str()).unwrap_or_default()),
        );
    }
    result
}

pub(crate) fn static_string_vector(values: &[&str]) -> SEXP {
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    for (i, value) in values.iter().enumerate() {
        SET_STRING_ELT(
            result,
            i as R_xlen_t,
            mk_char_cstr(&CString::new(*value).unwrap_or_default()),
        );
    }
    result
}

pub(crate) fn optional_string_vector(values: &[Option<String>]) -> SEXP {
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    for (i, value) in values.iter().enumerate() {
        let charsxp = match value {
            Some(value) => mk_char_cstr(&CString::new(value.as_str()).unwrap_or_default()),
            None => crate::sexp::globals::R_NaString(),
        };
        SET_STRING_ELT(result, i as R_xlen_t, charsxp);
    }
    result
}

pub(crate) fn named_string_vector(values: &[String], names: &[String]) -> SEXP {
    {
        let result = string_vector(values);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        set_string_names(result, names);
        result
    }
}

pub(crate) fn named_string_list(items: impl IntoIterator<Item = (String, String)>) -> SEXP {
    let items = items.into_iter().collect::<Vec<_>>();
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, items.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);

    let names = Rf_allocVector3(SEXPTYPE::STRSXP, items.len() as R_xlen_t);
    if names.is_null() {
        return R_NilValue();
    }
    let _names_guard = protect(names);

    for (i, (name, value)) in items.iter().enumerate() {
        let value_vec = string_vector(std::slice::from_ref(value));
        SET_VECTOR_ELT(result, i as R_xlen_t, value_vec);
        SET_STRING_ELT(
            names,
            i as R_xlen_t,
            mk_char_cstr(&CString::new(name.as_str()).unwrap_or_default()),
        );
    }

    crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_NamesSymbol(), names);
    result
}
