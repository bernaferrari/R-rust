//! Histogram class count and bandwidth selectors (nclass.*, bw.*)

use super::*;

/// GNU `nclass.Sturges(x)` is ceiling(log2(length(x)) + 1).
pub unsafe fn do_nclass_sturges(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = if x.is_null() || x == R_NilValue() {
            0.0
        } else {
            XLENGTH(x) as f64
        };
        let v = (n.log2() + 1.0).ceil();
        Rf_ScalarReal(v)
    }
}

fn nclass_numeric_copy(x: SEXP) -> Vec<f64> {
    unsafe {
        let n = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let mut v = Vec::with_capacity(n as usize);
        for i in 0..n {
            let val = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER {
                    f64::NAN
                } else {
                    iv as f64
                }
            } else {
                f64::NAN
            };
            if val.is_finite() {
                v.push(val);
            }
        }
        v
    }
}

fn nclass_sample_var(x: &[f64]) -> f64 {
    let n = x.len();
    if n < 2 {
        return 0.0;
    }
    let mean = x.iter().sum::<f64>() / n as f64;
    x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0)
}

fn nclass_range(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    let min = x.iter().copied().fold(f64::INFINITY, f64::min);
    let max = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    max - min
}

fn nclass_iqr(mut x: Vec<f64>) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    x.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = x.len();
    // GNU quantile type 7: (n-1)*p + 1
    let q = |p: f64| -> f64 {
        let h = (n as f64 - 1.0) * p + 1.0;
        let lo = h.floor().clamp(1.0, n as f64) as usize;
        let hi = h.ceil().clamp(1.0, n as f64) as usize;
        let a = x[lo - 1];
        let b = x[hi - 1];
        a + (h - lo as f64) * (b - a)
    };
    q(0.75) - q(0.25)
}

/// GNU `nclass.scott(x)`.
pub unsafe fn do_nclass_scott(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        let n = x.len() as f64;
        let h = 3.5 * nclass_sample_var(&x).sqrt() * n.powf(-1.0 / 3.0);
        let bins = if h > 0.0 {
            (nclass_range(&x) / h).ceil().max(1.0)
        } else {
            1.0
        };
        Rf_ScalarReal(bins)
    }
}

/// GNU `nclass.FD(x)`.
pub unsafe fn do_nclass_fd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        let n = x.len() as f64;
        let xs: Vec<f64> = x
            .iter()
            .copied()
            .map(|v| crate::fprec::fprec(v, 5.0))
            .collect();
        let mut h = 2.0 * nclass_iqr(xs.clone());
        if h == 0.0 {
            let mut sorted = xs.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let q7 = |p: f64| -> f64 {
                let n = sorted.len();
                if n == 0 {
                    return f64::NAN;
                }
                if n == 1 {
                    return sorted[0];
                }
                let idx = 1.0 + (n as f64 - 1.0) * p;
                let j = idx.floor() as usize;
                let g = idx - j as f64;
                let a = sorted[(j.saturating_sub(1)).min(n - 1)];
                let b = sorted[j.min(n - 1)];
                (1.0 - g) * a + g * b
            };
            let mut al = 0.25;
            while h == 0.0 && al / 2.0 >= 1.0 / 512.0 {
                al /= 2.0;
                h = (q7(1.0 - al) - q7(al)) / (1.0 - 2.0 * al);
            }
        }
        if h == 0.0 {
            h = 3.5 * nclass_sample_var(&x).sqrt();
        }
        let bins = if h > 0.0 {
            (nclass_range(&x) / h * n.powf(1.0 / 3.0)).ceil()
        } else {
            1.0
        };
        Rf_ScalarReal(bins)
    }
}

/// GNU `bw.nrd0(x)`.
pub unsafe fn do_bw_nrd0(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        if x.len() < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "need at least 2 data points",
            );
        }
        let hi = nclass_sample_var(&x).sqrt();
        let mut lo = hi.min(nclass_iqr(x.clone()) / 1.34);
        if lo == 0.0 {
            lo = hi;
        }
        if lo == 0.0 {
            lo = x[0].abs();
        }
        if lo == 0.0 {
            lo = 1.0;
        }
        Rf_ScalarReal(0.9 * lo * (x.len() as f64).powf(-0.2))
    }
}

/// GNU `bw.nrd(x)`.
pub unsafe fn do_bw_nrd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        if x.len() < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "need at least 2 data points",
            );
        }
        let h = nclass_iqr(x.clone()) / 1.34;
        let sd = nclass_sample_var(&x).sqrt();
        Rf_ScalarReal(1.06 * sd.min(h) * (x.len() as f64).powf(-0.2))
    }
}

fn bw_phi_binned(x: &[f64], h: f64, deriv: i32) -> f64 {
    let n = x.len();
    if n < 2 || h <= 0.0 {
        return f64::NAN;
    }
    let xmin = x.iter().copied().fold(f64::INFINITY, f64::min);
    let xmax = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let nb = 1000usize;
    let rang = (xmax - xmin) * 1.01;
    if rang == 0.0 {
        return f64::NAN;
    }
    let dd = rang / nb as f64;
    let mut cnt = vec![0.0; nb];
    for i in 1..n {
        let ii = (x[i] / dd) as i32;
        for j in 0..i {
            let jj = (x[j] / dd) as i32;
            let k = (ii - jj).unsigned_abs() as usize;
            if k < nb {
                cnt[k] += 1.0;
            }
        }
    }
    let mut sum = 0.0;
    for (i, &c) in cnt.iter().enumerate() {
        if c == 0.0 {
            continue;
        }
        let mut delta = (i as f64) * dd / h;
        delta *= delta;
        if delta >= 1000.0 {
            break;
        }
        let term = if deriv == 4 {
            (-delta / 2.0).exp() * (delta * delta - 6.0 * delta + 3.0)
        } else {
            (-delta / 2.0).exp()
                * (delta * delta * delta - 15.0 * delta * delta + 45.0 * delta - 15.0)
        };
        sum += term * c;
    }
    let nf = n as f64;
    if deriv == 4 {
        sum = 2.0 * sum + nf * 3.0;
        sum / (nf * (nf - 1.0) * h.powi(5)) / (2.0 * std::f64::consts::PI).sqrt()
    } else {
        sum = 2.0 * sum - 15.0 * nf;
        sum / (nf * (nf - 1.0) * h.powi(7)) / (2.0 * std::f64::consts::PI).sqrt()
    }
}

/// GNU `bw.SJ(x, method="dpi")` — Sheather–Jones direct plug-in.
pub unsafe fn do_bw_sj(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        let n = x.len();
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "need at least 2 data points",
            );
        }
        let nf = n as f64;
        let sd = nclass_sample_var(&x).sqrt();
        let iqr = nclass_iqr(x.clone()) / 1.349;
        let scale = sd.min(iqr);
        let b = 1.23 * scale * nf.powf(-1.0 / 9.0);
        let td = -bw_phi_binned(&x, b, 6);
        if !td.is_finite() || td <= 0.0 {
            return Rf_ScalarReal(f64::NAN);
        }
        let c1 = 1.0 / (2.0 * std::f64::consts::PI.sqrt() * nf);
        let h1 = (2.394 / (nf * td)).powf(1.0 / 7.0);
        let sd_h = bw_phi_binned(&x, h1, 4);
        let res = (c1 / sd_h).powf(1.0 / 5.0);
        Rf_ScalarReal(res)
    }
}

fn pair_cnts(x: &[f64]) -> (f64, Vec<f64>) {
    let n = x.len();
    let xmin = x.iter().copied().fold(f64::INFINITY, f64::min);
    let xmax = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let nb = 1000usize;
    let rang = (xmax - xmin) * 1.01;
    let dd = if rang > 0.0 { rang / nb as f64 } else { 1.0 };
    let mut cnt = vec![0.0; nb];
    for i in 1..n {
        let ii = (x[i] / dd) as i32;
        for j in 0..i {
            let jj = (x[j] / dd) as i32;
            let k = (ii - jj).unsigned_abs() as usize;
            if k < nb {
                cnt[k] += 1.0;
            }
        }
    }
    (dd, cnt)
}

fn bw_ucv_at(n: usize, dd: f64, cnt: &[f64], h: f64) -> f64 {
    if h <= 0.0 {
        return f64::INFINITY;
    }
    let nf = n as f64;
    let mut sum = 0.0;
    for (i, &c) in cnt.iter().enumerate() {
        if c == 0.0 {
            continue;
        }
        let mut delta = (i as f64) * dd / h;
        delta *= delta;
        if delta >= 1000.0 {
            break;
        }
        let term = (-delta / 4.0).exp() - 8.0f64.sqrt() * (-delta / 2.0).exp();
        sum += term * c;
    }
    (0.5 + sum / nf) / (nf * h * std::f64::consts::PI.sqrt())
}

/// GNU `bw.ucv(x)` — unbiased cross-validation bandwidth.
pub unsafe fn do_bw_ucv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        let n = x.len();
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "need at least 2 data points",
            );
        }
        let nf = n as f64;
        let hmax = 1.144 * nclass_sample_var(&x).sqrt() * nf.powf(-0.2);
        let mut lo = 0.1 * hmax;
        let mut hi = hmax;
        let (dd, cnt) = pair_cnts(&x);
        let gr = (5.0f64.sqrt() - 1.0) / 2.0;
        let mut x1 = hi - gr * (hi - lo);
        let mut x2 = lo + gr * (hi - lo);
        let mut f1 = bw_ucv_at(n, dd, &cnt, x1);
        let mut f2 = bw_ucv_at(n, dd, &cnt, x2);
        for _ in 0..80 {
            if f1 < f2 {
                hi = x2;
                x2 = x1;
                f2 = f1;
                x1 = hi - gr * (hi - lo);
                f1 = bw_ucv_at(n, dd, &cnt, x1);
            } else {
                lo = x1;
                x1 = x2;
                f1 = f2;
                x2 = lo + gr * (hi - lo);
                f2 = bw_ucv_at(n, dd, &cnt, x2);
            }
        }
        Rf_ScalarReal(0.5 * (lo + hi))
    }
}

fn bw_bcv_at(n: usize, dd: f64, cnt: &[f64], h: f64) -> f64 {
    if h <= 0.0 {
        return f64::INFINITY;
    }
    let nf = n as f64;
    let mut sum = 0.0;
    for (i, &c) in cnt.iter().enumerate() {
        if c == 0.0 {
            continue;
        }
        let mut delta = (i as f64) * dd / h;
        delta *= delta;
        if delta >= 1000.0 {
            break;
        }
        let term = (-delta / 4.0).exp() * (delta * delta - 12.0 * delta + 12.0);
        sum += term * c;
    }
    (1.0 + sum / (32.0 * nf)) / (2.0 * nf * h * std::f64::consts::PI.sqrt())
}

/// GNU `bw.bcv(x)` — biased cross-validation bandwidth.
pub unsafe fn do_bw_bcv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        let n = x.len();
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "need at least 2 data points",
            );
        }
        let nf = n as f64;
        let hmax = 1.144 * nclass_sample_var(&x).sqrt() * nf.powf(-0.2);
        let mut lo = 0.1 * hmax;
        let mut hi = hmax;
        let (dd, cnt) = pair_cnts(&x);
        let gr = (5.0f64.sqrt() - 1.0) / 2.0;
        let mut x1 = hi - gr * (hi - lo);
        let mut x2 = lo + gr * (hi - lo);
        let mut f1 = bw_bcv_at(n, dd, &cnt, x1);
        let mut f2 = bw_bcv_at(n, dd, &cnt, x2);
        for _ in 0..80 {
            if f1 < f2 {
                hi = x2;
                x2 = x1;
                f2 = f1;
                x1 = hi - gr * (hi - lo);
                f1 = bw_bcv_at(n, dd, &cnt, x1);
            } else {
                lo = x1;
                x1 = x2;
                f1 = f2;
                x2 = lo + gr * (hi - lo);
                f2 = bw_bcv_at(n, dd, &cnt, x2);
            }
        }
        Rf_ScalarReal(0.5 * (lo + hi))
    }
}
