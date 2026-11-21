//! Convolution/recursive time-series filters (cfilter, filter, rfilter)

use super::*;

pub fn cfilter(sx: SEXP, sfilter: SEXP, ssides: SEXP, scircular: SEXP) -> SEXP {
    use crate::main::errors::{Rf_error, Rf_error_cstr};

    if TYPEOF(sx) != SEXPTYPE::REALSXP || TYPEOF(sfilter) != SEXPTYPE::REALSXP {
        Rf_error_cstr(c"invalid input");
    }

    let nx = XLENGTH(sx) as isize;
    let nf = XLENGTH(sfilter) as isize;
    let sides = as_integer(ssides);
    let circular = as_logical(scircular);
    if sides == NA_INTEGER || circular == NA_INTEGER {
        Rf_error_cstr(c"invalid input");
    }

    let ans = Rf_allocVector(SEXPTYPE::REALSXP, nx as c_int);
    let x = crate::library::stats::checked::real_vec(sx);
    let filter = crate::library::stats::checked::real_vec(sfilter);
    let mut out = vec![0.0; nx as usize];

    let nshift = if sides == 2 { nf / 2 } else { 0 };

    if circular == 0 {
        for i in 0..(nx as usize) {
            let mut z: c_double = 0.0;
            let i_nshift = i as isize + nshift;

            if i_nshift - (nf - 1) < 0 || i_nshift >= nx {
                out[i] = NA_REAL;
                continue;
            }

            let j_start = if nshift + i as isize - nx > 0 {
                (nshift + i as isize - nx) as usize
            } else {
                0usize
            };
            let j_end = if nf < i_nshift + 1 {
                nf as usize
            } else {
                (i_nshift + 1) as usize
            };

            let mut bad = false;
            let mut j = j_start;
            loop {
                if j >= j_end {
                    break;
                }
                let tmp = x[(i_nshift - j as isize) as usize];
                if my_isok(tmp) {
                    z += filter[j] * tmp;
                } else {
                    out[i] = NA_REAL;
                    bad = true;
                    break;
                }
                j += 1;
            }
            if !bad {
                out[i] = z;
            }
        }
    } else {
        /* circular */
        for i in 0..(nx as usize) {
            let mut z: c_double = 0.0;
            let mut bad = false;

            for j in 0..(nf as usize) {
                let mut ii = i as isize + nshift - j as isize;
                if ii < 0 {
                    ii += nx;
                }
                if ii >= nx {
                    ii -= nx;
                }
                let tmp = x[ii as usize];
                if my_isok(tmp) {
                    z += filter[j] * tmp;
                } else {
                    out[i] = NA_REAL;
                    bad = true;
                    break;
                }
            }
            if !bad {
                out[i] = z;
            }
        }
    }

    crate::library::stats::checked::set_reals(ans, &out);
    ans
}

pub unsafe extern "C-unwind" fn c_cfilter(
    sx: SEXP,
    sfilter: SEXP,
    ssides: SEXP,
    scircular: SEXP,
) -> SEXP {
    cfilter(sx, sfilter, ssides, scircular)
}

/// GNU `filter(x, filter)` convolution, sides=2.
pub fn do_filter(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let f = CAR(CDR(args));
    let xd = if TYPEOF(x) == SEXPTYPE::REALSXP {
        x
    } else {
        coerceVector(x, SEXPTYPE::REALSXP.as_c_int())
    };
    let _xd = protect(xd);
    let fd = if TYPEOF(f) == SEXPTYPE::REALSXP {
        f
    } else {
        coerceVector(f, SEXPTYPE::REALSXP.as_c_int())
    };
    let _fd = protect(fd);
    let sides = Rf_ScalarInteger(2);
    let _s = protect(sides);
    let circ = Rf_ScalarLogical(0);
    let _c = protect(circ);
    cfilter(xd, fd, sides, circ)
}

/* recursive filtering */
pub fn rfilter(x: SEXP, filter: SEXP, out: SEXP) -> SEXP {
    use crate::main::errors::{Rf_error, Rf_error_cstr};

    if TYPEOF(x) != SEXPTYPE::REALSXP
        || TYPEOF(filter) != SEXPTYPE::REALSXP
        || TYPEOF(out) != SEXPTYPE::REALSXP
    {
        Rf_error_cstr(c"invalid input");
    }

    let nx = XLENGTH(x);
    let nf = XLENGTH(filter);
    let mut r = crate::library::stats::checked::real_vec(out);
    let rx = crate::library::stats::checked::real_vec(x);
    let rf = crate::library::stats::checked::real_vec(filter);

    for i in 0..(nx as usize) {
        let mut sum = rx[i];
        if !my_isok(sum) {
            r[nf as usize + i] = NA_REAL;
            continue;
        }
        let mut bad = false;
        for j in 0..(nf as usize) {
            let tmp = r[nf as usize + i - j - 1];
            if my_isok(tmp) {
                sum += tmp * rf[j];
            } else {
                r[nf as usize + i] = NA_REAL;
                bad = true;
                break;
            }
        }
        if !bad {
            r[nf as usize + i] = sum;
        }
    }

    crate::library::stats::checked::set_reals(out, &r);
    out
}

pub unsafe extern "C-unwind" fn c_rfilter(x: SEXP, filter: SEXP, out: SEXP) -> SEXP {
    rfilter(x, filter, out)
}
