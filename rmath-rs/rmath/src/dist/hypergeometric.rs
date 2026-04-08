// Hypergeometric distribution: dhyper, phyper, qhyper, rhyper
// Ported from dhyper.c, phyper.c, qhyper.c, rhyper.c
// dhyper originally by Catherine Loader, catherine@research.bell-labs.com, October 23, 2000
// phyper originally by Ross Ihaka, Copyright (C) 1998
// qhyper originally by Ross Ihaka, Copyright (C) 1998
// rhyper originally by Ross Ihaka, Copyright (C) 1998
//   Reference: V. Kachitvichyanukul and B. Schmeiser (1985).
//     "Computer generation of hypergeometric random variates,"
//     Journal of Statistical Computation and Simulation 22, 127-145.

use crate::constants::*;
use crate::dist::binomial::dbinom_raw;
use crate::dpq::*;
use crate::error::*;
use crate::rng::*;
use crate::special::gamma::lgammafn;
use crate::utils::*;
use libm::*;

const DBL_EPSILON: f64 = 2.220446049250313e-16;

// ---- lfastchoose ----
// log of binomial coefficient: log(choose(n, k)) = log(n! / (k! (n-k)!))
// = -log(n+1) - lbeta(n-k+1, k+1)
// Since we don't have lbeta yet, use lgammafn:
fn lfastchoose(n: f64, k: f64) -> f64 {
    lgammafn(n + 1.0) - lgammafn(k + 1.0) - lgammafn(n - k + 1.0)
}

// ---- dhyper ----

#[must_use]
pub fn dhyper_inner(x: f64, r: f64, b: f64, n: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(r) || isnan(b) || isnan(n) {
        return x + r + b + n;
    }

    if (r < 0.0 || r_nonint(r)) || (b < 0.0 || r_nonint(b)) || (n < 0.0 || r_nonint(n)) || n > r + b
    {
        return ml_warn_return_nan();
    }
    if x < 0.0 {
        return r_d__0(give_log);
    }
    // R_D_nonint_check(x):
    if r_nonint(x) {
        ml_warning(ME_DOMAIN, "");
    }

    let x = r_forceint(x);
    let r = r_forceint(r);
    let b = r_forceint(b);
    let n = r_forceint(n);

    if n < x || r < x || n - x > b {
        return r_d__0(give_log);
    }
    if n == 0.0 {
        return if x == 0.0 {
            r_d__1(give_log)
        } else {
            r_d__0(give_log)
        };
    }

    let p = n / (r + b);
    let q = (r + b - n) / (r + b);

    let p1 = dbinom_raw(x, r, p, q, give_log);
    let p2 = dbinom_raw(n - x, b, p, q, give_log);
    let p3 = dbinom_raw(n, r + b, p, q, give_log);

    if give_log { p1 + p2 - p3 } else { p1 * p2 / p3 }
}

// ---- phyper ----

fn pdhyper(mut x: f64, nr: f64, nb: f64, n: f64, log_p: bool) -> f64 {
    // Calculate phyper(x, NR, NB, n, TRUE, FALSE) / dhyper(x, NR, NB, n, FALSE)
    // Assumes x * (NR + NB) <= n * NR
    let mut sum: f64 = 0.0;
    let mut term: f64 = 1.0;

    while x > 0.0 && term >= DBL_EPSILON * sum {
        term *= x * (nb - n + x) / (n + 1.0 - x) / (nr + 1.0 - x);
        sum += term;
        x -= 1.0;
    }

    let ss = sum;
    if log_p { log1p(ss) } else { 1.0 + ss }
}

#[must_use]
pub fn phyper_inner(x: f64, nr: f64, nb: f64, n: f64, lower_tail: bool, log_p: bool) -> f64 {
    // Sample of n balls from NR red and NB black ones; x are red

    // IEEE_754
    if isnan(x) || isnan(nr) || isnan(nb) || isnan(n) {
        return x + nr + nb + n;
    }

    let mut x = floor(x + 1e-7);
    let mut nr = r_forceint(nr);
    let mut nb = r_forceint(nb);
    let n = r_forceint(n);

    if nr < 0.0 || nb < 0.0 || !r_finite(nr + nb) || n < 0.0 || n > nr + nb {
        return ml_warn_return_nan();
    }

    if x * (nr + nb) > n * nr {
        // Swap tails
        let old_nb = nb;
        nb = nr;
        nr = old_nb;
        x = n - x - 1.0;
        // lower_tail = !lower_tail; -- handled below
        return phyper_inner(x, nr, nb, n, !lower_tail, log_p);
    }

    if x < 0.0 || x < n - nb {
        return r_dt_0(lower_tail, log_p);
    }
    if x >= nr || x >= n {
        return r_dt_1(lower_tail, log_p);
    }

    let d = dhyper_inner(x, nr, nb, n, log_p);

    // dhyper(.., log_p=FALSE) > 0 mathematically, but not always numerically
    if (!log_p && d == 0.0) || (log_p && d == ML_NEGINF) {
        return r_dt_0(lower_tail, log_p);
    }

    let pd = pdhyper(x, nr, nb, n, log_p);

    if log_p {
        r_dt_log_known(d + pd, lower_tail)
    } else {
        r_d_lval(d * pd, lower_tail)
    }
}

// ---- qhyper ----

#[must_use]
pub fn qhyper_inner(p: f64, nr: f64, nb: f64, n: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(p) || isnan(nr) || isnan(nb) || isnan(n) {
        return p + nr + nb + n;
    }
    if !r_finite(p) || !r_finite(nr) || !r_finite(nb) || !r_finite(n) {
        return ml_warn_return_nan();
    }

    let nr = r_forceint(nr);
    let nb = r_forceint(nb);
    let n = r_forceint(n);
    let big_n = nr + nb;

    if nr < 0.0 || nb < 0.0 || n < 0.0 || n > big_n {
        return ml_warn_return_nan();
    }

    let xstart = fmax2(0.0, n - nb);
    let xend = fmin2(n, nr);

    // R_Q_P01_boundaries(p, xstart, xend)
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { xend } else { xstart };
        }
        if p == ML_NEGINF {
            return if lower_tail { xstart } else { xend };
        }
    } else {
        if p < 0.0 || p > 1.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { xstart } else { xend };
        }
        if p == 1.0 {
            return if lower_tail { xend } else { xstart };
        }
    }

    let mut xr = xstart;
    let mut xb = n - xr; // #{black balls in sample}

    let small_n = big_n < 1000.0;
    let mut term = lfastchoose(nr, xr) + lfastchoose(nb, xb) - lfastchoose(big_n, n);
    if small_n {
        term = exp(term);
    }
    let mut nr_rem = nr - xr;
    let mut nb_rem = nb - xb;

    let mut p_val = p;
    if !lower_tail || log_p {
        p_val = r_dt_qiv(p, lower_tail, log_p);
    }
    p_val *= 1.0 - 1000.0 * DBL_EPSILON;

    let mut sum = if small_n { term } else { exp(term) };

    while sum < p_val && xr < xend {
        xr += 1.0;
        nb_rem += 1.0;
        if small_n {
            term *= (nr_rem / xr) * (xb / nb_rem);
        } else {
            term += log((nr_rem / xr) * (xb / nb_rem));
        }
        sum += if small_n { term } else { exp(term) };
        xb -= 1.0;
        nr_rem -= 1.0;
    }

    xr
}

// ---- rhyper ----

fn afc(i: i32) -> f64 {
    const AL: [f64; 8] = [
        0.0,                                // ln(0!) = ln(1)
        0.0,                                // ln(1!) = ln(1)
        0.69314718055994530941723212145817, // ln(2)
        1.79175946922805500081247735838070, // ln(6)
        3.17805383034794561964694160129705, // ln(24)
        4.78749174278204599424770093452324,
        6.57925121201010099506017829290394,
        8.52516136106541430016553103634712,
    ];
    const M_LN_SQRT_2PI: f64 = 0.918938533204672741780329736406;

    if i < 0 {
        return -1.0; // should not happen
    }
    if i <= 7 {
        return AL[i as usize];
    }
    // i >= 8: Stirling's approximation
    let di = i as f64;
    let i2 = di * di;
    (di + 0.5) * log(di) - di + M_LN_SQRT_2PI + (0.0833333333333333 - 0.00277777777777778 / i2) / di
}

use std::cell::RefCell;

struct RhyperState {
    ks: i32,
    n1s: i32,
    n2s: i32,
    m: i32,
    minjx: i32,
    maxjx: i32,
    k: i32,
    n1: i32,
    n2: i32,
    big_n: f64,
    // HIN algorithm state
    w: f64,
    // H2PE algorithm state
    a: f64,
    xl: f64,
    xr: f64,
    lamdl: f64,
    lamdr: f64,
    p1: f64,
    p2: f64,
    p3: f64,
}

impl RhyperState {
    fn new() -> Self {
        RhyperState {
            ks: -1,
            n1s: -1,
            n2s: -1,
            m: 0,
            minjx: 0,
            maxjx: 0,
            k: 0,
            n1: 0,
            n2: 0,
            big_n: 0.0,
            w: 0.0,
            a: 0.0,
            xl: 0.0,
            xr: 0.0,
            lamdl: 0.0,
            lamdr: 0.0,
            p1: 0.0,
            p2: 0.0,
            p3: 0.0,
        }
    }
}

thread_local!(static RHYPER_STATE: RefCell<RhyperState> = RefCell::new(RhyperState::new()));

#[must_use]
pub fn rhyper_inner(nn1in: f64, nn2in: f64, kkin: f64) -> f64 {
    let int_max = i32::MAX as f64;

    if !r_finite(nn1in) || !r_finite(nn2in) || !r_finite(kkin) {
        return ml_warn_return_nan();
    }

    let nn1in = r_forceint(nn1in);
    let nn2in = r_forceint(nn2in);
    let kkin = r_forceint(kkin);

    if nn1in < 0.0 || nn2in < 0.0 || kkin < 0.0 || kkin > nn1in + nn2in {
        return ml_warn_return_nan();
    }

    if nn1in >= int_max || nn2in >= int_max || kkin >= int_max {
        // large n -- evade integer overflow
        if kkin == 1.0 {
            return crate::dist::binomial::rbinom_inner(kkin, nn1in / (nn1in + nn2in));
        }
        return qhyper_inner(unif_rand(), nn1in, nn2in, kkin, false, false);
    }

    let nn1 = nn1in as i32;
    let nn2 = nn2in as i32;
    let kk = kkin as i32;

    RHYPER_STATE.with(|state| {
        let mut st = state.borrow_mut();

        // Setup based on parameter changes
        let setup1 = nn1 != st.n1s || nn2 != st.n2s;
        let setup2 = kk != st.ks;

        if setup1 {
            st.n1s = nn1;
            st.n2s = nn2;
            st.big_n = nn1 as f64 + nn2 as f64;
            if nn1 <= nn2 {
                st.n1 = nn1;
                st.n2 = nn2;
            } else {
                st.n1 = nn2;
                st.n2 = nn1;
            }
        }

        if setup2 {
            st.ks = kk;
            if (kk as f64) + (kk as f64) >= st.big_n {
                st.k = (st.big_n - kk as f64) as i32;
            } else {
                st.k = kk;
            }
        }

        if setup1 || setup2 {
            st.m = (((st.k + 1) as f64) * ((st.n1 + 1) as f64) / (st.big_n + 2.0)) as i32;
            st.minjx = imax2(0, st.k - st.n2);
            st.maxjx = imin2(st.n1, st.k);
        }

        let mut ix: i32;

        if st.minjx == st.maxjx {
            // I: degenerate distribution
            ix = st.maxjx;
        } else if st.m - st.minjx < 10 {
            // II: (Scaled) algorithm HIN (inverse transformation)
            if setup1 || setup2 {
                let lw = if st.k < st.n2 {
                    afc(st.n2) + afc(st.n1 + st.n2 - st.k) - afc(st.n2 - st.k) - afc(st.n1 + st.n2)
                } else {
                    afc(st.n1) + afc(st.k) - afc(st.k - st.n2) - afc(st.n1 + st.n2)
                };
                st.w = exp(lw + 57.5646273248511421);
            }

            if st.w <= 0.0 {
                ml_warning(ME_UNDERFLOW, "");
            }

            loop {
                let mut p = st.w;
                ix = st.minjx;
                let mut u = unif_rand() * 1e25;
                while u > p {
                    u -= p;
                    p *= ((st.n1 - ix) as f64) * ((st.k - ix) as f64);
                    ix += 1;
                    p = p / (ix as f64) / ((st.n2 - st.k + ix) as f64);
                    if ix > st.maxjx {
                        break; // restart
                    }
                }
                if ix <= st.maxjx {
                    break;
                }
            }
        } else {
            // III: H2PE Algorithm
            if setup1 || setup2 {
                let s = sqrt(
                    ((st.big_n - st.k as f64) * st.k as f64 * st.n1 as f64 * st.n2 as f64)
                        / (st.big_n - 1.0)
                        / st.big_n
                        / st.big_n,
                );
                let d = (1.5 * s) as i32 as f64 + 0.5;
                st.xl = st.m as f64 - d + 0.5;
                st.xr = st.m as f64 + d + 0.5;
                st.a = afc(st.m) + afc(st.n1 - st.m) + afc(st.k - st.m) + afc(st.n2 - st.k + st.m);

                let n1f = st.n1 as f64;
                let n2f = st.n2 as f64;
                let kf = st.k as f64;
                let kl = exp(st.a
                    - afc(st.xl as i32)
                    - afc((n1f - st.xl) as i32)
                    - afc((kf - st.xl) as i32)
                    - afc((n2f - kf + st.xl) as i32));
                let kr = exp(st.a
                    - afc((st.xr - 1.0) as i32)
                    - afc((n1f - (st.xr - 1.0)) as i32)
                    - afc((kf - (st.xr - 1.0)) as i32)
                    - afc((n2f - kf + (st.xr - 1.0)) as i32));
                st.lamdl =
                    -log(st.xl * (n2f - kf + st.xl) / (n1f - st.xl + 1.0) / (kf - st.xl + 1.0));
                st.lamdr =
                    -log((n1f - st.xr + 1.0) * (kf - st.xr + 1.0) / st.xr / (n2f - kf + st.xr));
                st.p1 = d + d;
                st.p2 = st.p1 + kl / st.lamdl;
                st.p3 = st.p2 + kr / st.lamdr;
            }

            // acceptance/rejection test
            let mut n_uv = 0;
            loop {
                let u = unif_rand() * st.p3;
                let mut v = unif_rand();
                n_uv += 1;
                if n_uv >= 10000 {
                    return ml_warn_return_nan();
                }

                if u < st.p1 {
                    // rectangular region
                    ix = (st.xl + u) as i32;
                } else if u <= st.p2 {
                    // left tail
                    ix = (st.xl + log(v) / st.lamdl) as i32;
                    if ix < st.minjx {
                        continue;
                    }
                    v = v * (u - st.p1) * st.lamdl;
                } else {
                    // right tail
                    ix = (st.xr - log(v) / st.lamdr) as i32;
                    if ix > st.maxjx {
                        continue;
                    }
                    v = v * (u - st.p2) * st.lamdr;
                }

                let reject;
                if st.m < 100 || ix <= 50 {
                    // explicit evaluation
                    let mut f = 1.0;
                    if st.m < ix {
                        for i in (st.m + 1)..=ix {
                            f = f * ((st.n1 - i + 1) as f64) * ((st.k - i + 1) as f64)
                                / ((st.n2 - st.k + i) as f64)
                                / (i as f64);
                        }
                    } else if st.m > ix {
                        for i in (ix + 1)..=st.m {
                            f = f * (i as f64) * ((st.n2 - st.k + i) as f64)
                                / ((st.n1 - i + 1) as f64)
                                / ((st.k - i + 1) as f64);
                        }
                    }
                    reject = v > f;
                } else {
                    let deltal = 0.0078;
                    let deltau = 0.0034;

                    let y = ix as f64;
                    let y1 = y + 1.0;
                    let ym = y - st.m as f64;
                    let yn = (st.n1 - ix) as f64 + 1.0;
                    let yk = (st.k - ix) as f64 + 1.0;
                    let nk = (st.n2 - st.k + ix) as f64 + 1.0;
                    let r = -ym / y1;
                    let s = ym / yn;
                    let t = ym / yk;
                    let e = -ym / nk;
                    let g = yn * yk / (y1 * nk) - 1.0;
                    let dg = if g < 0.0 { 1.0 + g } else { 1.0 };
                    let gu = g * (1.0 + g * (-0.5 + g / 3.0));
                    let gl = gu - 0.25 * (g * g * g * g) / dg;
                    let xm = st.m as f64 + 0.5;
                    let xn = (st.n1 - st.m) as f64 + 0.5;
                    let xk = (st.k - st.m) as f64 + 0.5;
                    let nm = (st.n2 - st.k) as f64 + xm;
                    let ub = y * gu - (st.m as f64) * gl
                        + deltau
                        + xm * r * (1.0 + r * (-0.5 + r / 3.0))
                        + xn * s * (1.0 + s * (-0.5 + s / 3.0))
                        + xk * t * (1.0 + t * (-0.5 + t / 3.0))
                        + nm * e * (1.0 + e * (-0.5 + e / 3.0));

                    let alv = log(v);
                    if alv > ub {
                        reject = true;
                    } else {
                        let mut dr = xm * (r * r * r * r);
                        if r < 0.0 {
                            dr /= 1.0 + r;
                        }
                        let mut ds = xn * (s * s * s * s);
                        if s < 0.0 {
                            ds /= 1.0 + s;
                        }
                        let mut dt = xk * (t * t * t * t);
                        if t < 0.0 {
                            dt /= 1.0 + t;
                        }
                        let mut de = nm * (e * e * e * e);
                        if e < 0.0 {
                            de /= 1.0 + e;
                        }
                        if alv
                            < ub - 0.25 * (dr + ds + dt + de) + (y + st.m as f64) * (gl - gu)
                                - deltal
                        {
                            reject = false;
                        } else {
                            // Stirling's formula to machine accuracy
                            let stirling_val = st.a
                                - afc(ix)
                                - afc(st.n1 - ix)
                                - afc(st.k - ix)
                                - afc(st.n2 - st.k + ix);
                            reject = alv > stirling_val;
                        }
                    }
                }

                if !reject {
                    break;
                }
            }
        }

        // L_finis: return appropriate variate
        if (kk as f64) + (kk as f64) >= st.big_n {
            if nn1 > nn2 {
                ix += kk - nn2;
            } else {
                ix = nn1 - ix;
            }
        } else if nn1 > nn2 {
            ix = kk - ix;
        }

        ix as f64
    })
}

// ---- FFI shims ----

#[must_use]
pub extern "C" fn Rf_dhyper(x: f64, r: f64, b: f64, n: f64, give_log: i32) -> f64 {
    dhyper_inner(x, r, b, n, give_log != 0)
}

#[must_use]
pub extern "C" fn dhyper(x: f64, r: f64, b: f64, n: f64, give_log: i32) -> f64 {
    dhyper_inner(x, r, b, n, give_log != 0)
}

#[must_use]
pub extern "C" fn Rf_phyper(x: f64, nr: f64, nb: f64, n: f64, lower_tail: i32, log_p: i32) -> f64 {
    phyper_inner(x, nr, nb, n, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn phyper(x: f64, nr: f64, nb: f64, n: f64, lower_tail: i32, log_p: i32) -> f64 {
    phyper_inner(x, nr, nb, n, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn Rf_qhyper(p: f64, nr: f64, nb: f64, n: f64, lower_tail: i32, log_p: i32) -> f64 {
    qhyper_inner(p, nr, nb, n, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn qhyper(p: f64, nr: f64, nb: f64, n: f64, lower_tail: i32, log_p: i32) -> f64 {
    qhyper_inner(p, nr, nb, n, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn Rf_rhyper(nn1: f64, nn2: f64, kk: f64) -> f64 {
    rhyper_inner(nn1, nn2, kk)
}

#[must_use]
pub extern "C" fn rhyper(nn1: f64, nn2: f64, kk: f64) -> f64 {
    rhyper_inner(nn1, nn2, kk)
}
