use super::*;

// ---------------------------------------------------------------------------
// S3 print/summary dispatch
// ---------------------------------------------------------------------------

/// GNU `.Internal(print.default(x, args, missings))`. Extra arguments
/// (including `print(x, useS4 = FALSE)` from `showDefault`) set
/// `noParams = 0` so S4 objects print via `PrintValueRec` instead of
/// re-entering `show()`.
pub unsafe fn do_print_default(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::print::do_printdefault(call, op, args, rho) }
}

/// GNU `print.Date(x, max = NULL, ...)`.
pub unsafe fn do_print_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return R_NilValue();
        }
        let mut max = None;
        let max_sym = Rf_install(c"max".as_ptr());
        let mut p = CDR(args);
        while !p.is_null() && p != R_NilValue() {
            if TAG(p) == max_sym {
                let v = CAR(p);
                if !v.is_null() && v != R_NilValue() {
                    if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                        max = Some(*INTEGER(v) as i64);
                    } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                        max = Some(*REAL(v) as i64);
                    }
                }
                break;
            }
            p = CDR(p);
        }
        if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            let text = crate::sexp::output::format_date_vector_max(sexp, max);
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&format!("{text}\n"));
            } else {
                println!("{text}");
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// GNU `print.POSIXct(x, ..., usetz = TRUE, digits = getOption("digits.secs"))`.
pub unsafe fn do_print_POSIXct(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) == 0 {
            let text = "POSIXct of length 0";
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&format!("{text}\n"));
            } else {
                println!("{text}");
            }
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return if x.is_null() || x == R_NilValue() {
                R_NilValue()
            } else {
                x
            };
        }

        let mut max = None;
        let mut width = None;
        let max_sym = Rf_install(c"max".as_ptr());
        let digits_sym = Rf_install(c"digits".as_ptr());
        let usetz_sym = Rf_install(c"usetz".as_ptr());
        let width_sym = Rf_install(c"width".as_ptr());
        let mut digits = R_NilValue();
        let mut usetz_val = TRUE;
        let mut p = CDR(args);
        while !p.is_null() && p != R_NilValue() {
            if TAG(p) == max_sym {
                let v = CAR(p);
                if !v.is_null() && v != R_NilValue() {
                    if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                        max = Some(*INTEGER(v) as i64);
                    } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                        max = Some(*REAL(v) as i64);
                    }
                }
            } else if TAG(p) == digits_sym {
                digits = CAR(p);
            } else if TAG(p) == usetz_sym {
                let v = CAR(p);
                if TYPEOF(v) == SEXPTYPE::LGLSXP && XLENGTH(v) > 0 {
                    usetz_val = *INTEGER(v);
                }
            } else if TAG(p) == width_sym {
                let v = CAR(p);
                if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                    width = Some(*INTEGER(v));
                } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                    width = Some(*REAL(v) as i32);
                }
            }
            p = CDR(p);
        }

        let usetz = Rf_ScalarLogical(usetz_val);
        let _u = protect(usetz);
        let mut rest = Rf_cons(usetz, R_NilValue());
        SETTAG(rest, usetz_sym);
        let _r0 = protect(rest);
        if !digits.is_null() && digits != R_NilValue() {
            let cell = Rf_cons(digits, rest);
            SETTAG(cell, digits_sym);
            rest = cell;
            let _rd = protect(rest);
        }

        let fmt_args = Rf_cons(x, rest);
        let _fa = protect(fmt_args);
        let formatted = crate::mainutils::datetime::do_format_POSIXct(call, op, fmt_args, rho);
        let _f = protect(formatted);
        if let Some(sexp) = crate::sexp::object::Sexp::from_raw(formatted) {
            let print_max =
                max.unwrap_or_else(|| crate::mainutils::options::GetOptionMaxPrint() as i64);
            let old_width = width.map(|w| crate::mainutils::options::R_SetOptionWidth(w));
            let text = crate::sexp::output::format_vector_stock_n(sexp, true, Some(print_max));
            if let Some(old) = old_width {
                crate::mainutils::options::R_SetOptionWidth(old);
            }
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&format!("{text}\n"));
            } else {
                println!("{text}");
            }
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}
