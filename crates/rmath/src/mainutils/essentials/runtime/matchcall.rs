//! `match.call`, `sys.nframe`, `sys.function`, `on.exit`.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete R runtime — match.call, sys.nframe, sys.function, on.exit
// ---------------------------------------------------------------------------

/// Match the source call against closure formals without evaluating its arguments.
pub unsafe fn do_match_call(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // Use the same matcher for this public wrapper and the inspected call.
        let mut roots = Vec::new();
        let mut controls = R_NilValue();
        for name in [c"envir", c"expand.dots", c"call", c"definition"] {
            controls = Rf_cons(R_MissingArg(), controls);
            roots.push(protect(controls));
            SETTAG(controls, Rf_install(name.as_ptr()));
        }
        let supplied = crate::mainutils::duplicate::shallow_duplicate(args);
        let _supplied = protect(supplied);
        let values = crate::mainutils::match_mod::matchArgs_RC(controls, supplied, call);
        let _values = protect(values);
        let definition_arg = CAR(values);
        let call_arg = CAR(CDR(values));
        let expand_arg = CAR(CDR(CDR(values)));
        let envir_arg = CAR(CDR(CDR(CDR(values))));
        let top = crate::sexp::context::R_GlobalContext();
        let definition = if definition_arg == R_MissingArg() || definition_arg == R_NilValue() {
            crate::eval::context::R_sysfunction(0, top)
        } else {
            definition_arg
        };
        let mut source = if call_arg == R_MissingArg() {
            crate::eval::context::R_syscall(0, top)
        } else {
            call_arg
        };
        if TYPEOF(source) == SEXPTYPE::EXPRSXP && XLENGTH(source) > 0 {
            source = VECTOR_ELT(source, 0);
        }
        if TYPEOF(definition) != SEXPTYPE::CLOSXP {
            base_error("invalid 'definition' argument");
        }
        if TYPEOF(source) != SEXPTYPE::LANGSXP {
            base_error("invalid 'call' argument");
        }
        let _definition = protect(definition);
        let _source = protect(source);
        let expand = if expand_arg == R_MissingArg() {
            TRUE
        } else {
            crate::main::coerce::asLogical(expand_arg)
        };
        if expand == NA_LOGICAL {
            base_error("invalid 'expand.dots' argument");
        }
        let mut envir = envir_arg;
        if envir == R_MissingArg() {
            envir = rho;
            let mut context = top;
            while !context.is_null() {
                if (*context).cloenv == rho && !(*context).sysparent.is_null() {
                    envir = (*context).sysparent;
                    break;
                }
                context = (*context).nextcontext;
            }
        }
        if TYPEOF(envir) != SEXPTYPE::ENVSXP {
            base_error("'envir' must be an environment");
        }
        let _envir = protect(envir);
        let dots_symbol = Rf_install(c"...".as_ptr());
        let mut actuals = R_NilValue();
        let mut tail = R_NilValue();
        // Every allocated cell stays rooted until the final call is built.
        let mut append = |head: &mut SEXP, tail: &mut SEXP, value: SEXP, tag: SEXP| {
            let cell = Rf_cons(value, R_NilValue());
            roots.push(protect(cell));
            SETTAG(cell, tag);
            if *head == R_NilValue() {
                *head = cell;
            } else {
                SETCDR(*tail, cell);
            }
            *tail = cell;
        };
        let mut cursor = CDR(source);
        while cursor != R_NilValue() && !cursor.is_null() {
            if CAR(cursor) == dots_symbol {
                let mut dots = crate::sexp::envir::R_findVar(dots_symbol, envir);
                if dots != R_MissingArg() && dots != R_NilValue() {
                    if TYPEOF(dots) != SEXPTYPE::DOTSXP {
                        base_error("'...' used in an incorrect context");
                    }
                    let mut dot_index = 1usize;
                    while dots != R_NilValue() && !dots.is_null() {
                        let mut expr = CAR(dots);
                        while TYPEOF(expr) == SEXPTYPE::PROMSXP {
                            expr = crate::sexp::accessors::PRCODE(expr);
                        }
                        if TYPEOF(expr) == SEXPTYPE::SYMSXP || TYPEOF(expr) == SEXPTYPE::LANGSXP {
                            let name =
                                CString::new(format!("..{dot_index}")).expect("numeric dots name");
                            expr = Rf_install(name.as_ptr());
                        }
                        append(&mut actuals, &mut tail, expr, TAG(dots));
                        dot_index += 1;
                        dots = CDR(dots);
                    }
                }
            } else {
                append(&mut actuals, &mut tail, CAR(cursor), TAG(cursor));
            }
            cursor = CDR(cursor);
        }
        let matched =
            crate::mainutils::match_mod::matchArgs_RC(FORMALS(definition), actuals, source);
        let _matched = protect(matched);
        let mut result_args = R_NilValue();
        let mut result_tail = R_NilValue();
        let mut formal = FORMALS(definition);
        let mut entry = matched;
        while entry != R_NilValue() {
            let value = CAR(entry);
            if value != R_MissingArg() {
                if TAG(formal) == dots_symbol && value != R_NilValue() {
                    if expand != FALSE {
                        let mut dot = value;
                        while dot != R_NilValue() {
                            append(&mut result_args, &mut result_tail, CAR(dot), TAG(dot));
                            dot = CDR(dot);
                        }
                    } else {
                        let list = crate::mainutils::duplicate::shallow_duplicate(value);
                        let _list = protect(list);
                        (*list).sxpinfo.set_type(SEXPTYPE::LISTSXP);
                        append(&mut result_args, &mut result_tail, list, dots_symbol);
                    }
                } else if TAG(formal) != dots_symbol {
                    append(&mut result_args, &mut result_tail, value, TAG(formal));
                }
            }
            entry = CDR(entry);
            formal = CDR(formal);
        }
        let result = Rf_cons(CAR(source), result_args);
        (*result).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        result
    }
}

/// R's `sys.nframe()` — returns the number of frames on the call stack.
pub unsafe fn do_sys_nframe(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let top = crate::sexp::context::R_GlobalContext();
        Rf_ScalarInteger(crate::eval::context::framedepth(top))
    }
}

/// R's `sys.function(which)` — returns the function at the given frame level.
pub unsafe fn do_sys_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let which = context_index_arg(args, 0);
        let top = crate::sexp::context::R_GlobalContext();
        if top.is_null() {
            R_NilValue()
        } else {
            crate::eval::context::R_sysfunction(which, top)
        }
    }
}

/// R's `on.exit(expr, add, after)` — register an exit handler for the
/// current function context.
pub unsafe fn do_on_exit(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::eval::special::do_on_exit_from_args(args, rho) }
}
