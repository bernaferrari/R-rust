//! Faithful Rust port of GNU R's cpoly — TOMS Algorithm 419, Jenkins and
//! Traub, Comm. ACM 15 (1972) 97-99 — exactly as vendored in
//! r-source/src/main/complex.c. Index arithmetic, constants, and control
//! flow mirror the C translation line for line so numerical behavior —
//! including clustered exact-multiplicity roots — matches GNU bit for bit.

const ETA: f64 = f64::EPSILON;
const ARE: f64 = f64::EPSILON;
const MRE: f64 = 2.0 * std::f64::consts::SQRT_2 * f64::EPSILON;
const INFIN: f64 = f64::MAX;
const SMALNO: f64 = f64::MIN;
const BASE: f64 = 2.0; // FLT_RADIX

fn hypot(a: f64, b: f64) -> f64 {
    a.hypot(b)
}

fn cdivid(ar: f64, ai: f64, br: f64, bi: f64) -> (f64, f64) {
    // complex division c = a/b avoiding overflow, exactly as GNU.
    if br == 0.0 && bi == 0.0 {
        return (f64::INFINITY, f64::INFINITY);
    }
    if br.abs() >= bi.abs() {
        let r = bi / br;
        let d = br + r * bi;
        ((ar + ai * r) / d, (ai - ar * r) / d)
    } else {
        let r = br / bi;
        let d = bi + r * br;
        ((ar * r + ai) / d, (ai * r - ar) / d)
    }
}

fn pow_di(base: f64, mut n: i32) -> f64 {
    // R_pow_di: base raised to a small integer power by repeated squaring.
    let mut x = base;
    let mut result = 1.0;
    if n < 0 {
        n = -n;
        while n > 0 {
            if n & 1 != 0 {
                result *= x;
            }
            x *= x;
            n >>= 1;
        }
        1.0 / result
    } else {
        while n > 0 {
            if n & 1 != 0 {
                result *= x;
            }
            x *= x;
            n >>= 1;
        }
        result
    }
}

struct Cpoly {
    nn: usize,
    pr: Vec<f64>,
    pi: Vec<f64>,
    hr: Vec<f64>,
    hi: Vec<f64>,
    qpr: Vec<f64>,
    qpi: Vec<f64>,
    qhr: Vec<f64>,
    qhi: Vec<f64>,
    shr: Vec<f64>,
    shi: Vec<f64>,
    sr: f64,
    si: f64,
    tr: f64,
    ti: f64,
    pvr: f64,
    pvi: f64,
}

/// evaluates a polynomial p at s by the horner recurrence, placing the
/// partial sums in q and the computed value in v — exactly as GNU.
fn polyev(
    n: usize,
    s_r: f64,
    s_i: f64,
    p_r: &[f64],
    p_i: &[f64],
    q_r: &mut [f64],
    q_i: &mut [f64],
) -> (f64, f64) {
    q_r[0] = p_r[0];
    q_i[0] = p_i[0];
    let mut v_r = q_r[0];
    let mut v_i = q_i[0];
    for i in 1..n {
        let t = v_r * s_r - v_i * s_i + p_r[i];
        v_i = v_r * s_i + v_i * s_r + p_i[i];
        q_i[i] = v_i;
        v_r = t;
        q_r[i] = v_r;
    }
    (v_r, v_i)
}

/// bounds the error in evaluating the polynomial by the horner recurrence.
fn errev(n: usize, qr: &[f64], qi: &[f64], ms: f64, mp: f64, a_re: f64, m_re: f64) -> f64 {
    let mut e = hypot(qr[0], qi[0]) * m_re / (a_re + m_re);
    for i in 0..n {
        e = e * ms + hypot(qr[i], qi[i]);
    }
    e * (a_re + m_re) - mp * m_re
}

/// Computes a lower bound on the moduli of the zeros of a polynomial.
fn cpoly_cauchy(n: usize, pot: &mut [f64], q: &mut [f64]) -> f64 {
    let n1 = n - 1;
    pot[n1] = -pot[n1];

    // compute upper estimate of bound.
    let mut x = ((-pot[n1]).ln() - pot[0].ln()).exp_d(n1);

    // if newton step at the origin is better, use it.
    if pot[n1 - 1] != 0.0 {
        let xm = -pot[n1] / pot[n1 - 1];
        if xm < x {
            x = xm;
        }
    }

    // chop the interval (0,x) until f le 0.
    loop {
        let xm = x * 0.1;
        let mut f = pot[0];
        for i in 1..n {
            f = f * xm + pot[i];
        }
        if f <= 0.0 {
            break;
        }
        x = xm;
    }

    let mut dx = x;

    // Newton iteration until x converges to two decimal places.
    while (dx / x).abs() > 0.005 {
        q[0] = pot[0];
        for i in 1..n {
            q[i] = q[i - 1] * x + pot[i];
        }
        let f = q[n1];
        let mut delf = q[0];
        for i in 1..n1 {
            delf = delf * x + q[i];
        }
        dx = f / delf;
        x -= dx;
    }
    x
}

trait ExpD {
    fn exp_d(self, n1: usize) -> f64;
}
impl ExpD for f64 {
    #[inline]
    fn exp_d(self, n1: usize) -> f64 {
        (self / (n1 as f64)).exp()
    }
}

/// Returns a scale factor to multiply the coefficients of the polynomial.
fn cpoly_scale(n: usize, pot: &[f64], eps: f64, big: f64, small: f64, base: f64) -> f64 {
    let high = big.sqrt();
    let lo = small / eps;
    let mut max_ = 0.0f64;
    let mut min_ = big;
    for i in 0..n {
        let x = pot[i];
        if x > max_ {
            max_ = x;
        }
        if x != 0.0 && x < min_ {
            min_ = x;
        }
    }

    if min_ < lo || max_ > high {
        let x = lo / min_;
        let sc = if x <= 1.0 {
            1.0 / (max_.sqrt() * min_.sqrt())
        } else {
            if big / x > max_ {
                1.0
            } else {
                x
            }
        };
        let ell = (sc.ln() / base.ln() + 0.5) as i32;
        pow_di(base, ell)
    } else {
        1.0
    }
}

impl Cpoly {
    fn new(capacity: usize) -> Self {
        Cpoly {
            nn: 0,
            pr: vec![0.0; capacity],
            pi: vec![0.0; capacity],
            hr: vec![0.0; capacity],
            hi: vec![0.0; capacity],
            qpr: vec![0.0; capacity],
            qpi: vec![0.0; capacity],
            qhr: vec![0.0; capacity],
            qhi: vec![0.0; capacity],
            shr: vec![0.0; capacity],
            shi: vec![0.0; capacity],
            sr: 0.0,
            si: 0.0,
            tr: 0.0,
            ti: 0.0,
            pvr: 0.0,
            pvi: 0.0,
        }
    }

    /// computes t = -p(s)/h(s); h_s_0 true when h(s) is essentially zero.
    fn calct(&mut self) -> bool {
        let n = self.nn - 1;
        // Borrow-splitting: evaluate h(s) reading hr/hi, writing qhr/qhi.
        let (hvr, hvi) = {
            let (hr, hi, qhr, qhi) = (&self.hr, &self.hi, &mut self.qhr, &mut self.qhi);
            polyev(n, self.sr, self.si, hr, hi, qhr, qhi)
        };
        let h_s_0 = hypot(hvr, hvi) <= ARE * 10.0 * hypot(self.hr[n - 1], self.hi[n - 1]);
        if !h_s_0 {
            let (tr, ti) = cdivid(-self.pvr, -self.pvi, hvr, hvi);
            self.tr = tr;
            self.ti = ti;
        } else {
            self.tr = 0.0;
            self.ti = 0.0;
        }
        h_s_0
    }

    /// calculates the next shifted h polynomial.
    fn nexth(&mut self, h_s_0: bool) {
        let n = self.nn - 1;
        if !h_s_0 {
            for j in 1..n {
                let t1 = self.qhr[j - 1];
                let t2 = self.qhi[j - 1];
                self.hr[j] = self.tr * t1 - self.ti * t2 + self.qpr[j];
                self.hi[j] = self.tr * t2 + self.ti * t1 + self.qpi[j];
            }
            self.hr[0] = self.qpr[0];
            self.hi[0] = self.qpi[0];
        } else {
            // if h(s) is zero replace h with qh.
            for j in 1..n {
                self.hr[j] = self.qhr[j - 1];
                self.hi[j] = self.qhi[j - 1];
            }
            self.hr[0] = 0.0;
            self.hi[0] = 0.0;
        }
    }

    /// Computes the derivative polynomial as the initial polynomial and
    /// computes l1 no-shift h polynomials — exactly as GNU.
    fn noshft(&mut self, l1: usize) {
        let n = self.nn - 1;
        let nm1 = n - 1;

        for i in 0..n {
            let xni = (self.nn - i - 1) as f64;
            self.hr[i] = xni * self.pr[i] / n as f64;
            self.hi[i] = xni * self.pi[i] / n as f64;
        }

        for _jj in 1..=l1 {
            if hypot(self.hr[n - 1], self.hi[n - 1]) <= ETA * 10.0 * hypot(self.pr[n - 1], self.pi[n - 1]) {
                // constant term essentially zero — shift h coefficients.
                for i in 1..=nm1 {
                    let j = self.nn - i;
                    self.hr[j - 1] = self.hr[j - 2];
                    self.hi[j - 1] = self.hi[j - 2];
                }
                self.hr[0] = 0.0;
                self.hi[0] = 0.0;
            } else {
                let (tr, ti) = cdivid(-self.pr[self.nn - 1], -self.pi[self.nn - 1], self.hr[n - 1], self.hi[n - 1]);
                self.tr = tr;
                self.ti = ti;
                for i in 1..=nm1 {
                    let j = self.nn - i;
                    let t1 = self.hr[j - 2];
                    let t2 = self.hi[j - 2];
                    self.hr[j - 1] = self.tr * t1 - self.ti * t2 + self.pr[j - 1];
                    self.hi[j - 1] = self.tr * t2 + self.ti * t1 + self.pi[j - 1];
                }
                self.hr[0] = self.pr[0];
                self.hi[0] = self.pi[0];
            }
        }
    }

    /// Computes l2 fixed-shift h polynomials and tests for convergence;
    /// initiates variable-shift iteration, returning the approximate zero
    /// on success — exactly as GNU.
    fn fxshft(&mut self, l2: usize) -> Option<(f64, f64)> {
        let n = self.nn - 1;

        // evaluate p at s.
        let (pvr, pvi) = {
            let (pr, pi, qpr, qpi) = (&self.pr, &self.pi, &mut self.qpr, &mut self.qpi);
            polyev(self.nn, self.sr, self.si, pr, pi, qpr, qpi)
        };
        self.pvr = pvr;
        self.pvi = pvi;

        let mut test = true;
        let mut pasd = false;
        let mut zr = 0.0;
        let mut zi = 0.0;

        // calculate first t = -p(s)/h(s).
        let mut h_s_0 = self.calct();

        // main loop for one second stage step.
        for j in 1..=l2 {
            let otr = self.tr;
            let oti = self.ti;

            // compute next h polynomial and new t.
            self.nexth(h_s_0);
            h_s_0 = self.calct();
            zr = self.sr + self.tr;
            zi = self.si + self.ti;

            // test for convergence unless stage 3 has failed once or this
            // is the last h polynomial.
            if !h_s_0 && test && j != l2 {
                if hypot(self.tr - otr, self.ti - oti) >= hypot(zr, zi) * 0.5 {
                    pasd = false;
                } else if !pasd {
                    pasd = true;
                } else {
                    // weak convergence test passed twice — save current h
                    // polynomial and shift, start stage three.
                    for i in 0..n {
                        self.shr[i] = self.hr[i];
                        self.shi[i] = self.hi[i];
                    }
                    let svsr = self.sr;
                    let svsi = self.si;
                    if let Some(zero) = self.vrshft(10, zr, zi) {
                        return Some(zero);
                    }

                    // iteration failed to converge — turn off testing,
                    // restore h, s, pv and t.
                    test = false;
                    for i in 1..=n {
                        self.hr[i - 1] = self.shr[i - 1];
                        self.hi[i - 1] = self.shi[i - 1];
                    }
                    self.sr = svsr;
                    self.si = svsi;
                    let (pvr, pvi) = {
                        let (pr, pi, qpr, qpi) = (&self.pr, &self.pi, &mut self.qpr, &mut self.qpi);
                        polyev(self.nn, self.sr, self.si, pr, pi, qpr, qpi)
                    };
                    self.pvr = pvr;
                    self.pvi = pvi;
                    h_s_0 = self.calct();
                }
            }
        }

        // attempt an iteration with final h polynomial from second stage.
        self.vrshft(10, zr, zi)
    }

    /// carries out the third stage iteration exactly as GNU.
    fn vrshft(&mut self, l3: usize, zr_in: f64, zi_in: f64) -> Option<(f64, f64)> {
        let mut b = false;
        self.sr = zr_in;
        self.si = zi_in;
        let mut omp = 0.0f64;
        let mut relstp = 0.0f64;

        // main loop for stage three
        for i in 1..=l3 {
            // evaluate p at s and test for convergence.
            let (pvr, pvi) = {
                let (pr, pi, qpr, qpi) = (&self.pr, &self.pi, &mut self.qpr, &mut self.qpi);
                polyev(self.nn, self.sr, self.si, pr, pi, qpr, qpi)
            };
            self.pvr = pvr;
            self.pvi = pvi;

            let mp = hypot(pvr, pvi);
            let ms = hypot(self.sr, self.si);
            if mp <= 20.0 * errev(self.nn, &self.qpr.clone(), &self.qpi.clone(), ms, mp, ETA, MRE) {
                // converged
                return Some((self.sr, self.si));
            }

            if i != 1 {
                if !b && mp >= omp && relstp < 0.05 {
                    // iteration stalled — probably a cluster of zeros; do 5
                    // fixed shift steps into the cluster to force one zero
                    // to dominate.
                    let mut tp = relstp;
                    b = true;
                    if relstp < ETA {
                        tp = ETA;
                    }
                    let r1 = tp.sqrt();
                    let r2 = self.sr * (r1 + 1.0) - self.si * r1;
                    self.si = self.sr * r1 + self.si * (r1 + 1.0);
                    self.sr = r2;
                    let (pvr, pvi) = {
                        let (pr, pi, qpr, qpi) = (&self.pr, &self.pi, &mut self.qpr, &mut self.qpi);
                        polyev(self.nn, self.sr, self.si, pr, pi, qpr, qpi)
                    };
                    self.pvr = pvr;
                    self.pvi = pvi;
                    for _j in 1..=5 {
                        let h_s_0 = self.calct();
                        self.nexth(h_s_0);
                    }
                    omp = INFIN;
                    // fall through to next-iterate calculation below
                    let h_s_0 = self.calct();
                    self.nexth(h_s_0);
                    let h_s_0b = self.calct();
                    if !h_s_0b {
                        relstp = hypot(self.tr, self.ti) / hypot(self.sr, self.si);
                        self.sr += self.tr;
                        self.si += self.ti;
                    }
                    continue;
                } else {
                    // exit if polynomial value increases significantly.
                    if mp * 0.1 > omp {
                        return None;
                    }
                }
            }
            omp = mp;

            // calculate next iterate.
            let h_s_0 = self.calct();
            self.nexth(h_s_0);
            let h_s_0b = self.calct();
            if !h_s_0b {
                relstp = hypot(self.tr, self.ti) / hypot(self.sr, self.si);
                self.sr += self.tr;
                self.si += self.ti;
            }
        }
        None
    }
}

/// cpoly finds the zeros of a complex polynomial — faithful port of GNU's
/// R_cpolyroot exactly as vendored.
///
/// opr/opi: coefficients in order of DECREASING powers (leading first),
/// degree entries plus one. Returns (roots, fail).
pub fn r_cpolyroot(opr: &[f64], opi: &[f64], degree: usize) -> (Vec<(f64, f64)>, bool) {
    let cosr: f64 = -0.06975647374412529990; // cos 94 degrees
    let sinr: f64 = 0.99756405025982424767; // sin 94 degrees
    let mut xx: f64 = std::f64::consts::FRAC_1_SQRT_2;
    let mut yy: f64 = -xx;

    let mut zeror = vec![0.0f64; degree];
    let mut zeroi = vec![0.0f64; degree];
    let mut fail = false;

    let d1 = degree as isize - 1;
    let mut nn = degree;

    // algorithm fails if the leading coefficient is zero.
    if opr[0] == 0.0 && opi[0] == 0.0 {
        return (Vec::new(), true);
    }

    // remove the zeros at the origin if any.
    while opr[nn] == 0.0 && opi[nn] == 0.0 {
        let d_n = (d1 - nn as isize + 1) as usize;
        zeror[d_n] = 0.0;
        zeroi[d_n] = 0.0;
        nn -= 1;
    }
    nn += 1;
    // now nn = number of coefficients = (relevant degree)+1

    if nn == 1 {
        let roots: Vec<(f64, f64)> = zeror.into_iter().zip(zeroi).collect();
        return (roots, false);
    }

    let mut st = Cpoly::new(nn);

    // copy coefficients; shr = |p|
    for i in 0..nn {
        st.pr[i] = opr[i];
        st.pi[i] = opi[i];
        st.shr[i] = hypot(opr[i], opi[i]);
    }

    // scale the polynomial with factor bnd.
    let bnd = cpoly_scale(nn, &st.shr[..nn], ETA, INFIN, SMALNO, BASE);
    if bnd != 1.0 {
        for i in 0..nn {
            st.pr[i] *= bnd;
            st.pi[i] *= bnd;
        }
    }

    st.nn = nn;

    // start the algorithm for one zero
    'outer: while st.nn > 2 {
        // calculate bnd, a lower bound on the modulus of the zeros.
        for i in 0..st.nn {
            st.shr[i] = hypot(st.pr[i], st.pi[i]);
        }
        let bnd = {
            let nn_now = st.nn;
            cpoly_cauchy(nn_now, &mut st.shr[..nn_now], &mut st.shi[..nn_now])
        };

        // outer loop to control 2 major passes with different shift sequences
        let mut converged: Option<(f64, f64)> = None;
        for _i1 in 1..=2 {
            // first stage calculation, no shift
            st.noshft(5);

            // inner loop to select a shift
            for i2 in 1..=9 {
                // shift chosen with modulus bnd, amplitude rotated by 94
                // degrees from the previous shift.
                let xxx = cosr * xx - sinr * yy;
                yy = sinr * xx + cosr * yy;
                xx = xxx;
                st.sr = bnd * xx;
                st.si = bnd * yy;

                // second stage calculation, fixed shift
                if let Some(zero) = st.fxshft(i2 * 10) {
                    converged = Some(zero);
                    break;
                }
            }
            if converged.is_some() {
                break;
            }
        }

        let (zr, zi) = match converged {
            Some(z) => z,
            None => {
                // the zerofinder has failed on two major passes.
                fail = true;
                break 'outer;
            }
        };

        // the second stage jumps directly to the third stage iteration;
        // if successful the zero is stored and the polynomial deflated.
        let d_n = (d1 + 2 - st.nn as isize) as usize;
        zeror[d_n] = zr;
        zeroi[d_n] = zi;
        st.nn -= 1;
        for i in 0..st.nn {
            st.pr[i] = st.qpr[i];
            st.pi[i] = st.qpi[i];
        }
    }

    if !fail {
        // calculate the final zero and return.
        let (cr, ci) = cdivid(-st.pr[1], -st.pi[1], st.pr[0], st.pi[0]);
        zeror[d1 as usize] = cr;
        zeroi[d1 as usize] = ci;
    }

    let roots: Vec<(f64, f64)> = zeror.into_iter().zip(zeroi).collect();
    (roots, fail)
}
