use super::*;
use crate::library::grid::{
    alloc_vector, char_ptr, protect, real_elt, scalar_integer, set_real_elt, set_vector_elt,
    string_elt, vector_elt,
};

pub fn L_clip(x: SEXP, y: SEXP, w: SEXP, h: SEXP, hjust: SEXP, vjust: SEXP) -> SEXP {
    {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = gridStateElement(dd, GSS_GPAR);

        let (vpWidthCM, vpHeightCM, transform, rotationAngle) =
            { getViewportTransform(currentvp, dd) };

        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        {
            GEMode(1, dd)
        };

        /*
         * Only set ONE clip rectangle (i.e., NOT vectorised)
         */
        let gc_buf = R_GE_gcontext::zeroed();
        let gc = Some(&gc_buf);
        gcontextFromgpar(currentgp, 0, gc, dd);

        let mut xx: c_double;
        let mut yy: c_double;
        (xx, yy) = transformLocn(x, y, 0, vpc, gc, vpWidthCM, vpHeightCM, dd, &transform);
        let ww = transformWidthtoINCHES(w, 0, vpc, gc, vpWidthCM, vpHeightCM, dd);
        let hh = transformHeighttoINCHES(h, 0, vpc, gc, vpWidthCM, vpHeightCM, dd);

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
            let (ww_dev, hh_dev) = {
                xx = toDeviceX(xx, GE_INCHES, dd);
                yy = toDeviceY(yy, GE_INCHES, dd);
                let ww_dev = toDeviceWidth(ww, GE_INCHES, dd);
                let hh_dev = toDeviceHeight(hh, GE_INCHES, dd);
                if xx.is_finite() && yy.is_finite() && ww_dev.is_finite() && hh_dev.is_finite() {
                    GESetClip(xx, yy, xx + ww_dev, yy + hh_dev, dd);
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
            {
                Rf_warning_cstr(c"unable to clip to rotated rectangle")
            };
        }

        {
            GEMode(0, dd)
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
        let mut n = { crate::main::coerce::asInteger(n_) };
        let mut temp: f64;

        let swap = min > max;
        if swap {
            temp = min;
            min = max;
            max = temp;
        }

        GEPretty(&mut min, &mut max, &mut n);

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
        let dd = getDevice();
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

        let gc = R_GE_gcontext::zeroed();
        gcontextFromgpar(currentgp, 0, Some(&gc), dd);

        for i in 0..n as R_xlen_t {
            let mut ascent: f64 = 0.0;
            let mut descent: f64 = 0.0;
            let mut width: f64 = 0.0;
            if TYPEOF(label) == SEXPTYPE::EXPRSXP {
                GEExpressionMetric(
                    vector_elt(label, i),
                    Some(&gc),
                    &mut ascent,
                    &mut descent,
                    &mut width,
                    dd,
                );
            } else {
                let s = crate::library::grid::char_cstring(string_elt(label, i));
                let ce = getCharCE(string_elt(label, i));
                        GEStrMetric(
                    Some(&s),
                            ce,
                    Some(&gc),
                            &mut ascent,
                            &mut descent,
                            &mut width,
                            dd,
                );
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
