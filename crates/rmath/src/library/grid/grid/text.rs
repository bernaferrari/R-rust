use super::*;
use crate::library::grid::{
    alloc_vector, logical_elt, protect, real_elt, set_logical_elt, set_real_elt, set_vector_elt,
};

pub(crate) fn gridText(
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
    R_NilValue()
}

/* ==============================
 * L_text
 * ============================== */

pub fn L_text(
    label: SEXP,
    x: SEXP,
    y: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    rot: SEXP,
    checkOverlap: SEXP,
) -> SEXP {
    gridText(label, x, y, hjust, vjust, rot, checkOverlap, 0.0, true);
    R_NilValue()
}

/* ==============================
 * L_textBounds
 * ============================== */

pub fn L_textBounds(
    label: SEXP,
    x: SEXP,
    y: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    rot: SEXP,
    theta: SEXP,
) -> SEXP {
    let checkOverlap = alloc_vector(SEXPTYPE::LGLSXP, 1);
    set_logical_elt(checkOverlap, 0, 0);
    gridText(
        label,
        x,
        y,
        hjust,
        vjust,
        rot,
        checkOverlap,
        real_elt(theta, 0),
        false,
    )
}

/* ==============================
 * symbolCoords (internal helper)
 * ============================== */

pub(crate) fn symbolCoords(x: &[f64], y: &[f64], n: c_int, _dd: pGEDevDesc) -> SEXP {
    let result = alloc_vector(SEXPTYPE::VECSXP, 2);
    let _result_guard = protect(result);
    let xs = alloc_vector(SEXPTYPE::REALSXP, n);
    let _xs_guard = protect(xs);
    let ys = alloc_vector(SEXPTYPE::REALSXP, n);
    let _ys_guard = protect(ys);
    for i in 0..n as usize {
        set_real_elt(xs, i as R_xlen_t, x[i]);
        set_real_elt(ys, i as R_xlen_t, y[i]);
    }
    set_vector_elt(result, 0 as R_xlen_t, xs);
    set_vector_elt(result, 1 as R_xlen_t, ys);
    result
}

/* ==============================
 * symbolNumCoords (internal helper)
 * ============================== */

pub(crate) fn symbolNumCoords(pch: c_int, closed: bool) -> c_int {
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

pub fn gridSymbol(
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
    R_NilValue()
}

/* ==============================
 * gridPoints (internal)
 * ============================== */

pub(crate) fn gridPoints(
    _x: SEXP,
    _y: SEXP,
    _pch: SEXP,
    _size: SEXP,
    _draw: bool,
    _closed: bool,
) -> SEXP {
    R_NilValue()
}

/* ==============================
 * L_points
 * ============================== */

pub fn L_points(x: SEXP, y: SEXP, pch: SEXP, size: SEXP) -> SEXP {
    gridPoints(x, y, pch, size, true, false)
}

/* ==============================
 * L_pointsPoints
 * ============================== */

pub fn L_pointsPoints(x: SEXP, y: SEXP, pch: SEXP, size: SEXP, closed: SEXP) -> SEXP {
    // SAFETY: `closed` is a live object lent by the R caller; `asBool` is the
    // coercion bridge.
    gridPoints(x, y, pch, size, false, asBool(closed) != 0)
}

/* ==============================
 * L_clip
 * ============================== */
