#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

//! Running Median Smoother (Stuetzle algorithm)
//! Port of r-source/src/library/stats/src/Srunmed.c

use std::os::raw::{c_double, c_int};

use crate::library::stats::trunmed::Trunmed;
use crate::main::coerce::{asInteger, coerceVector};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::{REAL, TYPEOF, XLENGTH};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

const NA_BIG_alternate_P: c_int = 1;
const NA_BIG_alternate_M: c_int = 2;
const NA_OMIT: c_int = 3;
const NA_FAIL: c_int = 4;

static BIG_dbl: c_double = 8.888888888e307;

unsafe fn Srunmed(
    y: *const c_double,
    smo: *mut c_double,
    n: R_xlen_t,
    bw: c_int,
    end_rule: c_int,
    _print_level: c_int,
) {
    if bw > n as c_int {
        Rf_error(b"bandwidth/span of running medians is larger than n\0".as_ptr() as *const _);
    }

    let mut scrat = vec![0.0f64; bw as usize];

    // 1. Compute rmed := Median of the first 'band' values
    let mut i: c_int = 0;
    while i < bw {
        scrat[i as usize] = *y.add(i as usize);
        i += 1;
    }

    // find minimal value rmin = scrat[imin] <= scrat[j]
    let mut rmin = scrat[0];
    let mut imin: c_int = 0;
    i = 1;
    while i < bw {
        if scrat[i as usize] < rmin {
            rmin = scrat[i as usize];
            imin = i;
        }
        i += 1;
    }
    // swap scrat[0] <-> scrat[imin]
    let old_scrat0 = scrat[0];
    scrat[0] = rmin;
    scrat[imin as usize] = old_scrat0;

    // sort the rest of scrat[] by insertion sort
    let mut i: c_int = 2;
    while i < bw {
        if scrat[i as usize] < scrat[(i - 1) as usize] {
            let mut temp = scrat[i as usize];
            let mut j = i;
            loop {
                scrat[j as usize] = scrat[(j - 1) as usize];
                j -= 1;
                if !(scrat[(j - 1) as usize] > temp) {
                    break;
                }
            }
            scrat[j as usize] = temp;
        }
        i += 1;
    }

    let mut band2 = bw / 2;
    let mut rmed = scrat[band2 as usize];

    if end_rule == 0 {
        let mut i: R_xlen_t = 0;
        while i < band2 as R_xlen_t {
            *smo.add(i as usize) = *y.add(i as usize);
            i += 1;
        }
    } else {
        let mut i: R_xlen_t = 0;
        while i < band2 as R_xlen_t {
            *smo.add(i as usize) = rmed;
            i += 1;
        }
    }
    *smo.add(band2 as usize) = rmed;
    band2 += 1;

    // Big FOR Loop: RUNNING median, update the median 'rmed'
    let mut first: c_int = 1;
    let mut last: c_int = bw;
    let mut ismo: R_xlen_t = band2 as R_xlen_t;

    while last < n as c_int {
        let yin = *y.add(last as usize);
        let yout = *y.add((first - 1) as usize);

        let mut rnew = rmed;

        if yin < rmed {
            if yout >= rmed {
                let mut kminus: c_int = 0;
                if yout > rmed {
                    // yin < rmed < yout
                    rnew = yin;
                    let mut ii = first;
                    while ii <= last {
                        let yi = *y.add(ii as usize);
                        if yi < rmed {
                            kminus += 1;
                            if yi > rnew {
                                rnew = yi;
                            }
                        }
                        ii += 1;
                    }
                    if kminus < band2 {
                        rnew = rmed;
                    }
                } else {
                    // yin < rmed = yout
                    let mut rse = yin;
                    let mut rts = yin;
                    let mut ii = first;
                    while ii <= last {
                        let yi = *y.add(ii as usize);
                        if yi <= rmed {
                            if yi < rmed {
                                kminus += 1;
                                if yi > rts {
                                    rts = yi;
                                }
                                if yi > rse {
                                    rse = yi;
                                }
                            } else {
                                rse = yi;
                            }
                        }
                        ii += 1;
                    }
                    rnew = if kminus == band2 { rts } else { rse };
                }
            }
        } else if yin != rmed {
            // yin > rmed
            if yout <= rmed {
                let mut kplus: c_int = 0;
                if yout < rmed {
                    // yout < rmed < yin
                    rnew = yin;
                    let mut ii = first;
                    while ii <= last {
                        let yi = *y.add(ii as usize);
                        if yi > rmed {
                            kplus += 1;
                            if yi < rnew {
                                rnew = yi;
                            }
                        }
                        ii += 1;
                    }
                    if kplus < band2 {
                        rnew = rmed;
                    }
                } else {
                    // yout = rmed < yin
                    let mut rbe = yin;
                    let mut rtb = yin;
                    let mut ii = first;
                    while ii <= last {
                        let yi = *y.add(ii as usize);
                        if yi >= rmed {
                            if yi > rmed {
                                kplus += 1;
                                if yi < rtb {
                                    rtb = yi;
                                }
                                if yi < rbe {
                                    rbe = yi;
                                }
                            } else {
                                rbe = yi;
                            }
                        }
                        ii += 1;
                    }
                    rnew = if kplus == band2 { rtb } else { rbe };
                }
            }
        }

        rmed = rnew;
        *smo.add(ismo as usize) = rmed;

        first += 1;
        last += 1;
        ismo += 1;
    }

    if end_rule == 0 {
        let mut i = ismo;
        while i < n {
            *smo.add(i as usize) = *y.add(i as usize);
            i += 1;
        }
    } else {
        let mut i = ismo;
        while i < n {
            *smo.add(i as usize) = rmed;
            i += 1;
        }
    }
}

unsafe fn R_firstNA_dbl(x: *const c_double, n: R_xlen_t) -> R_xlen_t {
    let mut k: R_xlen_t = 0;
    while k < n {
        if x.add(k as usize).read().is_nan() {
            return k + 1;
        }
        k += 1;
    }
    0
}

pub unsafe fn runmed(
    sx: SEXP,
    stype: SEXP,
    sk: SEXP,
    end: SEXP,
    naAct: SEXP,
    printLev: SEXP,
) -> SEXP {
    let n = XLENGTH(sx);
    let mut nprot: c_int = 1;

    let mut sx = sx;
    if TYPEOF(sx) != SEXPTYPE::REALSXP {
        sx = Rf_protect(coerceVector(sx, SEXPTYPE::REALSXP.0));
        nprot += 1;
    }
    let x = REAL(sx);

    let type_ = asInteger(stype);
    let k = asInteger(sk);
    let end_rule = asInteger(end);
    let na_action = asInteger(naAct);
    let _print_level = asInteger(printLev);

    let firstNA = R_firstNA_dbl(x, n);
    let mut nn = n;

    let xx: *const c_double;
    let mut xx_buf: Vec<c_double> = Vec::new();

    if firstNA != 0 {
        let mut NA_pos = true;
        match na_action {
            NA_BIG_alternate_M => {
                NA_pos = false;
                xx_buf = vec![0.0; n as usize];
                let mut i: R_xlen_t = 0;
                while i < n {
                    let val = *x.add(i as usize);
                    if val.is_nan() {
                        xx_buf[i as usize] = if NA_pos { BIG_dbl } else { -BIG_dbl };
                        NA_pos = !NA_pos;
                    } else {
                        xx_buf[i as usize] = val;
                    }
                    i += 1;
                }
                xx = xx_buf.as_ptr();
            }
            NA_BIG_alternate_P => {
                xx_buf = vec![0.0; n as usize];
                let mut i: R_xlen_t = 0;
                while i < n {
                    let val = *x.add(i as usize);
                    if val.is_nan() {
                        xx_buf[i as usize] = if NA_pos { BIG_dbl } else { -BIG_dbl };
                        NA_pos = !NA_pos;
                    } else {
                        xx_buf[i as usize] = val;
                    }
                    i += 1;
                }
                xx = xx_buf.as_ptr();
            }
            NA_OMIT => {
                xx_buf = vec![0.0; (n - 1) as usize];
                let i1 = firstNA - 1;
                let mut i: R_xlen_t = 0;
                while i < i1 {
                    xx_buf[i as usize] = *x.add(i as usize);
                    i += 1;
                }
                let mut ix: R_xlen_t = i1;
                let mut i = i1;
                while i < n {
                    let val = *x.add(i as usize);
                    if val.is_nan() {
                        nn -= 1;
                    } else {
                        xx_buf[ix as usize] = val;
                        ix += 1;
                    }
                    i += 1;
                }
                xx = xx_buf.as_ptr();
            }
            NA_FAIL => {
                eprintln!(
                    "runmed(x, .., na.action=\"na.fail\"): have NAs starting at x[{}]",
                    firstNA
                );
                return std::ptr::null_mut();
            }
            _ => {
                Rf_error(b"runmed(): invalid 'na.action'\0".as_ptr() as *const _);
                unreachable!();
            }
        }
    } else {
        xx = x;
    }

    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, n as c_int));

    if type_ == 1 {
        // Trunmed takes &[f64] and &mut [f64]
        let xx_slice = std::slice::from_raw_parts(xx, nn as usize);
        let ans_slice = std::slice::from_raw_parts_mut(REAL(ans), nn as usize);
        Trunmed(xx_slice, ans_slice, nn, k as i64, end_rule);
    } else {
        Srunmed(xx, REAL(ans), nn, k, end_rule, 0);
    }

    if firstNA != 0 {
        let median = REAL(ans);
        match na_action {
            NA_BIG_alternate_P | NA_BIG_alternate_M => {
                let mut i = firstNA - 1;
                while i < n {
                    let xval = *x.add(i as usize);
                    let mval = *median.add(i as usize);
                    if xval.is_nan() && !mval.is_nan() && mval.abs() == BIG_dbl {
                        *median.add(i as usize) = xval;
                    }
                    i += 1;
                }
            }
            NA_OMIT => {
                let mut med = vec![0.0; nn as usize];
                if nn > 0 {
                    let mut i: R_xlen_t = 0;
                    while i < nn {
                        med[i as usize] = *median.add(i as usize);
                        i += 1;
                    }
                }
                let mut i = firstNA - 1;
                let mut ix: R_xlen_t = i;
                while i < n {
                    let xval = *x.add(i as usize);
                    if xval.is_nan() {
                        *median.add(i as usize) = xval;
                    } else {
                        *median.add(i as usize) = med[ix as usize];
                        ix += 1;
                    }
                    i += 1;
                }
            }
            _ => {} // intentionally unhandled: unknown output type for smooth median
        }
    }

    Rf_unprotect(nprot);
    ans
}
