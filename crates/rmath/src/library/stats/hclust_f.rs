//! GNU `stats/src/hclust.f`: `hclust` and `hcass2`.
//!
//! Indices in the algorithm are 1-based, matching the Fortran. The `.Fortran`
//! entry points receive pointers to the argument buffers.

use std::os::raw::{c_int, c_void};

fn ioffst(n: i32, i: i32, j: i32) -> usize {
    let n8 = n as i64;
    let i8 = i as i64;
    let j8 = j as i64;
    (j8 + (i8 - 1) * n8 - (i8 * (i8 + 1)) / 2) as usize
}

fn hclust_body(
    n: i32,
    len: i32,
    iopt: i32,
    ia: &mut [i32],
    ib: &mut [i32],
    crit: &mut [f64],
    membr: &mut [f64],
    nn: &mut [i32],
    disnn: &mut [f64],
    diss: &mut [f64],
) {
    const INF: f64 = 1.0e300;
    let n = n as usize;
    let mut active: Vec<i32> = (1..=n as i32).collect();
    let mut im = 0i32;
    let mut jj = 0i32;
    let mut jm = 0i32;
    let mut ncl = n as i32;

    if iopt == 8 {
        for i in 0..len as usize {
            diss[i] *= diss[i];
        }
    }

    for i in 1..n as i32 {
        let mut dmin = INF;
        for j in (i + 1)..=n as i32 {
            let ind = ioffst(n as i32, i, j) - 1;
            if dmin > diss[ind] {
                dmin = diss[ind];
                jm = j;
            }
        }
        nn[(i - 1) as usize] = jm;
        disnn[(i - 1) as usize] = dmin;
    }

    loop {
        let mut dmin = INF;
        for &i in &active {
            if i >= n as i32 {
                break;
            }
            if disnn[(i - 1) as usize] < dmin {
                dmin = disnn[(i - 1) as usize];
                im = i;
                jm = nn[(i - 1) as usize];
            }
        }
        ncl -= 1;
        let i2 = im.min(jm);
        let j2 = im.max(jm);
        let slot = (n as i32 - ncl - 1) as usize;
        ia[slot] = i2;
        ib[slot] = j2;
        let is_ward = iopt == 1 || iopt == 8;
        if iopt == 8 {
            dmin = dmin.sqrt();
        }
        crit[slot] = dmin;
        active.retain(|&k| k != j2);

        dmin = INF;
        jj = 0;
        for &k in &active {
            if k != i2 {
                let ind1 = if i2 < k {
                    ioffst(n as i32, i2, k)
                } else {
                    ioffst(n as i32, k, i2)
                } - 1;
                let ind2 = if j2 < k {
                    ioffst(n as i32, j2, k)
                } else {
                    ioffst(n as i32, k, j2)
                } - 1;
                let d12 = diss[ioffst(n as i32, i2, j2) - 1];
                if is_ward {
                    diss[ind1] = (membr[(i2 - 1) as usize] + membr[(k - 1) as usize]) * diss[ind1]
                        + (membr[(j2 - 1) as usize] + membr[(k - 1) as usize]) * diss[ind2]
                        - membr[(k - 1) as usize] * d12;
                    diss[ind1] /= membr[(i2 - 1) as usize]
                        + membr[(j2 - 1) as usize]
                        + membr[(k - 1) as usize];
                } else if iopt == 2 {
                    diss[ind1] = diss[ind1].min(diss[ind2]);
                } else if iopt == 3 {
                    diss[ind1] = diss[ind1].max(diss[ind2]);
                } else if iopt == 4 {
                    diss[ind1] = (membr[(i2 - 1) as usize] * diss[ind1]
                        + membr[(j2 - 1) as usize] * diss[ind2])
                        / (membr[(i2 - 1) as usize] + membr[(j2 - 1) as usize]);
                } else if iopt == 5 {
                    diss[ind1] = (diss[ind1] + diss[ind2]) / 2.0;
                } else if iopt == 6 {
                    diss[ind1] = ((diss[ind1] + diss[ind2]) - d12 / 2.0) / 2.0;
                } else if iopt == 7 {
                    let mi = membr[(i2 - 1) as usize];
                    let mj = membr[(j2 - 1) as usize];
                    diss[ind1] = (mi * diss[ind1] + mj * diss[ind2] - mi * mj * d12 / (mi + mj))
                        / (mi + mj);
                }
                if i2 < k {
                    if diss[ind1] < dmin {
                        dmin = diss[ind1];
                        jj = k;
                    }
                } else if diss[ind1] < disnn[(k - 1) as usize] {
                    disnn[(k - 1) as usize] = diss[ind1];
                    nn[(k - 1) as usize] = i2;
                }
            }
        }
        membr[(i2 - 1) as usize] += membr[(j2 - 1) as usize];
        disnn[(i2 - 1) as usize] = dmin;
        nn[(i2 - 1) as usize] = jj;

        for (ai, i) in active.iter().copied().enumerate() {
            if i >= n as i32 {
                break;
            }
            if nn[(i - 1) as usize] == i2 || nn[(i - 1) as usize] == j2 {
                let mut dmin = INF;
                let mut jj = 0i32;
                for &j in &active[ai + 1..] {
                    let ind = ioffst(n as i32, i, j) - 1;
                    if diss[ind] < dmin {
                        dmin = diss[ind];
                        jj = j;
                    }
                }
                nn[(i - 1) as usize] = jj;
                disnn[(i - 1) as usize] = dmin;
            }
        }

        if ncl <= 1 {
            break;
        }
    }
}

fn hcass2_body(n: i32, ia: &[i32], ib: &[i32], iorder: &mut [i32], iia: &mut [i32], iib: &mut [i32]) {
    let n = n as usize;
    for i in 0..n {
        iia[i] = ia[i];
        iib[i] = ib[i];
    }
    for i in 1..=(n as i32 - 2) {
        let k = ia[(i - 1) as usize].min(ib[(i - 1) as usize]);
        for j in (i + 1)..=(n as i32 - 1) {
            if ia[(j - 1) as usize] == k {
                iia[(j - 1) as usize] = -i;
            }
            if ib[(j - 1) as usize] == k {
                iib[(j - 1) as usize] = -i;
            }
        }
    }
    for i in 0..(n - 1) {
        iia[i] = -iia[i];
        iib[i] = -iib[i];
    }
    for i in 0..(n - 1) {
        if iia[i] > 0 && iib[i] < 0 {
            let k = iia[i];
            iia[i] = iib[i];
            iib[i] = k;
        }
        if iia[i] > 0 && iib[i] > 0 {
            let k1 = iia[i].min(iib[i]);
            let k2 = iia[i].max(iib[i]);
            iia[i] = k1;
            iib[i] = k2;
        }
    }
    iorder[0] = iia[n - 2];
    iorder[1] = iib[n - 2];
    let mut loc = 2i32;
    for i in (1..=(n as i32 - 2)).rev() {
        for j in 1..=loc {
            if iorder[(j - 1) as usize] == i {
                iorder[(j - 1) as usize] = iia[(i - 1) as usize];
                if j == loc {
                    loc += 1;
                    iorder[(loc - 1) as usize] = iib[(i - 1) as usize];
                } else {
                    loc += 1;
                    let mut k = loc;
                    while k >= j + 2 {
                        iorder[(k - 1) as usize] = iorder[(k - 2) as usize];
                        k -= 1;
                    }
                    iorder[j as usize] = iib[(i - 1) as usize];
                }
                break;
            }
        }
    }
    for i in 0..n {
        iorder[i] = -iorder[i];
    }
}

pub unsafe extern "C" fn c_hclust(
    n: *mut c_void,
    len: *mut c_void,
    iopt: *mut c_void,
    ia: *mut c_void,
    ib: *mut c_void,
    crit: *mut c_void,
    membr: *mut c_void,
    nn: *mut c_void,
    disnn: *mut c_void,
    diss: *mut c_void,
) {
    unsafe {
        let n = *(n as *const c_int);
        let len = *(len as *const c_int);
        let iopt = *(iopt as *const c_int);
        if n < 2 || len < 1 {
            return;
        }
        let ia = std::slice::from_raw_parts_mut(ia as *mut c_int, n as usize);
        let ib = std::slice::from_raw_parts_mut(ib as *mut c_int, n as usize);
        let crit = std::slice::from_raw_parts_mut(crit as *mut f64, n as usize);
        let membr = std::slice::from_raw_parts_mut(membr as *mut f64, n as usize);
        let nn = std::slice::from_raw_parts_mut(nn as *mut c_int, n as usize);
        let disnn = std::slice::from_raw_parts_mut(disnn as *mut f64, n as usize);
        let diss = std::slice::from_raw_parts_mut(diss as *mut f64, len as usize);
        hclust_body(n, len, iopt, ia, ib, crit, membr, nn, disnn, diss);
    }
}

pub unsafe extern "C" fn c_hcass2(
    n: *mut c_void,
    ia: *mut c_void,
    ib: *mut c_void,
    iorder: *mut c_void,
    iia: *mut c_void,
    iib: *mut c_void,
) {
    unsafe {
        let n = *(n as *const c_int);
        if n < 2 {
            return;
        }
        let ia = std::slice::from_raw_parts(ia as *const c_int, n as usize);
        let ib = std::slice::from_raw_parts(ib as *const c_int, n as usize);
        let iorder = std::slice::from_raw_parts_mut(iorder as *mut c_int, n as usize);
        let iia = std::slice::from_raw_parts_mut(iia as *mut c_int, n as usize);
        let iib = std::slice::from_raw_parts_mut(iib as *mut c_int, n as usize);
        hcass2_body(n, ia, ib, iorder, iia, iib);
    }
}
