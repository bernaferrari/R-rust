use super::*;

pub(crate) unsafe fn gridText(
    _label: SEXP,
    _x: SEXP,
    _y: SEXP,
    _hjust: SEXP,
    _vjust: SEXP,
    _rot: SEXP,
    _checkOverlap: SEXP,
    _theta: c_double,
    _draw: bool,
) -> SEXP {
    unsafe { R_NilValue() }
}

/* ==============================
 * L_text
 * ============================== */

pub unsafe fn L_text(
    label: SEXP,
    x: SEXP,
    y: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    rot: SEXP,
    checkOverlap: SEXP,
) -> SEXP {
    unsafe {
        gridText(label, x, y, hjust, vjust, rot, checkOverlap, 0.0, true);
        R_NilValue()
    }
}

/* ==============================
 * L_textBounds
 * ============================== */

pub unsafe fn L_textBounds(
    label: SEXP,
    x: SEXP,
    y: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    rot: SEXP,
    theta: SEXP,
) -> SEXP {
    unsafe {
        let checkOverlap = Rf_allocVector(SEXPTYPE::LGLSXP, 1);
        *LOGICAL(checkOverlap) = 0;
        gridText(
            label,
            x,
            y,
            hjust,
            vjust,
            rot,
            checkOverlap,
            *REAL(theta),
            false,
        )
    }
}

/* ==============================
 * symbolCoords (internal helper)
 * ============================== */

pub(crate) unsafe fn symbolCoords(x: *const f64, y: *const f64, n: c_int, _dd: pGEDevDesc) -> SEXP {
    unsafe {
        let result = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let xs = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _xs_guard = protect(xs);
        let ys = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _ys_guard = protect(ys);
        for i in 0..n as usize {
            *REAL(xs).add(i) = *x.add(i);
            *REAL(ys).add(i) = *y.add(i);
        }
        SET_VECTOR_ELT(result, 0 as R_xlen_t, xs);
        SET_VECTOR_ELT(result, 1 as R_xlen_t, ys);
        result
    }
}

/* ==============================
 * symbolNumCoords (internal helper)
 * ============================== */

pub(crate) unsafe fn symbolNumCoords(pch: c_int, closed: bool) -> c_int {
    let mut result: c_int = 1;
    match pch {
        0 | 1 | 2 => {}
        3 | 4 => {
            if !closed {
                result = 2;
            }
        }
        5 | 6 => {}
        7 => {
            if closed {
                result = 1;
            } else {
                result = 2;
            }
        }
        8 => {
            if !closed {
                result = 4;
            }
        }
        9 | 10 | 12 | 13 => {
            if closed {
                result = 1;
            } else {
                result = 2;
            }
        }
        11 => {
            if closed {
                result = 2;
            }
        }
        14 => {
            if closed {
                result = 2;
            }
        }
        15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 => {}
        _ => {} // intentionally unhandled: unknown arrowhead type
    }
    result
}

/* ==============================
 * gridSymbol
 * ============================== */

pub unsafe fn gridSymbol(
    _x: f64,
    _y: f64,
    _pch: c_int,
    _size: f64,
    _draw: bool,
    _closed: bool,
    _numCoords: c_int,
    _gc: pGEcontext,
    _dd: pGEDevDesc,
) -> SEXP {
    unsafe { R_NilValue() }
}

/* ==============================
 * gridPoints (internal)
 * ============================== */

pub(crate) unsafe fn gridPoints(
    _x: SEXP,
    _y: SEXP,
    _pch: SEXP,
    _size: SEXP,
    _draw: bool,
    _closed: bool,
) -> SEXP {
    unsafe { R_NilValue() }
}

/* ==============================
 * L_points
 * ============================== */

pub unsafe fn L_points(x: SEXP, y: SEXP, pch: SEXP, size: SEXP) -> SEXP {
    unsafe { gridPoints(x, y, pch, size, true, false) }
}

/* ==============================
 * L_pointsPoints
 * ============================== */

pub unsafe fn L_pointsPoints(x: SEXP, y: SEXP, pch: SEXP, size: SEXP, closed: SEXP) -> SEXP {
    unsafe { gridPoints(x, y, pch, size, false, asBool(closed) != 0) }
}

/* ==============================
 * L_clip
 * ============================== */
