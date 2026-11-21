use super::*;
use crate::library::grid::{duplicate, integer_elt, logical_elt, protect, real_elt, vector_elt};

pub(crate) fn getArrowN(
    x1: SEXP,
    x2: SEXP,
    xnm1: SEXP,
    xn: SEXP,
    y1: SEXP,
    y2: SEXP,
    ynm1: SEXP,
    yn: SEXP,
) -> c_int {
    {
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

pub fn L_arrows(
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
    {
        // SAFETY: device registry bridge.
        let dd = unsafe { getDevice() };
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        if gp_fill_is_pattern(currentgp) {
            set_gp_fill_string(currentgp, "transparent");
        }

        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        let (vpWidthCM, vpHeightCM, transform, _) = unsafe { getViewportTransform(currentvp, dd) };
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
        initGContext(currentgp, gc, dd, &mut gp_is_scalar, gc_cache);

        // SAFETY: `dd` is the live device returned above.
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        unsafe { GEMode(1, dd) };
        for i in 0..maxn {
            let mut first = true;
            let mut last = true;
            match integer_elt(ends, (i % ne) as R_xlen_t) {
                2 => first = false,
                1 => last = false,
                _ => {}
            }
            updateGContext(currentgp, i, gc, dd, &mut gp_is_scalar, gc_cache);

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
                        xx1 = real_elt(devloc, 0);
                        yy1 = real_elt(devloc, 1);
                    }
                } else {
                    (xx1, yy1) =
                        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                        unsafe { transformLocn(x1, y1, i, vpc, gc, vpWidthCM, vpHeightCM, dd, &transform) };
                }
                let (xx2, yy2) =
                    // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                    unsafe { transformLocn(x2, y2, i, vpc, gc, vpWidthCM, vpHeightCM, dd, &transform) };

                let mut vertx = [0.0; 3];
                let mut verty = [0.0; 3];
                // SAFETY: `vertx`/`verty` are live 3-element buffers; `gc`
                // and `dd` keep their engine/device contracts.
                unsafe {
                    calcArrow(
                        xx1, yy1, xx2, yy2, angle, length, i, vpc, vpWidthCM, vpHeightCM,
                        &mut vertx, &mut verty, gc, dd,
                    );
                    if toDeviceX(xx2, GE_INCHES, dd).is_finite()
                        && toDeviceY(yy2, GE_INCHES, dd).is_finite()
                        && vertx[1].is_finite()
                        && verty[1].is_finite()
                    {
                        drawArrow(vertx.as_ptr(), verty.as_ptr(), r#type, i, gc, dd);
                    }
                }
            }

            if last {
                let mut xxnm1: c_double = NA_REAL;
                let mut yynm1: c_double = NA_REAL;
                if isNull(xnm1) {
                    if TYPEOF(devloc) == SEXPTYPE::REALSXP && LENGTH(devloc) >= 2 {
                        xxnm1 = real_elt(devloc, 0);
                        yynm1 = real_elt(devloc, 1);
                    }
                } else {
                    // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                    (xxnm1, yynm1) = unsafe {
                        transformLocn(
                            xnm1, ynm1, i, vpc, gc, vpWidthCM, vpHeightCM, dd, &transform,
                        )
                    };
                }

                let (xxn, yyn) =
                    // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                    unsafe { transformLocn(xn, yn, i, vpc, gc, vpWidthCM, vpHeightCM, dd, &transform) };

                let mut vertx = [0.0; 3];
                let mut verty = [0.0; 3];
                // SAFETY: `vertx`/`verty` are live 3-element buffers; `gc`
                // and `dd` keep their engine/device contracts.
                unsafe {
                    calcArrow(
                        xxn, yyn, xxnm1, yynm1, angle, length, i, vpc, vpWidthCM, vpHeightCM,
                        &mut vertx, &mut verty, gc, dd,
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
        }
        // SAFETY: `dd` is the live device; engine mode teardown mirrors
        // the `GEMode(1, dd)` above.
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        unsafe { GEMode(0, dd) };
        R_NilValue()
    }
}

/* ==============================
 * L_polygon
 * ============================== */

pub fn L_polygon(x: SEXP, y: SEXP, index: SEXP) -> SEXP {
    {
        // SAFETY: device registry bridge.
        let dd = unsafe { getDevice() };
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && logical_elt(resolving_path, 0) != 0
        {
            set_gp_fill_string(currentgp, "black");
        }

        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        let (vpWidthCM, vpHeightCM, transform, _) = unsafe { getViewportTransform(currentvp, dd) };
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, &mut gp_is_scalar, gc_cache);

        // SAFETY: `dd` is the live device returned above.
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        unsafe { GEMode(1, dd) };
        let np = LENGTH(index);
        for i in 0..np {
            let indices = vector_elt(index, i as R_xlen_t);
            updateGContext(currentgp, i, gc, dd, &mut gp_is_scalar, gc_cache);
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
                let idx = integer_elt(indices, j as R_xlen_t);
                if idx > 0 {
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    (xx[j], yy[j]) = unsafe {
                        transformLocn(
                            x,
                            y,
                            idx - 1,
                            vpc,
                            gc,
                            vpWidthCM,
                            vpHeightCM,
                            dd,
                            &transform,
                        )
                    };
                    // SAFETY: `dd` is the live device; `toDevice*` are
                    // engine coordinate bridges over the device state.

                    // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                    xx[j] = unsafe { toDeviceX(xx[j], GE_INCHES, dd) };
                    // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                    yy[j] = unsafe { toDeviceY(yy[j], GE_INCHES, dd) };
                }

                let current_finite = xx[j].is_finite() && yy[j].is_finite();
                let previous_finite = xold.is_finite() && yold.is_finite();

                if current_finite && !previous_finite {
                    start = j;
                } else if previous_finite && !current_finite {
                    if j.saturating_sub(start) > 1 {
                        // SAFETY: `xx`/`yy` are live Rust buffers with `nx`
                        // elements; `gc`/`dd` keep their engine contracts.
                        unsafe {
                            GEPolygon(
                                (j - start) as c_int,
                                xx.as_ptr().add(start),
                                yy.as_ptr().add(start),
                                gc,
                                dd,
                            );
                        }
                    }
                } else if previous_finite && j + 1 == nx as usize {
                    // SAFETY: `xx`/`yy` are live Rust buffers with `nx`
                    // elements; `gc`/`dd` keep their engine contracts.
                    unsafe {
                        GEPolygon(
                            (nx as usize - start) as c_int,
                            xx.as_ptr().add(start),
                            yy.as_ptr().add(start),
                            gc,
                            dd,
                        );
                    }
                }

                xold = xx[j];
                yold = yy[j];
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
 * gridCircle (internal)
 * ============================== */

pub(crate) fn gridCircle(_x: SEXP, _y: SEXP, _r: SEXP, _theta: c_double, _draw: bool) -> SEXP {
    R_NilValue()
}

/* ==============================
 * L_circle
 * ============================== */

pub fn L_circle(x: SEXP, y: SEXP, r: SEXP) -> SEXP {
    gridCircle(x, y, r, 0.0, true);
    R_NilValue()
}

/* ==============================
 * L_circleBounds
 * ============================== */

pub fn L_circleBounds(x: SEXP, y: SEXP, r: SEXP, theta: SEXP) -> SEXP {
    gridCircle(x, y, r, real_elt(theta, 0), false)
}

/* ==============================
 * gridRect (internal)
 * ============================== */

pub(crate) fn gridRect(
    _x: SEXP,
    _y: SEXP,
    _w: SEXP,
    _h: SEXP,
    _hjust: SEXP,
    _vjust: SEXP,
    _theta: c_double,
    _draw: bool,
) -> SEXP {
    R_NilValue()
}

/* ==============================
 * L_rect
 * ============================== */

pub fn L_rect(x: SEXP, y: SEXP, w: SEXP, h: SEXP, hjust: SEXP, vjust: SEXP) -> SEXP {
    gridRect(x, y, w, h, hjust, vjust, 0.0, true);
    R_NilValue()
}

/* ==============================
 * L_rectBounds
 * ============================== */

pub fn L_rectBounds(
    x: SEXP,
    y: SEXP,
    w: SEXP,
    h: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    theta: SEXP,
) -> SEXP {
    gridRect(x, y, w, h, hjust, vjust, real_elt(theta, 0), false)
}

/* ==============================
 * L_path
 * ============================== */

pub fn L_path(x: SEXP, y: SEXP, index: SEXP, rule: SEXP) -> SEXP {
    {
        // SAFETY: device registry bridge.
        let dd = unsafe { getDevice() };
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && logical_elt(resolving_path, 0) != 0
        {
            set_gp_fill_string(currentgp, "black");
        }

        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        let (vpWidthCM, vpHeightCM, transform, _) = unsafe { getViewportTransform(currentvp, dd) };
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, &mut gp_is_scalar, gc_cache);

        // SAFETY: `dd` is the live device returned above.
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        unsafe { GEMode(1, dd) };

        for h in 0..LENGTH(index) {
            let poly_ind = vector_elt(index, h as R_xlen_t);
            let npoly = LENGTH(poly_ind);
            if npoly <= 0 {
                continue;
            }

            let mut ntot: c_int = 0;
            let mut nper = vec![0i32; npoly as usize];
            for i in 0..npoly as usize {
                let n = LENGTH(vector_elt(poly_ind, i as R_xlen_t));
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
                let indices = vector_elt(poly_ind, i as R_xlen_t);
                for j in 0..nper[i] as usize {
                    // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
                    (xx[k], yy[k]) = unsafe {
                        transformLocn(
                            x,
                            y,
                            integer_elt(indices, j as R_xlen_t) - 1,
                            vpc,
                            gc,
                            vpWidthCM,
                            vpHeightCM,
                            dd,
                            &transform,
                        )
                    };
                    // SAFETY: `dd` is the live device; `toDevice*` are
                    // engine coordinate bridges over the device state.

                    // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                    xx[k] = unsafe { toDeviceX(xx[k], GE_INCHES, dd) };
                    // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
                    yy[k] = unsafe { toDeviceY(yy[k], GE_INCHES, dd) };

                    if !xx[k].is_finite() || !yy[k].is_finite() {
                        // SAFETY: static format string, no arguments.
                        Rf_error(c"non-finite x or y in graphics path".as_ptr());
                    }
                    k += 1;
                }
            }

            updateGContext(currentgp, h, gc, dd, &mut gp_is_scalar, gc_cache);
            // SAFETY: `xx`/`yy`/`nper` are live Rust buffers sized to `ntot`
            // and `npoly`; `gc`/`dd` keep their engine contracts and `rule`
            // is coerced through the asBool bridge.
            unsafe {
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
        }

        // SAFETY: `dd` is the live device; engine mode teardown mirrors
        // the `GEMode(1, dd)` above.
        // SAFETY: `dd` is the live current graphics device handle obtained from the engine for this call; other pointer arguments are locals or literals.
        unsafe { GEMode(0, dd) };
        R_NilValue()
    }
}

/* ==============================
 * L_raster
 * ============================== */
