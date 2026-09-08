// Rust adaptation of the upstream LOESS algorithms.
// Copyright (C) 1998--2020 The R Core Team
//
// The authors of this software are Cleveland, Grosse, and Shyu.
// Copyright (c) 1989, 1992 by AT&T.
// Permission to use, copy, modify, and distribute this software for any
// purpose without fee is hereby granted, provided that this entire notice
// is included in all copies of any software which is or includes a copy
// or modification of this software and in all copies of the supporting
// documentation for such software.
// THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR IMPLIED
// WARRANTY. IN PARTICULAR, NEITHER THE AUTHORS NOR AT&T MAKE ANY
// REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE MERCHANTABILITY
// OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR PURPOSE.

//! Owned LOESS numerical engine, following Cleveland/Grosse/Shyu's algorithms
//! in the pinned R stats/src/loessf.f. No interpreter pointers enter this module.
#![forbid(unsafe_code)]
mod diagnostics;
mod surface;

#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub span: f64,
    pub degree: usize,
    pub normalize: bool,
    pub parametric: Vec<bool>,
    pub drop_square: Vec<bool>,
    pub interpolate: bool,
    pub cell: f64,
    pub iterations: usize,
    pub exact: bool,
    pub approximate_trace: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Model {
    pub x: Vec<Vec<f64>>,
    pub y: Vec<f64>,
    pub weights: Vec<f64>,
    pub divisor: Vec<f64>,
    pub config: Config,
    pub fitted: Vec<f64>,
    pub robust: Vec<f64>,
    pub trace: f64,
    pub delta1: f64,
    pub delta2: f64,
    pub s: f64,
}

impl Model {
    pub fn fit(
        mut x: Vec<Vec<f64>>,
        y: Vec<f64>,
        weights: Vec<f64>,
        config: Config,
    ) -> Result<Self, String> {
        let n = y.len();
        let d = x.first().map_or(0, Vec::len);
        if n == 0
            || !(1..=4).contains(&d)
            || x.len() != n
            || weights.len() != n
            || x.iter()
                .any(|row| row.len() != d || row.iter().any(|v| !v.is_finite()))
            || y.iter().any(|v| !v.is_finite())
            || weights.iter().any(|v| !v.is_finite() || *v < 0.)
        {
            return Err("invalid LOESS data or weights".into());
        }
        if !config.span.is_finite()
            || config.span <= 0.
            || config.degree > 2
            || config.iterations == 0
            || !config.cell.is_finite()
            || config.cell <= 0.
            || config.parametric.len() != d
            || config.drop_square.len() != d
            || config.parametric.iter().all(|v| *v)
        {
            return Err("invalid LOESS control parameters".into());
        }
        if config.drop_square.iter().any(|v| *v) && (d == 1 || config.degree != 2) {
            return Err("invalid dropped square for LOESS degree or predictor count".into());
        }
        let mut divisor = vec![1.; d];
        if config.normalize && d > 1 {
            let trim = (0.1 * n as f64).ceil() as usize;
            if n <= 2 * trim + 1 {
                return Err("not enough observations to normalize predictors".into());
            }
            for j in 0..d {
                let mut col: Vec<_> = x.iter().map(|row| row[j]).collect();
                col.sort_by(f64::total_cmp);
                let col = &col[trim..n - trim];
                let mean = col.iter().sum::<f64>() / col.len() as f64;
                divisor[j] = (col.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                    / (col.len() - 1) as f64)
                    .sqrt();
                if divisor[j] == 0. {
                    return Err("zero-width LOESS predictor".into());
                }
                for row in &mut x {
                    row[j] /= divisor[j];
                }
            }
        }
        let mut model = Self {
            x,
            y,
            weights,
            divisor,
            config,
            fitted: vec![],
            robust: vec![1.; n],
            trace: 0.,
            delta1: 0.,
            delta2: 0.,
            s: 0.,
        };
        let base = model.influence(&model.x, &model.weights)?;
        model.trace = (0..n).map(|i| base[i][i]).sum();
        if model.config.approximate_trace && model.config.interpolate && !model.config.exact {
            let tau = if model.config.degree == 0 {
                d + 1
            } else {
                model.basis(&vec![0.; d]).len()
            } as f64;
            let g1 = (-0.08125 * d as f64 + 0.13) * d as f64 + 1.05;
            model.trace = tau * (1. + ((g1 - model.config.span) / model.config.span).max(0.));
        }
        (model.delta1, model.delta2) = if model.config.exact {
            diagnostics::exact(&base)
        } else {
            diagnostics::approximate(
                n,
                d,
                if model.config.degree == 0 {
                    d + 1
                } else {
                    model.basis(&vec![0.; d]).len()
                },
                model.trace,
            )
        };
        model.fitted = multiply(&base, &model.y);
        for _ in 1..model.config.iterations {
            let residuals: Vec<_> = model
                .y
                .iter()
                .zip(&model.fitted)
                .map(|(y, f)| (y - f).abs())
                .collect();
            let cmad = 6. * median(residuals.clone());
            model.robust = residuals
                .iter()
                .map(|r| {
                    if cmad < f64::MIN_POSITIVE || *r <= cmad * 0.001 {
                        1.
                    } else if *r > cmad * 0.999 {
                        0.
                    } else {
                        (1. - (r / cmad).powi(2)).powi(2)
                    }
                })
                .collect();
            let w: Vec<_> = model
                .weights
                .iter()
                .zip(&model.robust)
                .map(|(w, r)| w * r)
                .collect();
            model.fitted = multiply(&model.influence(&model.x, &w)?, &model.y);
        }
        let residuals = if model.config.iterations > 1 {
            let residuals: Vec<_> = model
                .y
                .iter()
                .zip(&model.fitted)
                .map(|(y, f)| y - f)
                .collect();
            let mad = median(
                residuals
                    .iter()
                    .zip(&model.weights)
                    .map(|(r, w)| r.abs() * w.sqrt())
                    .collect(),
            );
            let c = (6. * mad).powi(2) / 5.;
            let scale = n as f64
                / residuals
                    .iter()
                    .zip(&model.weights)
                    .zip(&model.robust)
                    .map(|((r, w), rw)| (1. - r * r * w / c) * rw.sqrt())
                    .sum::<f64>();
            let pseudo: Vec<_> = model
                .fitted
                .iter()
                .zip(&residuals)
                .zip(&model.robust)
                .map(|((f, r), rw)| f + scale * rw * r)
                .collect();
            let pf = multiply(&base, &pseudo);
            pseudo
                .iter()
                .zip(pf)
                .map(|(y, f)| y - f)
                .collect::<Vec<_>>()
        } else {
            model
                .y
                .iter()
                .zip(&model.fitted)
                .map(|(y, f)| y - f)
                .collect()
        };
        model.s = (residuals
            .iter()
            .zip(&model.weights)
            .map(|(r, w)| r * r * w)
            .sum::<f64>()
            / model.delta1)
            .sqrt();
        Ok(model)
    }

    pub fn predict(&self, queries: &[Vec<f64>], se: bool) -> Result<(Vec<f64>, Vec<f64>), String> {
        let d = self.divisor.len();
        let n = self.y.len();
        if n == 0
            || !(1..=4).contains(&d)
            || self.x.len() != n
            || self.weights.len() != n
            || self.robust.len() != n
            || self
                .x
                .iter()
                .any(|r| r.len() != d || r.iter().any(|v| !v.is_finite()))
            || self.divisor.iter().any(|v| !v.is_finite() || *v <= 0.)
            || self
                .weights
                .iter()
                .chain(&self.robust)
                .any(|v| !v.is_finite() || *v < 0.)
            || self.config.degree > 2
            || !self.config.span.is_finite()
            || self.config.span <= 0.
            || !self.config.cell.is_finite()
            || self.config.cell <= 0.
            || self.config.parametric.len() != d
            || self.config.drop_square.len() != d
            || self.config.parametric.iter().all(|v| *v)
        {
            return Err("invalid LOESS model".into());
        }
        if queries.iter().any(|q| q.len() != d) {
            return Err("wrong number of LOESS predictors".into());
        }
        let q: Vec<Vec<_>> = queries
            .iter()
            .map(|q| q.iter().zip(&self.divisor).map(|(v, s)| v / s).collect())
            .collect();
        let weights: Vec<_> = self
            .weights
            .iter()
            .zip(&self.robust)
            .map(|(w, r)| w * r)
            .collect();
        let mut fitted = vec![f64::NAN; q.len()];
        let mut errors = fitted.clone();
        let valid: Vec<_> = q
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.iter().all(|v| v.is_finite())
                    && (!self.config.interpolate
                        || (0..d).all(|j| {
                            let (lo, hi) = self
                                .x
                                .iter()
                                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
                                    (a.min(x[j]), b.max(x[j]))
                                });
                            row[j] >= lo && row[j] <= hi
                        }))
            })
            .collect();
        let q: Vec<_> = valid.iter().map(|(_, q)| (*q).clone()).collect();
        // GNU R's direct robust prediction uses robustness weights alone when
        // standard errors are requested (loess_dfitse); preserve that contract.
        let fit_weights = if se && !self.config.interpolate && self.config.iterations > 1 {
            &self.robust
        } else {
            &weights
        };
        let l = self.influence(&q, fit_weights)?;
        let values = multiply(&l, &self.y);
        let uncertainty = if se {
            self.influence(
                &q,
                if self.config.interpolate {
                    &self.weights
                } else {
                    &weights
                },
            )?
        } else {
            vec![]
        };
        for (k, (i, _)) in valid.iter().enumerate() {
            fitted[*i] = values[k];
            if se {
                errors[*i] = self.s
                    * uncertainty[k]
                        .iter()
                        .zip(&self.weights)
                        .map(|(l, w)| if *w > 0. { l * l / w } else { 0. })
                        .sum::<f64>()
                        .sqrt();
            }
        }
        Ok((fitted, errors))
    }

    fn influence(&self, q: &[Vec<f64>], weights: &[f64]) -> Result<Vec<Vec<f64>>, String> {
        if self.config.interpolate {
            surface::interpolate(self, q, weights)
        } else {
            q.iter()
                .map(|q| self.local(q, weights).map(|coeff| coeff[0].clone()))
                .collect()
        }
    }

    fn basis(&self, delta: &[f64]) -> Vec<f64> {
        let mut terms = vec![1.];
        if self.config.degree >= 1 {
            terms.extend(delta);
        }
        if self.config.degree == 2 {
            for j in 0..delta.len() {
                if !self.config.drop_square[j] {
                    terms.push(delta[j] * delta[j]);
                }
                for k in j + 1..delta.len() {
                    terms.push(delta[j] * delta[k]);
                }
            }
        }
        terms
    }

    // Coefficient influence rows: intercept, followed by first derivatives.
    fn local(&self, q: &[f64], weights: &[f64]) -> Result<Vec<Vec<f64>>, String> {
        let n = self.x.len();
        let d = q.len();
        let nf = ((n as f64 * self.config.span + 1e-5).floor() as usize).min(n);
        if nf == 0 {
            return Err("span is too small".into());
        }
        let mut order: Vec<_> = self
            .x
            .iter()
            .enumerate()
            .map(|(i, row)| {
                (
                    i,
                    row.iter()
                        .zip(q)
                        .zip(&self.config.parametric)
                        .filter(|(_, p)| !**p)
                        .map(|((x, q), _)| (x - q).powi(2))
                        .sum::<f64>(),
                )
            })
            .collect();
        order.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let radius = order[nf - 1].1 * self.config.span.max(1.);
        if !radius.is_finite() || radius <= 0. {
            return Err("LOESS neighborhood has zero width".into());
        }
        let k = self.basis(q).len();
        let mut a = vec![vec![0.; k]; nf];
        let mut w = vec![0.; nf];
        for i in 0..nf {
            let (index, dist) = order[i];
            w[i] = (weights[index] * (1. - (dist / radius).sqrt().powi(3)).max(0.).powi(3)).sqrt();
            let delta: Vec<_> = self.x[index].iter().zip(q).map(|(x, q)| x - q).collect();
            for (j, b) in self.basis(&delta).iter().enumerate() {
                a[i][j] = w[i] * b;
            }
        }
        if w.iter().all(|w| *w == 0.) {
            return Err("all LOESS neighborhood weights are zero".into());
        }
        let scale: Vec<_> = (0..k)
            .map(|j| {
                a.iter()
                    .map(|r| r[j] * r[j])
                    .sum::<f64>()
                    .sqrt()
                    .max(f64::MIN_POSITIVE)
            })
            .collect();
        for row in &mut a {
            for j in 0..k {
                row[j] /= scale[j];
            }
        }
        let pinv = pseudoinverse(&a)?;
        let mut output = vec![vec![0.; n]; d + 1];
        for j in 0..(d + 1).min(k) {
            for i in 0..nf {
                output[j][order[i].0] = pinv[j][i] * w[i] / scale[j];
            }
        }
        Ok(output)
    }
}

fn multiply(matrix: &[Vec<f64>], y: &[f64]) -> Vec<f64> {
    matrix
        .iter()
        .map(|row| row.iter().zip(y).map(|(a, b)| a * b).sum())
        .collect()
}
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n % 2 == 0 {
        (values[n / 2 - 1] + values[n / 2]) / 2.
    } else {
        values[n / 2]
    }
}

#[cfg(feature = "rust-backend")]
fn pseudoinverse(a: &[Vec<f64>]) -> Result<Vec<Vec<f64>>, String> {
    let (m, n) = (a.len(), a[0].len());
    let matrix = faer::Mat::from_fn(m, n, |i, j| a[i][j]);
    let svd = matrix
        .svd()
        .map_err(|_| "LOESS singular value decomposition failed")?;
    let s = svd.S();
    let u = svd.U();
    let v = svd.V();
    let tolerance = s[0] * 100. * f64::EPSILON;
    Ok((0..n)
        .map(|i| {
            (0..m)
                .map(|j| {
                    (0..m.min(n))
                        .filter(|k| s[*k] > tolerance)
                        .map(|k| v[(i, k)] * u[(j, k)] / s[k])
                        .sum()
                })
                .collect()
        })
        .collect())
}

// One-sided Jacobi SVD keeps this owned kernel available with the system
// backend too, without raw LAPACK calls or squaring the condition number.
#[cfg(not(feature = "rust-backend"))]
fn pseudoinverse(a: &[Vec<f64>]) -> Result<Vec<Vec<f64>>, String> {
    let (m, n) = (a.len(), a[0].len());
    let mut b = a.to_vec();
    let mut v: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..n).map(|j| f64::from(i == j)).collect())
        .collect();
    for _ in 0..100 {
        let mut changed = false;
        for p in 0..n {
            for q in p + 1..n {
                let aa = b.iter().map(|r| r[p] * r[p]).sum::<f64>();
                let bb = b.iter().map(|r| r[q] * r[q]).sum::<f64>();
                let ab = b.iter().map(|r| r[p] * r[q]).sum::<f64>();
                if ab.abs() <= f64::EPSILON * (aa * bb).sqrt() {
                    continue;
                }
                let z = (bb - aa) / (2. * ab);
                let t = z.signum() / (z.abs() + (1. + z * z).sqrt());
                let t = if z == 0. { 1. } else { t };
                let c = 1. / (1. + t * t).sqrt();
                let s = c * t;
                for r in &mut b {
                    let x = r[p];
                    let y = r[q];
                    r[p] = c * x - s * y;
                    r[q] = s * x + c * y;
                }
                for r in &mut v {
                    let x = r[p];
                    let y = r[q];
                    r[p] = c * x - s * y;
                    r[q] = s * x + c * y;
                }
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let sigma: Vec<_> = (0..n)
        .map(|j| b.iter().map(|r| r[j] * r[j]).sum::<f64>())
        .collect();
    let tolerance = sigma.iter().copied().fold(0., f64::max) * (100. * f64::EPSILON).powi(2);
    Ok((0..n)
        .map(|i| {
            (0..m)
                .map(|j| {
                    (0..n)
                        .filter(|k| sigma[*k] > tolerance)
                        .map(|k| v[i][k] * b[j][k] / sigma[k])
                        .sum()
                })
                .collect()
        })
        .collect())
}

#[cfg(test)]
mod tests;
