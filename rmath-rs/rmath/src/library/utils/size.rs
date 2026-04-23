#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_assignments,
    non_camel_case_types
)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2000-2025  The R Core Team.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *  Ported from r-source/src/library/utils/src/size.c
 */

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;

type R_size_t = usize;

unsafe fn objectsize(s: SEXP) -> R_size_t {
    let mut cnt: R_size_t = 0;
    let mut vcnt: R_size_t = 0;
    let mut is_vec = false;
    let t = TYPEOF(s);

    if t == SEXPTYPE::NILSXP {
        return 0;
    }
    if t == SEXPTYPE::SYMSXP {
    } else if t == SEXPTYPE::LISTSXP
        || t == SEXPTYPE::LANGSXP
        || t == SEXPTYPE::BCODESXP
        || t == SEXPTYPE::DOTSXP
    {
        let mut current = s;
        loop {
            cnt += objectsize(TAG(current));
            cnt += objectsize(CAR(current));
            cnt += std::mem::size_of::<*mut std::ffi::c_void>();
            cnt += objectsize(ATTRIB(current));
            current = CDR(current);
            let ct = TYPEOF(current);
            if ct == SEXPTYPE::LISTSXP
                || ct == SEXPTYPE::LANGSXP
                || ct == SEXPTYPE::BCODESXP
                || ct == SEXPTYPE::DOTSXP
            {
                // continue
            } else if ct == SEXPTYPE::NILSXP {
                return cnt;
            } else {
                break;
            }
        }
        cnt += objectsize(current);
    } else if t == SEXPTYPE::CLOSXP {
        /* CLOSXP */
        cnt += objectsize(FORMALS(s));
        cnt += objectsize(BODY(s));
    } else if t == SEXPTYPE::ENVSXP
        || t == SEXPTYPE::PROMSXP
        || t == SEXPTYPE::SPECIALSXP
        || t == SEXPTYPE::BUILTINSXP
    {
        // nothing
    } else if t == SEXPTYPE::CHARSXP {
        /* CHARSXP */
        vcnt = (LENGTH(s) as usize + 1 + 7) / 8;
        is_vec = true;
    } else if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
        /* LGLSXP, INTSXP */
        vcnt = XLENGTH(s) as usize;
        is_vec = true;
    } else if t == SEXPTYPE::REALSXP {
        /* REALSXP */
        vcnt = XLENGTH(s) as usize;
        is_vec = true;
    } else if t == SEXPTYPE::CPLXSXP {
        /* CPLXSXP */
        vcnt = XLENGTH(s) as usize * 2;
        is_vec = true;
    } else if t == SEXPTYPE::STRSXP {
        /* STRSXP */
        vcnt = XLENGTH(s) as usize;
        for i in 0..(XLENGTH(s) as usize) {
            let tmp = STRING_ELT(s, i as R_xlen_t);
            if !tmp.is_null() {
                cnt += objectsize(tmp);
            }
        }
        is_vec = true;
    } else if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP || t == SEXPTYPE::WEAKREFSXP {
        /* VECSXP, EXPRSXP, WEAKREFSXP */
        vcnt = XLENGTH(s) as usize;
        for i in 0..(XLENGTH(s) as usize) {
            cnt += objectsize(VECTOR_ELT(s, i as R_xlen_t));
        }
        is_vec = true;
    } else if t == SEXPTYPE::EXTPTRSXP {
        /* EXTPTRSXP */
        cnt += std::mem::size_of::<*mut std::ffi::c_void>();
        cnt += objectsize(crate::main::memory_main::R_ExternalPtrTag(s));
        cnt += objectsize(crate::main::memory_main::R_ExternalPtrProtected(s));
    } else if t == SEXPTYPE::RAWSXP {
        /* RAWSXP */
        vcnt = (XLENGTH(s) as usize + 7) / 8;
        is_vec = true;
    } else if t == SEXPTYPE::S4SXP.0 {
        /* OBJSXP */
        cnt += objectsize(TAG(s));
    }
    // else ANYSXP etc — nothing

    if is_vec {
        cnt += std::mem::size_of::<*mut std::ffi::c_void>();
        if vcnt > 16 {
            cnt += 8 * vcnt;
        } else if vcnt > 8 {
            cnt += 128;
        } else if vcnt > 6 {
            cnt += 64;
        } else if vcnt > 4 {
            cnt += 48;
        } else if vcnt > 2 {
            cnt += 32;
        } else if vcnt > 1 {
            cnt += 16;
        } else if vcnt > 0 {
            cnt += 8;
        }
    } else {
        cnt += std::mem::size_of::<*mut std::ffi::c_void>();
    }

    if TYPEOF(s) != SEXPTYPE::CHARSXP {
        /* not CHARSXP */
        cnt += objectsize(ATTRIB(s));
    }
    cnt
}

pub unsafe fn objectSize(x: SEXP) -> SEXP {
    Rf_ScalarReal(objectsize(x) as f64)
}
