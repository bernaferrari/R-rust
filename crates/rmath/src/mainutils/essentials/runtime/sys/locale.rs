use super::*;

#[cfg(target_arch = "wasm32")]
const LC_CTYPE: c_int = 0;

#[cfg(target_arch = "wasm32")]
const LC_NUMERIC: c_int = 1;

#[cfg(target_arch = "wasm32")]
const LC_TIME: c_int = 2;

#[cfg(target_arch = "wasm32")]
const LC_COLLATE: c_int = 3;

#[cfg(target_arch = "wasm32")]
const LC_MONETARY: c_int = 4;

#[cfg(target_arch = "wasm32")]
const LC_MESSAGES: c_int = 5;

#[cfg(target_arch = "wasm32")]
const LC_ALL: c_int = 6;

/// Query or set the process locale via libc `setlocale(3)`.
#[cfg(not(target_arch = "wasm32"))]
#[inline]
unsafe fn r_setlocale(category: c_int, locale: *const c_char) -> *mut c_char {
    unsafe { libc::setlocale(category, locale) }
}

/// wasm32 stub: every query resolves to the "C" locale and setting any
/// locale leaves it unchanged ("C" is the effective locale either way).
#[cfg(target_arch = "wasm32")]
#[inline]
unsafe fn r_setlocale(_category: c_int, _locale: *const c_char) -> *mut c_char {
    b"C\0".as_ptr() as *mut c_char
}

// ---------------------------------------------------------------------------
// Complete R runtime — Sys.* functions, R.home
// ---------------------------------------------------------------------------

/// R's `Sys.localeconv()` — locale formatting conventions.
pub unsafe fn do_Sys_localeconv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = [
            "decimal_point",
            "thousands_sep",
            "grouping",
            "int_curr_symbol",
            "currency_symbol",
            "mon_decimal_point",
            "mon_thousands_sep",
            "mon_grouping",
            "positive_sign",
            "negative_sign",
            "int_frac_digits",
            "frac_digits",
            "p_cs_precedes",
            "p_sep_by_space",
            "n_cs_precedes",
            "n_sep_by_space",
            "p_sign_posn",
            "n_sign_posn",
        ];
        let values = [
            ".", "", "", "", "", ".", "", "", "", "", "127", "127", "127", "127", "127", "127",
            "127", "127",
        ];
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let name_vec = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        let _names_guard = protect(name_vec);
        for (i, (name, value)) in names.iter().zip(values.iter()).enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(*value).unwrap_or_default().as_ptr()),
            );
            SET_STRING_ELT(
                name_vec,
                i as R_xlen_t,
                Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            name_vec,
        );
        result
    }
}

/// R's `Sys.getlocale(category)` — get locale (simplified).
pub unsafe fn do_Sys_getlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let category = locale_category_from_arg(CAR(args));
        locale_string_from_libc(category)
    }
}

/// R's `Sys.setlocale(category, locale)` — set locale (simplified).
pub unsafe fn do_Sys_setlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let allowed = crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst).eval_state.capabilities.allow_environment_mutation
        });
        if !allowed {
            return Rf_mkString(c"".as_ptr());
        }

        let category = locale_category_from_arg(CAR(args));
        let locale_arg = CAR(CDR(args));
        let locale = locale_string_arg(locale_arg);
        let locale_ptr = match locale.as_ref() {
            Some(locale) => locale.as_ptr(),
            None => std::ptr::null(),
        };
        let result = r_setlocale(category, locale_ptr);
        if result.is_null() {
            Rf_mkString(b"\0".as_ptr() as *const c_char)
        } else {
            Rf_mkString(result)
        }
    }
}

unsafe fn locale_category_from_arg(category: SEXP) -> c_int {
    unsafe {
        if category.is_null() || category == R_NilValue() {
            return LC_ALL;
        }

        match TYPEOF(category) {
            t if t == SEXPTYPE::STRSXP => {
                let name = elt_to_string(category, 0);
                locale_category_from_name(&name)
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => match *INTEGER(category) {
                1 => LC_ALL,
                2 => LC_COLLATE,
                3 => LC_CTYPE,
                4 => LC_MONETARY,
                5 => LC_NUMERIC,
                6 => LC_TIME,
                7 => LC_MESSAGES,
                _ => base_error("invalid 'category' argument"),
            },
            _ => base_error("invalid 'category' argument"),
        }
    }
}

fn locale_category_from_name(name: &str) -> c_int {
    match name {
        "LC_ALL" => LC_ALL,
        "LC_COLLATE" => LC_COLLATE,
        "LC_CTYPE" => LC_CTYPE,
        "LC_MONETARY" => LC_MONETARY,
        "LC_NUMERIC" => LC_NUMERIC,
        "LC_TIME" => LC_TIME,
        "LC_MESSAGES" => LC_MESSAGES,
        _ => base_error("invalid 'category' argument"),
    }
}

unsafe fn locale_string_arg(locale: SEXP) -> Option<CString> {
    unsafe {
        if locale.is_null() || locale == R_NilValue() {
            return None;
        }
        if TYPEOF(locale) != SEXPTYPE::STRSXP || XLENGTH(locale) == 0 {
            base_error("invalid 'locale' argument");
        }
        CString::new(elt_to_string(locale, 0))
            .map(Some)
            .unwrap_or_else(|_| base_error("invalid 'locale' argument"))
    }
}

unsafe fn locale_string_from_libc(category: c_int) -> SEXP {
    unsafe {
        let result = r_setlocale(category, std::ptr::null());
        if result.is_null() {
            Rf_mkString(b"\0".as_ptr() as *const c_char)
        } else {
            Rf_mkString(result)
        }
    }
}
