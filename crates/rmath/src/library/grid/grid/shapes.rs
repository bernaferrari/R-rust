use super::*;

pub(crate) unsafe fn getArrowN(
    x1: SEXP,
    x2: SEXP,
    xnm1: SEXP,
    xn: SEXP,
    y1: SEXP,
    y2: SEXP,
    ynm1: SEXP,
    yn: SEXP,
) -> c_int {
    unsafe {
        let mut maxn = 0;
        let ny1 = if isNull(y1) { 0 } else { unitLength(y1) };
        let nx2 = unitLength(x2);
        let ny2 = unitLength(y2);
        let nxnm1 = if isNull(xnm1) { 0 } else { unitLength(xnm1) };
        let nynm1 = if isNull(ynm1) { 0 } else { unitLength(ynm1) };
        let nxn = unitLength(xn);
        let nyn = unitLength(yn);
        maxn = maxn.max(ny1);
        maxn = maxn.max(nx2);
        maxn = maxn.max(ny2);
        maxn = maxn.max(nxnm1);
        maxn = maxn.max(nynm1);
        maxn = maxn.max(nxn);
        maxn = maxn.max(nyn);
        maxn
    }
}

pub unsafe fn L_arrows(
    x1: SEXP,
    x2: SEXP,
    xnm1: SEXP,
    xn: SEXP,
    y1: SEXP,
    y2: SEXP,
    ynm1: SEXP,
    yn: SEXP,
    angle: SEXP,
    length: SEXP,
    ends: SEXP,
    r#type: SEXP,
) -> SEXP {
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

        let maxn = getArrowN(x1, x2, xnm1, xn, y1, y2, ynm1, yn);
        let ne = LENGTH(ends);
        resolveGPar(currentgp, 0);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        GEMode(1, dd);
        for i in 0..maxn {
            let mut first = true;
            let mut last = true;
            match *INTEGER(ends).add((i % ne) as usize) {
                2 => first = false,
                1 => last = false,
                _ => {}
            }
            updateGContext(currentgp, i, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

            let devloc = if isNull(x1) {
                gridStateElement(dd, GSS_CURRLOC)
            } else {
                R_NilValue()
            };
            let _devloc_guard = if isNull(x1) {
                Some(protect(devloc))
            } else {
                None
            };

            if first {
                let mut xx1: c_double = NA_REAL;
                let mut yy1: c_double = NA_REAL;
                if isNull(x1) {
                    if TYPEOF(devloc) == SEXPTYPE::REALSXP && LENGTH(devloc) >= 2 {
                        xx1 = *REAL(devloc).add(0);
                        yy1 = *REAL(devloc).add(1);
                    }
                } else {
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
                }
                let mut xx2: c_double = NA_REAL;
                let mut yy2: c_double = NA_REAL;
                transformLocn(
                    x2,
                    y2,
                    i,
                    vpc,
                    gc,
                    vpWidthCM,
                    vpHeightCM,
                    dd,
                    &mut transform,
                    &mut xx2,
                    &mut yy2,
                );

                let mut vertx = [0.0; 3];
                let mut verty = [0.0; 3];
                calcArrow(
                    xx1,
                    yy1,
                    xx2,
                    yy2,
                    angle,
                    length,
                    i,
                    vpc,
                    vpWidthCM,
                    vpHeightCM,
                    vertx.as_mut_ptr(),
                    verty.as_mut_ptr(),
                    gc,
                    dd,
                );
                if toDeviceX(xx2, GE_INCHES, dd).is_finite()
                    && toDeviceY(yy2, GE_INCHES, dd).is_finite()
                    && vertx[1].is_finite()
                    && verty[1].is_finite()
                {
                    drawArrow(vertx.as_ptr(), verty.as_ptr(), r#type, i, gc, dd);
                }
            }

            if last {
                let mut xxnm1: c_double = NA_REAL;
                let mut yynm1: c_double = NA_REAL;
                if isNull(xnm1) {
                    if TYPEOF(devloc) == SEXPTYPE::REALSXP && LENGTH(devloc) >= 2 {
                        xxnm1 = *REAL(devloc).add(0);
                        yynm1 = *REAL(devloc).add(1);
                    }
                } else {
                    transformLocn(
                        xnm1,
                        ynm1,
                        i,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xxnm1,
                        &mut yynm1,
                    );
                }

                let mut xxn: c_double = NA_REAL;
                let mut yyn: c_double = NA_REAL;
                transformLocn(
                    xn,
                    yn,
                    i,
                    vpc,
                    gc,
                    vpWidthCM,
                    vpHeightCM,
                    dd,
                    &mut transform,
                    &mut xxn,
                    &mut yyn,
                );

                let mut vertx = [0.0; 3];
                let mut verty = [0.0; 3];
                calcArrow(
                    xxn,
                    yyn,
                    xxnm1,
                    yynm1,
                    angle,
                    length,
                    i,
                    vpc,
                    vpWidthCM,
                    vpHeightCM,
                    vertx.as_mut_ptr(),
                    verty.as_mut_ptr(),
                    gc,
                    dd,
                );
                if toDeviceX(xxnm1, GE_INCHES, dd).is_finite()
                    && toDeviceY(yynm1, GE_INCHES, dd).is_finite()
                    && vertx[1].is_finite()
                    && verty[1].is_finite()
                {
                    drawArrow(vertx.as_ptr(), verty.as_ptr(), r#type, i, gc, dd);
                }
            }
        }
        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * L_polygon
 * ============================== */

pub unsafe fn L_polygon(x: SEXP, y: SEXP, index: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path).add(0) != 0
        {
            set_gp_fill_string(currentgp, c"black".as_ptr());
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
        let np = LENGTH(index);
        for i in 0..np {
            let indices = VECTOR_ELT(index, i as R_xlen_t);
            updateGContext(currentgp, i, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);
            let nx = LENGTH(indices);
            if nx <= 0 {
                continue;
            }

            let mut xx = vec![NA_REAL; nx as usize];
            let mut yy = vec![NA_REAL; nx as usize];
            let mut xold = NA_REAL;
            let mut yold = NA_REAL;
            let mut start: usize = 0;

            for j in 0..nx as usize {
                let idx = *INTEGER(indices).add(j);
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
                        &mut xx[j],
                        &mut yy[j],
                    );
                    xx[j] = toDeviceX(xx[j], GE_INCHES, dd);
                    yy[j] = toDeviceY(yy[j], GE_INCHES, dd);
                }

                let current_finite = xx[j].is_finite() && yy[j].is_finite();
                let previous_finite = xold.is_finite() && yold.is_finite();

                if current_finite && !previous_finite {
                    start = j;
                } else if previous_finite && !current_finite {
                    if j.saturating_sub(start) > 1 {
                        GEPolygon(
                            (j - start) as c_int,
                            xx.as_ptr().add(start),
                            yy.as_ptr().add(start),
                            gc,
                            dd,
                        );
                    }
                } else if previous_finite && j + 1 == nx as usize {
                    GEPolygon(
                        (nx as usize - start) as c_int,
                        xx.as_ptr().add(start),
                        yy.as_ptr().add(start),
                        gc,
                        dd,
                    );
                }

                xold = xx[j];
                yold = yy[j];
            }
        }
        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * gridCircle (internal)
 * ============================== */

pub(crate) unsafe fn gridCircle(
    _x: SEXP,
    _y: SEXP,
    _r: SEXP,
    _theta: c_double,
    _draw: bool,
) -> SEXP {
    unsafe { R_NilValue() }
}

/* ==============================
 * L_circle
 * ============================== */

pub unsafe fn L_circle(x: SEXP, y: SEXP, r: SEXP) -> SEXP {
    unsafe {
        gridCircle(x, y, r, 0.0, true);
        R_NilValue()
    }
}

/* ==============================
 * L_circleBounds
 * ============================== */

pub unsafe fn L_circleBounds(x: SEXP, y: SEXP, r: SEXP, theta: SEXP) -> SEXP {
    unsafe { gridCircle(x, y, r, *REAL(theta), false) }
}

/* ==============================
 * gridRect (internal)
 * ============================== */

pub(crate) unsafe fn gridRect(
    _x: SEXP,
    _y: SEXP,
    _w: SEXP,
    _h: SEXP,
    _hjust: SEXP,
    _vjust: SEXP,
    _theta: c_double,
    _draw: bool,
) -> SEXP {
    unsafe { R_NilValue() }
}

/* ==============================
 * L_rect
 * ============================== */

pub unsafe fn L_rect(x: SEXP, y: SEXP, w: SEXP, h: SEXP, hjust: SEXP, vjust: SEXP) -> SEXP {
    unsafe {
        gridRect(x, y, w, h, hjust, vjust, 0.0, true);
        R_NilValue()
    }
}

/* ==============================
 * L_rectBounds
 * ============================== */

pub unsafe fn L_rectBounds(
    x: SEXP,
    y: SEXP,
    w: SEXP,
    h: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    theta: SEXP,
) -> SEXP {
    unsafe { gridRect(x, y, w, h, hjust, vjust, *REAL(theta), false) }
}

/* ==============================
 * L_path
 * ============================== */

pub unsafe fn L_path(x: SEXP, y: SEXP, index: SEXP, rule: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path).add(0) != 0
        {
            set_gp_fill_string(currentgp, c"black".as_ptr());
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

        for h in 0..LENGTH(index) {
            let poly_ind = VECTOR_ELT(index, h as R_xlen_t);
            let npoly = LENGTH(poly_ind);
            if npoly <= 0 {
                continue;
            }

            let mut ntot: c_int = 0;
            let mut nper = vec![0i32; npoly as usize];
            for i in 0..npoly as usize {
                let n = LENGTH(VECTOR_ELT(poly_ind, i as R_xlen_t));
                nper[i] = n;
                ntot += n;
            }
            if ntot <= 0 {
                continue;
            }

            let mut xx = vec![0.0; ntot as usize];
            let mut yy = vec![0.0; ntot as usize];
            let mut k: usize = 0;
            for i in 0..npoly as usize {
                let indices = INTEGER(VECTOR_ELT(poly_ind, i as R_xlen_t));
                for j in 0..nper[i] as usize {
                    transformLocn(
                        x,
                        y,
                        *indices.add(j) - 1,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xx[k],
                        &mut yy[k],
                    );
                    xx[k] = toDeviceX(xx[k], GE_INCHES, dd);
                    yy[k] = toDeviceY(yy[k], GE_INCHES, dd);
                    if !xx[k].is_finite() || !yy[k].is_finite() {
                        Rf_error(c"non-finite x or y in graphics path".as_ptr());
                    }
                    k += 1;
                }
            }

            updateGContext(currentgp, h, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);
            GEPath(
                xx.as_mut_ptr(),
                yy.as_mut_ptr(),
                npoly,
                nper.as_mut_ptr(),
                asBool(rule),
                gc,
                dd,
            );
        }

        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * L_raster
 * ============================== */
