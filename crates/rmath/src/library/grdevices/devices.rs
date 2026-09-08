//! Port of R's `src/library/grDevices/src/devices.c`.
//!
//! Graphics device creation and listing:
//! `devcontrol`, `devdisplaylist`, `devcopy`, `devcur`, `devnext`,
//! `devprev`, `devset`, `devoff`, `devsize`, `devholdflush`,
//! `devcap`, `devcapture`.

use std::os::raw::{c_char, c_double, c_int, c_uint};

use crate::attrib_core::{R_ClassSymbol, R_DimSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asLogical};
use crate::main::colors::col2name;
use crate::main::errors::{Rf_error, Rf_warning};
use crate::sexp::accessors::{
    CAR, CDR, CHAR, INTEGER, LENGTH, REAL, SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT, TYPEOF,
};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_allocVector, Rf_isNull, Rf_mkChar, Rf_mkString,
};
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

use super::device_registry;

/* ==================== GE stub types ==================== */

/// Stub for pGEDevDesc (graphics engine device descriptor pointer).
pub type pGEDevDesc = device_registry::pGEDevDesc;

/// Stub for pDevDesc (device descriptor pointer).
pub type pDevDesc = device_registry::pDevDesc;

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

/// Stub: get current device number. Returns 0 (no device).
unsafe fn curDevice() -> c_int {
    unsafe { device_registry::curDevice() }
}

/// Stub: get next device number. Returns 0.
unsafe fn nextDevice(_dev: c_int) -> c_int {
    unsafe { device_registry::nextDevice(_dev) }
}

/// Stub: get previous device number. Returns 0.
unsafe fn prevDevice(_dev: c_int) -> c_int {
    unsafe { device_registry::prevDevice(_dev) }
}

/// Stub: select device by number. Returns 0.
unsafe fn selectDevice(_dev: c_int) -> c_int {
    unsafe { device_registry::selectDevice(_dev) }
}

/// Stub: kill device by number. No-op.
unsafe fn killDevice(_dev: c_int) {
    unsafe { device_registry::killDevice(_dev) }
}

/// Stub: get device by number. Returns null.
unsafe fn GEgetDevice(_dev: c_int) -> pGEDevDesc {
    unsafe { device_registry::GEgetDevice(_dev) }
}

/// Stub: capture device raster. Returns R_NilValue (unsupported).
unsafe fn GEcurrentDevice() -> pGEDevDesc {
    unsafe { device_registry::GEcurrentDevice() }
}

/// Return whether the registry currently has no real devices.
unsafe fn NoDevices() -> c_int {
    unsafe { device_registry::NoDevices() }
}

/// Return the current device count, including the null device.
unsafe fn NumDevices() -> c_int {
    unsafe { device_registry::NumDevices() }
}

/* ==================== Device functions ==================== */

/// Helper: check that the argument has positive length.
/// Equivalent to the `checkArity_length` macro in the C source.
unsafe fn checkArity_length(args: SEXP) -> SEXP {
    unsafe {
        let args = CDR(args);
        if LENGTH(CAR(args)) == 0 {
            Rf_error(b"argument must have positive length\0".as_ptr() as *const c_char);
        }
        args
    }
}

/// devcontrol(list) - enable/disable display list recording on current device.
pub unsafe fn devcontrol(args: SEXP) -> SEXP {
    unsafe {
        let mut args = args;
        let listFlag = {
            args = CDR(args);
            asLogical(CAR(args))
        };
        if listFlag == NA_LOGICAL {
            Rf_error(b"invalid argument\0".as_ptr() as *const c_char);
        }
        let gdd = GEcurrentDevice();
        if !gdd.is_null() {
            (*gdd).displayListOn = listFlag;
        }
        device_registry::GEinitDisplayList(gdd);
        Rf_ScalarLogical(listFlag)
    }
}

/// devdisplaylist() - query display list recording status on current device.
pub unsafe fn devdisplaylist(args: SEXP) -> SEXP {
    unsafe {
        let gdd = GEcurrentDevice();
        if gdd.is_null() {
            Rf_ScalarLogical(0)
        } else {
            Rf_ScalarLogical((*gdd).displayListOn)
        }
    }
}

/// devcopy(which) - copy display list from one device to another.
pub unsafe fn devcopy(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArity_length(args);
        let dev_num = *INTEGER(CAR(args)).add(0) - 1;
        device_registry::GEcopyDisplayList(dev_num);
        R_NilValue()
    }
}

/// dev.cur() - return the number of the current device.
pub unsafe fn devcur(args: SEXP) -> SEXP {
    unsafe { Rf_ScalarInteger(curDevice() + 1) }
}

/// dev.next(which) - return the number of the next device.
pub unsafe fn devnext(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArity_length(args);
        let nxt = *INTEGER(CAR(args)).add(0);
        if nxt == NA_INTEGER {
            Rf_error(b"NA argument is invalid\0".as_ptr() as *const c_char);
        }
        Rf_ScalarInteger(nextDevice(nxt - 1) + 1)
    }
}

/// dev.prev(which) - return the number of the previous device.
pub unsafe fn devprev(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArity_length(args);
        let prev = *INTEGER(CAR(args)).add(0);
        if prev == NA_INTEGER {
            Rf_error(b"NA argument is invalid\0".as_ptr() as *const c_char);
        }
        Rf_ScalarInteger(prevDevice(prev - 1) + 1)
    }
}

/// dev.set(which) - set the specified device as the current device.
pub unsafe fn devset(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArity_length(args);
        let dev_num = *INTEGER(CAR(args)).add(0);
        if dev_num == NA_INTEGER {
            Rf_error(b"NA argument is invalid\0".as_ptr() as *const c_char);
        }
        Rf_ScalarInteger(selectDevice(dev_num - 1) + 1)
    }
}

/// dev.off(which) - shut down the specified device.
pub unsafe fn devoff(args: SEXP) -> SEXP {
    unsafe {
        let args = checkArity_length(args);
        let dev_num = *INTEGER(CAR(args)).add(0);
        if dev_num == 1 {
            Rf_error(b"cannot shut down device 1 (the null device)\0".as_ptr() as *const c_char);
        }
        // Check device number is valid (64 is max num devices)
        if dev_num > 0 && dev_num < 64 {
            let gdd = GEgetDevice(dev_num - 1);
            if !gdd.is_null() && (*gdd).lock != 0 {
                Rf_warning(b"Killing locked device\0".as_ptr() as *const c_char);
                (*gdd).lock = 0;
            }
        }
        killDevice(*INTEGER(CAR(args)).add(0) - 1);
        R_NilValue()
    }
}

/// dev.size(units) - return the size of the current device.
pub unsafe fn devsize(args: SEXP) -> SEXP {
    unsafe {
        let args = CDR(args);
        let scale = devsize_unit_scale(CAR(args));
        let gdd = GEcurrentDevice();
        let ans = Rf_allocVector(SEXPTYPE::REALSXP, 2);
        if gdd.is_null() {
            *REAL(ans).add(0) = 7.0 * scale;
            *REAL(ans).add(1) = 7.0 * scale;
        } else {
            *REAL(ans).add(0) = (*gdd).width.abs() * scale;
            *REAL(ans).add(1) = (*gdd).height.abs() * scale;
        }
        ans
    }
}

unsafe fn devsize_unit_scale(units: SEXP) -> c_double {
    unsafe {
        if units.is_null() || Rf_isNull(units) != 0 || LENGTH(units) == 0 {
            return 1.0;
        }
        if TYPEOF(units) != SEXPTYPE::STRSXP {
            Rf_error(b"'units' must be a character string\0".as_ptr() as *const c_char);
        }
        let unit = STRING_ELT(units, 0);
        if unit.is_null() {
            Rf_error(b"'units' must be a character string\0".as_ptr() as *const c_char);
        }
        let unit = std::ffi::CStr::from_ptr(CHAR(unit)).to_str().unwrap_or("");
        match unit {
            "in" => 1.0,
            "cm" => 2.54,
            "px" => 72.0,
            _ => {
                Rf_error(
                    b"'arg' should be one of \"in\", \"cm\", \"px\"\0".as_ptr() as *const c_char
                );
                unreachable!("Rf_error returned");
            }
        }
    }
}

/// dev.holdflush(level) - hold/flush device output.
pub unsafe fn devholdflush(args: SEXP) -> SEXP {
    unsafe {
        let mut args = args;
        args = CDR(args);
        let mut level = asInteger(CAR(args));
        let gdd = GEcurrentDevice();
        if level == NA_INTEGER || gdd.is_null() {
            level = 0;
        } else {
            level = (*gdd).holdflush_level;
        }
        Rf_ScalarInteger(level)
    }
}

unsafe fn int_vector(values: &[c_int]) -> SEXP {
    unsafe {
        let vector = Rf_allocVector(SEXPTYPE::INTSXP, values.len() as c_int);
        for (index, value) in values.iter().enumerate() {
            *INTEGER(vector).add(index) = *value;
        }
        vector
    }
}

unsafe fn set_int_capability(capabilities: SEXP, capability: c_int, values: &[c_int]) {
    unsafe {
        SET_VECTOR_ELT(capabilities, capability as R_xlen_t, int_vector(values));
    }
}

/// dev.capabilities() - query capabilities of the current device.
pub unsafe fn devcap(args: SEXP) -> SEXP {
    unsafe {
        let mut args = args;

        args = CDR(args);
        let capabilities = CAR(args);

        let gdd = GEcurrentDevice();
        let deviceVersion = if gdd.is_null() {
            0
        } else {
            (*gdd).deviceVersion
        };
        let haveTransparency = if gdd.is_null() {
            0
        } else {
            (*gdd).haveTransparency
        };
        let haveTransparentBg = if gdd.is_null() {
            0
        } else {
            (*gdd).haveTransparentBg
        };
        let haveRaster = if gdd.is_null() { 0 } else { (*gdd).haveRaster };
        let haveCapture = if gdd.is_null() { 0 } else { (*gdd).haveCapture };
        let haveLocator = if gdd.is_null() { 0 } else { (*gdd).haveLocator };
        let canGenMouseDown = if gdd.is_null() {
            0
        } else {
            (*gdd).canGenMouseDown
        };
        let canGenMouseMove = if gdd.is_null() {
            0
        } else {
            (*gdd).canGenMouseMove
        };
        let canGenMouseUp = if gdd.is_null() {
            0
        } else {
            (*gdd).canGenMouseUp
        };
        let canGenKeybd = if gdd.is_null() { 0 } else { (*gdd).canGenKeybd };
        let canGenIdle = if gdd.is_null() { 0 } else { (*gdd).canGenIdle };

        set_int_capability(
            capabilities,
            R_GE_capability_semiTransparency,
            &[haveTransparency],
        );

        set_int_capability(
            capabilities,
            R_GE_capability_transparentBackground,
            &[haveTransparentBg],
        );

        set_int_capability(capabilities, R_GE_capability_rasterImage, &[haveRaster]);

        set_int_capability(capabilities, R_GE_capability_capture, &[haveCapture]);

        set_int_capability(capabilities, R_GE_capability_locator, &[haveLocator]);

        set_int_capability(
            capabilities,
            R_GE_capability_events,
            &[
                canGenMouseDown,
                canGenMouseMove,
                canGenMouseUp,
                canGenKeybd,
                canGenIdle,
            ],
        );

        set_int_capability(capabilities, R_GE_capability_patterns, &[NA_INTEGER]);
        set_int_capability(capabilities, R_GE_capability_clippingPaths, &[NA_INTEGER]);
        set_int_capability(capabilities, R_GE_capability_masks, &[NA_INTEGER]);

        // deviceVersion < R_GE_group (stub), so all 0
        let group_capability = if deviceVersion < R_GE_group {
            0
        } else {
            NA_INTEGER
        };
        set_int_capability(
            capabilities,
            R_GE_capability_compositing,
            &[group_capability],
        );
        set_int_capability(
            capabilities,
            R_GE_capability_transformations,
            &[group_capability],
        );
        set_int_capability(capabilities, R_GE_capability_paths, &[group_capability]);

        let glyphs = if deviceVersion < R_GE_glyphs {
            0
        } else {
            NA_INTEGER
        };
        set_int_capability(capabilities, R_GE_capability_glyphs, &[glyphs]);

        let variable_fonts = if deviceVersion < R_GE_fontVar {
            0
        } else {
            NA_INTEGER
        };
        set_int_capability(
            capabilities,
            R_GE_capability_variableFonts,
            &[variable_fonts],
        );

        // Stub: no device->capabilities callback to invoke
        capabilities
    }
}

/// dev.capture(native) - capture the current device contents as a raster.
pub unsafe fn devcapture(args: SEXP) -> SEXP {
    unsafe {
        let mut args = args;
        let _gdd = GEcurrentDevice();
        let mut raster;
        let mut native;

        args = CDR(args);
        native = asLogical(CAR(args));
        if native != 1 {
            native = 0;
        }

        raster = device_registry::GECap(_gdd);
        // GECap returns R_NilValue when unsupported
        if Rf_isNull(raster) != 0 {
            return raster;
        }

        let _raster_guard = protect(raster);
        if native != 0 {
            let class = Rf_mkString(b"nativeRaster\0".as_ptr() as *const c_char);
            let _class_guard = protect(class);
            setAttrib(raster, R_ClassSymbol(), class);
            return raster;
        }

        // Non-native: convert to color strings (based on grid.cap logic)
        let size = LENGTH(raster);
        let dim_attr = getAttrib(raster, R_DimSymbol());
        let nrow = *INTEGER(dim_attr).add(0);
        let ncol = *INTEGER(dim_attr).add(1);

        let image = Rf_allocVector(SEXPTYPE::STRSXP, size as c_int);
        let _image_guard = protect(image);
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

        let idim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
        let _idim_guard = protect(idim);
        *INTEGER(idim).add(0) = nrow;
        *INTEGER(idim).add(1) = ncol;
        setAttrib(image, R_DimSymbol(), idim);

        image
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::{INTEGER, LENGTH, REAL, TYPEOF, VECTOR_ELT};
    use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector, Rf_cons};
    use crate::sexp::session::RSession;

    unsafe fn one_arg_args(arg: SEXP) -> SEXP {
        unsafe { Rf_cons(R_NilValue(), Rf_cons(arg, R_NilValue())) }
    }

    #[test]
    fn devsize_reports_null_device_in_requested_units() {
        let _session = RSession::new();
        device_registry::reset_registry_for_tests();

        unsafe {
            let inches = devsize(one_arg_args(Rf_mkString(c"in".as_ptr())));
            assert_eq!(*REAL(inches).add(0), 7.0);
            assert_eq!(*REAL(inches).add(1), 7.0);

            let cm = devsize(one_arg_args(Rf_mkString(c"cm".as_ptr())));
            assert!((*REAL(cm).add(0) - 17.78).abs() < 1e-10);
            assert!((*REAL(cm).add(1) - 17.78).abs() < 1e-10);

            let px = devsize(one_arg_args(Rf_mkString(c"px".as_ptr())));
            assert_eq!(*REAL(px).add(0), 504.0);
            assert_eq!(*REAL(px).add(1), 504.0);
        }
    }

    unsafe fn int_capability(capabilities: SEXP, capability: c_int) -> c_int {
        unsafe {
            let value = VECTOR_ELT(capabilities, capability as R_xlen_t);
            *INTEGER(value)
        }
    }

    #[test]
    fn devcap_reports_implemented_headless_features_only() {
        let _session = RSession::new();
        device_registry::reset_registry_for_tests();

        unsafe {
            let capabilities = Rf_allocVector(SEXPTYPE::VECSXP, 15);
            devcap(one_arg_args(capabilities));

            assert_eq!(int_capability(capabilities, R_GE_capability_rasterImage), 1);
            assert_eq!(int_capability(capabilities, R_GE_capability_capture), 1);
            assert_eq!(int_capability(capabilities, R_GE_capability_locator), 0);
        }
    }

    #[test]
    fn devcapture_returns_headless_native_raster() {
        let _session = RSession::new();
        device_registry::reset_registry_for_tests();

        unsafe {
            let raster = devcapture(one_arg_args(Rf_ScalarLogical(1)));
            assert_eq!(TYPEOF(raster), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(raster), 504 * 504);
            assert_eq!(*INTEGER(raster), 0x00ff_ffff);

            let dim = getAttrib(raster, R_DimSymbol());
            assert_eq!(*INTEGER(dim).add(0), 504);
            assert_eq!(*INTEGER(dim).add(1), 504);

            let class = getAttrib(raster, R_ClassSymbol());
            assert_eq!(TYPEOF(class), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(class), 1);
        }
    }
}
