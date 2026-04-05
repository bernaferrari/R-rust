#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/list.c -- basic list handling features.
//!
//! Implements `all.names()` / `all.vars()` via `do_allnames()`.

use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::{
    CAR, CDR, CHAR, PRINTNAME, SET_STRING_ELT, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// SEXPTYPE values used in match patterns
// ---------------------------------------------------------------------------

// SEXPTYPE constants now imported from crate::sexp::ffi::SEXPTYPE

// ---------------------------------------------------------------------------
// Stub functions for features not yet implemented
// ---------------------------------------------------------------------------

unsafe fn checkArity(op: SEXP, args: SEXP) {
    crate::main::errors::Rf_checkArityCall(op, args, crate::main::errors::getCurrentCall());
}

unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_LOGICAL;
        }
        match TYPEOF(x) {
            10 => {
                // LGLSXP
                let p = crate::sexp::accessors::LOGICAL(x);
                if p.is_null() { NA_LOGICAL } else { *p }
            }
            13 => {
                // INTSXP
                let p = crate::sexp::accessors::INTEGER(x);
                if p.is_null() { NA_LOGICAL } else { *p }
            }
            _ => 0,
        }
    }
}

unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        match TYPEOF(x) {
            13 => {
                // INTSXP
                let p = crate::sexp::accessors::INTEGER(x);
                if p.is_null() { NA_INTEGER } else { *p }
            }
            14 => {
                // REALSXP
                let p = crate::sexp::accessors::REAL(x);
                if p.is_null() {
                    NA_INTEGER
                } else {
                    let v = *p;
                    if v.is_nan() || v > i32::MAX as f64 || v < i32::MIN as f64 {
                        NA_INTEGER
                    } else {
                        v as c_int
                    }
                }
            }
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// NameWalkData
// ---------------------------------------------------------------------------

/// Data structure for the recursive name-walking traversal.
struct NameWalkData {
    ans: SEXP,
    unique_names: c_int,
    include_functions: c_int,
    store_values: c_int,
    item_counts: c_int,
    max_count: c_int,
}

// ---------------------------------------------------------------------------
// namewalk: recursive symbol traversal
// ---------------------------------------------------------------------------

/// Recursively traverse an expression and collect symbol names.
unsafe fn namewalk(s: SEXP, d: &mut NameWalkData) {
    unsafe {
        let name: SEXP;

        match TYPEOF(s) {
            t if t == SEXPTYPE::SYMSXP.0 => {
                name = PRINTNAME(s);
                // skip blank symbols
                if !CHAR(name).is_null() && *CHAR(name) == 0 {
                    return;
                }
                if d.item_counts < d.max_count {
                    if d.store_values != 0 {
                        if d.unique_names != 0 {
                            for j in 0..d.item_counts {
                                if STRING_ELT(d.ans, j as R_xlen_t) == name {
                                    return;
                                }
                            }
                        }
                        SET_STRING_ELT(d.ans, d.item_counts as R_xlen_t, name);
                    }
                    d.item_counts += 1;
                }
            }
            t if t == SEXPTYPE::LANGSXP.0 => {
                let mut cur = if d.include_functions != 0 { s } else { CDR(s) };
                while cur != R_NilValue() {
                    namewalk(CAR(cur), d);
                    cur = CDR(cur);
                }
            }
            t if t == SEXPTYPE::EXPRSXP.0 => {
                for i in 0..XLENGTH(s) {
                    namewalk(VECTOR_ELT(s, i), d);
                }
            }
            _ => {
                // do nothing
            }
        }
    }
}

// ---------------------------------------------------------------------------
// do_allnames: .Internal(all.names(expr, functions, max.names, unique))
// ---------------------------------------------------------------------------

/// Also does all.vars with functions=FALSE.
pub unsafe fn do_allnames(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, env);
        checkArity(op, args);

        let expr = CAR(args);
        let mut args_rest = CDR(args);

        let mut data = NameWalkData {
            ans: ptr::null_mut(),
            unique_names: 0,
            include_functions: 0,
            store_values: 0,
            item_counts: 0,
            max_count: 0,
        };

        data.include_functions = asLogical(CAR(args_rest));
        if data.include_functions == NA_LOGICAL {
            data.include_functions = 0;
        }
        args_rest = CDR(args_rest);

        data.max_count = asInteger(CAR(args_rest));
        if data.max_count == -1 {
            data.max_count = i32::MAX;
        }
        if data.max_count < 0 || data.max_count == NA_INTEGER {
            data.max_count = 0;
        }
        args_rest = CDR(args_rest);

        data.unique_names = asLogical(CAR(args_rest));
        if data.unique_names == NA_LOGICAL {
            data.unique_names = 1;
        }

        // First pass: count items
        namewalk(expr, &mut data);
        let savecount = data.item_counts;

        // Allocate result vector
        data.ans = Rf_allocVector(SEXPTYPE::STRSXP.0, data.item_counts as c_int);

        // Second pass: store values
        data.store_values = 1;
        data.item_counts = 0;
        namewalk(expr, &mut data);

        // If unique names filtered some items, reallocate
        if data.item_counts != savecount {
            let old_ans = data.ans;
            data.ans = Rf_allocVector(SEXPTYPE::STRSXP.0, data.item_counts as c_int);
            for i in 0..data.item_counts {
                SET_STRING_ELT(data.ans, i as R_xlen_t, STRING_ELT(old_ans, i as R_xlen_t));
            }
        }

        data.ans
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::ffi::*;

    /// Helper: create a symbol node.
    unsafe fn make_sym(name: &str) -> SEXP {
        use crate::main::dstruct::mkSYMSXP;
        use crate::sexp::constructors::Rf_mkChar;
        let charsxp = Rf_mkChar(std::ffi::CString::new(name).unwrap().as_ptr());
        mkSYMSXP(charsxp, R_NilValue())
    }

    /// Helper: create a cons cell (LANGSXP-like pair).
    unsafe fn make_cons(car: SEXP, cdr: SEXP) -> SEXP {
        use crate::sexp::constructors::Rf_cons;
        Rf_cons(car, cdr)
    }

    #[test]
    fn test_namewalk_simple_symbol() {
        unsafe {
            let sym = make_sym("x");
            let mut data = NameWalkData {
                ans: Rf_allocVector(SEXPTYPE::STRSXP.0, 10),
                unique_names: 1,
                include_functions: 1,
                store_values: 1,
                item_counts: 0,
                max_count: 100,
            };
            namewalk(sym, &mut data);
            assert_eq!(data.item_counts, 1);
        }
    }

    #[test]
    fn test_namewalk_empty_symbol_skipped() {
        unsafe {
            let sym = make_sym("");
            let mut data = NameWalkData {
                ans: Rf_allocVector(SEXPTYPE::STRSXP.0, 10),
                unique_names: 1,
                include_functions: 1,
                store_values: 1,
                item_counts: 0,
                max_count: 100,
            };
            namewalk(sym, &mut data);
            assert_eq!(data.item_counts, 0);
        }
    }

    #[test]
    fn test_namewalk_max_count() {
        unsafe {
            let sym = make_sym("a");
            let mut data = NameWalkData {
                ans: Rf_allocVector(SEXPTYPE::STRSXP.0, 10),
                unique_names: 0,
                include_functions: 1,
                store_values: 0,
                item_counts: 0,
                max_count: 0,
            };
            namewalk(sym, &mut data);
            assert_eq!(data.item_counts, 0); // max_count=0 means nothing is counted
        }
    }
}
