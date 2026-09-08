// Beta distribution: dbeta, pbeta, qbeta, rbeta
// Ported from dbeta.c, pbeta.c, qbeta.c, rbeta.c
//
// dbeta.c author: Catherine Loader
// pbeta.c: wrapper for TOMS708 Algorithm
// qbeta.c: Cran, Martin, Thomas (AS 109) with R improvements
// rbeta.c: Cheng (1978), Algorithms BB and BC

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::rng::*;
use crate::state::with_required_current_instance;
use crate::utils::{fmax2, fmin2};
use libm::{exp, expm1, fabs, log, log1p, pow, sqrt, trunc};

const M_LN_2PI: f64 = 1.837877066409345483560659472811;

// =====================================================================
// dbeta
// =====================================================================

/// pow1p: Compute (1+x)^y accurately also for |x| << 1
/// Ported from dbinom.c
fn pow1p(x: f64, y: f64) -> f64 {
    if isnan(y) {
        return if x == 0.0 { 1.0 } else { y };
    }
    if 0.0 <= y && y == trunc(y) && y <= 4.0 {
        match y as i32 {
            0 => return 1.0,
            1 => return x + 1.0,
            2 => return x * (x + 2.0) + 1.0,
            3 => return x * (x * (x + 3.0) + 3.0) + 1.0,
            4 => return x * (x * (x * (x + 4.0) + 6.0) + 4.0) + 1.0,
            _ => {}
        }
    }
    // naive algorithm in two cases: (1) when 1+x is exact,
    // and (2) when |x| > 1/2
    let xp1 = x + 1.0;
    let x_ = xp1 - 1.0;
    if x_ == x || fabs(x) > 0.5 || isnan(x) {
        pow(xp1, y)
    } else {
        exp(y * log1p(x))
    }
}

/// dbinom_raw: raw binomial probability
/// Ported from dbinom.c -- needed by dbeta
pub(crate) fn dbinom_raw(x: f64, n: f64, p: f64, q: f64, give_log: bool) -> f64 {
    if p == 0.0 {
        return if x == 0.0 {
            r_d__1(give_log)
        } else {
            r_d__0(give_log)
        };
    }
    if q == 0.0 {
        return if x == n {
            r_d__1(give_log)
        } else {
            r_d__0(give_log)
        };
    }

    // NB: The smaller of p and q is the most accurate
    if x == 0.0 {
        if n == 0.0 {
            return r_d__1(give_log);
        }
        if p > q {
            return if give_log { n * log(q) } else { pow(q, n) };
        } else {
            return if give_log {
                n * log1p(-p)
            } else {
                pow1p(-p, n)
            };
        }
    }
    if x == n {
        if p > q {
            return if give_log {
                n * log1p(-q)
            } else {
                pow1p(-q, n)
            };
        } else {
            return if give_log { n * log(p) } else { pow(p, n) };
        }
    }
    if x < 0.0 || x > n {
        return r_d__0(give_log);
    }

    if !r_finite(n) {
        if r_finite(x) {
            return r_d__0(give_log);
        }
        // else n = DBL_MAX
    }

    // We need bd0 and stirlerr which are in special::bd0 and special::stirlerr
    use crate::special::bd0::bd0;
    use crate::special::stirlerr::stirlerr;

    let n_eff = if !r_finite(n) { DBL_MAX as f64 } else { n };

    let lc = stirlerr(n_eff)
        - stirlerr(x)
        - stirlerr(n_eff - x)
        - bd0(x, n_eff * p)
        - bd0(n_eff - x, n_eff * q);

    // lf = log(M_2PI) + log(x) + log1p(- x/n)
    let lf = M_LN_2PI + log(x) + log1p(-x / n_eff);

    r_d_exp(lc - 0.5 * lf, give_log)
}

/// lbeta: log of the beta function
/// Uses lgammafn from special::gamma
fn lbeta_fn(a: f64, b: f64) -> f64 {
    use crate::special::gamma::lgammafn;
    lgammafn(a) + lgammafn(b) - lgammafn(a + b)
}

#[must_use]
pub fn dbeta_inner(x: f64, a: f64, b: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(a) || isnan(b) {
        return x + a + b;
    }

    if a < 0.0 || b < 0.0 {
        return ml_warn_return_nan();
    }
    if x < 0.0 || x > 1.0 {
        return r_d__0(give_log);
    }

    // limit cases for (a,b), leading to point masses
    if a == 0.0 || b == 0.0 || !r_finite(a) || !r_finite(b) {
        if a == 0.0 && b == 0.0 {
            // point mass 1/2 at each of {0,1}
            if x == 0.0 || x == 1.0 {
                return ML_POSINF;
            } else {
                return r_d__0(give_log);
            }
        }
        if a == 0.0 || a / b == 0.0 {
            // point mass 1 at 0
            if x == 0.0 {
                return ML_POSINF;
            } else {
                return r_d__0(give_log);
            }
        }
        if b == 0.0 || b / a == 0.0 {
            // point mass 1 at 1
            if x == 1.0 {
                return ML_POSINF;
            } else {
                return r_d__0(give_log);
            }
        }
        // else, remaining case: a = b = Inf : point mass 1 at 1/2
        if x == 0.5 {
            return ML_POSINF;
        } else {
            return r_d__0(give_log);
        }
    }

    if x == 0.0 {
        if a > 1.0 {
            return r_d__0(give_log);
        }
        if a < 1.0 {
            return ML_POSINF;
        }
        // a == 1
        return r_d_val(b, give_log);
    }
    if x == 1.0 {
        if b > 1.0 {
            return r_d__0(give_log);
        }
        if b < 1.0 {
            return ML_POSINF;
        }
        // b == 1
        return r_d_val(a, give_log);
    }

    let lval = if a <= 2.0 || b <= 2.0 {
        (a - 1.0) * log(x) + (b - 1.0) * log1p(-x) - lbeta_fn(a, b)
    } else {
        log(a + b - 1.0) + dbinom_raw(a - 1.0, a + b - 2.0, x, 1.0 - x, true)
    };

    r_d_exp(lval, give_log)
}

// =====================================================================
// pbeta
// =====================================================================

/// pbeta_raw: incomplete beta ratio I_x(a,b) via TOMS 708 `bratio`
/// (matches R's `pbeta.c` → `toms708.c`).
fn pbeta_raw(x: f64, a: f64, b: f64, lower_tail: bool, log_p: bool) -> f64 {
    if x >= 1.0 {
        return r_dt_1(lower_tail, log_p);
    }
    // treat limit cases correctly here (0 <= x < 1):
    if a == 0.0 || b == 0.0 || !r_finite(a) || !r_finite(b) {
        if a == 0.0 && b == 0.0 {
            return if log_p { -M_LN2 } else { 0.5 };
        }
        if a == 0.0 || a / b == 0.0 {
            return r_dt_1(lower_tail, log_p);
        }
        if b == 0.0 || b / a == 0.0 {
            return r_dt_0(lower_tail, log_p);
        }
        // a = b = Inf : point mass 1 at 1/2
        if x < 0.5 {
            return r_dt_0(lower_tail, log_p);
        } else {
            return r_dt_1(lower_tail, log_p);
        }
    }
    if x <= 0.0 {
        return r_dt_0(lower_tail, log_p);
    }

    // Now: 0 < a < Inf; 0 < b < Inf and 0 < x < 1
    // Accurate complement: x1 = 0.5 - x + 0.5 (R's pbeta.c)
    let x1 = 0.5 - x + 0.5;
    let (w, wc, ierr) = crate::special::toms708::bratio(a, b, x, x1, log_p);
    // ierr in {10,14} <==> bgrat() error codes; R only warns for other codes.
    let _ = ierr;
    if lower_tail { w } else { wc }
}

/// Incomplete beta ratio I_x(a,b) (and complement via `lower_tail` / `log_p`).
#[must_use]
pub fn pbeta_inner(x: f64, a: f64, b: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(a) || isnan(b) {
        return x + a + b;
    }

    if a < 0.0 || b < 0.0 {
        return ml_warn_return_nan();
    }
    // allowing a==0 and b==0 <==> treat as one- or two-point mass

    pbeta_raw(x, a, b, lower_tail, log_p)
}

// =====================================================================
// qbeta
// =====================================================================

const USE_LOG_X_CUTOFF: f64 = -5.0;
const N_NEWTON_FREE: i32 = 4;

const DBL_VERY_MIN: f64 = DBL_MIN / 4.0;
const DBL_LOG_V_MIN: f64 = M_LN2 * ((DBL_MIN_EXP - 2) as f64);
const DBL_1__EPS: f64 = 1.0 - DBL_EPSILON / 2.0; // 0x1.fffffffffffffp-1

const FPU: f64 = 3e-308;
const ACU_MIN: f64 = 1e-300;
const P_LO: f64 = FPU;
const P_HI: f64 = 1.0 - 2.22e-16;

const CONST1: f64 = 2.30753;
const CONST2: f64 = 0.27061;
const CONST3: f64 = 0.99229;
const CONST4: f64 = 0.04481;

const LOG_EPS_C: f64 = M_LN2 * (1.0 - DBL_MANT_DIG as f64); // = log(DBL_EPSILON)

fn qbeta_raw(
    alpha: f64,
    p: f64,
    q: f64,
    lower_tail: bool,
    log_p: bool,
    log_q_cut: f64,
    n_n: i32,
) -> [f64; 2] {
    // qb[0:1] = { qbeta(), 1 - qbeta() }
    let mut qb = [ML_NAN, ML_NAN];

    let give_log_q = log_q_cut == ML_POSINF;
    let mut use_log_x = give_log_q;
    let mut add_n_step = true;

    // Deal with boundary cases here:
    if alpha == r_dt_0(lower_tail, log_p) {
        if give_log_q {
            qb[0] = ML_NEGINF;
            qb[1] = 0.0;
        } else {
            qb[0] = 0.0;
            qb[1] = 1.0;
        }
        return qb;
    }
    if alpha == r_dt_1(lower_tail, log_p) {
        if give_log_q {
            qb[0] = 0.0;
            qb[1] = ML_NEGINF;
        } else {
            qb[0] = 1.0;
            qb[1] = 0.0;
        }
        return qb;
    }

    // check alpha before transformation
    if (log_p && alpha > 0.0) || (!log_p && (alpha < 0.0 || alpha > 1.0)) {
        ml_warning(ME_DOMAIN, "");
        qb[0] = ML_NAN;
        qb[1] = ML_NAN;
        return qb;
    }

    // p==0, q==0, p = Inf, q = Inf <==> treat as one- or two-point mass
    if p == 0.0 || q == 0.0 || !r_finite(p) || !r_finite(q) {
        if p == 0.0 && q == 0.0 {
            // point mass 1/2 at each of {0,1}
            if alpha < r_d_half(log_p) {
                if give_log_q {
                    qb[0] = ML_NEGINF;
                    qb[1] = 0.0;
                } else {
                    qb[0] = 0.0;
                    qb[1] = 1.0;
                }
                return qb;
            }
            if alpha > r_d_half(log_p) {
                if give_log_q {
                    qb[0] = 0.0;
                    qb[1] = ML_NEGINF;
                } else {
                    qb[0] = 1.0;
                    qb[1] = 0.0;
                }
                return qb;
            }
            // alpha == "1/2"
            if give_log_q {
                qb[0] = -M_LN2;
                qb[1] = -M_LN2;
            } else {
                qb[0] = 0.5;
                qb[1] = 0.5;
            }
            return qb;
        } else if p == 0.0 || p / q == 0.0 {
            if give_log_q {
                qb[0] = ML_NEGINF;
                qb[1] = 0.0;
            } else {
                qb[0] = 0.0;
                qb[1] = 1.0;
            }
            return qb;
        } else if q == 0.0 || q / p == 0.0 {
            if give_log_q {
                qb[0] = 0.0;
                qb[1] = ML_NEGINF;
            } else {
                qb[0] = 1.0;
                qb[1] = 0.0;
            }
            return qb;
        }
        // else: p = q = Inf : point mass 1 at 1/2
        if give_log_q {
            qb[0] = -M_LN2;
            qb[1] = -M_LN2;
        } else {
            qb[0] = 0.5;
            qb[1] = 0.5;
        }
        return qb;
    }

    // initialize
    let p_ = r_dt_qiv(alpha, lower_tail, log_p);
    let logbeta = lbeta_fn(p, q);

    let mut swap_tail = p_ > 0.5;

    let mut n_maybe_swaps: i32 = 0;

    // calculate the initial approximation
    let (mut a, mut la, mut pp, mut qq): (f64, f64, f64, f64);

    'maybe_swap: loop {
        // change tail; afterwards 0 < a <= 1/2
        if swap_tail {
            a = r_dt_civ(alpha, lower_tail, log_p); // = 1 - p_, is < 1/2
            la = r_dt_clog(alpha, lower_tail, log_p);
            pp = q;
            qq = p;
        } else {
            a = p_;
            la = r_dt_log(alpha, lower_tail, log_p);
            pp = p;
            qq = q;
        }
        n_maybe_swaps += 1;

        // Desired accuracy for Newton iterations
        let acu = fmax2(ACU_MIN, pow(10.0, -13.0 - 2.5 / (pp * pp) - 0.5 / (a * a)));

        let u0 = (la + log(pp) + logbeta) / pp; // = log(x_0)
        let mut rp = pp * (1.0 - qq) / (pp + 1.0);

        let t = 0.2;
        let u0_maybe = M_LN2 * (DBL_MIN_EXP as f64) < u0 && u0 < -0.01;

        let mut u_n: f64; // to be log(xinbta) <==> xinbta = exp(u_n)
        let mut xinbta: f64;
        let mut tx: f64;
        let mut u: f64;

        if u0_maybe
            && u0
                < (t * LOG_EPS_C
                    - log(fabs(pp * (1.0 - qq) * (2.0 - qq) / (2.0 * (pp + 2.0)))) / 2.0)
        {
            // MM's one-step correction
            rp = rp * exp(u0);
            u = if rp > -1.0 { u0 - log1p(rp) / pp } else { u0 };
            tx = exp(u);
            xinbta = exp(u);
            use_log_x = true;
            // goto L_Newton equivalent
            u_n = u;
            // Fall through to Newton section below
        } else {
            // y := y_alpha in AS 64
            let r = sqrt(-2.0 * la);
            let y = r - (CONST1 + CONST2 * r) / (1.0 + (CONST3 + CONST4 * r) * r);

            if pp > 1.0 && qq > 1.0 {
                // use Carter(1947), see AS 109, remark '5.'
                let r = (y * y - 3.0) / 6.0;
                let s = 1.0 / (pp + pp - 1.0);
                let t = 1.0 / (qq + qq - 1.0);
                let h = 2.0 / (s + t);
                let w = y * sqrt(h + r) / h - (t - s) * (r + 5.0 / 6.0 - 2.0 / (3.0 * h));
                if w > 300.0 {
                    let t = w + w + log(qq) - log(pp);
                    u = if t <= 18.0 {
                        -log1p(exp(t))
                    } else {
                        -t - exp(-t)
                    };
                    xinbta = exp(u);
                } else {
                    xinbta = pp / (pp + qq * exp(w + w));
                    u = -log1p(qq / pp * exp(w + w));
                }
            } else {
                // use the original AS 64 proposal, Scheffe-Tukey and Wilson-Hilferty
                let r = qq + qq;
                let t = 1.0 / (3.0 * sqrt(qq));
                let t = r * crate::special::mlutils::R_pow_di(1.0 + t * (-t + y), 3);
                let s = 4.0 * pp + r - 2.0;

                if t == 0.0 || (t < 0.0 && s >= t) {
                    let l1ma = if swap_tail {
                        r_dt_log(alpha, lower_tail, log_p)
                    } else {
                        r_dt_clog(alpha, lower_tail, log_p)
                    };

                    let xx = (l1ma + log(qq) + logbeta) / qq;
                    if xx <= 0.0 {
                        xinbta = -expm1(xx);
                        u = r_log1_exp(xx);
                    } else {
                        let r_ = rp * exp(u0);
                        u = if r_ > -1.0 { u0 - log1p(r_) / pp } else { u0 };
                        xinbta = exp(u);
                    }
                } else {
                    let t = s / t;
                    if t <= 1.0 {
                        u = u0;
                        xinbta = exp(u);
                    } else {
                        xinbta = 1.0 - 2.0 / (t + 1.0);
                        u = log1p(-2.0 / (t + 1.0));
                    }
                }
            }

            // Problem: If initial u is completely wrong, we make a wrong decision here
            if (swap_tail && u >= -exp(log_q_cut))
                || (!swap_tail && u >= -exp(4.0 * log_q_cut) && pp / qq < 1000.0)
            {
                swap_tail = !swap_tail;

                if swap_tail {
                    a = r_dt_civ(alpha, lower_tail, log_p);
                    la = r_dt_clog(alpha, lower_tail, log_p);
                    pp = q;
                    qq = p;
                } else {
                    a = p_;
                    la = r_dt_log(alpha, lower_tail, log_p);
                    pp = p;
                    qq = q;
                }
                u = r_log1_exp(u);
                xinbta = exp(u);
            }

            if !use_log_x {
                use_log_x = u < log_q_cut;
            }

            let bad_u = !r_finite(u);
            let bad_init = bad_u || xinbta > P_HI;

            tx = xinbta;

            if bad_u || u < log_q_cut {
                let w = pbeta_raw(DBL_VERY_MIN, pp, qq, true, log_p);
                if w > (if log_p { la } else { a }) {
                    if log_p || fabs(w - a) < fabs(0.0 - a) {
                        tx = DBL_VERY_MIN;
                        u_n = DBL_LOG_V_MIN;
                    } else {
                        tx = 0.0;
                        u_n = ML_NEGINF;
                    }
                    use_log_x = log_p;
                    add_n_step = false;
                    // goto L_return
                    qb = compute_qbeta_return(
                        tx, u_n, swap_tail, give_log_q, use_log_x, add_n_step, pp, qq, a, logbeta,
                        log_p, lower_tail, alpha,
                    );
                    return qb;
                } else {
                    if u < DBL_LOG_V_MIN {
                        u = DBL_LOG_V_MIN;
                        xinbta = DBL_VERY_MIN;
                    }
                }
            }

            // Sometimes the approximation is negative (and == 0 is also not "ok")
            if bad_init && !(use_log_x && tx > 0.0) {
                if u == ML_NEGINF {
                    u = M_LN2 * (DBL_MIN_EXP as f64);
                    xinbta = DBL_MIN;
                } else {
                    xinbta = if xinbta > 1.1 {
                        0.5
                    } else if xinbta < P_LO {
                        exp(u)
                    } else {
                        P_HI
                    };
                    if bad_u {
                        u = log(xinbta);
                    }
                }
            }

            u_n = u;
        }

        // L_Newton: Newton-Raphson
        let r = 1.0 - pp;
        let t_val = 1.0 - qq;
        let mut wprev = 0.0;
        let mut prev = 1.0;
        let mut adj = 1.0;
        let mut warned = false;

        if use_log_x {
            let mut u = u_n;
            let mut xinbta = exp(u);
            let mut converged = false;
            let mut y = 0.0_f64;

            for i_pb in 0..1000 {
                y = pbeta_raw(xinbta, pp, qq, true, true);
                let w = if y == ML_NEGINF {
                    0.0
                } else {
                    (y - la) * exp(y - u + logbeta + r * u + t_val * r_log1_exp(u))
                };

                if !r_finite(w) {
                    if n_maybe_swaps <= 1 {
                        continue 'maybe_swap;
                    }
                    ml_warning(ME_DOMAIN, "");
                    qb[0] = ML_NAN;
                    qb[1] = ML_NAN;
                    return qb;
                }

                if i_pb >= n_n as usize && w * wprev <= 0.0 {
                    prev = fmax2(fabs(adj), FPU);
                }

                let mut g = 1.0;
                for _i_inn in 0..1000 {
                    adj = g * w;
                    if fabs(adj) < prev {
                        u_n = u - adj;
                        if u_n <= 0.0 {
                            if prev <= acu || fabs(w) <= acu {
                                converged = true;
                                break;
                            }
                            break;
                        }
                    }
                    g /= 3.0;
                }

                let d = fmin2(fabs(adj), fabs(u_n - u));
                if d <= 4e-16 * fabs(u_n + u) {
                    converged = true;
                    break;
                }
                u = u_n;
                xinbta = exp(u);
                wprev = w;
            }

            if !converged {
                warned = true;
                ml_warning(ME_PRECISION, "qbeta");
            }

            // L_converged:
            let log_ = log_p || use_log_x;
            if (log_ && y == ML_NEGINF) || (!log_ && y == 0.0) {
                let w = pbeta_raw(DBL_VERY_MIN, pp, qq, true, log_);
                if log_ || fabs(w - a) <= fabs(y - a) {
                    tx = DBL_VERY_MIN;
                    u_n = DBL_LOG_V_MIN;
                }
                add_n_step = false;
            } else if !warned && (log_ && fabs(y - la) > 3.0 || !log_ && fabs(y - a) > 1e-4) {
                // accuracy warning
            }

            qb = compute_qbeta_return(
                tx, u_n, swap_tail, give_log_q, use_log_x, add_n_step, pp, qq, a, logbeta, log_p,
                lower_tail, alpha,
            );
            return qb;
        } else {
            // "normal scale" Newton
            let mut xinbta = if u_n != 1.0 { exp(u_n) } else { xinbta };
            let mut converged = false;
            let mut y = 0.0_f64;

            for i_pb in 0..1000 {
                y = pbeta_raw(xinbta, pp, qq, true, log_p);
                let w = if log_p {
                    (y - la) * exp(y + logbeta + r * log(xinbta) + t_val * log1p(-xinbta))
                } else {
                    (y - a) * exp(logbeta + r * log(xinbta) + t_val * log1p(-xinbta))
                };

                if !r_finite(w) {
                    if n_maybe_swaps <= 2 {
                        if !log_p && n_maybe_swaps == 2 {
                            use_log_x = true;
                        }
                        if !log_p || n_maybe_swaps <= 1 {
                            continue 'maybe_swap;
                        }
                    }
                    ml_warning(ME_DOMAIN, "");
                    qb[0] = ML_NAN;
                    qb[1] = ML_NAN;
                    return qb;
                }

                if i_pb >= n_n as usize && w * wprev <= 0.0 {
                    prev = fmax2(fabs(adj), FPU);
                }

                let mut g = 1.0;
                for _i_inn in 0..1000 {
                    adj = g * w;
                    if i_pb < n_n as usize || fabs(adj) < prev {
                        tx = xinbta - adj;
                        if 0.0 <= tx && tx <= 1.0 {
                            if prev <= acu || fabs(w) <= acu {
                                converged = true;
                                break;
                            }
                            if tx != 0.0 && tx != 1.0 {
                                break;
                            }
                        }
                    }
                    g /= 3.0;
                }

                if fabs(tx - xinbta) <= 4e-16 * (tx + xinbta) {
                    converged = true;
                    break;
                }
                xinbta = tx;
                if tx == 0.0 {
                    break;
                }
                wprev = w;
            }

            if !converged {
                warned = true;
                ml_warning(ME_PRECISION, "qbeta");
            }

            // L_converged:
            let log_ = log_p || use_log_x;
            if (log_ && y == ML_NEGINF) || (!log_ && y == 0.0) {
                let w = pbeta_raw(DBL_VERY_MIN, pp, qq, true, log_);
                if log_ || fabs(w - a) <= fabs(y - a) {
                    tx = DBL_VERY_MIN;
                    u_n = DBL_LOG_V_MIN;
                }
                add_n_step = false;
            } else if !warned && (log_ && fabs(y - la) > 3.0 || !log_ && fabs(y - a) > 1e-4) {
                // accuracy warning
            }

            qb = compute_qbeta_return(
                tx, u_n, swap_tail, give_log_q, use_log_x, add_n_step, pp, qq, a, logbeta, log_p,
                lower_tail, alpha,
            );
            return qb;
        }
    }
}

/// Compute the return value for qbeta based on scale and tail
fn compute_qbeta_return(
    tx: f64,
    u_n: f64,
    swap_tail: bool,
    give_log_q: bool,
    use_log_x: bool,
    add_n_step: bool,
    pp: f64,
    qq: f64,
    a: f64,
    logbeta: f64,
    log_p: bool,
    lower_tail: bool,
    alpha: f64,
) -> [f64; 2] {
    let mut qb = [0.0, 0.0];
    let r = 1.0 - pp;
    let t = 1.0 - qq;

    if give_log_q {
        let r = r_log1_exp(u_n);
        if swap_tail {
            qb[0] = r;
            qb[1] = u_n;
        } else {
            qb[0] = u_n;
            qb[1] = r;
        }
    } else {
        if use_log_x {
            if add_n_step {
                // add one last Newton step on original x scale
                let xinbta = if u_n != 1.0 { exp(u_n) } else { 1.0 };
                let y = pbeta_raw(xinbta, pp, qq, true, log_p);
                let w = if log_p {
                    (y - r_dt_log(alpha, lower_tail, log_p))
                        * exp(y + logbeta + r * log(xinbta) + t * log1p(-xinbta))
                } else {
                    (y - a) * exp(logbeta + r * log(xinbta) + t * log1p(-xinbta))
                };
                if r_finite(w) {
                    let new_tx = xinbta - w;
                    if swap_tail {
                        qb[0] = 1.0 - new_tx;
                        qb[1] = new_tx;
                    } else {
                        qb[0] = new_tx;
                        qb[1] = 1.0 - new_tx;
                    }
                } else {
                    if swap_tail {
                        qb[0] = 1.0 - tx;
                        qb[1] = tx;
                    } else {
                        qb[0] = tx;
                        qb[1] = 1.0 - tx;
                    }
                }
            } else {
                if swap_tail {
                    qb[0] = -expm1(u_n);
                    qb[1] = exp(u_n);
                } else {
                    qb[0] = exp(u_n);
                    qb[1] = -expm1(u_n);
                }
            }
        } else {
            if swap_tail {
                qb[0] = 1.0 - tx;
                qb[1] = tx;
            } else {
                qb[0] = tx;
                qb[1] = 1.0 - tx;
            }
        }
    }

    qb
}

#[must_use]
pub fn qbeta_inner(alpha: f64, p: f64, q: f64, lower_tail: bool, log_p: bool) -> f64 {
    // test for admissibility of parameters
    // IEEE_754
    if isnan(p) || isnan(q) || isnan(alpha) {
        return p + q + alpha;
    }
    if p < 0.0 || q < 0.0 {
        return ml_warn_return_nan();
    }

    let qbet = qbeta_raw(
        alpha,
        p,
        q,
        lower_tail,
        log_p,
        USE_LOG_X_CUTOFF,
        N_NEWTON_FREE,
    );
    qbet[0]
}

// =====================================================================
// rbeta
// =====================================================================

#[derive(Clone, Copy)]
pub struct BetaState {
    olda: f64,
    oldb: f64,
    beta: f64,
    gamma: f64,
    delta: f64,
    k1: f64,
    k2: f64,
}

impl Default for BetaState {
    fn default() -> Self {
        BetaState {
            olda: -1.0,
            oldb: -1.0,
            beta: 0.0,
            gamma: 0.0,
            delta: 0.0,
            k1: 0.0,
            k2: 0.0,
        }
    }
}

fn with_beta_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut BetaState) -> R,
{
    with_required_current_instance(|instance| f(&mut instance.beta_state))
}

#[must_use]
pub fn rbeta_inner(aa: f64, bb: f64) -> f64 {
    const EXPMAX: f64 = (DBL_MAX_EXP as f64) * M_LN2; // = log(DBL_MAX)

    if isnan(aa) || isnan(bb) || aa < 0.0 || bb < 0.0 {
        return ml_warn_return_nan();
    }
    if !r_finite(aa) && !r_finite(bb) {
        return 0.5;
    } // a = b = Inf
    if aa == 0.0 && bb == 0.0 {
        return if unif_rand() < 0.5 { 0.0 } else { 1.0 };
    }
    // now, at least one of a, b is finite and positive
    if !r_finite(aa) || bb == 0.0 {
        return 1.0;
    }
    if !r_finite(bb) || aa == 0.0 {
        return 0.0;
    }

    let (a, b): (f64, f64);
    let alpha: f64;

    let qsame = with_beta_state(|state| {
        let same = (state.olda == aa) && (state.oldb == bb);
        if !same {
            state.olda = aa;
            state.oldb = bb;
        }
        same
    });

    a = fmin2(aa, bb);
    b = fmax2(aa, bb); // a <= b
    alpha = a + b;

    let v_w_from_u1_bet = |u1: f64, aa_val: f64| -> (f64, f64) {
        let beta = with_beta_state(|state| state.beta);
        let v = beta * log(u1 / (1.0 - u1));
        if v <= EXPMAX {
            let w = aa_val * exp(v);
            let w = if !r_finite(w) { DBL_MAX } else { w };
            (v, w)
        } else {
            (v, DBL_MAX)
        }
    };

    if a <= 1.0 {
        // --- Algorithm BC ---
        if !qsame {
            // fma() matches the contraction clang applies to these
            // a*b+-c subexpressions when compiling stock R's rbeta.c.
            with_beta_state(|state| {
                state.beta = 1.0 / a;
                state.delta = 1.0 + b - a;
                state.k1 = state.delta * 0.0416667_f64.mul_add(a, 0.0138889)
                    / b.mul_add(state.beta, -0.777778);
                state.k2 = (0.5 + 0.25 / state.delta).mul_add(a, 0.25);
            });
        }

        let (k1, k2) = with_beta_state(|state| (state.k1, state.k2));

        loop {
            let u1 = unif_rand();
            let u2 = unif_rand();
            let z: f64;
            if u1 < 0.5 {
                let y = u1 * u2;
                z = u1 * y;
                if 0.25 * u2 + z - y >= k1 {
                    continue;
                }
            } else {
                z = u1 * u1 * u2;
                if z <= 0.25 {
                    let (_, w) = v_w_from_u1_bet(u1, b);
                    return if aa == a { a / (a + w) } else { w / (a + w) };
                }
                if z >= k2 {
                    continue;
                }
            }

            let (v, w) = v_w_from_u1_bet(u1, b);
            if alpha.mul_add(log(alpha / (a + w)) + v, -1.3862944) >= log(z) {
                return if aa == a { a / (a + w) } else { w / (a + w) };
            }
        }
    } else {
        // Algorithm BB
        if !qsame {
            with_beta_state(|state| {
                state.beta = sqrt((alpha - 2.0) / (2.0 * a).mul_add(b, -alpha));
                state.gamma = a + 1.0 / state.beta;
            });
        }

        let gamma_v = with_beta_state(|state| state.gamma);

        // C: do { ... } while (r + alpha * log(alpha / (b + w)) < t);
        // repeat while that condition holds, return with the current w
        // once it fails -- no extra unif_rand() on the break paths.
        let mut w;
        loop {
            let u1 = unif_rand();
            let u2 = unif_rand();
            let (v, w_i) = v_w_from_u1_bet(u1, a);
            w = w_i;

            let z = u1 * u1 * u2;
            let r = gamma_v.mul_add(v, -1.3862944);
            let s = a + r - w;
            if s + 2.609438 >= 5.0 * z {
                break;
            }
            let t = log(z);
            if s > t {
                break;
            }
            if !(alpha.mul_add(log(alpha / (b + w)), r) < t) {
                break;
            }
        }

        return if aa != a { b / (b + w) } else { w / (b + w) };
    }
}

// =====================================================================
// FFI shims
// =====================================================================

#[must_use]
pub fn Rf_dbeta(x: f64, a: f64, b: f64, give_log: i32) -> f64 {
    dbeta_inner(x, a, b, give_log != 0)
}

#[must_use]
pub fn dbeta(x: f64, a: f64, b: f64, give_log: i32) -> f64 {
    dbeta_inner(x, a, b, give_log != 0)
}

#[must_use]
pub fn Rf_pbeta(x: f64, a: f64, b: f64, lower_tail: i32, log_p: i32) -> f64 {
    pbeta_inner(x, a, b, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn pbeta(x: f64, a: f64, b: f64, lower_tail: i32, log_p: i32) -> f64 {
    pbeta_inner(x, a, b, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_qbeta(p: f64, a: f64, b: f64, lower_tail: i32, log_p: i32) -> f64 {
    qbeta_inner(p, a, b, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn qbeta(p: f64, a: f64, b: f64, lower_tail: i32, log_p: i32) -> f64 {
    qbeta_inner(p, a, b, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_rbeta(a: f64, b: f64) -> f64 {
    rbeta_inner(a, b)
}

#[must_use]
pub fn rbeta(a: f64, b: f64) -> f64 {
    rbeta_inner(a, b)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    /// (name, aa, bb, scripted uniforms, consumed count, expected draw bits)
    type ConsumptionCase = (&'static str, f64, f64, &'static [f64], usize, u64);
    use crate::rng::set_unif_hook;
    use crate::test_session::TestSession;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    #[test]
    fn rbeta_state_is_session_local_on_same_thread() {
        let mut left = TestSession::new();
        let mut right = TestSession::new();

        left.with_protected(|| {
            let _ = rbeta_inner(2.0, 5.0);
            with_beta_state(|state| {
                assert_eq!(state.olda, 2.0);
                assert_eq!(state.oldb, 5.0);
                assert!(state.beta > 0.0);
            });
        });

        right.with_protected(|| {
            with_beta_state(|state| {
                assert_eq!(state.olda, -1.0);
                assert_eq!(state.oldb, -1.0);
                assert_eq!(state.beta, 0.0);
            });
        });
    }

    /// Golden values captured from stock R 4.6.1 (Rscript, rbeta.c Cheng
    /// BB/BC) driven with the same Marsaglia-MultiCarry stream: after
    /// RNGkind("Marsaglia-Multicarry"), .Random.seed <- c(10401L, i1, i2).
    /// Covers the BC squeeze/k2/quick/full paths, the BB squeeze / s>t /
    /// do-while paths, the a,b ~ 1+eps contracted init, the (0,0) point-mass
    /// path and alternating parameters (qsame re-init).
    ///
    /// Draws are asserted at a 1e-13 relative tolerance rather than
    /// bit-exactly: stock homebrew R compiles rbeta.c with clang's default
    /// FMA contraction (mirrored here via mul_add at the contracted sites),
    /// and Rust's libm exp/log can differ from macOS libm by a final ulp --
    /// the same residual class that affects rnorm/rgamma parity port-wide.
    /// A uniform-consumption divergence produces O(1) different draws, far
    /// outside this tolerance.
    #[test]
    fn rbeta_matches_stock_multicarry_stream() {
        fn assert_close(got: &[f64], want: &[f64]) {
            assert_eq!(got.len(), want.len());
            for (i, (g, w)) in got.iter().zip(want).enumerate() {
                assert!(
                    (g - w).abs() <= 1e-13 * w.abs().max(1e-300),
                    "draw {i}: got {g:.17e}, want {w:.17e}"
                );
            }
        }

        let mut session = TestSession::new();
        session.with_protected(|| {
            let draws = |n, a, b| -> Vec<f64> { (0..n).map(|_| rbeta_inner(a, b)).collect() };

            // --- Algorithm BC (a <= 1) ---
            crate::rng::set_seed(1234, 5678);
            assert_close(
                &draws(10, 0.5, 0.8),
                &[
                    0.64631808969296334,
                    8.8350185132821101e-07,
                    0.10606422460960811,
                    0.6553979068970045,
                    0.42130561400517058,
                    0.7571416186787735,
                    0.036577643477011527,
                    0.21163985128288318,
                    3.3797437316110677e-06,
                    0.52629433336682985,
                ],
            );

            crate::rng::set_seed(1234, 5678);
            assert_close(
                &draws(10, 0.3, 4.0),
                &[
                    0.30957547638272714,
                    1.335388546829043e-11,
                    0.0046808844253926644,
                    0.32398730063569103,
                    0.088188680053452415,
                    0.00070353750668065507,
                    0.018009018409832458,
                    1.2495010713374348e-10,
                    0.16362903596697756,
                    0.27781465493144225,
                ],
            );

            // aa != a: swapped return orientation
            crate::rng::set_seed(1234, 5678);
            assert_close(
                &draws(10, 2.0, 0.5),
                &[
                    0.57771418887069192,
                    0.99999964659907215,
                    0.95469090578900506,
                    0.56793687760031586,
                    0.77446668278526587,
                    0.44502784146973667,
                    0.98504063563128241,
                    0.90303049215531228,
                    0.99999864809976591,
                    0.69232602487299388,
                ],
            );

            // long squeeze / k2 / full-reject chains
            crate::rng::set_seed(99, 424_242);
            assert_close(
                &draws(12, 0.2, 0.7),
                &[
                    5.6833061096527566e-05,
                    0.00094560177296646895,
                    0.43187130864618328,
                    0.98098300463907373,
                    0.030710794107121506,
                    0.82453769730428739,
                    1.5078995313838586e-05,
                    0.0041721871770901871,
                    0.02836979647785003,
                    0.68710275556295086,
                    0.00020764137067298918,
                    0.09751887641958322,
                ],
            );

            crate::rng::set_seed(777, 333);
            assert_close(
                &draws(12, 0.75, 1.0),
                &[
                    0.68899338644560904,
                    0.65683652938523518,
                    0.064702761222018318,
                    0.67057873616464969,
                    0.78030144565066517,
                    0.45062619186948316,
                    0.4753353716258184,
                    0.37634624008382084,
                    0.66969812911064264,
                    0.74453575107931069,
                    0.10261010095475172,
                    0.44229292806177806,
                ],
            );

            // extreme small shapes
            crate::rng::set_seed(5, 5);
            assert_close(
                &draws(8, 0.01, 0.01),
                &[
                    1.0,
                    1.3544983664016456e-126,
                    1.6515443909291071e-06,
                    0.2910454584747354,
                    1.0,
                    1.0,
                    1.0,
                    1.0,
                ],
            );

            // --- Algorithm BB (a > 1) ---
            crate::rng::set_seed(1234, 5678);
            assert_close(
                &draws(10, 2.0, 5.0),
                &[
                    0.094090019232039065,
                    0.22287638999845477,
                    0.22867982605538428,
                    0.22301091566461806,
                    0.40105890551068973,
                    0.22073768725685719,
                    0.27615676621081064,
                    0.19550580505421722,
                    0.48807184544867188,
                    0.34203310658538399,
                ],
            );

            crate::rng::set_seed(1234, 5678);
            assert_close(
                &draws(10, 1.5, 3.7),
                &[
                    0.075571467517154087,
                    0.21451181811913961,
                    0.22122985901594147,
                    0.21466723507172875,
                    0.42775360057781819,
                    0.21204300209025564,
                    0.27706743427988195,
                    0.18322618069301955,
                    0.53209652563348342,
                    0.35624731802253179,
                ],
            );

            crate::rng::set_seed(99, 424_242);
            assert_close(
                &draws(12, 10.0, 10.0),
                &[
                    0.63158320880509089,
                    0.43231241590366881,
                    0.59082414074013356,
                    0.38538363121997021,
                    0.74528987127569646,
                    0.56310281389993255,
                    0.37497841315031705,
                    0.58931415216997118,
                    0.48453281866805525,
                    0.7095856799897875,
                    0.35932394486857594,
                    0.55451333317957441,
                ],
            );

            crate::rng::set_seed(777, 333);
            assert_close(
                &draws(12, 1.2, 1.3),
                &[
                    0.30799027502945509,
                    0.32936235574929068,
                    0.82130440656917603,
                    0.12190201749463517,
                    0.25416525649807536,
                    0.32026749754518408,
                    0.24463430942474479,
                    0.68014571452676242,
                    0.46496987463021622,
                    0.44835983240728655,
                    0.03469899021617695,
                    0.056578241350413797,
                ],
            );

            // shapes ~ 1+eps: init denominator 2ab - alpha cancels
            // catastrophically, so the mul_add contraction above matters.
            crate::rng::set_seed(1234, 5678);
            assert_close(
                &draws(10, 1.0000001, 1.0000001),
                &[
                    0.10208907977268006,
                    0.36901409043763672,
                    0.99881246063936469,
                    0.38156450024179511,
                    0.36930568403082176,
                    0.69652251782457397,
                    0.36437438638866743,
                    0.48093579536559361,
                    0.30926928137696896,
                    0.80226725714543545,
                ],
            );

            // point mass 1/2 at each of {0, 1}: one uniform per draw,
            // exact integers.
            crate::rng::set_seed(1234, 5678);
            let got: Vec<f64> = (0..10).map(|_| rbeta_inner(0.0, 0.0)).collect();
            assert_eq!(got, [0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0]);

            // alternating parameters on one stream: re-init (qsame) each call
            crate::rng::set_seed(1234, 5678);
            let alt: Vec<f64> = (0..10)
                .flat_map(|_| [rbeta_inner(0.4, 6.0), rbeta_inner(2.5, 2.5)])
                .collect();
            assert_close(
                &alt,
                &[
                    0.20311656681825777,
                    0.98606595520116769,
                    0.0082846470321573124,
                    0.41291789870597367,
                    0.074653166525108255,
                    0.37561514048540484,
                    0.0020064816534724793,
                    0.56641351869404788,
                    1.7384622336735472e-08,
                    0.45464528794138093,
                    0.18514882129945051,
                    0.67886413805152346,
                    0.044861207741992246,
                    0.049312525278236097,
                    0.13513997022951002,
                    0.72442248585300717,
                    0.049698564505604274,
                    0.45112953071029627,
                    0.0058476326222255091,
                    0.39971678235016006,
                ],
            );
        });
    }

    /// Replays deterministic uniform sequences through a hooked unif_rand
    /// and asserts the exact number of uniforms each draw consumes plus the
    /// exact returned bits. The sequences are consecutive values of the
    /// Marsaglia-MultiCarry stream; expected consumption and branch
    /// decisions were cross-checked against r-source/src/nmath/rbeta.c
    /// compiled standalone (clang -O2 with its default FMA contraction, and
    /// a counting unif_rand): every iteration consumes exactly one (u1, u2)
    /// pair and the accept/reject decisions match on all sequences below.
    /// Expected bits are the port's own deterministic output (pure-Rust
    /// libm, IEEE ops and mul_add are platform-stable).
    #[test]
    fn rbeta_consumption_order_matches_c_reference() {
        thread_local! {
            static SCRIPT: RefCell<VecDeque<f64>> = RefCell::new(VecDeque::new());
            static DRAWN: Cell<usize> = const { Cell::new(0) };
        }

        fn scripted_unif() -> f64 {
            DRAWN.with(|c| c.set(c.get() + 1));
            SCRIPT.with(|q| q.borrow_mut().pop_front().expect("script exhausted"))
        }

        fn rbeta_scripted(aa: f64, bb: f64, script: &[f64]) -> (f64, usize) {
            with_beta_state(|state| *state = BetaState::default());
            SCRIPT.with(|q| *q.borrow_mut() = script.iter().copied().collect());
            DRAWN.with(|c| c.set(0));
            set_unif_hook(Some(scripted_unif));
            let x = rbeta_inner(aa, bb);
            set_unif_hook(None);
            (x, DRAWN.with(Cell::get))
        }

        // (name, aa, bb, script, uniforms consumed, expected bits)
        #[rustfmt::skip]
        let cases: &[ConsumptionCase] = &[
            (
                "BC squeeze-continue then full-accept",
                0.5, 0.8,
                &[0.10208906980745704, 0.8541567381131826,
                  0.36901408419222892, 0.1646773105404985],
                4, 0x3fe4aea346416c51,
            ),
            (
                "BC quick-accept (z <= 0.25)",
                0.5, 0.8,
                &[0.99881246103877475, 0.092923666372178956],
                2, 0x3eada5391e0d8830,
            ),
            (
                "BC squeeze/full-reject/k2 chain (9 pairs)",
                0.2, 0.7,
                &[0.29703099543607272, 0.8300730434316379,
                  0.76164812309705821, 0.62819044795543655,
                  0.9520031618773942, 0.68892922012343272,
                  0.18602909175353799, 0.35821778149302524,
                  0.96755325025123362, 0.35674027571378741,
                  0.69050329194648719, 0.9527624400688246,
                  0.92918192872991345, 0.97516895899902289,
                  0.16580376475253225, 0.084314301629623917,
                  0.75804591918318653, 0.079398121004784022],
                18, 0x3f4efc48584fa55c,
            ),
            (
                "BC full-accept (a = b = 0.9)",
                0.9, 0.9,
                &[0.10208906980745704, 0.8541567381131826],
                2, 0x3fed607540be259b,
            ),
            (
                "BB do-while exit on first pair",
                2.0, 5.0,
                &[0.10208906980745704, 0.8541567381131826],
                2, 0x3fb81648937b4b5f,
            ),
            (
                "BB squeeze break",
                2.0, 5.0,
                &[0.36901408419222892, 0.1646773105404985],
                2, 0x3fcc8736ab0c0512,
            ),
            (
                "BB while-repeat then do-while exit",
                2.0, 5.0,
                &[0.99881246103877475, 0.092923666372178956,
                  0.38156449454407304, 0.99136333213917971],
                4, 0x3fcd45616b14d814,
            ),
            (
                "BB s > t break",
                2.0, 5.0,
                &[0.36930567779794926, 0.96001240144484012],
                2, 0x3fcc8b9f26b71c37,
            ),
            (
                "BB shapes 1+eps (contracted init)",
                1.0000001, 1.0000001,
                &[0.10208906980745704, 0.8541567381131826],
                2, 0x3fba22828ae7036c,
            ),
            (
                "BB s > t break (a = b = 2.5)",
                2.5, 2.5,
                &[0.30739534280900727, 0.84495957844074787],
                2, 0x3fd7f4bd33c47713,
            ),
            (
                "BB do-while exit (a = b = 2.5)",
                2.5, 2.5,
                &[0.8566246498042307, 0.62739101974931322],
                2, 0x3fe830a4573f71aa,
            ),
            (
                "point mass (0,0): single uniform",
                0.0, 0.0,
                &[0.3],
                1, 0x0,
            ),
        ];

        let mut session = TestSession::new();
        session.with_protected(|| {
            for (name, aa, bb, script, want_used, want_bits) in cases {
                let (x, used) = rbeta_scripted(*aa, *bb, script);
                assert_eq!(used, *want_used, "{name}: uniforms consumed");
                assert_eq!(x.to_bits(), *want_bits, "{name}: returned value");
            }
        });
    }
}
