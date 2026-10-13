use super::*;

pub unsafe fn L_clip(x: SEXP, y: SEXP, w: SEXP, h: SEXP, hjust: SEXP, vjust: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = gridStateElement(dd, GSS_GPAR);

        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );

        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        GEMode(1, dd);

        /*
         * Only set ONE clip rectangle (i.e., NOT vectorised)
         */
        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let mut xx: c_double = 0.0;
        let mut yy: c_double = 0.0;
        transformLocn(
            x,
            y,
            0,
            vpc,
            gc,
            vpWidthCM,
            vpHeightCM,
            dd,
            &mut transform,
            &mut xx,
            &mut yy,
        );
        let ww = transformWidthtoINCHES(w, 0, vpc, gc, vpWidthCM, vpHeightCM, dd);
        let hh = transformHeighttoINCHES(h, 0, vpc, gc, vpWidthCM, vpHeightCM, dd);

        /*
         * We can ONLY clip if the total rotation angle is zero.
         */
        if rotationAngle == 0.0 {
            let hjust_val = if TYPEOF(hjust) == SEXPTYPE::REALSXP && LENGTH(hjust) > 0 {
                *REAL(hjust)
            } else {
                0.0
            };
            let vjust_val = if TYPEOF(vjust) == SEXPTYPE::REALSXP && LENGTH(vjust) > 0 {
                *REAL(vjust)
            } else {
                0.0
            };
            xx = justifyX(xx, ww, hjust_val);
            yy = justifyY(yy, hh, vjust_val);

            /*
             * The graphics engine only takes device coordinates
             */
            xx = toDeviceX(xx, GE_INCHES, dd);
            yy = toDeviceY(yy, GE_INCHES, dd);
            let ww_dev = toDeviceWidth(ww, GE_INCHES, dd);
            let hh_dev = toDeviceHeight(hh, GE_INCHES, dd);

            if xx.is_finite() && yy.is_finite() && ww_dev.is_finite() && hh_dev.is_finite() {
                GESetClip(xx, yy, xx + ww_dev, yy + hh_dev, dd);

                /*
                 * ALSO set the current clip region for the current viewport so that,
                 * if a viewport is pushed within the current viewport, when that
                 * viewport gets popped again, the clip region returns to what was
                 * set by THIS clipGrob.
                 */
                let currentClip = Rf_allocVector(SEXPTYPE::REALSXP, 4);
                let _current_clip_guard = protect(currentClip);
                *REAL(currentClip).add(0) = xx;
                *REAL(currentClip).add(1) = yy;
                *REAL(currentClip).add(2) = xx + ww_dev;
                *REAL(currentClip).add(3) = yy + hh_dev;
                SET_VECTOR_ELT(currentvp, PVP_CLIPRECT as R_xlen_t, currentClip);
            }
        } else {
            Rf_warning(c"unable to clip to rotated rectangle".as_ptr());
        }

        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * L_pretty
 * ============================== */

pub unsafe fn L_pretty(scale: SEXP) -> SEXP {
    unsafe {
        let n_ = Rf_ScalarInteger(5);
        L_pretty2(scale, n_)
    }
}

/* ==============================
 * L_pretty2
 * ============================== */

pub unsafe fn L_pretty2(scale: SEXP, n_: SEXP) -> SEXP {
    unsafe {
        let mut min = numeric(scale, 0);
        let mut max = numeric(scale, 1);
        let mut n = crate::main::coerce::asInteger(n_);
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

        let mut axp = [min, max, n as f64];
        Rf_CreateAtVector(axp.as_mut_ptr(), ptr::null_mut(), n, 0)
    }
}

/* ==============================
 * L_locator
 * ============================== */

pub unsafe fn L_locator() -> SEXP {
    unsafe {
        let answer = Rf_allocVector(SEXPTYPE::REALSXP, 2);
        let _answer_guard = protect(answer);
        *REAL(answer).add(0) = f64::NAN;
        *REAL(answer).add(1) = f64::NAN;
        answer
    }
}

/* ==============================
 * L_locnBounds
 * ============================== */

pub unsafe fn L_locnBounds(x: SEXP, y: SEXP, theta: SEXP) -> SEXP {
    unsafe {
        // Full implementation requires unit conversion
        R_NilValue()
    }
}

/* ==============================
 * L_stringMetric
 * ============================== */

pub unsafe fn L_stringMetric(label: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentgp = gridStateElement(dd, GSS_GPAR);
        let n = if !label.is_null() && label != R_NilValue() {
            LENGTH(label)
        } else {
            0
        };

        let ascent_vec = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _ascent_guard = protect(ascent_vec);
        let descent_vec = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _descent_guard = protect(descent_vec);
        let width_vec = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _width_guard = protect(width_vec);

        let mut gc: [u8; 256] = [0; 256];
        gcontextFromgpar(currentgp, 0, gc.as_mut_ptr() as pGEcontext, dd);

        for i in 0..n as R_xlen_t {
            let mut ascent: f64 = 0.0;
            let mut descent: f64 = 0.0;
            let mut width: f64 = 0.0;
            if TYPEOF(label) == SEXPTYPE::EXPRSXP {
                GEExpressionMetric(
                    VECTOR_ELT(label, i),
                    gc.as_ptr() as pGEcontext,
                    &mut ascent,
                    &mut descent,
                    &mut width,
                    dd,
                );
            } else {
                let s = CHAR(STRING_ELT(label, i));
                let ce = getCharCE(STRING_ELT(label, i));
                GEStrMetric(
                    s,
                    ce,
                    gc.as_ptr() as pGEcontext,
                    &mut ascent,
                    &mut descent,
                    &mut width,
                    dd,
                );
            }
            *REAL(ascent_vec).add(i as usize) = ascent;
            *REAL(descent_vec).add(i as usize) = descent;
            *REAL(width_vec).add(i as usize) = width;
        }

        let result = Rf_allocVector(SEXPTYPE::VECSXP, 3);
        let _result_guard = protect(result);
        SET_VECTOR_ELT(result, 0, ascent_vec);
        SET_VECTOR_ELT(result, 1, descent_vec);
        SET_VECTOR_ELT(result, 2, width_vec);
        result
    }
}

/* ==============================
 * L_convertToNative (deprecated)
 * ============================== */

pub unsafe fn L_convertToNative(_x: SEXP, _what: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}
