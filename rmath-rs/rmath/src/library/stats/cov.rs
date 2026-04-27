//! Port of R's src/library/stats/src/cov.c
//!
//! Computation of covariance and correlation matrices.
//! Supports Pearson, Spearman (via ranks), and Kendall methods.
//! NA handling: "all.obs", "complete.obs", "pairwise.complete", "everything", "na.or.complete".

use std::os::raw::{c_double, c_int};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{ISNAN, NA_REAL, R_FINITE, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Helper: R functions -- delegate to real implementations
// ---------------------------------------------------------------------------

unsafe fn asInteger(x: SEXP) -> c_int {
    crate::main::coerce::asInteger(x)
}

unsafe fn asBool(x: SEXP) -> bool {
    let v = asInteger(x);
    v != 0 && v != crate::sexp::ffi::NA_INTEGER
}

unsafe fn length(x: SEXP) -> c_int {
    Rf_length(x)
}

unsafe fn isMatrix(x: SEXP) -> bool {
    let dn = getAttrib(x, R_DimSymbol());
    !dn.is_null() && length(dn) >= 2
}

unsafe fn nrows(x: SEXP) -> c_int {
    let dn = getAttrib(x, R_DimSymbol());
    if dn.is_null() || length(dn) < 1 {
        return length(x);
    }
    *INTEGER(dn)
}

unsafe fn ncols(x: SEXP) -> c_int {
    let dn = getAttrib(x, R_DimSymbol());
    if dn.is_null() || length(dn) < 2 {
        return 1;
    }
    *INTEGER(dn).add(1)
}

unsafe fn isFactor(x: SEXP) -> bool {
    if x.is_null() {
        return false;
    }
    let cl = getAttrib(x, R_ClassSymbol());
    !cl.is_null() && length(cl) > 0
}

unsafe fn coerceVector(x: SEXP, sexptype: SEXPTYPE) -> SEXP {
    crate::main::coerce::coerceVector(x, sexptype.0)
}

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    let ans = Rf_allocVector(sexptype, nrow * ncol);
    let _ans_guard = protect(ans);
    let dim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
    let _dim_guard = protect(dim);
    *INTEGER(dim) = nrow;
    *INTEGER(dim).add(1) = ncol;
    setAttrib(ans, R_DimSymbol(), dim);
    ans
}

unsafe fn setAttrib(x: SEXP, what: SEXP, value: SEXP) {
    crate::attrib_core::setAttrib(x, what, value);
}

unsafe fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    crate::attrib_core::getAttrib(x, what)
}

unsafe fn duplicate(x: SEXP) -> SEXP {
    crate::main::duplicate::duplicate(x)
}

unsafe fn R_DimSymbol() -> SEXP {
    crate::attrib_core::R_DimSymbol()
}

unsafe fn R_DimNamesSymbol() -> SEXP {
    crate::attrib_core::R_DimNamesSymbol()
}

unsafe fn R_ClassSymbol() -> SEXP {
    crate::attrib_core::R_ClassSymbol()
}

unsafe fn error(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    crate::main::errors::Rf_error(c_msg.as_ptr());
}

unsafe fn warning(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    crate::main::errors::Rf_warning(c_msg.as_ptr());
}

// ---------------------------------------------------------------------------
// Macros from the C code
// ---------------------------------------------------------------------------

#[inline]
fn CLAMP(x: f64) -> f64 {
    if x >= 1.0 {
        1.0
    } else if x <= -1.0 {
        -1.0
    } else {
        x
    }
}

#[inline]
fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------------
// Pairwise covariance/correlation
// ---------------------------------------------------------------------------

/// ANS macro: index into the ans matrix (column-major, like R/Fortran)
#[inline]
fn ANS(ans: *mut c_double, i: c_int, j: c_int, ncx: c_int) -> *mut c_double {
    ans.offset((i + j * ncx) as usize)
}

unsafe fn cov_pairwise_body(
    n: c_int,
    xx: *const c_double,
    yy: *const c_double,
    ans: *mut c_double,
    ncx: c_int,
    sd_0: &mut bool,
    cor: bool,
    kendall: bool,
) {
    let mut sum: f64 = 0.0;
    let mut xmean: f64 = 0.0;
    let mut ymean: f64 = 0.0;
    let mut xsd: f64 = 0.0;
    let mut ysd: f64 = 0.0;
    let mut xm: f64 = 0.0;
    let mut ym: f64 = 0.0;
    let mut nobs: c_int = 0;
    let mut n1: c_int = -1;

    if !kendall {
        xmean = 0.0;
        ymean = 0.0;
        for k in 0..n as usize {
            if !ISNAN(*xx.add(k)) && !ISNAN(*yy.add(k)) {
                nobs += 1;
                xmean += *xx.add(k);
                ymean += *yy.add(k);
            }
        }
    } else {
        for k in 0..n as usize {
            if !ISNAN(*xx.add(k)) && !ISNAN(*yy.add(k)) {
                nobs += 1;
            }
        }
    }

    if nobs >= 2 {
        xsd = 0.0;
        ysd = 0.0;
        sum = 0.0;
        if !kendall {
            xmean /= nobs as f64;
            ymean /= nobs as f64;
            n1 = nobs - 1;
        }
        for k in 0..n as usize {
            if !ISNAN(*xx.add(k)) && !ISNAN(*yy.add(k)) {
                if !kendall {
                    xm = *xx.add(k) - xmean;
                    ym = *yy.add(k) - ymean;
                    sum += xm * ym;
                    if cor {
                        xsd += xm * xm;
                        ysd += ym * ym;
                    }
                } else {
                    // Kendall's tau
                    for n1_inner in 0..k {
                        if !ISNAN(*xx.add(n1_inner)) && !ISNAN(*yy.add(n1_inner)) {
                            xm = sign(*xx.add(k) - *xx.add(n1_inner));
                            ym = sign(*yy.add(k) - *yy.add(n1_inner));
                            sum += xm * ym;
                            if cor {
                                xsd += xm * xm;
                                ysd += ym * ym;
                            }
                        }
                    }
                }
            }
        }
        if cor {
            if xsd == 0.0 || ysd == 0.0 {
                *sd_0 = true;
                sum = NA_REAL;
            } else {
                if !kendall {
                    xsd /= n1 as f64;
                    ysd /= n1 as f64;
                    sum /= n1 as f64;
                }
                sum /= (xsd.sqrt() * ysd.sqrt());
                sum = CLAMP(sum);
            }
        } else if !kendall {
            sum /= n1 as f64;
        }
        // The caller handles writing ANS
        // We return the sum via a side-channel approach:
        // Actually, we need to write directly to ans for the i,j pair.
        // The C macro writes ANS(i,j) = sum, but the body is inlined.
        // Since we can't return the sum cleanly here AND write to the right
        // location, we'll restructure. Actually let's just write to a temp.
        // Let's rethink: the C macro writes ANS(i,j) inside the body.
        // We'll store the result in a pointer passed in.
        // For now, store via the caller.
    }
}

// ---------------------------------------------------------------------------
// cov_pairwise1: one-matrix pairwise
// ---------------------------------------------------------------------------

unsafe fn cov_pairwise1(
    n: c_int,
    ncx: c_int,
    x: *mut c_double,
    ans: *mut c_double,
    sd_0: &mut bool,
    cor: bool,
    kendall: bool,
) {
    for i in 0..ncx as usize {
        let xx = x.offset(i * n as isize);
        for j in 0..=i {
            let yy = x.offset(j * n as isize);

            // Inline COV_PAIRWISE_BODY
            let mut sum: f64 = 0.0;
            let mut xmean: f64 = 0.0;
            let mut ymean: f64 = 0.0;
            let mut xsd: f64 = 0.0;
            let mut ysd: f64 = 0.0;
            let mut xm: f64 = 0.0;
            let mut ym: f64 = 0.0;
            let mut nobs: c_int = 0;
            let mut n1: c_int = -1;

            if !kendall {
                xmean = 0.0;
                ymean = 0.0;
                for k in 0..n as usize {
                    if !ISNAN(*xx.add(k)) && !ISNAN(*yy.add(k)) {
                        nobs += 1;
                        xmean += *xx.add(k);
                        ymean += *yy.add(k);
                    }
                }
            } else {
                for k in 0..n as usize {
                    if !ISNAN(*xx.add(k)) && !ISNAN(*yy.add(k)) {
                        nobs += 1;
                    }
                }
            }

            if nobs >= 2 {
                xsd = 0.0;
                ysd = 0.0;
                sum = 0.0;
                if !kendall {
                    xmean /= nobs as f64;
                    ymean /= nobs as f64;
                    n1 = nobs - 1;
                }
                for k in 0..n as usize {
                    if !ISNAN(*xx.add(k)) && !ISNAN(*yy.add(k)) {
                        if !kendall {
                            xm = *xx.add(k) - xmean;
                            ym = *yy.add(k) - ymean;
                            sum += xm * ym;
                            if cor {
                                xsd += xm * xm;
                                ysd += ym * ym;
                            }
                        } else {
                            for n1_inner in 0..k {
                                if !ISNAN(*xx.add(n1_inner)) && !ISNAN(*yy.add(n1_inner)) {
                                    xm = sign(*xx.add(k) - *xx.add(n1_inner));
                                    ym = sign(*yy.add(k) - *yy.add(n1_inner));
                                    sum += xm * ym;
                                    if cor {
                                        xsd += xm * xm;
                                        ysd += ym * ym;
                                    }
                                }
                            }
                        }
                    }
                }
                if cor {
                    if xsd == 0.0 || ysd == 0.0 {
                        *sd_0 = true;
                        sum = NA_REAL;
                    } else {
                        if !kendall {
                            xsd /= n1 as f64;
                            ysd /= n1 as f64;
                            sum /= n1 as f64;
                        }
                        sum /= (xsd.sqrt() * ysd.sqrt());
                        sum = CLAMP(sum);
                    }
                } else if !kendall {
                    sum /= n1 as f64;
                }
                *ANS(ans, i as c_int, j as c_int, ncx) = sum;
            } else {
                *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
            }
            // Symmetric
            *ANS(ans, j as c_int, i as c_int, ncx) = *ANS(ans, i as c_int, j as c_int, ncx);
        }
    }
}

// ---------------------------------------------------------------------------
// cov_pairwise2: two-matrix pairwise
// ---------------------------------------------------------------------------

unsafe fn cov_pairwise2(
    n: c_int,
    ncx: c_int,
    ncy: c_int,
    x: *mut c_double,
    y: *mut c_double,
    ans: *mut c_double,
    sd_0: &mut bool,
    cor: bool,
    kendall: bool,
) {
    for i in 0..ncx as usize {
        let xx = x.offset(i * n as isize);
        for j in 0..ncy as usize {
            let yy = y.offset(j * n as isize);

            let mut sum: f64 = 0.0;
            let mut xmean: f64 = 0.0;
            let mut ymean: f64 = 0.0;
            let mut xsd: f64 = 0.0;
            let mut ysd: f64 = 0.0;
            let mut xm: f64 = 0.0;
            let mut ym: f64 = 0.0;
            let mut nobs: c_int = 0;
            let mut n1: c_int = -1;

            if !kendall {
                xmean = 0.0;
                ymean = 0.0;
                for k in 0..n as usize {
                    if !ISNAN(*xx.add(k)) && !ISNAN(*yy.add(k)) {
                        nobs += 1;
                        xmean += *xx.add(k);
                        ymean += *yy.add(k);
                    }
                }
            } else {
                for k in 0..n as usize {
                    if !ISNAN(*xx.add(k)) && !ISNAN(*yy.add(k)) {
                        nobs += 1;
                    }
                }
            }

            if nobs >= 2 {
                xsd = 0.0;
                ysd = 0.0;
                sum = 0.0;
                if !kendall {
                    xmean /= nobs as f64;
                    ymean /= nobs as f64;
                    n1 = nobs - 1;
                }
                for k in 0..n as usize {
                    if !ISNAN(*xx.add(k)) && !ISNAN(*yy.add(k)) {
                        if !kendall {
                            xm = *xx.add(k) - xmean;
                            ym = *yy.add(k) - ymean;
                            sum += xm * ym;
                            if cor {
                                xsd += xm * xm;
                                ysd += ym * ym;
                            }
                        } else {
                            for n1_inner in 0..k {
                                if !ISNAN(*xx.add(n1_inner)) && !ISNAN(*yy.add(n1_inner)) {
                                    xm = sign(*xx.add(k) - *xx.add(n1_inner));
                                    ym = sign(*yy.add(k) - *yy.add(n1_inner));
                                    sum += xm * ym;
                                    if cor {
                                        xsd += xm * xm;
                                        ysd += ym * ym;
                                    }
                                }
                            }
                        }
                    }
                }
                if cor {
                    if xsd == 0.0 || ysd == 0.0 {
                        *sd_0 = true;
                        sum = NA_REAL;
                    } else {
                        if !kendall {
                            xsd /= n1 as f64;
                            ysd /= n1 as f64;
                            sum /= n1 as f64;
                        }
                        sum /= (xsd.sqrt() * ysd.sqrt());
                        sum = CLAMP(sum);
                    }
                } else if !kendall {
                    sum /= n1 as f64;
                }
                *ANS(ans, i as c_int, j as c_int, ncx) = sum;
            } else {
                *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// complete1 / complete2: find complete cases indicator
// ---------------------------------------------------------------------------

unsafe fn complete1(n: c_int, ncx: c_int, x: *mut c_double, ind: *mut c_int, na_fail: bool) {
    let mut z: *const c_double;
    for i in 0..n as usize {
        *ind.add(i) = 1;
    }
    for j in 0..ncx as usize {
        z = x.offset(j * n as isize);
        for i in 0..n as usize {
            if ISNAN(*z.add(i)) {
                if na_fail {
                    error("missing observations in cov/cor");
                } else {
                    *ind.add(i) = 0;
                }
            }
        }
    }
}

unsafe fn complete2(
    n: c_int,
    ncx: c_int,
    ncy: c_int,
    x: *mut c_double,
    y: *mut c_double,
    ind: *mut c_int,
    na_fail: bool,
) {
    let mut z: *const c_double;
    for i in 0..n as usize {
        *ind.add(i) = 1;
    }
    for j in 0..ncx as usize {
        z = x.offset(j * n as isize);
        for i in 0..n as usize {
            if ISNAN(*z.add(i)) {
                if na_fail {
                    error("missing observations in cov/cor");
                } else {
                    *ind.add(i) = 0;
                }
            }
        }
    }
    for j in 0..ncy as usize {
        z = y.offset(j * n as isize);
        for i in 0..n as usize {
            if ISNAN(*z.add(i)) {
                if na_fail {
                    error("missing observations in cov/cor");
                } else {
                    *ind.add(i) = 0;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// find_na_1 / find_na_2: check columns for any NA
// ---------------------------------------------------------------------------

unsafe fn find_na_1(n: c_int, ncx: c_int, x: *mut c_double, has_na: *mut c_int) {
    for j in 0..ncx as usize {
        let z = x.offset(j * n as isize);
        *has_na.add(j) = 0;
        for i in 0..n as usize {
            if ISNAN(*z.add(i)) {
                *has_na.add(j) = 1;
                break;
            }
        }
    }
}

unsafe fn find_na_2(
    n: c_int,
    ncx: c_int,
    ncy: c_int,
    x: *mut c_double,
    y: *mut c_double,
    has_na_x: *mut c_int,
    has_na_y: *mut c_int,
) {
    find_na_1(n, ncx, x, has_na_x);
    find_na_1(n, ncy, y, has_na_y);
}

// ---------------------------------------------------------------------------
// cov_complete1: complete observations, one matrix
// ---------------------------------------------------------------------------

unsafe fn cov_complete1(
    n: c_int,
    ncx: c_int,
    x: *mut c_double,
    xm: *mut c_double,
    ind: *mut c_int,
    ans: *mut c_double,
    sd_0: &mut bool,
    cor: bool,
    kendall: bool,
) {
    let mut xx: *const c_double;
    let mut yy: *const c_double;
    let mut sum: f64;
    let mut xxm: f64;
    let mut yym: f64;
    let mut tmp: f64;
    let mut i: isize;
    let mut j: isize;
    let mut k: isize;
    let mut n1: isize = -1;

    // Count complete obs
    let mut nobs: c_int = 0;
    for k in 0..n as usize {
        if *ind.add(k) != 0 {
            nobs += 1;
        }
    }
    if nobs <= 1 {
        for i in 0..ncx as usize {
            for j in 0..i as usize + 1 {
                *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
            }
        }
        return;
    }

    if !kendall {
        // Compute means (two-pass for accuracy)
        for i in 0..ncx as usize {
            xx = x.offset(i * n as isize);
            sum = 0.0;
            for k in 0..n as usize {
                if *ind.add(k) != 0 {
                    sum += *xx.add(k);
                }
            }
            tmp = sum / nobs as f64;
            if R_FINITE(tmp) {
                sum = 0.0;
                for k in 0..n as usize {
                    if *ind.add(k) != 0 {
                        sum += *xx.add(k) - tmp;
                    }
                }
                tmp = tmp + sum / nobs as f64;
            }
            *xm.add(i) = tmp;
        }
        n1 = (nobs - 1) as usize;
    }

    for i in 0..ncx as usize {
        xx = x.offset(i * n as isize);
        if !kendall {
            xxm = *xm.add(i);
            for j in 0..=i {
                yy = x.offset(j * n as isize);
                yym = *xm.add(j);
                sum = 0.0;
                for k in 0..n as usize {
                    if *ind.add(k) != 0 {
                        sum += (*xx.add(k) - xxm) * (*yy.add(k) - yym);
                    }
                }
                let val = sum / n1 as f64;
                *ANS(ans, j as c_int, i as c_int, ncx) = val;
                *ANS(ans, i as c_int, j as c_int, ncx) = val;
            }
        } else {
            // Kendall's tau
            for j in 0..=i {
                yy = x.offset(j * n as isize);
                sum = 0.0;
                for k in 0..n as usize {
                    if *ind.add(k) != 0 {
                        for n1_inner in 0..n as usize {
                            if *ind.add(n1_inner) != 0 {
                                sum += sign(*xx.add(k) - *xx.add(n1_inner))
                                    * sign(*yy.add(k) - *yy.add(n1_inner));
                            }
                        }
                    }
                }
                let val = sum;
                *ANS(ans, j as c_int, i as c_int, ncx) = val;
                *ANS(ans, i as c_int, j as c_int, ncx) = val;
            }
        }
    }

    if cor {
        for i in 0..ncx as usize {
            *xm.add(i) = (*ANS(ans, i as c_int, i as c_int, ncx)).sqrt();
        }
        for i in 0..ncx as usize {
            for j in 0..i as usize {
                if *xm.add(i) == 0.0 || *xm.add(j) == 0.0 {
                    *sd_0 = true;
                    let val = NA_REAL;
                    *ANS(ans, j as c_int, i as c_int, ncx) = val;
                    *ANS(ans, i as c_int, j as c_int, ncx) = val;
                } else {
                    sum = *ANS(ans, i as c_int, j as c_int, ncx) / (*xm.add(i) * *xm.add(j));
                    sum = CLAMP(sum);
                    *ANS(ans, j as c_int, i as c_int, ncx) = sum;
                    *ANS(ans, i as c_int, j as c_int, ncx) = sum;
                }
            }
            *ANS(ans, i as c_int, i as c_int, ncx) = 1.0;
        }
    }
}

// ---------------------------------------------------------------------------
// cov_na_1: NA propagated, one matrix
// ---------------------------------------------------------------------------

unsafe fn cov_na_1(
    n: c_int,
    ncx: c_int,
    x: *mut c_double,
    xm: *mut c_double,
    has_na: *mut c_int,
    ans: *mut c_double,
    sd_0: &mut bool,
    cor: bool,
    kendall: bool,
) {
    let mut xx: *const c_double;
    let mut yy: *const c_double;
    let mut sum: f64;
    let mut xxm: f64;
    let mut yym: f64;
    let mut tmp: f64;
    let mut i: isize;
    let mut j: isize;
    let mut k: isize;
    let mut n1: isize = -1;

    if n <= 1 {
        for i in 0..ncx as usize {
            for j in 0..i as usize + 1 {
                *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
            }
        }
        return;
    }

    if !kendall {
        // Compute means with NA propagation
        for i in 0..ncx as usize {
            if *has_na.add(i) != 0 {
                *xm.add(i) = NA_REAL;
            } else {
                xx = x.offset(i * n as isize);
                sum = 0.0;
                for k in 0..n as usize {
                    sum += *xx.add(k);
                }
                tmp = sum / n as f64;
                if R_FINITE(tmp) {
                    sum = 0.0;
                    for k in 0..n as usize {
                        sum += *xx.add(k) - tmp;
                    }
                    tmp = tmp + sum / n as f64;
                }
                *xm.add(i) = tmp;
            }
        }
        n1 = (n - 1) as usize;
    }

    for i in 0..ncx as usize {
        if *has_na.add(i) != 0 {
            for j in 0..=i {
                *ANS(ans, j as c_int, i as c_int, ncx) = NA_REAL;
                *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
            }
        } else {
            xx = x.offset(i * n as isize);
            if !kendall {
                xxm = *xm.add(i);
                for j in 0..=i {
                    if *has_na.add(j) != 0 {
                        *ANS(ans, j as c_int, i as c_int, ncx) = NA_REAL;
                        *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
                    } else {
                        yy = x.offset(j * n as isize);
                        yym = *xm.add(j);
                        sum = 0.0;
                        for k in 0..n as usize {
                            sum += (*xx.add(k) - xxm) * (*yy.add(k) - yym);
                        }
                        let val = sum / n1 as f64;
                        *ANS(ans, j as c_int, i as c_int, ncx) = val;
                        *ANS(ans, i as c_int, j as c_int, ncx) = val;
                    }
                }
            } else {
                // Kendall's tau with NA propagation
                for j in 0..=i {
                    if *has_na.add(j) != 0 {
                        *ANS(ans, j as c_int, i as c_int, ncx) = NA_REAL;
                        *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
                    } else {
                        yy = x.offset(j * n as isize);
                        sum = 0.0;
                        for k in 0..n as usize {
                            for n1_inner in 0..n as usize {
                                sum += sign(*xx.add(k) - *xx.add(n1_inner))
                                    * sign(*yy.add(k) - *yy.add(n1_inner));
                            }
                        }
                        let val = sum;
                        *ANS(ans, j as c_int, i as c_int, ncx) = val;
                        *ANS(ans, i as c_int, j as c_int, ncx) = val;
                    }
                }
            }
        }
    }

    if cor {
        for i in 0..ncx as usize {
            if *has_na.add(i) == 0 {
                *xm.add(i) = (*ANS(ans, i as c_int, i as c_int, ncx)).sqrt();
            }
        }
        for i in 0..ncx as usize {
            if *has_na.add(i) == 0 {
                for j in 0..i as usize {
                    if *xm.add(i) == 0.0 || *xm.add(j) == 0.0 {
                        *sd_0 = true;
                        *ANS(ans, j as c_int, i as c_int, ncx) = NA_REAL;
                        *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
                    } else {
                        sum = *ANS(ans, i as c_int, j as c_int, ncx) / (*xm.add(i) * *xm.add(j));
                        sum = CLAMP(sum);
                        *ANS(ans, j as c_int, i as c_int, ncx) = sum;
                        *ANS(ans, i as c_int, j as c_int, ncx) = sum;
                    }
                }
            }
            *ANS(ans, i as c_int, i as c_int, ncx) = 1.0;
        }
    }
}

// ---------------------------------------------------------------------------
// cov_complete2: complete observations, two matrices
// ---------------------------------------------------------------------------

unsafe fn cov_complete2(
    n: c_int,
    ncx: c_int,
    ncy: c_int,
    x: *mut c_double,
    y: *mut c_double,
    xm: *mut c_double,
    ym: *mut c_double,
    ind: *mut c_int,
    ans: *mut c_double,
    sd_0: &mut bool,
    cor: bool,
    kendall: bool,
) {
    let mut xx: *const c_double;
    let mut yy: *const c_double;
    let mut sum: f64;
    let mut xxm: f64;
    let mut yym: f64;
    let mut tmp: f64;
    let mut i: isize;
    let mut j: isize;
    let mut k: isize;
    let mut n1: isize = -1;

    let mut nobs: c_int = 0;
    for k in 0..n as usize {
        if *ind.add(k) != 0 {
            nobs += 1;
        }
    }
    if nobs <= 1 {
        for i in 0..ncx as usize {
            for j in 0..ncy as usize {
                *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
            }
        }
        return;
    }

    if !kendall {
        // Compute x means
        for i in 0..ncx as usize {
            xx = x.offset(i * n as isize);
            sum = 0.0;
            for k in 0..n as usize {
                if *ind.add(k) != 0 {
                    sum += *xx.add(k);
                }
            }
            tmp = sum / nobs as f64;
            if R_FINITE(tmp) {
                sum = 0.0;
                for k in 0..n as usize {
                    if *ind.add(k) != 0 {
                        sum += *xx.add(k) - tmp;
                    }
                }
                tmp = tmp + sum / nobs as f64;
            }
            *xm.add(i) = tmp;
        }
        // Compute y means
        for i in 0..ncy as usize {
            yy = y.offset(i * n as isize);
            sum = 0.0;
            for k in 0..n as usize {
                if *ind.add(k) != 0 {
                    sum += *yy.add(k);
                }
            }
            tmp = sum / nobs as f64;
            if R_FINITE(tmp) {
                sum = 0.0;
                for k in 0..n as usize {
                    if *ind.add(k) != 0 {
                        sum += *yy.add(k) - tmp;
                    }
                }
                tmp = tmp + sum / nobs as f64;
            }
            *ym.add(i) = tmp;
        }
        n1 = (nobs - 1) as usize;
    }

    for i in 0..ncx as usize {
        xx = x.offset(i * n as isize);
        if !kendall {
            xxm = *xm.add(i);
            for j in 0..ncy as usize {
                yy = y.offset(j * n as isize);
                yym = *ym.add(j);
                sum = 0.0;
                for k in 0..n as usize {
                    if *ind.add(k) != 0 {
                        sum += (*xx.add(k) - xxm) * (*yy.add(k) - yym);
                    }
                }
                *ANS(ans, i as c_int, j as c_int, ncx) = sum / n1 as f64;
            }
        } else {
            // Kendall's tau
            for j in 0..ncy as usize {
                yy = y.offset(j * n as isize);
                sum = 0.0;
                for k in 0..n as usize {
                    if *ind.add(k) != 0 {
                        for n1_inner in 0..n as usize {
                            if *ind.add(n1_inner) != 0 {
                                sum += sign(*xx.add(k) - *xx.add(n1_inner))
                                    * sign(*yy.add(k) - *yy.add(n1_inner));
                            }
                        }
                    }
                }
                *ANS(ans, i as c_int, j as c_int, ncx) = sum;
            }
        }
    }

    if cor {
        // Compute x standard deviations
        for i in 0..ncx as usize {
            xx = x.offset(i * n as isize);
            sum = 0.0;
            if !kendall {
                xxm = *xm.add(i);
                for k in 0..n as usize {
                    if *ind.add(k) != 0 {
                        sum += (*xx.add(k) - xxm) * (*xx.add(k) - xxm);
                    }
                }
                sum /= n1 as f64;
            } else {
                for k in 0..n as usize {
                    if *ind.add(k) != 0 {
                        for n1_inner in 0..n as usize {
                            if *ind.add(n1_inner) != 0 && *xx.add(k) != *xx.add(n1_inner) {
                                sum += 1.0;
                            }
                        }
                    }
                }
            }
            *xm.add(i) = sum.sqrt();
        }
        // Compute y standard deviations
        for i in 0..ncy as usize {
            yy = y.offset(i * n as isize);
            sum = 0.0;
            if !kendall {
                yym = *ym.add(i);
                for k in 0..n as usize {
                    if *ind.add(k) != 0 {
                        sum += (*yy.add(k) - yym) * (*yy.add(k) - yym);
                    }
                }
                sum /= n1 as f64;
            } else {
                for k in 0..n as usize {
                    if *ind.add(k) != 0 {
                        for n1_inner in 0..n as usize {
                            if *ind.add(n1_inner) != 0 && *yy.add(k) != *yy.add(n1_inner) {
                                sum += 1.0;
                            }
                        }
                    }
                }
            }
            *ym.add(i) = sum.sqrt();
        }

        for i in 0..ncx as usize {
            for j in 0..ncy as usize {
                if *xm.add(i) == 0.0 || *ym.add(j) == 0.0 {
                    *sd_0 = true;
                    *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
                } else {
                    let val = *ANS(ans, i as c_int, j as c_int, ncx) / (*xm.add(i) * *ym.add(j));
                    *ANS(ans, i as c_int, j as c_int, ncx) = CLAMP(val);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// cov_na_2: NA propagated, two matrices
// ---------------------------------------------------------------------------

unsafe fn cov_na_2(
    n: c_int,
    ncx: c_int,
    ncy: c_int,
    x: *mut c_double,
    y: *mut c_double,
    xm: *mut c_double,
    ym: *mut c_double,
    has_na_x: *mut c_int,
    has_na_y: *mut c_int,
    ans: *mut c_double,
    sd_0: &mut bool,
    cor: bool,
    kendall: bool,
) {
    let mut xx: *const c_double;
    let mut yy: *const c_double;
    let mut sum: f64;
    let mut xxm: f64;
    let mut yym: f64;
    let mut tmp: f64;
    let mut i: isize;
    let mut j: isize;
    let mut k: isize;
    let mut n1: isize = -1;

    if n <= 1 {
        for i in 0..ncx as usize {
            for j in 0..ncy as usize {
                *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
            }
        }
        return;
    }

    if !kendall {
        // Compute x means
        for i in 0..ncx as usize {
            if *has_na_x.add(i) != 0 {
                *xm.add(i) = NA_REAL;
            } else {
                xx = x.offset(i * n as isize);
                sum = 0.0;
                for k in 0..n as usize {
                    sum += *xx.add(k);
                }
                tmp = sum / n as f64;
                if R_FINITE(tmp) {
                    sum = 0.0;
                    for k in 0..n as usize {
                        sum += *xx.add(k) - tmp;
                    }
                    tmp = tmp + sum / n as f64;
                }
                *xm.add(i) = tmp;
            }
        }
        // Compute y means
        for i in 0..ncy as usize {
            if *has_na_y.add(i) != 0 {
                *ym.add(i) = NA_REAL;
            } else {
                yy = y.offset(i * n as isize);
                sum = 0.0;
                for k in 0..n as usize {
                    sum += *yy.add(k);
                }
                tmp = sum / n as f64;
                if R_FINITE(tmp) {
                    sum = 0.0;
                    for k in 0..n as usize {
                        sum += *yy.add(k) - tmp;
                    }
                    tmp = tmp + sum / n as f64;
                }
                *ym.add(i) = tmp;
            }
        }
        n1 = (n - 1) as usize;
    }

    for i in 0..ncx as usize {
        if *has_na_x.add(i) != 0 {
            for j in 0..ncy as usize {
                *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
            }
        } else {
            xx = x.offset(i * n as isize);
            if !kendall {
                xxm = *xm.add(i);
                for j in 0..ncy as usize {
                    if *has_na_y.add(j) != 0 {
                        *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
                    } else {
                        yy = y.offset(j * n as isize);
                        yym = *ym.add(j);
                        sum = 0.0;
                        for k in 0..n as usize {
                            sum += (*xx.add(k) - xxm) * (*yy.add(k) - yym);
                        }
                        *ANS(ans, i as c_int, j as c_int, ncx) = sum / n1 as f64;
                    }
                }
            } else {
                // Kendall's tau
                for j in 0..ncy as usize {
                    if *has_na_y.add(j) != 0 {
                        *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
                    } else {
                        yy = y.offset(j * n as isize);
                        sum = 0.0;
                        for k in 0..n as usize {
                            for n1_inner in 0..n as usize {
                                sum += sign(*xx.add(k) - *xx.add(n1_inner))
                                    * sign(*yy.add(k) - *yy.add(n1_inner));
                            }
                        }
                        *ANS(ans, i as c_int, j as c_int, ncx) = sum;
                    }
                }
            }
        }
    }

    if cor {
        // x std devs
        for i in 0..ncx as usize {
            if *has_na_x.add(i) == 0 {
                xx = x.offset(i * n as isize);
                sum = 0.0;
                if !kendall {
                    xxm = *xm.add(i);
                    for k in 0..n as usize {
                        sum += (*xx.add(k) - xxm) * (*xx.add(k) - xxm);
                    }
                    sum /= n1 as f64;
                } else {
                    for k in 0..n as usize {
                        for n1_inner in 0..n as usize {
                            if *xx.add(k) != *xx.add(n1_inner) {
                                sum += 1.0;
                            }
                        }
                    }
                }
                *xm.add(i) = sum.sqrt();
            }
        }
        // y std devs
        for i in 0..ncy as usize {
            if *has_na_y.add(i) == 0 {
                yy = y.offset(i * n as isize);
                sum = 0.0;
                if !kendall {
                    yym = *ym.add(i);
                    for k in 0..n as usize {
                        sum += (*yy.add(k) - yym) * (*yy.add(k) - yym);
                    }
                    sum /= n1 as f64;
                } else {
                    for k in 0..n as usize {
                        for n1_inner in 0..n as usize {
                            if *yy.add(k) != *yy.add(n1_inner) {
                                sum += 1.0;
                            }
                        }
                    }
                }
                *ym.add(i) = sum.sqrt();
            }
        }

        for i in 0..ncx as usize {
            if *has_na_x.add(i) == 0 {
                for j in 0..ncy as usize {
                    if *has_na_y.add(j) == 0 {
                        if *xm.add(i) == 0.0 || *ym.add(j) == 0.0 {
                            *sd_0 = true;
                            *ANS(ans, i as c_int, j as c_int, ncx) = NA_REAL;
                        } else {
                            let val =
                                *ANS(ans, i as c_int, j as c_int, ncx) / (*xm.add(i) * *ym.add(j));
                            *ANS(ans, i as c_int, j as c_int, ncx) = CLAMP(val);
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main corcov entry point
// ---------------------------------------------------------------------------

/// cor(x, y, use, kendall) - compute correlation matrix
pub unsafe fn cor(x: SEXP, y: SEXP, na_method: SEXP, kendall: SEXP) -> SEXP {
    corcov(x, y, na_method, kendall, true)
}

/// cov(x, y, use, kendall) - compute covariance matrix
pub unsafe fn cov(x: SEXP, y: SEXP, na_method: SEXP, kendall: SEXP) -> SEXP {
    corcov(x, y, na_method, kendall, false)
}

static VAR_FACTOR_MSG: &str = "Calling var(x) on a factor x is defunct.\n  Use something like 'all(duplicated(x)[-1L])' to test for a constant vector.";

unsafe fn corcov(mut x: SEXP, mut y: SEXP, na_method: SEXP, skendall: SEXP, cor: bool) -> SEXP {
    let mut ans: SEXP = std::ptr::null_mut();
    let mut xm: SEXP = std::ptr::null_mut();
    let mut ym: SEXP = std::ptr::null_mut();
    let mut ind: SEXP = std::ptr::null_mut();
    let mut guards = Vec::with_capacity(8);
    let mut ansmat: bool;
    let kendall: bool;
    let mut pair: bool;
    let mut na_fail: bool;
    let mut everything: bool;
    let mut sd_0: bool;
    let mut empty_err: bool;
    let method: c_int;
    let mut n: c_int;
    let mut ncx: c_int;
    let mut ncy: c_int;
    let mut i: c_int;

    // Arg.1: x
    if Rf_isNull(x) != 0 {
        error("'x' is NULL");
    }
    if isFactor(x) {
        error(VAR_FACTOR_MSG);
    }

    x = coerceVector(x, SEXPTYPE::REALSXP.as_c_int());
    guards.push(protect(x));
    if isMatrix(x) {
        n = nrows(x);
        ncx = ncols(x);
    } else {
        n = length(x);
        ncx = 1;
    }

    // Arg.2: y
    if Rf_isNull(y) != 0 {
        ncy = ncx;
    } else {
        if isFactor(y) {
            error(VAR_FACTOR_MSG);
        }
        y = coerceVector(y, SEXPTYPE::REALSXP.as_c_int());
        guards.push(protect(y));
        if isMatrix(y) {
            if nrows(y) != n {
                error("incompatible dimensions");
            }
            ncy = ncols(y);
            ansmat = true;
        } else {
            if length(y) != n {
                error("incompatible dimensions");
            }
            ncy = 1;
        }
    }

    // Arg.3: method
    method = asInteger(na_method);

    // Arg.4: kendall
    kendall = asBool(skendall);

    // Default values
    na_fail = false;
    everything = false;
    empty_err = true;
    pair = false;

    match method {
        1 => {
            // "all.obs" - no NAs
            na_fail = true;
        }
        2 => {
            // "complete"
            if LENGTH(x) == 0 {
                error("no complete element pairs");
            }
        }
        3 => {
            // "pairwise.complete"
            pair = true;
        }
        4 => {
            // "everything"
            everything = true;
            empty_err = false;
        }
        5 => {
            // "na.or.complete"
            empty_err = false;
        }
        _ => {
            error("invalid 'use' (computational method)");
        }
    }

    if empty_err && LENGTH(x) == 0 {
        error("'x' is empty");
    }

    ansmat = isMatrix(x);
    if ansmat {
        ans = allocMatrix(SEXPTYPE::REALSXP, ncx, ncy);
    } else {
        ans = Rf_allocVector(SEXPTYPE::REALSXP, ncx * ncy);
    }
    guards.push(protect(ans));

    sd_0 = false;

    if Rf_isNull(y) != 0 {
        if everything {
            xm = Rf_allocVector(SEXPTYPE::REALSXP, ncx);
            let _xm_guard = protect(xm);
            ind = Rf_allocVector(SEXPTYPE::LGLSXP, ncx);
            let _ind_guard = protect(ind);
            find_na_1(n, ncx, REAL(x), LOGICAL(ind));
            cov_na_1(
                n,
                ncx,
                REAL(x),
                REAL(xm),
                LOGICAL(ind),
                REAL(ans),
                &mut sd_0,
                cor,
                kendall,
            );
        } else if !pair {
            // all | complete "var"
            xm = Rf_allocVector(SEXPTYPE::REALSXP, ncx);
            let _xm_guard = protect(xm);
            ind = Rf_allocVector(SEXPTYPE::INTSXP, n);
            let _ind_guard = protect(ind);
            complete1(n, ncx, REAL(x), INTEGER(ind), na_fail);
            cov_complete1(
                n,
                ncx,
                REAL(x),
                REAL(xm),
                INTEGER(ind),
                REAL(ans),
                &mut sd_0,
                cor,
                kendall,
            );
            if empty_err {
                let mut indany = false;
                for i in 0..n {
                    if *INTEGER(ind).add(i as usize) == 1 {
                        indany = true;
                        break;
                    }
                }
                if !indany {
                    error("no complete element pairs");
                }
            }
        } else {
            // pairwise "var"
            cov_pairwise1(n, ncx, REAL(x), REAL(ans), &mut sd_0, cor, kendall);
        }
    } else {
        // Co[vr](x, y)
        if everything {
            let has_na_y = Rf_allocVector(SEXPTYPE::LGLSXP, ncy);
            let _has_na_y_guard = protect(has_na_y);
            xm = Rf_allocVector(SEXPTYPE::REALSXP, ncx);
            let _xm_guard = protect(xm);
            ym = Rf_allocVector(SEXPTYPE::REALSXP, ncy);
            let _ym_guard = protect(ym);
            ind = Rf_allocVector(SEXPTYPE::LGLSXP, ncx);
            let _ind_guard = protect(ind);

            find_na_2(
                n,
                ncx,
                ncy,
                REAL(x),
                REAL(y),
                LOGICAL(ind),
                LOGICAL(has_na_y),
            );
            cov_na_2(
                n,
                ncx,
                ncy,
                REAL(x),
                REAL(y),
                REAL(xm),
                REAL(ym),
                LOGICAL(ind),
                LOGICAL(has_na_y),
                REAL(ans),
                &mut sd_0,
                cor,
                kendall,
            );
        } else if !pair {
            xm = Rf_allocVector(SEXPTYPE::REALSXP, ncx);
            let _xm_guard = protect(xm);
            ym = Rf_allocVector(SEXPTYPE::REALSXP, ncy);
            let _ym_guard = protect(ym);
            ind = Rf_allocVector(SEXPTYPE::INTSXP, n);
            let _ind_guard = protect(ind);
            complete2(n, ncx, ncy, REAL(x), REAL(y), INTEGER(ind), na_fail);
            cov_complete2(
                n,
                ncx,
                ncy,
                REAL(x),
                REAL(y),
                REAL(xm),
                REAL(ym),
                INTEGER(ind),
                REAL(ans),
                &mut sd_0,
                cor,
                kendall,
            );
            if empty_err {
                let mut indany = false;
                for i in 0..n {
                    if *INTEGER(ind).add(i as usize) == 1 {
                        indany = true;
                        break;
                    }
                }
                if !indany {
                    error("no complete element pairs");
                }
            }
        } else {
            cov_pairwise2(
                n,
                ncx,
                ncy,
                REAL(x),
                REAL(y),
                REAL(ans),
                &mut sd_0,
                cor,
                kendall,
            );
        }
    }

    // Set dimnames when applicable
    if ansmat {
        if Rf_isNull(y) != 0 {
            let x_dn = getAttrib(x, R_DimNamesSymbol());
            if !x_dn.is_null() && Rf_isNull(VECTOR_ELT(x_dn, 1)) == 0 {
                ind = Rf_allocVector(SEXPTYPE::VECSXP, 2);
                let _ind_guard = protect(ind);
                SET_VECTOR_ELT(ind, 0, duplicate(VECTOR_ELT(x_dn, 1)));
                SET_VECTOR_ELT(ind, 1, duplicate(VECTOR_ELT(x_dn, 1)));
                setAttrib(ans, R_DimNamesSymbol(), ind);
            }
        } else {
            let x_dn = getAttrib(x, R_DimNamesSymbol());
            let y_dn = getAttrib(y, R_DimNamesSymbol());
            if (length(x_dn) >= 2 && Rf_isNull(VECTOR_ELT(x_dn, 1)) == 0)
                || (length(y_dn) >= 2 && Rf_isNull(VECTOR_ELT(y_dn, 1)) == 0)
            {
                ind = Rf_allocVector(SEXPTYPE::VECSXP, 2);
                let _ind_guard = protect(ind);
                if length(x_dn) >= 2 && Rf_isNull(VECTOR_ELT(x_dn, 1)) == 0 {
                    SET_VECTOR_ELT(ind, 0, duplicate(VECTOR_ELT(x_dn, 1)));
                }
                if length(y_dn) >= 2 && Rf_isNull(VECTOR_ELT(y_dn, 1)) == 0 {
                    SET_VECTOR_ELT(ind, 1, duplicate(VECTOR_ELT(y_dn, 1)));
                }
                setAttrib(ans, R_DimNamesSymbol(), ind);
            }
        }
    }

    if sd_0 {
        warning("the standard deviation is zero");
    }

    ans
}
