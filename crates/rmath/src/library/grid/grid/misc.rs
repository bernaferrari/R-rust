use super::*;
use crate::library::grid::{
    alloc_vector, char_ptr, protect, real_elt, scalar_integer, set_real_elt, set_vector_elt,
    string_elt, vector_elt,
};

pub fn L_clip(x: SEXP, y: SEXP, w: SEXP, h: SEXP, hjust: SEXP, vjust: SEXP) -> SEXP {
    {
        // SAFETY: device registry bridge.
        let dd = unsafe { getDevice() };
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = gridStateElement(dd, GSS_GPAR);

        // SAFETY: `currentvp`/`dd` keep their raw device-state contracts.
        let (vpWidthCM, vpHeightCM, transform, rotationAngle) =
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            { unsafe { getViewportTransform(currentvp, dd) } };

        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        // SAFETY: `dd` is the live device returned above.
        {
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            unsafe { GEMode(1, dd) }
        };

        /*
         * Only set ONE clip rectangle (i.e., NOT vectorised)
         */
        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let mut xx: c_double;
        let mut yy: c_double;
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        (xx, yy) =
            unsafe { transformLocn(x, y, 0, vpc, gc, vpWidthCM, vpHeightCM, dd, &transform) };
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        let ww = unsafe { transformWidthtoINCHES(w, 0, vpc, gc, vpWidthCM, vpHeightCM, dd) };
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        let hh = unsafe { transformHeighttoINCHES(h, 0, vpc, gc, vpWidthCM, vpHeightCM, dd) };

        /*
         * We can ONLY clip if the total rotation angle is zero.
         */
        if rotationAngle == 0.0 {
            let hjust_val = if TYPEOF(hjust) == SEXPTYPE::REALSXP && LENGTH(hjust) > 0 {
                real_elt(hjust, 0)
            } else {
                0.0
            };
            let vjust_val = if TYPEOF(vjust) == SEXPTYPE::REALSXP && LENGTH(vjust) > 0 {
                real_elt(vjust, 0)
            } else {
                0.0
            };
            xx = justifyX(xx, ww, hjust_val);
            yy = justifyY(yy, hh, vjust_val);

            /*
             * The graphics engine only takes device coordinates
             */
            // SAFETY: `dd` is the live device; `toDevice*` are engine
            // coordinate bridges over the device state.
            let (ww_dev, hh_dev) = {
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                xx = unsafe { toDeviceX(xx, GE_INCHES, dd) };
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                yy = unsafe { toDeviceY(yy, GE_INCHES, dd) };
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                let ww_dev = unsafe { toDeviceWidth(ww, GE_INCHES, dd) };
                // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                let hh_dev = unsafe { toDeviceHeight(hh, GE_INCHES, dd) };
                if xx.is_finite() && yy.is_finite() && ww_dev.is_finite() && hh_dev.is_finite() {
                    // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                    unsafe { GESetClip(xx, yy, xx + ww_dev, yy + hh_dev, dd) };
                }
                (ww_dev, hh_dev)
            };

            if xx.is_finite() && yy.is_finite() && ww_dev.is_finite() && hh_dev.is_finite() {
                /*
                 * ALSO set the current clip region for the current viewport so that,
                 * if a viewport is pushed within the current viewport, when that
                 * viewport gets popped again, the clip region returns to what was
                 * set by THIS clipGrob.
                 */
                let currentClip = alloc_vector(SEXPTYPE::REALSXP, 4);
                let _current_clip_guard = protect(currentClip);
                set_real_elt(currentClip, 0, xx);
                set_real_elt(currentClip, 1, yy);
                set_real_elt(currentClip, 2, xx + ww_dev);
                set_real_elt(currentClip, 3, yy + hh_dev);
                set_vector_elt(currentvp, PVP_CLIPRECT as R_xlen_t, currentClip);
            }
        } else {
            // SAFETY: static format string, no arguments.
            {
                Rf_warning_cstr(c"unable to clip to rotated rectangle")
            };
        }

        // SAFETY: `dd` is the live device; engine mode teardown mirrors
        // the `GEMode(1, dd)` above.
        {
            // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
            unsafe { GEMode(0, dd) }
        };
        R_NilValue()
    }
}

/* ==============================
 * L_pretty
 * ============================== */

pub fn L_pretty(scale: SEXP) -> SEXP {
    let n_ = scalar_integer(5);
    L_pretty2(scale, n_)
}

/* ==============================
 * L_pretty2
 * ============================== */

pub fn L_pretty2(scale: SEXP, n_: SEXP) -> SEXP {
    {
        let mut min = numeric(scale, 0);
        let mut max = numeric(scale, 1);
        // SAFETY: `n_` is a live object lent by the R caller.
        let mut n = { crate::main::coerce::asInteger(n_) };
        let mut temp: f64;

        let swap = min > max;
        if swap {
            temp = min;
            min = max;
            max = temp;
        }

        // SAFETY: all three arguments are live local scalars.
        unsafe { GEPretty(&mut min, &mut max, &mut n) };

        if swap {
            temp = min;
            min = max;
            max = temp;
        }

        crate::library::grdevices::axis_scales::create_at_vector_raw(
            [min, max, n as f64],
            [min, max],
            n,
            false,
        )
    }
}

/* ==============================
 * L_locator
 * ============================== */

pub fn L_locator() -> SEXP {
    let answer = alloc_vector(SEXPTYPE::REALSXP, 2);
    let _answer_guard = protect(answer);
    set_real_elt(answer, 0, f64::NAN);
    set_real_elt(answer, 1, f64::NAN);
    answer
}

/* ==============================
 * L_locnBounds
 * ============================== */

pub fn L_locnBounds(x: SEXP, y: SEXP, theta: SEXP) -> SEXP {
    // Full implementation requires unit conversion
    R_NilValue()
}

/* ==============================
 * L_stringMetric
 * ============================== */

pub fn L_stringMetric(label: SEXP) -> SEXP {
    {
        // SAFETY: device registry bridge.
        let dd = unsafe { getDevice() };
        let currentgp = gridStateElement(dd, GSS_GPAR);
        let n = if !label.is_null() && label != R_NilValue() {
            LENGTH(label)
        } else {
            0
        };

        let ascent_vec = alloc_vector(SEXPTYPE::REALSXP, n);
        let _ascent_guard = protect(ascent_vec);
        let descent_vec = alloc_vector(SEXPTYPE::REALSXP, n);
        let _descent_guard = protect(descent_vec);
        let width_vec = alloc_vector(SEXPTYPE::REALSXP, n);
        let _width_guard = protect(width_vec);

        let mut gc: [u8; 256] = [0; 256];
        gcontextFromgpar(currentgp, 0, gc.as_mut_ptr() as pGEcontext, dd);

        for i in 0..n as R_xlen_t {
            let mut ascent: f64 = 0.0;
            let mut descent: f64 = 0.0;
            let mut width: f64 = 0.0;
            if TYPEOF(label) == SEXPTYPE::EXPRSXP {
                GEExpressionMetric(
                    vector_elt(label, i),
                    gc.as_ptr() as pGEcontext,
                    &mut ascent,
                    &mut descent,
                    &mut width,
                    dd,
                );
            } else {
                let s = char_ptr(string_elt(label, i));
                let ce = getCharCE(string_elt(label, i));
                // SAFETY: `s` is the NUL-terminated CHAR payload of a live
                // string element; `dd` is the live device.
                {
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    unsafe {
                        GEStrMetric(
                            s,
                            ce,
                            gc.as_ptr() as pGEcontext,
                            &mut ascent,
                            &mut descent,
                            &mut width,
                            dd,
                        )
                    }
                };
            }
            set_real_elt(ascent_vec, i, ascent);
            set_real_elt(descent_vec, i, descent);
            set_real_elt(width_vec, i, width);
        }

        let result = alloc_vector(SEXPTYPE::VECSXP, 3);
        let _result_guard = protect(result);
        set_vector_elt(result, 0, ascent_vec);
        set_vector_elt(result, 1, descent_vec);
        set_vector_elt(result, 2, width_vec);
        result
    }
}

/* ==============================
 * L_convertToNative (deprecated)
 * ============================== */

pub fn L_convertToNative(_x: SEXP, _what: SEXP) -> SEXP {
    R_NilValue()
}
