
//! Port of R's `src/library/grDevices/src/devices.c`.
//!
//! Graphics device creation and listing:
//! `devcontrol`, `devdisplaylist`, `devcopy`, `devcur`, `devnext`,
//! `devprev`, `devset`, `devoff`, `devsize`, `devholdflush`,
//! `devcap`, `devcapture`.

use std::os::raw::{c_char, c_double, c_int, c_uint};
use std::ptr;

use crate::attrib_core::{R_ClassSymbol, R_DimSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asLogical};
use crate::main::colors::col2name;
use crate::main::errors::{Rf_error, Rf_warning};
use crate::sexp::accessors::{
    CAR, CDR, INTEGER, LENGTH, REAL, SET_STRING_ELT, SET_VECTOR_ELT, TYPEOF,
};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_allocVector, Rf_isNull, Rf_mkChar, Rf_mkString,
};
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

/* ==================== GE stub types ==================== */

/// Stub for pGEDevDesc (graphics engine device descriptor pointer).
pub type pGEDevDesc = *mut std::ffi::c_void;

/// Stub for pDevDesc (device descriptor pointer).
pub type pDevDesc = *mut std::ffi::c_void;

/* ==================== R_GE_capability constants ==================== */

pub const R_GE_capability_semiTransparency: i32 = 1;
pub const R_GE_capability_transparentBackground: i32 = 2;
pub const R_GE_capability_rasterImage: i32 = 3;
pub const R_GE_capability_capture: i32 = 4;
pub const R_GE_capability_locator: i32 = 5;
pub const R_GE_capability_events: i32 = 6;
pub const R_GE_capability_patterns: i32 = 7;
pub const R_GE_capability_clippingPaths: i32 = 8;
pub const R_GE_capability_masks: i32 = 9;
pub const R_GE_capability_compositing: i32 = 10;
pub const R_GE_capability_transformations: i32 = 11;
pub const R_GE_capability_paths: i32 = 12;
pub const R_GE_capability_glyphs: i32 = 13;
pub const R_GE_capability_variableFonts: i32 = 14;

/* ==================== R_GE device version constants ==================== */

pub const R_GE_group: c_int = 2;
pub const R_GE_glyphs: c_int = 14;
pub const R_GE_fontVar: c_int = 15;

/* ==================== GE stub functions ==================== */

/// Stub: get current graphics device. Returns null.
unsafe fn GEcurrentDevice() -> pGEDevDesc {
    ptr::null_mut()
}

/// Stub: initialize display list for device. No-op.
unsafe fn GEinitDisplayList(_gdd: pGEDevDesc) {
    // no-op
}

/// Stub: copy display list. No-op.
unsafe fn GEcopyDisplayList(_devnum: c_int) {
    // no-op
}

/// Stub: get current device number. Returns 0 (no device).
unsafe fn curDevice() -> c_int {
    0
}

/// Stub: get next device number. Returns 0.
unsafe fn nextDevice(_dev: c_int) -> c_int {
    0
}

/// Stub: get previous device number. Returns 0.
unsafe fn prevDevice(_dev: c_int) -> c_int {
    0
}

/// Stub: select device by number. Returns 0.
unsafe fn selectDevice(_dev: c_int) -> c_int {
    0
}

/// Stub: kill device by number. No-op.
unsafe fn killDevice(_dev: c_int) {
    // no-op
}

/// Stub: get device by number. Returns null.
unsafe fn GEgetDevice(_dev: c_int) -> pGEDevDesc {
    ptr::null_mut()
}

/// Stub: capture device raster. Returns R_NilValue (unsupported).
unsafe fn GECap(_gdd: pGEDevDesc) -> SEXP {
    R_NilValue()
}

/* ==================== Device functions ==================== */

/// Helper: check that the argument has positive length.
/// Equivalent to the `checkArity_length` macro in the C source.
unsafe fn checkArity_length(args: SEXP) -> SEXP {
    let args = CDR(args);
    if LENGTH(CAR(args)) == 0 {
        Rf_error(b"argument must have positive length\0".as_ptr() as *const c_char);
    }
    args
}

/// devcontrol(list) - enable/disable display list recording on current device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devcontrol(args: SEXP) -> SEXP {
    let mut args = args;
    let listFlag = {
        args = CDR(args);
        asLogical(CAR(args))
    };
    if listFlag == NA_LOGICAL {
        Rf_error(b"invalid argument\0".as_ptr() as *const c_char);
    }
    let _gdd = GEcurrentDevice();
    // Stub: cannot set displayListOn on void* gdd
    GEinitDisplayList(_gdd);
    Rf_ScalarLogical(listFlag)
}

/// devdisplaylist() - query display list recording status on current device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devdisplaylist(args: SEXP) -> SEXP {
    let _gdd = GEcurrentDevice();
    // Stub: cannot read displayListOn on void* gdd; return FALSE
    Rf_ScalarLogical(0)
}

/// devcopy(which) - copy display list from one device to another.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devcopy(args: SEXP) -> SEXP {
    let args = checkArity_length(args);
    let dev_num = *INTEGER(CAR(args)).add(0) - 1;
    GEcopyDisplayList(dev_num);
    R_NilValue()
}

/// dev.cur() - return the number of the current device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devcur(args: SEXP) -> SEXP {
    Rf_ScalarInteger(curDevice() + 1)
}

/// dev.next(which) - return the number of the next device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devnext(args: SEXP) -> SEXP {
    let args = checkArity_length(args);
    let nxt = *INTEGER(CAR(args)).add(0);
    if nxt == NA_INTEGER {
        Rf_error(b"NA argument is invalid\0".as_ptr() as *const c_char);
    }
    Rf_ScalarInteger(nextDevice(nxt - 1) + 1)
}

/// dev.prev(which) - return the number of the previous device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devprev(args: SEXP) -> SEXP {
    let args = checkArity_length(args);
    let prev = *INTEGER(CAR(args)).add(0);
    if prev == NA_INTEGER {
        Rf_error(b"NA argument is invalid\0".as_ptr() as *const c_char);
    }
    Rf_ScalarInteger(prevDevice(prev - 1) + 1)
}

/// dev.set(which) - set the specified device as the current device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devset(args: SEXP) -> SEXP {
    let args = checkArity_length(args);
    let dev_num = *INTEGER(CAR(args)).add(0);
    if dev_num == NA_INTEGER {
        Rf_error(b"NA argument is invalid\0".as_ptr() as *const c_char);
    }
    Rf_ScalarInteger(selectDevice(dev_num - 1) + 1)
}

/// dev.off(which) - shut down the specified device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devoff(args: SEXP) -> SEXP {
    let args = checkArity_length(args);
    let dev_num = *INTEGER(CAR(args)).add(0);
    // Check device number is valid (64 is max num devices)
    if dev_num > 0 && dev_num < 64 {
        let gdd = GEgetDevice(dev_num - 1);
        // Stub: cannot check gdd->lock on void*; skip unlock logic
        let _ = gdd;
    }
    killDevice(*INTEGER(CAR(args)).add(0) - 1);
    R_NilValue()
}

/// dev.size(units) - return the size of the current device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devsize(args: SEXP) -> SEXP {
    // Stub: GEcurrentDevice returns null, so we cannot call dd->size().
    // Return c(0, 0) as placeholder.
    let ans = Rf_allocVector(SEXPTYPE::REALSXP.0, 2);
    *REAL(ans).add(0) = 0.0;
    *REAL(ans).add(1) = 0.0;
    ans
}

/// dev.holdflush(level) - hold/flush device output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devholdflush(args: SEXP) -> SEXP {
    let mut args = args;
    args = CDR(args);
    let mut level = asInteger(CAR(args));
    // Stub: no device to call holdflush on
    if level == NA_INTEGER {
        level = 0;
    }
    Rf_ScalarInteger(level)
}

/// dev.capabilities() - query capabilities of the current device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devcap(args: SEXP) -> SEXP {
    let mut args = args;
    let capabilities;
    let trans;
    let transbg;
    let raster;
    let capture;
    let locator;
    let events;
    let patterns;
    let clippaths;
    let masks;
    let compositing;
    let transforms;
    let paths;
    let glyphs;
    let variableFonts;
    let _devcap: SEXP;

    // Stub: no current device, return capabilities list with conservative defaults

    args = CDR(args);
    capabilities = CAR(args);

    trans = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    *INTEGER(trans).add(0) = 0; // no transparency
    SET_VECTOR_ELT(
        capabilities,
        R_GE_capability_semiTransparency as R_xlen_t,
        trans,
    );
    Rf_unprotect(1);

    transbg = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    *INTEGER(transbg).add(0) = 0; // no transparent bg
    SET_VECTOR_ELT(
        capabilities,
        R_GE_capability_transparentBackground as R_xlen_t,
        transbg,
    );
    Rf_unprotect(1);

    raster = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    *INTEGER(raster).add(0) = 1; // conservative default
    SET_VECTOR_ELT(
        capabilities,
        R_GE_capability_rasterImage as R_xlen_t,
        raster,
    );
    Rf_unprotect(1);

    capture = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    *INTEGER(capture).add(0) = 1; // conservative default
    SET_VECTOR_ELT(capabilities, R_GE_capability_capture as R_xlen_t, capture);
    Rf_unprotect(1);

    locator = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    *INTEGER(locator).add(0) = 1; // conservative default
    SET_VECTOR_ELT(capabilities, R_GE_capability_locator as R_xlen_t, locator);
    Rf_unprotect(1);

    events = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 5));
    *INTEGER(events).add(0) = 0; // canGenMouseDown
    *INTEGER(events).add(1) = 0; // canGenMouseMove
    *INTEGER(events).add(2) = 0; // canGenMouseUp
    *INTEGER(events).add(3) = 0; // canGenKeybd
    *INTEGER(events).add(4) = 0; // canGenIdle
    SET_VECTOR_ELT(capabilities, R_GE_capability_events as R_xlen_t, events);
    Rf_unprotect(1);

    patterns = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    *INTEGER(patterns).add(0) = NA_INTEGER;
    SET_VECTOR_ELT(capabilities, R_GE_capability_patterns as R_xlen_t, patterns);
    Rf_unprotect(1);

    clippaths = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    *INTEGER(clippaths).add(0) = NA_INTEGER;
    SET_VECTOR_ELT(
        capabilities,
        R_GE_capability_clippingPaths as R_xlen_t,
        clippaths,
    );
    Rf_unprotect(1);

    masks = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    *INTEGER(masks).add(0) = NA_INTEGER;
    SET_VECTOR_ELT(capabilities, R_GE_capability_masks as R_xlen_t, masks);
    Rf_unprotect(1);

    // deviceVersion < R_GE_group (stub), so all 0
    compositing = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    transforms = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    paths = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    *INTEGER(compositing).add(0) = 0;
    *INTEGER(transforms).add(0) = 0;
    *INTEGER(paths).add(0) = 0;
    SET_VECTOR_ELT(
        capabilities,
        R_GE_capability_compositing as R_xlen_t,
        compositing,
    );
    SET_VECTOR_ELT(
        capabilities,
        R_GE_capability_transformations as R_xlen_t,
        transforms,
    );
    SET_VECTOR_ELT(capabilities, R_GE_capability_paths as R_xlen_t, paths);
    Rf_unprotect(3);

    glyphs = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    // deviceVersion < R_GE_glyphs (stub), so 0
    *INTEGER(glyphs).add(0) = 0;
    SET_VECTOR_ELT(capabilities, R_GE_capability_glyphs as R_xlen_t, glyphs);
    Rf_unprotect(1);

    variableFonts = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    // deviceVersion < R_GE_fontVar (stub), so 0
    *INTEGER(variableFonts).add(0) = 0;
    SET_VECTOR_ELT(
        capabilities,
        R_GE_capability_variableFonts as R_xlen_t,
        variableFonts,
    );
    Rf_unprotect(1);

    // Stub: no device->capabilities callback to invoke
    capabilities
}

/// dev.capture(native) - capture the current device contents as a raster.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn devcapture(args: SEXP) -> SEXP {
    let mut args = args;
    let _gdd = GEcurrentDevice();
    let mut raster;
    let mut native;

    args = CDR(args);
    native = asLogical(CAR(args));
    if native != 1 {
        native = 0;
    }

    raster = GECap(_gdd);
    // GECap returns R_NilValue when unsupported
    if Rf_isNull(raster) != 0 {
        return raster;
    }

    raster = Rf_protect(raster);
    if native != 0 {
        setAttrib(
            raster,
            R_ClassSymbol(),
            Rf_mkString(b"nativeRaster\0".as_ptr() as *const c_char),
        );
        Rf_unprotect(1);
        return raster;
    }

    // Non-native: convert to color strings (based on grid.cap logic)
    let size = LENGTH(raster);
    let dim_attr = getAttrib(raster, R_DimSymbol());
    let nrow = *INTEGER(dim_attr).add(0);
    let ncol = *INTEGER(dim_attr).add(1);

    let image = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, size as c_int));
    let rint = INTEGER(raster);
    let mut i: c_int = 0;
    while i < size {
        let col = (i % ncol) + 1;
        let row = (i / ncol) + 1;
        let idx = ((col - 1) * nrow + row - 1) as R_xlen_t;
        let name = col2name(*rint.add(i as usize) as c_uint);
        SET_STRING_ELT(image, idx, Rf_mkChar(name));
        i += 1;
    }

    let idim = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 2));
    *INTEGER(idim).add(0) = nrow;
    *INTEGER(idim).add(1) = ncol;
    setAttrib(image, R_DimSymbol(), idim);
    Rf_unprotect(3);

    image
}
