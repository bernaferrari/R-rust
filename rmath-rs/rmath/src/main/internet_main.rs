#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997--2021  The R Core Team
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
 *  Ported from r-source/src/main/internet.c
 *
 *  Internet module dispatch: routes internet/socket/curl calls through
 *  function pointers loaded from the "internet" module.
 */

use std::cell::{Cell, RefCell};
use std::os::raw::{c_char, c_double, c_int, c_void};

use crate::main::errors::Rf_error;
use crate::main::rdynload::R_moduleCdynload;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

/// PROTECT / UNPROTECT convenience wrappers
#[inline(always)]
unsafe fn PROTECT(s: SEXP) -> SEXP {
    unsafe { Rf_protect(s) }
}
#[inline(always)]
unsafe fn UNPROTECT(n: c_int) {
    unsafe {
        Rf_unprotect(n);
    }
}

/// Helper: asInteger -- extract integer from scalar SEXP.
unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::INTSXP.0 {
            *INTEGER(x)
        } else {
            *INTEGER(crate::main::coerce::coerceVector(
                x,
                SEXPTYPE::INTSXP.0 as c_int,
            ))
        }
    }
}

/// Helper: report error that internet routines cannot be loaded.
/// Rf_error does not return, but the compiler doesn't know that.
macro_rules! internet_error {
    ($($arg:expr),*) => {{
        Rf_error($($arg),*);
        unreachable!()
    }};
}

/// Rconnection: opaque pointer to a connection object.
type Rconnection = *mut c_void;

/// Function pointer types matching R_InternetRoutines struct.
type FnDownload = unsafe extern "C" fn(SEXP) -> SEXP;
type FnNewUrl = unsafe extern "C" fn(*const c_char, *const c_char, SEXP, c_int) -> Rconnection;
type FnNewSock = unsafe extern "C" fn(
    *const c_char,
    c_int,
    c_int,
    c_int,
    *const c_char,
    c_int,
    c_int,
) -> Rconnection;
type FnNewServSock = unsafe extern "C" fn(c_int) -> Rconnection;
type FnHTTPDCreate = unsafe extern "C" fn(*const c_char, c_int) -> c_int;
type FnHTTPDStop = unsafe extern "C" fn();
type FnSockConnect = unsafe extern "C" fn(*mut c_int, *mut *mut c_char);
type FnSockRead = unsafe extern "C" fn(*mut c_int, *mut *mut c_char, *mut c_int);
type FnSockClose = unsafe extern "C" fn(*mut c_int);
type FnSockOpen = unsafe extern "C" fn(*mut c_int);
type FnSockListen = unsafe extern "C" fn(*mut c_int, *mut *mut c_char, *mut c_int);
type FnSockWrite =
    unsafe extern "C" fn(*mut c_int, *mut *mut c_char, *mut c_int, *mut c_int, *mut c_int);
type FnSockSelect =
    unsafe extern "C" fn(c_int, *mut c_int, *mut c_int, *mut c_int, c_double) -> c_int;
type FnDoCall = unsafe extern "C" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP;
type FnNewCurlUrl = unsafe extern "C" fn(*const c_char, *const c_char, SEXP, c_int) -> Rconnection;

/// R_InternetRoutines -- table of function pointers for the internet module.
pub struct R_InternetRoutines {
    download: Option<FnDownload>,
    newurl: Option<FnNewUrl>,
    newsock: Option<FnNewSock>,
    newservsock: Option<FnNewServSock>,
    HTTPDCreate: Option<FnHTTPDCreate>,
    HTTPDStop: Option<FnHTTPDStop>,
    sockconnect: Option<FnSockConnect>,
    sockread: Option<FnSockRead>,
    sockclose: Option<FnSockClose>,
    sockopen: Option<FnSockOpen>,
    socklisten: Option<FnSockListen>,
    sockwrite: Option<FnSockWrite>,
    sockselect: Option<FnSockSelect>,
    curlVersion: Option<FnDoCall>,
    curlGetHeaders: Option<FnDoCall>,
    curlDownload: Option<FnDoCall>,
    newcurlurl: Option<FnNewCurlUrl>,
}

thread_local! {
    static routines: RefCell<R_InternetRoutines> = RefCell::new(R_InternetRoutines {
        download: None,
        newurl: None,
        newsock: None,
        newservsock: None,
        HTTPDCreate: None,
        HTTPDStop: None,
        sockconnect: None,
        sockread: None,
        sockclose: None,
        sockopen: None,
        socklisten: None,
        sockwrite: None,
        sockselect: None,
        curlVersion: None,
        curlGetHeaders: None,
        curlDownload: None,
        newcurlurl: None,
    });

    static ptr: Cell<*const R_InternetRoutines> = Cell::new(std::ptr::null());

    static initialized: Cell<c_int> = Cell::new(0);
}

/// R_setInternetRoutines -- set the internet routines table.
pub unsafe fn R_setInternetRoutines(
    new_routines: *const R_InternetRoutines,
) -> *const R_InternetRoutines {
    let tmp = ptr.with(|v| v.get());
    ptr.with(|v| v.set(new_routines));
    tmp
}

/// internet_Init -- initialize internet module by loading the "internet" dynload module.
unsafe fn internet_Init() {
    unsafe {
        let res = R_moduleCdynload(b"internet\0".as_ptr() as *const c_char, 1, 1);
        initialized.with(|v| v.set(-1));
        if res == 0 {
            return;
        }
        let p = ptr.with(|v| v.get());
        if p.is_null() {
            Rf_error(b"internet routines cannot be accessed in module\0".as_ptr() as *const c_char);
        }
        if (*p).download.is_none() {
            Rf_error(b"internet routines cannot be accessed in module\0".as_ptr() as *const c_char);
        }
        initialized.with(|v| v.set(1));
    }
}

/// Check that internet is initialized, return true if ready.
#[inline]
unsafe fn ensure_internet() -> bool {
    unsafe {
        if initialized.with(|v| v.get()) == 0 {
            internet_Init();
        }
        initialized.with(|v| v.get()) > 0
    }
}

/// Rdownload -- .Internal(download(args))
#[unsafe(no_mangle)]
pub unsafe fn Rdownload(args: SEXP) -> SEXP {
    unsafe {
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).download {
                return func(args);
            }
        }
        internet_error!(b"internet routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// R_newurl -- create a new URL connection (Windows only, as of R 4.2.0).
pub unsafe fn R_newurl(
    description: *const c_char,
    mode: *const c_char,
    headers: SEXP,
    rtype: c_int,
) -> Rconnection {
    unsafe {
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).newurl {
                return func(description, mode, headers, rtype);
            }
        }
        internet_error!(b"internet routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// R_newsock -- create a new socket connection.
pub unsafe fn R_newsock(
    host: *const c_char,
    port: c_int,
    server: c_int,
    serverfd: c_int,
    mode: *const c_char,
    timeout: c_int,
    options: c_int,
) -> Rconnection {
    unsafe {
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).newsock {
                return func(host, port, server, serverfd, mode, timeout, options);
            }
        }
        internet_error!(b"internet routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// R_newservsock -- create a new server socket.
pub unsafe fn R_newservsock(port: c_int) -> Rconnection {
    unsafe {
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).newservsock {
                return func(port);
            }
        }
        internet_error!(b"internet routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// extR_HTTPDCreate -- create an HTTP daemon.
pub unsafe fn extR_HTTPDCreate(ip: *const c_char, port: c_int) -> c_int {
    unsafe {
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).HTTPDCreate {
                return func(ip, port);
            }
        }
        Rf_error(b"internet routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// extR_HTTPDStop -- stop the HTTP daemon.
pub unsafe fn extR_HTTPDStop() {
    unsafe {
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).HTTPDStop {
                func();
                return;
            }
        }
        Rf_error(b"internet routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// Rsockconnect -- connect to a socket.
#[unsafe(no_mangle)]
pub unsafe fn Rsockconnect(sport: SEXP, shost: SEXP) -> SEXP {
    unsafe {
        if LENGTH(sport) != 1 {
            Rf_error(b"invalid 'socket' argument\0".as_ptr() as *const c_char);
        }
        let mut port = asInteger(sport);
        let host_str = crate::main::sysutils::translateCharFP(STRING_ELT(shost, 0));
        let mut host_buf: *mut c_char = host_str as *mut c_char;
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).sockconnect {
                func(&mut port, &mut host_buf);
                return Rf_ScalarInteger(port);
            }
        }
        internet_error!(b"socket routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// Rsockread -- read from a socket.
#[unsafe(no_mangle)]
pub unsafe fn Rsockread(ssock: SEXP, smaxlen: SEXP) -> SEXP {
    unsafe {
        if LENGTH(ssock) != 1 {
            Rf_error(b"invalid 'socket' argument\0".as_ptr() as *const c_char);
        }
        let mut sock = asInteger(ssock);
        let mut maxlen = asInteger(smaxlen);
        if maxlen < 0 {
            Rf_error(b"maxlen must be non-negative\0".as_ptr() as *const c_char);
        }
        let rbuf = PROTECT(Rf_allocVector(SEXPTYPE::RAWSXP.0 as c_int, maxlen + 1));
        let buf = RAW(rbuf) as *mut c_char;
        let mut abuf: *mut c_char = buf;
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).sockread {
                func(&mut sock, &mut abuf, &mut maxlen);
            } else {
                Rf_error(b"socket routines cannot be loaded\0".as_ptr() as *const c_char);
            }
        } else {
            Rf_error(b"socket routines cannot be loaded\0".as_ptr() as *const c_char);
        }
        if maxlen < 0 {
            Rf_error(b"Error reading data in Rsockread\0".as_ptr() as *const c_char);
        }
        let ans = PROTECT(Rf_allocVector(SEXPTYPE::STRSXP.0 as c_int, 1));
        SET_STRING_ELT(ans, 0, Rf_mkCharLen(buf as *const c_char, maxlen));
        UNPROTECT(2);
        ans
    }
}

/// Rsockclose -- close a socket.
#[unsafe(no_mangle)]
pub unsafe fn Rsockclose(ssock: SEXP) -> SEXP {
    unsafe {
        if LENGTH(ssock) != 1 {
            Rf_error(b"invalid 'socket' argument\0".as_ptr() as *const c_char);
        }
        let mut sock = asInteger(ssock);
        if sock <= 0 {
            Rf_error(b"attempt to close invalid socket\0".as_ptr() as *const c_char);
        }
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).sockclose {
                func(&mut sock);
                return Rf_ScalarLogical(sock);
            }
        }
        internet_error!(b"socket routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// Rsockopen -- open a socket.
#[unsafe(no_mangle)]
pub unsafe fn Rsockopen(sport: SEXP) -> SEXP {
    unsafe {
        if LENGTH(sport) != 1 {
            Rf_error(b"invalid 'port' argument\0".as_ptr() as *const c_char);
        }
        let mut port = asInteger(sport);
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).sockopen {
                func(&mut port);
                return Rf_ScalarInteger(port);
            }
        }
        internet_error!(b"socket routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// Rsocklisten -- listen on a socket.
#[unsafe(no_mangle)]
pub unsafe fn Rsocklisten(ssock: SEXP) -> SEXP {
    unsafe {
        if LENGTH(ssock) != 1 {
            Rf_error(b"invalid 'socket' argument\0".as_ptr() as *const c_char);
        }
        let mut sock = asInteger(ssock);
        let mut len: c_int = 256;
        let mut buf: [c_char; 257] = [0; 257];
        let mut abuf: *mut c_char = buf.as_mut_ptr();
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).socklisten {
                func(&mut sock, &mut abuf, &mut len);
            } else {
                Rf_error(b"socket routines cannot be loaded\0".as_ptr() as *const c_char);
            }
        } else {
            Rf_error(b"socket routines cannot be loaded\0".as_ptr() as *const c_char);
        }
        let ans = PROTECT(Rf_ScalarInteger(sock));
        let host = PROTECT(Rf_allocVector(SEXPTYPE::STRSXP.0 as c_int, 1));
        SET_STRING_ELT(host, 0, Rf_mkChar(buf.as_ptr() as *const c_char));
        crate::attrib_core::setAttrib(
            ans,
            crate::sexp::symbol::Rf_install(b"host\0".as_ptr() as *const c_char),
            host,
        );
        UNPROTECT(2);
        ans
    }
}

/// Rsockwrite -- write to a socket.
#[unsafe(no_mangle)]
pub unsafe fn Rsockwrite(ssock: SEXP, sstring: SEXP) -> SEXP {
    unsafe {
        if LENGTH(ssock) != 1 {
            Rf_error(b"invalid 'socket' argument\0".as_ptr() as *const c_char);
        }
        let mut sock = asInteger(ssock);
        let mut start: c_int = 0;
        let mut end: c_int;
        let mut len: c_int;
        let buf = crate::main::sysutils::translateCharFP(STRING_ELT(sstring, 0));
        let buf_len = std::ffi::CStr::from_ptr(buf).to_bytes().len() as c_int;
        end = buf_len;
        len = buf_len;
        let mut abuf: *mut c_char = buf as *mut c_char;
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).sockwrite {
                func(&mut sock, &mut abuf, &mut start, &mut end, &mut len);
                return Rf_ScalarInteger(len);
            }
        }
        internet_error!(b"socket routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// Rsockselect -- select on sockets.
pub unsafe fn Rsockselect(
    nsock: c_int,
    insockfd: *mut c_int,
    ready: *mut c_int,
    write: *mut c_int,
    timeout: c_double,
) -> c_int {
    unsafe {
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).sockselect {
                return func(nsock, insockfd, ready, write, timeout);
            }
        }
        Rf_error(b"socket routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// do_curlVersion -- .Internal(curlVersion(...))
pub unsafe fn do_curlVersion(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).curlVersion {
                return func(call, op, args, rho);
            }
        }
        internet_error!(b"internet routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// do_curlGetHeaders -- .Internal(curlGetHeaders(...))
pub unsafe fn do_curlGetHeaders(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).curlGetHeaders {
                return func(call, op, args, rho);
            }
        }
        internet_error!(b"internet routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// do_curlDownload -- .Internal(curlDownload(...))
pub unsafe fn do_curlDownload(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).curlDownload {
                return func(call, op, args, rho);
            }
        }
        internet_error!(b"internet routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}

/// R_newCurlUrl -- create a new curl URL connection.
pub unsafe fn R_newCurlUrl(
    description: *const c_char,
    mode: *const c_char,
    headers: SEXP,
    rtype: c_int,
) -> Rconnection {
    unsafe {
        if ensure_internet() {
            let p = ptr.with(|v| v.get());
            if let Some(ref func) = (*p).newcurlurl {
                return func(description, mode, headers, rtype);
            }
        }
        internet_error!(b"internet routines cannot be loaded\0".as_ptr() as *const c_char);
    }
}
