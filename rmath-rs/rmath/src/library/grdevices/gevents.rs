/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2004-2007  The R Foundation
 *  Copyright (C) 2013-2017  The R Core Team
 *
 *  Ported from r-source/src/main/gevents.c
 *
 *  Modal event handling in R graphics (by Duncan Murdoch).
 *  Functions: do_setGraphicsEventEnv, do_getGraphicsEventEnv,
 *  do_getGraphicsEvent, doMouseEvent, doKeybd, doIdle, doesIdle.
 */

use std::os::raw::{c_char, c_double, c_int};

use crate::main::coerce::asLogical;
use crate::main::errors::{Rf_error, Rf_warning};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;
use crate::sexp::symbol::Rf_install;

use super::device_registry;

type pGEDevDesc = device_registry::pGEDevDesc;
type pDevDesc = device_registry::pDevDesc;

const CLOSXP: SEXPTYPE = SEXPTYPE::CLOSXP;
const ENVSXP: SEXPTYPE = SEXPTYPE::ENVSXP;
const PROMSXP: SEXPTYPE = SEXPTYPE::PROMSXP;
const INTSXP: SEXPTYPE = SEXPTYPE::INTSXP;

unsafe fn R_findVar(_name: SEXP, _env: SEXP) -> SEXP {
    R_NilValue()
}
unsafe fn Rf_eval(_expr: SEXP, _env: SEXP) -> SEXP {
    R_NilValue()
}
unsafe fn defineVar(_name: SEXP, _val: SEXP, _env: SEXP) {}
unsafe fn isString(_x: SEXP) -> bool {
    false
}
unsafe fn R_ProcessEvents() {}
unsafe fn R_CheckUserInterrupt() {}
unsafe fn R_FlushConsole() {}
unsafe fn Rf_lang1(_a: SEXP) -> SEXP {
    R_NilValue()
}
unsafe fn Rf_lang4(_a: SEXP, _b: SEXP, _c: SEXP, _d: SEXP) -> SEXP {
    R_NilValue()
}

const R_MAX_DEVICES: c_int = 65;

const MOUSE_HANDLERS: &[&[u8]] = &[b"onMouseDown\0", b"onMouseUp\0", b"onMouseMove\0"];
const KEYBD_HANDLER: &[u8] = b"onKeybd\0";
const IDLE_HANDLER: &[u8] = b"onIdle\0";

const KEYNAMES: &[&[u8]] = &[
    b"Left\0", b"Up\0", b"Right\0", b"Down\0",
    b"F1\0", b"F2\0", b"F3\0", b"F4\0", b"F5\0", b"F6\0",
    b"F7\0", b"F8\0", b"F9\0", b"F10\0", b"F11\0", b"F12\0",
    b"PgUp\0", b"PgDn\0", b"End\0", b"Home\0", b"Ins\0", b"Del\0",
];

pub const LEFT_BUTTON: c_int = 1;
pub const MIDDLE_BUTTON: c_int = 2;
pub const RIGHT_BUTTON: c_int = 4;

unsafe fn check_handler(name: *const c_char, event_env: SEXP) {
    let handler = R_findVar(Rf_install(name), event_env);
    if TYPEOF(handler) == CLOSXP {
        Rf_warning(b"'%s' events not supported in this device\0".as_ptr() as *const c_char);
    }
}

fn cstr_ptr(s: &[u8]) -> *const c_char {
    s.as_ptr() as *const c_char
}

unsafe fn dev_number(dd: pDevDesc) -> c_int {
    for i in 1..device_registry::NumDevices() {
        let gd = device_registry::GEgetDevice(i);
        if gd == dd {
            return i;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe fn do_setGraphicsEventEnv(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _env: SEXP,
) -> SEXP {
    let mut args = args;

    let devnum = *INTEGER(CAR(args)).add(0) - 1;
    if devnum < 1 || devnum >= R_MAX_DEVICES {
        Rf_error(b"invalid graphical device number\0".as_ptr() as *const c_char);
    }

    let gdd = device_registry::GEgetDevice(devnum);
    if gdd.is_null() {
        Rf_error(b"invalid device\0".as_ptr() as *const c_char);
    }
    args = CDR(args);

    let event_env = CAR(args);
    if TYPEOF(event_env) != ENVSXP {
        Rf_error(b"internal error\0".as_ptr() as *const c_char);
    }

    if (*gdd).canGenMouseDown == 0
        && (*gdd).canGenMouseUp == 0
        && (*gdd).canGenMouseMove == 0
        && (*gdd).canGenKeybd == 0
        && (*gdd).canGenIdle == 0
    {
        Rf_error(b"this graphics device does not support event handling\0".as_ptr() as *const c_char);
    }

    if (*gdd).canGenMouseDown == 0 {
        check_handler(cstr_ptr(MOUSE_HANDLERS[0]), event_env);
    }
    if (*gdd).canGenMouseUp == 0 {
        check_handler(cstr_ptr(MOUSE_HANDLERS[1]), event_env);
    }
    if (*gdd).canGenMouseMove == 0 {
        check_handler(cstr_ptr(MOUSE_HANDLERS[2]), event_env);
    }
    if (*gdd).canGenKeybd == 0 {
        check_handler(cstr_ptr(KEYBD_HANDLER), event_env);
    }
    if (*gdd).canGenIdle == 0 {
        check_handler(cstr_ptr(IDLE_HANDLER), event_env);
    }

    (*gdd).eventEnv = event_env as usize;

    R_NilValue()
}

#[unsafe(no_mangle)]
pub unsafe fn do_getGraphicsEventEnv(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _env: SEXP,
) -> SEXP {
    let mut devnum = *INTEGER(CAR(args)).add(0);
    if devnum == NA_INTEGER {
        Rf_error(b"invalid graphical device number\0".as_ptr() as *const c_char);
    }
    devnum -= 1;
    if devnum < 1 || devnum >= R_MAX_DEVICES {
        Rf_error(b"invalid graphical device number\0".as_ptr() as *const c_char);
    }

    let gdd = device_registry::GEgetDevice(devnum);
    if gdd.is_null() {
        Rf_error(b"invalid device\0".as_ptr() as *const c_char);
    }
    (*gdd).eventEnv as SEXP
}

unsafe fn have_listening_dev() -> bool {
    if device_registry::NoDevices() != 0 {
        return false;
    }
    for i in 1..device_registry::NumDevices() {
        let gd = device_registry::GEgetDevice(i);
        if !gd.is_null() && (*gd).gettingEvent != 0 {
            return true;
        }
    }
    false
}

#[unsafe(no_mangle)]
pub unsafe fn do_getGraphicsEvent(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _env: SEXP,
) -> SEXP {
    let mut result = R_NilValue();
    let prompt = CAR(args);
    if !isString(prompt) || LENGTH(prompt) == 0 {
        Rf_error(b"invalid prompt\0".as_ptr() as *const c_char);
    }

    if device_registry::NoDevices() == 0 {
        let mut count: c_int = 0;
        let mut i: c_int;
        let mut dev_num = device_registry::curDevice();
        i = 1;
        while i < device_registry::NumDevices() {
            let gd = device_registry::GEgetDevice(dev_num);
            if !gd.is_null() {
                if (*gd).gettingEvent != 0 {
                    Rf_error(b"recursive use of 'getGraphicsEvent' not supported\0".as_ptr() as *const c_char);
                }
            }
        }
    }
    result
}

#[unsafe(no_mangle)]
pub unsafe fn doMouseEvent(
    dd: pDevDesc,
    event: c_int,
    buttons: c_int,
    x: c_double,
    y: c_double,
) {
    (*dd).gettingEvent = 0;

    let handler_name = if (event as usize) < MOUSE_HANDLERS.len() {
        cstr_ptr(MOUSE_HANDLERS[event as usize])
    } else {
        (*dd).gettingEvent = 1;
        return;
    };

    Rf_protect(handler_name as SEXP);
    let handler = R_findVar(Rf_install(handler_name), (*dd).eventEnv as SEXP);
    Rf_protect(handler);
    let handler = if TYPEOF(handler) == PROMSXP {
        let h = Rf_eval(handler, (*dd).eventEnv as SEXP);
        Rf_unprotect(1);
        Rf_protect(h);
        h
    } else {
        handler
    };

    if TYPEOF(handler) == CLOSXP {
        let s_which = Rf_install(b"which\0".as_ptr() as *const c_char);
        defineVar(s_which, Rf_ScalarInteger(dev_number(dd) + 1), (*dd).eventEnv as SEXP);

        let len = ((buttons & LEFT_BUTTON) != 0) as c_int
            + ((buttons & MIDDLE_BUTTON) != 0) as c_int
            + ((buttons & RIGHT_BUTTON) != 0) as c_int;

        let bvec = Rf_allocVector(INTSXP, len as c_int);
        Rf_protect(bvec);
        let mut idx = 0;
        if buttons & LEFT_BUTTON != 0 {
            *INTEGER(bvec).add(idx) = 0;
            idx += 1;
        }
    }
    Rf_unprotect(2);
    (*dd).gettingEvent = 1;
}

#[unsafe(no_mangle)]
pub unsafe fn doKeybd(dd: pDevDesc, rkey: c_int, keyname: *const c_char) {
    (*dd).gettingEvent = 0;

    let handler = R_findVar(Rf_install(cstr_ptr(KEYBD_HANDLER)), (*dd).eventEnv as SEXP);
    Rf_protect(handler);
    let handler = if TYPEOF(handler) == PROMSXP {
        let h = Rf_eval(handler, (*dd).eventEnv as SEXP);
        Rf_unprotect(1);
        Rf_protect(h);
        h
    } else {
        handler
    };

    if TYPEOF(handler) == CLOSXP {
        let s_which = Rf_install(b"which\0".as_ptr() as *const c_char);
        defineVar(s_which, Rf_ScalarInteger(dev_number(dd) + 1), (*dd).eventEnv as SEXP);

        let skey = if !keyname.is_null() {
            Rf_mkString(keyname)
        } else if (rkey as usize) < KEYNAMES.len() {
            Rf_mkString(cstr_ptr(KEYNAMES[rkey as usize]))
        } else {
            Rf_mkString(b"\0".as_ptr() as *const c_char)
        };
        Rf_protect(skey);
        let temp = Rf_lang2(handler, skey);
        Rf_protect(temp);
        let result = Rf_eval(temp, (*dd).eventEnv as SEXP);
        Rf_protect(result);
        defineVar(Rf_install(b"result\0".as_ptr() as *const c_char), result, (*dd).eventEnv as SEXP);
        Rf_unprotect(3);
        R_FlushConsole();
    }
    Rf_unprotect(1);
    (*dd).gettingEvent = 1;
}

#[unsafe(no_mangle)]
pub unsafe fn doIdle(dd: pDevDesc) {
    (*dd).gettingEvent = 0;

    let handler = R_findVar(Rf_install(cstr_ptr(IDLE_HANDLER)), (*dd).eventEnv as SEXP);
    Rf_protect(handler);
    let handler = if TYPEOF(handler) == PROMSXP {
        let h = Rf_eval(handler, (*dd).eventEnv as SEXP);
        Rf_unprotect(1);
        Rf_protect(h);
        h
    } else {
        handler
    };

    if TYPEOF(handler) == CLOSXP {
        let s_which = Rf_install(b"which\0".as_ptr() as *const c_char);
        defineVar(s_which, Rf_ScalarInteger(dev_number(dd) + 1), (*dd).eventEnv as SEXP);
        let temp = Rf_lang1(handler);
        Rf_protect(temp);
        let result = Rf_eval(temp, (*dd).eventEnv as SEXP);
        Rf_protect(result);
        defineVar(Rf_install(b"result\0".as_ptr() as *const c_char), result, (*dd).eventEnv as SEXP);
        Rf_unprotect(2);
        R_FlushConsole();
    }
    Rf_unprotect(1);
    (*dd).gettingEvent = 1;
}

#[unsafe(no_mangle)]
pub unsafe fn doesIdle(dd: pDevDesc) -> c_int {
    let handler = R_findVar(Rf_install(cstr_ptr(IDLE_HANDLER)), (*dd).eventEnv as SEXP);
    if handler != R_UnboundValue() && handler != R_NilValue() {
        1
    } else {
        0
    }
}
