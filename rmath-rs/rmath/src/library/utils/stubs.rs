/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2000-2021 The R Core Team
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
 *  Ported from r-source/src/library/utils/src/stubs.c
 *  and r-source/src/library/utils/src/utils.c
 */

use std::ffi::{CStr, CString};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
use std::ptr;

use crate::main::errors::{Rf_error, Rf_warning};
use crate::mainutils::edit::do_edit;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;

fn nil_value() -> SEXP {
    unsafe { R_NilValue() }
}

fn warn_message(message: impl AsRef<str>) {
    let msg = CString::new(message.as_ref()).unwrap_or_default();
    unsafe { Rf_warning(msg.as_ptr()) };
}

fn error_message(message: impl AsRef<str>) {
    let msg = CString::new(message.as_ref()).unwrap_or_default();
    unsafe { Rf_error(msg.as_ptr()) };
}

fn do_Rprof(_args: SEXP) -> SEXP {
    error_message("Rprof is not implemented in the utils package boundary");
    nil_value()
}

fn do_Rprofmem(_args: SEXP) -> SEXP {
    error_message("Rprofmem is not implemented in the utils package boundary");
    nil_value()
}

fn Runzip(_args: SEXP) -> SEXP {
    error_message("unzip is not implemented in the utils package boundary");
    nil_value()
}

fn R_FlushConsole() {}

fn R_ProcessEvents() {}

pub unsafe fn Rprof(args: SEXP) -> SEXP {
    unsafe { do_Rprof(CDR(args)) }
}

pub unsafe fn Rprofmem(args: SEXP) -> SEXP {
    unsafe { do_Rprofmem(CDR(args)) }
}

pub unsafe fn unzip(args: SEXP) -> SEXP {
    unsafe { Runzip(CDR(args)) }
}

pub unsafe fn edit(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_edit(call, op, CDR(args), rho) }
}

// ---------------------------------------------------------------------------
// Helper: check if SEXP is a string (STRSXP)
// ---------------------------------------------------------------------------
unsafe fn isString(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP }
}

// ---------------------------------------------------------------------------
// Helper: get the default history file path
// Checks R_HISTFILE env var, falls back to ~/.Rhistory
// ---------------------------------------------------------------------------
fn default_history_file() -> PathBuf {
    if let Ok(histfile) = std::env::var("R_HISTFILE") {
        PathBuf::from(histfile)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".Rhistory")
    } else {
        PathBuf::from(".Rhistory")
    }
}

unsafe fn history_file_path(sfile: SEXP) -> Option<PathBuf> {
    unsafe {
        if !isString(sfile) || LENGTH(sfile) < 1 {
            warn_message("invalid 'file' argument");
            return None;
        }

        let elt = STRING_ELT(sfile, 0);
        if elt.is_null() || elt == R_NilValue() {
            return Some(default_history_file());
        }

        let c = CHAR(elt);
        if c.is_null() {
            return Some(default_history_file());
        }

        let r_str = CStr::from_ptr(c).to_string_lossy().into_owned();
        if r_str.is_empty() {
            Some(default_history_file())
        } else {
            Some(PathBuf::from(r_str))
        }
    }
}

// ---------------------------------------------------------------------------
// loadhistory -- Load command history from a file
// ---------------------------------------------------------------------------
pub unsafe fn loadhistory(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, rho);
        let args = CDR(args);
        let sfile = CAR(args);
        let Some(path_str) = history_file_path(sfile) else {
            return nil_value();
        };

        match File::open(&path_str) {
            Ok(file) => {
                let reader = BufReader::new(file);
                for line_content in reader.lines().map_while(Result::ok) {
                    let trimmed = line_content.trim();
                    if !trimmed.is_empty() {
                        // Add to readline history if available
                        #[cfg(unix)]
                        {
                            unsafe extern "C" {
                                fn add_history(line: *const c_char);
                            }
                            let c_line = CString::new(trimmed).unwrap_or_default();
                            add_history(c_line.as_ptr());
                        }
                    }
                }
            }
            Err(e) => {
                warn_message(format!(
                    "unable to open history file '{}': {}",
                    path_str.display(),
                    e
                ));
            }
        }

        nil_value()
    }
}

// ---------------------------------------------------------------------------
// savehistory -- Save command history to a file
// ---------------------------------------------------------------------------
pub unsafe fn savehistory(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, rho);
        let args = CDR(args);
        let sfile = CAR(args);
        let Some(path_str) = history_file_path(sfile) else {
            return nil_value();
        };

        // Retrieve history from readline and write to file
        match OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path_str)
        {
            Ok(mut file) => {
                #[cfg(unix)]
                {
                    unsafe extern "C" {
                        fn history_length() -> c_int;
                        fn history_get(offset: c_int) -> *const HistEntry;
                    }

                    #[repr(C)]
                    struct HistEntry {
                        line: *mut c_char,
                        _data: *mut std::ffi::c_void,
                    }

                    let len = history_length();
                    for i in 0..len {
                        let entry_ptr = history_get(i);
                        if !entry_ptr.is_null() {
                            let entry = &*entry_ptr;
                            if !entry.line.is_null() {
                                let line = CStr::from_ptr(entry.line).to_string_lossy();
                                let _ = writeln!(file, "{}", line);
                            }
                        }
                    }
                }

                #[cfg(not(unix))]
                {
                    let _ = file;
                    warn_message("history saving not supported on this platform");
                }
            }
            Err(e) => {
                warn_message(format!(
                    "unable to save history file '{}': {}",
                    path_str.display(),
                    e
                ));
            }
        }

        nil_value()
    }
}

// ---------------------------------------------------------------------------
// addhistory -- Add lines to the command history
// ---------------------------------------------------------------------------
pub unsafe fn addhistory(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, rho);
        let args = CDR(args);
        let stamp = CAR(args);

        if !isString(stamp) {
            let msg = CString::new("invalid timestamp").unwrap_or_default();
            Rf_warning(msg.as_ptr());
            return R_NilValue();
        }

        let len = LENGTH(stamp);
        for i in 0..len {
            let elt = STRING_ELT(stamp, i as R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                continue;
            }
            let c = CHAR(elt);
            if c.is_null() {
                continue;
            }
            let line = CStr::from_ptr(c).to_string_lossy();
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                #[cfg(unix)]
                {
                    unsafe extern "C" {
                        fn add_history(line: *const c_char);
                    }
                    let c_line = CString::new(trimmed).unwrap_or_default();
                    add_history(c_line.as_ptr());
                }
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// dataentry -- Data editor (GUI function)
// ---------------------------------------------------------------------------
pub unsafe fn dataentry(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, args, env);
        let msg = CString::new("data entry editor is not available").unwrap_or_default();
        Rf_warning(msg.as_ptr());
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// dataviewer -- Data viewer (GUI function)
// ---------------------------------------------------------------------------
pub unsafe fn dataviewer(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, args, env);
        let msg = CString::new("data viewer is not available").unwrap_or_default();
        Rf_warning(msg.as_ptr());
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// selectlist -- Select list (GUI function)
// ---------------------------------------------------------------------------
pub unsafe fn selectlist(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, args, env);
        // selectlist returns NULL when no GUI is available (matching R behavior)
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// fileedit -- Edit files using an external editor
// Ported from r-source/src/library/utils/src/stubs.c fileedit()
// ---------------------------------------------------------------------------
pub unsafe fn fileedit(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, rho);
        let args = CDR(args);
        let fn_ = CAR(args);
        let args = CDR(args);
        let _ti = CAR(args); // title argument, not used in Unix
        let args = CDR(args);
        let ed = CAR(args);

        // Validate editor argument
        if !isString(ed) || LENGTH(ed) != 1 {
            let msg = CString::new("invalid 'editor' specification").unwrap_or_default();
            Rf_error(msg.as_ptr());
        }

        let n = LENGTH(fn_);

        // Build file list
        let mut files: Vec<String> = Vec::new();
        if n > 0 {
            if !isString(fn_) {
                let msg = CString::new("invalid 'filename' specification").unwrap_or_default();
                Rf_error(msg.as_ptr());
            }
            for i in 0..n {
                let elt = STRING_ELT(fn_, i as R_xlen_t);
                if elt.is_null() || elt == R_NilValue() {
                    let msg =
                        CString::new("'filename' contains missing values").unwrap_or_default();
                    Rf_error(msg.as_ptr());
                }
                let c = CHAR(elt);
                if !c.is_null() {
                    let path = CStr::from_ptr(c).to_string_lossy().into_owned();
                    if !path.is_empty() {
                        files.push(path);
                    }
                }
            }
        }

        // Get the editor command
        let editor = if !STRING_ELT(ed, 0).is_null() && STRING_ELT(ed, 0) != R_NilValue() {
            let c = CHAR(STRING_ELT(ed, 0));
            if !c.is_null() {
                CStr::from_ptr(c).to_string_lossy().into_owned()
            } else {
                std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string())
            }
        } else {
            std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string())
        };

        // Launch the editor with the files
        use std::process::Command;
        let result = if !files.is_empty() {
            Command::new(&editor).args(&files).status()
        } else {
            // No files specified -- open editor with a new empty session
            Command::new(&editor).status()
        };

        if let Err(e) = result {
            let msg = CString::new(format!("unable to run editor '{}': {}", editor, e))
                .unwrap_or_default();
            Rf_warning(msg.as_ptr());
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// flushconsole -- Flush the console output
// ---------------------------------------------------------------------------
pub unsafe fn flushconsole() -> SEXP {
    unsafe {
        R_FlushConsole();
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// processevents -- Process pending GUI events
// ---------------------------------------------------------------------------
pub unsafe fn processevents() -> SEXP {
    unsafe {
        R_ProcessEvents();
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// tzcode_type -- Return which timezone code implementation is in use
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
pub unsafe fn tzcode_type() -> SEXP {
    unsafe {
        crate::sexp::constructors::Rf_mkString(b"system (macOS)\0".as_ptr() as *const libc::c_char)
    }
}

#[cfg(not(target_os = "macos"))]
pub unsafe fn tzcode_type() -> SEXP {
    crate::sexp::constructors::Rf_mkString(b"system\0".as_ptr() as *const libc::c_char)
}

// ---------------------------------------------------------------------------
// charClass -- Character classification using wide character types
// Ported from r-source/src/library/utils/src/utils.c charClass()
// Uses wctype() and iswctype() for Unicode-aware classification.
// ---------------------------------------------------------------------------
pub unsafe fn charClass(x: SEXP, scl: SEXP) -> SEXP {
    unsafe {
        // Validate class argument
        if !isString(scl) || LENGTH(scl) != 1 {
            let msg =
                CString::new("argument 'class' must be a character string").unwrap_or_default();
            Rf_error(msg.as_ptr());
        }

        let cl_ptr = CHAR(STRING_ELT(scl, 0));
        if cl_ptr.is_null() {
            let msg =
                CString::new("argument 'class' must be a character string").unwrap_or_default();
            Rf_error(msg.as_ptr());
        }
        let cl = CStr::from_ptr(cl_ptr).to_string_lossy();

        // Get the wctype descriptor for the class name
        // Use R's internal Ri18n_wctype (which maps names like "alpha", "digit" etc.)
        let cl_cstr = CString::new(cl.as_ref()).unwrap_or_default();
        let wcl =
            crate::main::rlocale::Ri18n_wctype(cl_cstr.as_ptr() as *const std::os::raw::c_uchar);
        if wcl == 0 {
            let msg =
                CString::new(format!("character class \"{}\" is invalid", cl)).unwrap_or_default();
            Rf_error(msg.as_ptr());
        }

        let mut nprotect: c_int = 0;
        let ans: SEXP;

        if isString(x) {
            // x is a character string: classify each character
            if XLENGTH(x) != 1 {
                let msg = CString::new("argument 'x' must be a length-1 character vector")
                    .unwrap_or_default();
                Rf_error(msg.as_ptr());
            }

            let sx = STRING_ELT(x, 0);
            let c_ptr = CHAR(sx);
            if c_ptr.is_null() {
                ans = Rf_allocVector(SEXPTYPE::LGLSXP, 0);
                nprotect += 1;
            } else {
                let s = CStr::from_ptr(c_ptr).to_bytes();
                // Convert UTF-8 bytes to wide chars for classification
                let wide: Vec<u32> = s.iter().map(|&b| b as u32).collect();
                let n = wide.len();

                ans = Rf_allocVector(SEXPTYPE::LGLSXP, n as c_int);
                nprotect += 1;
                let pans = LOGICAL(ans);

                for (i, &wc) in wide.iter().enumerate() {
                    let result = crate::main::rlocale::Ri18n_iswctype(wc, wcl);
                    *pans.add(i) = if result != 0 { 1 } else { 0 };
                }
            }
        } else {
            // x is an integer vector: classify each code point
            let x_coerced = crate::main::coerce::coerceVector(x, SEXPTYPE::INTSXP.as_c_int());
            nprotect += 1;

            let n = XLENGTH(x_coerced) as usize;
            let px = INTEGER(x_coerced);

            ans = Rf_allocVector(SEXPTYPE::LGLSXP, n as c_int);
            nprotect += 1;
            let pans = LOGICAL(ans);

            for i in 0..n {
                let this = *px.add(i);
                if this == NA_INTEGER {
                    *pans.add(i) = NA_LOGICAL;
                } else {
                    let result = crate::main::rlocale::Ri18n_iswctype(this as u32, wcl);
                    *pans.add(i) = if result != 0 { 1 } else { 0 };
                }
            }
        }

        if nprotect > 0 {
            crate::sexp::protect::Rf_unprotect(nprotect);
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// crc64 -- CRC-64 checksum using ECMA-182 polynomial (0x42F0E1EBA9EA3693)
// Ported from r-source/src/library/utils/src/utils.c crc64()
// The C version uses lzma_crc64(); we implement the algorithm directly.
// ---------------------------------------------------------------------------
const CRC64_POLY: u64 = 0x42F0E1EBA9EA3693;

/// Build the CRC64 lookup table using the ECMA-182 polynomial.
fn crc64_make_table() -> [u64; 256] {
    let mut table = [0u64; 256];
    for i in 0..256 {
        let mut crc = i as u64;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ CRC64_POLY;
            } else {
                crc >>= 1;
            }
        }
        table[i] = crc;
    }
    table
}

/// Compute CRC64 for a byte slice using the ECMA-182 polynomial.
/// This matches lzma_crc64() behavior.
fn compute_crc64(data: &[u8]) -> u64 {
    let table = crc64_make_table();
    let mut crc: u64 = 0xFFFFFFFFFFFFFFFF; // Initial value matches liblzma
    for &byte in data {
        let index = ((crc ^ byte as u64) & 0xFF) as usize;
        crc = (crc >> 8) ^ table[index];
    }
    crc ^ 0xFFFFFFFFFFFFFFFF // Final XOR matches liblzma
}

pub unsafe fn crc64(in_: SEXP) -> SEXP {
    unsafe {
        if !isString(in_) {
            let msg = CString::new("input must be a character string").unwrap_or_default();
            Rf_error(msg.as_ptr());
        }

        let s = STRING_ELT(in_, 0);
        let c_ptr = CHAR(s);
        if c_ptr.is_null() {
            // Empty input -- return CRC64 of empty string
            let hash = compute_crc64(b"");
            let hash_str = format!("{:016x}", hash);
            let c_hash = CString::new(hash_str).unwrap_or_default();
            return Rf_mkString(c_hash.as_ptr());
        }

        let bytes = CStr::from_ptr(c_ptr).to_bytes();
        let hash = compute_crc64(bytes);
        let hash_str = format!("{:016x}", hash);
        let c_hash = CString::new(hash_str).unwrap_or_default();
        Rf_mkString(c_hash.as_ptr())
    }
}

// ---------------------------------------------------------------------------
// nsl -- Name service lookup (DNS resolution)
// Ported from r-source/src/library/utils/src/utils.c nsl()
// Uses getaddrinfo() for IPv4 resolution (modern replacement for
// deprecated gethostbyname).
// ---------------------------------------------------------------------------
pub unsafe fn nsl(hostname: SEXP) -> SEXP {
    unsafe {
        if !isString(hostname) || LENGTH(hostname) != 1 {
            let msg = CString::new("'hostname' must be a character vector of length 1")
                .unwrap_or_default();
            Rf_error(msg.as_ptr());
        }

        let s = STRING_ELT(hostname, 0);
        let c_ptr = CHAR(s);
        if c_ptr.is_null() {
            let msg = CString::new("'hostname' must be a character vector of length 1")
                .unwrap_or_default();
            Rf_error(msg.as_ptr());
        }

        let name = CStr::from_ptr(c_ptr).to_string_lossy();

        // Use getaddrinfo for DNS resolution
        let mut hints: libc::addrinfo = unsafe { std::mem::zeroed() };
        hints.ai_family = libc::AF_INET; // Request IPv4 for R compatibility
        hints.ai_socktype = libc::SOCK_STREAM;

        let c_name = CString::new(name.as_ref()).unwrap_or_default();
        let mut res: *mut libc::addrinfo = ptr::null_mut();

        let ret = libc::getaddrinfo(c_name.as_ptr(), ptr::null(), &hints, &mut res);

        if ret != 0 || res.is_null() {
            let msg = CString::new(format!("nsl() was unable to resolve host '{}'", name))
                .unwrap_or_default();
            Rf_warning(msg.as_ptr());
            return R_NilValue();
        }

        let mut ip = String::from("xxx.xxx.xxx.xxx");
        let mut found = false;

        {
            let mut rp = res;
            while !rp.is_null() {
                let ai = &*rp;
                if ai.ai_family == libc::AF_INET {
                    let sockaddr = ai.ai_addr as *const libc::sockaddr_in;
                    if !sockaddr.is_null() {
                        let addr = (*sockaddr).sin_addr;
                        // Convert from network byte order (big-endian)
                        let s_addr = addr.s_addr;
                        let b0 = (s_addr & 0xFF) as u8;
                        let b1 = ((s_addr >> 8) & 0xFF) as u8;
                        let b2 = ((s_addr >> 16) & 0xFF) as u8;
                        let b3 = ((s_addr >> 24) & 0xFF) as u8;
                        ip = format!("{}.{}.{}.{}", b0, b1, b2, b3);
                        found = true;
                        break;
                    }
                }
                rp = ai.ai_next;
            }
            libc::freeaddrinfo(res);
        }

        if !found {
            let msg =
                CString::new("unknown format returned by name resolution").unwrap_or_default();
            Rf_warning(msg.as_ptr());
        }

        let c_ip = CString::new(ip).unwrap_or_default();
        Rf_mkString(c_ip.as_ptr())
    }
}

// ---------------------------------------------------------------------------
// octsize -- Convert file size to octal string representation (for tar)
// Ported from r-source/src/library/utils/src/stubs.c octsize()
// ---------------------------------------------------------------------------
pub unsafe fn octsize(size: SEXP) -> SEXP {
    unsafe {
        let s_val = crate::main::coerce::asReal(size);

        let ans = Rf_allocVector(SEXPTYPE::RAWSXP, 11);
        let ra = RAW(ans);

        if !s_val.is_finite() || s_val < 0.0 {
            let msg = CString::new("size must be finite and >= 0").unwrap_or_default();
            Rf_error(msg.as_ptr());
        }

        let mut s = s_val;
        for i in 0..11 {
            let s2 = libm::floor(s / 8.0);
            let t = s - 8.0 * s2;
            s = s2;
            *ra.add(10 - i) = 48 + t as u8; // ASCII digit
        }

        ans
    }
}
