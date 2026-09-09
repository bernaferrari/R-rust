//! Unevaluated-expression counterpart of `eval`, with GNU environment defaults.
use crate::eval::eval::Rf_eval;
use crate::eval::missing::VectorToPairListNamed;
use crate::eval::runtime::base_env;
use crate::sexp::accessors::*;
use crate::sexp::constructors::Rf_allocList;
use crate::sexp::context::{RError, RSignal, begin_context_guard, ctxt_flags};
use crate::sexp::ffi::{NA_INTEGER, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::memory_ext::NewEnvironment;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

fn fail(message: &str) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    })
}

pub unsafe fn do_evalq(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let formals = Rf_allocList(3);
        let _formals_root = protect(formals);
        let mut cell = formals;
        for name in [c"expr", c"envir", c"enclos"] {
            SETCAR(cell, R_MissingArg());
            SETTAG(cell, Rf_install(name.as_ptr()));
            cell = CDR(cell);
        }
        let actuals =
            super::closure::match_closure_args(formals, args).unwrap_or_else(|e| fail(&e));
        let _actuals_root = protect(actuals);
        let expr = CAR(actuals);
        if expr == R_MissingArg() {
            fail("argument 'expr' is missing, with no default")
        }
        let envir_expr = CADR(actuals);
        let enclos_expr = CADDR(actuals);
        let mut env = if envir_expr == R_MissingArg() {
            rho
        } else {
            Rf_eval(envir_expr, rho)
        };
        let _input_root = protect(env);
        let enclosure = if enclos_expr == R_MissingArg() {
            if TYPEOF(env) == SEXPTYPE::VECSXP || TYPEOF(env) == SEXPTYPE::LISTSXP {
                rho
            } else {
                base_env()
            }
        } else {
            Rf_eval(enclos_expr, rho)
        };
        let enclosure = if enclosure == R_NilValue() {
            base_env()
        } else {
            enclosure
        };
        let _enclosure_root = protect(enclosure);
        if TYPEOF(enclosure) != SEXPTYPE::ENVSXP {
            fail("invalid 'enclos' argument")
        }
        match TYPEOF(env) {
            t if t == SEXPTYPE::ENVSXP => {}
            t if t == SEXPTYPE::NILSXP => env = enclosure,
            t if t == SEXPTYPE::LISTSXP => {
                let frame = crate::mainutils::duplicate::Rf_duplicate(env);
                let _frame_root = protect(frame);
                env = NewEnvironment(frame, enclosure, R_NilValue());
            }
            t if t == SEXPTYPE::VECSXP => {
                let frame = VectorToPairListNamed(env);
                let _frame_root = protect(frame);
                let mut entry = frame;
                while entry != R_NilValue() && !entry.is_null() {
                    SET_NAMED(CAR(entry), 2);
                    entry = CDR(entry);
                }
                env = NewEnvironment(frame, enclosure, R_NilValue());
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP => {
                if XLENGTH(env) != 1 {
                    fail("numeric 'envir' arg not of length one")
                }
                let frame = crate::mainutils::coerce::asInteger(env);
                if frame == NA_INTEGER {
                    fail("invalid 'envir' argument")
                }
                // The port exposes evalq directly, without GNU's wrapper closure.
                // Account for that omitted frame when resolving relative indices.
                env = if frame == -1 {
                    rho
                } else {
                    super::context::R_sysframe(
                        if frame < -1 { frame + 1 } else { frame },
                        std::ptr::null_mut(),
                    )
                };
            }
            _ => {
                let kind =
                    std::ffi::CStr::from_ptr(crate::mainutils::util_main::type2char(TYPEOF(env)))
                        .to_string_lossy();
                fail(&format!("invalid 'envir' argument of type '{kind}'"))
            }
        }
        let _env_root = protect(env);
        let _context =
            begin_context_guard(ctxt_flags::CTXT_RETURN, call, env, rho, None, op, actuals);
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Rf_eval(expr, env))) {
            Ok(value) => value,
            Err(payload) => match payload.downcast::<RSignal>() {
                Ok(signal) => match *signal {
                    RSignal::Return(value) => value,
                    other => std::panic::resume_unwind(Box::new(other)),
                },
                Err(payload) => std::panic::resume_unwind(payload),
            },
        }
    }
}
