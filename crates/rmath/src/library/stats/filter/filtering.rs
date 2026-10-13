//! Convolution/recursive time-series filters (cfilter, filter, rfilter)

use super::*;

pub unsafe fn cfilter(sx: SEXP, sfilter: SEXP, ssides: SEXP, scircular: SEXP) -> SEXP {
    unsafe {
        use crate::main::errors::Rf_error;

        if TYPEOF(sx) != SEXPTYPE::REALSXP || TYPEOF(sfilter) != SEXPTYPE::REALSXP {
            Rf_error(b"invalid input\0".as_ptr() as *const core::ffi::c_char);
        }

        let nx = XLENGTH(sx) as isize;
        let nf = XLENGTH(sfilter) as isize;
        let sides = as_integer(ssides);
        let circular = as_logical(scircular);
        if sides == NA_INTEGER || circular == NA_INTEGER {
            Rf_error(b"invalid input\0".as_ptr() as *const core::ffi::c_char);
        }

        let ans = Rf_allocVector(SEXPTYPE::REALSXP, nx as c_int);
        let x = REAL(sx);
        let filter = REAL(sfilter);
        let out = REAL(ans);

        let nshift = if sides == 2 { nf / 2 } else { 0 };

        if circular == 0 {
            for i in 0..(nx as usize) {
                let mut z: c_double = 0.0;
                let i_nshift = i as isize + nshift;

                if i_nshift - (nf - 1) < 0 || i_nshift >= nx {
                    *out.add(i) = NA_REAL;
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
                    let tmp = *x.add((i_nshift - j as isize) as usize);
                    if my_isok(tmp) {
                        z += *filter.add(j) * tmp;
                    } else {
                        *out.add(i) = NA_REAL;
                        bad = true;
                        break;
                    }
                    j += 1;
                }
                if !bad {
                    *out.add(i) = z;
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
                    let tmp = *x.add(ii as usize);
                    if my_isok(tmp) {
                        z += *filter.add(j) * tmp;
                    } else {
                        *out.add(i) = NA_REAL;
                        bad = true;
                        break;
                    }
                }
                if !bad {
                    *out.add(i) = z;
                }
            }
        }

        ans
    }
}

pub unsafe extern "C-unwind" fn c_cfilter(
    sx: SEXP,
    sfilter: SEXP,
    ssides: SEXP,
    scircular: SEXP,
) -> SEXP {
    unsafe { cfilter(sx, sfilter, ssides, scircular) }
}

/// GNU `filter(x, filter)` convolution, sides=2.
pub unsafe fn do_filter(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

/* recursive filtering */
pub unsafe fn rfilter(x: SEXP, filter: SEXP, out: SEXP) -> SEXP {
    unsafe {
        use crate::main::errors::Rf_error;

        if TYPEOF(x) != SEXPTYPE::REALSXP
            || TYPEOF(filter) != SEXPTYPE::REALSXP
            || TYPEOF(out) != SEXPTYPE::REALSXP
        {
            Rf_error(b"invalid input\0".as_ptr() as *const core::ffi::c_char);
        }

        let nx = XLENGTH(x);
        let nf = XLENGTH(filter);
        let r = REAL(out);
        let rx = REAL(x);
        let rf = REAL(filter);

        for i in 0..(nx as usize) {
            let mut sum = *rx.add(i);
            if !my_isok(sum) {
                *r.add(nf as usize + i) = NA_REAL;
                continue;
            }
            let mut bad = false;
            for j in 0..(nf as usize) {
                let tmp = *r.add(nf as usize + i - j - 1);
                if my_isok(tmp) {
                    sum += tmp * *rf.add(j);
                } else {
                    *r.add(nf as usize + i) = NA_REAL;
                    bad = true;
                    break;
                }
            }
            if !bad {
                *r.add(nf as usize + i) = sum;
            }
        }

        out
    }
}

pub unsafe extern "C-unwind" fn c_rfilter(x: SEXP, filter: SEXP, out: SEXP) -> SEXP {
    unsafe { rfilter(x, filter, out) }
}
