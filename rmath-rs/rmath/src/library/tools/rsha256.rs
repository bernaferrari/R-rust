#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2003-2024   The R Core Team.
 *
 *  Ported to Rust for the rmath-rs project.
 *  Based on R's r-source/src/library/tools/src/Rsha256.c
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::main::errors::Rf_error;
use crate::sexp::accessors::{CHAR, LENGTH, RAW, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_allocVector, Rf_isString, Rf_mkChar, Rf_mkString};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

use super::sha256::Sha256Ctx;

const SHA256_HASH_SIZE: usize = 32;
const SHA256_HEX_SIZE: usize = 64;

/// Convenience function for init + process + finish (mirrors C's Rsha256_buffer).
fn rsha256_buffer(buffer: *const std::ffi::c_void, len: usize, resblock: &mut [u8; 32]) -> bool {
    let mut ctx = Sha256Ctx {
        H: [0u32; 8],
        total: [0u32; 2],
        buflen: 0,
        buffer: [0u8; 128],
    };
    unsafe {
        crate::library::tools::sha256::Rsha256_init_ctx(&mut ctx);
        crate::library::tools::sha256::Rsha256_process_bytes(buffer, len, &mut ctx);
        let result = crate::library::tools::sha256::Rsha256_finish_ctx(
            &mut ctx,
            resblock.as_mut_ptr() as *mut std::ffi::c_void,
        );
        !result.is_null()
    }
}

/// Compute SHA256 hash of files or a raw buffer.
///
/// If `files` is RAWSXP, computes the SHA256 hash of the raw bytes.
/// If `files` is STRSXP, computes SHA256 hashes for each file path.
pub unsafe fn Rsha256(files: SEXP) -> SEXP {
    if files.is_null() {
        return R_NilValue();
    }

    // RAW mode: hash of one buffer instead of files
    if TYPEOF(files) == SEXPTYPE::RAWSXP.0 {
        let raw_len = XLENGTH(files) as usize;
        let raw_ptr = RAW(files);
        if raw_ptr.is_null() || raw_len == 0 {
            return Rf_mkString(ptr::null());
        }
        let raw_bytes = std::slice::from_raw_parts(raw_ptr, raw_len);

        let mut resblock = [0u8; SHA256_HASH_SIZE];
        let result = rsha256_buffer(
            raw_bytes.as_ptr() as *const std::ffi::c_void,
            raw_len,
            &mut resblock,
        );
        if !result {
            // Return NA string on failure
            return Rf_mkString(ptr::null());
        }

        let mut out = [0u8; SHA256_HEX_SIZE + 1];
        for j in 0..SHA256_HASH_SIZE {
            let hex = format!("{:02x}", resblock[j]);
            let hex_bytes = hex.as_bytes();
            out[2 * j] = hex_bytes[0];
            out[2 * j + 1] = hex_bytes[1];
        }
        out[SHA256_HEX_SIZE] = 0;

        return Rf_mkString(out.as_ptr() as *const c_char);
    }

    // Otherwise: list of files
    if Rf_isString(files) == 0 {
        Rf_error(b"argument 'files' must be character\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let nfiles = LENGTH(files);
    let ans = Rf_allocVector(SEXPTYPE::STRSXP.0, nfiles);
    Rf_protect(ans);

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

        // Read file and compute SHA256
        match std::fs::read(&path) {
            Ok(data) => {
                let mut resblock = [0u8; SHA256_HASH_SIZE];
                let result = rsha256_buffer(
                    data.as_ptr() as *const std::ffi::c_void,
                    data.len(),
                    &mut resblock,
                );
                if !result {
                    let msg = std::ffi::CString::new(format!("sha256 failed on file '{}'", path))
                        .unwrap_or_default();
                    Rf_error(msg.as_ptr());
                    SET_STRING_ELT(ans, i as R_xlen_t, ptr::null_mut());
                } else {
                    let mut out = [0u8; SHA256_HEX_SIZE + 1];
                    for j in 0..SHA256_HASH_SIZE {
                        let hex = format!("{:02x}", resblock[j]);
                        let hex_bytes = hex.as_bytes();
                        out[2 * j] = hex_bytes[0];
                        out[2 * j + 1] = hex_bytes[1];
                    }
                    out[SHA256_HEX_SIZE] = 0;
                    let char_sxp = Rf_mkChar(out.as_ptr() as *const c_char);
                    SET_STRING_ELT(ans, i as R_xlen_t, char_sxp);
                }
            }
            Err(_) => {
                // File not found
                SET_STRING_ELT(ans, i as R_xlen_t, ptr::null_mut());
            }
        }
    }

    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[inline]
unsafe fn SET_STRING_ELT(x: SEXP, i: R_xlen_t, val: SEXP) {
    crate::sexp::accessors::SET_STRING_ELT(x, i, val);
}
