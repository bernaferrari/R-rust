use super::*;

pub(crate) unsafe fn GEcurrentDevice() -> pGEDevDesc {
    unsafe { crate::library::grdevices::device_registry::GEcurrentDevice() as pGEDevDesc }
}

pub(crate) unsafe fn lang4(symbol: SEXP, arg1: SEXP, arg2: SEXP, arg3: SEXP) -> SEXP {
    unsafe {
        let tail = Rf_cons(arg3, R_NilValue());
        let tail = Rf_cons(arg2, tail);
        let tail = Rf_cons(arg1, tail);
        let call = Rf_cons(symbol, tail);
        if !call.is_null() {
            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
        }
        call
    }
}

pub(crate) unsafe fn SET_TAG(x: SEXP, y: SEXP) {
    unsafe {
        SETTAG(x, y);
    }
}

pub(crate) unsafe fn NewFrameConfirm(_dev: *const c_void) {}

pub(crate) unsafe fn Rf_CreateAtVector(
    axp: *mut c_double,
    usr: *mut c_double,
    n: c_int,
    log: c_int,
) -> SEXP {
    unsafe {
        if axp.is_null() {
            return R_NilValue();
        }
        let axp_values = [*axp, *axp.add(1), *axp.add(2)];
        let usr_values = if usr.is_null() {
            [axp_values[0], axp_values[1]]
        } else {
            [*usr, *usr.add(1)]
        };
        crate::library::grdevices::axis_scales::create_at_vector_raw(
            axp_values,
            usr_values,
            n,
            log != 0,
        )
    }
}

pub(crate) unsafe fn GEExpressionMetric(
    _x: SEXP,
    _gc: pGEcontext,
    ascent: *mut f64,
    descent: *mut f64,
    width: *mut f64,
    _dd: pGEDevDesc,
) {
    unsafe {
        if !ascent.is_null() {
            *ascent = 0.0;
        }
        if !descent.is_null() {
            *descent = 0.0;
        }
        if !width.is_null() {
            *width = 0.0;
        }
    }
}

pub(crate) unsafe fn rmath_grid_release_definitions(dd: pGEDevDesc, clear_groups: c_int) {
    unsafe {
        release_grid_definitions(
            dd as crate::mainutils::graphics_ffi::pGEDevDesc,
            clear_groups,
        );
    }
}

pub(crate) unsafe fn initVP(dd: pGEDevDesc) {
    unsafe {
        crate::library::grid::viewport::initVP(dd as *const u8);
    }
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
pub(crate) unsafe fn numeric(x: SEXP, index: c_int) -> f64 {
    unsafe { *REAL(x).add(index as usize) }
}

/* ==============================
 * Local helper: isNull
 * ============================== */

#[inline]
pub(crate) unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
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

pub unsafe fn getDevice() -> pGEDevDesc {
    unsafe { GEcurrentDevice() }
}

/* ==============================
 * getDeviceSize
 * ============================== */

pub unsafe fn getDeviceSize(dd: pGEDevDesc, devWidthCM: *mut c_double, devHeightCM: *mut c_double) {
    unsafe {
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
        *devWidthCM = width_in * 2.54;
        *devHeightCM = height_in * 2.54;
    }
}

/* ==============================
 * deviceChanged
 * ============================== */

pub(crate) unsafe fn deviceChanged(
    devWidthCM: c_double,
    devHeightCM: c_double,
    currentvp: SEXP,
) -> bool {
    unsafe {
        let mut result = false;
        let pvpDevWidthCM = VECTOR_ELT(currentvp, PVP_DEVWIDTHCM as R_xlen_t);
        let _width_guard = protect(pvpDevWidthCM);
        let pvpDevHeightCM = VECTOR_ELT(currentvp, PVP_DEVHEIGHTCM as R_xlen_t);
        let _height_guard = protect(pvpDevHeightCM);
        if (*REAL(pvpDevWidthCM) - devWidthCM).abs() > 1e-6 {
            result = true;
            *REAL(pvpDevWidthCM) = devWidthCM;
            SET_VECTOR_ELT(currentvp, PVP_DEVWIDTHCM as R_xlen_t, pvpDevWidthCM);
        }
        if (*REAL(pvpDevHeightCM) - devHeightCM).abs() > 1e-6 {
            result = true;
            *REAL(pvpDevHeightCM) = devHeightCM;
            SET_VECTOR_ELT(currentvp, PVP_DEVHEIGHTCM as R_xlen_t, pvpDevHeightCM);
        }
        result
    }
}

/* ==============================
 * L_initGrid
 * ============================== */
