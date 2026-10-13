//! Natural-language translation: gettext, ngettext, bindtextdomain

use super::*;

/// GNU `gettext(...)` without catalogs is identity.
pub unsafe fn do_gettext(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            return x;
        }
        x
    }
}

/// GNU `gettextf(fmt, ..., domain=, trim=)` — sprintf of untranslated fmt.
pub unsafe fn do_gettextf(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fmt = CAR(args);
        let mut sprintf_tail = R_NilValue();
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let skip = if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy();
                name == "domain" || name == "trim"
            } else {
                false
            };
            if !skip {
                sprintf_tail = Rf_cons(CAR(cell), sprintf_tail);
            }
            cell = CDR(cell);
        }
        // Rf_cons prepends; reverse by walking into a new list.
        let mut sprintf_args = R_NilValue();
        let mut rev = sprintf_tail;
        while !rev.is_null() && rev != R_NilValue() {
            sprintf_args = Rf_cons(CAR(rev), sprintf_args);
            rev = CDR(rev);
        }
        sprintf_args = Rf_cons(fmt, sprintf_args);
        crate::mainutils::sprintf_main::do_sprintf(call, op, sprintf_args, rho)
    }
}

/// GNU `ngettext(n, msg1, msg2)` without catalogs.
pub unsafe fn do_ngettext(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP && XLENGTH(n_s) > 0 {
            *INTEGER(n_s)
        } else if TYPEOF(n_s) == SEXPTYPE::REALSXP && XLENGTH(n_s) > 0 {
            *REAL(n_s) as i32
        } else {
            0
        };
        let msg1 = CAR(CDR(args));
        let msg2 = CAR(CDR(CDR(args)));
        let pick = if n == 1 { msg1 } else { msg2 };
        if pick.is_null() || pick == R_NilValue() {
            return Rf_mkString(c"".as_ptr());
        }
        if TYPEOF(pick) == SEXPTYPE::STRSXP && XLENGTH(pick) == 1 {
            return pick;
        }
        if TYPEOF(pick) == SEXPTYPE::STRSXP && XLENGTH(pick) > 0 {
            let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            let _o = protect(out);
            SET_STRING_ELT(out, 0, STRING_ELT(pick, 0));
            return out;
        }
        pick
    }
}

fn bindtextdomain_map() -> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    static MAP: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    &MAP
}

/// GNU `bindtextdomain(domain, dirname)`.
pub unsafe fn do_bindtextdomain(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let domain = CAR(args);
        let dirname = CAR(CDR(args));
        if (domain.is_null() || domain == R_NilValue())
            && (dirname.is_null() || dirname == R_NilValue())
        {
            return Rf_ScalarLogical(TRUE);
        }
        if domain.is_null()
            || domain == R_NilValue()
            || TYPEOF(domain) != SEXPTYPE::STRSXP
            || XLENGTH(domain) != 1
        {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'domain' value",
            );
        }
        let dch = STRING_ELT(domain, 0);
        let dname = if dch.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(CHAR(dch))
                .to_string_lossy()
                .into_owned()
        };
        if dirname.is_null() || dirname == R_NilValue() {
            let path = bindtextdomain_map()
                .lock()
                .ok()
                .and_then(|map| map.get(&dname).cloned())
                .unwrap_or_default();
            let c = CString::new(path).unwrap_or_else(|_| CString::new("").unwrap());
            return Rf_mkString(c.as_ptr());
        }
        if TYPEOF(dirname) != SEXPTYPE::STRSXP || XLENGTH(dirname) != 1 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'dirname' value",
            );
        }
        let pch = STRING_ELT(dirname, 0);
        let path = if pch.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(CHAR(pch))
                .to_string_lossy()
                .into_owned()
        };
        if let Ok(mut map) = bindtextdomain_map().lock() {
            map.insert(dname, path.clone());
        }
        let c = CString::new(path).unwrap_or_else(|_| CString::new("").unwrap());
        Rf_mkString(c.as_ptr())
    }
}
