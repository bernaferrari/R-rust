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

#[derive(Copy, Clone)]
struct SizeSexp(SEXP);

impl SizeSexp {
    unsafe fn new(raw: SEXP) -> Self {
        Self(raw)
    }

    fn type_of(self) -> std::os::raw::c_int {
        unsafe { TYPEOF(self.0) }
    }

    fn length(self) -> usize {
        unsafe { LENGTH(self.0) as usize }
    }

    fn xlength(self) -> usize {
        unsafe { XLENGTH(self.0) as usize }
    }

    fn tag(self) -> SEXP {
        unsafe { TAG(self.0) }
    }

    fn car(self) -> SEXP {
        unsafe { CAR(self.0) }
    }

    fn cdr(self) -> SEXP {
        unsafe { CDR(self.0) }
    }

    fn attrib(self) -> SEXP {
        unsafe { ATTRIB(self.0) }
    }

    fn formals(self) -> SEXP {
        unsafe { FORMALS(self.0) }
    }

    fn body(self) -> SEXP {
        unsafe { BODY(self.0) }
    }

    fn string_elt(self, index: R_xlen_t) -> SEXP {
        unsafe { STRING_ELT(self.0, index) }
    }

    fn vector_elt(self, index: R_xlen_t) -> SEXP {
        unsafe { VECTOR_ELT(self.0, index) }
    }

    fn external_ptr_tag(self) -> SEXP {
        unsafe { crate::main::memory_main::R_ExternalPtrTag(self.0) }
    }

    fn external_ptr_protected(self) -> SEXP {
        unsafe { crate::main::memory_main::R_ExternalPtrProtected(self.0) }
    }
}

unsafe fn objectsize(raw: SEXP) -> R_size_t {
    let s = unsafe { SizeSexp::new(raw) };
    let mut cnt: R_size_t = 0;
    let mut vcnt: R_size_t = 0;
    let mut is_vec = false;
    let t = s.type_of();

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
            cnt += unsafe { objectsize(current.tag()) };
            cnt += unsafe { objectsize(current.car()) };
            cnt += std::mem::size_of::<*mut std::ffi::c_void>();
            cnt += unsafe { objectsize(current.attrib()) };
            current = unsafe { SizeSexp::new(current.cdr()) };
            let ct = current.type_of();
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
        cnt += unsafe { objectsize(current.0) };
    } else if t == SEXPTYPE::CLOSXP {
        /* CLOSXP */
        cnt += unsafe { objectsize(s.formals()) };
        cnt += unsafe { objectsize(s.body()) };
    } else if t == SEXPTYPE::ENVSXP
        || t == SEXPTYPE::PROMSXP
        || t == SEXPTYPE::SPECIALSXP
        || t == SEXPTYPE::BUILTINSXP
    {
        // nothing
    } else if t == SEXPTYPE::CHARSXP {
        /* CHARSXP */
        vcnt = (s.length() + 1).div_ceil(8);
        is_vec = true;
    } else if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
        /* LGLSXP, INTSXP */
        vcnt = s.xlength();
        is_vec = true;
    } else if t == SEXPTYPE::REALSXP {
        /* REALSXP */
        vcnt = s.xlength();
        is_vec = true;
    } else if t == SEXPTYPE::CPLXSXP {
        /* CPLXSXP */
        vcnt = s.xlength() * 2;
        is_vec = true;
    } else if t == SEXPTYPE::STRSXP {
        /* STRSXP */
        vcnt = s.xlength();
        for i in 0..s.xlength() {
            let tmp = s.string_elt(i as R_xlen_t);
            if !tmp.is_null() {
                cnt += unsafe { objectsize(tmp) };
            }
        }
        is_vec = true;
    } else if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP || t == SEXPTYPE::WEAKREFSXP {
        /* VECSXP, EXPRSXP, WEAKREFSXP */
        vcnt = s.xlength();
        for i in 0..s.xlength() {
            cnt += unsafe { objectsize(s.vector_elt(i as R_xlen_t)) };
        }
        is_vec = true;
    } else if t == SEXPTYPE::EXTPTRSXP {
        /* EXTPTRSXP */
        cnt += std::mem::size_of::<*mut std::ffi::c_void>();
        cnt += unsafe { objectsize(s.external_ptr_tag()) };
        cnt += unsafe { objectsize(s.external_ptr_protected()) };
    } else if t == SEXPTYPE::RAWSXP {
        /* RAWSXP */
        vcnt = s.xlength().div_ceil(8);
        is_vec = true;
    } else if t == SEXPTYPE::S4SXP.0 {
        /* OBJSXP */
        cnt += unsafe { objectsize(s.tag()) };
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

    if s.type_of() != SEXPTYPE::CHARSXP {
        /* not CHARSXP */
        cnt += unsafe { objectsize(s.attrib()) };
    }
    cnt
}

pub unsafe fn objectSize(x: SEXP) -> SEXP {
    unsafe { Rf_ScalarReal(objectsize(x) as f64) }
}
