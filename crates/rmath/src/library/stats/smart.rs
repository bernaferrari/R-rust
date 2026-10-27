//!
//! Statement-level port of the pinned `r-source/src/library/stats/src/ppr.f`
//! (subroutines `smart`, `smart1`, `subfit`, `fulfit`, `onetrm`, `oneone`,
//! `pprdir`, `ppconj`, `pprder`, `pool`, `newb`, `setppr`, `sort` and the
//! `spline`/`splineAA` smoothing-spline branch routed through `rbart`).
//! Column-major Fortran layouts are preserved exactly; all indices below are
//! 0-based unless a comment quotes the original 1-based statement.

use crate::library::stats::supsmu::{self, Input, Output, Parameters};

// ---------------------------------------------------------------------------
// Labelled COMMON blocks (block data bkppr / bksupsmu defaults)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Common {
    // /pprpar/
    pub ifl: i32,
    pub lf: i32,
    pub span: f64,
    pub alpha: f64,
    pub big: f64,
    // /pprz01/
    pub conv: f64,
    pub maxit: i32,
    pub mitone: i32,
    pub cutmin: f64,
    pub fdel: f64,
    pub cjeps: f64,
    pub mitcj: i32,
    // /spsmooth/
    pub df: f64,
    pub gcvpen: f64,
    pub ismethod: i32,
    pub trace: bool,
}

const DEFAULTS: Common = Common {
    ifl: 6,
    lf: 2,
    span: 0.0,
    alpha: 0.0,
    big: 1.0e20,
    conv: 0.005,
    maxit: 20,
    mitone: 20,
    cutmin: 0.1,
    fdel: 0.02,
    cjeps: 0.001,
    mitcj: 1,
    df: 4.0,
    gcvpen: 1.0,
    ismethod: 0,
    trace: false,
};

impl Default for Common {
    fn default() -> Self {
        DEFAULTS
    }
}

/// Copy settings before a numerical invocation; no runtime lend survives it.
pub(crate) fn settings() -> Common {
    crate::sexp::instance::with_required_current_instance(|instance| unsafe {
        (*instance).ppr_settings.get()
    })
}

pub(crate) fn trace_enabled() -> Result<bool, &'static str> {
    if !crate::sexp::instance::current_instance_is_configured() {
        return Ok(false);
    }
    crate::sexp::instance::with_current_instance(|instance| unsafe {
        (*instance).ppr_settings.get().trace
    })
    .ok_or("configured R runtime owner is no longer active")
}

fn update_settings(update: impl FnOnce(&mut Common)) {
    crate::sexp::instance::with_required_current_instance(|instance| unsafe {
        let mut settings = (*instance).ppr_settings.get();
        update(&mut settings);
        (*instance).ppr_settings.set(settings);
    });
}

/// GNU setsmu resets only the spline method and trace switch.
pub(crate) fn setsmu(trace: i32) {
    update_settings(|settings| {
        settings.ismethod = 0;
        settings.trace = trace != 0;
    });
}

/// `subroutine setppr` — store ppr parameters into the COMMON blocks.
pub(crate) fn setppr(span1: f64, alpha1: f64, optlevel: i32, ism: i32, df1: f64, gcvpen1: f64) {
    update_settings(|c| {
        c.span = span1;
        c.lf = optlevel;
        c.alpha = alpha1;
        if ism >= 0 {
            c.ismethod = ism;
            c.trace = false;
        } else {
            c.ismethod = -(ism + 1);
            c.trace = true;
        }
        c.df = df1;
        c.gcvpen = gcvpen1;
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split `buf` into consecutive disjoint mutable slices of the given lengths.
fn split_mut<'a>(buf: &'a mut [f64], lens: &[usize]) -> Vec<&'a mut [f64]> {
    let mut out = Vec::with_capacity(lens.len());
    let mut rest = buf;
    for &l in lens {
        let (head, tail) = rest.split_at_mut(l);
        out.push(head);
        rest = tail;
    }
    out
}

#[inline]
fn rchkusr() {
    unsafe {
        crate::main::errors::R_CheckUserInterrupt();
    }
}

// ---------------------------------------------------------------------------
// smart / smart1
// ---------------------------------------------------------------------------

/// `subroutine smart` — pack the model header and dispatch to `smart1`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn smart(
    mut par: Common,
    m: usize,
    mu: usize,
    p: usize,
    q: usize,
    n: usize,
    w: &[f64],
    x: &[f64],
    y: &[f64],
    ww: &[f64],
    smod: &mut [f64],
    sp: &mut [f64],
    dp: &mut [f64],
    edf: &mut [f64],
) -> Result<(), String> {
    smod[0] = m as f64;
    smod[1] = p as f64;
    smod[2] = q as f64;
    smod[3] = n as f64;
    // smod partitions (0-based): yb(6..6+q), ys(q+6), a, b, f, t, asr.
    let qn = q * n;
    let mut parts = split_mut(
        smod,
        &[
            5, // header: m, p, q, n, flm
            q,
            1,
            p * m,
            q * m,
            n * m,
            n * m,
            15.min(m + 1),
        ],
    );
    let asr = parts.pop().unwrap_or(&mut [][..]);
    let t = parts.pop().unwrap_or(&mut [][..]);
    let f = parts.pop().unwrap_or(&mut [][..]);
    let b = parts.pop().unwrap_or(&mut [][..]);
    let a = parts.pop().unwrap_or(&mut [][..]);
    let ys = parts.pop().unwrap_or(&mut [][..]);
    let yb = parts.pop().unwrap_or(&mut [][..]);
    let sp_parts = split_mut(sp, &[qn, 15 * n, q, 3 * p]);
    let (r, sc, bt, g) = {
        let mut sp_parts = sp_parts;
        let g = sp_parts.pop().unwrap_or(&mut [][..]);
        let bt = sp_parts.pop().unwrap_or(&mut [][..]);
        let sc = sp_parts.pop().unwrap_or(&mut [][..]);
        let r = sp_parts.pop().unwrap_or(&mut [][..]);
        (r, sc, bt, g)
    };
    let lm = smart1(
        &mut par, m, mu, p, q, n, w, x, y, ww, yb, &mut ys[0], a, b, f, t, asr, r, sc, bt, g, dp,
        edf,
    )?;
    smod[4] = lm as f64; // flm
    Ok(())
}

/// `subroutine smart1` — standardize responses, fit and backfit terms.
/// Returns `lm`, the number of fitted terms (written to `smod(5)` by smart).
#[allow(clippy::too_many_arguments)]
fn smart1(
    par: &mut Common,
    m: usize,
    mu: usize,
    p: usize,
    q: usize,
    n: usize,
    w: &[f64],
    x: &[f64],
    y: &[f64],
    ww: &[f64],
    yb: &mut [f64],
    ys: &mut f64,
    a: &mut [f64],
    b: &mut [f64],
    f: &mut [f64],
    t: &mut [f64],
    asr: &mut [f64],
    r: &mut [f64],
    sc: &mut [f64],
    bt: &mut [f64],
    g: &mut [f64],
    dp: &mut [f64],
    edf: &mut [f64],
) -> Result<usize, String> {
    let qn = q * n;
    let mut sw = 0.0;
    for j in 0..n {
        sw += w[j];
    }
    r[..qn].copy_from_slice(&y[..qn]);
    for i in 0..q {
        let mut s = 0.0;
        for j in 0..n {
            s += w[j] * r[j * q + i];
        }
        yb[i] = s / sw;
    }
    for j in 0..n {
        for i in 0..q {
            r[j * q + i] -= yb[i];
        }
    }
    let mut ys2 = 0.0;
    for i in 0..q {
        let mut s = 0.0;
        for j in 0..n {
            let e = r[j * q + i];
            s += w[j] * e * e;
        }
        ys2 += ww[i] * s / sw;
    }
    if !(ys2 > 0.0) {
        return Ok(0); // ys is zero: quit (flm left unset in GNU)
    }
    let ys_v = ys2.sqrt();
    *ys = ys_v;
    let s = 1.0 / ys_v;
    for v in r[..qn].iter_mut() {
        *v *= s;
    }
    // subfit adds up to m terms one at a time.
    let mut lm = subfit(
        par, m, p, q, n, w, sw, x, r, ww, a, b, f, t, asr, sc, bt, g, dp, edf,
    )?;
    if par.lf > 0 {
        fulfit(
            par, lm, par.lf, p, q, n, w, sw, x, r, ww, a, b, f, t, asr, sc, bt, g, dp, edf,
        )?;
    }
    loop {
        // 371: order terms by total |beta| and drop the weakest above mu.
        for l in 0..lm {
            sc[l] = (l + 1) as f64 + 0.1; // sc(l,1)
            let mut s = 0.0;
            for i in 0..q {
                s += ww[i] * b[l * q + i].abs();
            }
            sc[n + l] = -s; // sc(l,2)
        }
        let (key, rest) = sc.split_at_mut(n);
        let (perm, _) = rest.split_at_mut(n);
        sort(&mut perm[..lm], &mut key[..lm], 1, lm);
        r[..qn].copy_from_slice(&y[..qn]);
        for i in 0..q {
            for j in 0..n {
                let mut s = 0.0;
                for l in 0..lm {
                    s += b[l * q + i] * f[l * n + j];
                }
                r[j * q + i] = (r[j * q + i] - yb[i]) / ys_v - s;
            }
        }
        if lm <= mu {
            break; // 9999
        }
        let l = sc[lm - 1].trunc() as usize; // int(sc(lm,1))
        let l0 = l - 1;
        let mut asr1 = 0.0;
        for j in 0..n {
            for i in 0..q {
                r[j * q + i] += b[l0 * q + i] * f[l0 * n + j];
                let e = r[j * q + i];
                asr1 += w[j] * ww[i] * e * e;
            }
        }
        asr1 /= sw;
        asr[0] = asr1;
        if l < lm {
            for i in 0..p {
                a[l0 * p + i] = a[(lm - 1) * p + i];
            }
            for i in 0..q {
                b[l0 * q + i] = b[(lm - 1) * q + i];
            }
            for j in 0..n {
                f[l0 * n + j] = f[(lm - 1) * n + j];
                t[l0 * n + j] = t[(lm - 1) * n + j];
            }
        }
        // 591
        lm -= 1;
        fulfit(
            par, lm, par.lf, p, q, n, w, sw, x, r, ww, a, b, f, t, asr, sc, bt, g, dp, edf,
        )?;
    }
    Ok(lm)
}

// ---------------------------------------------------------------------------
// subfit / fulfit
// ---------------------------------------------------------------------------

/// `subroutine subfit` — forward stepwise addition of terms.
#[allow(clippy::too_many_arguments)]
fn subfit(
    par: &mut Common,
    m: usize,
    p: usize,
    q: usize,
    n: usize,
    w: &[f64],
    sw: f64,
    x: &[f64],
    r: &mut [f64],
    ww: &[f64],
    a: &mut [f64],
    b: &mut [f64],
    f: &mut [f64],
    t: &mut [f64],
    asr: &mut [f64],
    sc: &mut [f64],
    bt: &mut [f64],
    g: &mut [f64],
    dp: &mut [f64],
    edf: &mut [f64],
) -> Result<usize, String> {
    let mut lm = 0usize;
    asr[0] = par.big;
    for _ in 1..=m {
        rchkusr();
        lm += 1;
        let asrold = asr[0];
        newb(par.big, lm, q, ww, b);
        {
            let (ac, bc, fc, tc) = (
                &mut a[(lm - 1) * p..lm * p],
                &mut b[(lm - 1) * q..lm * q],
                &mut f[(lm - 1) * n..lm * n],
                &mut t[(lm - 1) * n..lm * n],
            );
            let mut asr0 = asr[0];
            let mut edf0 = edf[0];
            let (scw, gw) = (&mut sc[..13 * n], &mut g[..2 * p]);
            onetrm(
                par, 0, p, q, n, w, sw, x, r, ww, ac, bc, fc, tc, &mut asr0, scw, gw, dp, &mut edf0,
            )?;
            asr[0] = asr0;
            edf[0] = edf0;
        }
        for j in 0..n {
            for i in 0..q {
                r[j * q + i] -= b[(lm - 1) * q + i] * f[(lm - 1) * n + j];
            }
        }
        if lm == 1 {
            continue; // 100
        }
        if par.lf > 0 {
            if lm == m {
                return Ok(lm);
            }
            let iflsv = par.ifl;
            par.ifl = 0;
            fulfit(
                par, lm, 1, p, q, n, w, sw, x, r, ww, a, b, f, t, asr, sc, bt, g, dp, edf,
            )?;
            par.ifl = iflsv;
        }
        if asr[0] <= 0.0 || (asrold - asr[0]) / asrold < par.conv {
            return Ok(lm);
        }
    }
    Ok(lm)
}

/// `subroutine fulfit` — alternating backfitting over all lm terms.
#[allow(clippy::too_many_arguments)]
fn fulfit(
    par: &mut Common,
    lm: usize,
    lbf: i32,
    p: usize,
    q: usize,
    n: usize,
    w: &[f64],
    sw: f64,
    x: &[f64],
    r: &mut [f64],
    ww: &[f64],
    a: &mut [f64],
    b: &mut [f64],
    f: &mut [f64],
    t: &mut [f64],
    asr: &mut [f64],
    sc: &mut [f64],
    bt: &mut [f64],
    g: &mut [f64],
    dp: &mut [f64],
    edf: &mut [f64],
) -> Result<(), String> {
    if lbf <= 0 {
        return Ok(());
    }
    let mut asri = asr[0];
    let fsv = par.cutmin;
    let isv = par.mitone;
    if lbf < 3 {
        par.cutmin = 1.0;
        par.mitone = lbf - 1;
    }
    let mut iter = 0i32;
    loop {
        // 1000: outer loop
        let asrold = asri;
        iter += 1;
        for lp in 0..lm {
            for i in 0..q {
                bt[i] = b[lp * q + i];
            }
            for i in 0..p {
                g[2 * p + i] = a[lp * p + i]; // g(i,3)
            }
            for j in 0..n {
                for i in 0..q {
                    r[j * q + i] += bt[i] * f[lp * n + j];
                }
            }
            {
                // onetrm(1, ..., a=g(1,3), b=bt, f=sc(1,14), t=sc(1,15), ...)
                let (scw, sc_tail) = sc.split_at_mut(13 * n);
                let (scf, sct) = sc_tail.split_at_mut(n);
                let (gws, a3) = g[..3 * p].split_at_mut(2 * p);
                let mut edf_lp = edf[lp];
                onetrm(
                    par,
                    1,
                    p,
                    q,
                    n,
                    w,
                    sw,
                    x,
                    r,
                    ww,
                    a3,
                    bt,
                    scf,
                    sct,
                    &mut asri,
                    scw,
                    gws,
                    dp,
                    &mut edf_lp,
                )?;
                edf[lp] = edf_lp;
            }
            if asri < asrold {
                for i in 0..q {
                    b[lp * q + i] = bt[i];
                }
                for i in 0..p {
                    a[lp * p + i] = g[2 * p + i];
                }
                for j in 0..n {
                    f[lp * n + j] = sc[13 * n + j]; // sc(j,14)
                    t[lp * n + j] = sc[14 * n + j]; // sc(j,15)
                }
            } else {
                asri = asrold;
            }
            for j in 0..n {
                for i in 0..q {
                    r[j * q + i] -= b[lp * q + i] * f[lp * n + j];
                }
            }
        }
        if !(iter <= par.maxit && asri > 0.0 && (asrold - asri) / asrold >= par.conv) {
            break;
        }
    }
    par.cutmin = fsv;
    par.mitone = isv;
    if par.ifl > 0 {
        asr[lm] = asri;
        asr[0] = asri;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// onetrm / oneone
// ---------------------------------------------------------------------------

/// `subroutine onetrm` — fit one (projection, ridge) term to residuals `y`.
#[allow(clippy::too_many_arguments)]
fn onetrm(
    par: &mut Common,
    jfl: i32,
    p: usize,
    q: usize,
    n: usize,
    w: &[f64],
    sw: f64,
    x: &[f64],
    y: &[f64],
    ww: &[f64],
    a: &mut [f64],
    b: &mut [f64],
    f: &mut [f64],
    t: &mut [f64],
    asr: &mut f64,
    sc: &mut [f64],
    g: &mut [f64],
    dp: &mut [f64],
    edf: &mut f64,
) -> Result<(), String> {
    let mut iter = 0i32;
    *asr = par.big;
    loop {
        // 1000
        iter += 1;
        let asrold = *asr;
        for j in 0..n {
            let mut s = 0.0;
            for i in 0..q {
                s += ww[i] * b[i] * y[j * q + i];
            }
            sc[12 * n + j] = s; // sc(j,13)
        }
        let ist = std::cmp::max(jfl, iter - 1);
        oneone(par, ist, p, n, w, sw, x, a, f, t, asr, sc, g, dp, edf)?;
        for i in 0..q {
            let mut s = 0.0;
            for j in 0..n {
                s += w[j] * y[j * q + i] * f[j];
            }
            b[i] = s / sw;
        }
        *asr = 0.0;
        for i in 0..q {
            let mut s = 0.0;
            for j in 0..n {
                let e = y[j * q + i] - b[i] * f[j];
                s += w[j] * e * e;
            }
            *asr += ww[i] * s / sw;
        }
        if !(q != 1 && iter <= par.maxit && *asr > 0.0 && (asrold - *asr) / asrold >= par.conv) {
            return Ok(());
        }
    }
}

/// `subroutine oneone` — alternate direction search and smoothing for one term.
/// The projection response `y(n)` aliases `sc(1,13)` exactly as in GNU.
#[allow(clippy::too_many_arguments)]
fn oneone(
    par: &mut Common,
    ist: i32,
    p: usize,
    n: usize,
    w: &[f64],
    sw: f64,
    x: &[f64],
    a: &mut [f64],
    f: &mut [f64],
    t: &mut [f64],
    asr: &mut f64,
    sc: &mut [f64],
    g: &mut [f64],
    dp: &mut [f64],
    edf: &mut f64,
) -> Result<(), String> {
    let sml = 1.0 / par.big;
    if ist <= 0 {
        if p <= 1 {
            a[0] = 1.0;
        }
        for j in 0..n {
            sc[n + j] = 1.0; // sc(j,2)
        }
        let mut parts = split_mut(sc, &[2 * n, 10 * n, n]); // c1-c2, c3-c12, y(=c13)
        let y_slice = parts.pop().unwrap_or(&mut [][..]);
        let _c3_c12 = parts.pop().unwrap_or(&mut [][..]);
        let mut d_slice = parts.pop().unwrap_or(&mut [][..]);
        pprdir(
            par,
            p,
            n,
            w,
            sw,
            &y_slice[..], // r = y
            x,
            &d_slice[n..2 * n], // d = sc(1,2)
            a,
            dp,
        );
    }
    let mut s = 0.0;
    for i in 0..p {
        g[i] = 0.0; // g(i,1)
        s += a[i] * a[i];
    }
    s = 1.0 / s.sqrt();
    for i in 0..p {
        a[i] *= s;
    }
    let mut iter = 0i32;
    *asr = par.big;
    let mut cut = 1.0;
    'outer: loop {
        // 100
        iter += 1;
        let asrold = *asr;
        let mut s;
        let mut rejected = false;
        'inner: loop {
            // 60
            s = 0.0;
            for i in 0..p {
                g[p + i] = a[i] + g[i]; // g(i,2)
                s += g[p + i] * g[p + i];
            }
            s = 1.0 / s.sqrt();
            for i in 0..p {
                g[p + i] *= s;
            }
            for j in 0..n {
                sc[j] = (j + 1) as f64 + 0.1; // sc(j,1)
                let mut s = 0.0;
                for i in 0..p {
                    s += g[p + i] * x[j * p + i];
                }
                sc[10 * n + j] = s; // sc(j,11)
            }
            {
                // sort(sc(1,11), sc, 1, n): v is column 11, a is column 1.
                let (key, rest) = sc.split_at_mut(10 * n);
                sort(&mut rest[..n], &mut key[..n], 1, n);
            }
            for j in 0..n {
                let k = sc[j].trunc() as usize - 1; // int(sc(j,1))
                sc[n + j] = sc[12 * n + k]; // sc(j,2) = y(k)
                sc[2 * n + j] = w[k].max(sml); // sc(j,3)
            }
            {
                // supsmu(n, sc(1,11), sc(1,2), sc(1,3), 1, span, alpha,
                //        sc(1,12), sc(1,4), edf)
                let mut parts = split_mut(sc, &[n, n, n, 7 * n, n, n]); // c1,c2,c3,c4-c10,c11,c12
                let c12 = parts.pop().unwrap_or(&mut [][..]);
                let c11 = parts.pop().unwrap_or(&mut [][..]);
                let c4_c10 = parts.pop().unwrap_or(&mut [][..]);
                let c3 = parts.pop().unwrap_or(&mut [][..]);
                let c2 = parts.pop().unwrap_or(&mut [][..]);
                supsmu_ppr(
                    par,
                    n,
                    &c11[..],
                    &c2[..],
                    &c3[..],
                    &mut c12[..],
                    &mut c4_c10[..],
                    edf,
                )?;
            }
            s = 0.0;
            for j in 0..n {
                let e = sc[n + j] - sc[11 * n + j]; // sc(j,2) - sc(j,12)
                s += sc[2 * n + j] * e * e;
            }
            s /= sw;
            if s < *asr {
                break 'inner; // 140
            }
            cut *= 0.5;
            if cut < par.cutmin {
                rejected = true;
                break 'inner; // 199
            }
            for i in 0..p {
                g[i] *= cut;
            }
            // goto 60
        }
        if rejected {
            break 'outer; // 199
        }
        // 140
        *asr = s;
        cut = 1.0;
        for i in 0..p {
            a[i] = g[p + i];
        }
        for j in 0..n {
            let k = sc[j].trunc() as usize - 1;
            t[k] = sc[10 * n + j];
            f[k] = sc[11 * n + j];
        }
        if *asr <= 0.0 || (asrold - *asr) / asrold < par.conv {
            break 'outer; // 199
        }
        if iter > par.mitone || p <= 1 {
            break 'outer; // 199
        }
        {
            // pprder(n, sc(1,11), sc(1,12), sc(1,3), fdel, sc(1,4), sc(1,5))
            let mut parts = split_mut(sc, &[n, n, n, 4 * n, 5 * n]); // c1,c2,c3,c4-c7,c8-c12
            let c8_c12 = parts.pop().unwrap_or(&mut [][..]);
            let c4_c7 = parts.pop().unwrap_or(&mut [][..]);
            let c3 = parts.pop().unwrap_or(&mut [][..]);
            let (d, sc5) = c4_c7.split_at_mut(n);
            pprder(
                n,
                &c8_c12[3 * n..4 * n], // x = sc(1,11)
                &c8_c12[4 * n..5 * n], // s = sc(1,12)
                &c3[..],               // w = sc(1,3)
                par.fdel,
                &mut d[..],   // d = sc(1,4)
                &mut sc5[..], // sc = sc(1,5) (n,3)
            )?;
        }
        for j in 0..n {
            let k = sc[j].trunc() as usize - 1;
            sc[4 * n + j] = sc[12 * n + j] - f[j]; // sc(j,5) = y(j)-f(j)
            sc[5 * n + k] = sc[3 * n + j]; // sc(k,6) = sc(j,4)
        }
        {
            // pprdir(p,n,w,sw,sc(1,5),x,sc(1,6),g,dp)
            let mut parts = split_mut(sc, &[5 * n, n, 7 * n]); // c1-c5, c6, c7-c13
            let _c7_c13 = parts.pop().unwrap_or(&mut [][..]);
            let c6 = parts.pop().unwrap_or(&mut [][..]);
            let c1_c5 = parts.pop().unwrap_or(&mut [][..]);
            pprdir(
                par,
                p,
                n,
                w,
                sw,
                &c1_c5[4 * n..5 * n], // r = sc(1,5)
                x,
                &c6[..],     // d = sc(1,6)
                &mut g[..p], // e = g(1,1)
                dp,
            );
        }
        // goto 100
    }
    // 199: center f and scale to unit weighted variance
    let mut s = 0.0;
    let mut v = 0.0;
    for j in 0..n {
        s += w[j] * f[j];
    }
    s /= sw;
    for j in 0..n {
        f[j] -= s;
        v += w[j] * f[j] * f[j];
    }
    if v > 0.0 {
        let vi = 1.0 / (v / sw).sqrt();
        for j in 0..n {
            f[j] *= vi;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// pprdir / ppconj
// ---------------------------------------------------------------------------

/// `subroutine pprdir` — optimal projection direction via conjugate gradients.
fn pprdir(
    par: &Common,
    p: usize,
    n: usize,
    w: &[f64],
    sw: f64,
    r: &[f64],
    x: &[f64],
    d: &[f64],
    e: &mut [f64],
    g: &mut [f64],
) {
    let m1 = p * (p + 1) / 2;
    let m2 = m1 + p;
    for i in 0..p {
        let mut s = 0.0;
        for j in 0..n {
            s += w[j] * d[j] * x[j * p + i];
        }
        e[i] = s / sw;
    }
    let mut k = 0usize;
    for j in 0..p {
        let mut s = 0.0;
        for l in 0..n {
            s += w[l] * r[l] * (d[l] * x[l * p + j] - e[j]);
        }
        g[m1 + j] = s / sw;
        for i in 0..=j {
            let mut s = 0.0;
            for l in 0..n {
                s += w[l] * (d[l] * x[l * p + i] - e[i]) * (d[l] * x[l * p + j] - e[j]);
            }
            k += 1;
            g[k - 1] = s / sw;
        }
    }
    ppconj(p, g, par.cjeps, par.mitcj);
    for i in 0..p {
        e[i] = g[m2 + i];
    }
}

/// `subroutine ppconj` — conjugate gradient solution of g·x = c.
/// Layout within `g`: packed matrix (1..m1), c (m1+1..m1+p), x (m2+1..m2+p),
/// workspace sc(p,4) at m2+p+1.
fn ppconj(p: usize, g: &mut [f64], eps: f64, maxit: i32) {
    let m1 = p * (p + 1) / 2;
    let m2 = m1 + p;
    let scb = m2 + p; // sc(i,c) = g[scb + (c-1)*p + i]
    #[inline]
    fn diag(g: &[f64], i: usize) -> f64 {
        g[i * (i + 1) / 2 + i]
    }
    #[inline]
    fn off(g: &[f64], i: usize, j: usize) -> f64 {
        // g(i*(i-1)/2+j) for j < i (1-based): packed A(i,j) sits at
        // i*(i+1)/2+j in 0-based offsets (column j+1 block, row i).
        g[i * (i + 1) / 2 + j]
    }
    for i in 0..p {
        g[m2 + i] = 0.0; // x(i)
        g[scb + p + i] = 0.0; // sc(i,2)
    }
    let mut nit = 0i32;
    loop {
        // 11321
        nit += 1;
        let mut h = 0.0;
        let mut beta = 0.0;
        for i in 0..p {
            g[scb + 3 * p + i] = g[m2 + i]; // sc(i,4) = x(i)
            let mut s = diag(g, i) * g[m2 + i];
            for j in 0..i {
                s += off(g, i, j) * g[m2 + j];
            }
            for j in (i + 1)..p {
                s += off(g, j, i) * g[m2 + j];
            }
            g[scb + i] = s - g[m1 + i]; // sc(i,1)
            h += g[scb + i] * g[scb + i];
        }
        if h <= 0.0 {
            return;
        }
        let mut broke = false;
        for _iter in 0..p {
            for i in 0..p {
                g[scb + p + i] = beta * g[scb + p + i] - g[scb + i]; // sc(i,2)
            }
            let mut t = 0.0;
            for i in 0..p {
                let mut s = diag(g, i) * g[scb + p + i];
                for j in 0..i {
                    s += off(g, i, j) * g[scb + p + j];
                }
                for j in (i + 1)..p {
                    s += off(g, j, i) * g[scb + p + j];
                }
                g[scb + 2 * p + i] = s; // sc(i,3)
                t += s * g[scb + p + i];
            }
            let alpha = h / t;
            let mut s = 0.0;
            for i in 0..p {
                g[m2 + i] += alpha * g[scb + p + i]; // x(i)
                g[scb + i] += alpha * g[scb + 2 * p + i];
                s += g[scb + i] * g[scb + i];
            }
            if s <= 0.0 {
                broke = true;
                break; // 11362
            }
            beta = s / h;
            h = s;
        }
        let _ = broke;
        // 11362
        let mut s = 0.0_f64;
        for i in 0..p {
            s = s.max((g[m2 + i] - g[scb + 3 * p + i]).abs());
        }
        if !(s >= eps && nit < maxit) {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// pprder / pool
// ---------------------------------------------------------------------------

/// `subroutine pprder` — derivatives of the smoothed curve via pooled points.
/// All indices below are 1-based in the local variables, matching ppr.f.
#[allow(clippy::too_many_arguments)]
fn pprder(
    n: usize,
    x: &[f64],
    s_: &[f64],
    w_: &[f64],
    fdel: f64,
    d: &mut [f64],
    sc: &mut [f64],
) -> Result<(), String> {
    // sc is (n,3): sc(i,c) = sc[(c-1)*n + i-1]
    #[inline]
    fn c1(sc: &[f64], n: usize, i: usize) -> f64 {
        sc[i - 1]
    }
    #[inline]
    fn c2(sc: &[f64], n: usize, i: usize) -> f64 {
        sc[n + i - 1]
    }
    #[inline]
    fn c3(sc: &[f64], n: usize, i: usize) -> f64 {
        sc[2 * n + i - 1]
    }
    if x[n - 1] <= x[0] {
        for j in 1..=n {
            d[j - 1] = 0.0;
        }
        return Ok(());
    }
    let mut i = (n / 4).max(1); // GNU reads x(0) for n < 4 (undefined); clamp
    let mut j = (3 * i).min(n);
    let mut scale = x[j - 1] - x[i - 1];
    while scale <= 0.0 {
        if j < n {
            j += 1;
        }
        if i > 1 {
            i -= 1;
        }
        scale = x[j - 1] - x[i - 1];
    }
    let del = fdel * scale * 2.0;
    for jj in 1..=n {
        sc[jj - 1] = x[jj - 1];
        sc[n + jj - 1] = s_[jj - 1];
        sc[2 * n + jj - 1] = w_[jj - 1];
    }
    pool(n, sc, del);
    let mut bc = 0usize;
    let mut br = 0usize;
    let mut er = 0usize;
    let mut bl = 0usize;
    let mut el = 0usize;
    let mut ec = 0usize;
    loop {
        // 11481
        br = er + 1;
        er = br;
        // 11491: extend er while x values coincide with x(br)
        while er < n && c1(sc, n, br) == c1(sc, n, er + 1) {
            er += 1;
        }
        if br == 1 {
            bl = br;
            el = er;
            continue; // goto 11481
        }
        if bc == 0 {
            bc = br;
            ec = er;
            for jj in bl..=el {
                d[jj - 1] = (c2(sc, n, bc) - c2(sc, n, bl)) / (c1(sc, n, bc) - c1(sc, n, bl));
            }
            continue; // goto 11481
        }
        // sanity check needed for PR#13517
        if br > n {
            return Err("br is too large".to_string());
        }
        for jj in bc..=ec {
            d[jj - 1] = (c2(sc, n, br) - c2(sc, n, bl)) / (c1(sc, n, br) - c1(sc, n, bl));
        }
        if er == n {
            for jj in br..=er {
                d[jj - 1] = (c2(sc, n, br) - c2(sc, n, bc)) / (c1(sc, n, br) - c1(sc, n, bc));
            }
            return Ok(()); // 11482
        }
        bl = bc;
        el = ec;
        bc = br;
        ec = er;
    }
}

/// `subroutine pool` — pool adjacent points closer than `del`.
/// Operates in place on the (n,3) workspace [x, y, w].
fn pool(n: usize, sc: &mut [f64], del: f64) {
    #[inline]
    fn x(sc: &[f64], i: usize) -> f64 {
        sc[i - 1]
    }
    #[inline]
    fn y(sc: &[f64], n: usize, i: usize) -> f64 {
        sc[n + i - 1]
    }
    #[inline]
    fn w(sc: &[f64], n: usize, i: usize) -> f64 {
        sc[2 * n + i - 1]
    }
    let mut bb;
    let mut eb = 0usize;
    'outer: loop {
        // 11621
        if eb >= n {
            return; // 11622
        }
        bb = eb + 1;
        eb = bb;
        // 11631: pool identical abscissae into [bb..eb]
        while eb < n && x(sc, bb) == x(sc, eb + 1) {
            eb += 1;
        }
        if eb < n && x(sc, eb + 1) - x(sc, eb) < del {
            // 11711: candidate right run
            let br = eb + 1;
            let mut er = br;
            while er < n && x(sc, er + 1) == x(sc, br) {
                er += 1;
            }
            if er >= n || x(sc, er + 1) - x(sc, er) >= x(sc, eb + 1) - x(sc, eb) {
                eb = er;
                let pw = w(sc, n, bb) + w(sc, n, eb);
                let px = (x(sc, bb) * w(sc, n, bb) + x(sc, eb) * w(sc, n, eb)) / pw;
                let py = (y(sc, n, bb) * w(sc, n, bb) + y(sc, n, eb) * w(sc, n, eb)) / pw;
                for i in bb..=eb {
                    sc[i - 1] = px;
                    sc[n + i - 1] = py;
                    sc[2 * n + i - 1] = pw;
                }
            } else {
                continue 'outer; // right run is tighter: pool it first
            }
        }
        // 11761: backward merge loop
        loop {
            if bb <= 1 {
                break; // 11762
            }
            if x(sc, bb) - x(sc, bb - 1) >= del {
                break; // 11762
            }
            let mut bl = bb - 1;
            let el = bl;
            while bl > 1 && x(sc, bl - 1) == x(sc, el) {
                bl -= 1;
            }
            bb = bl;
            let pw = w(sc, n, bb) + w(sc, n, eb);
            let px = (x(sc, bb) * w(sc, n, bb) + x(sc, eb) * w(sc, n, eb)) / pw;
            let py = (y(sc, n, bb) * w(sc, n, bb) + y(sc, n, eb) * w(sc, n, eb)) / pw;
            for i in bb..=eb {
                sc[i - 1] = px;
                sc[n + i - 1] = py;
                sc[2 * n + i - 1] = pw;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// newb
// ---------------------------------------------------------------------------

/// `subroutine newb` — initial ridge loadings for term `lm`.
fn newb(big: f64, lm: usize, q: usize, ww: &[f64], b: &mut [f64]) {
    let sml = 1.0 / big;
    // Fortran writes column lm of b(q,m): b(i,lm) = b[(lm-1)*q + i].
    let c = (lm - 1) * q;
    if q == 1 {
        b[c] = 1.0;
        return;
    }
    if lm == 1 {
        for i in 0..q {
            b[c + i] = (i + 1) as f64;
        }
        return;
    }
    let lm1 = lm - 1;
    for i in 0..q {
        b[c + i] = 0.0;
    }
    let mut t = 0.0;
    for i in 0..q {
        let mut s = 0.0;
        for l in 0..lm1 {
            s += b[l * q + i].abs();
        }
        b[c + i] = s;
        t += s;
    }
    for i in 0..q {
        b[c + i] = ww[i] * (t - b[c + i]);
    }
    let l1 = if lm > q { lm - q + 1 } else { 1 };
    for l in l1..=lm1 {
        let mut s = 0.0;
        let mut t = 0.0;
        for i in 0..q {
            s += ww[i] * b[c + i] * b[(l - 1) * q + i];
            t += ww[i] * b[(l - 1) * q + i] * b[(l - 1) * q + i];
        }
        s /= t.sqrt();
        for i in 0..q {
            b[c + i] -= s * b[(l - 1) * q + i];
        }
    }
    for i in 1..q {
        if (b[c + i - 1] - b[c + i]).abs() > sml {
            return;
        }
    }
    for i in 0..q {
        b[c + i] = (i + 1) as f64;
    }
}

// ---------------------------------------------------------------------------
// supsmu / spline / splineAA (ppr-driven smoothing dispatch)
// ---------------------------------------------------------------------------

/// `subroutine supsmu` as called from oneone: iper is always 1; span/alpha and
/// the spline settings come from the ppr COMMON blocks set by `setppr`.
fn supsmu_ppr(
    par: &mut Common,
    n: usize,
    x: &[f64],
    y: &[f64],
    w: &[f64],
    smo: &mut [f64],
    sc: &mut [f64], // 7n scratch (cols 4..10 of oneone's sc)
    edf: &mut f64,
) -> Result<(), String> {
    if x[n - 1] > x[0] {
        if par.ismethod != 0 {
            spline(par, n, x, y, w, smo, edf, sc);
        } else {
            let params = Parameters {
                n,
                periodic: 1,
                span: par.span,
                alpha: par.alpha,
            };
            supsmu::filter(
                Input {
                    x: &x[..n],
                    y: &y[..n],
                    weights: &w[..n],
                },
                params,
                Output {
                    smoothed: smo,
                    scratch: sc,
                    edf: std::slice::from_mut(edf),
                    trace: None,
                },
            )
            .map_err(|e| e.to_string())?;
        }
    } else {
        // boundary case: weighted mean
        let mut sy = 0.0;
        let mut sw = 0.0;
        for j in 0..n {
            sy += w[j] * y[j];
            sw += w[j];
        }
        let a = if sw > 0.0 { sy / sw } else { 0.0 };
        smo[..n].fill(a);
    }
    Ok(())
}

/// `subroutine spline` — penalized regression spline via rbart (ppr.f).
fn spline(
    par: &Common,
    n: usize,
    x: &[f64],
    y: &[f64],
    w: &[f64],
    smo: &mut [f64],
    edf: &mut f64,
    sc: &mut [f64], // (n,7): cols 1..5 used as dx,dy,dw,dsmo,lev
) {
    let mut cols = split_mut(sc, &[5 * n, 2 * n]);
    let _unused = cols.pop().unwrap_or(&mut [][..]);
    let used = cols.pop().unwrap_or(&mut [][..]);
    let (dxa, rest) = used.split_at_mut(n);
    let (dya, rest) = rest.split_at_mut(n);
    let (dwa, rest) = rest.split_at_mut(n);
    let (dsmo, lev) = rest.split_at_mut(n);
    splineAA(par, n, x, y, w, smo, edf, dxa, dya, dwa, dsmo, lev);
}

/// `subroutine splineAA` — workhorse of spline(); statement-level port.
#[allow(clippy::too_many_arguments)]
fn splineAA(
    par: &Common,
    n: usize,
    x: &[f64],
    y: &[f64],
    w: &[f64],
    smo: &mut [f64],
    edf: &mut f64,
    dx: &mut [f64],
    dy: &mut [f64],
    dw: &mut [f64],
    dsmo: &mut [f64],
    lev: &mut [f64],
) {
    for i in 0..n {
        dx[i] = (x[i] - x[0]) / (x[n - 1] - x[0]);
        dy[i] = y[i];
        dw[i] = w[i];
    }
    let nk = n.min(15);
    let mut knot = [0.0f64; 29];
    let mut coef = [0.0f64; 25];
    let mut work = [0.0f64; (17 + 25) * 25];
    for v in knot[0..4].iter_mut() {
        *v = dx[0];
    }
    for v in knot[nk..nk + 4].iter_mut() {
        *v = dx[n - 1];
    }
    for i in 5..=nk {
        // (n-1)*real(i-4)/real(nk-3): single-precision real() arithmetic
        let p = ((n - 1) as f32 * (i - 4) as f32 / (nk - 3) as f32) as f64;
        let ip = p.trunc() as usize;
        let p = p - p.trunc();
        knot[i - 1] = (1.0 - p) * dx[ip] + p * dx[ip + 1];
    }
    let mut iparms = [0i32; 4];
    let df1;
    if par.ismethod == 1 {
        iparms[0] = 3;
        df1 = par.df;
    } else {
        iparms[0] = 1;
        df1 = 0.0;
    }
    iparms[1] = 0; // ispar := 0 <==> estimate spar
    iparms[2] = 500; // maxit
    iparms[3] = 0;
    let mut param = [0.0f64; 5];
    param[1] = 1.5; // uspar
    param[2] = 1.0e-2; // tol for spar estimation
    param[3] = 0.000244; // eps for sbart
    let mut crit = 0.0;
    let mut lambda = 0.0;
    let mut ier = 0i32;
    let mut penalt = par.gcvpen;
    let mut dofoff = df1;
    let mut ssw = 0.0f64;
    let mut ni = n as i32;
    let mut nki = nk as i32;
    let mut ld4 = 4i32;
    let mut ldnk = 1i32;
    unsafe {
        crate::library::stats::sbart::c_rbart(
            &mut penalt as *mut f64 as *mut std::ffi::c_void,
            &mut dofoff as *mut f64 as *mut std::ffi::c_void,
            dx.as_mut_ptr() as *mut std::ffi::c_void,
            dy.as_mut_ptr() as *mut std::ffi::c_void,
            dw.as_mut_ptr() as *mut std::ffi::c_void,
            &mut ssw as *mut f64 as *mut std::ffi::c_void,
            &mut ni as *mut i32 as *mut std::ffi::c_void,
            knot.as_mut_ptr() as *mut std::ffi::c_void,
            &mut nki as *mut i32 as *mut std::ffi::c_void,
            coef.as_mut_ptr() as *mut std::ffi::c_void,
            dsmo.as_mut_ptr() as *mut std::ffi::c_void,
            lev.as_mut_ptr() as *mut std::ffi::c_void,
            &mut crit as *mut f64 as *mut std::ffi::c_void,
            iparms.as_mut_ptr() as *mut std::ffi::c_void,
            &mut lambda as *mut f64 as *mut std::ffi::c_void,
            param.as_mut_ptr() as *mut std::ffi::c_void,
            work.as_mut_ptr() as *mut std::ffi::c_void,
            &mut ld4 as *mut i32 as *mut std::ffi::c_void,
            &mut ldnk as *mut i32 as *mut std::ffi::c_void,
            &mut ier as *mut i32 as *mut std::ffi::c_void,
        );
    }
    if ier > 0 {
        let msg = format!("spline(.) TROUBLE: {}\n", ier);
        if let Ok(c) = std::ffi::CString::new(msg) {
            unsafe {
                crate::mainutils::printutils::Rprintf(c.as_ptr(), std::ptr::null_mut());
            }
        }
    }
    smo[..n].copy_from_slice(&dsmo[..n]);
    let mut s = 0.0;
    for i in 0..n {
        s += lev[i];
    }
    *edf = s;
    if par.trace {
        let mut df = par.df;
        let mut gcvpen = par.gcvpen;
        let mut ismethod = par.ismethod;
        let mut lam = lambda;
        let mut e = *edf;
        unsafe {
            crate::library::stats::ksmooth::splineprt_(
                &mut df,
                &mut gcvpen,
                &mut ismethod,
                &mut lam,
                &mut e,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// sort (CACM #347, R. C. Singleton — modified Hoare quicksort)
// ---------------------------------------------------------------------------

/// `subroutine sort(v, a, ii, jj)` — permutation `a` sorting `v` increasingly.
/// `v` and `a` are disjoint slices; indices `ii..jj` are 1-based inclusive.
fn sort(v: &mut [f64], a: &mut [f64], ii: usize, jj: usize) {
    #[inline]
    fn va(buf: &mut [f64], i: usize) -> f64 {
        buf[i - 1]
    }
    let mut iu = [0i32; 20];
    let mut il = [0i32; 20];
    let mut m = 1usize;
    let mut i = ii;
    let mut j = jj;
    // states: 10 (outer), 20 (partition), 80 (pop), 90 (dispatch), 100 (insert)
    let mut t: i64 = 0;
    let mut vt: f64 = 0.0;
    let mut tt: i64 = 0;
    let mut vtt: f64 = 0.0;
    let mut k = i;
    let mut l = j;
    let mut state = 10;
    loop {
        match state {
            10 => {
                if i >= j {
                    state = 80;
                } else {
                    state = 20;
                }
            }
            20 => {
                k = i;
                let ij = (j + i) / 2;
                t = a[ij - 1].trunc() as i64;
                vt = v[ij - 1];
                if v[i - 1] > vt {
                    a[ij - 1] = a[i - 1];
                    a[i - 1] = t as f64;
                    t = a[ij - 1].trunc() as i64;
                    v[ij - 1] = v[i - 1];
                    v[i - 1] = vt;
                    vt = v[ij - 1];
                }
                l = j;
                if v[j - 1] < vt {
                    a[ij - 1] = a[j - 1];
                    a[j - 1] = t as f64;
                    t = a[ij - 1].trunc() as i64;
                    v[ij - 1] = v[j - 1];
                    v[j - 1] = vt;
                    vt = v[ij - 1];
                    if v[i - 1] > vt {
                        a[ij - 1] = a[i - 1];
                        a[i - 1] = t as f64;
                        t = a[ij - 1].trunc() as i64;
                        v[ij - 1] = v[i - 1];
                        v[i - 1] = vt;
                        vt = v[ij - 1];
                    }
                }
                'part: loop {
                    // 50
                    loop {
                        l -= 1;
                        if !(l >= 1 && va(v, l) > vt) {
                            break;
                        }
                    }
                    tt = a[l - 1].trunc() as i64;
                    vtt = v[l - 1];
                    // 60
                    loop {
                        k += 1;
                        if !(k <= jj && va(v, k) < vt) {
                            break;
                        }
                    }
                    if k > l {
                        break 'part;
                    }
                    a[l - 1] = a[k - 1];
                    a[k - 1] = tt as f64;
                    v[l - 1] = v[k - 1];
                    v[k - 1] = vtt;
                }
                if (l as i64 - i as i64) <= (j as i64 - k as i64) {
                    il[m - 1] = k as i32;
                    iu[m - 1] = j as i32;
                    j = l;
                } else {
                    il[m - 1] = i as i32;
                    iu[m - 1] = l as i32;
                    i = k;
                }
                m += 1;
                state = 90;
            }
            80 => loop {
                m -= 1;
                if m == 0 {
                    return;
                }
                i = il[m - 1] as usize;
                j = iu[m - 1] as usize;
                state = 90;
                break;
            },
            90 => {
                if (j as i64 - i as i64) > 10 {
                    state = 20;
                } else if i == ii {
                    state = 10;
                } else {
                    i -= 1;
                    state = 100;
                }
            }
            100 => loop {
                // insertion
                i += 1;
                if i == j {
                    state = 80;
                    break;
                }
                t = a[i].trunc() as i64; // a(i+1)
                vt = v[i]; // v(i+1)
                if v[i - 1] <= vt {
                    continue;
                }
                k = i;
                loop {
                    // 110
                    a[k] = a[k - 1];
                    v[k] = v[k - 1];
                    k -= 1;
                    if k == 0 || vt >= v[k - 1] {
                        break;
                    }
                }
                a[k] = t as f64;
                v[k] = vt;
            },
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn ppr_settings_are_original_runtime_owned_and_snapshots_stay_immutable() {
        assert_eq!(
            trace_enabled(),
            Ok(false),
            "unscoped direct calls stay quiet"
        );
        let first = RSession::new_for_gc_tests();
        let second = RSession::new_for_gc_tests();
        let original = first.with_active(|| {
            assert_eq!(settings(), Common::default());
            assert_eq!(trace_enabled(), Ok(false));
            setppr(0.3, 2.0, 1, -2, 5.0, 1.5);
            let saved = settings();
            assert_eq!(saved.ismethod, 1);
            assert!(saved.trace);
            assert_eq!(trace_enabled(), Ok(true));
            second.with_active(|| {
                assert_eq!(settings(), Common::default());
                assert_eq!(trace_enabled(), Ok(false));
                setppr(0.8, 7.0, 3, 2, 8.0, 2.5);
                assert_eq!(settings().span, 0.8);
            });
            assert_eq!(
                settings(),
                saved,
                "nested runtime restores original settings"
            );
            setsmu(0);
            let reset = settings();
            assert_eq!(reset.ismethod, 0);
            assert!(!reset.trace);
            assert_eq!(trace_enabled(), Ok(false));
            assert_eq!(reset.span, saved.span);
            assert_eq!(reset.alpha, saved.alpha);
            assert_eq!(reset.lf, saved.lf);
            assert_eq!(reset.df, saved.df);
            assert_eq!(reset.gcvpen, saved.gcvpen);
            setsmu(-1);
            assert!(settings().trace);
            assert_eq!(trace_enabled(), Ok(true));
            assert_eq!(saved.ismethod, 1, "the invocation snapshot cannot change");
            saved
        });
        second.with_active(|| {
            let saved = settings();
            assert_eq!(saved.span, 0.8);
            assert_eq!(saved.ismethod, 2);
            assert!(!saved.trace);
        });
        drop(first);
        assert_eq!(original.span, 0.3);
        assert_eq!(original.ismethod, 1);
        second.with_active(|| assert_eq!(settings().span, 0.8));
    }

    #[test]
    fn trace_setting_rejects_a_configured_but_revoked_owner() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            crate::sexp::instance::with_current_instance(|instance| unsafe {
                crate::sexp::instance::revoke_instance_availability(instance);
                assert_eq!(
                    trace_enabled(),
                    Err("configured R runtime owner is no longer active")
                );
            })
            .expect("the initial owner is live");
        });
        drop(session);
    }
}
