/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2003-2024   The R Core Team.
 *
 *  Ported to Rust for the rmath-rs project.
 *  Based on R's r-source/src/library/tools/src/Rmd5.c
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::ptr;

use crate::main::errors::Rf_error;
use crate::sexp::accessors::{CHAR, LENGTH, RAW, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_allocVector, Rf_isString, Rf_mkChar, Rf_mkString};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

use super::md5::md5_buffer;

/// Compute MD5 hash of files or a raw buffer.
///
/// If `files` is RAWSXP, computes the MD5 hash of the raw bytes.
/// If `files` is STRSXP, computes MD5 hashes for each file path.
pub unsafe fn Rmd5(files: SEXP) -> SEXP {
    unsafe {
        if files.is_null() {
            return R_NilValue();
        }

        // RAW mode: hash of one buffer instead of files
        if TYPEOF(files) == SEXPTYPE::RAWSXP {
            let raw_len = XLENGTH(files) as usize;
            let raw_ptr = RAW(files);
            if raw_ptr.is_null() || raw_len == 0 {
                return Rf_mkString(ptr::null());
            }
            let raw_bytes = std::slice::from_raw_parts(raw_ptr, raw_len);

            let mut resblock = [0u8; 16];
            let result = md5_buffer(
                raw_bytes.as_ptr(),
                raw_len as libc::size_t,
                resblock.as_mut_ptr() as *mut c_void,
            );
            if result.is_null() {
                return Rf_mkString(ptr::null());
            }

            let out = hex_encode_lower(&resblock);
            return Rf_mkString(out.as_ptr() as *const c_char);
        }

        if Rf_isString(files) == 0 {
            Rf_error(b"argument 'files' must be character\0".as_ptr() as *const _);
            return R_NilValue();
        }

        let nfiles = LENGTH(files);
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, nfiles);
        let _ans_guard = protect(ans);

        for i in 0..nfiles as usize {
            let file_elt = STRING_ELT(files, i as R_xlen_t);
            if file_elt.is_null() {
                SET_STRING_ELT(ans, i as R_xlen_t, ptr::null_mut());
                continue;
            }
            let path_ptr = CHAR(file_elt);
            if path_ptr.is_null() {
                SET_STRING_ELT(ans, i as R_xlen_t, ptr::null_mut());
                continue;
            }
            let path = match CStr::from_ptr(path_ptr).to_str() {
                Ok(s) => s.to_owned(),
                Err(_) => {
                    SET_STRING_ELT(ans, i as R_xlen_t, ptr::null_mut());
                    continue;
                }
            };

            match std::fs::read(&path) {
                Ok(data) => {
                    let mut resblock = [0u8; 16];
                    let result = md5_buffer(
                        data.as_ptr(),
                        data.len() as libc::size_t,
                        resblock.as_mut_ptr() as *mut c_void,
                    );
                    if result.is_null() {
                        let msg = std::ffi::CString::new(format!("md5 failed on file '{}'", path))
                            .unwrap_or_default();
                        Rf_error(msg.as_ptr());
                        SET_STRING_ELT(ans, i as R_xlen_t, ptr::null_mut());
                    } else {
                        let out = hex_encode_lower(&resblock);
                        let char_sxp = Rf_mkChar(out.as_ptr() as *const c_char);
                        SET_STRING_ELT(ans, i as R_xlen_t, char_sxp);
                    }
                }
                Err(_) => {
                    SET_STRING_ELT(ans, i as R_xlen_t, ptr::null_mut());
                }
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[inline]
fn SET_STRING_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    unsafe {
        crate::sexp::accessors::SET_STRING_ELT(x, i, val);
    }
}

fn hex_encode_lower(bytes: &[u8; 16]) -> [u8; 33] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = [0u8; 33];
    for (i, &byte) in bytes.iter().enumerate() {
        out[2 * i] = HEX[(byte >> 4) as usize];
        out[2 * i + 1] = HEX[(byte & 0x0f) as usize];
    }
    out[32] = 0;
    out
}
