//! complete.cases() implementation
//! Port of r-source/src/library/stats/src/complete_cases.c

use std::os::raw::c_int;

use crate::attrib_core::{R_RowNamesSymbol, getAttrib};
use crate::main::errors::{Rf_error, Rf_error_cstr};
use crate::main::relop::NA_STRING;
use crate::main::relop::R_DimSymbol;
use crate::main::relop::R_NamesSymbol;
use crate::sexp::accessors::{CAR, CDR, LENGTH, STRING_ELT, TYPEOF, VECTOR_ELT};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::constructors::Rf_isList;
use crate::sexp::ffi::NA_INTEGER;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::object::SexpMut;
use crate::sexp::protect::protect as protect_sexp;

fn checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()))
}

fn isMatrix(x: SEXP) -> bool {
    let u = getAttrib(x, R_DimSymbol());
    !u.is_null() && LENGTH(u) >= 2
}

fn isVector(x: SEXP) -> bool {
    let t = TYPEOF(x);
    t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::RAWSXP
        || t == SEXPTYPE::VECSXP
        || t == SEXPTYPE::EXPRSXP
}

fn isNewList(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::VECSXP
}

fn length_sexp(x: SEXP) -> c_int {
    let t = TYPEOF(x);
    if t == SEXPTYPE::NILSXP {
        return 0;
    }
    if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP {
        let mut count = 0i32;
        let mut cur = x;
        while !cur.is_null() {
            count += 1;
            cur = CDR(cur);
        }
        return count;
    }
    LENGTH(x)
}

/// First integer payload element of a checked dimension attribute.
fn dim_nrow(owner: crate::sexp::owner::OwnerToken<'_>, dim: SEXP) -> c_int {
    checked(owner.sexp(dim))
        .integer_elt(0)
        .unwrap_or(NA_INTEGER)
}

fn check_vector_na(
    owner: crate::sexp::owner::OwnerToken<'_>,
    u: SEXP,
    rval: &mut [c_int],
    len: c_int,
) {
    let len = len as usize;
    let n = LENGTH(u) as usize;
    let t = TYPEOF(u);
    let u_value = checked(owner.sexp(u));
    match t {
        x if x == SEXPTYPE::INTSXP => {
            for i in 0..n {
                if u_value.integer_elt(i as R_xlen_t) == Some(NA_INTEGER) {
                    rval[i % len] = 0;
                }
            }
        }
        x if x == SEXPTYPE::LGLSXP => {
            for i in 0..n {
                if u_value.logical_elt(i as R_xlen_t) == Some(NA_INTEGER) {
                    rval[i % len] = 0;
                }
            }
        }
        x if x == SEXPTYPE::REALSXP => {
            for i in 0..n {
                if u_value
                    .real_elt(i as R_xlen_t)
                    .is_some_and(|value| value.is_nan())
                {
                    rval[i % len] = 0;
                }
            }
        }
        x if x == SEXPTYPE::CPLXSXP => {
            for i in 0..n {
                if let Some(value) = u_value.complex_elt(i as R_xlen_t)
                    && (value.r.is_nan() || value.i.is_nan())
                {
                    rval[i % len] = 0;
                }
            }
        }
        x if x == SEXPTYPE::STRSXP => {
            for i in 0..n {
                if STRING_ELT(u, i as i64) == NA_STRING() {
                    rval[i % len] = 0;
                }
            }
        }
        _ => {
            Rf_error_cstr(c"invalid 'type' of argument");
        }
    }
}

pub fn compcases(args: SEXP) -> SEXP {
    checked(crate::sexp::owner::OwnerToken::with_current(|owner| {
        let mut len: c_int = -1;
        let nil = R_NilValue();

        let mut args_iter = CDR(args);

        // First pass: determine length
        let mut s = args_iter;
        while !s.is_null() && s != nil {
            let car = CAR(s);
            // GNU isList(NULL) is true and the inner walk is empty, so NULL is a no-op.
            if car.is_null() || car == nil || TYPEOF(car) == SEXPTYPE::NILSXP {
                s = CDR(s);
                continue;
            }
            if Rf_isList(car) != 0 {
                let mut t = car;
                while !t.is_null() && t != nil {
                    let u = CAR(t);
                    if isMatrix(u) {
                        let dim = getAttrib(u, R_DimSymbol());
                        if len < 0 {
                            len = dim_nrow(owner, dim);
                        } else if len != dim_nrow(owner, dim) {
                            Rf_error_cstr(c"not all arguments have the same length");

                            return nil;
                        }
                    } else if isVector(u) {
                        if len < 0 {
                            len = LENGTH(u);
                        } else if len != LENGTH(u) {
                            Rf_error_cstr(c"not all arguments have the same length");

                            return nil;
                        }
                    } else {
                        Rf_error_cstr(c"invalid 'type' of argument");

                        return nil;
                    }
                    t = CDR(t);
                }
            } else if isNewList(car) {
                let mut t = car;
                let nt = length_sexp(t);
                if nt > 0 {
                    let mut it: c_int = 0;
                    while it < nt {
                        let u = VECTOR_ELT(t, it as i64);
                        if isMatrix(u) {
                            let dim = getAttrib(u, R_DimSymbol());
                            if len < 0 {
                                len = dim_nrow(owner, dim);
                            } else if len != dim_nrow(owner, dim) {
                                Rf_error_cstr(c"not all arguments have the same length");

                                return nil;
                            }
                        } else if isVector(u) {
                            if len < 0 {
                                len = LENGTH(u);
                            } else if len != LENGTH(u) {
                                Rf_error_cstr(c"not all arguments have the same length");

                                return nil;
                            }
                        } else {
                            Rf_error_cstr(c"invalid 'type' of argument");

                            return nil;
                        }
                        it += 1;
                    }
                } else {
                    let u = getAttrib(t, R_RowNamesSymbol());
                    if !u.is_null() && u != nil {
                        if len < 0 {
                            len = LENGTH(u);
                        } else if len != checked(owner.sexp(u)).integer_elt(0).unwrap_or(NA_INTEGER)
                        {
                            Rf_error_cstr(c"not all arguments have the same length");

                            return nil;
                        }
                    }
                }
            } else if isMatrix(car) {
                let dim = getAttrib(car, R_DimSymbol());
                if len < 0 {
                    len = dim_nrow(owner, dim);
                } else if len != dim_nrow(owner, dim) {
                    Rf_error_cstr(c"not all arguments have the same length");

                    return nil;
                }
            } else if isVector(car) {
                if len < 0 {
                    len = LENGTH(car);
                } else if len != LENGTH(car) {
                    Rf_error_cstr(c"not all arguments have the same length");

                    return nil;
                }
            } else {
                Rf_error_cstr(c"invalid 'type' of argument");

                return nil;
            }
            s = CDR(s);
        }

        if len < 0 {
            Rf_error_cstr(c"no input has determined the number of cases");

            return nil;
        }

        let rval = Rf_allocVector(SEXPTYPE::LGLSXP, len);
        let _rval_guard = protect_sexp(rval);
        let mut rval_buf = vec![1; len as usize];

        // Second pass: check for NAs
        s = args_iter;
        while !s.is_null() && s != nil {
            let car = CAR(s);
            if car.is_null() || car == nil || TYPEOF(car) == SEXPTYPE::NILSXP {
                s = CDR(s);
                continue;
            }
            if Rf_isList(car) != 0 {
                let mut t = car;
                while !t.is_null() && t != nil {
                    let u = CAR(t);
                    check_vector_na(owner, u, &mut rval_buf, len);
                    t = CDR(t);
                }
            } else if isNewList(car) {
                let t = car;
                let nt = length_sexp(t);
                let mut it: c_int = 0;
                while it < nt {
                    let u = VECTOR_ELT(t, it as i64);
                    check_vector_na(owner, u, &mut rval_buf, len);
                    it += 1;
                }
            } else {
                check_vector_na(owner, car, &mut rval_buf, len);
            }
            s = CDR(s);
        }

        let mut rval_out = checked(SexpMut::try_from_checked(checked(owner.sexp(rval))));
        for (index, value) in rval_buf.iter().enumerate() {
            checked(rval_out.try_set_logical_elt(index as R_xlen_t, *value));
        }
        rval
    }))
}
