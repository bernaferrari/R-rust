use super::*;

pub(crate) fn lang4(symbol: SEXP, arg1: SEXP, arg2: SEXP, arg3: SEXP) -> SEXP {
    let tail = Rf_cons(arg3, R_NilValue());
    let tail = Rf_cons(arg2, tail);
    let tail = Rf_cons(arg1, tail);
    let call = Rf_cons(symbol, tail);
    if !call.is_null() {
        crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
    }
    call
}

pub(crate) fn SET_TAG(x: SEXP, y: SEXP) {
    SETTAG(x, y);
}

pub(crate) fn NewFrameConfirm(_dev: *const c_void) {}

pub(crate) fn GEExpressionMetric(
    _x: SEXP,
    _gc: pGEcontext,
    ascent: &mut f64,
    descent: &mut f64,
    width: &mut f64,
    _dd: pGEDevDesc,
) {
    *ascent = 0.0;
    *descent = 0.0;
    *width = 0.0;
}

pub(crate) fn rmath_grid_release_definitions(dd: pGEDevDesc, clear_groups: c_int) {
    release_grid_definitions(dd.cast(), clear_groups);
}

pub(crate) fn initVP(dd: pGEDevDesc) {
    crate::library::grid::viewport::initVP(dd as *const u8);
}

/* ==============================
 * Constants
 * ============================== */

pub(crate) const GE_INCHES: c_int = crate::mainutils::engine::GE_INCHES;
pub(crate) const R_TRANWHITE: c_int = 0x7FFFFFFF;
pub(crate) const DEG2RAD: f64 = std::f64::consts::PI / 180.0;
const NA_REAL: f64 = crate::sexp::ffi::NA_REAL;
pub(crate) const CE_UTF8: c_int = crate::sexp::encoding::CE_UTF8;
pub(crate) const CE_SYMBOL: c_int = crate::sexp::encoding::CE_SYMBOL;
pub(crate) const CE_NATIVE: c_int = crate::sexp::encoding::CE_NATIVE;

// Symbol drawing constants
pub(crate) const SMALL: f64 = 0.25;
pub(crate) const RADIUS: f64 = 0.375;
pub(crate) const SQRC: f64 = 0.88622692545275801364;
pub(crate) const DMDC: f64 = 1.25331413731550025119;
pub(crate) const TRC0: f64 = 1.55512030155621416073;
pub(crate) const TRC1: f64 = 1.34677368708859836060;
pub(crate) const TRC2: f64 = 0.77756015077810708036;

/* ==============================
 * Local helper: numeric(x, index)
 * ============================== */

#[inline]
pub(crate) fn numeric(x: SEXP, index: c_int) -> f64 {
    crate::library::grid::real_elt(x, index as R_xlen_t)
}

/* ==============================
 * Local helper: isNull
 * ============================== */

#[inline]
pub(crate) fn isNull(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue()
}

/* ==============================
 * Local helper: fmin2, fmax2
 * ============================== */

#[inline]
pub(crate) fn fmin2(a: f64, b: f64) -> f64 {
    a.min(b)
}

#[inline]
pub(crate) fn fmax2(a: f64, b: f64) -> f64 {
    a.max(b)
}

/* ==============================
 * getDevice
 * ============================== */

/// The current device (engine `GEcurrentDevice`) as an opaque registry key.
pub fn getDevice() -> pGEDevDesc {
    crate::library::grdevices::device_registry::GEcurrentDevice().cast()
}

/* ==============================
 * getDeviceSize
 * ============================== */

pub fn getDeviceSize(dd: pGEDevDesc) -> (c_double, c_double) {
    // Prefer device-driven conversion helpers when available.
    // In headless mode these still give deterministic non-zero sizes.
    let mut width_in = toDeviceWidth(1.0, GE_INCHES, dd).abs();
    let mut height_in = toDeviceHeight(1.0, GE_INCHES, dd).abs();
    if !width_in.is_finite() || width_in == 0.0 {
        width_in = 1.0;
    }
    if !height_in.is_finite() || height_in == 0.0 {
        height_in = 1.0;
    }
    (width_in * 2.54, height_in * 2.54)
}

/* ==============================
 * deviceChanged
 * ============================== */

pub(crate) fn deviceChanged(devWidthCM: c_double, devHeightCM: c_double, currentvp: SEXP) -> bool {
    let mut result = false;
    let pvpDevWidthCM = crate::library::grid::vector_elt(currentvp, PVP_DEVWIDTHCM as R_xlen_t);
    let _width_guard = crate::library::grid::protect(pvpDevWidthCM);
    let pvpDevHeightCM = crate::library::grid::vector_elt(currentvp, PVP_DEVHEIGHTCM as R_xlen_t);
    let _height_guard = crate::library::grid::protect(pvpDevHeightCM);
    if (crate::library::grid::real_elt(pvpDevWidthCM, 0) - devWidthCM).abs() > 1e-6 {
        result = true;
        crate::library::grid::set_real_elt(pvpDevWidthCM, 0, devWidthCM);
        crate::library::grid::set_vector_elt(currentvp, PVP_DEVWIDTHCM as R_xlen_t, pvpDevWidthCM);
    }
    if (crate::library::grid::real_elt(pvpDevHeightCM, 0) - devHeightCM).abs() > 1e-6 {
        result = true;
        crate::library::grid::set_real_elt(pvpDevHeightCM, 0, devHeightCM);
        crate::library::grid::set_vector_elt(
            currentvp,
            PVP_DEVHEIGHTCM as R_xlen_t,
            pvpDevHeightCM,
        );
    }
    result
}

/* ==============================
 * L_initGrid
 * ============================== */
