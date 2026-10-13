use super::*;

pub unsafe fn L_moveTo(x: SEXP, y: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, c"transparent".as_ptr());

        let prevloc = gridStateElement(dd, GSS_PREVLOC);
        let _prevloc_guard = protect(prevloc);
        let devloc = gridStateElement(dd, GSS_CURRLOC);
        let _devloc_guard = protect(devloc);
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

        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let mut xx: c_double = NA_REAL;
        let mut yy: c_double = NA_REAL;
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

        if TYPEOF(prevloc) == SEXPTYPE::REALSXP
            && LENGTH(prevloc) >= 2
            && TYPEOF(devloc) == SEXPTYPE::REALSXP
            && LENGTH(devloc) >= 2
        {
            *REAL(prevloc).add(0) = *REAL(devloc).add(0);
            *REAL(prevloc).add(1) = *REAL(devloc).add(1);
            *REAL(devloc).add(0) = xx;
            *REAL(devloc).add(1) = yy;
        }

        R_NilValue()
    }
}

/* ==============================
 * L_lineTo
 * ============================== */

pub unsafe fn L_lineTo(x: SEXP, y: SEXP, arrow: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        if gp_fill_is_pattern(currentgp) {
            set_gp_fill_string(currentgp, c"transparent".as_ptr());
        }

        let prevloc = gridStateElement(dd, GSS_PREVLOC);
        let _prevloc_guard = protect(prevloc);
        let devloc = gridStateElement(dd, GSS_CURRLOC);
        let _devloc_guard = protect(devloc);
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

        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let mut xx: c_double = NA_REAL;
        let mut yy: c_double = NA_REAL;
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

        if TYPEOF(prevloc) == SEXPTYPE::REALSXP
            && LENGTH(prevloc) >= 2
            && TYPEOF(devloc) == SEXPTYPE::REALSXP
            && LENGTH(devloc) >= 2
        {
            *REAL(prevloc).add(0) = *REAL(devloc).add(0);
            *REAL(prevloc).add(1) = *REAL(devloc).add(1);
            *REAL(devloc).add(0) = xx;
            *REAL(devloc).add(1) = yy;

            let xx0 = toDeviceX(*REAL(prevloc).add(0), GE_INCHES, dd);
            let yy0 = toDeviceY(*REAL(prevloc).add(1), GE_INCHES, dd);
            let xx1 = toDeviceX(xx, GE_INCHES, dd);
            let yy1 = toDeviceY(yy, GE_INCHES, dd);

            if xx0.is_finite() && yy0.is_finite() && xx1.is_finite() && yy1.is_finite() {
                GEMode(1, dd);
                GELine(xx0, yy0, xx1, yy1, gc, dd);
                if !isNull(arrow) {
                    let ax = [xx0, xx1];
                    let ay = [yy0, yy1];
                    arrows(
                        ax.as_ptr(),
                        ay.as_ptr(),
                        2,
                        arrow,
                        0,
                        true,
                        true,
                        vpc,
                        vpWidthCM,
                        vpHeightCM,
                        gc,
                        dd,
                    );
                }
                GEMode(0, dd);
            }
        }

        R_NilValue()
    }
}

/* ==============================
 * L_lines
 * ============================== */

pub unsafe fn L_lines(x: SEXP, y: SEXP, index: SEXP, arrow: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        if gp_fill_is_pattern(currentgp) {
            set_gp_fill_string(currentgp, c"transparent".as_ptr());
        }
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

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        GEMode(1, dd);
        let nl = LENGTH(index);
        for j in 0..nl {
            let indices = VECTOR_ELT(index, j as R_xlen_t);
            updateGContext(currentgp, j, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);
            let nx = LENGTH(indices);
            if nx <= 0 {
                continue;
            }

            let mut xx = vec![NA_REAL; nx as usize];
            let mut yy = vec![NA_REAL; nx as usize];
            let mut xold = NA_REAL;
            let mut yold = NA_REAL;
            let mut start: usize = 0;

            for i in 0..nx as usize {
                let idx = *INTEGER(indices).add(i);
                if idx > 0 {
                    transformLocn(
                        x,
                        y,
                        idx - 1,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xx[i],
                        &mut yy[i],
                    );
                    xx[i] = toDeviceX(xx[i], GE_INCHES, dd);
                    yy[i] = toDeviceY(yy[i], GE_INCHES, dd);
                }

                let current_finite = xx[i].is_finite() && yy[i].is_finite();
                let previous_finite = xold.is_finite() && yold.is_finite();

                if current_finite && !previous_finite {
                    start = i;
                } else if previous_finite && !current_finite {
                    if i.saturating_sub(start) > 1 {
                        GEPolyline(
                            (i - start) as c_int,
                            xx.as_ptr().add(start),
                            yy.as_ptr().add(start),
                            gc,
                            dd,
                        );
                        if !isNull(arrow) {
                            arrows(
                                xx.as_ptr().add(start),
                                yy.as_ptr().add(start),
                                (i - start) as c_int,
                                arrow,
                                j,
                                start == 0,
                                false,
                                vpc,
                                vpWidthCM,
                                vpHeightCM,
                                gc,
                                dd,
                            );
                        }
                    }
                } else if previous_finite && i + 1 == nx as usize {
                    GEPolyline(
                        (nx as usize - start) as c_int,
                        xx.as_ptr().add(start),
                        yy.as_ptr().add(start),
                        gc,
                        dd,
                    );
                    if !isNull(arrow) {
                        arrows(
                            xx.as_ptr().add(start),
                            yy.as_ptr().add(start),
                            (nx as usize - start) as c_int,
                            arrow,
                            j,
                            start == 0,
                            true,
                            vpc,
                            vpWidthCM,
                            vpHeightCM,
                            gc,
                            dd,
                        );
                    }
                }

                xold = xx[i];
                yold = yy[i];
            }
        }
        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * gridXspline (internal)
 * ============================== */

pub(crate) unsafe fn gridXspline(
    _x: SEXP,
    _y: SEXP,
    _s: SEXP,
    _o: SEXP,
    _a: SEXP,
    _rep: SEXP,
    _index: SEXP,
    _theta: c_double,
    _draw: bool,
    _trace: bool,
) -> SEXP {
    unsafe { R_NilValue() }
}

/* ==============================
 * L_xspline
 * ============================== */

pub unsafe fn L_xspline(
    x: SEXP,
    y: SEXP,
    s: SEXP,
    o: SEXP,
    a: SEXP,
    rep: SEXP,
    index: SEXP,
) -> SEXP {
    unsafe {
        gridXspline(x, y, s, o, a, rep, index, 0.0, true, false);
        R_NilValue()
    }
}

/* ==============================
 * L_xsplineBounds
 * ============================== */

pub unsafe fn L_xsplineBounds(
    x: SEXP,
    y: SEXP,
    s: SEXP,
    o: SEXP,
    a: SEXP,
    rep: SEXP,
    index: SEXP,
    theta: SEXP,
) -> SEXP {
    unsafe { gridXspline(x, y, s, o, a, rep, index, *REAL(theta), false, false) }
}

/* ==============================
 * L_xsplinePoints
 * ============================== */

pub unsafe fn L_xsplinePoints(
    x: SEXP,
    y: SEXP,
    s: SEXP,
    o: SEXP,
    a: SEXP,
    rep: SEXP,
    index: SEXP,
    theta: SEXP,
) -> SEXP {
    unsafe { gridXspline(x, y, s, o, a, rep, index, *REAL(theta), false, true) }
}

/* ==============================
 * L_segments
 * ============================== */

pub unsafe fn L_segments(x0: SEXP, y0: SEXP, x1: SEXP, y1: SEXP, arrow: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        if gp_fill_is_pattern(currentgp) {
            set_gp_fill_string(currentgp, c"transparent".as_ptr());
        }

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

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        let maxn = unitLength(x0)
            .max(unitLength(y0))
            .max(unitLength(x1))
            .max(unitLength(y1));
        GEMode(1, dd);
        for i in 0..maxn {
            updateGContext(currentgp, i, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);
            let mut xx0: c_double = NA_REAL;
            let mut yy0: c_double = NA_REAL;
            let mut xx1: c_double = NA_REAL;
            let mut yy1: c_double = NA_REAL;
            transformLocn(
                x0,
                y0,
                i,
                vpc,
                gc,
                vpWidthCM,
                vpHeightCM,
                dd,
                &mut transform,
                &mut xx0,
                &mut yy0,
            );
            transformLocn(
                x1,
                y1,
                i,
                vpc,
                gc,
                vpWidthCM,
                vpHeightCM,
                dd,
                &mut transform,
                &mut xx1,
                &mut yy1,
            );
            xx0 = toDeviceX(xx0, GE_INCHES, dd);
            yy0 = toDeviceY(yy0, GE_INCHES, dd);
            xx1 = toDeviceX(xx1, GE_INCHES, dd);
            yy1 = toDeviceY(yy1, GE_INCHES, dd);
            if xx0.is_finite() && yy0.is_finite() && xx1.is_finite() && yy1.is_finite() {
                GELine(xx0, yy0, xx1, yy1, gc, dd);
                if !isNull(arrow) {
                    let ax = [xx0, xx1];
                    let ay = [yy0, yy1];
                    arrows(
                        ax.as_ptr(),
                        ay.as_ptr(),
                        2,
                        arrow,
                        i,
                        true,
                        true,
                        vpc,
                        vpWidthCM,
                        vpHeightCM,
                        gc,
                        dd,
                    );
                }
            }
        }
        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * L_arrows
 * ============================== */
