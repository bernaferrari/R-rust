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
 *  Ported to Rust for the rmath-rs project.
 *  Based on R's r-source/src/library/tools/src/signals.c
 *
 *  Copyright (C) 2011--2018   The R Core Team
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 */

use std::os::raw::c_int;

use crate::sexp::accessors::{INTEGER, LENGTH, LOGICAL, TYPEOF};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector, Rf_isString};
use crate::sexp::ffi::{FALSE, NA_INTEGER, SEXP, SEXPTYPE, TRUE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

/// ps_kill: send a signal to a process.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ps_kill(spid: SEXP, ssignal: SEXP) -> SEXP {
    let signal: c_int;
    unsafe {
        signal = coerce_to_int(ssignal);
    }
    let sspid = unsafe { Rf_coerceVector(spid, SEXPTYPE::INTSXP.0) };
    Rf_protect(sspid);
    let ns = unsafe { LENGTH(sspid) as u32 };
    let sres = unsafe { Rf_allocVector(SEXPTYPE::LGLSXP.0, ns as c_int) };
    Rf_protect(sres);
    let pid = unsafe { INTEGER(sspid) };
    let res = unsafe { LOGICAL(sres) };

    #[cfg(not(target_os = "windows"))]
    {
        for i in 0..ns {
            unsafe {
                *res.add(i as usize) = FALSE;
                if signal != NA_INTEGER {
                    let p = *pid.add(i as usize);
                    if p > 0 && p != NA_INTEGER {
                        if libc::kill(p as libc::pid_t, signal as libc::c_int) == 0 {
                            *res.add(i as usize) = TRUE;
                        }
                    }
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        for i in 0..ns {
            unsafe {
                *res.add(i as usize) = FALSE;
                if signal != NA_INTEGER {
                    let p = *pid.add(i as usize);
                    // Windows: use TerminateProcess via OpenProcess
                    // This is a simplified port; full Windows support would need winapi
                    let _ = p; // suppress unused warning
                }
            }
        }
    }

    Rf_unprotect(2);
    sres
}

/// ps_priority: get/set process priority.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ps_priority(spid: SEXP, svalue: SEXP) -> SEXP {
    let val: c_int;
    unsafe {
        val = coerce_to_int(svalue);
    }
    let sspid = unsafe { Rf_coerceVector(spid, SEXPTYPE::INTSXP.0) };
    Rf_protect(sspid);
    let ns = unsafe { LENGTH(sspid) as u32 };
    let sres = unsafe { Rf_allocVector(SEXPTYPE::INTSXP.0, ns as c_int) };
    Rf_protect(sres);
    let pid = unsafe { INTEGER(sspid) };
    let res = unsafe { INTEGER(sres) };

    #[cfg(all(unix, not(target_os = "windows")))]
    {
        for i in 0..ns {
            unsafe {
                let p = *pid.add(i as usize);
                if p <= 0 {
                    *res.add(i as usize) = NA_INTEGER;
                    continue;
                }
                if p != NA_INTEGER {
                    let mut errno_save: libc::c_int = 0;
                    let r = libc::getpriority(libc::PRIO_PROCESS, p as libc::id_t);
                    if r == -1 {
                        errno_save = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                    }
                    if errno_save != 0 {
                        *res.add(i as usize) = NA_INTEGER;
                    } else {
                        *res.add(i as usize) = r;
                    }
                    if val != NA_INTEGER {
                        libc::setpriority(libc::PRIO_PROCESS, p as libc::id_t, val);
                    }
                } else {
                    *res.add(i as usize) = NA_INTEGER;
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        for i in 0..ns {
            unsafe {
                *res.add(i as usize) = NA_INTEGER;
            }
        }
    }

    Rf_unprotect(2);
    sres
}

/// ps_sigs: map signal number to platform signal value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ps_sigs(signo: SEXP) -> SEXP {
    let s = coerce_to_int(signo);
    let res: c_int = match s {
        1 => {
            #[cfg(unix)]
            {
                libc::SIGHUP as c_int
            }
            #[cfg(not(unix))]
            {
                NA_INTEGER
            }
        }
        2 => {
            #[cfg(unix)]
            {
                libc::SIGINT as c_int
            }
            #[cfg(not(unix))]
            {
                NA_INTEGER
            }
        }
        3 => {
            #[cfg(all(unix, target_os = "macos"))]
            {
                libc::SIGQUIT as c_int
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                libc::SIGQUIT as c_int
            }
            #[cfg(not(unix))]
            {
                NA_INTEGER
            }
        }
        9 => {
            #[cfg(unix)]
            {
                libc::SIGKILL as c_int
            }
            #[cfg(not(unix))]
            {
                NA_INTEGER
            }
        }
        15 => {
            #[cfg(unix)]
            {
                libc::SIGTERM as c_int
            }
            #[cfg(not(unix))]
            {
                NA_INTEGER
            }
        }
        17 => {
            #[cfg(unix)]
            {
                libc::SIGSTOP as c_int
            }
            #[cfg(not(unix))]
            {
                NA_INTEGER
            }
        }
        18 => {
            #[cfg(unix)]
            {
                libc::SIGTSTP as c_int
            }
            #[cfg(not(unix))]
            {
                NA_INTEGER
            }
        }
        19 => {
            #[cfg(unix)]
            {
                libc::SIGCONT as c_int
            }
            #[cfg(not(unix))]
            {
                NA_INTEGER
            }
        }
        20 => {
            #[cfg(unix)]
            {
                libc::SIGCHLD as c_int
            }
            #[cfg(not(unix))]
            {
                NA_INTEGER
            }
        }
        30 => {
            #[cfg(unix)]
            {
                libc::SIGUSR1 as c_int
            }
            #[cfg(not(unix))]
            {
                NA_INTEGER
            }
        }
        31 => {
            #[cfg(unix)]
            {
                libc::SIGUSR2 as c_int
            }
            #[cfg(not(unix))]
            {
                NA_INTEGER
            }
        }
        _ => NA_INTEGER,
    };
    Rf_ScalarInteger(res)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Coerce an SEXP to an integer (simplified asLogical/asInteger equivalent).
unsafe fn coerce_to_int(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP.0 {
        let p = INTEGER(x);
        if !p.is_null() {
            return *p;
        }
    } else if t == SEXPTYPE::LGLSXP.0 {
        let p = LOGICAL(x);
        if !p.is_null() {
            return *p;
        }
    } else if t == SEXPTYPE::REALSXP.0 {
        let p = crate::sexp::accessors::REAL(x);
        if !p.is_null() {
            let v = *p;
            if v.is_nan() {
                return NA_INTEGER;
            }
            return v as c_int;
        }
    }
    NA_INTEGER
}

/// Coerce an SEXP to integer vector (simplified coerceVector equivalent).
unsafe fn Rf_coerceVector(x: SEXP, _type: c_int) -> SEXP {
    if x.is_null() {
        return std::ptr::null_mut();
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP.0 && _type == SEXPTYPE::INTSXP.0 {
        return x;
    }
    let n = LENGTH(x);
    let ans = Rf_allocVector(_type, n);
    if ans.is_null() {
        return std::ptr::null_mut();
    }
    if t == SEXPTYPE::INTSXP.0 {
        let src = INTEGER(x);
        let dst = INTEGER(ans);
        for i in 0..n as usize {
            if !src.is_null() && !dst.is_null() {
                *dst.add(i) = *src.add(i);
            }
        }
    } else if t == SEXPTYPE::REALSXP.0 {
        let src = crate::sexp::accessors::REAL(x);
        let dst = INTEGER(ans);
        for i in 0..n as usize {
            if !src.is_null() && !dst.is_null() {
                let v = *src.add(i);
                if v.is_nan() {
                    *dst.add(i) = NA_INTEGER;
                } else {
                    *dst.add(i) = v as c_int;
                }
            }
        }
    } else if t == SEXPTYPE::LGLSXP.0 {
        let src = LOGICAL(x);
        let dst = INTEGER(ans);
        for i in 0..n as usize {
            if !src.is_null() && !dst.is_null() {
                *dst.add(i) = *src.add(i);
            }
        }
    }
    ans
}
