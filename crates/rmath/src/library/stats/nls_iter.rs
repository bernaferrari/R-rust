//! GNU `nls_iter` (`stats/src/nls.c`): Gauss-Newton over an nlsModel.

use crate::sexp::ffi::{SEXP, SEXPTYPE};

unsafe fn named(list: SEXP, name: &str) -> SEXP {
    unsafe {
        if list.is_null() || crate::sexp::accessors::TYPEOF(list) != SEXPTYPE::VECSXP {
            return crate::sexp::globals::R_NilValue();
        }
        let names = crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
        let n = crate::sexp::accessors::XLENGTH(list);
        for i in 0..n {
            if crate::sexp::accessors::TYPEOF(names) == SEXPTYPE::STRSXP {
                let ch = crate::sexp::accessors::STRING_ELT(names, i);
                if !ch.is_null() {
                    let text = std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(ch))
                        .to_string_lossy();
                    if text == name {
                        return crate::sexp::accessors::VECTOR_ELT(list, i);
                    }
                }
            }
        }
        crate::sexp::globals::R_NilValue()
    }
}

unsafe fn as_f64(x: SEXP) -> f64 {
    unsafe {
        if crate::sexp::accessors::TYPEOF(x) == SEXPTYPE::REALSXP
            && crate::sexp::accessors::XLENGTH(x) > 0
        {
            *crate::sexp::accessors::REAL(x)
        } else if crate::sexp::accessors::TYPEOF(x) == SEXPTYPE::INTSXP
            && crate::sexp::accessors::XLENGTH(x) > 0
        {
            *crate::sexp::accessors::INTEGER(x) as f64
        } else {
            f64::NAN
        }
    }
}
unsafe fn eval_global(call: SEXP) -> SEXP {
    unsafe { crate::eval::eval::Rf_eval(call, crate::sexp::globals::R_GlobalEnv()) }
}

unsafe fn lang1(f: SEXP) -> SEXP {
    unsafe {
        let call = crate::sexp::constructors::Rf_cons(f, crate::sexp::globals::R_NilValue());
        (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        call
    }
}

unsafe fn lang2(f: SEXP, arg: SEXP) -> SEXP {
    unsafe {
        let call = crate::sexp::constructors::Rf_cons(
            f,
            crate::sexp::constructors::Rf_cons(arg, crate::sexp::globals::R_NilValue()),
        );
        (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        call
    }
}

unsafe fn conv_info(msg: &str, iter: i32, why: i32, conv_new: f64) -> SEXP {
    unsafe {
        let ans = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::VECSXP, 5);
        let _g = crate::sexp::protect::protect(ans);
        let names = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, 5);
        for (i, name) in ["isConv", "finIter", "finTol", "stopCode", "stopMessage"]
            .iter()
            .enumerate()
        {
            let c = std::ffi::CString::new(*name).unwrap();
            crate::sexp::accessors::SET_STRING_ELT(
                names,
                i as i64,
                crate::sexp::constructors::Rf_mkChar(c.as_ptr()),
            );
        }
        crate::sexp::attrib_core::setAttrib(ans, crate::sexp::attrib_core::R_NamesSymbol(), names);
        crate::sexp::accessors::SET_VECTOR_ELT(
            ans,
            0,
            crate::sexp::constructors::Rf_ScalarLogical(if why == 0 { 1 } else { 0 }),
        );
        crate::sexp::accessors::SET_VECTOR_ELT(ans, 1, crate::sexp::constructors::Rf_ScalarInteger(iter));
        crate::sexp::accessors::SET_VECTOR_ELT(ans, 2, crate::sexp::constructors::Rf_ScalarReal(conv_new));
        crate::sexp::accessors::SET_VECTOR_ELT(ans, 3, crate::sexp::constructors::Rf_ScalarInteger(why));
        let cmsg = std::ffi::CString::new(msg).unwrap_or_default();
        crate::sexp::accessors::SET_VECTOR_ELT(
            ans,
            4,
            crate::sexp::constructors::Rf_mkString(cmsg.as_ptr()),
        );
        ans
    }
}

pub unsafe extern "C-unwind" fn c_nls_iter(m: SEXP, control: SEXP, do_trace_arg: SEXP) -> SEXP {
    unsafe {
        let do_trace = crate::main::coerce::asLogical(do_trace_arg) == 1;
        if crate::sexp::accessors::TYPEOF(control) != SEXPTYPE::VECSXP {
            crate::mainutils::errors::errorcall_str(std::ptr::null_mut(), "'control' must be a list");
        }
        if crate::sexp::accessors::TYPEOF(m) != SEXPTYPE::VECSXP {
            crate::mainutils::errors::errorcall_str(std::ptr::null_mut(), "'m' must be a list");
        }
        let max_iter = crate::main::coerce::asInteger(named(control, "maxiter"));
        let tolerance = as_f64(named(control, "tol"));
        let min_fac = as_f64(named(control, "minFactor"));
        let warn_only = crate::main::coerce::asLogical(named(control, "warnOnly")) == 1;
        let print_eval = crate::main::coerce::asLogical(named(control, "printEval")) == 1;

        let conv = lang1(named(m, "conv"));
        let _c = crate::sexp::protect::protect(conv);
        let incr = lang1(named(m, "incr"));
        let _i = crate::sexp::protect::protect(incr);
        let deviance = lang1(named(m, "deviance"));
        let _d = crate::sexp::protect::protect(deviance);
        let trace = lang1(named(m, "trace"));
        let _t = crate::sexp::protect::protect(trace);
        let set_pars = named(m, "setPars");
        let get_pars = lang1(named(m, "getPars"));
        let _g = crate::sexp::protect::protect(get_pars);

        let mut pars = eval_global(get_pars);
        let _p = crate::sexp::protect::protect(pars);
        let n_pars = crate::sexp::accessors::XLENGTH(pars) as usize;
        let mut dev = as_f64(eval_global(deviance));
        if do_trace {
            eval_global(trace);
        }
        let mut fac = 1.0;
        let mut converged = false;
        let mut new_pars = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::REALSXP, n_pars as i64);
        let _np = crate::sexp::protect::protect(new_pars);
        let mut conv_new = -1.0;
        let mut i = 0i32;
        while i < max_iter {
            conv_new = as_f64(eval_global(conv));
            if conv_new <= tolerance {
                converged = true;
                break;
            }
            let new_incr = eval_global(incr);
            let _ni = crate::sexp::protect::protect(new_incr);
            while fac >= min_fac {
                for j in 0..n_pars {
                    *crate::sexp::accessors::REAL(new_pars).add(j) =
                        *crate::sexp::accessors::REAL(pars).add(j)
                            + fac * *crate::sexp::accessors::REAL(new_incr).add(j);
                }
                let call = lang2(set_pars, new_pars);
                let _call = crate::sexp::protect::protect(call);
                if crate::main::coerce::asLogical(eval_global(call)) == 1 {
                    let msg = "singular gradient";
                    if warn_only {
                        let c = std::ffi::CString::new(msg).unwrap();
                        crate::mainutils::errors::Rf_warning(c.as_ptr());
                        return conv_info(msg, i, 1, conv_new);
                    }
                    crate::mainutils::errors::errorcall_str(std::ptr::null_mut(), msg);
                }
                let new_dev = as_f64(eval_global(deviance));
                if new_dev <= dev {
                    dev = new_dev;
                    for j in 0..n_pars {
                        *crate::sexp::accessors::REAL(pars).add(j) =
                            *crate::sexp::accessors::REAL(new_pars).add(j);
                    }
                    fac = (2.0 * fac).min(1.0);
                    break;
                }
                fac /= 2.0;
            }
            if do_trace {
                eval_global(trace);
            }
            if fac < min_fac {
                let msg = format!("step factor {fac} reduced below 'minFactor' of {min_fac}");
                if warn_only {
                    let c = std::ffi::CString::new(msg.clone()).unwrap();
                    crate::mainutils::errors::Rf_warning(c.as_ptr());
                    return conv_info(&msg, i, 2, conv_new);
                }
                crate::mainutils::errors::errorcall_str(std::ptr::null_mut(), &msg);
            }
            i += 1;
        }
        if !converged {
            let msg = format!("number of iterations exceeded maximum of {max_iter}");
            if warn_only {
                let c = std::ffi::CString::new(msg.clone()).unwrap();
                crate::mainutils::errors::Rf_warning(c.as_ptr());
                return conv_info(&msg, i, 3, conv_new);
            }
            crate::mainutils::errors::errorcall_str(std::ptr::null_mut(), &msg);
        }
        conv_info("converged", i, 0, conv_new)
    }
}
