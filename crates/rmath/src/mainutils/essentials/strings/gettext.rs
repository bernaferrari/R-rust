//! Natural-language translation: gettext, ngettext, bindtextdomain

use super::*;


/// GNU `gettextf(fmt, ..., domain=, trim=)` — sprintf of untranslated fmt.
pub fn do_gettextf(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fmt = CAR(args);
        let mut sprintf_tail = R_NilValue();
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let skip = if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let name = charsxp_text(PRINTNAME(tag)).unwrap_or_default();
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



