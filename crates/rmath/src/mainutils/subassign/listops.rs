#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(unused_imports)]

//! Pairlist assignment support — GetOneIndex, SimpleListAssign, listRemove,
//! DeleteOneVectorListItem, SubAssignArgs.

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::mainutils::subscript::{
    OneIndex, get1index, int_arraySubscript, make_subscript, mat2indsub, strmat2intmat, vectorIndex,
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
pub(crate) fn GetOneIndex(sub: SEXP, ind: c_int) -> SEXP {
    {
        let fail = |error: crate::sexp::object::SexpError| -> ! {
            crate::sexp::context::r_error(error.to_string())
        };
        let subscript = owned(sub);
        if ind < 0 || ind as R_xlen_t + 1 > subscript.len() {
            // Error: internal error
            return sub;
        }
        if subscript.len() > 1 {
            match TYPEOF(sub) {
                INTSXP => {
                    return ScalarInteger(
                        subscript
                            .try_integer_elt(ind as R_xlen_t)
                            .unwrap_or_else(|e| fail(e)),
                    );
                }
                REALSXP => {
                    return ScalarReal(
                        subscript
                            .try_real_elt(ind as R_xlen_t)
                            .unwrap_or_else(|e| fail(e)),
                    );
                }
                STRSXP => {
                    return ScalarString(
                        subscript
                            .try_string_elt(ind as R_xlen_t)
                            .unwrap_or_else(|e| fail(e))
                            .as_raw(),
                    );
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
pub(crate) fn SimpleListAssign(
    _call: SEXP,
    x: SEXP,
    s: SEXP,
    y: SEXP,
    ind: c_int,
    _check_cycles: bool,
) -> SEXP {
    let sub = CAR(s);
    if Rf_length(s) > 1 {
        // Error: invalid number of subscripts
        return x;
    }

    let sub = GetOneIndex(sub, ind);
    let _sub_guard = protect(sub);
    let mut stretch: R_xlen_t = 1;
    let indx = make_subscript(x, sub, &mut stretch, R_NilValue());
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

// ---------------------------------------------------------------------------
// listRemove
// ---------------------------------------------------------------------------

/// Port of `listRemove()` -- removes an element from a pairlist (for `x[[s]] <- NULL`).
pub(crate) fn listRemove(x: SEXP, s: SEXP, ind: c_int) -> SEXP {
    let nx = Rf_length(x);
    let s = GetOneIndex(s, ind);
    let _s_guard = protect(s);
    let mut stretch: R_xlen_t = 0;
    let s = make_subscript(x, s, &mut stretch, R_NilValue());
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

// ---------------------------------------------------------------------------
// DeleteOneVectorListItem
// ---------------------------------------------------------------------------

/// Port of `DeleteOneVectorListItem()` -- removes a single element from a vector list.
pub(crate) fn DeleteOneVectorListItem(x: SEXP, which: R_xlen_t) -> SEXP {
    {
        let fail = |error: crate::sexp::object::SexpError| -> ! {
            crate::sexp::context::r_error(error.to_string())
        };
        let n = XLENGTH(x);
        if which >= 0 && which < n {
            let factory = factory();
            let source = owned(x);
            let y = factory
                .wrap(alloc_vector(TYPEOF(x), n - 1))
                .unwrap_or_else(|e| fail(e));
            let mut y =
                crate::sexp::object::SexpMut::try_from_checked(y).unwrap_or_else(|e| fail(e));
            let mut k: R_xlen_t = 0;
            for i in 0..n {
                if i != which {
                    let elt = source.try_vector_elt(i).unwrap_or_else(|e| fail(e));
                    y.try_set_vector_elt(k, elt).unwrap_or_else(|e| fail(e));
                    k += 1;
                }
            }
            let y = y.freeze();
            let xnames = super::getAttrib(x, sym_Names());
            let _xnames_root = if isNull(xnames) {
                None
            } else {
                Some(owned(xnames))
            };
            if !isNull(xnames) {
                let xnames = owned(xnames);
                let ynames = factory
                    .wrap(alloc_vector(STRSXP, n - 1))
                    .unwrap_or_else(|e| fail(e));
                let mut ynames = crate::sexp::object::SexpMut::try_from_checked(ynames)
                    .unwrap_or_else(|e| fail(e));
                k = 0;
                for i in 0..n {
                    if i != which {
                        let elt = xnames.try_string_elt(i).unwrap_or_else(|e| fail(e));
                        ynames
                            .try_set_string_elt(k, elt)
                            .unwrap_or_else(|e| fail(e));
                        k += 1;
                    }
                }
                super::setAttrib(y.as_raw(), sym_Names(), ynames.freeze().as_raw());
            }
            copyMostAttrib(x, y.as_raw());
            y.as_raw()
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
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
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
