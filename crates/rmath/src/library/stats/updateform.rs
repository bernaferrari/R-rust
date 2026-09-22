//! GNU `stats/src/model.c` `updateform`: replace `.` in a formula update.

use crate::sexp::accessors::{
    CADR, CAR, CADDR, CDR, SETCAR, SETCDR, SET_ATTRIB, TYPEOF,
};
use crate::sexp::constructors::{Rf_cons, Rf_lang2};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

fn sym(name: &str) -> SEXP {
    unsafe {
        let c = std::ffi::CString::new(name).unwrap_or_default();
        Rf_install(c.as_ptr())
    }
}
unsafe fn setcadr(x: SEXP, y: SEXP) {
    unsafe { SETCAR(CDR(x), y) }
}

unsafe fn setcaddr(x: SEXP, y: SEXP) {
    unsafe { SETCAR(CDR(CDR(x)), y) }
}


fn lang_len(object: SEXP) -> i32 {
    unsafe { crate::sexp::constructors::Rf_length(object) }
}

fn is_sum(op: SEXP) -> bool {
    op == sym("+") || op == sym("-")
}

unsafe fn maybe_paren(side: SEXP, value: SEXP, wrap: bool) -> SEXP {
    unsafe {
        let expanded = expand_dots(side, value);
        if side == sym(".") && wrap {
            Rf_lang2(sym("("), expanded)
        } else {
            expanded
        }
    }
}

unsafe fn expand_dots(object: SEXP, value: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(object) == SEXPTYPE::SYMSXP {
            if object == sym(".") {
                return crate::mainutils::duplicate::duplicate(value);
            }
            return object;
        }
        if TYPEOF(object) != SEXPTYPE::LANGSXP {
            return object;
        }
        let op = if TYPEOF(value) == SEXPTYPE::LANGSXP {
            CAR(value)
        } else {
            R_NilValue()
        };
        let _guard = protect(object);
        let head = CAR(object);
        let n = lang_len(object);
        let bad = || {
            crate::main::errors::Rf_error(
                b"invalid formula in 'update'\0".as_ptr() as *const std::os::raw::c_char,
            );
        };
        if head == sym("+") {
            if n == 2 {
                setcadr(object, expand_dots(CADR(object), value));
            } else if n == 3 {
                setcadr(object, expand_dots(CADR(object), value));
                setcaddr(object, expand_dots(CADDR(object), value));
            } else {
                bad();
            }
            return object;
        }
        if head == sym("-") {
            if n == 2 {
                setcadr(object, maybe_paren(CADR(object), value, is_sum(op)));
            } else if n == 3 {
                setcadr(object, maybe_paren(CADR(object), value, is_sum(op)));
                setcaddr(object, maybe_paren(CADDR(object), value, is_sum(op)));
            } else {
                bad();
            }
            return object;
        }
        if head == sym("*") || head == sym("/") {
            if n != 3 {
                bad();
            }
            setcadr(object, maybe_paren(CADR(object), value, is_sum(op)));
            setcaddr(object, maybe_paren(CADDR(object), value, is_sum(op)));
            return object;
        }
        if head == sym(":") {
            if n != 3 {
                bad();
            }
            setcadr(object,
            maybe_paren(CADR(object), value, is_sum(op) || op == sym("*") || op == sym("/")),);
            setcaddr(object, maybe_paren(CADDR(object), value, is_sum(op)));
            return object;
        }
        if head == sym("^") {
            if n != 3 {
                bad();
            }
            setcadr(object,
            maybe_paren(
                CADR(object),
                value,
                is_sum(op) || op == sym("*") || op == sym("/") || op == sym(":"),
            ),);
            setcaddr(object, maybe_paren(CADDR(object), value, is_sum(op)));
            return object;
        }
        let mut cell = object;
        while cell != R_NilValue() && !cell.is_null() {
            SETCAR(cell, expand_dots(CAR(cell), value));
            cell = CDR(cell);
        }
        object
    }
}

pub unsafe fn updateform(old: SEXP, new: SEXP) -> SEXP {
    unsafe {
        let new = crate::mainutils::duplicate::duplicate(new);
        let _new = protect(new);
        let tilde = sym("~");
        if TYPEOF(old) != SEXPTYPE::LANGSXP
            || (TYPEOF(new) != SEXPTYPE::LANGSXP && CAR(old) != tilde)
            || CAR(new) != tilde
        {
            crate::main::errors::Rf_error(
                b"formula expected\0".as_ptr() as *const std::os::raw::c_char,
            );
        }
        if lang_len(old) == 3 {
            let lhs = CADR(old);
            let rhs = CADDR(old);
            if lang_len(new) == 2 {
                SETCDR(new, Rf_cons(lhs, CDR(new)));
            }
            let _rhs = protect(rhs);
            setcadr(new, expand_dots(CADR(new), lhs));
            setcaddr(new, expand_dots(CADDR(new), rhs));
        } else {
            let rhs = CADR(old);
            if lang_len(new) == 3 {
                setcaddr(new, expand_dots(CADDR(new), rhs));
            } else {
                setcadr(new, expand_dots(CADR(new), rhs));
            }
        }
        SET_ATTRIB(new, R_NilValue());
        (*new).sxpinfo.set_obj(false);
        let env_sym = sym(".Environment");
        crate::sexp::attrib_core::setAttrib(
            new,
            env_sym,
            crate::sexp::attrib_core::getAttrib(old, env_sym),
        );
        new
    }
}

pub unsafe extern "C-unwind" fn c_updateform(old: SEXP, new: SEXP) -> SEXP {
    unsafe { updateform(old, new) }
}
