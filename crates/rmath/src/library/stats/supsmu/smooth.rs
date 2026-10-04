//! Running weighted linear smoother translated from pinned GNU ppr.f.
#![forbid(unsafe_code)]
use super::Input;
#[derive(Clone, Copy)]
pub(super) struct Plan {
    pub periodic: bool,
    pub variance: f64,
    bw: usize,
    crossvalidate: bool,
}
impl Plan {
    pub fn new(
        n: usize,
        span: f64,
        periodic: bool,
        variance: f64,
        crossvalidate: bool,
    ) -> Option<Self> {
        let bw = (0.5 * span * n as f64 + 0.5).trunc().max(2.);
        if !bw.is_finite() || bw > i32::MAX as f64 || (periodic && bw >= n as f64) {
            return None;
        }
        Some(Self {
            periodic,
            variance,
            bw: bw as usize,
            crossvalidate,
        })
    }
}
pub(super) fn smooth(input: Input<'_>, p: Plan, smo: &mut [f64], acvr: &mut [f64]) {
    let n = input.x.len();
    let bw = p.bw as i64;
    let ni = n as i64;
    let (mut xm, mut ym, mut var, mut cvar, mut total) = (0., 0., 0., 0., 0.);
    let count = (2 * p.bw + 1).min(n);
    for i in 1..=count {
        let j = if p.periodic {
            i as i64 - bw - 1
        } else {
            i as i64
        };
        let (index, x) = if j >= 1 {
            let index = (j - 1) as usize;
            (index, input.x[index])
        } else {
            let index = (ni + j - 1) as usize;
            (index, input.x[index] - 1.)
        };
        let w = input.weights[index];
        let before = total;
        total += w;
        if total > 0. {
            xm = (before * xm + w * x) / total;
            ym = (before * ym + w * input.y[index]) / total;
        }
        let delta = if before > 0. {
            total * w * (x - xm) / before
        } else {
            0.
        };
        var += delta * (x - xm);
        cvar += delta * (input.y[index] - ym);
    }
    for j in 1..=n {
        let out = j as i64 - bw - 1;
        let new = j as i64 + bw;
        if p.periodic || (out >= 1 && new <= ni) {
            let (out, new, xo, xi) = if out < 1 {
                let o = (ni + out - 1) as usize;
                let i = (new - 1) as usize;
                (o, i, input.x[o] - 1., input.x[i])
            } else if new > ni {
                let o = (out - 1) as usize;
                let i = (new - ni - 1) as usize;
                (o, i, input.x[o], input.x[i] + 1.)
            } else {
                let o = (out - 1) as usize;
                let i = (new - 1) as usize;
                (o, i, input.x[o], input.x[i])
            };
            let w = input.weights[out];
            let before = total;
            total -= w;
            let delta = if total > 0. {
                before * w * (xo - xm) / total
            } else {
                0.
            };
            var -= delta * (xo - xm);
            cvar -= delta * (input.y[out] - ym);
            if total > 0. {
                xm = (before * xm - w * xo) / total;
                ym = (before * ym - w * input.y[out]) / total;
            }
            let w = input.weights[new];
            let before = total;
            total += w;
            if total > 0. {
                xm = (before * xm + w * xi) / total;
                ym = (before * ym + w * input.y[new]) / total;
            }
            let delta = if before > 0. {
                total * w * (xi - xm) / before
            } else {
                0.
            };
            var += delta * (xi - xm);
            cvar += delta * (input.y[new] - ym);
        }
        let i = j - 1;
        let slope = if var > p.variance { cvar / var } else { 0. };
        smo[i] = slope * (input.x[i] - xm) + ym;
        if p.crossvalidate {
            let mut h = if total > 0. { 1. / total } else { 0. };
            if var > p.variance {
                let deviation = input.x[i] - xm;
                // GNU squares this deviation before division. General powi
                // permits rounding variation that can split identical CV ties.
                h += deviation * deviation / var;
            }
            let a = 1. - input.weights[i] * h;
            acvr[i] = if a > 0. {
                (input.y[i] - smo[i]).abs() / a
            } else if i > 0 {
                acvr[i - 1]
            } else {
                0.
            };
        }
    }
    let mut j = 0;
    while j < n {
        let start = j;
        let mut sy = smo[j] * input.weights[j];
        let mut sum = input.weights[j];
        while j + 1 < n && input.x[j + 1] <= input.x[j] {
            j += 1;
            sy += smo[j] * input.weights[j];
            sum += input.weights[j];
        }
        if j > start {
            smo[start..=j].fill(if sum > 0. { sy / sum } else { 0. });
        }
        j += 1;
    }
}
