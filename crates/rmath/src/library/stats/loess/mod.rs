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
pub(crate) mod pruned;
pub(crate) mod robust;
mod surface;

/// Byte estimate for LOESS workspaces. `None` means the product overflowed.
pub(crate) fn loess_workspace_bytes(
    n: usize,
    d: usize,
    queries: usize,
    interpolate: bool,
) -> Option<usize> {
    let vertices = if interpolate {
        n.max(200).checked_add(32)?
    } else {
        0
    };
    let rows = n
        .checked_mul(4)?
        .checked_add(queries.checked_mul(2)?)?
        .checked_add(vertices.checked_mul(d.checked_add(1)?)?)?
        .checked_add(256)?;
    n.checked_mul(rows)?
        .checked_add(queries.checked_mul(16)?)?
        .checked_mul(8)
}

#[cfg(kani)]
mod kani_proofs {
    use super::loess_workspace_bytes;

    #[kani::proof]
    fn loess_workspace_rejects_overflow() {
        let n: usize = kani::any();
        let d: usize = kani::any();
        let queries: usize = kani::any();
        let interpolate: bool = kani::any();
        kani::assume(n <= 3 && d <= 2 && queries <= 3);
        let got = loess_workspace_bytes(n, d, queries, interpolate);
        assert!(got.is_some());
        assert!(loess_workspace_bytes(usize::MAX, 1, 1, false).is_none());
        kani::cover(interpolate, "interpolate");
        kani::cover(!interpolate && n == 0, "empty");
    }
}

/// Per-operation limits and cancellation; never retained in serialized models.
pub(crate) struct Execution<'a> {
    workspace_limit: usize,
    check: &'a dyn Fn() -> Result<(), String>,
    // GNU's ehg131 keeps `sing`/`rcond` in saved state for a whole fit and
    // emits its singularity diagnostics through R's warning(); the recorded
    // messages are replayed by the R-facing bridges after a successful fit.
    warnings: std::cell::RefCell<Vec<String>>,
    sing: std::cell::Cell<usize>,
    rcond: std::cell::Cell<f64>,
    span_warned: std::cell::Cell<bool>,
}
impl<'a> Execution<'a> {
    pub fn new(check: &'a dyn Fn() -> Result<(), String>) -> Self {
        Self {
            workspace_limit: 256 * 1024 * 1024,
            check,
            warnings: std::cell::RefCell::default(),
            sing: std::cell::Cell::new(0),
            rcond: std::cell::Cell::new(1.),
            span_warned: std::cell::Cell::new(false),
        }
    }

    #[cfg(test)]
    pub fn with_workspace_limit(
        workspace_limit: usize,
        check: &'a dyn Fn() -> Result<(), String>,
    ) -> Self {
        Self {
            workspace_limit,
            check,
            warnings: std::cell::RefCell::default(),
            sing: std::cell::Cell::new(0),
            rcond: std::cell::Cell::new(1.),
            span_warned: std::cell::Cell::new(false),
        }
    }
    fn checkpoint(&self) -> Result<(), String> {
        (self.check)()
    }

    /// Record a GNU-format diagnostic; order matches ehg131's emission.
    pub fn warn(&self, message: impl Into<String>) {
        self.warnings.borrow_mut().push(message.into());
    }

    /// Drain recorded diagnostics (the caller replays them as R warnings).
    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut *self.warnings.borrow_mut())
    }

    /// ehg184-style message: prefix followed by one ` %.5g` value per entry.
    fn warn_values(&self, prefix: &str, values: &[f64]) {
        let mut message = String::from(prefix);
        for value in values {
            message.push(' ');
            message.push_str(&format_g5(*value));
        }
        self.warn(message);
    }

    /// loesswarn(104): emitted once per fit when the local model needs more
    /// degrees of freedom than the span supplies.
    fn warn_span_too_small(&self, terms: usize, neighborhood: usize) {
        if terms > neighborhood.saturating_sub(1) && !self.span_warned.replace(true) {
            self.warn("span too small.   fewer data values than degrees of freedom.");
        }
    }

    /// ehg131's cumulative near-singularity diagnostics.
    fn note_singular(&self, ratio: f64, q: &[f64], rho: f64) {
        // gfortran's min() keeps the previous value when the new ratio is
        // NaN (0/0 from an exactly zero design), and so do we.
        if !ratio.is_nan() {
            self.rcond.set(self.rcond.get().min(ratio));
        }
        // GNU: tol = sigma(1) * 100 * machep; singular when sigma(k) <= tol.
        if ratio > 100. * f64::EPSILON {
            return;
        }
        let sing = self.sing.get() + 1;
        self.sing.set(sing);
        if sing == 1 {
            self.warn_values("pseudoinverse used at", q);
            self.warn_values("neighborhood radius", &[rho.sqrt()]);
            self.warn_values("reciprocal condition number ", &[self.rcond.get()]);
        } else if sing == 2 {
            self.warn_values("There are other near singularities as well.", &[rho]);
        }
    }

    fn workspace(
        &self,
        n: usize,
        d: usize,
        queries: usize,
        interpolate: bool,
    ) -> Result<(), String> {
        // Conservative admission estimate for simultaneous influence matrices,
        // interpolation coefficients, local SVD scratch and KD-cell storage.
        // A split can add up to 2^(d+1) vertices beyond the tree's stopping bound.
        // This bounds numerical workspaces, not the host's total process memory.
        let bytes = loess_workspace_bytes(n, d, queries, interpolate);
        if bytes.is_none_or(|bytes| bytes > self.workspace_limit) {
            return Err(format!(
                "LOESS workspace limit exceeded ({} MiB); reduce observations or prediction rows",
                self.workspace_limit / (1024 * 1024)
            ));
        }
        self.checkpoint()
    }
}

/// Format like C's `%.5g` (loessc.c's ehg184a diagnostic values).
fn format_g5(x: f64) -> String {
    if x == 0. {
        return "0".into();
    }
    if !x.is_finite() {
        return format!("{x}");
    }
    let exponent = x.abs().log10().floor() as i32;
    if (-4..5).contains(&exponent) {
        let decimals = (4 - exponent).max(0) as usize;
        let mut s = format!("{:.*}", decimals, x);
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    } else {
        let s = format!("{:.4e}", x);
        let (mantissa, power) = s.split_once('e').unwrap_or((s.as_str(), ""));
        let sign = if power.starts_with('-') { "-" } else { "+" };
        let digits = power.trim_start_matches(['+', '-']);
        format!("{mantissa}e{sign}{digits:0>2}")
    }
}

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
    #[cfg(test)]
    pub fn fit(
        x: Vec<Vec<f64>>,
        y: Vec<f64>,
        weights: Vec<f64>,
        config: Config,
    ) -> Result<Self, String> {
        Self::fit_with_execution(x, y, weights, config, &Execution::new(&|| Ok(())))
    }

    pub fn fit_with_execution(
        mut x: Vec<Vec<f64>>,
        y: Vec<f64>,
        weights: Vec<f64>,
        config: Config,
        execution: &Execution<'_>,
    ) -> Result<Self, String> {
        execution.checkpoint()?;
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
        execution.workspace(n, d, n, config.interpolate)?;
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
        let base = model.influence(&model.x, &model.weights, execution)?;
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
            diagnostics::exact(&base, execution)?
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
            execution.checkpoint()?;
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
            model.fitted = multiply(&model.influence(&model.x, &w, execution)?, &model.y);
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
        execution.checkpoint()?;
        Ok(model)
    }

    #[cfg(test)]
    pub fn predict(&self, queries: &[Vec<f64>], se: bool) -> Result<(Vec<f64>, Vec<f64>), String> {
        self.predict_with_execution(queries, se, &Execution::new(&|| Ok(())))
    }

    pub fn predict_with_execution(
        &self,
        queries: &[Vec<f64>],
        se: bool,
        execution: &Execution<'_>,
    ) -> Result<(Vec<f64>, Vec<f64>), String> {
        self.predict_with_execution_se(queries, se, None, execution)
    }

    /// Like [`Self::predict_with_execution`], but the standard-error weights
    /// (`se.fit = s * sqrt(sum(L^2 / se_divisor))`) may differ from the model
    /// weights. GNU R's `loess_dfitse` computes L with `weights*robust` yet
    /// divides by the raw `weights` when forming `se.fit`.
    pub fn predict_with_execution_se(
        &self,
        queries: &[Vec<f64>],
        se: bool,
        se_divisor: Option<&[f64]>,
        execution: &Execution<'_>,
    ) -> Result<(Vec<f64>, Vec<f64>), String> {
        execution.checkpoint()?;
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
        execution.workspace(n, d, queries.len(), self.config.interpolate)?;
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
        // Unavailable predictions are R NA, distinguishable from computational
        // NaN by is.nan(). This is only a numeric sentinel, not runtime state.
        let mut fitted = vec![crate::sexp::ffi::NA_REAL; q.len()];
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
        let l = self.influence(&q, fit_weights, execution)?;
        let values = multiply(&l, &self.y);
        let uncertainty = if se {
            self.influence(
                &q,
                if self.config.interpolate {
                    &self.weights
                } else {
                    &weights
                },
                execution,
            )?
        } else {
            vec![]
        };
        for (k, (i, _)) in valid.iter().enumerate() {
            fitted[*i] = values[k];
            if se {
                let divisor = se_divisor.unwrap_or(&self.weights);
                errors[*i] = self.s
                    * uncertainty[k]
                        .iter()
                        .zip(divisor)
                        .map(|(l, w)| if *w > 0. { l * l / w } else { 0. })
                        .sum::<f64>()
                        .sqrt();
            }
        }
        execution.checkpoint()?;
        Ok((fitted, errors))
    }

    fn influence(
        &self,
        q: &[Vec<f64>],
        weights: &[f64],
        execution: &Execution<'_>,
    ) -> Result<Vec<Vec<f64>>, String> {
        if self.config.interpolate {
            surface::interpolate(self, q, weights, execution)
        } else {
            q.iter()
                .map(|q| {
                    self.local(q, weights, execution)
                        .map(|coeff| coeff[0].clone())
                })
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
    fn local(
        &self,
        q: &[f64],
        weights: &[f64],
        execution: &Execution<'_>,
    ) -> Result<Vec<Vec<f64>>, String> {
        execution.checkpoint()?;
        let n = self.x.len();
        let d = q.len();
        let nf = ((n as f64 * self.config.span + 1e-5).floor() as usize).min(n);
        if nf == 0 {
            return Err("span is too small".into());
        }
        execution.warn_span_too_small(self.basis(q).len(), nf);
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
        // GNU's ehg131 calls this `rho` (a squared distance) and only warns
        // when it degenerates; the fit continues through the singular paths.
        let rho = order[nf - 1].1 * self.config.span.max(1.);
        if rho.is_finite() && rho <= 0. {
            execution.warn("zero-width neighborhood. make span bigger");
        }
        let k = self.basis(q).len();
        let mut a = vec![vec![0.; k]; nf];
        let mut w = vec![0.; nf];
        for i in 0..nf {
            let (index, dist) = order[i];
            // kernel = tricube on sqrt(dist/rho); rho == 0 propagates the
            // 0/0 NaN exactly as GNU's Fortran does.
            let t = if rho > 0. {
                (dist / rho).sqrt()
            } else {
                f64::NAN
            };
            w[i] = (weights[index] * (1. - t.powi(3)).powi(3)).sqrt();
            let delta: Vec<_> = self.x[index].iter().zip(q).map(|(x, q)| x - q).collect();
            for (j, b) in self.basis(&delta).iter().enumerate() {
                a[i][j] = w[i] * b;
            }
        }
        if w.iter().any(|w| w.is_nan()) {
            // GNU's NaN kernel weights fail every tol/sigma comparison in
            // ehg127, which flushes the whole coefficient vector to zero.
            return Ok(vec![vec![0.; n]; d + 1]);
        }
        if w.iter().all(|w| *w == 0.) {
            execution.warn_values("at ", q);
            execution.warn_values("radius ", &[rho]);
            execution.warn("all data on boundary of neighborhood. make span bigger");
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
        execution.checkpoint()?;
        let (pinv, ratio) = pseudoinverse_with_condition(&a)?;
        execution.note_singular(ratio, q, rho);
        // loesswarn(104)'s condition: with more terms than observations the
        // underdetermined solve is rank deficient, and GNU's dqrsl leaves
        // NaN coefficients for that neighborhood.
        if k > nf - 1 && w.iter().all(|w| *w == 0.) {
            return Ok(vec![vec![f64::NAN; n]; d + 1]);
        }
        execution.checkpoint()?;
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
fn pseudoinverse_with_condition(a: &[Vec<f64>]) -> Result<(Vec<Vec<f64>>, f64), String> {
    let (m, n) = (a.len(), a[0].len());
    let matrix = faer::Mat::from_fn(m, n, |i, j| a[i][j]);
    let svd = matrix
        .svd()
        .map_err(|_| "LOESS singular value decomposition failed")?;
    let s = svd.S();
    let u = svd.U();
    let v = svd.V();
    let tolerance = s[0] * 100. * f64::EPSILON;
    // GNU's rcond candidate: sigma(k)/sigma(1) of the equilibrated design.
    let ratio = if s[0] > 0. {
        s[m.min(n) - 1] / s[0]
    } else {
        f64::NAN
    };
    Ok((
        (0..n)
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
            .collect(),
        ratio,
    ))
}

// One-sided Jacobi SVD keeps this owned kernel available with the system
// backend too, without raw LAPACK calls or squaring the condition number.
#[cfg(not(feature = "rust-backend"))]
fn pseudoinverse_with_condition(a: &[Vec<f64>]) -> Result<(Vec<Vec<f64>>, f64), String> {
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
    // Columns hold squared singular values; GNU's rcond is sigma(k)/sigma(1).
    let max = sigma.iter().copied().fold(0., f64::max);
    let min = sigma.iter().copied().fold(f64::INFINITY, f64::min);
    let ratio = if max > 0. {
        (min / max).sqrt()
    } else {
        f64::NAN
    };
    Ok((
        (0..n)
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
            .collect(),
        ratio,
    ))
}

#[cfg(test)]
mod tests;
