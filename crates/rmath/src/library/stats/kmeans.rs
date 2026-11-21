use core::ffi::c_int;
use std::slice;

use crate::main::errors::{Rf_error, Rf_error_cstr};
use crate::sexp::ffi::SEXP;

fn kmeans_lloyd_impl(
    x: &[f64],
    n: c_int,
    p: c_int,
    k: c_int,
    cen: &mut [f64],
    cl: &mut [c_int],
    maxiter: &mut c_int,
    nc: &mut [c_int],
    wss: &mut [f64],
) {
    let mut iter: c_int;
    let mut inew: c_int = 0;

    cl.iter_mut().take(n as usize).for_each(|slot| *slot = -1);

    iter = 0;
    while iter < *maxiter {
        let mut updated = false;
        for i in 0..n {
            let mut best = f64::INFINITY;
            for j in 0..k {
                let mut dd = 0.0f64;
                for c in 0..p {
                    let tmp = x[(i + n * c) as usize] - cen[(j + k * c) as usize];
                    dd += tmp * tmp;
                }
                if dd < best {
                    best = dd;
                    inew = j + 1;
                }
            }
            if cl[i as usize] != inew {
                updated = true;
                cl[i as usize] = inew;
            }
        }
        if !updated {
            break;
        }

        cen.fill(0.0);
        nc.fill(0);
        for i in 0..n {
            let it = cl[i as usize] - 1;
            nc[it as usize] += 1;
            for c in 0..p {
                cen[(it + c * k) as usize] += x[(i + c * n) as usize];
            }
        }
        for j in 0..(k * p) {
            let idx = j % k;
            if nc[idx as usize] > 0 {
                cen[j as usize] /= nc[idx as usize] as f64;
            }
        }
        iter += 1;
    }

    // GNU counts the terminating scan, including the exhaustion sentinel.
    *maxiter = iter.saturating_add(1);
    wss.fill(0.0);
    for i in 0..n {
        let it = cl[i as usize] - 1;
        for c in 0..p {
            let tmp = x[(i + n * c) as usize] - cen[(it + k * c) as usize];
            wss[it as usize] += tmp * tmp;
        }
    }
}

fn kmeans_macqueen_impl(
    x: &[f64],
    n: c_int,
    p: c_int,
    k: c_int,
    cen: &mut [f64],
    cl: &mut [c_int],
    maxiter: &mut c_int,
    nc: &mut [c_int],
    wss: &mut [f64],
) {
    let mut iter: c_int;
    let mut inew: c_int = 0;

    for i in 0..n {
        let mut best = f64::INFINITY;
        for j in 0..k {
            let mut dd = 0.0f64;
            for c in 0..p {
                let tmp = x[(i + n * c) as usize] - cen[(j + k * c) as usize];
                dd += tmp * tmp;
            }
            if dd < best {
                best = dd;
                inew = j + 1;
            }
        }
        cl[i as usize] = inew;
    }

    cen.fill(0.0);
    nc.fill(0);
    for i in 0..n {
        let it = cl[i as usize] - 1;
        nc[it as usize] += 1;
        for c in 0..p {
            cen[(it + c * k) as usize] += x[(i + c * n) as usize];
        }
    }
    for j in 0..(k * p) {
        let idx = j % k;
        if nc[idx as usize] > 0 {
            cen[j as usize] /= nc[idx as usize] as f64;
        }
    }

    iter = 0;
    while iter < *maxiter {
        let mut updated = false;
        for i in 0..n {
            let mut best = f64::INFINITY;
            for j in 0..k {
                let mut dd = 0.0f64;
                for c in 0..p {
                    let tmp = x[(i + n * c) as usize] - cen[(j + k * c) as usize];
                    dd += tmp * tmp;
                }
                if dd < best {
                    best = dd;
                    inew = j;
                }
            }
            let iold = cl[i as usize] - 1;
            if iold != inew {
                updated = true;
                cl[i as usize] = inew + 1;
                nc[iold as usize] -= 1;
                nc[inew as usize] += 1;
                for c in 0..p {
                    let nci = nc[iold as usize];
                    let ncn = nc[inew as usize];
                    if nci > 0 {
                        cen[(iold + k * c) as usize] +=
                            (cen[(iold + k * c) as usize] - x[(i + n * c) as usize]) / nci as f64;
                    }
                    if ncn > 0 {
                        cen[(inew + k * c) as usize] +=
                            (x[(i + n * c) as usize] - cen[(inew + k * c) as usize]) / ncn as f64;
                    }
                }
            }
        }
        if !updated {
            break;
        }
        iter += 1;
    }

    // GNU counts the terminating scan, including the exhaustion sentinel.
    *maxiter = iter.saturating_add(1);
    wss.fill(0.0);
    for i in 0..n {
        let it = cl[i as usize] - 1;
        for c in 0..p {
            let tmp = x[(i + n * c) as usize] - cen[(it + k * c) as usize];
            wss[it as usize] += tmp * tmp;
        }
    }
}

pub unsafe fn kmeans_Lloyd(
    x: *mut f64,
    pn: *const c_int,
    pp: *const c_int,
    cen: *mut f64,
    pk: *const c_int,
    cl: *mut c_int,
    pmaxiter: *mut c_int,
    nc: *mut c_int,
    wss: *mut f64,
) {
    // SAFETY: the caller passes the `.C` argument buffers: readable scalars
    // `n`, `p`, `k`, an `n * p` data matrix, a `k * p` centre matrix, `n`
    // cluster codes, `k` counts and sums of squares, and a writable
    // iteration count, all distinct, live and unaliased for this call.
    let (n, p, k, x, cen, cl, maxiter, nc, wss) = unsafe {
        let (n, k, p) = (*pn, *pk, *pp);
        (
            n,
            p,
            k,
            slice::from_raw_parts(x, (n * p) as usize),
            slice::from_raw_parts_mut(cen, (k * p) as usize),
            slice::from_raw_parts_mut(cl, n as usize),
            &mut *pmaxiter,
            slice::from_raw_parts_mut(nc, k as usize),
            slice::from_raw_parts_mut(wss, k as usize),
        )
    };
    kmeans_lloyd_impl(x, n, p, k, cen, cl, maxiter, nc, wss);
}

pub unsafe fn kmeans_MacQueen(
    x: *mut f64,
    pn: *const c_int,
    pp: *const c_int,
    cen: *mut f64,
    pk: *const c_int,
    cl: *mut c_int,
    pmaxiter: *mut c_int,
    nc: *mut c_int,
    wss: *mut f64,
) {
    // SAFETY: the caller passes the `.C` argument buffers: readable scalars
    // `n`, `p`, `k`, an `n * p` data matrix, a `k * p` centre matrix, `n`
    // cluster codes, `k` counts and sums of squares, and a writable
    // iteration count, all distinct, live and unaliased for this call.
    let (n, p, k, x, cen, cl, maxiter, nc, wss) = unsafe {
        let (n, k, p) = (*pn, *pk, *pp);
        (
            n,
            p,
            k,
            slice::from_raw_parts(x, (n * p) as usize),
            slice::from_raw_parts_mut(cen, (k * p) as usize),
            slice::from_raw_parts_mut(cl, n as usize),
            &mut *pmaxiter,
            slice::from_raw_parts_mut(nc, k as usize),
            slice::from_raw_parts_mut(wss, k as usize),
        )
    };
    kmeans_macqueen_impl(x, n, p, k, cen, cl, maxiter, nc, wss);
}
pub unsafe extern "C-unwind" fn c_kmeans_lloyd(
    x: *mut std::ffi::c_void,
    m: *mut std::ffi::c_void,
    p: *mut std::ffi::c_void,
    centers: *mut std::ffi::c_void,
    k: *mut std::ffi::c_void,
    c1: *mut std::ffi::c_void,
    iter: *mut std::ffi::c_void,
    nc: *mut std::ffi::c_void,
    wss: *mut std::ffi::c_void,
) {
    unsafe {
        kmeans_Lloyd(
            x as *mut f64,
            m as *const c_int,
            p as *const c_int,
            centers as *mut f64,
            k as *const c_int,
            c1 as *mut c_int,
            iter as *mut c_int,
            nc as *mut c_int,
            wss as *mut f64,
        );
    }
}

pub unsafe extern "C-unwind" fn c_kmeans_macqueen(
    x: *mut std::ffi::c_void,
    m: *mut std::ffi::c_void,
    p: *mut std::ffi::c_void,
    centers: *mut std::ffi::c_void,
    k: *mut std::ffi::c_void,
    c1: *mut std::ffi::c_void,
    iter: *mut std::ffi::c_void,
    nc: *mut std::ffi::c_void,
    wss: *mut std::ffi::c_void,
) {
    unsafe {
        kmeans_MacQueen(
            x as *mut f64,
            m as *const c_int,
            p as *const c_int,
            centers as *mut f64,
            k as *const c_int,
            c1 as *mut c_int,
            iter as *mut c_int,
            nc as *mut c_int,
            wss as *mut f64,
        );
    }
}

// Fortran tracing stubs (F77_SUB name mangling: lowercase + underscore suffix)
pub fn kmns1_(_k: *const c_int, _it: *const c_int, _indx: *const c_int) {
    Rf_error_cstr(c"kmeans tracing stub kmns1_ is not implemented");
}

pub fn kmnsqpr_(
    _istep: *const c_int,
    _icoun: *const c_int,
    _ncp: *const c_int,
    _k: *const c_int,
    _trace: *const c_int,
) {
    Rf_error_cstr(c"kmeans tracing stub kmnsqpr_ is not implemented");
}

/// GNU `kmeans(x, k)` Lloyd, min/max initial centers.
pub fn do_kmeans(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use crate::sexp::accessors::{
        CAR, CDR, INTEGER_ELT, REAL_ELT, SET_INTEGER_ELT, SET_REAL_ELT, SET_VECTOR_ELT, TYPEOF,
        XLENGTH,
    };
    use crate::sexp::constructors::Rf_allocVector3;
    use crate::sexp::ffi::{SEXP, SEXPTYPE};
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::protect::protect;
    let x0 = CAR(args);
    let karg = CAR(CDR(args));
    let n = XLENGTH(x0) as c_int;
    let k = if TYPEOF(karg) == SEXPTYPE::INTSXP {
        INTEGER_ELT(karg, 0)
    } else if TYPEOF(karg) == SEXPTYPE::REALSXP {
        REAL_ELT(karg, 0) as c_int
    } else {
        2
    };
    if k < 1 || k > n {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "number of cluster centres must lie between 1 and nrow(x)",
        );
    }
    let mut x = vec![0.0f64; n as usize];
    let mut xmin = f64::INFINITY;
    let mut xmax = f64::NEG_INFINITY;
    for i in 0..n as usize {
        let v = if TYPEOF(x0) == SEXPTYPE::REALSXP {
            REAL_ELT(x0, i as i32)
        } else {
            INTEGER_ELT(x0, i as i32) as f64
        };
        x[i] = v;
        xmin = xmin.min(v);
        xmax = xmax.max(v);
    }
    let mut cen = vec![0.0f64; k as usize];
    if k == 1 {
        cen[0] = xmin;
    } else {
        for j in 0..k as usize {
            cen[j] = xmin + (xmax - xmin) * (j as f64) / ((k - 1) as f64);
        }
    }
    let mut cl = vec![0i32; n as usize];
    let mut nc = vec![0i32; k as usize];
    let mut wss = vec![0.0f64; k as usize];
    let mut maxiter: c_int = 10;
    let p: c_int = 1;
    kmeans_lloyd_impl(
        &x,
        n,
        p,
        k,
        &mut cen,
        &mut cl,
        &mut maxiter,
        &mut nc,
        &mut wss,
    );
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
    let _r = protect(result);
    let cluster = Rf_allocVector3(SEXPTYPE::INTSXP, n as i64);
    for i in 0..n as usize {
        SET_INTEGER_ELT(cluster, i as i32, cl[i]);
    }
    let centers = Rf_allocVector3(SEXPTYPE::REALSXP, k as i64);
    for j in 0..k as usize {
        SET_REAL_ELT(centers, j as i32, cen[j]);
    }
    let size = Rf_allocVector3(SEXPTYPE::INTSXP, k as i64);
    for j in 0..k as usize {
        SET_INTEGER_ELT(size, j as i32, nc[j]);
    }
    SET_VECTOR_ELT(result, 0, cluster);
    SET_VECTOR_ELT(result, 1, centers);
    SET_VECTOR_ELT(result, 2, size);
    crate::mainutils::essentials::set_string_names(
        result,
        &[
            "cluster".to_string(),
            "centers".to_string(),
            "size".to_string(),
        ],
    );
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_ClassSymbol(),
        crate::library::stats::checked::mk_string("kmeans"),
    );
    let _ = R_NilValue();
    result
}

/// `.Fortran(C_kmns, ...)`: Hartigan-Wong (AS 136).
pub unsafe extern "C-unwind" fn c_kmns(
    x: *mut std::ffi::c_void,
    m: *mut std::ffi::c_void,
    p: *mut std::ffi::c_void,
    centers: *mut std::ffi::c_void,
    k: *mut std::ffi::c_void,
    c1: *mut std::ffi::c_void,
    c2: *mut std::ffi::c_void,
    nc: *mut std::ffi::c_void,
    an1: *mut std::ffi::c_void,
    an2: *mut std::ffi::c_void,
    ncp: *mut std::ffi::c_void,
    d: *mut std::ffi::c_void,
    itran: *mut std::ffi::c_void,
    live: *mut std::ffi::c_void,
    iter: *mut std::ffi::c_void,
    wss: *mut std::ffi::c_void,
    ifault: *mut std::ffi::c_void,
) {
    use std::os::raw::c_int;
    unsafe {
        let mm = *(m as *const c_int);
        let nn = *(p as *const c_int);
        let kk = *(k as *const c_int);
        super::kmns::kmns(
            std::slice::from_raw_parts_mut(x as *mut f64, (mm * nn) as usize),
            mm,
            nn,
            std::slice::from_raw_parts_mut(centers as *mut f64, (kk * nn) as usize),
            kk,
            std::slice::from_raw_parts_mut(c1 as *mut c_int, mm as usize),
            std::slice::from_raw_parts_mut(c2 as *mut c_int, mm as usize),
            std::slice::from_raw_parts_mut(nc as *mut c_int, kk as usize),
            std::slice::from_raw_parts_mut(an1 as *mut f64, kk as usize),
            std::slice::from_raw_parts_mut(an2 as *mut f64, kk as usize),
            std::slice::from_raw_parts_mut(ncp as *mut c_int, kk as usize),
            std::slice::from_raw_parts_mut(d as *mut f64, mm as usize),
            std::slice::from_raw_parts_mut(itran as *mut c_int, (kk + 1) as usize),
            std::slice::from_raw_parts_mut(live as *mut c_int, kk as usize),
            &mut *(iter as *mut c_int),
            std::slice::from_raw_parts_mut(wss as *mut f64, kk as usize),
            &mut *(ifault as *mut c_int),
        );
    }
}
