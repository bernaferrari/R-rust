use super::*;
use crate::library::grid::{
    alloc_vector, duplicate, integer_elt, logical_elt, protect, real_elt, set_real_elt,
    set_vector_elt,
};

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getViewportTransform(currentvp: SEXP, dd: pGEDevDesc) -> (f64, f64, LTransform, f64) {
    {
        // SAFETY: `dd` is the live device pointer passed by the caller.
        let (device_width, device_height) = unsafe { getDeviceSize(dd) };
        if deviceChanged(device_width, device_height, currentvp) {
            // SAFETY: forwarded raw pointers satisfy `calcViewportTransform`'s contract per this function's own `# Safety` requirements.
            unsafe { calcViewportTransform(currentvp, viewportParent(currentvp), true, dd) };
        }

        let width = viewportWidthCM(currentvp);
        let height = viewportHeightCM(currentvp);
        let rotation = viewportRotation(currentvp);
        let matrix = viewportTransform(currentvp);

        let real_or = |value, fallback| {
            if TYPEOF(value) == SEXPTYPE::REALSXP && XLENGTH(value) > 0 {
                real_elt(value, 0)
            } else {
                fallback
            }
        };
        let width = real_or(width, device_width);
        let height = real_or(height, device_height);
        let rotation = real_or(rotation, 0.0);

        let mut transform = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        if TYPEOF(matrix) == SEXPTYPE::REALSXP && XLENGTH(matrix) >= 9 {
            for column in 0..3 {
                for (row, output_row) in transform.iter_mut().enumerate() {
                    output_row[column] = real_elt(matrix, (column * 3 + row) as R_xlen_t);
                }
            }
        }
        (width, height, transform, rotation)
    }
}

/* ==============================
 * L_convert
 * ============================== */

pub fn L_convert(x: SEXP, whatfrom: SEXP, whatto: SEXP, unitto: SEXP) -> SEXP {
    {
        // SAFETY: device registry bridge.
        let dd = unsafe { getDevice() };
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, "black");

        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        let (vpWidthCM, vpHeightCM, _, _) = unsafe { getViewportTransform(currentvp, dd) };
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, &mut gp_is_scalar, gc_cache);

        let nx = unitLength(x);
        let answer = alloc_vector(SEXPTYPE::REALSXP, nx);
        let _answer_guard = protect(answer);
        let unitto_len = LENGTH(unitto).max(1);
        let from_axis = if LENGTH(whatfrom) > 0 {
            integer_elt(whatfrom, 0)
        } else {
            0
        };
        let to_axis = if LENGTH(whatto) > 0 {
            integer_elt(whatto, 0)
        } else {
            0
        };

        for i in 0..nx {
            updateGContext(currentgp, i, gc, dd, &mut gp_is_scalar, gc_cache);
            let to_unit = integer_elt(unitto, (i % unitto_len) as R_xlen_t);
            let mut rel_convert = (unitUnit(x, i) == L_NATIVE || unitUnit(x, i) == L_NPC)
                && (to_unit == L_NATIVE || to_unit == L_NPC)
                && ((from_axis == to_axis)
                    || (from_axis == 0 && to_axis == 2)
                    || (from_axis == 2 && to_axis == 0)
                    || (from_axis == 1 && to_axis == 3)
                    || (from_axis == 3 && to_axis == 1));

            let mut value_in = match from_axis {
                0 => {
                    if rel_convert && vpWidthCM < 1e-6 {
                        transformXYtoNPC(
                            unitValue(x, i),
                            unitUnit(x, i),
                            vpc.xscalemin,
                            vpc.xscalemax,
                        )
                    } else {
                        rel_convert = false;
                        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                        unsafe { transformXtoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd) }
                    }
                }
                1 => {
                    if rel_convert && vpHeightCM < 1e-6 {
                        transformXYtoNPC(
                            unitValue(x, i),
                            unitUnit(x, i),
                            vpc.yscalemin,
                            vpc.yscalemax,
                        )
                    } else {
                        rel_convert = false;
                        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                        unsafe { transformYtoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd) }
                    }
                }
                2 => {
                    if rel_convert && vpWidthCM < 1e-6 {
                        transformWHtoNPC(
                            unitValue(x, i),
                            unitUnit(x, i),
                            vpc.xscalemin,
                            vpc.xscalemax,
                        )
                    } else {
                        rel_convert = false;
                        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                        unsafe { transformWidthtoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd) }
                    }
                }
                3 => {
                    if rel_convert && vpHeightCM < 1e-6 {
                        transformWHtoNPC(
                            unitValue(x, i),
                            unitUnit(x, i),
                            vpc.yscalemin,
                            vpc.yscalemax,
                        )
                    } else {
                        rel_convert = false;
                        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                        unsafe { transformHeighttoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd) }
                    }
                }
                _ => unitValue(x, i),
            };

            value_in = match to_axis {
                0 => {
                    if rel_convert {
                        transformXYfromNPC(value_in, to_unit, vpc.xscalemin, vpc.xscalemax)
                    } else {
                        transformXYFromINCHES(
                            value_in,
                            to_unit,
                            vpc.xscalemin,
                            vpc.xscalemax,
                            gc,
                            vpWidthCM,
                            vpHeightCM,
                            dd,
                        )
                    }
                }
                1 => {
                    if rel_convert {
                        transformXYfromNPC(value_in, to_unit, vpc.yscalemin, vpc.yscalemax)
                    } else {
                        transformXYFromINCHES(
                            value_in,
                            to_unit,
                            vpc.yscalemin,
                            vpc.yscalemax,
                            gc,
                            vpHeightCM,
                            vpWidthCM,
                            dd,
                        )
                    }
                }
                2 => {
                    if rel_convert {
                        transformWHfromNPC(value_in, to_unit, vpc.xscalemin, vpc.xscalemax)
                    } else {
                        transformWidthHeightFromINCHES(
                            value_in,
                            to_unit,
                            vpc.xscalemin,
                            vpc.xscalemax,
                            gc,
                            vpWidthCM,
                            vpHeightCM,
                            dd,
                        )
                    }
                }
                3 => {
                    if rel_convert {
                        transformWHfromNPC(value_in, to_unit, vpc.yscalemin, vpc.yscalemax)
                    } else {
                        transformWidthHeightFromINCHES(
                            value_in,
                            to_unit,
                            vpc.yscalemin,
                            vpc.yscalemax,
                            gc,
                            vpHeightCM,
                            vpWidthCM,
                            dd,
                        )
                    }
                }
                _ => value_in,
            };

            set_real_elt(answer, i as R_xlen_t, value_in);
        }

        answer
    }
}

/* ==============================
 * L_devLoc
 * ============================== */

pub fn L_devLoc(x: SEXP, y: SEXP, device: SEXP) -> SEXP {
    {
        // SAFETY: device registry bridge.
        let dd = unsafe { getDevice() };
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, "black");

        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        let (vpWidthCM, vpHeightCM, transform, _) = unsafe { getViewportTransform(currentvp, dd) };
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let maxn = unitLength(x).max(unitLength(y));
        let devx = alloc_vector(SEXPTYPE::REALSXP, maxn);
        let _devx_guard = protect(devx);
        let devy = alloc_vector(SEXPTYPE::REALSXP, maxn);
        let _devy_guard = protect(devy);
        let result = alloc_vector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let as_device =
            TYPEOF(device) == SEXPTYPE::LGLSXP && LENGTH(device) > 0 && logical_elt(device, 0) != 0;

        for i in 0..maxn {
            let mut xx: c_double;
            let mut yy: c_double;
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            (xx, yy) =
                unsafe { transformLocn(x, y, i, vpc, gc, vpWidthCM, vpHeightCM, dd, &transform) };
            if as_device {
                // SAFETY: `dd` is the live device; `toDevice*` are engine
                // coordinate bridges over the device state.

                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                xx = unsafe { toDeviceX(xx, GE_INCHES, dd) };
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                yy = unsafe { toDeviceY(yy, GE_INCHES, dd) };
            }
            set_real_elt(devx, i as R_xlen_t, xx);
            set_real_elt(devy, i as R_xlen_t, yy);
        }

        set_vector_elt(result, 0 as R_xlen_t, devx);
        set_vector_elt(result, 1 as R_xlen_t, devy);
        result
    }
}

/* ==============================
 * L_devDim
 * ============================== */

pub fn L_devDim(x: SEXP, y: SEXP, device: SEXP) -> SEXP {
    {
        // SAFETY: device registry bridge.
        let dd = unsafe { getDevice() };
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, "black");

        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        let (vpWidthCM, vpHeightCM, _, rotationAngle) =
            unsafe { getViewportTransform(currentvp, dd) };
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let maxn = unitLength(x).max(unitLength(y));
        let devx = alloc_vector(SEXPTYPE::REALSXP, maxn);
        let _devx_guard = protect(devx);
        let devy = alloc_vector(SEXPTYPE::REALSXP, maxn);
        let _devy_guard = protect(devy);
        let result = alloc_vector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let as_device =
            TYPEOF(device) == SEXPTYPE::LGLSXP && LENGTH(device) > 0 && logical_elt(device, 0) != 0;

        for i in 0..maxn {
            let mut xx: c_double;
            let mut yy: c_double;
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            (xx, yy) = unsafe {
                transformDimn(x, y, i, vpc, gc, vpWidthCM, vpHeightCM, dd, rotationAngle)
            };
            if as_device {
                // SAFETY: `dd` is the live device; `toDevice*` are engine
                // coordinate bridges over the device state.

                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                xx = unsafe { toDeviceWidth(xx, GE_INCHES, dd) };
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                yy = unsafe { toDeviceHeight(yy, GE_INCHES, dd) };
            }
            set_real_elt(devx, i as R_xlen_t, xx);
            set_real_elt(devy, i as R_xlen_t, yy);
        }

        set_vector_elt(result, 0 as R_xlen_t, devx);
        set_vector_elt(result, 1 as R_xlen_t, devy);
        result
    }
}

/* ==============================
 * L_layoutRegion
 * ============================== */

pub fn L_layoutRegion(layoutPosRow: SEXP, layoutPosCol: SEXP) -> SEXP {
    {
        // SAFETY: device registry bridge.
        let dd = unsafe { getDevice() };
        let currentvp = gridStateElement(dd, GSS_VP);
        if isNull(viewportLayout(currentvp)) {
            return R_NilValue();
        }
        let mut vpl = LViewportLocation {
            x: R_NilValue(),
            y: R_NilValue(),
            width: R_NilValue(),
            height: R_NilValue(),
            hjust: 0.0,
            vjust: 0.0,
        };
        calcViewportLocationFromLayout(layoutPosRow, layoutPosCol, currentvp, &mut vpl);

        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);
        let vp_width_cm = if !isNull(viewportWidthCM(currentvp))
            && TYPEOF(viewportWidthCM(currentvp)) == SEXPTYPE::REALSXP
            && LENGTH(viewportWidthCM(currentvp)) > 0
        {
            real_elt(viewportWidthCM(currentvp), 0)
        } else {
            1.0
        };
        let vp_height_cm = if !isNull(viewportHeightCM(currentvp))
            && TYPEOF(viewportHeightCM(currentvp)) == SEXPTYPE::REALSXP
            && LENGTH(viewportHeightCM(currentvp)) > 0
        {
            real_elt(viewportHeightCM(currentvp), 0)
        } else {
            1.0
        };
        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;

        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        let x_cm =
            unsafe { transformXtoINCHES(vpl.x, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) } * 2.54;
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        let y_cm =
            unsafe { transformYtoINCHES(vpl.y, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) } * 2.54;
        let w_cm =
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            (unsafe { transformWidthtoINCHES(vpl.width, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) }) * 2.54;
        let h_cm =
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            (unsafe { transformHeighttoINCHES(vpl.height, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) }) * 2.54;

        let answer = alloc_vector(SEXPTYPE::REALSXP, 4);
        let _answer_guard = protect(answer);
        set_real_elt(answer, 0, x_cm);
        set_real_elt(answer, 1, y_cm);
        set_real_elt(answer, 2, w_cm);
        set_real_elt(answer, 3, h_cm);
        answer
    }
}

/* ==============================
 * Edge detection functions
 * ============================== */
