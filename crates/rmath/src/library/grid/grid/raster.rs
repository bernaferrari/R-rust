use super::*;
use crate::library::grid::{
    alloc_vector, checked, duplicate, get_attrib, inherits, integer_elt, logical_elt, protect,
    real_elt,
};

pub fn L_raster(
    raster: SEXP,
    x: SEXP,
    y: SEXP,
    w: SEXP,
    h: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    interpolate: SEXP,
) -> SEXP {
    {
        // SAFETY: device registry bridge.
        let dd = unsafe { getDevice() };
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, "transparent");

        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        let (vpWidthCM, vpHeightCM, transform, rotationAngle) =
            unsafe { getViewportTransform(currentvp, dd) };
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, &mut gp_is_scalar, gc_cache);

        let n = LENGTH(raster);
        if n <= 0 {
            // SAFETY: longjmp-style error bridge; does not return.
            Rf_error_cstr(c"Empty raster");
        }

        let mut image_owned: Vec<c_uint>;
        let image: *mut c_uint = if inherits(raster, c"nativeRaster")
            && TYPEOF(raster) == SEXPTYPE::INTSXP
        {
            checked(raster).data_ptr().unwrap_or(std::ptr::null_mut()) as *mut c_uint
        } else {
            image_owned = vec![0; n as usize];
            for i in 0..n as usize {
                // SAFETY: `raster` is a live object; `RGBpar3` is the engine
                // colour parser bridge over its element data.
                image_owned[i] =
                    unsafe { RGBpar3(raster as *mut c_void, i as c_int, R_TRANWHITE as c_uint) };
            }
            image_owned.as_mut_ptr()
        };

        let dim = get_attrib(raster, R_DimSymbol());
        if TYPEOF(dim) != SEXPTYPE::INTSXP || LENGTH(dim) < 2 {
            // SAFETY: longjmp-style error bridge; does not return.
            Rf_error_cstr(c"invalid raster dimensions");
        }

        let mut maxn = unitLength(x);
        maxn = maxn.max(unitLength(y));
        maxn = maxn.max(unitLength(w));
        maxn = maxn.max(unitLength(h));
        let hjust_len = LENGTH(hjust).max(1) as usize;
        let vjust_len = LENGTH(vjust).max(1) as usize;
        let interp_len = LENGTH(interpolate).max(1) as usize;

        // SAFETY: `dd` is the live device returned above.
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        unsafe { GEMode(1, dd) };
        for i in 0..maxn as usize {
            updateGContext(currentgp, i as c_int, gc, dd, &mut gp_is_scalar, gc_cache);

            let mut xx: c_double;
            let mut yy: c_double;
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            (xx, yy) = unsafe {
                transformLocn(
                    x, y, i as c_int, vpc, gc, vpWidthCM, vpHeightCM, dd, &transform,
                )
            };
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            let mut ww = unsafe {
                transformWidthtoINCHES(w, i as c_int, vpc, gc, vpWidthCM, vpHeightCM, dd)
            };
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            let mut hh = unsafe {
                transformHeighttoINCHES(h, i as c_int, vpc, gc, vpWidthCM, vpHeightCM, dd)
            };
            let hjust_i = if TYPEOF(hjust) == SEXPTYPE::REALSXP && LENGTH(hjust) > 0 {
                real_elt(hjust, (i % hjust_len) as R_xlen_t)
            } else {
                0.0
            };
            let vjust_i = if TYPEOF(vjust) == SEXPTYPE::REALSXP && LENGTH(vjust) > 0 {
                real_elt(vjust, (i % vjust_len) as R_xlen_t)
            } else {
                0.0
            };
            let interp_i = if TYPEOF(interpolate) == SEXPTYPE::LGLSXP && LENGTH(interpolate) > 0 {
                logical_elt(interpolate, (i % interp_len) as R_xlen_t)
            } else {
                0
            };

            if rotationAngle == 0.0 {
                xx = justifyX(xx, ww, hjust_i);
                yy = justifyY(yy, hh, vjust_i);
                // SAFETY: `dd` is the live device; `toDevice*`/`GERaster` are
                // engine bridges. `image` is either a live INTSXP data buffer
                // or a Rust Vec with `n` elements whose lifetime covers the
                // call.

                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                xx = unsafe { toDeviceX(xx, GE_INCHES, dd) };
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                yy = unsafe { toDeviceY(yy, GE_INCHES, dd) };
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                ww = unsafe { toDeviceWidth(ww, GE_INCHES, dd) };
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                hh = unsafe { toDeviceHeight(hh, GE_INCHES, dd) };
                if xx.is_finite() && yy.is_finite() && ww.is_finite() && hh.is_finite() {
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    unsafe {
                        GERaster(
                            image,
                            integer_elt(dim, 1),
                            integer_elt(dim, 0),
                            xx,
                            yy,
                            ww,
                            hh,
                            rotationAngle,
                            interp_i,
                            gc,
                            dd,
                        )
                    };
                }
            } else {
                let (xadj, yadj) = justification(ww, hh, hjust_i, vjust_i);
                let xadjInches = unit(xadj, L_INCHES);
                let _xadj_guard = protect(xadjInches);
                let yadjInches = unit(yadj, L_INCHES);
                let _yadj_guard = protect(yadjInches);
                // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                let (dw, dh) = unsafe {
                    transformDimn(
                        xadjInches,
                        yadjInches,
                        0,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        rotationAngle,
                    )
                };
                let mut xbl = xx + dw;
                let mut ybl = yy + dh;
                // SAFETY: `dd` is the live device; `toDevice*`/`GERaster`
                // are engine bridges over the live `image` buffer.

                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                xbl = unsafe { toDeviceX(xbl, GE_INCHES, dd) };
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                ybl = unsafe { toDeviceY(ybl, GE_INCHES, dd) };
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                ww = unsafe { toDeviceWidth(ww, GE_INCHES, dd) };
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                hh = unsafe { toDeviceHeight(hh, GE_INCHES, dd) };
                if xbl.is_finite() && ybl.is_finite() && ww.is_finite() && hh.is_finite() {
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    unsafe {
                        GERaster(
                            image,
                            integer_elt(dim, 1),
                            integer_elt(dim, 0),
                            xbl,
                            ybl,
                            ww,
                            hh,
                            rotationAngle,
                            interp_i,
                            gc,
                            dd,
                        )
                    };
                }
            }
        }
        // SAFETY: `dd` is the live device; engine mode teardown mirrors
        // the `GEMode(1, dd)` above.
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        unsafe { GEMode(0, dd) };
        R_NilValue()
    }
}

/* ==============================
 * L_cap
 * ============================== */

pub fn L_cap() -> SEXP {
    {
        // SAFETY: `dd` is the live device; `GECap` is the device capture
        // bridge returning a live object or NULL.
        let dd = unsafe { getDevice() };
        let raster = unsafe { GECap(dd) };
        let _raster_guard = protect(raster);
        if isNull(raster) {
            R_NilValue()
        } else {
            let image = alloc_vector(SEXPTYPE::STRSXP, LENGTH(raster));
            let _image_guard = protect(image);
            image
        }
    }
}

/* ==============================
 * gridText (internal)
 * ============================== */
