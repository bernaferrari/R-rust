use super::*;

pub(crate) unsafe fn rectEdge(
    xmin: c_double,
    ymin: c_double,
    xmax: c_double,
    ymax: c_double,
    theta: c_double,
    edgex: *mut c_double,
    edgey: *mut c_double,
) {
    unsafe {
        let xm = (xmin + xmax) / 2.0;
        let ym = (ymin + ymax) / 2.0;
        let dx = (xmax - xmin) / 2.0;
        let dy = (ymax - ymin) / 2.0;

        if theta == 0.0 {
            *edgex = xmax;
            *edgey = ym;
        } else if theta == 270.0 {
            *edgex = xm;
            *edgey = ymin;
        } else if theta == 180.0 {
            *edgex = xmin;
            *edgey = ym;
        } else if theta == 90.0 {
            *edgex = xm;
            *edgey = ymax;
        } else {
            let cutoff = dy / dx;
            let angle = theta / 180.0 * std::f64::consts::PI;
            let tan_theta = angle.tan();
            let cos_theta = angle.cos();
            let sin_theta = angle.sin();
            if tan_theta.abs() < cutoff {
                if cos_theta > 0.0 {
                    *edgex = xmax;
                    *edgey = ym + tan_theta * dx;
                } else {
                    *edgex = xmin;
                    *edgey = ym - tan_theta * dx;
                }
            } else {
                if sin_theta > 0.0 {
                    *edgey = ymax;
                    *edgex = xm + dy / tan_theta;
                } else {
                    *edgey = ymin;
                    *edgex = xm - dy / tan_theta;
                }
            }
        }
    }
}

pub(crate) unsafe fn circleEdge(
    x: c_double,
    y: c_double,
    r: c_double,
    theta: c_double,
    edgex: *mut c_double,
    edgey: *mut c_double,
) {
    unsafe {
        let angle = theta / 180.0 * std::f64::consts::PI;
        *edgex = x + r * angle.cos();
        *edgey = y + r * angle.sin();
    }
}

pub(crate) unsafe fn polygonEdge(
    x: *const f64,
    y: *const f64,
    n: c_int,
    theta: c_double,
    edgex: *mut c_double,
    edgey: *mut c_double,
) {
    unsafe {
        let mut xmin_val = f64::MAX;
        let mut xmax_val = f64::MIN;
        let mut ymin_val = f64::MAX;
        let mut ymax_val = f64::MIN;
        let angle = theta / 180.0 * std::f64::consts::PI;
        let xm: f64;
        let ym: f64;
        let mut found = 0;

        for i in 0..n as usize {
            if *x.add(i) < xmin_val {
                xmin_val = *x.add(i);
            }
            if *x.add(i) > xmax_val {
                xmax_val = *x.add(i);
            }
            if *y.add(i) < ymin_val {
                ymin_val = *y.add(i);
            }
            if *y.add(i) > ymax_val {
                ymax_val = *y.add(i);
            }
        }
        xm = (xmin_val + xmax_val) / 2.0;
        ym = (ymin_val + ymax_val) / 2.0;

        if (xmin_val - xmax_val).abs() < 1e-6
            || (ymin_val - ymax_val).abs() / (xmin_val - xmax_val).abs() > 1000.0
        {
            *edgex = xmin_val;
            if theta == 90.0 {
                *edgey = ymax_val;
            } else if theta == 270.0 {
                *edgey = ymin_val;
            } else {
                *edgey = ym;
            }
            return;
        }
        if (ymin_val - ymax_val).abs() < 1e-6
            || (xmin_val - xmax_val).abs() / (ymin_val - ymax_val).abs() > 1000.0
        {
            *edgey = ymin_val;
            if theta == 0.0 {
                *edgex = xmax_val;
            } else if theta == 180.0 {
                *edgex = xmin_val;
            } else {
                *edgex = xm;
            }
            return;
        }

        let mut found_i: usize = 0;
        for i in 0..n as usize {
            let v1 = i;
            let v2 = if i + 1 == n as usize { 0 } else { i + 1 };
            let mut vangle1 = (*y.add(v1) - ym).atan2(*x.add(v1) - xm);
            if vangle1 < 0.0 {
                vangle1 += 2.0 * std::f64::consts::PI;
            }
            let mut vangle2 = (*y.add(v2) - ym).atan2(*x.add(v2) - xm);
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
            let mut x2: f64 = 0.0;
            let mut y2: f64 = 0.0;
            rectEdge(
                xmin_val, ymin_val, xmax_val, ymax_val, theta, &mut x2, &mut y2,
            );
            let x3 = *x.add(found_i);
            let y3 = *y.add(found_i);
            let x4 = *x.add(if found_i + 1 == n as usize {
                0
            } else {
                found_i + 1
            });
            let y4 = *y.add(if found_i + 1 == n as usize {
                0
            } else {
                found_i + 1
            });
            let numa = (x4 - x3) * (ym - y3) - (y4 - y3) * (xm - x3);
            let denom = (y4 - y3) * (x2 - xm) - (x4 - x3) * (y2 - ym);
            let ua = numa / denom;
            *edgex = xm + ua * (x2 - xm);
            *edgey = ym + ua * (y2 - ym);
        } else {
            *edgex = xm;
            *edgey = ym;
        }
    }
}

/* ==============================
 * Drawing primitives: arrows
 * ============================== */

pub(crate) unsafe fn drawArrow(
    x: *const f64,
    y: *const f64,
    atype: SEXP,
    i: c_int,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        let nt = LENGTH(atype);
        match *INTEGER(atype).add((i % nt) as usize) {
            1 => GEPolyline(3, x, y, gc, dd),
            2 => GEPolygon(3, x, y, gc, dd),
            _ => {} // intentionally unhandled: unknown arrowhead type
        }
    }
}

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
    vertx: *mut f64,
    verty: *mut f64,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        let l1 = transformWidthtoINCHES(length, i, vpc, gc, vpWidthCM, vpHeightCM, dd).abs();
        let l2 = transformHeighttoINCHES(length, i, vpc, gc, vpWidthCM, vpHeightCM, dd).abs();
        let l = fmin2(l1, l2);
        let na = LENGTH(angle);
        let a = DEG2RAD * *REAL(angle).add((i % na) as usize);
        let xc = x2 - x1;
        let yc = y2 - y1;
        let rot = (yc).atan2(xc);
        *vertx.add(0) = toDeviceX(x1 + l * (rot + a).cos(), GE_INCHES, dd);
        *verty.add(0) = toDeviceY(y1 + l * (rot + a).sin(), GE_INCHES, dd);
        *vertx.add(1) = toDeviceX(x1, GE_INCHES, dd);
        *verty.add(1) = toDeviceY(y1, GE_INCHES, dd);
        *vertx.add(2) = toDeviceX(x1 + l * (rot - a).cos(), GE_INCHES, dd);
        *verty.add(2) = toDeviceY(y1 + l * (rot - a).sin(), GE_INCHES, dd);
    }
}

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
    unsafe {
        let ends = VECTOR_ELT(arrow, GRID_ARROWENDS as R_xlen_t);
        let ne = LENGTH(ends);
        let mut vertx = [0.0f64; 3];
        let mut verty = [0.0f64; 3];
        let mut first = true;
        let mut last = true;

        if n < 2 {
            return;
        }

        match *INTEGER(ends).add((i % ne) as usize) {
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
                fromDeviceX(*x.add(0), GE_INCHES, dd),
                fromDeviceY(*y.add(0), GE_INCHES, dd),
                fromDeviceX(*x.add(1), GE_INCHES, dd),
                fromDeviceY(*y.add(1), GE_INCHES, dd),
                VECTOR_ELT(arrow, GRID_ARROWANGLE as R_xlen_t),
                VECTOR_ELT(arrow, GRID_ARROWLENGTH as R_xlen_t),
                i,
                vpc,
                vpWidthCM,
                vpHeightCM,
                vertx.as_mut_ptr(),
                verty.as_mut_ptr(),
                gc,
                dd,
            );
            drawArrow(
                vertx.as_ptr(),
                verty.as_ptr(),
                VECTOR_ELT(arrow, GRID_ARROWTYPE as R_xlen_t),
                i,
                gc,
                dd,
            );
        }
        if last && end {
            calcArrow(
                fromDeviceX(*x.add((n - 1) as usize), GE_INCHES, dd),
                fromDeviceY(*y.add((n - 1) as usize), GE_INCHES, dd),
                fromDeviceX(*x.add((n - 2) as usize), GE_INCHES, dd),
                fromDeviceY(*y.add((n - 2) as usize), GE_INCHES, dd),
                VECTOR_ELT(arrow, GRID_ARROWANGLE as R_xlen_t),
                VECTOR_ELT(arrow, GRID_ARROWLENGTH as R_xlen_t),
                i,
                vpc,
                vpWidthCM,
                vpHeightCM,
                vertx.as_mut_ptr(),
                verty.as_mut_ptr(),
                gc,
                dd,
            );
            drawArrow(
                vertx.as_ptr(),
                verty.as_ptr(),
                VECTOR_ELT(arrow, GRID_ARROWTYPE as R_xlen_t),
                i,
                gc,
                dd,
            );
        }
    }
}

#[inline]
pub(crate) unsafe fn gp_fill_is_pattern(gp: SEXP) -> bool {
    unsafe {
        let fill = gpFillSXP(gp);
        Rf_inherits(fill, c"GridPattern".as_ptr()) != 0
            || Rf_inherits(fill, c"GridPatternList".as_ptr()) != 0
    }
}

#[inline]
pub(crate) unsafe fn set_gp_fill_string(gp: SEXP, fill: *const c_char) {
    unsafe {
        SET_VECTOR_ELT(gp, GP_FILL as R_xlen_t, Rf_mkString(fill));
    }
}

/* ==============================
 * L_moveTo
 * ============================== */
