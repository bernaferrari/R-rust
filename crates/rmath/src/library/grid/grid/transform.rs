use super::*;

pub unsafe fn getViewportTransform(currentvp: SEXP, dd: pGEDevDesc) -> (f64, f64, LTransform, f64) {
    use crate::sexp::accessors::REAL_ELT;

    unsafe {
        let (device_width, device_height) = getDeviceSize(dd);
        if deviceChanged(device_width, device_height, currentvp) {
            calcViewportTransform(currentvp, viewportParent(currentvp), true, dd);
        }

        let width = viewportWidthCM(currentvp);
        let height = viewportHeightCM(currentvp);
        let rotation = viewportRotation(currentvp);
        let matrix = viewportTransform(currentvp);

        let real_or = |value, fallback| {
            if TYPEOF(value) == SEXPTYPE::REALSXP && XLENGTH(value) > 0 {
                REAL_ELT(value, 0)
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
                    output_row[column] = REAL_ELT(matrix, (column * 3 + row) as c_int);
                }
            }
        }
        (width, height, transform, rotation)
    }
}

/* ==============================
 * L_convert
 * ============================== */

pub unsafe fn L_convert(x: SEXP, whatfrom: SEXP, whatto: SEXP, unitto: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, c"black".as_ptr());

        let (vpWidthCM, vpHeightCM, _, _) = getViewportTransform(currentvp, dd);
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        let nx = unitLength(x);
        let answer = Rf_allocVector(SEXPTYPE::REALSXP, nx);
        let _answer_guard = protect(answer);
        let unitto_len = LENGTH(unitto).max(1);
        let from_axis = if LENGTH(whatfrom) > 0 {
            *INTEGER(whatfrom).add(0)
        } else {
            0
        };
        let to_axis = if LENGTH(whatto) > 0 {
            *INTEGER(whatto).add(0)
        } else {
            0
        };

        for i in 0..nx {
            updateGContext(currentgp, i, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);
            let to_unit = *INTEGER(unitto).add((i % unitto_len) as usize);
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
                        transformXtoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd)
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
                        transformYtoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd)
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
                        transformWidthtoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd)
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
                        transformHeighttoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd)
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

            *REAL(answer).add(i as usize) = value_in;
        }

        answer
    }
}

/* ==============================
 * L_devLoc
 * ============================== */

pub unsafe fn L_devLoc(x: SEXP, y: SEXP, device: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, c"black".as_ptr());

        let (vpWidthCM, vpHeightCM, mut transform, _) = getViewportTransform(currentvp, dd);
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let maxn = unitLength(x).max(unitLength(y));
        let devx = Rf_allocVector(SEXPTYPE::REALSXP, maxn);
        let _devx_guard = protect(devx);
        let devy = Rf_allocVector(SEXPTYPE::REALSXP, maxn);
        let _devy_guard = protect(devy);
        let result = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let as_device =
            TYPEOF(device) == SEXPTYPE::LGLSXP && LENGTH(device) > 0 && *LOGICAL(device) != 0;

        for i in 0..maxn {
            let mut xx: c_double = NA_REAL;
            let mut yy: c_double = NA_REAL;
            transformLocn(
                x,
                y,
                i,
                vpc,
                gc,
                vpWidthCM,
                vpHeightCM,
                dd,
                &mut transform,
                &mut xx,
                &mut yy,
            );
            if as_device {
                xx = toDeviceX(xx, GE_INCHES, dd);
                yy = toDeviceY(yy, GE_INCHES, dd);
            }
            *REAL(devx).add(i as usize) = xx;
            *REAL(devy).add(i as usize) = yy;
        }

        SET_VECTOR_ELT(result, 0 as R_xlen_t, devx);
        SET_VECTOR_ELT(result, 1 as R_xlen_t, devy);
        result
    }
}

/* ==============================
 * L_devDim
 * ============================== */

pub unsafe fn L_devDim(x: SEXP, y: SEXP, device: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, c"black".as_ptr());

        let (vpWidthCM, vpHeightCM, _, rotationAngle) = getViewportTransform(currentvp, dd);
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let maxn = unitLength(x).max(unitLength(y));
        let devx = Rf_allocVector(SEXPTYPE::REALSXP, maxn);
        let _devx_guard = protect(devx);
        let devy = Rf_allocVector(SEXPTYPE::REALSXP, maxn);
        let _devy_guard = protect(devy);
        let result = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let as_device =
            TYPEOF(device) == SEXPTYPE::LGLSXP && LENGTH(device) > 0 && *LOGICAL(device) != 0;

        for i in 0..maxn {
            let mut xx: c_double = NA_REAL;
            let mut yy: c_double = NA_REAL;
            transformDimn(
                x,
                y,
                i,
                vpc,
                gc,
                vpWidthCM,
                vpHeightCM,
                dd,
                rotationAngle,
                &mut xx,
                &mut yy,
            );
            if as_device {
                xx = toDeviceWidth(xx, GE_INCHES, dd);
                yy = toDeviceHeight(yy, GE_INCHES, dd);
            }
            *REAL(devx).add(i as usize) = xx;
            *REAL(devy).add(i as usize) = yy;
        }

        SET_VECTOR_ELT(result, 0 as R_xlen_t, devx);
        SET_VECTOR_ELT(result, 1 as R_xlen_t, devy);
        result
    }
}

/* ==============================
 * L_layoutRegion
 * ============================== */

pub unsafe fn L_layoutRegion(layoutPosRow: SEXP, layoutPosCol: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
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
            *REAL(viewportWidthCM(currentvp))
        } else {
            1.0
        };
        let vp_height_cm = if !isNull(viewportHeightCM(currentvp))
            && TYPEOF(viewportHeightCM(currentvp)) == SEXPTYPE::REALSXP
            && LENGTH(viewportHeightCM(currentvp)) > 0
        {
            *REAL(viewportHeightCM(currentvp))
        } else {
            1.0
        };
        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;

        let x_cm = transformXtoINCHES(vpl.x, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) * 2.54;
        let y_cm = transformYtoINCHES(vpl.y, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) * 2.54;
        let w_cm =
            transformWidthtoINCHES(vpl.width, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) * 2.54;
        let h_cm =
            transformHeighttoINCHES(vpl.height, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) * 2.54;

        let answer = Rf_allocVector(SEXPTYPE::REALSXP, 4);
        let _answer_guard = protect(answer);
        *REAL(answer).add(0) = x_cm;
        *REAL(answer).add(1) = y_cm;
        *REAL(answer).add(2) = w_cm;
        *REAL(answer).add(3) = h_cm;
        answer
    }
}

/* ==============================
 * Edge detection functions
 * ============================== */
