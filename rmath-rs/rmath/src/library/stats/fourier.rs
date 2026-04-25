/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1998--2025  The R Core Team.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *  Ported from r-source/src/library/stats/src/fourier.c
 *
 *  These are the R interface routines to the plain FFT code
 *  fft_factor() & fft_work() in fft.c.
 */

use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::attrib_core::{R_DimSymbol, getAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::protect::*;

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    crate::main::coerce::coerceVector(x, type_)
}

unsafe fn duplicate(x: SEXP) -> SEXP {
    crate::main::duplicate::duplicate(x)
}

use crate::library::stats::fft::{fft_factor, fft_work};

/// true if namedness > 0 (potentially shared).
unsafe fn MAYBE_REFERENCED(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    crate::sexp::accessors::NAMED(x) as c_int
}

unsafe fn as_integer(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP {
        return *INTEGER(x);
    }
    if t == SEXPTYPE::REALSXP {
        let v = *REAL(x);
        if v.is_nan() || v < c_int::MIN as f64 || v > c_int::MAX as f64 {
            return NA_INTEGER;
        }
        return v as c_int;
    }
    if t == SEXPTYPE::LGLSXP {
        return *INTEGER(x);
    }
    NA_INTEGER
}

unsafe fn as_logical(x: SEXP) -> c_int {
    if x.is_null() {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP {
        return *INTEGER(x);
    }
    if t == SEXPTYPE::INTSXP {
        return *INTEGER(x);
    }
    NA_INTEGER
}

/* Fourier Transform for Univariate Spatial and Time Series */

pub unsafe fn fft(z: SEXP, inverse: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    let mut z = z;

    match TYPEOF(z) as c_int {
        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP || t == SEXPTYPE::REALSXP => {
            z = coerceVector(z, SEXPTYPE::CPLXSXP.as_c_int());
        }
        t if t == SEXPTYPE::CPLXSXP => {
            if MAYBE_REFERENCED(z) != 0 {
                z = duplicate(z);
            }
        }
        _ => {
            Rf_error(b"non-numeric argument\0".as_ptr() as *const libc::c_char);
        }
    }
    Rf_protect(z);

    /* -2 for forward transform, complex values */
    /* +2 for backward transform, complex values */
    let mut inv = as_logical(inverse);
    if inv == NA_INTEGER || inv == 0 {
        inv = -2;
    } else {
        inv = 2;
    }

    if LENGTH(z) > 1 {
        let d = getAttrib(z, R_DimSymbol());
        if d.is_null() {
            /* temporal transform */
            let n = LENGTH(z);
            let mut maxf: c_int = 0;
            let mut maxp: c_int = 0;
            fft_factor(n, &mut maxf, &mut maxp);
            if maxf == 0 {
                Rf_error(b"fft factorization error\0".as_ptr() as *const libc::c_char);
            }
            let smaxf = maxf as usize;
            let maxsize = usize::MAX / 4;
            if smaxf > maxsize {
                Rf_error(b"fft too large\0".as_ptr() as *const libc::c_char);
            }
            let mut work = vec![0.0f64; 4 * smaxf];
            let mut iwork = vec![0i32; maxp as usize];
            let re = &mut (*COMPLEX(z)).r;
            let im = &mut (*COMPLEX(z)).i;
            fft_work(
                re as *mut f64,
                im as *mut f64,
                1,
                n,
                1,
                inv,
                work.as_mut_ptr(),
                iwork.as_mut_ptr(),
            );
        } else {
            /* spatial transform */
            let mut maxmaxf: c_int = 1;
            let mut maxmaxp: c_int = 1;
            let ndims = LENGTH(d);
            for i in 0..(ndims as usize) {
                if *INTEGER(d.add(i)) > 1 {
                    let mut maxf: c_int = 0;
                    let mut maxp: c_int = 0;
                    fft_factor(*INTEGER(d.add(i)), &mut maxf, &mut maxp);
                    if maxf == 0 {
                        Rf_error(b"fft factorization error\0".as_ptr() as *const libc::c_char);
                    }
                    if maxf > maxmaxf {
                        maxmaxf = maxf;
                    }
                    if maxp > maxmaxp {
                        maxmaxp = maxp;
                    }
                }
            }
            let smaxf = maxmaxf as usize;
            let maxsize = usize::MAX / 4;
            if smaxf > maxsize {
                Rf_error(b"fft too large\0".as_ptr() as *const libc::c_char);
            }
            let mut work = vec![0.0f64; 4 * smaxf];
            let mut iwork = vec![0i32; maxmaxp as usize];
            let mut nseg = LENGTH(z);
            let mut n: c_int = 1;
            let mut nspn: c_int = 1;
            for i in 0..(ndims as usize) {
                if *INTEGER(d.add(i)) > 1 {
                    nspn *= n;
                    n = *INTEGER(d.add(i));
                    nseg /= n;
                    let mut maxf: c_int = 0;
                    let mut maxp: c_int = 0;
                    fft_factor(n, &mut maxf, &mut maxp);
                    fft_work(
                        &mut (*COMPLEX(z)).r,
                        &mut (*COMPLEX(z)).i,
                        nseg,
                        n,
                        nspn,
                        inv,
                        work.as_mut_ptr(),
                        iwork.as_mut_ptr(),
                    );
                }
            }
        }
    }
    Rf_unprotect(1);
    z
}

/* Fourier Transform for Vector-Valued ("multivariate") Series */

pub unsafe fn mvfft(z: SEXP, inverse: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    let d = getAttrib(z, R_DimSymbol());
    if d.is_null() || LENGTH(d) > 2 {
        Rf_error(b"vector-valued (multivariate) series required\0".as_ptr() as *const libc::c_char);
    }
    let n = *INTEGER(d);
    let p = *INTEGER(d.add(1));

    let mut z = z;
    match TYPEOF(z) as c_int {
        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP || t == SEXPTYPE::REALSXP => {
            z = coerceVector(z, SEXPTYPE::CPLXSXP.as_c_int());
        }
        t if t == SEXPTYPE::CPLXSXP => {
            if MAYBE_REFERENCED(z) != 0 {
                z = duplicate(z);
            }
        }
        _ => {
            Rf_error(b"non-numeric argument\0".as_ptr() as *const libc::c_char);
        }
    }
    Rf_protect(z);

    let mut inv = as_logical(inverse);
    if inv == NA_INTEGER || inv == 0 {
        inv = -2;
    } else {
        inv = 2;
    }

    if n > 1 {
        let mut maxf: c_int = 0;
        let mut maxp: c_int = 0;
        fft_factor(n, &mut maxf, &mut maxp);
        if maxf == 0 {
            Rf_error(b"fft factorization error\0".as_ptr() as *const libc::c_char);
        }
        let smaxf = maxf as usize;
        let maxsize = usize::MAX / 4;
        if smaxf > maxsize {
            Rf_error(b"fft too large\0".as_ptr() as *const libc::c_char);
        }
        let mut work = vec![0.0f64; 4 * smaxf];
        let mut iwork = vec![0i32; maxp as usize];
        for i in 0..(p as usize) {
            fft_factor(n, &mut maxf, &mut maxp);
            let base = COMPLEX(z).add(i * n as usize);
            fft_work(
                &mut (*base).r,
                &mut (*base).i,
                1,
                n,
                1,
                inv,
                work.as_mut_ptr(),
                iwork.as_mut_ptr(),
            );
        }
    }
    Rf_unprotect(1);
    z
}

unsafe fn ok_n(mut n: c_int, f: *const c_int, nf: c_int) -> bool {
    for i in 0..(nf as usize) {
        loop {
            if n % *f.add(i) != 0 {
                break;
            }
            n = n / *f.add(i);
            if n == 1 {
                return true;
            }
        }
    }
    n == 1
}

unsafe fn ok_n_64(mut n: u64, f: *const c_int, nf: c_int) -> bool {
    for i in 0..(nf as usize) {
        loop {
            if n % (*f.add(i) as u64) != 0 {
                break;
            }
            n = n / (*f.add(i) as u64);
            if n == 1 {
                return true;
            }
        }
    }
    n == 1
}

unsafe fn nextn0(mut n: c_int, f: *const c_int, nf: c_int) -> c_int {
    loop {
        if ok_n(n, f, nf) {
            break;
        }
        if n >= c_int::MAX {
            break;
        }
        n += 1;
    }
    if n >= c_int::MAX {
        crate::main::errors::Rf_warning(
            b"nextn() found no solution < INT_MAX (the maximal integer)\0".as_ptr()
                as *const libc::c_char,
        );
        return NA_INTEGER;
    }
    n
}

unsafe fn nextn0_64(mut n: u64, f: *const c_int, nf: c_int) -> u64 {
    loop {
        if ok_n_64(n, f, nf) {
            break;
        }
        if n >= u64::MAX {
            break;
        }
        n += 1;
    }
    if n >= u64::MAX {
        crate::main::errors::Rf_warning(
            b"nextn<64>() found no solution < UINT64_MAX\0".as_ptr() as *const libc::c_char
        );
        return 0;
    }
    n
}

pub unsafe fn nextn(mut n: SEXP, f: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    if TYPEOF(n) == SEXPTYPE::NILSXP {
        return Rf_allocVector(SEXPTYPE::INTSXP, 0);
    }

    let mut nprot: c_int = 0;
    let mut f = f;
    if TYPEOF(f) != SEXPTYPE::INTSXP {
        f = coerceVector(f, SEXPTYPE::INTSXP.as_c_int());
        Rf_protect(f);
        nprot += 1;
    }
    let nf = LENGTH(f);
    let f_ = INTEGER(f);

    /* check the factors */
    if nf == 0 {
        Rf_error(b"no factors\0".as_ptr() as *const libc::c_char);
    }
    if nf < 0 {
        Rf_error(b"too many factors\0".as_ptr() as *const libc::c_char);
    }
    for i in 0..(nf as usize) {
        if *f_.add(i) == NA_INTEGER || *f_.add(i) <= 1 {
            Rf_error(b"invalid factors\0".as_ptr() as *const libc::c_char);
        }
    }

    let mut use_int = TYPEOF(n) == SEXPTYPE::INTSXP;
    if !use_int && TYPEOF(n) != SEXPTYPE::REALSXP {
        Rf_error(
            b"'n' must have typeof(.) \"integer\" or \"double\"\0".as_ptr() as *const libc::c_char,
        );
    }
    let nn = XLENGTH(n);
    if !use_int && nn > 0 {
        let d_n = REAL(n);
        let mut n_max: c_double = -1.0;
        for i in 0..(nn as usize) {
            if !ISNAN(*d_n.add(i)) && *d_n.add(i) > n_max {
                n_max = *d_n.add(i);
            }
        }
        if n_max <= (c_int::MAX as c_double) / (*f_ as c_double) {
            use_int = true;
            let n_new = coerceVector(n, SEXPTYPE::INTSXP.as_c_int());
            Rf_protect(n_new);
            nprot += 1;
            n = n_new;
        }
    }

    let ans_type = if use_int {
        SEXPTYPE::INTSXP.as_c_int()
    } else {
        SEXPTYPE::REALSXP.as_c_int()
    };
    let ans = Rf_protect(Rf_allocVector(ans_type, nn as c_int));
    nprot += 1;

    if nn == 0 {
        Rf_unprotect(nprot);
        return ans;
    }

    if use_int {
        let n_ = INTEGER(n);
        let r = INTEGER(ans);
        for i in 0..(nn as usize) {
            if *n_.add(i) == NA_INTEGER {
                *r.add(i) = NA_INTEGER;
            } else if *n_.add(i) <= 1 {
                *r.add(i) = 1;
            } else {
                *r.add(i) = nextn0(*n_.add(i), f_, nf);
            }
        }
    } else {
        let n_ = REAL(n);
        let r = REAL(ans);
        for i in 0..(nn as usize) {
            if ISNAN(*n_.add(i)) {
                *r.add(i) = NA_REAL;
            } else if *n_.add(i) <= 1.0 {
                *r.add(i) = 1.0;
            } else {
                let max_dbl_int: u64 = 9007199254740992; // 2^53
                let n_n = nextn0_64(*n_.add(i) as u64, f_, nf);
                if n_n > max_dbl_int {
                    crate::main::errors::Rf_warning(
                        b"nextn() may not be exactly representable in R (as \"double\")\0".as_ptr()
                            as *const libc::c_char,
                    );
                }
                *r.add(i) = n_n as c_double;
            }
        }
    }

    Rf_unprotect(nprot);
    ans
}
