#![allow(unsafe_op_in_unsafe_fn)]

use core::ffi::c_int;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmeans_Lloyd(
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
    let n = *pn;
    let k = *pk;
    let p = *pp;
    let mut maxiter = *pmaxiter;
    let mut iter: c_int;
    let mut inew: c_int = 0;

    for i in 0..n {
        *cl.add(i as usize) = -1;
    }

    iter = 0;
    while iter < maxiter {
        let mut updated = false;
        for i in 0..n {
            let mut best = f64::INFINITY;
            for j in 0..k {
                let mut dd = 0.0f64;
                for c in 0..p {
                    let tmp = *x.add((i + n * c) as usize) - *cen.add((j + k * c) as usize);
                    dd += tmp * tmp;
                }
                if dd < best {
                    best = dd;
                    inew = j + 1;
                }
            }
            if *cl.add(i as usize) != inew {
                updated = true;
                *cl.add(i as usize) = inew;
            }
        }
        if !updated {
            break;
        }

        for j in 0..(k * p) {
            *cen.add(j as usize) = 0.0;
        }
        for j in 0..k {
            *nc.add(j as usize) = 0;
        }
        for i in 0..n {
            let it = *cl.add(i as usize) - 1;
            *nc.add(it as usize) += 1;
            for c in 0..p {
                *cen.add((it + c * k) as usize) += *x.add((i + c * n) as usize);
            }
        }
        for j in 0..(k * p) {
            let idx = j % k;
            if *nc.add(idx as usize) > 0 {
                *cen.add(j as usize) /= *nc.add(idx as usize) as f64;
            }
        }
        iter += 1;
    }

    *pmaxiter = iter;
    for j in 0..k {
        *wss.add(j as usize) = 0.0;
    }
    for i in 0..n {
        let it = *cl.add(i as usize) - 1;
        for c in 0..p {
            let tmp = *x.add((i + n * c) as usize) - *cen.add((it + k * c) as usize);
            *wss.add(it as usize) += tmp * tmp;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmeans_MacQueen(
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
    let n = *pn;
    let k = *pk;
    let p = *pp;
    let mut maxiter = *pmaxiter;
    let mut iter: c_int;
    let mut inew: c_int = 0;

    // first assign each point to the nearest cluster centre
    for i in 0..n {
        let mut best = f64::INFINITY;
        for j in 0..k {
            let mut dd = 0.0f64;
            for c in 0..p {
                let tmp = *x.add((i + n * c) as usize) - *cen.add((j + k * c) as usize);
                dd += tmp * tmp;
            }
            if dd < best {
                best = dd;
                inew = j + 1;
            }
        }
        if *cl.add(i as usize) != inew {
            *cl.add(i as usize) = inew;
        }
    }

    // recompute centres as centroids
    for j in 0..(k * p) {
        *cen.add(j as usize) = 0.0;
    }
    for j in 0..k {
        *nc.add(j as usize) = 0;
    }
    for i in 0..n {
        let it = *cl.add(i as usize) - 1;
        *nc.add(it as usize) += 1;
        for c in 0..p {
            *cen.add((it + c * k) as usize) += *x.add((i + c * n) as usize);
        }
    }
    for j in 0..(k * p) {
        let idx = j % k;
        if *nc.add(idx as usize) > 0 {
            *cen.add(j as usize) /= *nc.add(idx as usize) as f64;
        }
    }

    iter = 0;
    while iter < maxiter {
        let mut updated = false;
        for i in 0..n {
            let mut best = f64::INFINITY;
            for j in 0..k {
                let mut dd = 0.0f64;
                for c in 0..p {
                    let tmp = *x.add((i + n * c) as usize) - *cen.add((j + k * c) as usize);
                    dd += tmp * tmp;
                }
                if dd < best {
                    best = dd;
                    inew = j;
                }
            }
            let iold = *cl.add(i as usize) - 1;
            if iold != inew {
                updated = true;
                *cl.add(i as usize) = inew + 1;
                *nc.add(iold as usize) -= 1;
                *nc.add(inew as usize) += 1;
                // update old and new cluster centres
                for c in 0..p {
                    let nci = *nc.add(iold as usize);
                    let ncn = *nc.add(inew as usize);
                    if nci > 0 {
                        *cen.add((iold + k * c) as usize) += (*cen.add((iold + k * c) as usize)
                            - *x.add((i + n * c) as usize))
                            / nci as f64;
                    }
                    if ncn > 0 {
                        *cen.add((inew + k * c) as usize) += (*x.add((i + n * c) as usize)
                            - *cen.add((inew + k * c) as usize))
                            / ncn as f64;
                    }
                }
            }
        }
        if !updated {
            break;
        }
        iter += 1;
    }

    *pmaxiter = iter;
    for j in 0..k {
        *wss.add(j as usize) = 0.0;
    }
    for i in 0..n {
        let it = *cl.add(i as usize) - 1;
        for c in 0..p {
            let tmp = *x.add((i + n * c) as usize) - *cen.add((it + k * c) as usize);
            *wss.add(it as usize) += tmp * tmp;
        }
    }
}

// Fortran tracing stubs (F77_SUB name mangling: lowercase + underscore suffix)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmns1_(_k: *const c_int, _it: *const c_int, _indx: *const c_int) {
    // Tracing stub - no-op in Rust port
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kmnsqpr_(
    _istep: *const c_int,
    _icoun: *const c_int,
    _ncp: *const c_int,
    _k: *const c_int,
    _trace: *const c_int,
) {
    // Tracing stub - no-op in Rust port
}
