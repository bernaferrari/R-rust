/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2012-2024   The R Core Team.
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
 *  Ported from r-source/src/library/utils/src/sock.c
 */

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::attrib_core::setAttrib;
use crate::main::coerce::asInteger;
use crate::mainutils::errors::Rf_error;
use crate::modules::internet::rsock::{
    in_Rsockclose, in_Rsockconnect, in_Rsocklisten, in_Rsockopen, in_Rsockread, in_Rsockwrite,
};
use crate::sexp::accessors::{LENGTH, SET_STRING_ELT, STRING_ELT, translateChar};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_allocVector, Rf_mkChar, Rf_mkCharLen,
};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

unsafe fn require_scalar(x: SEXP, message: &[u8]) {
    unsafe {
        if LENGTH(x) != 1 {
            Rf_error(message.as_ptr() as *const c_char);
        }
    }
}

pub unsafe fn sockconnect(sport: SEXP, shost: SEXP) -> SEXP {
    unsafe {
        require_scalar(sport, b"invalid 'socket' argument\0");
        let mut port = asInteger(sport);
        let mut host = translateChar(STRING_ELT(shost, 0)) as *mut c_char;
        let mut hostp = &mut host as *mut *mut c_char;
        in_Rsockconnect(&mut port, hostp);
        Rf_ScalarInteger(port)
    }
}

pub unsafe fn sockread(sport: SEXP, smaxlen: SEXP) -> SEXP {
    unsafe {
        require_scalar(sport, b"invalid 'socket' argument\0");
        let mut sock = asInteger(sport);
        let mut maxlen = asInteger(smaxlen);
        if maxlen < 0 {
            Rf_error(b"maxlen must be non-negative\0".as_ptr() as *const c_char);
        }

        let mut buf: *mut c_char = ptr::null_mut();
        let mut abuf = &mut buf as *mut *mut c_char;
        in_Rsockread(&mut sock, abuf, &mut maxlen);
        if maxlen < 0 {
            Rf_error(b"Error reading data in Rsockread\0".as_ptr() as *const c_char);
        }

        let ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, 1));
        let charsxp = if maxlen == 0 {
            Rf_mkCharLen(b"\0".as_ptr() as *const c_char, 0)
        } else {
            Rf_mkCharLen(buf, maxlen)
        };
        SET_STRING_ELT(ans, 0, charsxp);
        Rf_unprotect(1);
        ans
    }
}

pub unsafe fn sockclose(sport: SEXP) -> SEXP {
    unsafe {
        require_scalar(sport, b"invalid 'socket' argument\0");
        let mut sock = asInteger(sport);
        if sock <= 0 {
            Rf_error(b"attempt to close invalid socket\0".as_ptr() as *const c_char);
        }
        in_Rsockclose(&mut sock);
        Rf_ScalarLogical(sock)
    }
}

pub unsafe fn sockopen(sport: SEXP) -> SEXP {
    unsafe {
        require_scalar(sport, b"invalid 'port' argument\0");
        let mut port = asInteger(sport);
        in_Rsockopen(&mut port);
        Rf_ScalarInteger(port)
    }
}

pub unsafe fn socklisten(sport: SEXP) -> SEXP {
    unsafe {
        require_scalar(sport, b"invalid 'socket' argument\0");
        let mut sock = asInteger(sport);
        let mut len: c_int = 256;
        let mut buf = [0 as c_char; 257];
        let mut bufp = buf.as_mut_ptr();
        let mut abuf = &mut bufp as *mut *mut c_char;
        in_Rsocklisten(&mut sock, abuf, &mut len);

        let ans = Rf_protect(Rf_ScalarInteger(sock));
        let host = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, 1));
        SET_STRING_ELT(host, 0, Rf_mkChar(buf.as_ptr()));
        setAttrib(ans, Rf_install(b"host\0".as_ptr() as *const c_char), host);
        Rf_unprotect(2);
        ans
    }
}

pub unsafe fn sockwrite(sport: SEXP, sstring: SEXP) -> SEXP {
    unsafe {
        require_scalar(sport, b"invalid 'socket' argument\0");
        let mut sock = asInteger(sport);
        let mut buf = translateChar(STRING_ELT(sstring, 0)) as *mut c_char;
        let mut abuf = &mut buf as *mut *mut c_char;
        let mut start: c_int = 0;
        let mut len = libc::strlen(buf) as c_int;
        let mut end = len;
        in_Rsockwrite(&mut sock, abuf, &mut start, &mut end, &mut len);
        Rf_ScalarInteger(len)
    }
}
