/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2012-2021   The R Core Team.
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
 *  Ported from r-source/src/library/utils/src/hashtab.c
 */

use std::os::raw::c_int;

use crate::attrib_core::{R_ClassSymbol, setAttrib};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

unsafe fn asInteger(x: SEXP) -> c_int {
    crate::main::coerce::asInteger(x)
}

const HT_TYPE_IDENTICAL: c_int = 1;
const HT_TYPE_ADDRESS: c_int = 2;

type R_hashtab_type = SEXP;

unsafe fn checkArgCountPop(args: SEXP, n: c_int) -> SEXP {
    let args = CDR(args);
    if LENGTH(args) != n {
        Rf_error(b"wrong argument count\0".as_ptr() as *const libc::c_char);
    }
    args
}

unsafe fn HT_TypeFromString(x: SEXP) -> c_int {
    if TYPEOF(x) != SEXPTYPE::STRSXP || XLENGTH(x) != 1 {
        Rf_error(b"hash table type must be a scalar string\0".as_ptr() as *const libc::c_char);
    }
    let s = CHAR(STRING_ELT(x, 0));
    let s_str = std::ffi::CStr::from_ptr(s);
    let s_bytes = s_str.to_bytes();
    if s_bytes == b"identical" {
        HT_TYPE_IDENTICAL
    } else if s_bytes == b"address" {
        HT_TYPE_ADDRESS
    } else {
        Rf_error(b"hash table type is not supported\0".as_ptr() as *const libc::c_char);
        0
    }
}

/* Stub: hash table internals not yet ported */
unsafe fn R_mkhashtab(_type: c_int, _k: c_int) -> SEXP {
    R_NilValue()
}

unsafe fn R_HashtabSEXP(h: SEXP) -> SEXP {
    h
}

unsafe fn R_asHashtable(x: SEXP) -> R_hashtab_type {
    x
}

unsafe fn R_gethash(_h: R_hashtab_type, _key: SEXP, nomatch: SEXP) -> SEXP {
    nomatch
}

unsafe fn R_sethash(_h: R_hashtab_type, _key: SEXP, _value: SEXP) -> SEXP {
    R_NilValue()
}

unsafe fn R_remhash(_h: R_hashtab_type, _key: SEXP) -> c_int {
    0
}

unsafe fn R_numhash(_h: R_hashtab_type) -> c_int {
    0
}

unsafe fn R_typhash(_h: R_hashtab_type) -> c_int {
    HT_TYPE_IDENTICAL
}

unsafe fn R_maphash(_h: R_hashtab_type, _fun: SEXP) -> SEXP {
    R_NilValue()
}

unsafe fn R_clrhash(_h: R_hashtab_type) {}

unsafe fn R_isHashtable(_x: SEXP) -> c_int {
    0
}

pub unsafe fn hashtab_Ext(args: SEXP) -> SEXP {
    let args = checkArgCountPop(args, 2);
    let _type = HT_TypeFromString(CAR(args));
    let _k = asInteger(CADR(args));
    let val = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 1));
    SET_VECTOR_ELT(val, 0, R_HashtabSEXP(R_mkhashtab(_type, _k)));
    setAttrib(
        val,
        R_ClassSymbol(),
        Rf_mkString(b"hashtab\0".as_ptr() as *const libc::c_char),
    );
    Rf_unprotect(1);
    val
}

pub unsafe fn gethash_Ext(args: SEXP) -> SEXP {
    let args = checkArgCountPop(args, 3);
    let h = R_asHashtable(CAR(args));
    let key = CADR(args);
    let nomatch = CADDR(args);
    R_gethash(h, key, nomatch)
}

pub unsafe fn sethash_Ext(args: SEXP) -> SEXP {
    let args = checkArgCountPop(args, 3);
    let h = R_asHashtable(CAR(args));
    let key = CADR(args);
    let value = CADDR(args);
    R_sethash(h, key, value)
}

pub unsafe fn remhash_Ext(args: SEXP) -> SEXP {
    let args = checkArgCountPop(args, 2);
    let h = R_asHashtable(CAR(args));
    let key = CADR(args);
    Rf_ScalarLogical(R_remhash(h, key))
}

pub unsafe fn numhash_Ext(args: SEXP) -> SEXP {
    let args = checkArgCountPop(args, 1);
    let h = R_asHashtable(CAR(args));
    Rf_ScalarInteger(R_numhash(h))
}

pub unsafe fn typhash_Ext(args: SEXP) -> SEXP {
    let args = checkArgCountPop(args, 1);
    let h = R_asHashtable(CAR(args));
    match R_typhash(h) {
        HT_TYPE_IDENTICAL => Rf_mkString(b"identical\0".as_ptr() as *const libc::c_char),
        HT_TYPE_ADDRESS => Rf_mkString(b"address\0".as_ptr() as *const libc::c_char),
        _ => {
            Rf_error(b"bad hash table type\0".as_ptr() as *const libc::c_char);
            std::ptr::null_mut()
        }
    }
}

pub unsafe fn maphash_Ext(args: SEXP) -> SEXP {
    let args = checkArgCountPop(args, 2);
    let h = R_asHashtable(CAR(args));
    let fun = CADR(args);
    R_maphash(h, fun)
}

pub unsafe fn clrhash_Ext(args: SEXP) -> SEXP {
    let args = checkArgCountPop(args, 1);
    let h = R_asHashtable(CAR(args));
    R_clrhash(h);
    R_NilValue()
}

pub unsafe fn ishashtab_Ext(args: SEXP) -> SEXP {
    let args = checkArgCountPop(args, 1);
    Rf_ScalarLogical(R_isHashtable(CAR(args)))
}
