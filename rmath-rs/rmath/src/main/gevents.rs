#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/main/gevents.c
 *
 *  Modal event handling in R graphics
 *  Original by Duncan Murdoch
 */

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::eval::eval::Rf_eval;
use crate::main::engine::{pDevDesc, pGEDevDesc};
use crate::main::errors::Rf_error;
use crate::main::errors::Rf_warning;
use crate::sexp::accessors::{CAR, CDR, CHAR, INTEGER, LENGTH, TYPEOF};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector, Rf_cons, Rf_lang2, Rf_mkString,
};
use crate::sexp::envir::{R_findVar, defineVar};
use crate::sexp::ffi::{NA_INTEGER, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R_MouseEvent enum values (matching R_ext/GraphicsDevice.h)
pub const R_ME_mouseDown: c_int = 0;
pub const R_ME_mouseUp: c_int = 1;
pub const R_ME_mouseMove: c_int = 2;

/// Mouse button bit masks
pub const leftButton: c_int = 1;
pub const middleButton: c_int = 2;
pub const rightButton: c_int = 4;

/// R_KeyName enum values (matching R_ext/GraphicsDevice.h)
pub const R_Key_Up: c_int = 0;
pub const R_Key_Down: c_int = 1;
pub const R_Key_Right: c_int = 2;
pub const R_Key_Left: c_int = 3;
pub const R_Key_F1: c_int = 4;
pub const R_Key_F2: c_int = 5;
pub const R_Key_F3: c_int = 6;
pub const R_Key_F4: c_int = 7;
pub const R_Key_F5: c_int = 8;
pub const R_Key_F6: c_int = 9;
pub const R_Key_F7: c_int = 10;
pub const R_Key_F8: c_int = 11;
pub const R_Key_F9: c_int = 12;
pub const R_Key_F10: c_int = 13;
pub const R_Key_F11: c_int = 14;
pub const R_Key_F12: c_int = 15;
pub const R_Key_PgUp: c_int = 16;
pub const R_Key_PgDn: c_int = 17;
pub const R_Key_End: c_int = 18;
pub const R_Key_Home: c_int = 19;
pub const R_Key_Ins: c_int = 20;
pub const R_Key_Del: c_int = 21;

/// Maximum number of graphics devices.
/// TODO: This should come from the devices module; define locally for now.
pub const R_MaxDevices: c_int = 64;

// ---------------------------------------------------------------------------
// Static strings (using AtomicPtr for Sync safety with raw pointers)
// ---------------------------------------------------------------------------

/// Helper macro to create a static C string as an AtomicPtr for use in
/// static contexts (raw pointers don't implement Sync).
macro_rules! static_cstr {
    ($name:ident, $lit:expr) => {
        static $name: AtomicPtr<c_char> = AtomicPtr::new($lit.as_ptr() as *mut c_char);
    };
}

static_cstr!(MOUSE_HANDLER_0, b"onMouseDown\0");
static_cstr!(MOUSE_HANDLER_1, b"onMouseUp\0");
static_cstr!(MOUSE_HANDLER_2, b"onMouseMove\0");
static_cstr!(KEYBD_HANDLER_NAME, b"onKeybd\0");
static_cstr!(IDLE_HANDLER_NAME, b"onIdle\0");

static_cstr!(KEYNAME_0, b"Left\0");
static_cstr!(KEYNAME_1, b"Up\0");
static_cstr!(KEYNAME_2, b"Right\0");
static_cstr!(KEYNAME_3, b"Down\0");
static_cstr!(KEYNAME_4, b"F1\0");
static_cstr!(KEYNAME_5, b"F2\0");
static_cstr!(KEYNAME_6, b"F3\0");
static_cstr!(KEYNAME_7, b"F4\0");
static_cstr!(KEYNAME_8, b"F5\0");
static_cstr!(KEYNAME_9, b"F6\0");
static_cstr!(KEYNAME_10, b"F7\0");
static_cstr!(KEYNAME_11, b"F8\0");
static_cstr!(KEYNAME_12, b"F9\0");
static_cstr!(KEYNAME_13, b"F10\0");
static_cstr!(KEYNAME_14, b"F11\0");
static_cstr!(KEYNAME_15, b"F12\0");
static_cstr!(KEYNAME_16, b"PgUp\0");
static_cstr!(KEYNAME_17, b"PgDn\0");
static_cstr!(KEYNAME_18, b"End\0");
static_cstr!(KEYNAME_19, b"Home\0");
static_cstr!(KEYNAME_20, b"Ins\0");
static_cstr!(KEYNAME_21, b"Del\0");

/// Get mouse handler name by index.
#[inline]
unsafe fn mouseHandlers(i: usize) -> *const c_char {
    match i {
        0 => MOUSE_HANDLER_0.load(Ordering::Relaxed) as *const c_char,
        1 => MOUSE_HANDLER_1.load(Ordering::Relaxed) as *const c_char,
        2 => MOUSE_HANDLER_2.load(Ordering::Relaxed) as *const c_char,
        _ => ptr::null(),
    }
}

/// Keyboard handler name.
#[inline]
unsafe fn keybdHandler() -> *const c_char {
    KEYBD_HANDLER_NAME.load(Ordering::Relaxed) as *const c_char
}

/// Idle handler name.
#[inline]
unsafe fn idleHandler() -> *const c_char {
    IDLE_HANDLER_NAME.load(Ordering::Relaxed) as *const c_char
}

/// Get key name by R_KeyName enum value.
#[inline]
unsafe fn keyname(rkey: c_int) -> *const c_char {
    match rkey {
        0 => KEYNAME_0.load(Ordering::Relaxed) as *const c_char,
        1 => KEYNAME_1.load(Ordering::Relaxed) as *const c_char,
        2 => KEYNAME_2.load(Ordering::Relaxed) as *const c_char,
        3 => KEYNAME_3.load(Ordering::Relaxed) as *const c_char,
        4 => KEYNAME_4.load(Ordering::Relaxed) as *const c_char,
        5 => KEYNAME_5.load(Ordering::Relaxed) as *const c_char,
        6 => KEYNAME_6.load(Ordering::Relaxed) as *const c_char,
        7 => KEYNAME_7.load(Ordering::Relaxed) as *const c_char,
        8 => KEYNAME_8.load(Ordering::Relaxed) as *const c_char,
        9 => KEYNAME_9.load(Ordering::Relaxed) as *const c_char,
        10 => KEYNAME_10.load(Ordering::Relaxed) as *const c_char,
        11 => KEYNAME_11.load(Ordering::Relaxed) as *const c_char,
        12 => KEYNAME_12.load(Ordering::Relaxed) as *const c_char,
        13 => KEYNAME_13.load(Ordering::Relaxed) as *const c_char,
        14 => KEYNAME_14.load(Ordering::Relaxed) as *const c_char,
        15 => KEYNAME_15.load(Ordering::Relaxed) as *const c_char,
        16 => KEYNAME_16.load(Ordering::Relaxed) as *const c_char,
        17 => KEYNAME_17.load(Ordering::Relaxed) as *const c_char,
        18 => KEYNAME_18.load(Ordering::Relaxed) as *const c_char,
        19 => KEYNAME_19.load(Ordering::Relaxed) as *const c_char,
        20 => KEYNAME_20.load(Ordering::Relaxed) as *const c_char,
        21 => KEYNAME_21.load(Ordering::Relaxed) as *const c_char,
        _ => ptr::null(),
    }
}

// ---------------------------------------------------------------------------
// Local stubs for GE device management functions
//
// These are defined in grDevices/devices.c in R. In the Rust port they are
// local stubs in grdevices/devices.rs. We redeclare them here as local
// helper stubs since the gevents module cannot depend on the library crate.
// ---------------------------------------------------------------------------

/// Stub: NoDevices - returns TRUE (no devices open).
unsafe fn NoDevices() -> c_int {
    1
}

/// Stub: NumDevices - returns 0 (no devices registered).
unsafe fn NumDevices() -> c_int {
    0
}

/// Stub: curDevice - returns 0 (no current device).
unsafe fn curDevice() -> c_int {
    0
}

/// Stub: nextDevice - returns 0.
unsafe fn nextDevice(_dev: c_int) -> c_int {
    0
}

/// Stub: GEgetDevice - returns null.
unsafe fn GEgetDevice(_dev: c_int) -> pGEDevDesc {
    ptr::null_mut()
}

/// Stub: ndevNumber - returns 0.
unsafe fn ndevNumber(_dd: pDevDesc) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// Local helper stubs for R functions used by event handling
// ---------------------------------------------------------------------------

/// Stub: checkArity - no-op for now.
unsafe fn checkArity(_op: SEXP, _args: SEXP) {
    // TODO: delegate to crate::main::errors::Rf_checkArityCall
}

/// Stub: errorcall - panic with message.
unsafe fn errorcall(_call: SEXP, msg: &str) {
    Rf_error(format!("{}\0", msg).as_ptr() as *const c_char);
}

/// Stub: error - panic with message.
unsafe fn error(msg: &str) {
    Rf_error(format!("{}\0", msg).as_ptr() as *const c_char);
}

/// Stub: warning - print warning message.
unsafe fn warning(msg: &str) {
    Rf_warning(format!("{}\0", msg).as_ptr() as *const c_char);
}

/// Stub: isString - check if SEXP is a string vector.
#[inline]
unsafe fn isString(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::STRSXP.0 }
}

/// Stub: length - get the length of an SEXP.
#[inline]
unsafe fn length(x: SEXP) -> c_int {
    unsafe { LENGTH(x) }
}

/// Stub: Rprintf - print formatted string to R console.
unsafe fn Rprintf(_format: *const c_char) {
    // TODO: delegate to crate::main::printutils::Rprintf
}

/// Stub: R_FlushConsole - flush the R console output.
#[unsafe(no_mangle)]
unsafe fn R_FlushConsole() {
    // TODO: delegate to crate::unix::system::R_FlushConsole
}

/// Stub: R_ProcessEvents - process pending R events.
#[unsafe(no_mangle)]
unsafe fn R_ProcessEvents() {
    // TODO: delegate to crate::unix::sys_unix::R_ProcessEvents
}

/// Stub: R_CheckUserInterrupt - check for user interrupt.
unsafe fn R_CheckUserInterrupt() {
    // TODO: delegate to crate::main::errors::R_CheckUserInterrupt
}

/// Stub: asChar - coerce SEXP to CHARSXP.
unsafe fn asChar(x: SEXP) -> SEXP {
    unsafe {
        if !x.is_null() && isString(x) && length(x) > 0 {
            // Return the first CHARSXP element
            let data = (*x).gengc_next_node as *mut SEXP;
            *data
        } else {
            ptr::null_mut()
        }
    }
}

/// Helper: create a lang1 call (single-element language object).
unsafe fn lang1(fn_: SEXP) -> SEXP {
    unsafe { Rf_cons(fn_, R_NilValue()) }
}

/// Helper: create a lang4 call (four-element language object).
#[unsafe(no_mangle)]
unsafe fn lang4(fn_: SEXP, a1: SEXP, a2: SEXP, a3: SEXP) -> SEXP {
    unsafe {
        let cdr3 = Rf_cons(a3, R_NilValue());
        let cdr2 = Rf_cons(a2, cdr3);
        let cdr1 = Rf_cons(a1, cdr2);
        Rf_cons(fn_, cdr1)
    }
}

// ---------------------------------------------------------------------------
// checkHandler - check if a handler exists and warn if not supported
// ---------------------------------------------------------------------------

/// Check if a handler function exists in the event environment.
/// If the handler is a closure (CLOSXP), warn that events are not supported.
unsafe fn checkHandler(name: *const c_char, eventEnv: SEXP) {
    unsafe {
        let sym = Rf_install(name);
        let handler = R_findVar(sym, eventEnv);
        if TYPEOF(handler) == SEXPTYPE::CLOSXP.0 {
            let name_str = std::ffi::CStr::from_ptr(name).to_string_lossy();
            warning(&format!(
                "'{}' events not supported in this device",
                name_str
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// haveListeningDev - check if at least one device is listening for events
// ---------------------------------------------------------------------------

/// Returns true if at least one open graphics device is listening for events.
unsafe fn haveListeningDev() -> bool {
    unsafe {
        let mut ret = false;
        if NoDevices() == 0 {
            let mut i = 1;
            loop {
                if i >= NumDevices() {
                    break;
                }
                let gd = GEgetDevice(i);
                if !gd.is_null() {
                    let dd = (*gd).dev;
                    if !dd.is_null() && (*dd).gettingEvent != 0 {
                        ret = true;
                        break;
                    }
                }
                i += 1;
            }
        }
        ret
    }
}

// ---------------------------------------------------------------------------
// do_setGraphicsEventEnv
// ---------------------------------------------------------------------------

/// Set the event environment for a graphics device.
pub unsafe fn do_setGraphicsEventEnv(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let eventEnv: SEXP;
        let devnum: c_int;
        let gdd: pGEDevDesc;
        let dd: pDevDesc;

        checkArity(op, args);

        devnum = *INTEGER(CAR(args)).add(0) - 1;
        if devnum < 1 || devnum >= R_MaxDevices {
            error("invalid graphical device number");
        }

        gdd = GEgetDevice(devnum);
        if gdd.is_null() {
            errorcall(call, "invalid device");
        }
        dd = (*gdd).dev;
        let args = CDR(args);

        eventEnv = CAR(args);
        if TYPEOF(eventEnv) != SEXPTYPE::ENVSXP.0 {
            error("internal error");
        }

        if (*dd).canGenMouseDown == 0
            && (*dd).canGenMouseUp == 0
            && (*dd).canGenMouseMove == 0
            && (*dd).canGenKeybd == 0
            && (*dd).canGenIdle == 0
        {
            error("this graphics device does not support event handling");
        }

        if (*dd).canGenMouseDown == 0 {
            checkHandler(mouseHandlers(0), eventEnv);
        }
        if (*dd).canGenMouseUp == 0 {
            checkHandler(mouseHandlers(1), eventEnv);
        }
        if (*dd).canGenMouseMove == 0 {
            checkHandler(mouseHandlers(2), eventEnv);
        }
        if (*dd).canGenKeybd == 0 {
            checkHandler(keybdHandler(), eventEnv);
        }
        if (*dd).canGenIdle == 0 {
            checkHandler(idleHandler(), eventEnv);
        }

        (*dd).eventEnv = eventEnv;

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_getGraphicsEventEnv
// ---------------------------------------------------------------------------

/// Get the event environment for a graphics device.
pub unsafe fn do_getGraphicsEventEnv(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let mut devnum: c_int;
        let gdd: pGEDevDesc;

        checkArity(op, args);

        devnum = *INTEGER(CAR(args)).add(0);
        if devnum == NA_INTEGER {
            error("invalid graphical device number");
        }
        devnum -= 1;
        if devnum < 1 || devnum >= R_MaxDevices {
            error("invalid graphical device number");
        }

        gdd = GEgetDevice(devnum);
        if gdd.is_null() {
            errorcall(call, "invalid device");
        }
        (*(*gdd).dev).eventEnv
    }
}

// ---------------------------------------------------------------------------
// do_getGraphicsEvent
// ---------------------------------------------------------------------------

/// Get a graphics event from the event loop.
pub unsafe fn do_getGraphicsEvent(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut result: SEXP = R_NilValue();
        let prompt: SEXP;
        let mut count: c_int = 0;

        checkArity(op, args);

        prompt = CAR(args);
        if !isString(prompt) || length(prompt) == 0 {
            error("invalid prompt");
        }

        /* NB: cleanup of event handlers must be done by driver in onExit handler */

        if NoDevices() == 0 {
            /* Initialize all devices */
            let mut i = 1;
            let mut devNum = curDevice();
            loop {
                i += 1;
                if i > NumDevices() {
                    break;
                }
                let gd = GEgetDevice(devNum);
                if !gd.is_null() {
                    let dd = (*gd).dev;
                    if !dd.is_null() {
                        if (*dd).gettingEvent != 0 {
                            error("recursive use of 'getGraphicsEvent' not supported");
                        }
                        if (*dd).eventEnv != R_NilValue() {
                            if let Some(helper) = (*dd).eventHelper {
                                helper(dd, 1);
                            }
                            (*dd).gettingEvent = 1;
                            let sym_result = Rf_install(b"result\0".as_ptr() as *const c_char);
                            defineVar(sym_result, R_NilValue(), (*dd).eventEnv);
                            count += 1;
                        }
                    }
                }
                devNum = nextDevice(devNum);
            }
            if count == 0 {
                error("no graphics event handlers set");
            }

            /* Print prompt */
            let prompt_str = asChar(prompt);
            if !prompt_str.is_null() {
                let _s = std::ffi::CStr::from_ptr(CHAR(prompt_str));
                // Rprintf("%s\n", CHAR(asChar(prompt)));
                // For now, we skip the actual printf since Rprintf is a stub
            }
            R_FlushConsole();

            /* Poll them */
            loop {
                if result == R_NilValue() {
                    /* make sure we still have at least one device listening */
                    if !haveListeningDev() {
                        return R_NilValue();
                    }
                } else {
                    break;
                }

                R_ProcessEvents();
                R_CheckUserInterrupt();

                let mut i = 1;
                let mut devNum = curDevice();
                loop {
                    i += 1;
                    if i > NumDevices() {
                        break;
                    }
                    let gd = GEgetDevice(devNum);
                    if !gd.is_null() {
                        let dd = (*gd).dev;
                        if !dd.is_null() && (*dd).eventEnv != R_NilValue() {
                            if let Some(helper) = (*dd).eventHelper {
                                helper(dd, 2);
                            }
                            let sym_result = Rf_install(b"result\0".as_ptr() as *const c_char);
                            result = R_findVar(sym_result, (*dd).eventEnv);
                            if result != R_NilValue() && result != R_UnboundValue() {
                                break;
                            }
                        }
                    }
                    devNum = nextDevice(devNum);
                }

                if result != R_NilValue() && result != R_UnboundValue() {
                    break;
                }
            }

            /* clean up */
            let mut i = 1;
            let mut devNum = curDevice();
            loop {
                i += 1;
                if i > NumDevices() {
                    break;
                }
                let gd = GEgetDevice(devNum);
                if !gd.is_null() {
                    let dd = (*gd).dev;
                    if !dd.is_null() && (*dd).eventEnv != R_NilValue() {
                        if let Some(helper) = (*dd).eventHelper {
                            helper(dd, 0);
                        }
                        (*dd).gettingEvent = 0;
                    }
                }
                devNum = nextDevice(devNum);
            }
        }

        result
    }
}

// ---------------------------------------------------------------------------
// doMouseEvent - handle a mouse event on a device
// ---------------------------------------------------------------------------

/// Process a mouse event. Used by devWindows.c and cairoDevice.
pub unsafe fn doMouseEvent(
    dd: pDevDesc,
    event: c_int,
    buttons: c_int,
    x: c_double,
    y: c_double,
) {
    unsafe {
        (*dd).gettingEvent = 0; /* avoid recursive calls */

        let sym = Rf_install(mouseHandlers(event as usize));
        let mut handler = Rf_protect(R_findVar(sym, (*dd).eventEnv));
        let _prot_count: c_int = 1;

        if TYPEOF(handler) == SEXPTYPE::PROMSXP.0 {
            handler = Rf_eval(handler, (*dd).eventEnv);
            Rf_unprotect(1);
            handler = Rf_protect(handler);
        }

        if TYPEOF(handler) == SEXPTYPE::CLOSXP.0 {
            let s_which = Rf_install(b"which\0".as_ptr() as *const c_char);
            defineVar(
                s_which,
                Rf_ScalarInteger(ndevNumber(dd) + 1),
                (*dd).eventEnv,
            );

            // Be portable: see PR#15793
            let len = ((buttons & leftButton) != 0) as c_int
                + ((buttons & middleButton) != 0) as c_int
                + ((buttons & rightButton) != 0) as c_int;

            let bvec = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, len));
            let mut idx: c_int = 0;
            if (buttons & leftButton) != 0 {
                *INTEGER(bvec).add(idx as usize) = 0;
                idx += 1;
            }
            if (buttons & middleButton) != 0 {
                *INTEGER(bvec).add(idx as usize) = 1;
                idx += 1;
            }
            if (buttons & rightButton) != 0 {
                *INTEGER(bvec).add(idx as usize) = 2;
                idx += 1;
            }

            let sx = Rf_protect(Rf_ScalarReal((x - (*dd).left) / ((*dd).right - (*dd).left)));
            let sy = Rf_protect(Rf_ScalarReal(
                (y - (*dd).bottom) / ((*dd).top - (*dd).bottom),
            ));
            let temp = Rf_protect(lang4(handler, bvec, sx, sy));
            let result = Rf_protect(Rf_eval(temp, (*dd).eventEnv));
            let sym_result = Rf_install(b"result\0".as_ptr() as *const c_char);
            defineVar(sym_result, result, (*dd).eventEnv);
            Rf_unprotect(5);
            R_FlushConsole();
        }
        Rf_unprotect(1); /* handler */
        (*dd).gettingEvent = 1;
    }
}

// ---------------------------------------------------------------------------
// doKeybd - handle a keyboard event on a device
// ---------------------------------------------------------------------------

/// Process a keyboard event. Used by devWindows.c and cairoDevice.
pub unsafe fn doKeybd(dd: pDevDesc, rkey: c_int, keyname_ptr: *const c_char) {
    unsafe {
        (*dd).gettingEvent = 0; /* avoid recursive calls */

        let sym = Rf_install(keybdHandler());
        let mut handler = Rf_protect(R_findVar(sym, (*dd).eventEnv));
        let _prot_count: c_int = 1;

        if TYPEOF(handler) == SEXPTYPE::PROMSXP.0 {
            handler = Rf_eval(handler, (*dd).eventEnv);
            Rf_unprotect(1);
            handler = Rf_protect(handler);
        }

        if TYPEOF(handler) == SEXPTYPE::CLOSXP.0 {
            let s_which = Rf_install(b"which\0".as_ptr() as *const c_char);
            defineVar(
                s_which,
                Rf_ScalarInteger(ndevNumber(dd) + 1),
                (*dd).eventEnv,
            );

            let actual_keyname = if !keyname_ptr.is_null() {
                keyname_ptr
            } else {
                keyname(rkey)
            };
            let skey = Rf_protect(Rf_mkString(actual_keyname));
            let temp = Rf_protect(Rf_lang2(handler, skey));
            let result = Rf_protect(Rf_eval(temp, (*dd).eventEnv));
            let sym_result = Rf_install(b"result\0".as_ptr() as *const c_char);
            defineVar(sym_result, result, (*dd).eventEnv);
            Rf_unprotect(3);
            R_FlushConsole();
        }
        Rf_unprotect(1); /* handler */
        (*dd).gettingEvent = 1;
    }
}

// ---------------------------------------------------------------------------
// doIdle - handle an idle event on a device
// ---------------------------------------------------------------------------

/// Process an idle event (background processing hook).
/// Copy-modified from doKeybd -- Frederick Eaton 12 Jun 2016
pub unsafe fn doIdle(dd: pDevDesc) {
    unsafe {
        (*dd).gettingEvent = 0; /* avoid recursive calls */

        let sym = Rf_install(idleHandler());
        let mut handler = Rf_protect(R_findVar(sym, (*dd).eventEnv));
        let _prot_count: c_int = 1;

        if TYPEOF(handler) == SEXPTYPE::PROMSXP.0 {
            handler = Rf_eval(handler, (*dd).eventEnv);
            Rf_unprotect(1);
            handler = Rf_protect(handler);
        }

        if TYPEOF(handler) == SEXPTYPE::CLOSXP.0 {
            let s_which = Rf_install(b"which\0".as_ptr() as *const c_char);
            defineVar(
                s_which,
                Rf_ScalarInteger(ndevNumber(dd) + 1),
                (*dd).eventEnv,
            );
            let temp = Rf_protect(lang1(handler));
            let result = Rf_protect(Rf_eval(temp, (*dd).eventEnv));
            let sym_result = Rf_install(b"result\0".as_ptr() as *const c_char);
            defineVar(sym_result, result, (*dd).eventEnv);
            Rf_unprotect(2);
            R_FlushConsole();
        }
        Rf_unprotect(1); /* handler */
        (*dd).gettingEvent = 1;
    }
}

// ---------------------------------------------------------------------------
// doesIdle - check if the device has an idle handler
// ---------------------------------------------------------------------------

/// Returns TRUE if the device has an idle handler set.
pub unsafe fn doesIdle(dd: pDevDesc) -> c_int {
    unsafe {
        let sym = Rf_install(idleHandler());
        let handler = R_findVar(sym, (*dd).eventEnv);
        if handler != R_UnboundValue() && handler != R_NilValue() {
            1
        } else {
            0
        }
    }
}
