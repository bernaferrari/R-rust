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
use crate::mainutils::identical::R_compute_identical;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::protect;

unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe { crate::main::coerce::asInteger(x) }
}

const HT_TYPE_IDENTICAL: c_int = 1;
const HT_TYPE_ADDRESS: c_int = 2;

type R_hashtab_type = SEXP;

const HASH_TYPE_SLOT: R_xlen_t = 0;
const HASH_KEYS_SLOT: R_xlen_t = 1;
const HASH_VALUES_SLOT: R_xlen_t = 2;
const HASH_SLOT_COUNT: c_int = 3;
const HASH_IDENTICAL_FLAGS: c_int = 0;

unsafe fn checkArgCountPop(args: SEXP, n: c_int) -> SEXP {
    unsafe {
        let args = CDR(args);
        if LENGTH(args) != n {
            Rf_error(b"wrong argument count\0".as_ptr() as *const libc::c_char);
        }
        args
    }
}

unsafe fn HT_TypeFromString(x: SEXP) -> c_int {
    unsafe {
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
}

fn nil_value() -> SEXP {
    unsafe { R_NilValue() }
}

unsafe fn hash_error(message: &'static [u8]) -> ! {
    unsafe {
        Rf_error(message.as_ptr() as *const libc::c_char);
        panic!("Rf_error returned unexpectedly")
    }
}

unsafe fn R_mkhashtab(hash_type: c_int, _k: c_int) -> SEXP {
    unsafe {
        let table = Rf_allocVector(SEXPTYPE::VECSXP, HASH_SLOT_COUNT);
        if table.is_null() {
            return R_NilValue();
        }
        let _table_guard = protect(table);
        SET_VECTOR_ELT(table, HASH_TYPE_SLOT, Rf_ScalarInteger(hash_type));
        SET_VECTOR_ELT(table, HASH_KEYS_SLOT, Rf_allocVector(SEXPTYPE::VECSXP, 0));
        SET_VECTOR_ELT(table, HASH_VALUES_SLOT, Rf_allocVector(SEXPTYPE::VECSXP, 0));
        setAttrib(
            table,
            R_ClassSymbol(),
            Rf_mkString(b"rust_hashtab\0".as_ptr() as *const libc::c_char),
        );
        table
    }
}

unsafe fn R_HashtabSEXP(h: SEXP) -> SEXP {
    h
}

unsafe fn is_hash_payload(x: SEXP) -> bool {
    unsafe {
        !x.is_null() && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) == HASH_SLOT_COUNT as R_xlen_t
    }
}

unsafe fn R_asHashtable(x: SEXP) -> R_hashtab_type {
    unsafe {
        if is_hash_payload(x) {
            return x;
        }
        if !x.is_null() && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) == 1 {
            let payload = VECTOR_ELT(x, 0);
            if is_hash_payload(payload) {
                return payload;
            }
        }
        hash_error(b"invalid hash table object\0");
    }
}

unsafe fn hash_type(h: R_hashtab_type) -> c_int {
    unsafe { asInteger(VECTOR_ELT(h, HASH_TYPE_SLOT)) }
}

unsafe fn hash_keys(h: R_hashtab_type) -> SEXP {
    unsafe { VECTOR_ELT(h, HASH_KEYS_SLOT) }
}

unsafe fn hash_values(h: R_hashtab_type) -> SEXP {
    unsafe { VECTOR_ELT(h, HASH_VALUES_SLOT) }
}

unsafe fn keys_match(hash_type: c_int, stored: SEXP, key: SEXP) -> bool {
    unsafe {
        match hash_type {
            HT_TYPE_ADDRESS => stored == key,
            HT_TYPE_IDENTICAL => R_compute_identical(stored, key, HASH_IDENTICAL_FLAGS) != 0,
            _ => false,
        }
    }
}

unsafe fn hash_index(h: R_hashtab_type, key: SEXP) -> Option<R_xlen_t> {
    unsafe {
        let keys = hash_keys(h);
        let hash_type = hash_type(h);
        for i in 0..XLENGTH(keys) {
            if keys_match(hash_type, VECTOR_ELT(keys, i), key) {
                return Some(i);
            }
        }
        None
    }
}

unsafe fn grow_vec_with_replacement(values: SEXP, index: Option<R_xlen_t>, value: SEXP) -> SEXP {
    unsafe {
        if let Some(index) = index {
            let result = Rf_allocVector(SEXPTYPE::VECSXP, XLENGTH(values) as c_int);
            for i in 0..XLENGTH(values) {
                SET_VECTOR_ELT(
                    result,
                    i,
                    if i == index {
                        value
                    } else {
                        VECTOR_ELT(values, i)
                    },
                );
            }
            return result;
        }

        let len = XLENGTH(values);
        let result = Rf_allocVector(SEXPTYPE::VECSXP, (len + 1) as c_int);
        for i in 0..len {
            SET_VECTOR_ELT(result, i, VECTOR_ELT(values, i));
        }
        SET_VECTOR_ELT(result, len, value);
        result
    }
}

unsafe fn remove_vec_index(values: SEXP, index: R_xlen_t) -> SEXP {
    unsafe {
        let len = XLENGTH(values);
        let result = Rf_allocVector(SEXPTYPE::VECSXP, (len - 1) as c_int);
        let mut out = 0;
        for i in 0..len {
            if i != index {
                SET_VECTOR_ELT(result, out, VECTOR_ELT(values, i));
                out += 1;
            }
        }
        result
    }
}

unsafe fn R_gethash(h: R_hashtab_type, key: SEXP, nomatch: SEXP) -> SEXP {
    unsafe {
        hash_index(h, key)
            .map(|index| VECTOR_ELT(hash_values(h), index))
            .unwrap_or(nomatch)
    }
}

unsafe fn R_sethash(h: R_hashtab_type, key: SEXP, value: SEXP) -> SEXP {
    unsafe {
        let index = hash_index(h, key);
        let keys = grow_vec_with_replacement(hash_keys(h), index, key);
        let values = grow_vec_with_replacement(hash_values(h), index, value);
        SET_VECTOR_ELT(h, HASH_KEYS_SLOT, keys);
        SET_VECTOR_ELT(h, HASH_VALUES_SLOT, values);
        value
    }
}

unsafe fn R_remhash(h: R_hashtab_type, key: SEXP) -> c_int {
    unsafe {
        let Some(index) = hash_index(h, key) else {
            return 0;
        };
        let keys = remove_vec_index(hash_keys(h), index);
        let values = remove_vec_index(hash_values(h), index);
        SET_VECTOR_ELT(h, HASH_KEYS_SLOT, keys);
        SET_VECTOR_ELT(h, HASH_VALUES_SLOT, values);
        1
    }
}

unsafe fn R_numhash(h: R_hashtab_type) -> c_int {
    unsafe { XLENGTH(hash_keys(h)) as c_int }
}

unsafe fn R_typhash(h: R_hashtab_type) -> c_int {
    unsafe { hash_type(h) }
}

unsafe fn R_maphash(_h: R_hashtab_type, _fun: SEXP) -> SEXP {
    unsafe { hash_error(b"maphash is not implemented in the Rust utils hash table yet\0") }
}

unsafe fn R_clrhash(h: R_hashtab_type) {
    unsafe {
        SET_VECTOR_ELT(h, HASH_KEYS_SLOT, Rf_allocVector(SEXPTYPE::VECSXP, 0));
        SET_VECTOR_ELT(h, HASH_VALUES_SLOT, Rf_allocVector(SEXPTYPE::VECSXP, 0));
    }
}

unsafe fn R_isHashtable(x: SEXP) -> c_int {
    unsafe {
        if is_hash_payload(x) {
            return TRUE;
        }
        if !x.is_null() && TYPEOF(x) == SEXPTYPE::VECSXP && XLENGTH(x) == 1 {
            return is_hash_payload(VECTOR_ELT(x, 0)) as c_int;
        }
        FALSE
    }
}

pub unsafe fn hashtab_Ext(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArgCountPop(args, 2);
        let _type = HT_TypeFromString(CAR(args));
        let _k = asInteger(CADR(args));
        let val = Rf_allocVector(SEXPTYPE::VECSXP, 1);
        let _val_guard = protect(val);
        SET_VECTOR_ELT(val, 0, R_HashtabSEXP(R_mkhashtab(_type, _k)));
        setAttrib(
            val,
            R_ClassSymbol(),
            Rf_mkString(b"hashtab\0".as_ptr() as *const libc::c_char),
        );
        val
    }
}

pub unsafe fn gethash_Ext(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArgCountPop(args, 3);
        let h = R_asHashtable(CAR(args));
        let key = CADR(args);
        let nomatch = CADDR(args);
        R_gethash(h, key, nomatch)
    }
}

pub unsafe fn sethash_Ext(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArgCountPop(args, 3);
        let h = R_asHashtable(CAR(args));
        let key = CADR(args);
        let value = CADDR(args);
        R_sethash(h, key, value)
    }
}

pub unsafe fn remhash_Ext(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArgCountPop(args, 2);
        let h = R_asHashtable(CAR(args));
        let key = CADR(args);
        Rf_ScalarLogical(R_remhash(h, key))
    }
}

pub unsafe fn numhash_Ext(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArgCountPop(args, 1);
        let h = R_asHashtable(CAR(args));
        Rf_ScalarInteger(R_numhash(h))
    }
}

pub unsafe fn typhash_Ext(args: SEXP) -> SEXP {
    unsafe {
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
}

pub unsafe fn maphash_Ext(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArgCountPop(args, 2);
        let h = R_asHashtable(CAR(args));
        let fun = CADR(args);
        R_maphash(h, fun)
    }
}

pub unsafe fn clrhash_Ext(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArgCountPop(args, 1);
        let h = R_asHashtable(CAR(args));
        R_clrhash(h);
        nil_value()
    }
}

pub unsafe fn ishashtab_Ext(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArgCountPop(args, 1);
        Rf_ScalarLogical(R_isHashtable(CAR(args)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn identical_hash_table_sets_replaces_gets_and_removes() {
        let _session = RSession::new();
        unsafe {
            let table = R_mkhashtab(HT_TYPE_IDENTICAL, 8);
            assert_eq!(R_isHashtable(table), TRUE);
            assert_eq!(R_numhash(table), 0);

            let key = Rf_ScalarInteger(1);
            let equal_key = Rf_ScalarInteger(1);
            let value = Rf_ScalarInteger(10);
            let replacement = Rf_ScalarInteger(20);
            let missing = Rf_ScalarInteger(-1);

            assert_eq!(R_gethash(table, key, missing), missing);
            assert_eq!(R_sethash(table, key, value), value);
            assert_eq!(R_numhash(table), 1);
            assert_eq!(R_gethash(table, equal_key, missing), value);

            assert_eq!(R_sethash(table, equal_key, replacement), replacement);
            assert_eq!(R_numhash(table), 1);
            assert_eq!(R_gethash(table, key, missing), replacement);

            assert_eq!(R_remhash(table, equal_key), TRUE);
            assert_eq!(R_numhash(table), 0);
            assert_eq!(R_gethash(table, key, missing), missing);
        }
    }

    #[test]
    fn address_hash_table_uses_pointer_identity() {
        let _session = RSession::new();
        unsafe {
            let table = R_mkhashtab(HT_TYPE_ADDRESS, 8);
            let key = Rf_ScalarInteger(1);
            let equal_but_distinct_key = Rf_ScalarInteger(1);
            let value = Rf_ScalarInteger(10);
            let missing = Rf_ScalarInteger(-1);

            R_sethash(table, key, value);
            assert_eq!(R_gethash(table, key, missing), value);
            assert_eq!(R_gethash(table, equal_but_distinct_key, missing), missing);
        }
    }

    #[test]
    fn wrapper_is_recognized_as_hash_table() {
        let _session = RSession::new();
        unsafe {
            let wrapper = Rf_allocVector(SEXPTYPE::VECSXP, 1);
            SET_VECTOR_ELT(wrapper, 0, R_mkhashtab(HT_TYPE_IDENTICAL, 1));
            assert_eq!(R_isHashtable(wrapper), TRUE);
            assert!(!R_asHashtable(wrapper).is_null());
        }
    }

    #[test]
    fn clear_hash_removes_all_entries() {
        let _session = RSession::new();
        unsafe {
            let table = R_mkhashtab(HT_TYPE_IDENTICAL, 8);
            R_sethash(table, Rf_ScalarInteger(1), Rf_ScalarInteger(10));
            R_sethash(table, Rf_ScalarInteger(2), Rf_ScalarInteger(20));
            assert_eq!(R_numhash(table), 2);
            R_clrhash(table);
            assert_eq!(R_numhash(table), 0);
        }
    }
}
