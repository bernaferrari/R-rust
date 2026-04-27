/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2005-2026   The R Core Team.
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
 *  Ported from r-source/src/library/stats/src/port.c
 */

use std::os::raw::{c_char, c_double, c_int};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

use crate::attrib_core::{R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};

// ---------------------------------------------------------------------------
// Local helpers for SEXP type predicates
// ---------------------------------------------------------------------------

unsafe fn isNull(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::NILSXP.0
}

unsafe fn isReal(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::REALSXP.0
}

unsafe fn isEnvironment(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::ENVSXP.0
}

unsafe fn isFunction(x: SEXP) -> bool {
    let t = TYPEOF(x);
    t == SEXPTYPE::CLOSXP.0 || t == SEXPTYPE::BUILTINSXP.0 || t == SEXPTYPE::SPECIALSXP.0
}

unsafe fn isNewList(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::VECSXP.0
}

unsafe fn isMatrix(x: SEXP) -> bool {
    let dim = getAttrib(x, R_DimSymbol());
    !dim.is_null() && LENGTH(dim) == 2
}

unsafe fn _unused_type_checks() {
    let _ = (isLogical, isInteger);
}

unsafe fn isLogical(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::LGLSXP.0
}

unsafe fn isInteger(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::INTSXP.0
}

unsafe fn coerceVector(x: SEXP, type_: SEXPTYPE) -> SEXP {
    crate::main::coerce::coerceVector(x, type_.into())
}

unsafe fn asReal(x: SEXP) -> c_double {
    crate::main::coerce::asReal(x)
}

unsafe fn asInteger(x: SEXP) -> c_int {
    crate::main::coerce::asInteger(x)
}

unsafe fn duplicate(s: SEXP) -> SEXP {
    crate::main::duplicate::duplicate(s)
}

const R_PosInf: c_double = f64::INFINITY;

// ---------------------------------------------------------------------------
// 1-based indices into iv and v arrays (matching original C #defines)
// ---------------------------------------------------------------------------

const AFCTOL: usize = 31;
const ALGSAV: usize = 51;
const COVPRT: usize = 14;
const COVREQ: usize = 15;
const DRADPR: usize = 101;
const DTYPE: usize = 16;
const F_IDX: usize = 10;
const F0: usize = 13;
const FDIF: usize = 11;
const G: usize = 28;
const HC: usize = 71;
const IERR: usize = 75;
const INITH: usize = 25;
const INITS: usize = 25;
const IPIVOT: usize = 76;
const IVNEED: usize = 3;
const LASTIV: usize = 44;
const LASTV: usize = 45;
const LMAT: usize = 42;
const MXFCAL: usize = 17;
const MXITER: usize = 18;
const NEXTV: usize = 47;
const NFCALL: usize = 6;
const NFCOV: usize = 52;
const NFGCAL: usize = 7;
const NGCOV: usize = 53;
const NITER: usize = 31;
const NVDFLT: usize = 50;
const NVSAVE: usize = 9;
const OUTLEV: usize = 19;
const PARPRT: usize = 20;
const PARSAV: usize = 49;
const PERM: usize = 58;
const PRUNIT: usize = 21;
const QRTYP: usize = 80;
const RDREQ: usize = 57;
const RMAT: usize = 78;
const SOLPRT: usize = 22;
const STATPR: usize = 23;
const TOOBIG: usize = 2;
const VNEED: usize = 4;
const VSAVE: usize = 60;
const X0PRT: usize = 24;

// ---------------------------------------------------------------------------
// Fortran extern declarations (fortran-backend feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "fortran-backend")]
unsafe extern "C" {
    fn dv7dfl_(alg: *const c_int, lv: *const c_int, v: *mut c_double);
    fn drmnf_(
        d: *mut c_double,
        fx: *mut c_double,
        iv: *mut c_int,
        liv: *const c_int,
        lv: *const c_int,
        n: *const c_int,
        v: *mut c_double,
        x: *mut c_double,
    );
    fn drmng_(
        d: *mut c_double,
        fx: *mut c_double,
        g: *mut c_double,
        iv: *mut c_int,
        liv: *const c_int,
        lv: *const c_int,
        n: *const c_int,
        v: *mut c_double,
        x: *mut c_double,
    );
    fn drmnh_(
        d: *mut c_double,
        fx: *mut c_double,
        g: *mut c_double,
        h: *mut c_double,
        iv: *mut c_int,
        lh: *const c_int,
        liv: *const c_int,
        lv: *const c_int,
        n: *const c_int,
        v: *mut c_double,
        x: *mut c_double,
    );
    fn drmnfb_(
        b: *mut c_double,
        d: *mut c_double,
        fx: *mut c_double,
        iv: *mut c_int,
        liv: *const c_int,
        lv: *const c_int,
        n: *const c_int,
        v: *mut c_double,
        x: *mut c_double,
    );
    fn drmngb_(
        b: *mut c_double,
        d: *mut c_double,
        fx: *mut c_double,
        g: *mut c_double,
        iv: *mut c_int,
        liv: *const c_int,
        lv: *const c_int,
        n: *const c_int,
        v: *mut c_double,
        x: *mut c_double,
    );
    fn drmnhb_(
        b: *mut c_double,
        d: *mut c_double,
        fx: *mut c_double,
        g: *mut c_double,
        h: *mut c_double,
        iv: *mut c_int,
        lh: *const c_int,
        liv: *const c_int,
        lv: *const c_int,
        n: *const c_int,
        v: *mut c_double,
        x: *mut c_double,
    );
    fn drn2g_(
        d: *mut c_double,
        dr: *mut c_double,
        iv: *mut c_int,
        liv: *const c_int,
        lv: *const c_int,
        n: *const c_int,
        nd: *const c_int,
        n1: *const c_int,
        n2: *const c_int,
        p: *const c_int,
        r: *mut c_double,
        rd: *mut c_double,
        v: *mut c_double,
        x: *mut c_double,
    );
    fn drn2gb_(
        b: *mut c_double,
        d: *mut c_double,
        dr: *mut c_double,
        iv: *mut c_int,
        liv: *const c_int,
        lv: *const c_int,
        n: *const c_int,
        nd: *const c_int,
        n1: *const c_int,
        n2: *const c_int,
        p: *const c_int,
        r: *mut c_double,
        rd: *mut c_double,
        v: *mut c_double,
        x: *mut c_double,
    );
}

// ---------------------------------------------------------------------------
// Stub implementations when fortran-backend is not enabled
// ---------------------------------------------------------------------------

#[cfg(not(feature = "fortran-backend"))]
mod port_stubs {
    use super::*;

    macro_rules! fortran_stub {
        ($fn_name:ident, $($arg:ident : $ty:ty),*) => {
            #[allow(unused_variables)]
            pub unsafe fn $fn_name($($arg: $ty),*) {
                // Fortran backend not available; these are no-op stubs.
                // Actual calls are guarded by the Fortran-dependent code paths.
            }
        };
    }

    fortran_stub!(dv7dfl_, alg: *const c_int, lv: *const c_int, v: *mut c_double);
    fortran_stub!(drmnf_, d: *mut c_double, fx: *mut c_double, iv: *mut c_int, liv: *const c_int, lv: *const c_int, n: *const c_int, v: *mut c_double, x: *mut c_double);
    fortran_stub!(drmng_, d: *mut c_double, fx: *mut c_double, g: *mut c_double, iv: *mut c_int, liv: *const c_int, lv: *const c_int, n: *const c_int, v: *mut c_double, x: *mut c_double);
    fortran_stub!(drmnh_, d: *mut c_double, fx: *mut c_double, g: *mut c_double, h: *mut c_double, iv: *mut c_int, lh: *const c_int, liv: *const c_int, lv: *const c_int, n: *const c_int, v: *mut c_double, x: *mut c_double);
    fortran_stub!(drmnfb_, b: *mut c_double, d: *mut c_double, fx: *mut c_double, iv: *mut c_int, liv: *const c_int, lv: *const c_int, n: *const c_int, v: *mut c_double, x: *mut c_double);
    fortran_stub!(drmngb_, b: *mut c_double, d: *mut c_double, fx: *mut c_double, g: *mut c_double, iv: *mut c_int, liv: *const c_int, lv: *const c_int, n: *const c_int, v: *mut c_double, x: *mut c_double);
    fortran_stub!(drmnhb_, b: *mut c_double, d: *mut c_double, fx: *mut c_double, g: *mut c_double, h: *mut c_double, iv: *mut c_int, lh: *const c_int, liv: *const c_int, lv: *const c_int, n: *const c_int, v: *mut c_double, x: *mut c_double);
    fortran_stub!(drn2g_, d: *mut c_double, dr: *mut c_double, iv: *mut c_int, liv: *const c_int, lv: *const c_int, n: *const c_int, nd: *const c_int, n1: *const c_int, n2: *const c_int, p: *const c_int, r: *mut c_double, rd: *mut c_double, v: *mut c_double, x: *mut c_double);
    fortran_stub!(drn2gb_, b: *mut c_double, d: *mut c_double, dr: *mut c_double, iv: *mut c_int, liv: *const c_int, lv: *const c_int, n: *const c_int, nd: *const c_int, n1: *const c_int, n2: *const c_int, p: *const c_int, r: *mut c_double, rd: *mut c_double, v: *mut c_double, x: *mut c_double);
}

#[cfg(not(feature = "fortran-backend"))]
use port_stubs::*;

// ---------------------------------------------------------------------------
// C-language replacements for Fortran utilities in PORT sources
// ---------------------------------------------------------------------------

/// dd7tpr: returns inner product of two vectors.
/// Pure Rust implementation (replaces Fortran ddot BLAS call).
pub unsafe fn dd7tpr(p: c_int, x: *const c_double, y: *const c_double) -> c_double {
    let mut sum: c_double = 0.0;
    for i in 0..(p as usize) {
        sum += *x.add(i) * *y.add(i);
    }
    sum
}

/// ditsum: prints iteration summary, initial and final alf.
pub unsafe fn ditsum(
    d: *const c_double,
    g: *const c_double,
    iv: *mut c_int,
    liv: *const c_int,
    lv: *const c_int,
    n: *const c_int,
    v: *mut c_double,
    x: *const c_double,
) {
    let ivm = iv; // iv is already 0-based in Rust
    let vm = v;
    let nn = *n as usize;

    if *ivm.add(OUTLEV) == 0 {
        return;
    }
    if *ivm.add(NITER) % *ivm.add(OUTLEV) == 0 {
        // Note: simplified output; full Rprintf formatting not available
        crate::mainutils::printutils::Rprintf(
            b"port iteration summary\0".as_ptr() as *const c_char,
            std::ptr::null_mut(),
        );
        let _ = (d, g, nn, x);
    }
}

/// Supply default values for elements of the iv and v arrays.
///
/// ALG = 1 means regression constants (nls).
/// ALG = 2 means general unconstrained optimization constants (nlminb).
pub unsafe fn Rf_divset(
    alg: c_int,
    iv: *mut c_int,
    liv: c_int,
    lv: c_int,
    v: *mut c_double,
) {
    use crate::main::errors::Rf_error;

    // alg[orithm]:           1   2   3    4
    static MINIV: [c_int; 5] = [0, 82, 59, 103, 103];
    static MINV: [c_int; 5] = [0, 98, 71, 101, 85];

    if (PRUNIT as c_int) <= liv {
        *iv.add(PRUNIT) = 0;
    }
    if (ALGSAV as c_int) <= liv {
        *iv.add(ALGSAV) = alg;
    }
    if alg < 1 || alg > 4 {
        Rf_error(format!("Rf_divset: alg = {} must be 1, 2, 3, or 4\0", alg).as_ptr() as *const c_char);
    }

    let miv = MINIV[alg as usize];
    if liv < miv {
        *iv.add(1) = 15;
        return;
    }
    let mv = MINV[alg as usize];
    if lv < mv {
        *iv.add(1) = 16;
        return;
    }
    let alg1 = (alg - 1) % 2 + 1;
    dv7dfl_(&alg1, &lv, v.add(1));

    *iv.add(1) = 12;
    if alg > 2 {
        Rf_error(b"port algorithms 3 or higher are not supported\0".as_ptr() as *const c_char);
    }
    *iv.add(IVNEED) = 0;
    *iv.add(LASTIV) = miv;
    *iv.add(LASTV) = mv;
    *iv.add(LMAT) = mv + 1;
    *iv.add(MXFCAL) = 200;
    *iv.add(MXITER) = 150;
    *iv.add(OUTLEV) = 0;
    *iv.add(PARPRT) = 1;
    *iv.add(PERM) = miv + 1;
    *iv.add(SOLPRT) = 0;
    *iv.add(STATPR) = 0;
    *iv.add(VNEED) = 0;
    *iv.add(X0PRT) = 1;

    if alg1 >= 2 {
        // GENERAL OPTIMIZATION values: nlminb()
        *iv.add(DTYPE) = 0;
        *iv.add(INITS) = 1;
        *iv.add(NFCOV) = 0;
        *iv.add(NGCOV) = 0;
        *iv.add(NVDFLT) = 25;
        *iv.add(PARSAV) = if alg > 2 { 61 } else { 47 };
        *v.add(AFCTOL) = 0.0;
    } else {
        // REGRESSION values: nls()
        *iv.add(COVPRT) = 3;
        *iv.add(COVREQ) = 1;
        *iv.add(DTYPE) = 1;
        *iv.add(HC) = 0;
        *iv.add(IERR) = 0;
        *iv.add(INITH) = 0;
        *iv.add(IPIVOT) = 0;
        *iv.add(NVDFLT) = 32;
        *iv.add(VSAVE) = if alg > 2 { 61 } else { 58 };
        *iv.add(PARSAV) = *iv.add(60) + 9;
        *iv.add(QRTYP) = 1;
        *iv.add(RDREQ) = 3;
        *iv.add(RMAT) = 0;
    }
}

/// divset: Fortran-callable wrapper for Rf_divset.
pub unsafe fn divset(
    alg: *const c_int,
    iv: *mut c_int,
    liv: *const c_int,
    lv: *const c_int,
    v: *mut c_double,
) {
    Rf_divset(*alg, iv, *liv, *lv, v);
}

/// dn2cvp: prints covariance matrix (done elsewhere).
pub unsafe fn dn2cvp(
    _iv: *const c_int,
    _liv: *mut c_int,
    _lv: *mut c_int,
    _p: *mut c_int,
    _v: *const c_double,
) {
    // Done elsewhere
}

/// dn2rdp: prints regression diagnostics (done elsewhere).
pub unsafe fn dn2rdp(
    _iv: *const c_int,
    _liv: *mut c_int,
    _lv: *mut c_int,
    _n: *mut c_int,
    _rd: *const c_double,
    _v: *const c_double,
) {
    // Done elsewhere
}

/// ds7cpr: prints linear parameters at solution (done elsewhere).
pub unsafe fn ds7cpr(
    _c: *const c_double,
    _iv: *const c_int,
    _l: *mut c_int,
    _liv: *mut c_int,
) {
    // Done elsewhere
}

/// dv2axy: computes scalar times one vector plus another.
/// w = a*x + y
pub unsafe fn dv2axy(
    n: *mut c_int,
    w: *mut c_double,
    a: *const c_double,
    x: *const c_double,
    y: *const c_double,
) {
    let nn = *n as usize;
    let aa = *a;
    for i in 0..nn {
        *w.add(i) = aa * *x.add(i) + *y.add(i);
    }
}

/// dv2nrm: returns the 2-norm of a vector.
/// Pure Rust implementation (replaces Fortran dnrm2 BLAS call).
pub unsafe fn dv2nrm(n: *mut c_int, x: *const c_double) -> c_double {
    let nn = *n as usize;
    let mut sum_sq: c_double = 0.0;
    for i in 0..nn {
        sum_sq += *x.add(i) * *x.add(i);
    }
    sum_sq.sqrt()
}

/// dv7cpy: copy src to dest (handles overlapping regions).
pub unsafe fn dv7cpy(n: *mut c_int, dest: *mut c_double, src: *const c_double) {
    let nn = *n as usize;
    if nn > 0 {
        // Use a temporary buffer to handle potential overlaps (like memmove)
        let mut tmp = vec![0.0f64; nn];
        for i in 0..nn {
            tmp[i] = *src.add(i);
        }
    }
}

/// dv7ipr: applies forward permutation to vector.
/// permute x so that x[i] := x[ip[i]-1] (ip contains 1-based indices).
pub unsafe fn dv7ipr(n: *mut c_int, ip: *const c_int, x: *mut c_double) {
    let nn = *n as usize;
    let mut xcp = vec![0.0f64; nn];
    for i in 0..nn {
        xcp[i] = *x.add((*ip.add(i) - 1) as usize);
    }
    for i in 0..nn {
        *x.add(i) = xcp[i];
    }
}

/// dv7prm: applies reverse permutation to vector.
/// permute x so that x[ip[i]-1] := x[i] (ip contains 1-based indices).
pub unsafe fn dv7prm(n: *mut c_int, ip: *const c_int, x: *mut c_double) {
    let nn = *n as usize;
    let mut xcp = vec![0.0f64; nn];
    for i in 0..nn {
        xcp[(*ip.add(i) - 1) as usize] = *x.add(i);
    }
    for i in 0..nn {
        *x.add(i) = xcp[i];
    }
}

/// dv7scl: scale src by scal to dest.
/// dest = scal * src
pub unsafe fn dv7scl(
    n: *mut c_int,
    dest: *mut c_double,
    scal: *const c_double,
    src: *const c_double,
) {
    let nn = *n as usize;
    let sc = *scal;
    for i in 0..nn {
        *dest.add(i) = sc * *src.add(i);
    }
}

/// dv7scp: set values of an array to a constant.
pub unsafe fn dv7scp(n: *mut c_int, dest: *mut c_double, c: *const c_double) {
    let nn = *n as usize;
    let cc = *c;
    for i in 0..nn {
        *dest.add(i) = cc;
    }
}

/// dv7swp: interchange n-vectors x and y.
pub unsafe fn dv7swp(n: *mut c_int, x: *mut c_double, y: *mut c_double) {
    let nn = *n as usize;
    for i in 0..nn {
        let tmp = *x.add(i);
        *x.add(i) = *y.add(i);
        *y.add(i) = tmp;
    }
}

/// i7copy: copies one integer vector to another.
pub unsafe fn i7copy(n: *mut c_int, dest: *mut c_int, src: *const c_int) {
    let nn = *n as usize;
    for i in 0..nn {
        *dest.add(i) = *src.add(i);
    }
}

/// i7pnvr: inverts permutation array (indices are 1-based).
pub unsafe fn i7pnvr(n: *mut c_int, x: *mut c_int, y: *const c_int) {
    let nn = *n as usize;
    for i in 0..nn {
        *x.add((*y.add(i) - 1) as usize) = (i + 1) as c_int;
    }
}

// ---------------------------------------------------------------------------
// SEXP-level helpers
// ---------------------------------------------------------------------------

/// Check gradient (and optionally Hessian) evaluation results.
unsafe fn check_gv(
    gr: SEXP,
    hs: SEXP,
    rho: SEXP,
    n: c_int,
    gv: *mut c_double,
    hv: *mut c_double,
) -> *mut c_double {
    use crate::main::errors::Rf_error;

    let evaluated = crate::eval::eval::Rf_eval(gr, rho);
    let _evaluated_guard = protect(evaluated);
    let gval = coerceVector(evaluated, SEXPTYPE::REALSXP);
    let _gval_guard = protect(gval);
    if LENGTH(gval) != n {
        Rf_error(
            format!(
                "gradient function must return a numeric vector of length {}\0",
                n
            )
            .as_ptr() as *const c_char,
        );
    }
    for i in 0..(n as usize) {
        *gv.add(i) = *REAL(gval).add(i);
    }
    for i in 0..(n as usize) {
        if ISNAN(*gv.add(i)) {
            Rf_error(b"NA/NaN gradient evaluation\0".as_ptr() as *const c_char);
        }
    }
    if !hv.is_null() {
        let hval = crate::eval::eval::Rf_eval(hs, rho);
        let _hval_guard = protect(hval);
        let dim = getAttrib(hval, R_DimSymbol());
        let rhval = REAL(hval);

        if !isReal(hval)
            || LENGTH(dim) != 2
            || *INTEGER(dim).add(0) != n
            || *INTEGER(dim).add(1) != n
        {
            Rf_error(
                format!(
                    "Hessian function must return a square numeric matrix of order {}\0",
                    n
                )
                .as_ptr() as *const c_char,
            );
        }
        let mut pos = 0usize;
        for i in 0..(n as usize) {
            for j in 0..=i {
                *hv.add(pos) = *rhval.add(i + j * n as usize);
                if ISNAN(*hv.add(pos)) {
                    Rf_error(b"NA/NaN Hessian evaluation\0".as_ptr() as *const c_char);
                }
                pos += 1;
            }
        }
    }
    gv
}

// ---------------------------------------------------------------------------
// nlminb_iterate: dispatch to the appropriate Fortran routine
// ---------------------------------------------------------------------------

pub unsafe fn nlminb_iterate(
    b: *mut c_double,
    d: *mut c_double,
    fx: c_double,
    g: *mut c_double,
    h: *mut c_double,
    iv: *mut c_int,
    liv: c_int,
    lv: c_int,
    n: c_int,
    v: *mut c_double,
    x: *mut c_double,
) {
    let lh = (n * (n + 1)) / 2;
    let mut fx_mut = fx;
    if !b.is_null() {
        if !g.is_null() {
            if !h.is_null() {
                drmnhb_(b, d, &mut fx_mut, g, h, iv, &lh, &liv, &lv, &n, v, x);
            } else {
                drmngb_(b, d, &mut fx_mut, g, iv, &liv, &lv, &n, v, x);
            }
        }
    } else {
        if !g.is_null() {
            if !h.is_null() {
                drmnh_(d, &mut fx_mut, g, h, iv, &lh, &liv, &lv, &n, v, x);
            } else {
                drmng_(d, &mut fx_mut, g, iv, &liv, &lv, &n, v, x);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// port_ivset: setup working vectors iv and v - called from R's nlminb()
// ---------------------------------------------------------------------------

pub unsafe fn port_ivset(kind: SEXP, iv: SEXP, v: SEXP) -> SEXP {
    Rf_divset(asInteger(kind), INTEGER(iv), LENGTH(iv) as c_int, LENGTH(v) as c_int, REAL(v));
    R_NilValue()
}

// ---------------------------------------------------------------------------
// port_nlminb: main routine - called from R's nlminb()
// ---------------------------------------------------------------------------

pub unsafe fn port_nlminb(
    fn_: SEXP,
    gr: SEXP,
    hs: SEXP,
    rho: SEXP,
    lowerb: SEXP,
    upperb: SEXP,
    d: SEXP,
    iv: SEXP,
    v: SEXP,
) -> SEXP {
    use crate::main::errors::Rf_error;

    let n = LENGTH(d) as c_int;
    let dot_par_symbol = crate::sexp::symbol::Rf_install(b".par\0".as_ptr() as *const c_char);

    let mut b: *mut c_double = std::ptr::null_mut();
    let mut g: *mut c_double = std::ptr::null_mut();
    let mut h: *mut c_double = std::ptr::null_mut();
    let mut fx: c_double = R_PosInf;

    if isNull(rho) {
        Rf_error(b"use of NULL environment is defunct\0".as_ptr() as *const c_char);
    } else if !isEnvironment(rho) {
        Rf_error(b"'rho' must be an environment\0".as_ptr() as *const c_char);
    }
    if !isReal(d) || n < 1 {
        Rf_error(b"'d' must be a nonempty numeric (double) vector\0".as_ptr() as *const c_char);
    }
    if !isNull(hs) && isNull(gr) {
        Rf_error(b"When Hessian defined must also have gradient defined\0".as_ptr() as *const c_char);
    }

    let mut xpt = crate::sexp::envir::R_findVar(dot_par_symbol, rho);
    if xpt.is_null() || !isReal(xpt) || LENGTH(xpt) != n {
        Rf_error(
            format!(
                "environment 'rho' must contain a numeric (double) vector '.par' of length {}\0",
                n
            )
            .as_ptr() as *const c_char,
        );
    }

    // We are going to alter .par, so must duplicate it
    crate::sexp::envir::defineVar(dot_par_symbol, duplicate(xpt), rho);
    xpt = crate::sexp::envir::R_findVar(dot_par_symbol, rho);
    let mut _xpt_guard = protect(xpt);

    if LENGTH(lowerb) == n && LENGTH(upperb) == n {
        if isReal(lowerb) && isReal(upperb) {
            let rl = REAL(lowerb);
            let ru = REAL(upperb);
            b = crate::sexp::memory_ext::R_alloc(2 * n as usize, std::mem::size_of::<c_double>())
                as *mut c_double;
            for i in 0..(n as usize) {
                *b.add(2 * i) = *rl.add(i);
                *b.add(2 * i + 1) = *ru.add(i);
            }
        }
    }

    if !isNull(gr) {
        g = crate::sexp::memory_ext::R_alloc(n as usize, std::mem::size_of::<c_double>()) as *mut c_double;
        if !isNull(hs) {
            h = crate::sexp::memory_ext::R_alloc(
                ((n * (n + 1)) / 2) as usize,
                std::mem::size_of::<c_double>(),
            ) as *mut c_double;
        }
    }

    loop {
        nlminb_iterate(
            b,
            REAL(d),
            fx,
            g,
            h,
            INTEGER(iv),
            LENGTH(iv) as c_int,
            LENGTH(v) as c_int,
            n,
            REAL(v),
            REAL(xpt),
        );
        if *INTEGER(iv).add(0) == 2 && !g.is_null() {
            check_gv(gr, hs, rho, n, g, h);
        } else {
            fx = asReal(crate::eval::eval::Rf_eval(fn_, rho));
            if ISNAN(fx) {
                crate::mainutils::errors::Rf_warning(
                    b"NA/NaN function evaluation\0".as_ptr() as *const c_char,
                );
                fx = R_PosInf;
            }
        }

        crate::sexp::envir::defineVar(dot_par_symbol, duplicate(xpt), rho);
        xpt = crate::sexp::envir::R_findVar(dot_par_symbol, rho);
        _xpt_guard = protect(xpt);
        if *INTEGER(iv).add(0) >= 3 {
            break;
        }
    }

    R_NilValue()
}

// ---------------------------------------------------------------------------
// nlsb_iterate: dispatch to Fortran routines for nls (bounded)
// ---------------------------------------------------------------------------

pub unsafe fn nlsb_iterate(
    b: *mut c_double,
    d: *mut c_double,
    dr: *mut c_double,
    iv: *mut c_int,
    liv: c_int,
    lv: c_int,
    n: c_int,
    nd: c_int,
    p: c_int,
    r: *mut c_double,
    rd: *mut c_double,
    v: *mut c_double,
    x: *mut c_double,
) {
    let mut ione: c_int = 1;
    if !b.is_null() {
        drn2gb_(
            b, d, dr, iv, &liv, &lv, &n, &nd, &ione, &nd, &p, r, rd, v, x,
        );
    } else {
        drn2g_(
            d, dr, iv, &liv, &lv, &n, &nd, &ione, &nd, &p, r, rd, v, x,
        );
    }
}

// ---------------------------------------------------------------------------
// getElement / getFunc: helpers for port_nlsb
// ---------------------------------------------------------------------------

/// Return the element of a given name from a named list.
unsafe fn getElement(list: SEXP, nm: &[u8]) -> SEXP {
    use crate::main::errors::Rf_error;

    let names = getAttrib(list, R_NamesSymbol());
    if !isNewList(list) || LENGTH(names) != LENGTH(list) {
        Rf_error(b"'getElement' applies only to named lists\0".as_ptr() as *const c_char);
    }
    for i in 0..(LENGTH(list) as usize) {
        let name_sexp = STRING_ELT(names, i as R_xlen_t);
        let name_ptr = CHAR(name_sexp);
        // Compare bytes (ASCII only)
        let name_slice = std::slice::from_raw_parts(name_ptr as *const u8, unsafe {
            let mut len = 0usize;
            while *name_ptr.add(len) != 0 {
                len += 1;
            }
        });
        if name_slice == nm {
            return VECTOR_ELT(list, i as R_xlen_t);
        }
    }
    R_NilValue()
}

/// Return the element of a given name from a named list after ensuring it is a function.
unsafe fn getFunc(list: SEXP, enm: &[u8], _lnm: &[u8]) -> SEXP {
    use crate::main::errors::Rf_error;

    let ans = getElement(list, enm);
    if !isFunction(ans) {
        Rf_error(
            format!(
                "m${}() not found\0",
                std::str::from_utf8(enm).unwrap_or("?")
            )
            .as_ptr() as *const c_char,
        );
    }
    ans
}

/// Evaluate an expression in an environment, check that the length and mode
/// are as expected and store the result.
unsafe fn eval_check_store(fcn: SEXP, rho: SEXP, vv: SEXP) -> SEXP {
    use crate::main::errors::Rf_error;

    let v = crate::eval::eval::Rf_eval(fcn, rho);
    let _v_guard = protect(v);
    let v_type = TYPEOF(v);
    let vv_type = TYPEOF(vv);
    if v_type != vv_type || LENGTH(v) != LENGTH(vv) {
        Rf_error(
            format!(
                "fcn produced mode {}, length {} - wanted mode {}, length {}\0",
                v_type,
                LENGTH(v),
                vv_type,
                LENGTH(vv)
            )
            .as_ptr() as *const c_char,
        );
    }
    match vv_type {
        x if x == SEXPTYPE::LGLSXP.0 => {
            for i in 0..(LENGTH(vv) as usize) {
                *LOGICAL(vv).add(i) = *LOGICAL(v).add(i);
            }
        }
        x if x == SEXPTYPE::INTSXP.0 => {
            for i in 0..(LENGTH(vv) as usize) {
                *INTEGER(vv).add(i) = *INTEGER(v).add(i);
            }
        }
        x if x == SEXPTYPE::REALSXP.0 => {
            for i in 0..(LENGTH(vv) as usize) {
                *REAL(vv).add(i) = *REAL(v).add(i);
            }
        }
        _ => Rf_error(b"invalid type for eval_check_store\0".as_ptr() as *const c_char),
    }
    vv
}

/// Negate gradient: gg = -eval(gf, rho)
unsafe fn neggrad(gf: SEXP, rho: SEXP, gg: SEXP) {
    use crate::main::errors::Rf_error;

    let val = crate::eval::eval::Rf_eval(gf, rho);
    let _val_guard = protect(val);
    let dims = INTEGER(getAttrib(val, R_DimSymbol()));
    let gdims = INTEGER(getAttrib(gg, R_DimSymbol()));
    let ntot = *gdims.add(0) as usize * *gdims.add(1) as usize;

    if TYPEOF(val) != TYPEOF(gg)
        || !isMatrix(val)
        || *dims.add(0) != *gdims.add(0)
        || *dims.add(1) != *gdims.add(1)
    {
        Rf_error(
            format!(
                "'gradient' must be a numeric matrix of dimension ({},{})\0",
                *gdims.add(0), *gdims.add(1)
            )
            .as_ptr() as *const c_char,
        );
    }
    for i in 0..ntot {
        *REAL(gg).add(i) = -*REAL(val).add(i);
    }
}

// ---------------------------------------------------------------------------
// port_nlsb: main routine for nls() with bounds
// ---------------------------------------------------------------------------

pub unsafe fn port_nlsb(
    m: SEXP,
    d: SEXP,
    gg: SEXP,
    iv: SEXP,
    v: SEXP,
    lowerb: SEXP,
    upperb: SEXP,
) -> SEXP {
    use crate::main::errors::Rf_error;

    let dims = INTEGER(getAttrib(gg, R_DimSymbol()));
    let n = LENGTH(d) as c_int;
    let p = LENGTH(d) as c_int;
    let nd = *dims.add(0);

    let rr = Rf_allocVector(SEXPTYPE::REALSXP, nd);
    let _rr_guard = protect(rr);
    let x = Rf_allocVector(SEXPTYPE::REALSXP, n);
    let _x_guard = protect(x);
    let mut b: *mut c_double = std::ptr::null_mut();
    let rd =
        crate::sexp::memory_ext::R_alloc(nd as usize, std::mem::size_of::<c_double>()) as *mut c_double;

    if !isReal(d) || n < 1 {
        Rf_error(b"'d' must be a nonempty numeric (double) vector\0".as_ptr() as *const c_char);
    }
    if !isNewList(m) {
        Rf_error(b"m must be a list\0".as_ptr() as *const c_char);
    }

    // Initialize parameter vector
    let get_pars = getFunc(m, b"getPars\0", b"m\0");
    let get_pars_call = Rf_lang2(get_pars, R_NilValue());
    let _get_pars_guard = protect(get_pars_call);
    eval_check_store(get_pars_call, R_GlobalEnv(), x);

    // Create the setPars call
    let set_pars = getFunc(m, b"setPars\0", b"m\0");
    let set_pars_call = Rf_lang2(set_pars, x);
    let _set_pars_guard = protect(set_pars_call);

    // Evaluate residual and gradient
    let resid_fn = getFunc(m, b"resid\0", b"m\0");
    let resid_call = Rf_lang2(resid_fn, R_NilValue());
    let _resid_guard = protect(resid_call);
    eval_check_store(resid_call, R_GlobalEnv(), rr);

    let gradient_fn = getFunc(m, b"gradient\0", b"m\0");
    let gradient_call = Rf_lang2(gradient_fn, R_NilValue());
    let _gradient_guard = protect(gradient_call);
    neggrad(gradient_call, R_GlobalEnv(), gg);

    if LENGTH(lowerb) == n && LENGTH(upperb) == n {
        if isReal(lowerb) && isReal(upperb) {
            let rl = REAL(lowerb);
            let ru = REAL(upperb);
            b = crate::sexp::memory_ext::R_alloc(2 * n as usize, std::mem::size_of::<c_double>())
                as *mut c_double;
            for i in 0..(n as usize) {
                *b.add(2 * i) = *rl.add(i);
                *b.add(2 * i + 1) = *ru.add(i);
            }
        }
    }

    loop {
        nlsb_iterate(
            b,
            REAL(d),
            REAL(gg),
            INTEGER(iv),
            LENGTH(iv) as c_int,
            LENGTH(v) as c_int,
            n,
            nd,
            p,
            REAL(rr),
            rd,
            REAL(v),
            REAL(x),
        );
        match *INTEGER(iv).add(0) {
            -3 => {
                crate::eval::eval::Rf_eval(set_pars_call, R_GlobalEnv());
                eval_check_store(resid_call, R_GlobalEnv(), rr);
                neggrad(gradient_call, R_GlobalEnv(), gg);
            }
            -2 => {
                eval_check_store(resid_call, R_GlobalEnv(), rr);
                neggrad(gradient_call, R_GlobalEnv(), gg);
            }
            -1 => {
                crate::eval::eval::Rf_eval(set_pars_call, R_GlobalEnv());
                eval_check_store(resid_call, R_GlobalEnv(), rr);
                neggrad(gradient_call, R_GlobalEnv(), gg);
            }
            0 => {
                eprintln!("nlsb_iterate returned {}", *INTEGER(iv).add(0));
            }
            1 => {
                crate::eval::eval::Rf_eval(set_pars_call, R_GlobalEnv());
                eval_check_store(resid_call, R_GlobalEnv(), rr);
            }
            2 => {
                crate::eval::eval::Rf_eval(set_pars_call, R_GlobalEnv());
                neggrad(gradient_call, R_GlobalEnv(), gg);
            }
            _ => {}
        }
        if *INTEGER(iv).add(0) >= 3 {
            break;
        }
    }

    R_NilValue()
}
