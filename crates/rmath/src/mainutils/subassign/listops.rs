#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(unused_imports)]

//! Pairlist assignment support — GetOneIndex, SimpleListAssign, listRemove,
//! DeleteOneVectorListItem, SubAssignArgs.

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::mainutils::subscript::{
    OneIndex, get1index, int_arraySubscript, makeSubscript, mat2indsub, strmat2intmat, vectorIndex,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::{allocList, allocSExp};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::support::{SET_S4_OBJECT, SET_TRUELENGTH, UNSET_S4_OBJECT};
use super::*;

// ---------------------------------------------------------------------------
// GetOneIndex
// ---------------------------------------------------------------------------

/// Port of `GetOneIndex()` -- extracts a single subscript index for pairlist assignment.
pub(crate) unsafe fn GetOneIndex(sub: SEXP, ind: c_int) -> SEXP {
    unsafe {
        if ind < 0 || ind + 1 > Rf_length(sub) {
            // Error: internal error
            return sub;
        }
        if Rf_length(sub) > 1 {
            match TYPEOF(sub) {
                INTSXP => {
                    return ScalarInteger(INTEGER_ELT(sub, ind));
                }
                REALSXP => {
                    return ScalarReal(REAL_ELT(sub, ind));
                }
                STRSXP => {
                    return ScalarString(STRING_ELT(sub, ind as R_xlen_t));
                }
                _ => {
                    // Error: invalid subscript
                    return sub;
                }
            }
        }
        sub
    }
}

// ---------------------------------------------------------------------------
// SimpleListAssign
// ---------------------------------------------------------------------------

/// Port of `SimpleListAssign()` -- handles `x[[s]] <- y` for pairlists.
pub(crate) unsafe fn SimpleListAssign(
    _call: SEXP,
    x: SEXP,
    s: SEXP,
    y: SEXP,
    ind: c_int,
    _check_cycles: bool,
) -> SEXP {
    unsafe {
        let sub = CAR(s);
        if Rf_length(s) > 1 {
            // Error: invalid number of subscripts
            return x;
        }

        let sub = GetOneIndex(sub, ind);
        let _sub_guard = protect(sub);
        let mut stretch: R_xlen_t = 1;
        let indx = makeSubscript(x, sub, &mut stretch, R_NilValue());
        let _indx_guard = protect(indx);

        let n = Rf_length(indx);
        if n > 1 {
            // Error: invalid subscript
            return x;
        }

        let mut nx = Rf_length(x);
        let mut x = x;

        if stretch > 0 {
            let t = CAR(s);
            let yi = allocList((stretch - nx as R_xlen_t) as c_int);
            let _yi_guard = protect(yi);
            if isString(t) && Rf_length(t) == (stretch - nx as R_xlen_t) as c_int {
                let mut z = yi;
                for i in 0..Rf_length(t) {
                    SETTAG(z, installTrChar(STRING_ELT(t, i as R_xlen_t)));
                    z = CDR(z);
                }
            }
            x = listAppend(x, yi);
            nx = stretch as c_int;
        }
        let _x_guard = protect(x);

        if n == 1 {
            let ii = asInteger(indx);
            if ii != NA_INTEGER {
                let ii = ii - 1;
                let xi = nthcdr(x, ii % nx);
                SETCAR(xi, y);
            }
        }
        x
    }
}

// ---------------------------------------------------------------------------
// listRemove
// ---------------------------------------------------------------------------

/// Port of `listRemove()` -- removes an element from a pairlist (for `x[[s]] <- NULL`).
pub(crate) unsafe fn listRemove(x: SEXP, s: SEXP, ind: c_int) -> SEXP {
    unsafe {
        let nx = Rf_length(x);
        let s = GetOneIndex(s, ind);
        let _s_guard = protect(s);
        let mut stretch: R_xlen_t = 0;
        let s = makeSubscript(x, s, &mut stretch, R_NilValue());
        let _subscript_guard = protect(s);
        let ns = Rf_length(s);

        let mut indx = vec![1i32; nx as usize];
        if TYPEOF(s) == REALSXP {
            for i in 0..ns {
                let di = REAL_ELT(s, i);
                if R_FINITE(di) {
                    indx[(di as R_xlen_t - 1) as usize] = 0;
                }
            }
        } else {
            for i in 0..ns {
                let ii = INTEGER_ELT(s, i);
                if ii != NA_INTEGER {
                    indx[(ii - 1) as usize] = 0;
                }
            }
        }

        let mut px = x;
        let mut pv: SEXP = ptr::null_mut();
        let mut val: SEXP = ptr::null_mut();
        for i in 0..nx {
            if indx[i as usize] != 0 {
                if isNull(val) {
                    val = px;
                }
                pv = px;
            } else {
                if !isNull(pv) {
                    SETCDR(pv, CDR(px));
                }
            }
            px = CDR(px);
        }

        if !isNull(val) {
            SET_ATTRIB(val, ATTRIB(x));
            if IS_S4_OBJECT(x) != 0 {
                SET_S4_OBJECT(val);
            } else {
                UNSET_S4_OBJECT(val);
            }
            SET_OBJECT(val, OBJECT(x));
            RAISE_NAMED(val, NAMED(x));
        }

        val
    }
}

// ---------------------------------------------------------------------------
// DeleteOneVectorListItem
// ---------------------------------------------------------------------------

/// Port of `DeleteOneVectorListItem()` -- removes a single element from a vector list.
pub(crate) unsafe fn DeleteOneVectorListItem(x: SEXP, which: R_xlen_t) -> SEXP {
    unsafe {
        let n = XLENGTH(x);
        if which >= 0 && which < n {
            let y = Rf_allocVector3(TYPEOF(x), n - 1);
            let _y_guard = protect(y);
            let mut k: R_xlen_t = 0;
            for i in 0..n {
                if i != which {
                    SET_VECTOR_ELT(y, k, VECTOR_ELT(x, i));
                    k += 1;
                }
            }
            let xnames = getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
            let _xnames_guard = protect(xnames);
            if !isNull(xnames) {
                let ynames = Rf_allocVector3(STRSXP, n - 1);
                let _ynames_guard = protect(ynames);
                k = 0;
                for i in 0..n {
                    if i != which {
                        SET_STRING_ELT(ynames, k, STRING_ELT(xnames, i));
                        k += 1;
                    }
                }
                setAttrib(y, crate::eval::attrib_core::R_NamesSymbol(), ynames);
            }
            copyMostAttrib(x, y);
            y
        } else {
            x
        }
    }
}

// ---------------------------------------------------------------------------
// SubAssignArgs
// ---------------------------------------------------------------------------

/// Port of `SubAssignArgs()` -- extracts (x, s, y) from the argument list
/// and returns the number of subscripts.
pub(crate) unsafe fn SubAssignArgs(args: SEXP, x: *mut SEXP, s: *mut SEXP, y: *mut SEXP) -> c_int {
    unsafe {
        if isNull(CDR(args)) {
            // Error: invalid number of arguments
            *x = CAR(args);
            *s = R_NilValue();
            *y = R_NilValue();
            return 0;
        }
        *x = CAR(args);
        if isNull(CDDR(args)) {
            *s = R_NilValue();
            *y = CADR(args);
            return 0;
        } else {
            let mut nsubs = 1;
            let mut p = CDR(args);
            *s = p;
            while !isNull(CDDR(p)) {
                p = CDR(p);
                nsubs += 1;
            }
            *y = CADR(p);
            SETCDR(p, R_NilValue());
            nsubs
        }
    }
}
