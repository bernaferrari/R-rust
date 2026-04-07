#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/split.c -- `do_split`, the `.Internal`
//! implementation behind R's `split()` function.
//!
//! Splits a vector `x` into groups defined by a factor `f`.
//! Faithfully ports the C logic including:
//! - Two-pass counting (count per level, then fill)
//! - MOD_ITERATE1 recycling of the factor over x
//! - Propagation of names on x to sub-vectors
//! - Long vector support (uses R_xlen_t counts for long vectors,
//!   c_int counts for normal vectors, matching the C code)

use std::os::raw::c_int;

use crate::eval::attrib_core::{
    R_ClassSymbol, R_LevelsSymbol, R_NamesSymbol, getAttrib, setAttrib,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// Panic with an R error message.
unsafe fn error(msg: &str) {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

/// Emit an R warning (currently just eprintln).
unsafe fn warning(msg: &str) {
    eprintln!("Warning: {}", msg);
}

/// Check if SEXP is a factor (has class "factor" or "ordered").
unsafe fn isFactor(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let klass = getAttrib(s, R_ClassSymbol());
        if klass.is_null() || TYPEOF(klass) != SEXPTYPE::STRSXP.0 || LENGTH(klass) < 2 {
            return 0;
        }
        let c1 = CHAR(STRING_ELT(klass, 0));
        let c1_str = std::ffi::CStr::from_ptr(c1).to_str().unwrap_or("");
        (c1_str == "factor" || c1_str == "ordered") as c_int
    }
}

/// Return the number of levels in a factor.
unsafe fn nlevels(f: SEXP) -> c_int {
    unsafe {
        if isFactor(f) == 0 {
            return 0;
        }
        LENGTH(getAttrib(f, R_LevelsSymbol()))
    }
}

/// Check if SEXP is a vector type (atomic or list).
unsafe fn isVector(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP.0
            || t == SEXPTYPE::INTSXP.0
            || t == SEXPTYPE::REALSXP.0
            || t == SEXPTYPE::CPLXSXP.0
            || t == SEXPTYPE::STRSXP.0
            || t == SEXPTYPE::VECSXP.0
            || t == SEXPTYPE::RAWSXP.0
        {
            1
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// do_split -- the .Internal entry point
// ---------------------------------------------------------------------------

pub unsafe fn do_split(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let f = CADR(args);

        if isVector(x) == 0 {
            error("first argument must be a vector");
        }
        if isFactor(f) == 0 {
            error("second argument must be a factor");
        }

        let nlevs = nlevels(f);
        let nfac = XLENGTH(CADR(args));
        let nobs = XLENGTH(CAR(args));

        if nfac <= 0 && nobs > 0 {
            error("group length is 0 but data length > 0");
        }
        if nfac > 0 && (nobs % nfac) != 0 {
            warning("data length is not a multiple of split variable");
        }

        let nm = getAttrib(x, R_NamesSymbol());
        let have_names = !nm.is_null() && nm != R_NilValue();

        let f_data = INTEGER(f);
        let xtype = TYPEOF(x);

        // For long vectors, use REALSXP for counts (R_xlen_t stored as double);
        // for normal vectors, use INTSXP (c_int counts). This matches the C code
        // which includes split-incl.c twice with different type parameters.
        let is_long = nobs > c_int::MAX as R_xlen_t;

        // Allocate counts vector
        let counts = Rf_allocVector3(
            if is_long {
                SEXPTYPE::REALSXP.0
            } else {
                SEXPTYPE::INTSXP.0
            },
            nlevs as R_xlen_t,
        );
        Rf_protect(counts);

        if is_long {
            // Long vector path: counts stored as R_xlen_t in REALSXP
            let counts_data = REAL(counts);
            for i in 0..nlevs as usize {
                *counts_data.add(i) = 0.0;
            }

            // First pass: count elements per level
            let mut i1: R_xlen_t = 0;
            for i in 0..nobs {
                let j = *f_data.add(i1 as usize);
                if j != NA_INTEGER {
                    if j > nlevs || j < 1 {
                        error("factor has bad level");
                    }
                    *counts_data.add((j - 1) as usize) += 1.0;
                }
                i1 += 1;
                if i1 == nfac {
                    i1 = 0;
                }
            }

            // Allocate result list
            let vec = Rf_allocVector3(SEXPTYPE::VECSXP.0, nlevs as R_xlen_t);
            Rf_protect(vec);

            for i in 0..nlevs as R_xlen_t {
                let count = *counts_data.add(i as usize) as R_xlen_t;
                let sub = Rf_allocVector3(xtype, count);
                SET_VECTOR_ELT(vec, i, sub);
                setAttrib(sub, R_LevelsSymbol(), getAttrib(x, R_LevelsSymbol()));
                if have_names {
                    setAttrib(
                        sub,
                        R_NamesSymbol(),
                        Rf_allocVector3(SEXPTYPE::STRSXP.0, count),
                    );
                }
            }

            // Reset counts for second pass
            for i in 0..nlevs as usize {
                *counts_data.add(i) = 0.0;
            }

            // Second pass: fill sub-vectors
            let mut i1: R_xlen_t = 0;
            for i in 0..nobs {
                let j = *f_data.add(i1 as usize);
                if j != NA_INTEGER {
                    let k = *counts_data.add((j - 1) as usize) as R_xlen_t;
                    let sub = VECTOR_ELT(vec, (j - 1) as R_xlen_t);

                    match xtype {
                        t if t == SEXPTYPE::LGLSXP.0 || t == SEXPTYPE::INTSXP.0 => {
                            *INTEGER(sub).add(k as usize) = *INTEGER(x).add(i as usize);
                        }
                        t if t == SEXPTYPE::REALSXP.0 => {
                            *REAL(sub).add(k as usize) = *REAL(x).add(i as usize);
                        }
                        t if t == SEXPTYPE::CPLXSXP.0 => {
                            *COMPLEX(sub).add(k as usize) = *COMPLEX(x).add(i as usize);
                        }
                        t if t == SEXPTYPE::STRSXP.0 => {
                            SET_STRING_ELT(sub, k, STRING_ELT(x, i));
                        }
                        t if t == SEXPTYPE::VECSXP.0 => {
                            SET_VECTOR_ELT(sub, k, VECTOR_ELT(x, i));
                        }
                        t if t == SEXPTYPE::RAWSXP.0 => {
                            *RAW(sub).add(k as usize) = *RAW(x).add(i as usize);
                        }
                        _ => {
                            error("split: unimplemented type");
                        }
                    }

                    if have_names {
                        let nmj = getAttrib(sub, R_NamesSymbol());
                        SET_STRING_ELT(nmj, k, STRING_ELT(nm, i));
                    }

                    *counts_data.add((j - 1) as usize) += 1.0;
                }

                i1 += 1;
                if i1 == nfac {
                    i1 = 0;
                }
            }

            setAttrib(vec, R_NamesSymbol(), getAttrib(f, R_LevelsSymbol()));
            Rf_unprotect(2);
            vec
        } else {
            // Normal (non-long) vector path: counts stored as c_int in INTSXP
            let counts_data = INTEGER(counts);
            for i in 0..nlevs as usize {
                *counts_data.add(i) = 0;
            }

            // First pass: count elements per level
            let mut i1: R_xlen_t = 0;
            for i in 0..nobs {
                let j = *f_data.add(i1 as usize);
                if j != NA_INTEGER {
                    if j > nlevs || j < 1 {
                        error("factor has bad level");
                    }
                    *counts_data.add((j - 1) as usize) += 1;
                }
                i1 += 1;
                if i1 == nfac {
                    i1 = 0;
                }
            }

            // Allocate result list
            let vec = Rf_allocVector3(SEXPTYPE::VECSXP.0, nlevs as R_xlen_t);
            Rf_protect(vec);

            for i in 0..nlevs as R_xlen_t {
                let count = *counts_data.add(i as usize) as R_xlen_t;
                let sub = Rf_allocVector3(xtype, count);
                SET_VECTOR_ELT(vec, i, sub);
                setAttrib(sub, R_LevelsSymbol(), getAttrib(x, R_LevelsSymbol()));
                if have_names {
                    setAttrib(
                        sub,
                        R_NamesSymbol(),
                        Rf_allocVector3(SEXPTYPE::STRSXP.0, count),
                    );
                }
            }

            // Reset counts for second pass
            for i in 0..nlevs as usize {
                *counts_data.add(i) = 0;
            }

            // Second pass: fill sub-vectors
            let mut i1: R_xlen_t = 0;
            for i in 0..nobs {
                let j = *f_data.add(i1 as usize);
                if j != NA_INTEGER {
                    let k = *counts_data.add((j - 1) as usize) as R_xlen_t;
                    let sub = VECTOR_ELT(vec, (j - 1) as R_xlen_t);

                    match xtype {
                        t if t == SEXPTYPE::LGLSXP.0 || t == SEXPTYPE::INTSXP.0 => {
                            *INTEGER(sub).add(k as usize) = *INTEGER(x).add(i as usize);
                        }
                        t if t == SEXPTYPE::REALSXP.0 => {
                            *REAL(sub).add(k as usize) = *REAL(x).add(i as usize);
                        }
                        t if t == SEXPTYPE::CPLXSXP.0 => {
                            *COMPLEX(sub).add(k as usize) = *COMPLEX(x).add(i as usize);
                        }
                        t if t == SEXPTYPE::STRSXP.0 => {
                            SET_STRING_ELT(sub, k, STRING_ELT(x, i));
                        }
                        t if t == SEXPTYPE::VECSXP.0 => {
                            SET_VECTOR_ELT(sub, k, VECTOR_ELT(x, i));
                        }
                        t if t == SEXPTYPE::RAWSXP.0 => {
                            *RAW(sub).add(k as usize) = *RAW(x).add(i as usize);
                        }
                        _ => {
                            error("split: unimplemented type");
                        }
                    }

                    if have_names {
                        let nmj = getAttrib(sub, R_NamesSymbol());
                        SET_STRING_ELT(nmj, k, STRING_ELT(nm, i));
                    }

                    *counts_data.add((j - 1) as usize) += 1;
                }

                i1 += 1;
                if i1 == nfac {
                    i1 = 0;
                }
            }

            setAttrib(vec, R_NamesSymbol(), getAttrib(f, R_LevelsSymbol()));
            Rf_unprotect(2);
            vec
        }
    }
}
