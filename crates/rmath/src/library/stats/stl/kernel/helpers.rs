//! GNU STL smoothing and robustness mathematics over checked slices.
#![forbid(unsafe_code)]
use core::ffi::c_int;
use std::cmp;

fn stless(
    y: &[f64],
    len: usize,
    ideg: c_int,
    njump: usize,
    use_rw: bool,
    rw: &[f64],
    ys: &mut [f64],
    res: &mut [f64],
) {
    let n = y.len();
    if n < 2 {
        ys[0] = y[0];
        return;
    }

    let newnj = cmp::min(njump, n - 1).max(1);
    let mut nleft;
    let mut nright;

    if len >= n {
        nleft = 1;
        nright = n;
        for i in (0..n).step_by(newnj) {
            if !stlest(
                y,
                len,
                ideg,
                (i + 1) as f64,
                nleft,
                nright,
                res,
                use_rw,
                rw,
                &mut ys[i],
            ) {
                ys[i] = y[i];
            }
        }
    } else {
        let nsh = (len + 1) / 2;
        if newnj == 1 {
            nleft = 1;
            nright = len;
            for i in 0..n {
                if i + 1 > nsh && nright != n {
                    nleft += 1;
                    nright += 1;
                }
                if !stlest(
                    y,
                    len,
                    ideg,
                    (i + 1) as f64,
                    nleft,
                    nright,
                    res,
                    use_rw,
                    rw,
                    &mut ys[i],
                ) {
                    ys[i] = y[i];
                }
            }
        } else {
            nleft = 1;
            nright = len;
            for i in (0..n).step_by(newnj) {
                if i + 1 < nsh {
                    nleft = 1;
                    nright = len;
                } else if i >= n - nsh {
                    nleft = n - len + 1;
                    nright = n;
                } else {
                    nleft = i + 1 - nsh + 1;
                    nright = len + i + 1 - nsh;
                }
                if !stlest(
                    y,
                    len,
                    ideg,
                    (i + 1) as f64,
                    nleft,
                    nright,
                    res,
                    use_rw,
                    rw,
                    &mut ys[i],
                ) {
                    ys[i] = y[i];
                }
            }
        }
    }

    if newnj != 1 {
        for i in (0..(n - newnj)).step_by(newnj) {
            let delta = (ys[i + newnj] - ys[i]) / newnj as f64;
            for j in (i + 1)..=(i + newnj - 1) {
                ys[j] = ys[i] + delta * (j - i) as f64;
            }
        }
        let k = (n - 1) / newnj * newnj;
        if k != n - 1 {
            if !stlest(
                y,
                len,
                ideg,
                n as f64,
                nleft,
                nright,
                res,
                use_rw,
                rw,
                &mut ys[n - 1],
            ) {
                ys[n - 1] = y[n - 1];
            }
            let delta = (ys[n - 1] - ys[k]) / (n - 1 - k) as f64;
            for j in (k + 1)..(n - 1) {
                ys[j] = ys[k] + delta * (j - k) as f64;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn stlest(
    y: &[f64],
    len: usize,
    ideg: c_int,
    xs: f64,
    nleft: usize,
    nright: usize,
    w: &mut [f64],
    use_rw: bool,
    rw: &[f64],
    ys: &mut f64,
) -> bool {
    let n = y.len();
    let range = (n - 1) as f64;
    let mut h = f64::max(xs - nleft as f64, nright as f64 - xs);
    if len > n {
        h += ((len - n) / 2) as f64;
    }
    let h9 = h * 0.999;
    let h1 = h * 0.001;
    let mut a = 0.0;
    for j in (nleft - 1)..nright {
        let r = ((j + 1) as f64 - xs).abs();
        if r <= h9 {
            w[j] = if r <= h1 {
                1.0
            } else {
                (1.0 - (r / h).powi(3)).powi(3)
            };
            if use_rw {
                w[j] *= rw[j];
            }
            a += w[j];
        } else {
            w[j] = 0.0;
        }
    }

    if a <= 0.0 {
        return false;
    }

    for weight in w.iter_mut().take(nright).skip(nleft - 1) {
        *weight /= a;
    }
    if h > 0.0 && ideg > 0 {
        a = 0.0;
        for (j, weight) in w.iter().enumerate().take(nright).skip(nleft - 1) {
            a += *weight * (j + 1) as f64;
        }
        let mut c = 0.0;
        for (j, weight) in w.iter().enumerate().take(nright).skip(nleft - 1) {
            let d = (j + 1) as f64 - a;
            c += *weight * d * d;
        }
        if c.sqrt() > range * 0.001 {
            let b = (xs - a) / c;
            for (j, weight) in w.iter_mut().enumerate().take(nright).skip(nleft - 1) {
                *weight *= b * ((j + 1) as f64 - a) + 1.0;
            }
        }
    }

    *ys = 0.0;
    for j in (nleft - 1)..nright {
        *ys += w[j] * y[j];
    }
    true
}

fn stlma(x: &[f64], len: usize, ave: &mut [f64]) {
    let flen = len as f64;
    let mut v = x.iter().take(len).sum::<f64>();
    ave[0] = v / flen;
    let newn = x.len() - len + 1;
    if newn > 1 {
        let mut k = len;
        let mut m = 0;
        for out in ave.iter_mut().take(newn).skip(1) {
            v += x[k] - x[m];
            *out = v / flen;
            k += 1;
            m += 1;
        }
    }
}

fn stlfts(x: &[f64], np: usize, trend: &mut [f64], work: &mut [f64]) {
    stlma(x, np, trend);
    stlma(&trend[..x.len() - np + 1], np, work);
    stlma(&work[..x.len() - (np << 1) + 2], 3, trend);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn stlstp(
    y: &[f64],
    np: usize,
    ns: usize,
    nt: usize,
    nl: usize,
    isdeg: c_int,
    itdeg: c_int,
    ildeg: c_int,
    nsjump: usize,
    ntjump: usize,
    nljump: usize,
    niter: usize,
    use_rw: bool,
    rw: &[f64],
    season: &mut [f64],
    trend: &mut [f64],
    work: &mut [f64],
) {
    let n = y.len();
    let n2p = n + (np << 1);
    for _ in 0..niter {
        for i in 0..n {
            work[i] = y[i] - trend[i];
        }
        let (work1, rest) = work.split_at_mut(n2p);
        let (work2, rest) = rest.split_at_mut(n2p);
        let (work3, rest) = rest.split_at_mut(n2p);
        let (work4, work5) = rest.split_at_mut(n2p);
        stlss(
            &work1[..n],
            np,
            ns,
            isdeg,
            nsjump,
            use_rw,
            rw,
            work2,
            work3,
            work4,
            work5,
            season,
        );
        stlfts(work2, np, work3, work1);
        stless(&work3[..n], nl, ildeg, nljump, false, work4, work1, work5);
        for i in 0..n {
            season[i] = work2[np + i] - work1[i];
            work1[i] = y[i] - season[i];
        }
        stless(&work1[..n], nt, itdeg, ntjump, use_rw, rw, trend, work3);
    }
}

#[allow(clippy::too_many_arguments)]
fn stlss(
    y: &[f64],
    np: usize,
    ns: usize,
    isdeg: c_int,
    nsjump: usize,
    use_rw: bool,
    rw: &[f64],
    season: &mut [f64],
    work1: &mut [f64],
    work2: &mut [f64],
    work3: &mut [f64],
    work4: &mut [f64],
) {
    let n = y.len();
    for j in 0..np {
        let k = (n - (j + 1)) / np + 1;
        for i in 0..k {
            work1[i] = y[i * np + j];
        }
        if use_rw {
            for i in 0..k {
                work3[i] = rw[i * np + j];
            }
        }
        stless(
            &work1[..k],
            ns,
            isdeg,
            nsjump,
            use_rw,
            work3,
            &mut work2[1..],
            work4,
        );
        let nright = cmp::min(ns, k);
        if !stlest(
            &work1[..k],
            ns,
            isdeg,
            0.0,
            1,
            nright,
            work4,
            use_rw,
            work3,
            &mut work2[0],
        ) {
            work2[0] = work2[1];
        }
        let nleft = cmp::max(1, k.saturating_sub(ns) + 1);
        if !stlest(
            &work1[..k],
            ns,
            isdeg,
            (k + 1) as f64,
            nleft,
            k,
            work4,
            use_rw,
            work3,
            &mut work2[k + 1],
        ) {
            work2[k + 1] = work2[k];
        }
        for m in 0..(k + 2) {
            season[m * np + j] = work2[m];
        }
    }
}

pub(super) fn stlrwt(y: &[f64], fit: &[f64], rw: &mut [f64], sorted: &mut [f64]) {
    for i in 0..y.len() {
        rw[i] = (y[i] - fit[i]).abs();
    }
    sorted.copy_from_slice(rw);
    sorted.sort_unstable_by(|a, b| a.total_cmp(b));
    let mid0 = y.len() / 2;
    let mid1 = y.len() - mid0 - 1;
    let cmad = (sorted[mid0] + sorted[mid1]) * 3.0;
    let c9 = cmad * 0.999;
    let c1 = cmad * 0.001;
    for i in 0..y.len() {
        let r = (y[i] - fit[i]).abs();
        rw[i] = if r <= c1 {
            1.0
        } else if r <= c9 {
            let d2 = r / cmad;
            let id2 = 1.0 - d2 * d2;
            id2 * id2
        } else {
            0.0
        };
    }
}
