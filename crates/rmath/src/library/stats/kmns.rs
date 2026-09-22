//! GNU `stats/src/kmns.f`: Hartigan-Wong (AS 136).
//!
//! Indices in the loops are 1-based, as in the Fortran. Slices are
//! addressed with `i - 1`. Column-major `A(M,N)` and `C(K,N)`.

const BIG: f64 = 1.0e30;

pub fn kmns(
    a: &mut [f64],
    m: i32,
    n: i32,
    c: &mut [f64],
    k: i32,
    ic1: &mut [i32],
    ic2: &mut [i32],
    nc: &mut [i32],
    an1: &mut [f64],
    an2: &mut [f64],
    ncp: &mut [i32],
    d: &mut [f64],
    itran: &mut [i32],
    live: &mut [i32],
    iter: &mut i32,
    wss: &mut [f64],
    ifault: &mut i32,
) {
    let m = m as usize;
    let n = n as usize;
    let k = k as usize;
    let imax_qtr = itran[0];
    *ifault = 3;
    if k <= 1 || k >= m {
        return;
    }
    *ifault = 0;

    for i in 1..=m {
        ic1[i - 1] = 1;
        ic2[i - 1] = 2;
        let mut dt = [0.0f64; 2];
        for il in 1..=2 {
            for j in 1..=n {
                let da = a[(i - 1) + (j - 1) * m] - c[(il - 1) + (j - 1) * k];
                dt[il - 1] += da * da;
            }
        }
        if dt[0] > dt[1] {
            ic1[i - 1] = 2;
            ic2[i - 1] = 1;
            dt.swap(0, 1);
        }
        for l in 3..=k {
            let mut db = 0.0;
            let mut early = false;
            for j in 1..=n {
                let dc = a[(i - 1) + (j - 1) * m] - c[(l - 1) + (j - 1) * k];
                db += dc * dc;
                if db >= dt[1] {
                    early = true;
                    break;
                }
            }
            if early {
                continue;
            }
            if db >= dt[0] {
                dt[1] = db;
                ic2[i - 1] = l as i32;
            } else {
                dt[1] = dt[0];
                ic2[i - 1] = ic1[i - 1];
                dt[0] = db;
                ic1[i - 1] = l as i32;
            }
        }
    }

    nc.fill(0);
    for l in 0..k {
        for j in 0..n {
            c[l + j * k] = 0.0;
        }
    }
    for i in 0..m {
        let l = ic1[i] as usize - 1;
        nc[l] += 1;
        for j in 0..n {
            c[l + j * k] += a[i + j * m];
        }
    }
    for l in 0..k {
        if nc[l] == 0 {
            *ifault = 1;
            return;
        }
        let aa = nc[l] as f64;
        for j in 0..n {
            c[l + j * k] /= aa;
        }
        an2[l] = aa / (aa + 1.0);
        an1[l] = if aa > 1.0 { aa / (aa - 1.0) } else { BIG };
        itran[l] = 1;
        ncp[l] = -1;
    }

    let mut indx = 0i32;
    let mut ij = 0i32;
    let mut qtran_hit = false;
    for step in 1..=*iter {
        ij = step;
        optra(a, m, n, c, k, ic1, ic2, nc, an1, an2, ncp, d, itran, live, &mut indx);
        if indx == m as i32 {
            break;
        }
        let mut max_qtr = imax_qtr;
        qtran(a, m, n, c, k, ic1, ic2, nc, an1, an2, ncp, d, itran, &mut indx, &mut max_qtr);
        if max_qtr < 0 {
            *ifault = 4;
            qtran_hit = true;
            break;
        }
        if k == 2 {
            break;
        }
        ncp[..k].fill(0);
    }
    if !qtran_hit && indx != m as i32 && k != 2 {
        *ifault = 2;
    }
    *iter = ij;

    wss[..k].fill(0.0);
    for l in 0..k {
        for j in 0..n {
            c[l + j * k] = 0.0;
        }
    }
    for i in 0..m {
        let ii = ic1[i] as usize - 1;
        for j in 0..n {
            c[ii + j * k] += a[i + j * m];
        }
    }
    for j in 0..n {
        for l in 0..k {
            c[l + j * k] /= nc[l] as f64;
        }
        for i in 0..m {
            let ii = ic1[i] as usize - 1;
            let da = a[i + j * m] - c[ii + j * k];
            wss[ii] += da * da;
        }
    }
}

fn optra(
    a: &[f64], m: usize, n: usize, c: &mut [f64], k: usize,
    ic1: &mut [i32], ic2: &mut [i32], nc: &mut [i32], an1: &mut [f64], an2: &mut [f64],
    ncp: &mut [i32], d: &mut [f64], itran: &mut [i32], live: &mut [i32], indx: &mut i32,
) {
    for l in 0..k {
        if itran[l] == 1 {
            live[l] = m as i32 + 1;
        }
    }
    for i in 1..=m {
        *indx += 1;
        let l1 = ic1[i - 1] as usize;
        let mut l2 = ic2[i - 1] as usize;
        let ll = l2;
        if nc[l1 - 1] != 1 {
            if ncp[l1 - 1] != 0 {
                let mut de = 0.0;
                for j in 1..=n {
                    let df = a[(i - 1) + (j - 1) * m] - c[(l1 - 1) + (j - 1) * k];
                    de += df * df;
                }
                d[i - 1] = de * an1[l1 - 1];
            }
            let mut da = 0.0;
            for j in 1..=n {
                let db = a[(i - 1) + (j - 1) * m] - c[(l2 - 1) + (j - 1) * k];
                da += db * db;
            }
            let mut r2 = da * an2[l2 - 1];
            for l in 1..=k {
                if (i as i32 >= live[l1 - 1] && i as i32 >= live[l - 1]) || l == l1 || l == ll {
                    continue;
                }
                let rr = r2 / an2[l - 1];
                let mut dc = 0.0;
                let mut early = false;
                for j in 1..=n {
                    let dd = a[(i - 1) + (j - 1) * m] - c[(l - 1) + (j - 1) * k];
                    dc += dd * dd;
                    if dc >= rr {
                        early = true;
                        break;
                    }
                }
                if early {
                    continue;
                }
                r2 = dc * an2[l - 1];
                l2 = l;
            }
            if r2 >= d[i - 1] {
                ic2[i - 1] = l2 as i32;
            } else {
                *indx = 0;
                live[l1 - 1] = m as i32 + i as i32;
                live[l2 - 1] = m as i32 + i as i32;
                ncp[l1 - 1] = i as i32;
                ncp[l2 - 1] = i as i32;
                let al1 = nc[l1 - 1] as f64;
                let alw = al1 - 1.0;
                let al2 = nc[l2 - 1] as f64;
                let alt = al2 + 1.0;
                for j in 1..=n {
                    let ai = a[(i - 1) + (j - 1) * m];
                    c[(l1 - 1) + (j - 1) * k] = (c[(l1 - 1) + (j - 1) * k] * al1 - ai) / alw;
                    c[(l2 - 1) + (j - 1) * k] = (c[(l2 - 1) + (j - 1) * k] * al2 + ai) / alt;
                }
                nc[l1 - 1] -= 1;
                nc[l2 - 1] += 1;
                an2[l1 - 1] = alw / al1;
                an1[l1 - 1] = if alw > 1.0 { alw / (alw - 1.0) } else { BIG };
                an1[l2 - 1] = alt / al2;
                an2[l2 - 1] = alt / (alt + 1.0);
                ic1[i - 1] = l2 as i32;
                ic2[i - 1] = l1 as i32;
            }
        }
        if *indx == m as i32 {
            return;
        }
    }
    for l in 0..k {
        itran[l] = 0;
        live[l] -= m as i32;
    }
}

fn qtran(
    a: &[f64], m: usize, n: usize, c: &mut [f64], k: usize,
    ic1: &mut [i32], ic2: &mut [i32], nc: &mut [i32], an1: &mut [f64], an2: &mut [f64],
    ncp: &mut [i32], d: &mut [f64], itran: &mut [i32], indx: &mut i32, imax_qtr: &mut i32,
) {
    let mut icoun = 0i32;
    let mut istep = 0i32;
    loop {
        for i in 1..=m {
            icoun += 1;
            istep += 1;
            if istep >= *imax_qtr {
                *imax_qtr = -1;
                return;
            }
            let l1 = ic1[i - 1] as usize;
            let l2 = ic2[i - 1] as usize;
            if nc[l1 - 1] != 1 {
                if istep <= ncp[l1 - 1] {
                    let mut da = 0.0;
                    for j in 1..=n {
                        let db = a[(i - 1) + (j - 1) * m] - c[(l1 - 1) + (j - 1) * k];
                        da += db * db;
                    }
                    d[i - 1] = da * an1[l1 - 1];
                }
                if istep < ncp[l1 - 1] || istep < ncp[l2 - 1] {
                    let r2 = d[i - 1] / an2[l2 - 1];
                    let mut dd = 0.0;
                    let mut early = false;
                    for j in 1..=n {
                        let de = a[(i - 1) + (j - 1) * m] - c[(l2 - 1) + (j - 1) * k];
                        dd += de * de;
                        if dd >= r2 {
                            early = true;
                            break;
                        }
                    }
                    if !early {
                        icoun = 0;
                        *indx = 0;
                        itran[l1 - 1] = 1;
                        itran[l2 - 1] = 1;
                        ncp[l1 - 1] = istep + m as i32;
                        ncp[l2 - 1] = istep + m as i32;
                        let al1 = nc[l1 - 1] as f64;
                        let alw = al1 - 1.0;
                        let al2 = nc[l2 - 1] as f64;
                        let alt = al2 + 1.0;
                        for j in 1..=n {
                            let ai = a[(i - 1) + (j - 1) * m];
                            c[(l1 - 1) + (j - 1) * k] = (c[(l1 - 1) + (j - 1) * k] * al1 - ai) / alw;
                            c[(l2 - 1) + (j - 1) * k] = (c[(l2 - 1) + (j - 1) * k] * al2 + ai) / alt;
                        }
                        nc[l1 - 1] -= 1;
                        nc[l2 - 1] += 1;
                        an2[l1 - 1] = alw / al1;
                        an1[l1 - 1] = if alw > 1.0 { alw / (alw - 1.0) } else { BIG };
                        an1[l2 - 1] = alt / al2;
                        an2[l2 - 1] = alt / (alt + 1.0);
                        ic1[i - 1] = l2 as i32;
                        ic2[i - 1] = l1 as i32;
                    }
                }
            }
            if icoun == m as i32 {
                return;
            }
        }
    }
}
