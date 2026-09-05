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
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector};
use crate::sexp::ffi::{FALSE, NA_INTEGER, SEXP, SEXPTYPE, TRUE};
use crate::sexp::protect::protect;

/// ps_kill: send a signal to a process.
pub unsafe fn ps_kill(spid: SEXP, ssignal: SEXP) -> SEXP {
    unsafe {
        let signal = coerce_to_int(ssignal);
        let sspid = Rf_coerceVector(spid, SEXPTYPE::INTSXP.as_c_int());
        let _sspid_guard = protect(sspid);
        let ns = LENGTH(sspid) as u32;
        let sres = Rf_allocVector(SEXPTYPE::LGLSXP, ns as c_int);
        let _sres_guard = protect(sres);
        let pid = INTEGER(sspid);
        let res = LOGICAL(sres);

        #[cfg(not(target_os = "windows"))]
        {
            for i in 0..ns {
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
        #[cfg(target_os = "windows")]
        {
            let _ = (signal, spid);
            crate::mainutils::errors::Rf_error(
                b"ps_kill is not supported on Windows\0".as_ptr() as *const _
            );
            return crate::sexp::globals::R_NilValue();
        }

        sres
    }
}

/// ps_priority: get/set process priority.
pub unsafe fn ps_priority(spid: SEXP, svalue: SEXP) -> SEXP {
    unsafe {
        let val = coerce_to_int(svalue);
        let sspid = Rf_coerceVector(spid, SEXPTYPE::INTSXP.as_c_int());
        let _sspid_guard = protect(sspid);
        let ns = LENGTH(sspid) as u32;
        let sres = Rf_allocVector(SEXPTYPE::INTSXP, ns as c_int);
        let _sres_guard = protect(sres);
        let pid = INTEGER(sspid);
        let res = INTEGER(sres);

        #[cfg(not(target_os = "windows"))]
        {
            for i in 0..ns {
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
                }
            }
        }
        #[cfg(target_os = "windows")]
        {
            crate::mainutils::errors::Rf_error(
                b"ps_priority is not supported on Windows\0".as_ptr() as *const _,
            );
            return crate::sexp::globals::R_NilValue();
        }

        sres
    }
}

/// ps_sigs: map signal number to platform signal value.
pub unsafe fn ps_sigs(signo: SEXP) -> SEXP {
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
    unsafe { Rf_ScalarInteger(res) }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Coerce an SEXP to an integer (simplified asLogical/asInteger equivalent).
fn coerce_to_int(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP {
            let p = INTEGER(x);
            if !p.is_null() {
                return *p;
            }
        } else if t == SEXPTYPE::LGLSXP {
            let p = LOGICAL(x);
            if !p.is_null() {
                return *p;
            }
        } else if t == SEXPTYPE::REALSXP {
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
}

/// Coerce an SEXP to integer vector (simplified coerceVector equivalent).
fn Rf_coerceVector(x: SEXP, _type: c_int) -> SEXP {
    unsafe {
        if x.is_null() {
            return std::ptr::null_mut();
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP && _type == SEXPTYPE::INTSXP {
            return x;
        }
        let n = LENGTH(x);
        let ans = Rf_allocVector(_type, n);
        if ans.is_null() {
            return std::ptr::null_mut();
        }
        let _ans_guard = protect(ans);
        if t == SEXPTYPE::INTSXP {
            let src = INTEGER(x);
            let dst = INTEGER(ans);
            for i in 0..n as usize {
                if !src.is_null() && !dst.is_null() {
                    *dst.add(i) = *src.add(i);
                }
            }
        } else if t == SEXPTYPE::REALSXP {
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
        } else if t == SEXPTYPE::LGLSXP {
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
}
