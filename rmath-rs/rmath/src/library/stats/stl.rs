//! Port of r-source/src/library/stats/src/stl.c.

use std::os::raw::c_int;
use std::{cmp, ptr, slice};

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
fn stlstp(
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

fn stlrwt(y: &[f64], fit: &[f64], rw: &mut [f64]) {
    for i in 0..y.len() {
        rw[i] = (y[i] - fit[i]).abs();
    }
    let mut sorted = rw.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
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

#[allow(clippy::too_many_arguments)]
pub unsafe fn stl(
    y: *mut f64,
    n: *mut c_int,
    np: *mut c_int,
    ns: *mut c_int,
    nt: *mut c_int,
    nl: *mut c_int,
    isdeg: *mut c_int,
    itdeg: *mut c_int,
    ildeg: *mut c_int,
    nsjump: *mut c_int,
    ntjump: *mut c_int,
    nljump: *mut c_int,
    ni: *mut c_int,
    no: *mut c_int,
    rw: *mut f64,
    season: *mut f64,
    trend: *mut f64,
) {
    unsafe {
        if [
            y as *mut u8,
            n as *mut u8,
            np as *mut u8,
            ns as *mut u8,
            nt as *mut u8,
            nl as *mut u8,
            isdeg as *mut u8,
            itdeg as *mut u8,
            ildeg as *mut u8,
            nsjump as *mut u8,
            ntjump as *mut u8,
            nljump as *mut u8,
            ni as *mut u8,
            no as *mut u8,
            rw as *mut u8,
            season as *mut u8,
            trend as *mut u8,
        ]
        .iter()
        .any(|p| p.is_null())
        {
            return;
        }

        let n = (*n).max(0) as usize;
        if n == 0 {
            return;
        }
        let y = slice::from_raw_parts_mut(y, n);
        let rw = slice::from_raw_parts_mut(rw, n);
        let season = slice::from_raw_parts_mut(season, n);
        let trend = slice::from_raw_parts_mut(trend, n);

        trend.fill(0.0);
        let normalize_span = |x: c_int| {
            let mut x = x.max(3) as usize;
            if x % 2 == 0 {
                x += 1;
            }
            x
        };
        let newns = normalize_span(*ns);
        let newnt = normalize_span(*nt);
        let newnl = normalize_span(*nl);
        let nperiod = (*np).max(2) as usize;
        let niter = (*ni).max(0) as usize;
        let nouter = (*no).max(0) as usize;
        let nsjump = (*nsjump).max(1) as usize;
        let ntjump = (*ntjump).max(1) as usize;
        let nljump = (*nljump).max(1) as usize;
        let work_len = 5usize
            .checked_mul(n.saturating_add(2 * nperiod))
            .unwrap_or(0);
        if work_len == 0 {
            return;
        }
        let mut work = vec![0.0; work_len];
        let mut use_rw = false;

        let mut k = 0usize;
        loop {
            stlstp(
                y, nperiod, newns, newnt, newnl, *isdeg, *itdeg, *ildeg, nsjump, ntjump, nljump,
                niter, use_rw, rw, season, trend, &mut work,
            );
            k += 1;
            if k > nouter {
                break;
            }
            for i in 0..n {
                work[i] = trend[i] + season[i];
            }
            stlrwt(y, &work[..n], rw);
            use_rw = true;
        }

        if nouter == 0 {
            ptr::write_bytes(rw.as_mut_ptr(), 0, n);
            rw.fill(1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::stl;

    #[test]
    fn stl_decomposes_additive_series_and_sets_default_weights() {
        let mut y: Vec<f64> = (0..24)
            .map(|i| 10.0 + i as f64 * 0.25 + [1.0, -1.0, 0.5, -0.5][i % 4])
            .collect();
        let mut n = y.len() as i32;
        let mut np = 4;
        let mut ns = 7;
        let mut nt = 9;
        let mut nl = 7;
        let mut isdeg = 1;
        let mut itdeg = 1;
        let mut ildeg = 1;
        let mut nsjump = 1;
        let mut ntjump = 1;
        let mut nljump = 1;
        let mut ni = 2;
        let mut no = 0;
        let mut rw = vec![0.0; y.len()];
        let mut season = vec![0.0; y.len()];
        let mut trend = vec![0.0; y.len()];

        unsafe {
            stl(
                y.as_mut_ptr(),
                &mut n,
                &mut np,
                &mut ns,
                &mut nt,
                &mut nl,
                &mut isdeg,
                &mut itdeg,
                &mut ildeg,
                &mut nsjump,
                &mut ntjump,
                &mut nljump,
                &mut ni,
                &mut no,
                rw.as_mut_ptr(),
                season.as_mut_ptr(),
                trend.as_mut_ptr(),
            );
        }

        assert!(rw.iter().all(|w| (*w - 1.0).abs() < 1e-12));
        assert!(season.iter().all(|x| x.is_finite()));
        assert!(trend.iter().all(|x| x.is_finite()));
        let max_residual = y
            .iter()
            .zip(season.iter().zip(&trend))
            .map(|(y, (s, t))| (y - s - t).abs())
            .fold(0.0, f64::max);
        assert!(max_residual < 2.5, "max residual was {max_residual}");
    }

    #[test]
    fn stl_normalizes_small_even_spans_and_period() {
        let mut y: Vec<f64> = (0..12).map(|i| i as f64).collect();
        let mut n = y.len() as i32;
        let mut np = 1;
        let mut ns = 2;
        let mut nt = 2;
        let mut nl = 2;
        let mut degree = 0;
        let mut jump = 1;
        let mut ni = 1;
        let mut no = 0;
        let mut rw = vec![0.0; y.len()];
        let mut season = vec![0.0; y.len()];
        let mut trend = vec![0.0; y.len()];

        unsafe {
            stl(
                y.as_mut_ptr(),
                &mut n,
                &mut np,
                &mut ns,
                &mut nt,
                &mut nl,
                &mut degree,
                &mut degree,
                &mut degree,
                &mut jump,
                &mut jump,
                &mut jump,
                &mut ni,
                &mut no,
                rw.as_mut_ptr(),
                season.as_mut_ptr(),
                trend.as_mut_ptr(),
            );
        }

        assert!(season.iter().all(|x| x.is_finite()));
        assert!(trend.iter().all(|x| x.is_finite()));
        assert!(rw.iter().all(|w| (*w - 1.0).abs() < 1e-12));
    }
}
