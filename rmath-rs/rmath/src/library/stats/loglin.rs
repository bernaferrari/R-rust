/*
 * Algorithm AS 51 Appl. Statist. (1972), vol. 21, p. 218
 *   original (C) Royal Statistical Society 1972
 *
 * Performs an iterative proportional fit of the marginal totals of a
 * contingency table.
 *
 * Ported to Rust from r-source/src/library/stats/src/loglin.c
 */

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::attrib_core::{R_NamesSymbol, setAttrib};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// External declarations
// ---------------------------------------------------------------------------

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    unsafe { crate::main::coerce::coerceVector(x, type_) }
}

unsafe fn duplicate(x: SEXP) -> SEXP {
    unsafe { crate::main::duplicate::duplicate(x) }
}

use crate::mainutils::util_main::ncols;

// ---------------------------------------------------------------------------
// Helper: asReal
// ---------------------------------------------------------------------------

unsafe fn as_real(x: SEXP) -> c_double {
    unsafe {
        if x.is_null() {
            return NA_REAL;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            return *REAL(x);
        }
        if t == SEXPTYPE::INTSXP {
            let v = *INTEGER(x);
            if v == NA_INTEGER {
                return NA_REAL;
            }
            return v as c_double;
        }
        if t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x);
            if v == NA_INTEGER {
                return NA_REAL;
            }
            return if v != 0 { 1.0 } else { 0.0 };
        }
        NA_REAL
    }
}

// ---------------------------------------------------------------------------
// Helper: asInteger
// ---------------------------------------------------------------------------

unsafe fn as_integer(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP {
            return *INTEGER(x);
        }
        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x);
            if v.is_nan() || v < c_int::MIN as c_double || v > c_int::MAX as c_double {
                return NA_INTEGER;
            }
            return v as c_int;
        }
        if t == SEXPTYPE::LGLSXP {
            return *INTEGER(x);
        }
        NA_INTEGER
    }
}

// ---------------------------------------------------------------------------
// collap -- Algorithm AS 51.1
// Computes a marginal table from a complete table.
// All parameters are assumed valid without test.
//
// The larger table is X (0-indexed in Rust) and the smaller one is Y.
// ---------------------------------------------------------------------------

unsafe fn collap(
    nvar: c_int,
    x: *const c_double,
    y: *mut c_double,
    locy: c_int,
    dim: *const c_int,
    config: *const c_int,
) {
    unsafe {
        // Initialize arrays
        let mut size = vec![0i32; (nvar + 1) as usize];
        size[0] = 1;
        let mut k = 1;
        while k <= nvar {
            let l = *config.add(k as usize);
            if l == 0 {
                break;
            }
            size[k as usize] = size[(k - 1) as usize] * *dim.add(l as usize);
            k += 1;
        }
        let n = k - 1; // number of variables in configuration

        // Initialize Y: first cell of marginal table is at y[locy-1] (1-based)
        // In Rust 0-based: y[locy-1..locy-1+size[n]]
        let locu = locy - 1 + size[n as usize];
        for j in (locy - 1)..locu {
            *y.add(j as usize) = 0.0;
        }

        // Initialize coordinates
        let mut coord = vec![0i32; nvar as usize];

        // Find locations in tables
        let mut i = 1; // 1-based index into x
        loop {
            let mut j = locy - 1; // 1-based index into y
            for kk in 1..=n {
                let l = *config.add(kk as usize);
                j += coord[(l - 1) as usize] * size[(kk - 1) as usize];
            }
            *y.add(j as usize) += *x.add((i - 1) as usize);

            // Update coordinates
            i += 1;
            let mut done = true;
            for kk in 1..=nvar {
                coord[(kk - 1) as usize] += 1;
                if coord[(kk - 1) as usize] < *dim.add(kk as usize) {
                    done = false;
                    break;
                }
                coord[(kk - 1) as usize] = 0;
            }
            if done {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// adjust -- Algorithm AS 51.2
// Makes proportional adjustment corresponding to CONFIG.
// All parameters are assumed valid without test.
// ---------------------------------------------------------------------------

unsafe fn adjust(
    nvar: c_int,
    x: *mut c_double,
    y: *const c_double,
    z: *const c_double,
    locz: *const c_int,
    dim: *const c_int,
    config: *const c_int,
    d: *mut c_double,
) {
    unsafe {
        // Set size array
        let mut size = vec![0i32; (nvar + 1) as usize];
        size[0] = 1;
        let mut k = 1;
        while k <= nvar {
            let l = *config.add(k as usize);
            if l == 0 {
                break;
            }
            size[k as usize] = size[(k - 1) as usize] * *dim.add(l as usize);
            k += 1;
        }
        let n = k - 1;

        // Test size of deviation
        let l = size[n as usize];
        let mut j = 1; // 1-based into y
        let kk0 = *locz; // 1-based into z
        for _i in 1..=l {
            let e = (*z.add((kk0 - 1) as usize) - *y.add((j - 1) as usize)).abs();
            if e > *d {
                *d = e;
            }
            j += 1;
        }

        // Initialize coordinates
        let mut coord = vec![0i32; nvar as usize];
        let mut i = 1; // 1-based into x

        // Perform adjustment
        loop {
            let mut j = 0; // 0-based offset
            for kk in 1..=n {
                let l = *config.add(kk as usize);
                j += coord[(l - 1) as usize] * size[(kk - 1) as usize];
            }
            let kk = j + *locz - 1; // 0-based into z
            j += 1; // 1-based into y

            // Note that Y(J) should be non-negative
            if *y.add((j - 1) as usize) <= 0.0 {
                *x.add((i - 1) as usize) = 0.0;
            }
            if *y.add((j - 1) as usize) > 0.0 {
                *x.add((i - 1) as usize) =
                    *x.add((i - 1) as usize) * *z.add(kk as usize) / *y.add((j - 1) as usize);
            }

            // Update coordinates
            i += 1;
            let mut done = true;
            for kk in 1..=nvar {
                coord[(kk - 1) as usize] += 1;
                if coord[(kk - 1) as usize] < *dim.add(kk as usize) {
                    done = false;
                    break;
                }
                coord[(kk - 1) as usize] = 0;
            }
            if done {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// loglin -- Algorithm AS 51 main
// ---------------------------------------------------------------------------

unsafe fn loglin(
    nvar: c_int,
    dim: *const c_int,
    ncon: c_int,
    config: *const c_int,
    ntab: c_int,
    table: *const c_double,
    fit: *mut c_double,
    locmar: *mut c_int,
    nmar: c_int,
    marg: *mut c_double,
    nu: c_int,
    u: *mut c_double,
    maxdev: c_double,
    maxit: c_int,
    dev: *mut c_double,
    nlast: *mut c_int,
    ifault: *mut c_int,
) {
    unsafe {
        *ifault = 0;
        *nlast = 0;

        // Check validity of NVAR and maxit
        if nvar <= 0 || maxit <= 0 {
            *ifault = 4;
            return;
        }

        // Look at table and fit constants
        let mut size = 1;
        for j in 0..nvar {
            if *dim.add(j as usize) <= 0 {
                *ifault = 4;
                return;
            }
            size *= *dim.add(j as usize);
        }
        if size > ntab {
            *ifault = 2;
            return;
        }

        let mut x = 0.0;
        let mut y = 0.0;
        for i in 0..size {
            if *table.add(i as usize) < 0.0 || *fit.add(i as usize) < 0.0 {
                *ifault = 4;
                return;
            }
            x += *table.add(i as usize);
            y += *fit.add(i as usize);
        }

        // Make a preliminary adjustment to obtain the fit to an empty configuration list
        if y == 0.0 {
            *ifault = 4;
            return;
        }
        x /= y;
        for i in 0..size {
            *fit.add(i as usize) = x * *fit.add(i as usize);
        }
        if ncon <= 0 || *config.add(0) == 0 {
            return;
        }

        // Allocate marginal tables
        let mut point = 1;
        let mut n = 0;
        for i in 1..=ncon {
            // A zero beginning a configuration indicates that the list is completed
            if *config.add((i * nvar) as usize) == 0 {
                break;
            }
            // Get marginal table size. While doing this task, see if the
            // configuration list contains duplications or elements out of range.
            let mut sz = 1;
            let mut check = vec![0i32; nvar as usize];
            let mut j = 1;
            while j <= nvar {
                let kk = *config.add((j + i * nvar - (nvar + 1)) as usize);
                // A zero indicates the end of the string
                if kk == 0 {
                    break;
                }
                // See if element is valid
                if kk < 0 || kk > nvar {
                    *ifault = 1;
                    return;
                }
                // Check for duplication
                if check[(kk - 1) as usize] != 0 {
                    *ifault = 1;
                    return;
                }
                check[(kk - 1) as usize] = 1;
                // Get size
                sz *= *dim.add((kk - 1) as usize);
                j += 1;
            }

            // Since U is used to store fitted marginals, size must not exceed NU
            if sz > nu {
                *ifault = 2;
                return;
            }

            // LOCMAR points to marginal tables to be placed in MARG
            *locmar.add((i - 1) as usize) = point;
            point += sz;
        }

        // Get N, number of valid configurations
        n = ncon; // i was ncon+1 in C, so n = ncon

        // See if MARG can hold all marginal tables
        if point > nmar + 1 {
            *ifault = 2;
            return;
        }

        // Obtain marginal tables
        let mut icon = vec![0i32; nvar as usize];
        for i in 1..=n {
            for j in 1..=nvar {
                icon[(j - 1) as usize] = *config.add((j + i * nvar - (nvar + 1)) as usize);
            }
            collap(
                nvar,
                table,
                marg,
                *locmar.add((i - 1) as usize),
                dim,
                icon.as_ptr(),
            );
        }

        // Perform iterations
        for k in 1..=maxit {
            // XMAX is maximum deviation observed between fitted and true
            // marginal during a cycle
            let mut xmax = 0.0;
            for i in 1..=n {
                for j in 1..=nvar {
                    icon[(j - 1) as usize] = *config.add((j + i * nvar - (nvar + 1)) as usize);
                }
                collap(nvar, fit, u, 1, dim, icon.as_ptr());
                adjust(
                    nvar,
                    fit,
                    u,
                    marg,
                    locmar.add((i - 1) as usize),
                    dim,
                    icon.as_ptr(),
                    &mut xmax,
                );
            }
            // Test convergence
            *dev.add((k - 1) as usize) = xmax;
            if xmax < maxdev {
                *nlast = k;
                return;
            }
        }

        if maxit > 1 {
            *ifault = 3;
            *nlast = maxit;
        } else {
            *nlast = 1;
        }
    }
}

// ---------------------------------------------------------------------------
// LogLin -- R interface
// ---------------------------------------------------------------------------

pub unsafe fn LogLin(
    dtab: SEXP,
    conf: SEXP,
    table: SEXP,
    start: SEXP,
    snmar: SEXP,
    eps: SEXP,
    iter: SEXP,
) -> SEXP {
    unsafe {
        let nvar = LENGTH(dtab);
        let ncon = ncols(dtab as *const _);
        let ntab = LENGTH(table);
        let nmar = as_integer(snmar);
        let maxit = as_integer(iter);
        let maxdev = as_real(eps);

        let fit = if TYPEOF(start) == SEXPTYPE::REALSXP {
            duplicate(start)
        } else {
            coerceVector(start, SEXPTYPE::REALSXP.as_c_int())
        };
        let _fit_guard = protect(fit);
        let locmar = Rf_allocVector(SEXPTYPE::INTSXP, ncon);
        let _locmar_guard = protect(locmar);
        let marg = Rf_allocVector(SEXPTYPE::REALSXP, nmar);
        let _marg_guard = protect(marg);
        let u = Rf_allocVector(SEXPTYPE::REALSXP, ntab);
        let _u_guard = protect(u);
        let dev = Rf_allocVector(SEXPTYPE::REALSXP, maxit);
        let _dev_guard = protect(dev);
        let dtab = coerceVector(dtab, SEXPTYPE::INTSXP.as_c_int());
        let _dtab_guard = protect(dtab);
        let conf = coerceVector(conf, SEXPTYPE::INTSXP.as_c_int());
        let _conf_guard = protect(conf);
        let table = coerceVector(table, SEXPTYPE::REALSXP.as_c_int());
        let _table_guard = protect(table);

        let mut nlast: c_int = 0;
        let mut ifault: c_int = 0;

        loglin(
            nvar,
            INTEGER(dtab),
            ncon,
            INTEGER(conf),
            ntab,
            REAL(table),
            REAL(fit),
            INTEGER(locmar),
            nmar,
            REAL(marg),
            ntab,
            REAL(u),
            maxdev,
            maxit,
            REAL(dev),
            &mut nlast,
            &mut ifault,
        );

        match ifault {
            1 | 2 => {
                Rf_error(b"this should not happen\0".as_ptr() as *const _);
            }
            3 => {
                Rf_error(b"algorithm did not converge\0".as_ptr() as *const _);
            }
            4 => {
                Rf_error(b"incorrect specification of 'table' or 'start'\0".as_ptr() as *const _);
            }
            _ => {} // intentionally unhandled: unknown convergence error code
        }

        let ans = Rf_allocVector(SEXPTYPE::VECSXP, 3);
        let _ans_guard = protect(ans);
        SET_VECTOR_ELT(ans, 0, fit);
        SET_VECTOR_ELT(ans, 1, dev);
        SET_VECTOR_ELT(ans, 2, Rf_ScalarInteger(nlast));
        let nm = Rf_allocVector(SEXPTYPE::STRSXP, 3);
        let _nm_guard = protect(nm);
        setAttrib(ans, R_NamesSymbol(), nm);
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"fit\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"dev\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 2, Rf_mkChar(b"nlast\0".as_ptr() as *const c_char));
        ans
    }
}
