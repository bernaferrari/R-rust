use core::ffi::c_int;
use std::slice;

use crate::main::errors::Rf_error;

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

    *maxiter = iter;
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

    *maxiter = iter;
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
    let n = unsafe { *pn };
    let k = unsafe { *pk };
    let p = unsafe { *pp };
    let x_len = (n * p) as usize;
    let cen_len = (k * p) as usize;
    let cl_len = n as usize;
    let nc_len = k as usize;
    let wss_len = k as usize;
    let x = unsafe { slice::from_raw_parts(x, x_len) };
    let cen = unsafe { slice::from_raw_parts_mut(cen, cen_len) };
    let cl = unsafe { slice::from_raw_parts_mut(cl, cl_len) };
    let maxiter = unsafe { &mut *pmaxiter };
    let nc = unsafe { slice::from_raw_parts_mut(nc, nc_len) };
    let wss = unsafe { slice::from_raw_parts_mut(wss, wss_len) };
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
    let n = unsafe { *pn };
    let k = unsafe { *pk };
    let p = unsafe { *pp };
    let x_len = (n * p) as usize;
    let cen_len = (k * p) as usize;
    let cl_len = n as usize;
    let nc_len = k as usize;
    let wss_len = k as usize;
    let x = unsafe { slice::from_raw_parts(x, x_len) };
    let cen = unsafe { slice::from_raw_parts_mut(cen, cen_len) };
    let cl = unsafe { slice::from_raw_parts_mut(cl, cl_len) };
    let maxiter = unsafe { &mut *pmaxiter };
    let nc = unsafe { slice::from_raw_parts_mut(nc, nc_len) };
    let wss = unsafe { slice::from_raw_parts_mut(wss, wss_len) };
    kmeans_macqueen_impl(x, n, p, k, cen, cl, maxiter, nc, wss);
}

// Fortran tracing stubs (F77_SUB name mangling: lowercase + underscore suffix)
pub unsafe fn kmns1_(_k: *const c_int, _it: *const c_int, _indx: *const c_int) {
    unsafe {
        Rf_error(b"kmeans tracing stub kmns1_ is not implemented\0".as_ptr() as *const _);
    }
}

pub unsafe fn kmnsqpr_(
    _istep: *const c_int,
    _icoun: *const c_int,
    _ncp: *const c_int,
    _k: *const c_int,
    _trace: *const c_int,
) {
    unsafe {
        Rf_error(b"kmeans tracing stub kmnsqpr_ is not implemented\0".as_ptr() as *const _);
    }
}
