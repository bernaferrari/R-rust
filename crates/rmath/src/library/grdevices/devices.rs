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
use crate::main::errors::{Rf_error, Rf_error_cstr, Rf_warning, Rf_warning_cstr};
use crate::sexp::accessors::{
    CAR, CDR, INTEGER_ELT, LENGTH, SET_INTEGER_ELT, SET_REAL_ELT, SET_STRING_ELT, SET_VECTOR_ELT,
    STRING_ELT, TYPEOF, charsxp_as_utf8,
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
fn curDevice() -> c_int {
    device_registry::curDevice()
}

/// Stub: get next device number. Returns 0.
fn nextDevice(_dev: c_int) -> c_int {
    device_registry::nextDevice(_dev)
}

/// Stub: get previous device number. Returns 0.
fn prevDevice(_dev: c_int) -> c_int {
    device_registry::prevDevice(_dev)
}

/// Stub: select device by number. Returns 0.
fn selectDevice(_dev: c_int) -> c_int {
    device_registry::selectDevice(_dev)
}

/// Stub: kill device by number. No-op.
fn killDevice(_dev: c_int) {
    device_registry::killDevice(_dev)
}

use device_registry::{GEcurrentDevice, GEgetDevice};

/// Return whether the registry currently has no real devices.
fn NoDevices() -> c_int {
    device_registry::NoDevices()
}

/// Return the current device count, including the null device.
fn NumDevices() -> c_int {
    device_registry::NumDevices()
}

/* ==================== Device functions ==================== */

/// Helper: check that the argument has positive length.
/// Equivalent to the `checkArity_length` macro in the C source.
fn checkArity_length(args: SEXP) -> SEXP {
    let args = CDR(args);
    if LENGTH(CAR(args)) == 0 {
        Rf_error_cstr(c"argument must have positive length");
    }
    args
}

/// devcontrol(list) - enable/disable display list recording on current device.
pub fn devcontrol(args: SEXP) -> SEXP {
    let mut args = args;
    let listFlag = {
        args = CDR(args);
        asLogical(CAR(args))
    };
    if listFlag == NA_LOGICAL {
        Rf_error_cstr(c"invalid argument");
    }
    let gdd = GEcurrentDevice();
    let _ = device_registry::with_device_mut(gdd, |device| device.displayListOn = listFlag);
    device_registry::GEinitDisplayList(gdd);
    Rf_ScalarLogical(listFlag)
}

/// devdisplaylist() - query display list recording status on current device.
pub fn devdisplaylist(args: SEXP) -> SEXP {
    let gdd = GEcurrentDevice();
    Rf_ScalarLogical(
        device_registry::with_device_mut(gdd, |device| device.displayListOn).unwrap_or(0),
    )
}

/// devcopy(which) - copy display list from one device to another.
pub fn devcopy(args: SEXP) -> SEXP {
    let args = checkArity_length(args);
    let dev_num = INTEGER_ELT(CAR(args), 0) - 1;
    device_registry::GEcopyDisplayList(dev_num);
    R_NilValue()
}

/// dev.cur() - return the number of the current device.
pub fn devcur(args: SEXP) -> SEXP {
    Rf_ScalarInteger(curDevice() + 1)
}

/// dev.next(which) - return the number of the next device.
pub fn devnext(args: SEXP) -> SEXP {
    let args = checkArity_length(args);
    let nxt = INTEGER_ELT(CAR(args), 0);
    if nxt == NA_INTEGER {
        Rf_error_cstr(c"NA argument is invalid");
    }
    Rf_ScalarInteger(nextDevice(nxt - 1) + 1)
}

/// dev.prev(which) - return the number of the previous device.
pub fn devprev(args: SEXP) -> SEXP {
    let args = checkArity_length(args);
    let prev = INTEGER_ELT(CAR(args), 0);
    if prev == NA_INTEGER {
        Rf_error_cstr(c"NA argument is invalid");
    }
    Rf_ScalarInteger(prevDevice(prev - 1) + 1)
}

/// dev.set(which) - set the specified device as the current device.
pub fn devset(args: SEXP) -> SEXP {
    let args = checkArity_length(args);
    let dev_num = INTEGER_ELT(CAR(args), 0);
    if dev_num == NA_INTEGER {
        Rf_error_cstr(c"NA argument is invalid");
    }
    Rf_ScalarInteger(selectDevice(dev_num - 1) + 1)
}

/// dev.off(which) - shut down the specified device.
pub fn devoff(args: SEXP) -> SEXP {
    let args = checkArity_length(args);
    let dev_num = INTEGER_ELT(CAR(args), 0);
    if dev_num == 1 {
        Rf_error_cstr(c"cannot shut down device 1 (the null device)");
    }
    if dev_num > 1 {
        let gdd = GEgetDevice(dev_num - 1);
        let _ = device_registry::with_device_mut(gdd, |device| {
            if device.lock != 0 {
                Rf_warning_cstr(c"Killing locked device");
                device.lock = 0;
            }
        });
    }
    device_registry::close_device(dev_num).unwrap_or_else(|error| {
        crate::sexp::context::r_error(format!("cannot finish PDF: {error}"))
    });
    R_NilValue()
}

/// dev.size(units) - return the size of the current device.
pub fn devsize(args: SEXP) -> SEXP {
    let args = CDR(args);
    let scale = devsize_unit_scale(CAR(args));
    let gdd = GEcurrentDevice();
    let ans = Rf_allocVector(SEXPTYPE::REALSXP, 2);
    let (width, height) =
        device_registry::with_device_mut(gdd, |device| (device.width.abs(), device.height.abs()))
            .unwrap_or((7.0, 7.0));
    SET_REAL_ELT(ans, 0, width * scale);
    SET_REAL_ELT(ans, 1, height * scale);
    ans
}

fn devsize_unit_scale(units: SEXP) -> c_double {
    if units.is_null() || Rf_isNull(units) != 0 || LENGTH(units) == 0 {
        return 1.0;
    }
    if TYPEOF(units) != SEXPTYPE::STRSXP {
        Rf_error_cstr(c"'units' must be a character string");
    }
    let unit = STRING_ELT(units, 0);
    if unit.is_null() {
        Rf_error_cstr(c"'units' must be a character string");
    }
    match charsxp_as_utf8(unit).as_slice() {
        b"in" => 1.0,
        b"cm" => 2.54,
        b"px" => 72.0,
        _ => {
            Rf_error_cstr(c"'arg' should be one of \"in\", \"cm\", \"px\"");
            unreachable!("Rf_error returned");
        }
    }
}

/// dev.holdflush(level) - hold/flush device output.
pub fn devholdflush(args: SEXP) -> SEXP {
    let mut args = args;
    args = CDR(args);
    let mut level = asInteger(CAR(args));
    let gdd = GEcurrentDevice();
    if level == NA_INTEGER {
        level = 0;
    } else {
        level = device_registry::with_device_mut(gdd, |device| device.holdflush_level).unwrap_or(0);
    }
    Rf_ScalarInteger(level)
}

fn int_vector(values: &[c_int]) -> SEXP {
    let vector = Rf_allocVector(SEXPTYPE::INTSXP, values.len() as c_int);
    for (index, value) in values.iter().enumerate() {
        SET_INTEGER_ELT(vector, index as c_int, *value);
    }
    vector
}

fn set_int_capability(capabilities: SEXP, capability: c_int, values: &[c_int]) {
    SET_VECTOR_ELT(capabilities, capability as R_xlen_t, int_vector(values));
}

/// dev.capabilities() - query capabilities of the current device.
pub fn devcap(args: SEXP) -> SEXP {
    let mut args = args;

    args = CDR(args);
    let capabilities = CAR(args);

    let gdd = GEcurrentDevice();
    let fields = device_registry::with_device_mut(gdd, |device| {
        (
            device.deviceVersion,
            device.haveTransparency,
            device.haveTransparentBg,
            device.haveRaster,
            device.haveCapture,
            device.haveLocator,
            device.canGenMouseDown,
            device.canGenMouseMove,
            device.canGenMouseUp,
            device.canGenKeybd,
            device.canGenIdle,
        )
    })
    .unwrap_or_default();
    let (
        deviceVersion,
        haveTransparency,
        haveTransparentBg,
        haveRaster,
        haveCapture,
        haveLocator,
        canGenMouseDown,
        canGenMouseMove,
        canGenMouseUp,
        canGenKeybd,
        canGenIdle,
    ) = fields;

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

/// dev.capture(native) - capture the current device contents as a raster.
pub fn devcapture(args: SEXP) -> SEXP {
    let mut args = args;
    let _gdd = GEcurrentDevice();
    let mut native;

    args = CDR(args);
    native = asLogical(CAR(args));
    if native != 1 {
        native = 0;
    }

    let raster = device_registry::GECap(_gdd);
    // GECap returns R_NilValue when unsupported
    if Rf_isNull(raster) != 0 {
        return raster;
    }

    let _raster_guard = protect(raster);
    if native != 0 {
        let class = crate::sexp::constructors::mk_string_str("nativeRaster");
        let _class_guard = protect(class);
        setAttrib(raster, R_ClassSymbol(), class);
        return raster;
    }

    // Non-native: convert to color strings (based on grid.cap logic)
    let size = LENGTH(raster);
    let dim_attr = getAttrib(raster, R_DimSymbol());
    let nrow = INTEGER_ELT(dim_attr, 0);
    let ncol = INTEGER_ELT(dim_attr, 1);

    let image = Rf_allocVector(SEXPTYPE::STRSXP, size as c_int);
    let _image_guard = protect(image);
    let mut i: c_int = 0;
    while i < size {
        let col = (i % ncol) + 1;
        let row = (i / ncol) + 1;
        let idx = ((col - 1) * nrow + row - 1) as R_xlen_t;
        let charsexp = super::colors::mk_col2name_char(INTEGER_ELT(raster, i) as c_uint);
        SET_STRING_ELT(image, idx, charsexp);
        i += 1;
    }

    let idim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
    let _idim_guard = protect(idim);
    SET_INTEGER_ELT(idim, 0, nrow);
    SET_INTEGER_ELT(idim, 1, ncol);
    setAttrib(image, R_DimSymbol(), idim);

    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::{INTEGER_ELT, LENGTH, REAL_ELT, TYPEOF, VECTOR_ELT};
    use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector, Rf_cons};
    use crate::sexp::session::RSession;

    fn one_arg_args(arg: SEXP) -> SEXP {
        Rf_cons(R_NilValue(), Rf_cons(arg, R_NilValue()))
    }

    /// `Rf_mkString` on a literal: safe-wrapper for the test arg lists.
    fn mk_test_string(text: &std::ffi::CStr) -> SEXP {
        // SAFETY: `text` is a live NUL-terminated string for the call.
        unsafe { crate::sexp::constructors::mk_string_bytes(text.to_bytes()) }
    }

    #[test]
    fn devsize_reports_null_device_in_requested_units() {
        let _session = RSession::new();
        device_registry::reset_registry_for_tests();
        // GEcurrentDevice() evaluates options("device") only when nothing
        // beyond the null device exists (GNU devices.c); selecting the null
        // device opens a real one, so the helper resolves the current device
        // without reentering the evaluator.
        assert_eq!(device_registry::selectDevice(0), 0);
        let inches = devsize(one_arg_args(mk_test_string(c"in")));
        assert_eq!(REAL_ELT(inches, 0), 7.0);
        assert_eq!(REAL_ELT(inches, 1), 7.0);

        let cm = devsize(one_arg_args(mk_test_string(c"cm")));
        assert!((REAL_ELT(cm, 0) - 17.78).abs() < 1e-10);
        assert!((REAL_ELT(cm, 1) - 17.78).abs() < 1e-10);

        let px = devsize(one_arg_args(mk_test_string(c"px")));
        assert_eq!(REAL_ELT(px, 0), 504.0);
        assert_eq!(REAL_ELT(px, 1), 504.0);
    }

    fn int_capability(capabilities: SEXP, capability: c_int) -> c_int {
        let value = VECTOR_ELT(capabilities, capability as R_xlen_t);
        INTEGER_ELT(value, 0)
    }

    #[test]
    fn devcap_reports_implemented_headless_features_only() {
        let _session = RSession::new();
        device_registry::reset_registry_for_tests();
        device_registry::selectDevice(0);

        let capabilities = Rf_allocVector(SEXPTYPE::VECSXP, 15);
        devcap(one_arg_args(capabilities));

        assert_eq!(int_capability(capabilities, R_GE_capability_rasterImage), 1);
        assert_eq!(int_capability(capabilities, R_GE_capability_capture), 1);
        assert_eq!(int_capability(capabilities, R_GE_capability_locator), 0);
    }

    #[test]
    fn devcapture_returns_headless_native_raster() {
        let _session = RSession::new();
        device_registry::reset_registry_for_tests();
        device_registry::selectDevice(0);

        let raster = devcapture(one_arg_args(Rf_ScalarLogical(1)));
        assert_eq!(TYPEOF(raster), SEXPTYPE::INTSXP);
        assert_eq!(LENGTH(raster), 504 * 504);
        assert_eq!(INTEGER_ELT(raster, 0), 0x00ff_ffff);

        let dim = getAttrib(raster, R_DimSymbol());
        assert_eq!(INTEGER_ELT(dim, 0), 504);
        assert_eq!(INTEGER_ELT(dim, 1), 504);

        let class = getAttrib(raster, R_ClassSymbol());
        assert_eq!(TYPEOF(class), SEXPTYPE::STRSXP);
        assert_eq!(LENGTH(class), 1);
    }
}
