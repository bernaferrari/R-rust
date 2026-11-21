use super::*;

pub(crate) fn rectEdge(
    xmin: c_double,
    ymin: c_double,
    xmax: c_double,
    ymax: c_double,
    theta: c_double,
) -> (c_double, c_double) {
    let xm = (xmin + xmax) / 2.0;
    let ym = (ymin + ymax) / 2.0;
    let dx = (xmax - xmin) / 2.0;
    let dy = (ymax - ymin) / 2.0;

    if theta == 0.0 {
        return (xmax, ym);
    }
    if theta == 270.0 {
        return (xm, ymin);
    }
    if theta == 180.0 {
        return (xmin, ym);
    }
    if theta == 90.0 {
        return (xm, ymax);
    }
    let cutoff = dy / dx;
    let angle = theta / 180.0 * std::f64::consts::PI;
    let tan_theta = angle.tan();
    let cos_theta = angle.cos();
    let sin_theta = angle.sin();
    if tan_theta.abs() < cutoff {
        if cos_theta > 0.0 {
            (xmax, ym + tan_theta * dx)
        } else {
            (xmin, ym - tan_theta * dx)
        }
    } else if sin_theta > 0.0 {
        (xm + dy / tan_theta, ymax)
    } else {
        (xm - dy / tan_theta, ymin)
    }
}

pub(crate) fn circleEdge(
    x: c_double,
    y: c_double,
    r: c_double,
    theta: c_double,
) -> (c_double, c_double) {
    let angle = theta / 180.0 * std::f64::consts::PI;
    (x + r * angle.cos(), y + r * angle.sin())
}

pub(crate) fn polygonEdge(x: &[f64], y: &[f64], theta: c_double) -> (c_double, c_double) {
    let n = x.len() as c_int;
    let mut xmin_val = f64::MAX;
    let mut xmax_val = f64::MIN;
    let mut ymin_val = f64::MAX;
    let mut ymax_val = f64::MIN;
    let angle = theta / 180.0 * std::f64::consts::PI;
    let xm: f64;
    let ym: f64;
    let mut found = 0;

    for i in 0..n as usize {
        if x[i] < xmin_val {
            xmin_val = x[i];
        }
        if x[i] > xmax_val {
            xmax_val = x[i];
        }
        if y[i] < ymin_val {
            ymin_val = y[i];
        }
        if y[i] > ymax_val {
            ymax_val = y[i];
        }
    }
    xm = (xmin_val + xmax_val) / 2.0;
    ym = (ymin_val + ymax_val) / 2.0;

    if (xmin_val - xmax_val).abs() < 1e-6
        || (ymin_val - ymax_val).abs() / (xmin_val - xmax_val).abs() > 1000.0
    {
        return if theta == 90.0 {
            (xmin_val, ymax_val)
        } else if theta == 270.0 {
            (xmin_val, ymin_val)
        } else {
            (xmin_val, ym)
        };
    }
    if (ymin_val - ymax_val).abs() < 1e-6
        || (xmin_val - xmax_val).abs() / (ymin_val - ymax_val).abs() > 1000.0
    {
        return if theta == 0.0 {
            (xmax_val, ymin_val)
        } else if theta == 180.0 {
            (xmin_val, ymin_val)
        } else {
            (xm, ymin_val)
        };
    }

    let mut found_i: usize = 0;
    for i in 0..n as usize {
        let v1 = i;
        let v2 = if i + 1 == n as usize { 0 } else { i + 1 };
        let mut vangle1 = (y[v1] - ym).atan2(x[v1] - xm);
        if vangle1 < 0.0 {
            vangle1 += 2.0 * std::f64::consts::PI;
        }
        let mut vangle2 = (y[v2] - ym).atan2(x[v2] - xm);
        if vangle2 < 0.0 {
            vangle2 += 2.0 * std::f64::consts::PI;
        }

        if (vangle1 >= vangle2 && vangle1 >= angle && vangle2 <= angle)
            || (vangle1 < vangle2
                && ((vangle1 >= angle && 0.0 <= angle)
                    || (vangle2 <= angle && 2.0 * std::f64::consts::PI >= angle)))
        {
            found = 1;
            found_i = i;
            break;
        }
    }

    if found != 0 {
        let (x2, y2) = rectEdge(xmin_val, ymin_val, xmax_val, ymax_val, theta);
        let x3 = x[found_i];
        let y3 = y[found_i];
        let next = if found_i + 1 == n as usize {
            0
        } else {
            found_i + 1
        };
        let x4 = x[next];
        let y4 = y[next];
        let numa = (x4 - x3) * (ym - y3) - (y4 - y3) * (xm - x3);
        let denom = (y4 - y3) * (x2 - xm) - (x4 - x3) * (y2 - ym);
        let ua = numa / denom;
        (xm + ua * (x2 - xm), ym + ua * (y2 - ym))
    } else {
        (xm, ym)
    }
}

/* ==============================
 * Drawing primitives: arrows
 * ============================== */

/// Draw one arrowhead. `x`/`y` are raw engine coordinate buffers lent for
/// the `GEPolyline`/`GEPolygon` bridge calls below.
///
/// # Safety
/// `x` and `y` must point to at least 3 readable `f64` elements; `gc` and
/// `dd` keep their engine/device contracts.
pub(crate) unsafe fn drawArrow(
    x: *const f64,
    y: *const f64,
    atype: SEXP,
    i: c_int,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    let nt = LENGTH(atype);
    match crate::library::grid::integer_elt(atype, (i % nt) as R_xlen_t) {
        // SAFETY: `x`/`y`/`gc`/`dd` are the raw engine buffers described
        // above; the callee contract is unchanged.
        1 => unsafe { GEPolyline(3, x, y, gc, dd) },
        2 => unsafe { GEPolygon(3, x, y, gc, dd) },
        _ => {} // intentionally unhandled: unknown arrowhead type
    }
}

/// Compute arrowhead vertices. `vertx`/`verty` are caller-owned vertex
/// buffers written through safe indexing.
///
/// # Safety
/// `gc` and `dd` keep their engine/device contracts for the `toDevice*`
/// coordinate bridges below.
pub(crate) unsafe fn calcArrow(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    angle: SEXP,
    length: SEXP,
    i: c_int,
    vpc: LViewportContext,
    vpWidthCM: f64,
    vpHeightCM: f64,
    vertx: &mut [f64; 3],
    verty: &mut [f64; 3],
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    let l1 = transformWidthtoINCHES(length, i, vpc, gc, vpWidthCM, vpHeightCM, dd).abs();
    let l2 = transformHeighttoINCHES(length, i, vpc, gc, vpWidthCM, vpHeightCM, dd).abs();
    let l = fmin2(l1, l2);
    let na = LENGTH(angle);
    let a = DEG2RAD * crate::library::grid::real_elt(angle, (i % na) as R_xlen_t);
    let xc = x2 - x1;
    let yc = y2 - y1;
    let rot = (yc).atan2(xc);
    // SAFETY: `dd` is the live device; `toDevice*` are engine coordinate
    // bridges over the device state.
    unsafe {
        vertx[0] = toDeviceX(x1 + l * (rot + a).cos(), GE_INCHES, dd);
        verty[0] = toDeviceY(y1 + l * (rot + a).sin(), GE_INCHES, dd);
        vertx[1] = toDeviceX(x1, GE_INCHES, dd);
        verty[1] = toDeviceY(y1, GE_INCHES, dd);
        vertx[2] = toDeviceX(x1 + l * (rot - a).cos(), GE_INCHES, dd);
        verty[2] = toDeviceY(y1 + l * (rot - a).sin(), GE_INCHES, dd);
    }
}

/// Draw arrowheads at the start/end of a polyline. `x`/`y` are raw engine
/// coordinate buffers with `n` readable elements each.
///
/// # Safety
/// `x` and `y` must point to at least `n` readable `f64` elements; `gc` and
/// `dd` keep their engine/device contracts.
pub(crate) unsafe fn arrows(
    x: *const f64,
    y: *const f64,
    n: c_int,
    arrow: SEXP,
    i: c_int,
    start: bool,
    end: bool,
    vpc: LViewportContext,
    vpWidthCM: f64,
    vpHeightCM: f64,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    let ends = crate::library::grid::vector_elt(arrow, GRID_ARROWENDS as R_xlen_t);
    let ne = LENGTH(ends);
    let mut vertx = [0.0f64; 3];
    let mut verty = [0.0f64; 3];
    let mut first = true;
    let mut last = true;

    if n < 2 {
        return;
    }

    match crate::library::grid::integer_elt(ends, (i % ne) as R_xlen_t) {
        2 => {
            first = false;
        }
        1 => {
            last = false;
        }
        _ => {} // intentionally unhandled: unknown grid boundary value
    }

    if first && start {
        // SAFETY: `x`/`y`/`gc`/`dd` are the raw engine buffers described
        // above; `vertx`/`verty` are fully initialized local buffers.
        unsafe {
            calcArrow(
                fromDeviceX(*x, GE_INCHES, dd),
                fromDeviceY(*y, GE_INCHES, dd),
                fromDeviceX(*x.add(1), GE_INCHES, dd),
                fromDeviceY(*y.add(1), GE_INCHES, dd),
                crate::library::grid::vector_elt(arrow, GRID_ARROWANGLE as R_xlen_t),
                crate::library::grid::vector_elt(arrow, GRID_ARROWLENGTH as R_xlen_t),
                i,
                vpc,
                vpWidthCM,
                vpHeightCM,
                &mut vertx,
                &mut verty,
                gc,
                dd,
            );
            drawArrow(
                vertx.as_ptr(),
                verty.as_ptr(),
                crate::library::grid::vector_elt(arrow, GRID_ARROWTYPE as R_xlen_t),
                i,
                gc,
                dd,
            );
        }
    }
    if last && end {
        // SAFETY: see the `first && start` branch; `x`/`y` hold `n` elements.
        unsafe {
            calcArrow(
                fromDeviceX(*x.add((n - 1) as usize), GE_INCHES, dd),
                fromDeviceY(*y.add((n - 1) as usize), GE_INCHES, dd),
                fromDeviceX(*x.add((n - 2) as usize), GE_INCHES, dd),
                fromDeviceY(*y.add((n - 2) as usize), GE_INCHES, dd),
                crate::library::grid::vector_elt(arrow, GRID_ARROWANGLE as R_xlen_t),
                crate::library::grid::vector_elt(arrow, GRID_ARROWLENGTH as R_xlen_t),
                i,
                vpc,
                vpWidthCM,
                vpHeightCM,
                &mut vertx,
                &mut verty,
                gc,
                dd,
            );
            drawArrow(
                vertx.as_ptr(),
                verty.as_ptr(),
                crate::library::grid::vector_elt(arrow, GRID_ARROWTYPE as R_xlen_t),
                i,
                gc,
                dd,
            );
        }
    }
}

#[inline]
pub(crate) fn gp_fill_is_pattern(gp: SEXP) -> bool {
    let fill = gpFillSXP(gp);
    crate::library::grid::inherits(fill, c"GridPattern")
        || crate::library::grid::inherits(fill, c"GridPatternList")
}

#[inline]
pub(crate) fn set_gp_fill_string(gp: SEXP, fill: &str) {
    crate::library::grid::set_vector_elt(
        gp,
        GP_FILL as R_xlen_t,
        crate::library::grid::mk_string(fill),
    );
}

/* ==============================
 * L_moveTo
 * ============================== */
