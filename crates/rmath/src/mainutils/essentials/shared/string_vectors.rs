//! String vector construction helpers.

use super::*;

pub(crate) unsafe fn datetime_c_value(
    source: SEXP,
    index: R_xlen_t,
    class: DatetimeVectorClass,
) -> f64 {
    unsafe {
        match class {
            DatetimeVectorClass::Date => {
                if TYPEOF(source) == SEXPTYPE::STRSXP {
                    let value = STRING_ELT(source, index);
                    if value.is_null() || value == crate::sexp::globals::R_NaString() {
                        return NA_REAL;
                    }
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
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
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
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
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) unsafe fn string_vector(values: &[String]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in values.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

pub(crate) unsafe fn static_string_vector(values: &[&str]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in values.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(*value).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

pub(crate) unsafe fn optional_string_vector(values: &[Option<String>]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in values.iter().enumerate() {
            let charsxp = match value {
                Some(value) => Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
                None => crate::sexp::globals::R_NaString(),
            };
            SET_STRING_ELT(result, i as R_xlen_t, charsxp);
        }
        result
    }
}

pub(crate) unsafe fn named_string_vector(values: &[String], names: &[String]) -> SEXP {
    unsafe {
        let result = string_vector(values);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        set_string_names(result, names);
        result
    }
}

pub(crate) unsafe fn named_string_list(items: impl IntoIterator<Item = (String, String)>) -> SEXP {
    unsafe {
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
                Rf_mkChar(CString::new(name.as_str()).unwrap_or_default().as_ptr()),
            );
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        result
    }
}
