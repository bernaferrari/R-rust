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
 *  Copyright (C) 1998--2023 The R Core Team.
 *
 *  Ported to Rust for the rmath-rs project.
 *  Based on R's r-source/src/library/tools/src/install.c
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
use crate::sexp::accessors::{CHAR, INTEGER, LENGTH, LOGICAL, STRING_ELT, TYPEOF};
use crate::sexp::constructors::{Rf_allocVector, Rf_isString};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// dirchmod
// ---------------------------------------------------------------------------

/// Recursively fix up permissions: used for R CMD INSTALL and build.
/// 'gwsxp' means set group-write permissions on directories.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dirchmod(dr: SEXP, gwsxp: SEXP) -> SEXP {
    if dr.is_null() {
        return R_NilValue();
    }
    if Rf_isString(dr) == 0 || LENGTH(dr) != 1 {
        Rf_error(b"invalid 'dir' argument\0".as_ptr() as *const _);
    }

    let grpwrt = if !gwsxp.is_null() {
        coerce_to_logical(gwsxp) != 0
    } else {
        false
    };

    let dir_elt = STRING_ELT(dr, 0);
    if dir_elt.is_null() {
        return R_NilValue();
    }
    let dir_ptr = CHAR(dir_elt);
    if dir_ptr.is_null() {
        return R_NilValue();
    }
    let dir_str = match CStr::from_ptr(dir_ptr).to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => return R_NilValue(),
    };

    chmod_one(&dir_str, grpwrt);

    R_NilValue()
}

/// Recursively chmod a directory tree.
fn chmod_one(name: &str, grpwrt: bool) {
    if name == "." || name == ".." {
        return;
    }

    #[cfg(unix)]
    {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let metadata = match fs::metadata(name) {
            Ok(m) => m,
            Err(_) => return,
        };

        let mut mode = metadata.permissions().mode();

        let mask: u32 = if grpwrt {
            0o664 // S_IRUSR | S_IRGRP | S_IROTH | S_IWUSR | S_IWGRP
        } else {
            0o644 // S_IRUSR | S_IRGRP | S_IROTH | S_IWUSR
        };
        let dirmask: u32 = mask | 0o111; // S_IXUSR | S_IXGRP | S_IXOTH

        if metadata.is_dir() {
            // Set directory permissions
            if let Err(_) = fs::set_permissions(name, fs::Permissions::from_mode(dirmask)) {
                // Ignore errors
            }

            // Recurse into directory
            if let Ok(entries) = fs::read_dir(name) {
                for entry in entries.flatten() {
                    let child_name = entry.file_name();
                    let child_str = child_name.to_string_lossy();
                    let full_path = if name.ends_with('/') {
                        format!("{}{}", name, child_str)
                    } else {
                        format!("{}/{}", name, child_str)
                    };
                    chmod_one(&full_path, grpwrt);
                }
            }
        } else {
            // Set file permissions
            if let Err(_) =
                fs::set_permissions(name, fs::Permissions::from_mode((mode | mask) & dirmask))
            {
                // Ignore errors
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (name, grpwrt); // suppress unused warnings
    }
}

// ---------------------------------------------------------------------------
// codeFilesAppend
// ---------------------------------------------------------------------------

const APPENDBUFSIZE: usize = if cfg!(target_os = "windows") {
    512
} else {
    8192
};

/// Append code from f2 files to f1 file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn codeFilesAppend(f1: SEXP, f2: SEXP) -> SEXP {
    if f1.is_null() || f2.is_null() {
        return Rf_allocVector(SEXPTYPE::LGLSXP.0, 0);
    }
    let n1 = if Rf_isString(f1) != 0 { LENGTH(f1) } else { 0 };
    if Rf_isString(f1) == 0 || n1 != 1 {
        Rf_error(b"invalid 'file1' argument\0".as_ptr() as *const _);
    }
    if Rf_isString(f2) == 0 {
        Rf_error(b"invalid 'file2' argument\0".as_ptr() as *const _);
        return Rf_allocVector(SEXPTYPE::LGLSXP.0, 0);
    }
    let n2 = LENGTH(f2);
    if n2 < 1 {
        return Rf_allocVector(SEXPTYPE::LGLSXP.0, 0);
    }
    let n = if n1 > n2 { n1 } else { n2 };

    let ans = Rf_allocVector(SEXPTYPE::LGLSXP.0, n);
    Rf_protect(ans);
    for i in 0..n as usize {
        *LOGICAL(ans).add(i) = FALSE;
    }

    // Get file1 path
    let f1_elt = STRING_ELT(f1, 0);
    if f1_elt.is_null() {
        Rf_unprotect(1);
        return ans;
    }
    let f1_name = char_to_string(f1_elt);
    if f1_name.is_none() {
        Rf_unprotect(1);
        return ans;
    }
    let f1_path = f1_name.expect("unwrap on None/Err");

    // Open f1 for append
    use std::fs::OpenOptions;
    use std::io::{BufReader, BufWriter, Read, Write};

    let mut fp1 = match OpenOptions::new().create(true).append(true).open(&f1_path) {
        Ok(f) => BufWriter::new(f),
        Err(_) => {
            Rf_unprotect(1);
            return ans;
        }
    };

    for i in 0..n as usize {
        let mut status: c_int = 0;

        if i >= n2 as usize {
            *LOGICAL(ans).add(i) = status;
            continue;
        }

        let f2_elt = STRING_ELT(f2, i as crate::sexp::ffi::R_xlen_t);
        if f2_elt.is_null() {
            continue;
        }
        let f2_name = char_to_string(f2_elt);
        if f2_name.is_none() {
            continue;
        }
        let f2_path = f2_name.expect("unwrap on None/Err");

        let fp2_file = match std::fs::File::open(&f2_path) {
            Ok(f) => BufReader::new(f),
            Err(_) => continue,
        };

        // Write #line directive
        let line_directive = format!("#line 1 \"{}\"\n", f2_path);
        if fp1.write_all(line_directive.as_bytes()).is_err() {
            *LOGICAL(ans).add(i) = 0;
            continue;
        }

        // Copy file content
        let mut buf = [0u8; APPENDBUFSIZE];
        let mut fp2 = fp2_file;
        loop {
            match fp2.read(&mut buf) {
                Ok(0) => break,
                Ok(nbytes) => {
                    if fp1.write_all(&buf[..nbytes]).is_err() {
                        status = 0;
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        // Ensure newline at end
        if status == 0 {
            // Check if the last byte was a newline
            // Simplified: just write a newline
            let _ = fp1.write_all(b"\n");
        }

        status = 1;
        *LOGICAL(ans).add(i) = status;
    }

    drop(fp1); // flush and close
    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a CHARSXP to a Rust String.
unsafe fn char_to_string(s: SEXP) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let p = CHAR(s);
    if p.is_null() {
        return None;
    }
    CStr::from_ptr(p).to_str().ok().map(|s| s.to_owned())
}

/// Coerce SEXP to logical.
unsafe fn coerce_to_logical(x: SEXP) -> c_int {
    if x.is_null() {
        return crate::sexp::ffi::NA_LOGICAL;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP.0 {
        let p = LOGICAL(x);
        if !p.is_null() {
            return *p;
        }
    } else if t == SEXPTYPE::INTSXP.0 {
        let p = INTEGER(x);
        if !p.is_null() {
            return *p;
        }
    }
    crate::sexp::ffi::NA_LOGICAL
}
