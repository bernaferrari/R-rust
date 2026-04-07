#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/main/devices.c
 *
 *  This is an extensive reworking by Paul Murrell of an original
 *  quick hack by Ross Ihaka designed to give a superset of the
 *  functionality in the AT&T Bell Laboratories GRZ library.
 *
 *  This should be regarded as part of the graphics engine.
 */

use std::cell::{Cell, RefCell};
use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use crate::eval::eval::Rf_eval;
use crate::main::colors::R_GE_str2col;
use crate::main::engine::{
    DevDesc, GEDevDesc, MAX_GRAPHICS_SYSTEMS, R_GE_version, pDevDesc, pGEDevDesc, pGEcontext,
};
use crate::main::errors::Rf_error;
use crate::main::errors::Rf_warning;
use crate::main::options::Rf_GetOptionDeviceAsk;
use crate::sexp::accessors::{CDR, LENGTH, SETCAR, SETCDR, STRING_ELT, TYPEOF, VECTOR_ELT};
use crate::sexp::attrib_core::Rf_setAttrib;
use crate::sexp::constructors::{Rf_cons, Rf_mkString};
use crate::sexp::envir::{R_findVar, R_findVarInFrame, defineVar};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_BaseEnv, R_GlobalEnv, R_NilValue, R_UnboundValue};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of graphics devices.
/// Defined in Defn.h as 64.
pub const R_MaxDevices: c_int = 64;

/// FALSE value for Rboolean
const FALSE: c_int = 0;
/// TRUE value for Rboolean
const TRUE: c_int = 1;

// ---------------------------------------------------------------------------
// Static state
// ---------------------------------------------------------------------------

/// Index returned by GEregisterSystem for the base system.
thread_local! { pub static baseRegisterIndex: Cell<c_int> = Cell::new(-1); }

/// Index of the current device (0 = null device).
thread_local! { static R_CurrentDevice: Cell<c_int> = Cell::new(0); }

/// Number of active devices (including null device at slot 0).
thread_local! { static R_NumDevices: Cell<c_int> = Cell::new(1); }

/// Device array. Slot 0 is the null device, slot 63 is a sentinel.
thread_local! { static R_Devices: RefCell<[pGEDevDesc; R_MaxDevices as usize]> = RefCell::new([ptr::null_mut(); 64]); }

/// Whether each slot is active.
thread_local! { static active: RefCell<[c_int; R_MaxDevices as usize]> = RefCell::new([0; 64]); }

/// Dummy null device description (never dereferenced for real ops).
thread_local! { static nullDevice: RefCell<GEDevDesc> = RefCell::new(GEDevDesc {
    dev: ptr::null_mut(),
    displayListOn: 0,
    displayList: ptr::null_mut(),
    DLlastElt: ptr::null_mut(),
    savedSnapshot: ptr::null_mut(),
    dirty: 0,
    recordGraphics: 0,
    lock: 0,
    gesd: [ptr::null_mut(); MAX_GRAPHICS_SYSTEMS as usize],
    ask: 0,
    appending: 0,
}); }

#[inline(always)]
unsafe fn get_current_device_index() -> c_int {
    R_CurrentDevice.with(|v| v.get())
}
#[inline(always)]
unsafe fn set_current_device_index(v: c_int) {
    R_CurrentDevice.with(|c| c.set(v));
}
#[inline(always)]
unsafe fn get_num_devices() -> c_int {
    R_NumDevices.with(|v| v.get())
}
#[inline(always)]
unsafe fn set_num_devices(v: c_int) {
    R_NumDevices.with(|c| c.set(v));
}
#[inline(always)]
unsafe fn get_device_slot(i: c_int) -> pGEDevDesc {
    R_Devices.with(|v| v.borrow()[i as usize])
}
#[inline(always)]
unsafe fn set_device_slot(i: c_int, dev: pGEDevDesc) {
    R_Devices.with(|v| v.borrow_mut()[i as usize] = dev);
}
#[inline(always)]
unsafe fn get_active_slot(i: c_int) -> c_int {
    active.with(|v| v.borrow()[i as usize])
}
#[inline(always)]
unsafe fn set_active_slot(i: c_int, val: c_int) {
    active.with(|v| v.borrow_mut()[i as usize] = val);
}

// ---------------------------------------------------------------------------
// Helper: getSymbolValue
// ---------------------------------------------------------------------------

unsafe fn getSymbolValue(symbol: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(symbol) != SEXPTYPE::SYMSXP.0 {
            Rf_error(b"argument to \'getSymbolValue\' is not a symbol\0".as_ptr() as *const c_char);
            return ptr::null_mut();
        }
        R_findVar(symbol, R_BaseEnv())
    }
}

// ---------------------------------------------------------------------------
// Helper: R_DeviceSymbol / R_DevicesSymbol (lazily installed)
// ---------------------------------------------------------------------------

/// Return the SEXP for the ".Device" symbol.
unsafe fn R_DeviceSymbol() -> SEXP {
    unsafe { Rf_install(b".Device\0".as_ptr() as *const c_char) }
}

/// Return the SEXP for the ".Devices" symbol.
unsafe fn R_DevicesSymbol() -> SEXP {
    unsafe { Rf_install(b".Devices\0".as_ptr() as *const c_char) }
}

// ---------------------------------------------------------------------------
// Device query functions
// ---------------------------------------------------------------------------

/// Returns true if there are no active (non-null) devices.
/// Used in grid.
pub unsafe fn NoDevices() -> c_int {
    unsafe {
        if get_num_devices() == 1 || get_current_device_index() == 0 {
            TRUE
        } else {
            FALSE
        }
    }
}

/// Returns the number of devices (including null device).
pub unsafe fn NumDevices() -> c_int {
    unsafe { get_num_devices() }
}

/// Get the current device descriptor. If there are no active devices,
/// try to start the default device from options.
pub unsafe fn GEcurrentDevice() -> pGEDevDesc {
    unsafe {
        if NoDevices() != 0 {
            let device_sym = Rf_install(b"device\0".as_ptr() as *const c_char);
            let mut defdev = GetOption1(device_sym);
            let t = TYPEOF(defdev);
            // isString check: STRSXP
            if t == SEXPTYPE::STRSXP.0 && LENGTH(defdev) > 0 {
                let devName = installTrChar(STRING_ELT(defdev, 0));
                // Try global env first
                defdev = R_findVar(devName, R_GlobalEnv());
                if defdev != R_UnboundValue() {
                    let call = Rf_protect(Rf_cons(devName, R_NilValue()));
                    let _ = Rf_eval(call, R_GlobalEnv());
                    Rf_unprotect(1);
                } else {
                    // Try grDevices namespace
                    let ns_sym = Rf_install(b"grDevices\0".as_ptr() as *const c_char);
                    let mut ns = R_findVarInFrame(R_NamespaceRegistry_stub(), ns_sym);
                    ns = Rf_protect(ns);
                    if ns != R_UnboundValue() && R_findVar(devName, ns) != R_UnboundValue() {
                        let call = Rf_protect(Rf_cons(devName, R_NilValue()));
                        let _ = Rf_eval(call, ns);
                        Rf_unprotect(1);
                    } else {
                        Rf_error(b"no active or default device\0".as_ptr() as *const c_char);
                    }
                    Rf_unprotect(1);
                }
            } else if t == SEXPTYPE::CLOSXP.0 {
                let call = Rf_protect(Rf_cons(defdev, R_NilValue()));
                let _ = Rf_eval(call, R_GlobalEnv());
                Rf_unprotect(1);
            } else {
                Rf_error(b"no active or default device\0".as_ptr() as *const c_char);
            }
            if NoDevices() != 0 {
                Rf_error(
                    b"no active device and default getOption(\"device\") is invalid\0".as_ptr()
                        as *const c_char,
                );
            }
        }
        get_device_slot(get_current_device_index())
    }
}

/// Get device by index.
pub unsafe fn GEgetDevice(i: c_int) -> pGEDevDesc {
    unsafe { get_device_slot(i) }
}

/// Get the current device number.
pub unsafe fn curDevice() -> c_int {
    unsafe { get_current_device_index() }
}

// ---------------------------------------------------------------------------
// nextDevice / prevDevice
// ---------------------------------------------------------------------------

/// Find the next active device after `from`.
pub unsafe fn nextDevice(from: c_int) -> c_int {
    unsafe {
        if get_num_devices() == 1 {
            return 0;
        }
        let mut i = from;
        let mut nextDev: c_int = 0;
        while i < (R_MaxDevices - 1) && nextDev == 0 {
            i += 1;
            if get_active_slot(i) != 0 {
                nextDev = i;
            }
        }
        if nextDev == 0 {
            i = 0;
            while i < (R_MaxDevices - 1) && nextDev == 0 {
                i += 1;
                if get_active_slot(i) != 0 {
                    nextDev = i;
                }
            }
        }
        nextDev
    }
}

/// Find the previous active device before `from`.
pub unsafe fn prevDevice(from: c_int) -> c_int {
    unsafe {
        if get_num_devices() == 1 {
            return 0;
        }
        let mut i = from;
        let mut prevDev: c_int = 0;
        if i < R_MaxDevices {
            while i > 1 && prevDev == 0 {
                i -= 1;
                if get_active_slot(i) != 0 {
                    prevDev = i;
                }
            }
        }
        if prevDev == 0 {
            i = R_MaxDevices;
            while i > 1 && prevDev == 0 {
                i -= 1;
                if get_active_slot(i) != 0 {
                    prevDev = i;
                }
            }
        }
        prevDev
    }
}

// ---------------------------------------------------------------------------
// GEdeviceNumber / ndevNumber
// ---------------------------------------------------------------------------

/// Find device number given a pGEDevDesc.
pub unsafe fn GEdeviceNumber(dd: pGEDevDesc) -> c_int {
    unsafe {
        let mut i: c_int = 1;
        while i < R_MaxDevices {
            if *ptr::addr_of_mut!(R_Devices)
                .cast::<pGEDevDesc>()
                .add(i as usize)
                == dd
            {
                return i;
            }
            i += 1;
        }
        0
    }
}

/// Find device number given a pDevDesc.
pub unsafe fn ndevNumber(dd: pDevDesc) -> c_int {
    unsafe {
        let mut i: c_int = 1;
        while i < R_MaxDevices {
            let gdd = *ptr::addr_of_mut!(R_Devices)
                .cast::<pGEDevDesc>()
                .add(i as usize);
            if !gdd.is_null() && (*gdd).dev == dd {
                return i;
            }
            i += 1;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// selectDevice
// ---------------------------------------------------------------------------

/// Select a device as the current device.
pub unsafe fn selectDevice(devNum: c_int) -> c_int {
    unsafe {
        if devNum >= 0 && devNum < R_MaxDevices {
            let gdd_slot = *ptr::addr_of_mut!(R_Devices)
                .cast::<pGEDevDesc>()
                .add(devNum as usize);
            if !gdd_slot.is_null()
                && *ptr::addr_of_mut!(active)
                    .cast::<c_int>()
                    .add(devNum as usize)
                    != 0
            {
                // Deactivate old device
                if NoDevices() == 0 {
                    let oldd = GEcurrentDevice();
                    if !oldd.is_null() && !(*oldd).dev.is_null() {
                        if let Some(deactivate_fn) = (*(*oldd).dev).deactivate {
                            deactivate_fn((*oldd).dev);
                        }
                    }
                }

                R_CurrentDevice = devNum;

                // maintain .Device
                defineVar(
                    R_DeviceSymbol(),
                    VECTOR_ELT(getSymbolValue(R_DevicesSymbol()), devNum as i64),
                    R_BaseEnv(),
                );

                let gdd = GEcurrentDevice();
                if NoDevices() == 0 {
                    if !gdd.is_null() && !(*gdd).dev.is_null() {
                        if let Some(activate_fn) = (*(*gdd).dev).activate {
                            activate_fn((*gdd).dev);
                        }
                    }
                }
                return devNum;
            }
        }
        selectDevice(nextDevice(devNum))
    }
}

// ---------------------------------------------------------------------------
// removeDevice (internal)
// ---------------------------------------------------------------------------

/// Remove a device. `findNext` should be FALSE only when shutting down.
unsafe fn removeDevice(devNum: c_int, findNext: c_int) {
    unsafe {
        if devNum > 0 && devNum < R_MaxDevices {
            let gdd_slot = *ptr::addr_of_mut!(R_Devices)
                .cast::<pGEDevDesc>()
                .add(devNum as usize);
            if !gdd_slot.is_null()
                && *ptr::addr_of_mut!(active)
                    .cast::<c_int>()
                    .add(devNum as usize)
                    != 0
            {
                let g = gdd_slot;

                ptr::write(
                    ptr::addr_of_mut!(active)
                        .cast::<c_int>()
                        .add(devNum as usize),
                    0,
                );
                R_NumDevices -= 1;

                if findNext != 0 {
                    // maintain .Devices
                    let s = Rf_protect(getSymbolValue(R_DevicesSymbol()));
                    let mut si = s;
                    let mut i: c_int = 0;
                    while i < devNum {
                        si = CDR(si);
                        i += 1;
                    }
                    SETCAR(si, Rf_mkString(b"\0".as_ptr() as *const c_char));
                    Rf_unprotect(1);

                    // determine new current device
                    if devNum == R_CurrentDevice {
                        R_CurrentDevice = nextDevice(R_CurrentDevice);
                        // maintain .Device
                        defineVar(
                            R_DeviceSymbol(),
                            VECTOR_ELT(getSymbolValue(R_DevicesSymbol()), R_CurrentDevice as i64),
                            R_BaseEnv(),
                        );

                        // activate new current device
                        if R_CurrentDevice != 0 {
                            let new_gdd = GEcurrentDevice();
                            if !new_gdd.is_null() && !(*new_gdd).dev.is_null() {
                                if let Some(activate_fn) = (*(*new_gdd).dev).activate {
                                    activate_fn((*new_gdd).dev);
                                }
                            }
                        }
                    }
                }

                // Close the device
                if !g.is_null() && !(*g).dev.is_null() {
                    if let Some(close_fn) = (*(*g).dev).close {
                        close_fn((*g).dev);
                    }
                }

                // Free the GE device descriptor
                crate::main::engine::GEdestroyDevDesc(g as *mut c_void);
                ptr::write(
                    ptr::addr_of_mut!(R_Devices)
                        .cast::<pGEDevDesc>()
                        .add(devNum as usize),
                    ptr::null_mut(),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GEkillDevice / killDevice
// ---------------------------------------------------------------------------

/// Kill a device given its GEDevDesc pointer.
pub unsafe fn GEkillDevice(gdd: pGEDevDesc) {
    unsafe {
        let lock: c_int = if !gdd.is_null() { (*gdd).lock } else { FALSE };
        if lock != 0 {
            Rf_warning(b"can\'t shut down a locked device\0".as_ptr() as *const c_char);
            return;
        }
        removeDevice(GEdeviceNumber(gdd), TRUE);
    }
}

/// Kill a device given its device number.
pub unsafe fn killDevice(devNum: c_int) {
    unsafe {
        if devNum > 0 && devNum < R_MaxDevices {
            let gdd_slot = *ptr::addr_of_mut!(R_Devices)
                .cast::<pGEDevDesc>()
                .add(devNum as usize);
            if !gdd_slot.is_null()
                && *ptr::addr_of_mut!(active)
                    .cast::<c_int>()
                    .add(devNum as usize)
                    != 0
            {
                if !gdd_slot.is_null() && (*gdd_slot).lock != 0 {
                    Rf_warning(b"can\'t shut down a locked device\0".as_ptr() as *const c_char);
                    return;
                }
            }
        }
        removeDevice(devNum, TRUE);
    }
}

// ---------------------------------------------------------------------------
// KillAllDevices
// ---------------------------------------------------------------------------

/// Shut down all graphics devices at end of session.
pub unsafe fn KillAllDevices() {
    unsafe {
        let mut i: c_int = R_MaxDevices - 1;
        while i > 0 {
            removeDevice(i, FALSE);
            i -= 1;
        }
        R_CurrentDevice = 0; // the null device

        if *ptr::addr_of_mut!(baseRegisterIndex) != -1 {
            crate::main::engine::GEunregisterSystem(*ptr::addr_of_mut!(baseRegisterIndex));
            *ptr::addr_of_mut!(baseRegisterIndex) = -1;
        }
    }
}

// ---------------------------------------------------------------------------
// desc2GEDesc
// ---------------------------------------------------------------------------

/// A common construction in some graphics devices:
/// map a pDevDesc to its corresponding pGEDevDesc.
pub unsafe fn desc2GEDesc(dd: pDevDesc) -> pGEDevDesc {
    unsafe {
        let mut i: c_int = 1;
        while i < R_MaxDevices {
            let gdd = *ptr::addr_of_mut!(R_Devices)
                .cast::<pGEDevDesc>()
                .add(i as usize);
            if !gdd.is_null() && (*gdd).dev == dd {
                return gdd;
            }
            i += 1;
        }
        // shouldn't happen ... return null device slot
        *ptr::addr_of_mut!(R_Devices).cast::<pGEDevDesc>().add(0)
    }
}

// ---------------------------------------------------------------------------
// Noop / default device callbacks
// ---------------------------------------------------------------------------

unsafe extern "C" fn noopCircle(
    _x: c_double,
    _y: c_double,
    _r: c_double,
    _gc: pGEcontext,
    _dd: pDevDesc,
) {
}

unsafe extern "C" fn noopClip(
    _x0: c_double,
    _x1: c_double,
    _y0: c_double,
    _y1: c_double,
    _dd: pDevDesc,
) {
}

unsafe extern "C" fn noopClose(_dd: pDevDesc) {}

unsafe extern "C" fn noopLine(
    _x1: c_double,
    _y1: c_double,
    _x2: c_double,
    _y2: c_double,
    _gc: pGEcontext,
    _dd: pDevDesc,
) {
}

unsafe extern "C" fn defaultMetricInfo(
    _c: c_int,
    _gc: pGEcontext,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: pDevDesc,
) {
    unsafe {
        if !ascent.is_null() {
            *ascent = (*dd).cra[1];
        }
        if !descent.is_null() {
            *descent = 0.0;
        }
        if !width.is_null() {
            *width = (*dd).cra[0];
        }
    }
}

unsafe extern "C" fn noopNewPage(_gc: pGEcontext, _dd: pDevDesc) {}

unsafe extern "C" fn noopPolygon(
    _n: c_int,
    _x: *const c_double,
    _y: *const c_double,
    _gc: pGEcontext,
    _dd: pDevDesc,
) {
}

unsafe extern "C" fn noopPolyline(
    _n: c_int,
    _x: *const c_double,
    _y: *const c_double,
    _gc: pGEcontext,
    _dd: pDevDesc,
) {
}

unsafe extern "C" fn noopRect(
    _x0: c_double,
    _y0: c_double,
    _x1: c_double,
    _y1: c_double,
    _gc: pGEcontext,
    _dd: pDevDesc,
) {
}

unsafe extern "C" fn defaultStrWidth(
    _str: *const c_char,
    _gc: pGEcontext,
    _dd: pDevDesc,
) -> c_double {
    0.0
}

unsafe extern "C" fn noopText(
    _x: c_double,
    _y: c_double,
    _str: *const c_char,
    _rot: c_double,
    _hadj: c_double,
    _gc: pGEcontext,
    _dd: pDevDesc,
) {
}

unsafe extern "C" fn defaultGetEvent(_eventRho: SEXP, _prompt: *const c_char) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe extern "C" fn noopTextUTF8(
    _x: c_double,
    _y: c_double,
    _str: *const c_char,
    _rot: c_double,
    _hadj: c_double,
    _gc: pGEcontext,
    _dd: pDevDesc,
) {
}

unsafe extern "C" fn defaultStrWidthUTF8(
    _str: *const c_char,
    _gc: pGEcontext,
    _dd: pDevDesc,
) -> c_double {
    0.0
}

unsafe extern "C" fn defaultSetPattern(_pattern: SEXP, _dd: pDevDesc) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe extern "C" fn noopReleasePattern(_ref: SEXP, _dd: pDevDesc) {}

unsafe extern "C" fn defaultSetClipPath(_path: SEXP, _ref: SEXP, _dd: pDevDesc) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe extern "C" fn noopReleaseClipPath(_ref: SEXP, _dd: pDevDesc) {}

unsafe extern "C" fn defaultSetMask(_path: SEXP, _ref: SEXP, _dd: pDevDesc) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe extern "C" fn noopReleaseMask(_ref: SEXP, _dd: pDevDesc) {}

unsafe extern "C" fn defaultDefineGroup(
    _source: SEXP,
    _op: c_int,
    _destination: SEXP,
    _dd: pDevDesc,
) -> SEXP {
    unsafe { R_NilValue() }
}

unsafe extern "C" fn noopUseGroup(_ref: SEXP, _trans: SEXP, _dd: pDevDesc) {}

unsafe extern "C" fn noopReleaseGroup(_ref: SEXP, _dd: pDevDesc) {}

unsafe extern "C" fn noopStroke(_path: SEXP, _gc: pGEcontext, _dd: pDevDesc) {}

unsafe extern "C" fn noopFill(_path: SEXP, _rule: c_int, _gc: pGEcontext, _dd: pDevDesc) {}

unsafe extern "C" fn noopFillStroke(_path: SEXP, _rule: c_int, _gc: pGEcontext, _dd: pDevDesc) {}

unsafe extern "C" fn defaultCapabilities(cap: SEXP) -> SEXP {
    cap
}

unsafe extern "C" fn noopGlyph(
    _n: c_int,
    _glyphs: *const c_int,
    _x: *const c_double,
    _y: *const c_double,
    _font: SEXP,
    _size: c_double,
    _colour: c_int,
    _rot: c_double,
    _dd: pDevDesc,
) {
}

// ---------------------------------------------------------------------------
// GEcreateDD / GEfreeDD
// ---------------------------------------------------------------------------

/// Allocate and initialise a DevDesc with defaults.
pub unsafe fn GEcreateDD() -> pDevDesc {
    unsafe {
        let dd = libc::calloc(1, std::mem::size_of::<DevDesc>()) as pDevDesc;
        if dd.is_null() {
            return ptr::null_mut();
        }

        // 1inch device with 100dpi and 10pt text
        (*dd).left = 0.0;
        (*dd).right = 100.0;
        (*dd).bottom = 0.0;
        (*dd).top = 100.0;
        (*dd).clipLeft = 0.0;
        (*dd).clipRight = 100.0;
        (*dd).clipBottom = 0.0;
        (*dd).clipTop = 100.0;
        (*dd).xCharOffset = 0.0;
        (*dd).yCharOffset = 0.0;
        (*dd).yLineBias = 0.0;
        (*dd).ipr[0] = 1.0 / 100.0;
        (*dd).ipr[1] = 1.0 / 100.0;
        (*dd).cra[0] = 0.6 * 10.0 * 100.0 / 72.0;
        (*dd).cra[1] = 1.0 * 10.0 * 100.0 / 72.0;
        (*dd).gamma = 1.0;
        (*dd).canClip = FALSE;
        (*dd).canChangeGamma = FALSE;
        (*dd).canHAdj = 0;
        (*dd).startps = 10.0;
        (*dd).startcol = R_GE_str2col(b"black\0".as_ptr() as *const c_char) as c_int;
        (*dd).startfill = R_GE_str2col(b"transparent\0".as_ptr() as *const c_char) as c_int;
        (*dd).startlty = 0;
        (*dd).startfont = 1;
        (*dd).startgamma = 1.0;
        (*dd).deviceSpecific = ptr::null_mut();
        (*dd).displayListOn = FALSE;
        (*dd).canGenMouseDown = FALSE;
        (*dd).canGenMouseMove = FALSE;
        (*dd).canGenMouseUp = FALSE;
        (*dd).canGenKeybd = FALSE;
        (*dd).canGenIdle = FALSE;
        (*dd).gettingEvent = FALSE;
        (*dd).activate = None;
        (*dd).circle = Some(noopCircle);
        (*dd).clip = Some(noopClip);
        (*dd).close = Some(noopClose);
        (*dd).deactivate = None;
        (*dd).locator = None;
        (*dd).line = Some(noopLine);
        (*dd).metricInfo = Some(defaultMetricInfo);
        (*dd).mode = None;
        (*dd).newPage = Some(noopNewPage);
        (*dd).polygon = Some(noopPolygon);
        (*dd).polyline = Some(noopPolyline);
        (*dd).rect = Some(noopRect);
        (*dd).path = None;
        (*dd).raster = None;
        (*dd).cap = None;
        (*dd).size = None;
        (*dd).strWidth = Some(defaultStrWidth);
        (*dd).text = Some(noopText);
        (*dd).onExit = None;
        (*dd).getEvent = Some(defaultGetEvent);
        (*dd).newFrameConfirm = None;
        (*dd).hasTextUTF8 = FALSE;
        (*dd).textUTF8 = Some(noopTextUTF8);
        (*dd).strWidthUTF8 = Some(defaultStrWidthUTF8);
        (*dd).wantSymbolUTF8 = FALSE;
        (*dd).useRotatedTextInContour = FALSE;
        (*dd).eventEnv = ptr::null_mut();
        (*dd).eventHelper = None;
        (*dd).holdflush = None;
        (*dd).haveTransparency = 1;
        (*dd).haveTransparentBg = 1;
        (*dd).haveRaster = 1;
        (*dd).haveCapture = 1;
        (*dd).haveLocator = 1;
        (*dd).setPattern = Some(defaultSetPattern);
        (*dd).releasePattern = Some(noopReleasePattern);
        (*dd).setClipPath = Some(defaultSetClipPath);
        (*dd).releaseClipPath = Some(noopReleaseClipPath);
        (*dd).setMask = Some(defaultSetMask);
        (*dd).releaseMask = Some(noopReleaseMask);
        (*dd).deviceVersion = R_GE_version;
        (*dd).deviceClip = FALSE;
        (*dd).defineGroup = Some(defaultDefineGroup);
        (*dd).useGroup = Some(noopUseGroup);
        (*dd).releaseGroup = Some(noopReleaseGroup);
        (*dd).stroke = Some(noopStroke);
        (*dd).fill = Some(noopFill);
        (*dd).fillStroke = Some(noopFillStroke);
        (*dd).capabilities = Some(defaultCapabilities);
        (*dd).glyph = Some(noopGlyph);

        dd
    }
}

/// Free a DevDesc allocated by GEcreateDD.
pub unsafe fn GEfreeDD(dd: pDevDesc) {
    unsafe {
        if !dd.is_null() {
            libc::free(dd as *mut c_void);
        }
    }
}

// ---------------------------------------------------------------------------
// R_CheckDeviceAvailable / R_CheckDeviceAvailableBool
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_CheckDeviceAvailable() {
    unsafe {
        if R_NumDevices >= R_MaxDevices - 1 {
            Rf_error(b"too many open devices\0".as_ptr() as *const c_char);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_CheckDeviceAvailableBool() -> c_int {
    unsafe {
        if R_NumDevices >= R_MaxDevices - 1 {
            FALSE
        } else {
            TRUE
        }
    }
}

// ---------------------------------------------------------------------------
// GEaddDevice
// ---------------------------------------------------------------------------

pub unsafe fn GEaddDevice(gdd: pGEDevDesc) {
    unsafe {
        let mut i: c_int = 1;
        let mut appnd: c_int = FALSE;

        let s = Rf_protect(getSymbolValue(R_DevicesSymbol()));

        if NoDevices() == 0 {
            let oldd = GEcurrentDevice();
            if !oldd.is_null() && !(*oldd).dev.is_null() {
                if let Some(deactivate_fn) = (*(*oldd).dev).deactivate {
                    deactivate_fn((*oldd).dev);
                }
            }
        }

        // find empty slot for new descriptor
        if CDR(s) == R_NilValue() {
            appnd = TRUE;
        }

        while *ptr::addr_of_mut!(R_Devices)
            .cast::<pGEDevDesc>()
            .add(i as usize)
            != ptr::null_mut()
        {
            i += 1;
        }

        // Walk the list to find where to insert
        let mut list_ptr = getSymbolValue(R_DevicesSymbol());
        let mut j: c_int = 0;
        while j < i {
            if CDR(list_ptr) == R_NilValue() {
                appnd = TRUE;
            } else {
                list_ptr = CDR(list_ptr);
            }
            j += 1;
        }

        R_CurrentDevice = i;
        R_NumDevices += 1;
        ptr::write(
            ptr::addr_of_mut!(R_Devices)
                .cast::<pGEDevDesc>()
                .add(i as usize),
            gdd,
        );
        ptr::write(
            ptr::addr_of_mut!(active).cast::<c_int>().add(i as usize),
            TRUE,
        );

        crate::main::engine::GEregisterWithDevice(gdd as *mut c_void);
        if !gdd.is_null() && !(*gdd).dev.is_null() {
            if let Some(activate_fn) = (*(*gdd).dev).activate {
                activate_fn((*gdd).dev);
            }
        }

        // maintain .Devices (.Device has already been set)
        let t = Rf_protect(crate::main::duplicate::Rf_duplicate(getSymbolValue(
            R_DeviceSymbol(),
        )));
        if appnd != 0 {
            SETCDR(list_ptr, Rf_cons(t, R_NilValue()));
        } else {
            SETCAR(list_ptr, t);
        }

        Rf_unprotect(2);

        // Sentinel check: if driver didn't call R_CheckDeviceAvailable
        if i == R_MaxDevices - 1 {
            killDevice(i);
            Rf_error(b"too many open devices\0".as_ptr() as *const c_char);
        }
    }
}

// ---------------------------------------------------------------------------
// GEaddDevice2 / GEaddDevice2f
// ---------------------------------------------------------------------------

pub unsafe fn GEaddDevice2(gdd: pGEDevDesc, name: *const c_char) {
    unsafe {
        defineVar(R_DeviceSymbol(), Rf_mkString(name), R_BaseEnv());
        GEaddDevice(gdd);
        crate::main::engine::GEinitDisplayList(gdd as *mut c_void);
    }
}

pub unsafe fn GEaddDevice2f(gdd: pGEDevDesc, name: *const c_char, file: *const c_char) {
    unsafe {
        let f = Rf_protect(Rf_mkString(name));
        if !file.is_null() {
            let s_filepath = Rf_install(b"filepath\0".as_ptr() as *const c_char);
            Rf_setAttrib(f, s_filepath, Rf_mkString(file));
        }
        defineVar(R_DeviceSymbol(), f, R_BaseEnv());
        Rf_unprotect(1);
        GEaddDevice(gdd);
        crate::main::engine::GEinitDisplayList(gdd as *mut c_void);
    }
}

// ---------------------------------------------------------------------------
// GEcreateDevDesc
// ---------------------------------------------------------------------------

/// Create a GEDevDesc wrapping a pDevDesc.
pub unsafe fn GEcreateDevDesc(dev: pDevDesc) -> pGEDevDesc {
    unsafe {
        let gdd = libc::calloc(1, std::mem::size_of::<GEDevDesc>()) as pGEDevDesc;
        if gdd.is_null() {
            Rf_error(
                b"not enough memory to allocate device (in GEcreateDevDesc)\0".as_ptr()
                    as *const c_char,
            );
        }
        // NULL the gesd array
        for i in 0..MAX_GRAPHICS_SYSTEMS as usize {
            (*gdd).gesd[i] = ptr::null_mut();
        }
        (*gdd).dev = dev;
        (*gdd).displayListOn = if !dev.is_null() {
            (*dev).displayListOn
        } else {
            FALSE
        };
        (*gdd).displayList = R_NilValue();
        (*gdd).DLlastElt = R_NilValue();
        (*gdd).savedSnapshot = R_NilValue();
        (*gdd).dirty = FALSE;
        (*gdd).recordGraphics = TRUE;
        (*gdd).lock = FALSE;
        (*gdd).ask = Rf_GetOptionDeviceAsk();
        if !dev.is_null() {
            (*dev).eventEnv = R_NilValue();
        }
        (*gdd).appending = FALSE;
        gdd
    }
}

// ---------------------------------------------------------------------------
// InitGraphics
// ---------------------------------------------------------------------------

/// Initialise the graphics device system. Called at R startup.
pub unsafe fn InitGraphics() {
    unsafe {
        ptr::write(
            ptr::addr_of_mut!(R_Devices).cast::<pGEDevDesc>().add(0),
            ptr::addr_of_mut!(nullDevice) as pGEDevDesc,
        );
        ptr::write(ptr::addr_of_mut!(active).cast::<c_int>().add(0), TRUE);
        let mut i: usize = 1;
        while i < R_MaxDevices as usize {
            ptr::write(
                ptr::addr_of_mut!(R_Devices).cast::<pGEDevDesc>().add(i),
                ptr::null_mut(),
            );
            ptr::write(ptr::addr_of_mut!(active).cast::<c_int>().add(i), FALSE);
            i += 1;
        }

        // init .Device and .Devices
        let s1 = Rf_protect(Rf_mkString(b"null device\0".as_ptr() as *const c_char));
        defineVar(R_DeviceSymbol(), s1, R_BaseEnv());
        let s2 = Rf_protect(Rf_mkString(b"null device\0".as_ptr() as *const c_char));
        defineVar(R_DevicesSymbol(), Rf_cons(s2, R_NilValue()), R_BaseEnv());
        Rf_unprotect(2);
    }
}

// ---------------------------------------------------------------------------
// NewFrameConfirm
// ---------------------------------------------------------------------------

/// Prompt the user to confirm a new frame (in interactive mode).
pub unsafe fn NewFrameConfirm(dd: pDevDesc) {
    unsafe {
        use crate::main::main::R_Interactive;
        if R_Interactive() == 0 {
            return;
        }
        if !dd.is_null() {
            if let Some(confirm_fn) = (*dd).newFrameConfirm {
                if confirm_fn(dd) != 0 {
                    return;
                }
            }
        }
        let mut buf: [u8; 1024] = [0; 1024];
        R_ReadConsole_stub(
            b"Hit <Return> to see next plot: \0".as_ptr() as *const c_char,
            buf.as_mut_ptr() as *mut c_char,
            1024,
            0,
        );
    }
}

// ---------------------------------------------------------------------------
// Stubs for symbols/functions not yet fully ported
// ---------------------------------------------------------------------------

/// Stub for GetOption1 -- forward to the real implementation.
unsafe fn GetOption1(tag: SEXP) -> SEXP {
    unsafe { crate::main::options::GetOption1(tag) }
}

/// Stub for installTrChar.
unsafe fn installTrChar(x: SEXP) -> SEXP {
    unsafe { crate::main::sysutils::installTrChar(x) }
}

/// Stub for R_NamespaceRegistry (not yet fully implemented).
unsafe fn R_NamespaceRegistry_stub() -> SEXP {
    unsafe {
        // In the full R implementation, this returns the namespace registry
        // environment. For now, return R_NilValue as a safe stub.
        R_NilValue()
    }
}

/// Stub for R_ReadConsole.
unsafe fn R_ReadConsole_stub(
    _prompt: *const c_char,
    _buf: *mut c_char,
    _buflen: c_int,
    _addtohistory: c_int,
) -> c_int {
    0
}
