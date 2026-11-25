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



/* ==============================
 * Drawing primitives: arrows
 * ============================== */

/// Draw one arrowhead through the three vertices in `x`/`y`.
pub(crate) fn drawArrow(
    x: &[f64; 3],
    y: &[f64; 3],
    atype: SEXP,
    i: c_int,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    let nt = LENGTH(atype);
    match crate::library::grid::integer_elt(atype, (i % nt) as R_xlen_t) {
        1 => GEPolyline(x, y, gc, dd),
        2 => GEPolygon(x, y, gc, dd),
        _ => {} // intentionally unhandled: unknown arrowhead type
    }
}

/// Compute arrowhead vertices. `vertx`/`verty` are caller-owned vertex
/// buffers written through safe indexing.
pub(crate) fn calcArrow(
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

    vertx[0] = toDeviceX(x1 + l * (rot + a).cos(), GE_INCHES, dd);
    verty[0] = toDeviceY(y1 + l * (rot + a).sin(), GE_INCHES, dd);
    vertx[1] = toDeviceX(x1, GE_INCHES, dd);
    verty[1] = toDeviceY(y1, GE_INCHES, dd);
    vertx[2] = toDeviceX(x1 + l * (rot - a).cos(), GE_INCHES, dd);
    verty[2] = toDeviceY(y1 + l * (rot - a).sin(), GE_INCHES, dd);
}

/// Draw arrowheads at the start/end of the polyline through `x`/`y`
/// (device coordinates).
pub(crate) fn arrows(
    x: &[f64],
    y: &[f64],
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

    let n = x.len().min(y.len());
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
        calcArrow(
            fromDeviceX(x[0], GE_INCHES, dd),
            fromDeviceY(y[0], GE_INCHES, dd),
            fromDeviceX(x[1], GE_INCHES, dd),
            fromDeviceY(y[1], GE_INCHES, dd),
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
            &vertx,
            &verty,
            crate::library::grid::vector_elt(arrow, GRID_ARROWTYPE as R_xlen_t),
            i,
            gc,
            dd,
        );
    }
    if last && end {
        calcArrow(
            fromDeviceX(x[n - 1], GE_INCHES, dd),
            fromDeviceY(y[n - 1], GE_INCHES, dd),
            fromDeviceX(x[n - 2], GE_INCHES, dd),
            fromDeviceY(y[n - 2], GE_INCHES, dd),
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
            &vertx,
            &verty,
            crate::library::grid::vector_elt(arrow, GRID_ARROWTYPE as R_xlen_t),
            i,
            gc,
            dd,
        );
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
