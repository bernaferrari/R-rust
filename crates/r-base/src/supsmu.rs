//! Friedman's super smoother exactly as the Fortran in ppr.f arranges —
//! running-line fits at the three tweeter/midrange/woofer spans,
//! cross-validated span selection with the bass tone adjustment, and the
//! final interpolation smooth. The port keeps the original's indexing
//! arithmetic and online mean/variance recurrences statement-for-statement.

const SPANS: [f64; 3] = [0.05, 0.2, 0.5];
const BIG: f64 = 1.0e20;
const SML: f64 = 1.0e-7;
const EPS: f64 = 1.0e-3;

/// The running-lines smoother exactly as the Fortran smooth arranges —
/// iper positive answers cross-validated residuals in acvr, negative
/// suppressing them; the window recurrence slides one out/one in per step
/// with the non-periodic edge freeze.
#[allow(clippy::too_many_arguments)]
fn smooth(
    x: &[f64],
    y: &[f64],
    w: &[f64],
    span: f64,
    iper: i32,
    vsmlsq: f64,
    smo: &mut [f64],
    acvr: &mut [f64],
) {
    let n = x.len();
    let mut xm = 0.0f64;
    let mut ym = 0.0f64;
    let mut var = 0.0f64;
    let mut cvar = 0.0f64;
    let mut fbw = 0.0f64;
    let jper = iper.abs();
    let mut ibw = (0.5 * span * n as f64 + 0.5) as i64;
    if ibw < 2 {
        ibw = 2;
    }
    let mut it = 2 * ibw + 1;
    if it > n as i64 {
        it = n as i64;
    }
    // Initial window fill exactly as the Fortran arranges — 1-based i in
    // 1..=it, periodic windows wrapping with the unit-period shift.
    for i in 1..=it {
        let mut j = i;
        if jper == 2 {
            j = i - ibw - 1;
        }
        let (xti, yj, wt);
        if j >= 1 {
            let idx = (j - 1) as usize;
            xti = x[idx];
            yj = y[idx];
            wt = w[idx];
        } else {
            let idx = ((n as i64 + j) - 1) as usize;
            xti = x[idx] - 1.0;
            yj = y[idx];
            wt = w[idx];
        }
        let fbo = fbw;
        fbw += wt;
        if fbw > 0.0 {
            xm = (fbo * xm + wt * xti) / fbw;
        }
        if fbw > 0.0 {
            ym = (fbo * ym + wt * yj) / fbw;
        }
        let mut tmp = 0.0;
        if fbo > 0.0 {
            tmp = fbw * wt * (xti - xm) / fbo;
        }
        var += tmp * (xti - xm);
        cvar += tmp * (yj - ym);
    }

    for j1 in 1..=(n as i64) {
        let j = j1; // 1-based position exactly as the Fortran arranges
        let mut out = j - ibw - 1;
        let mut inn = j + ibw;
        let frozen = jper != 2 && (out < 1 || inn > n as i64);
        if !frozen {
            let (xto, xti, wout, yin, yout, win);
            if out < 1 {
                out += n as i64;
                xto = x[(out - 1) as usize] - 1.0;
                xti = x[(inn - 1) as usize];
            } else if inn > n as i64 {
                inn -= n as i64;
                xti = x[(inn - 1) as usize] + 1.0;
                xto = x[(out - 1) as usize];
            } else {
                xto = x[(out - 1) as usize];
                xti = x[(inn - 1) as usize];
            }
            wout = w[(out - 1) as usize];
            yout = y[(out - 1) as usize];
            win = w[(inn - 1) as usize];
            yin = y[(inn - 1) as usize];
            // out first exactly as the Fortran ordering arranges.
            let fbo = fbw;
            fbw -= wout;
            let mut tmp = 0.0;
            if fbw > 0.0 {
                tmp = fbo * wout * (xto - xm) / fbw;
            }
            var -= tmp * (xto - xm);
            cvar -= tmp * (yout - ym);
            if fbw > 0.0 {
                xm = (fbo * xm - wout * xto) / fbw;
            }
            if fbw > 0.0 {
                ym = (fbo * ym - wout * yout) / fbw;
            }
            let fbo = fbw;
            fbw += win;
            if fbw > 0.0 {
                xm = (fbo * xm + win * xti) / fbw;
            }
            if fbw > 0.0 {
                ym = (fbo * ym + win * yin) / fbw;
            }
            let mut tmp = 0.0;
            if fbo > 0.0 {
                tmp = fbw * win * (xti - xm) / fbo;
            }
            var += tmp * (xti - xm);
            cvar += tmp * (yin - ym);
        }
        let idx = (j - 1) as usize;
        let mut a = 0.0;
        if var > vsmlsq {
            a = cvar / var;
        }
        smo[idx] = a * (x[idx] - xm) + ym;
        if iper > 0 {
            let mut h = 0.0;
            if fbw > 0.0 {
                h = 1.0 / fbw;
            }
            if var > vsmlsq {
                h += (x[idx] - xm) * (x[idx] - xm) / var;
            }
            let a2 = 1.0 - w[idx] * h;
            if a2 > 0.0 {
                acvr[idx] = (y[idx] - smo[idx]).abs() / a2;
            } else if idx > 0 {
                acvr[idx] = acvr[idx - 1];
            } else {
                acvr[idx] = 0.0;
            }
        }
    }

    // Tied-x recomputation exactly as the Fortran arranges: runs of equal
    // abscissa values answer the weighted mean of their fitted values.
    let mut j = 0usize;
    while j < n {
        let j0 = j;
        let mut sy = smo[j] * w[j];
        let mut fbw2 = w[j];
        while j + 1 < n && x[j + 1] <= x[j] {
            j += 1;
            sy += w[j] * smo[j];
            fbw2 += w[j];
        }
        if j > j0 {
            let a = if fbw2 > 0.0 { sy / fbw2 } else { 0.0 };
            for i in j0..=j {
                smo[i] = a;
            }
        }
        j += 1;
    }
}

/// The super smoother entry exactly as the Fortran supsmu arranges —
/// boundary equal-x collapse to the weighted mean, the IQR scale walk,
/// fixed-span single pass, or the three-span cross-validation with the
/// bass tone control and interpolation smooth.
pub fn supsmu(x: &[f64], y: &[f64], w: &[f64], iper: i32, span: f64, alpha: f64) -> Vec<f64> {
    let n = x.len();
    let mut smo = vec![0.0; n];
    if n == 0 {
        return smo;
    }
    // x(n) <= x(1): every fitted value is the weighted mean exactly.
    if x[n - 1] <= x[0] {
        let mut sy = 0.0;
        let mut sw = 0.0;
        for j in 0..n {
            sy += w[j] * y[j];
            sw += w[j];
        }
        let a = if sw > 0.0 { sy / sw } else { 0.0 };
        for v in smo.iter_mut() {
            *v = a;
        }
        return smo;
    }
    // The IQR scale walk exactly as the Fortran arranges — 1-based i = n/4,
    // j = 3i, widening until the gap opens.
    let mut i = (n / 4).max(1) as i64;
    let mut j = 3 * i;
    if j as usize > n {
        j = n as i64;
    }
    let mut scale = x[(j - 1) as usize] - x[(i - 1) as usize];
    loop {
        if scale > 0.0 {
            break;
        }
        if j < n as i64 {
            j += 1;
        }
        if i > 1 {
            i -= 1;
        }
        scale = x[(j - 1) as usize] - x[(i - 1) as usize];
        if j >= n as i64 && i <= 1 && scale <= 0.0 {
            break;
        }
    }
    let vsmlsq = (EPS * scale) * (EPS * scale);
    let mut jper = iper;
    if iper == 2 && (x[0] < 0.0 || x[n - 1] > 1.0) {
        jper = 1;
    }
    if !(1..=2).contains(&jper) {
        jper = 1;
    }
    if span > 0.0 {
        let mut acvr = vec![0.0; n];
        smooth(x, y, w, span, jper, vsmlsq, &mut smo, &mut acvr);
        return smo;
    }
    // Cross-validation over the three spans exactly as the Fortran arranges:
    // sc columns pair the fitted values with their smoothed CV residuals.
    let mut sc = vec![vec![0.0f64; 7]; n];
    let mut scratch = vec![0.0f64; n];
    for i in 0..3 {
        let mut col_out: Vec<f64> = vec![0.0; n];
        let mut acvr = vec![0.0; n];
        smooth(x, y, w, SPANS[i], jper, vsmlsq, &mut col_out, &mut acvr);
        for r in 0..n {
            sc[r][2 * i] = col_out[r];
            sc[r][6] = acvr[r];
        }
        // Smooth the CV residuals with the midrange span, negative iper
        // suppressing their own cross-validation exactly as the Fortran.
        smooth(x, &sc.iter().map(|r| r[6]).collect::<Vec<_>>(), w, SPANS[1], -jper, vsmlsq, &mut scratch, &mut vec![0.0; n]);
        for r in 0..n {
            sc[r][2 * i + 1] = scratch[r];
        }
    }
    // Span selection with the bass adjustment exactly as the Fortran.
    for r in 0..n {
        let mut resmin = BIG;
        for i in 0..3 {
            if sc[r][2 * i + 1] < resmin {
                resmin = sc[r][2 * i + 1];
                sc[r][6] = SPANS[i];
            }
        }
        if alpha > 0.0 && alpha <= 10.0 && resmin < sc[r][5] && resmin > 0.0 {
            sc[r][6] += (SPANS[2] - sc[r][6]) * SML.max(resmin / sc[r][5]).powf(10.0 - alpha);
        }
    }
    // Smooth the span sequence, clamp into range, interpolate the fitted
    // values between the adjacent span smooths exactly as the Fortran.
    let spans_col: Vec<f64> = sc.iter().map(|r| r[6]).collect();
    smooth(x, &spans_col, w, SPANS[1], -jper, vsmlsq, &mut scratch, &mut vec![0.0; n]);
    let mut interp_input = vec![0.0; n];
    for r in 0..n {
        let mut s2 = scratch[r];
        if s2 <= SPANS[0] {
            s2 = SPANS[0];
        }
        if s2 >= SPANS[2] {
            s2 = SPANS[2];
        }
        let mut f = s2 - SPANS[1];
        let val;
        if f < 0.0 {
            f = -f / (SPANS[1] - SPANS[0]);
            val = (1.0 - f) * sc[r][2] + f * sc[r][0];
        } else {
            f /= SPANS[2] - SPANS[1];
            val = (1.0 - f) * sc[r][2] + f * sc[r][4];
        }
        interp_input[r] = val;
    }
    let mut acvr = vec![0.0; n];
    smooth(x, &interp_input, w, SPANS[0], -jper, vsmlsq, &mut smo, &mut acvr);
    smo
}
