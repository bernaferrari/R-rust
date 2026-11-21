/*
 * Copyright (C) 1998--2025  The R Core Team
 *
 * The authors of this software are Cleveland, Grosse, and Shyu.
 * Copyright (c) 1989, 1992 by AT&T.
 * Permission to use, copy, modify, and distribute this software for any
 * purpose without fee is hereby granted, provided that this entire notice
 * is included in all copies of any software which is or includes a copy
 * or modification of this software and in all copies of the supporting
 * documentation for such software.
 * THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR IMPLIED
 * WARRANTY.  IN PARTICULAR, NEITHER THE AUTHORS NOR AT&T MAKE ANY
 * REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE MERCHANTABILITY
 * OF THIS SOFTWARE OR ITS FITNESS FOR A PARTICULAR PURPOSE.
 *
 * Ported from r-source/src/library/stats/src/loessc.c
 */

use std::ffi::CStr;
use std::os::raw::{c_double, c_int};

use crate::mainutils::native_routines::buffers::{Args, BufferError, NativeBuffer};
use crate::sexp::instance::with_required_current_instance;

fn column_major_rows(x: &[c_double], n: usize, d: usize) -> Vec<Vec<f64>> {
    (0..n)
        .map(|i| (0..d).map(|j| x[i + j * n]).collect())
        .collect()
}

/// The `surf_stat` string of `.C(C_loess_raw)`, e.g. `"interpolate/1.approx"`.
fn surf_text(surf_stat: &[Vec<u8>]) -> String {
    surf_stat
        .first()
        .and_then(|bytes| CStr::from_bytes_until_nul(bytes).ok())
        .map(|text| text.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
fn fit_loess_model(
    y: &[c_double],
    x: &[c_double],
    weights: &[c_double],
    d: c_int,
    n: c_int,
    span: f64,
    degree: c_int,
    nonparametric: c_int,
    drop_square: &[c_int],
    cell: f64,
    interpolate: bool,
    exact: bool,
    approximate_trace: bool,
) -> Result<super::loess::Model, String> {
    let n = n as usize;
    let d = d as usize;
    if n == 0 || d == 0 {
        return Err("invalid LOESS arguments".into());
    }
    let np = (nonparametric as usize).min(d);
    let parametric = (0..d).map(|j| j >= np).collect();
    let drop = (0..d)
        .map(|j| drop_square.get(j).is_some_and(|&flag| flag == 1))
        .collect();
    let config = super::loess::Config {
        span,
        degree: (degree as usize).min(2),
        normalize: false,
        parametric,
        drop_square: drop,
        interpolate,
        cell: if cell.is_finite() && cell > 0.0 {
            cell
        } else {
            0.2
        },
        iterations: 1,
        exact,
        approximate_trace,
    };
    let execution = super::loess::Execution::new(&|| Ok(()));
    let model = super::loess::Model::fit_with_execution(
        column_major_rows(x, n, d),
        y[..n].to_vec(),
        weights[..n].to_vec(),
        config,
        &execution,
    );
    match model {
        Ok(model) => {
            // GNU emits ehg131's diagnostics through warning() during the
            // fit; replay the recorded stream in emission order.
            for message in execution.take_warnings() {
                let mut bytes = message.into_bytes();
                bytes.push(0);

                // GNU's .C pushes no warning-visible context, so
                // warning() inside the routine attributes to the
                // calling closure (findCall = simpleLoess(...)),
                // not the .C(...) expression.
                crate::mainutils::errors::warningcall_cstr(
                    crate::mainutils::errors::findCall(),
                    std::ffi::CStr::from_bytes_until_nul(&bytes).unwrap_or_default(),
                );
            }
            Ok(model)
        }
        Err(message) => Err(message),
    }
}

/// Publish `values` into the leading cells of an output buffer.
fn write_fit<T: Copy>(dest: &mut [T], values: &[T]) {
    let Some(dest) = dest.get_mut(..values.len()) else {
        fail_loess("LOESS output buffer is too short");
    };
    dest.copy_from_slice(values);
}

fn same_rows(model: &super::loess::Model, queries: &[Vec<f64>]) -> bool {
    model.x.len() == queries.len()
        && model.x.iter().zip(queries).all(|(left, right)| {
            left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a == b)
        })
}

fn predict_model(model: &super::loess::Model, queries: &[Vec<f64>]) -> Result<Vec<f64>, String> {
    if same_rows(model, queries) {
        return Ok(model.fitted.clone());
    }
    model
        .predict_with_execution(queries, false, &super::loess::Execution::new(&|| Ok(())))
        .map(|(fitted, _)| fitted)
}

/// Shared fit description of the `.C` LOESS entry points.
struct FitSpec<'a> {
    y: &'a [c_double],
    x: &'a [c_double],
    weights: &'a [c_double],
    span: c_double,
    degree: c_int,
    nonparametric: c_int,
    drop_square: &'a [c_int],
    d: c_int,
    n: c_int,
}

impl FitSpec<'_> {
    fn fit(
        &self,
        weights: &[c_double],
        cell: f64,
        interpolate: bool,
        exact: bool,
        approximate_trace: bool,
    ) -> Result<super::loess::Model, String> {
        fit_loess_model(
            self.y,
            self.x,
            weights,
            self.d,
            self.n,
            self.span,
            self.degree,
            self.nonparametric,
            self.drop_square,
            cell,
            interpolate,
            exact,
            approximate_trace,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn engine_loess_se(
    spec: &FitSpec<'_>,
    x_evaluate: &[c_double],
    cell: c_double,
    interpolate: bool,
    fit_weights: Option<&[c_double]>,
    m: c_int,
    fit: &mut [c_double],
    leverages: &mut [c_double],
) {
    let span_v = spec.span;
    // R passes span*cell for loess_ise; the model's kd-tree threshold is
    // floor(n * span * config.cell), so undo the span pre-multiplication.
    let cell_v = if span_v > 0.0 { cell / span_v } else { cell };
    let model = match spec.fit(spec.weights, cell_v, interpolate, false, false) {
        Ok(model) => model,
        Err(message) => fail_loess(&message),
    };
    let mm = m.max(0) as usize;
    let nn = spec.n.max(0) as usize;
    let queries = column_major_rows(x_evaluate, mm, spec.d as usize);
    let execution = super::loess::Execution::new(&|| Ok(()));
    let influence = model
        .prediction_influence(&queries, &execution)
        .unwrap_or_else(|message| fail_loess(&message));
    // loess_dfitse's SYMMETRIC branch refits the point estimates with the
    // robustness weights alone (lowesf's second call in loessc.c).
    let fitted = if let Some(fit_weights) = fit_weights {
        match spec.fit(fit_weights, cell_v, false, false, false) {
            Ok(fit_model) => match predict_model(&fit_model, &queries) {
                Ok(values) => values,
                Err(message) => fail_loess(&message),
            },
            Err(message) => fail_loess(&message),
        }
    } else {
        influence
            .iter()
            .map(|row| {
                row.iter()
                    .zip(&model.y)
                    .map(|(coefficient, value)| coefficient * value)
                    .sum()
            })
            .collect()
    };
    // GNU loess_ise publishes only L; its fit argument is untouched.
    if !interpolate {
        write_fit(fit, &fitted);
    }
    if leverages.len() < mm * nn {
        fail_loess("LOESS operator buffer is too short");
    }
    for j in 0..nn {
        for i in 0..mm {
            leverages[i + j * mm] = influence[i][j];
        }
    }
}

fn fail_loess(message: &str) -> ! {
    let mut bytes = message.as_bytes().to_vec();
    bytes.push(0);

    crate::main::errors::Rf_error_cstr(
        std::ffi::CStr::from_bytes_until_nul(&bytes).unwrap_or_default(),
    );
    unreachable!();
}

/// `.C(C_loess_raw, y, x, weights, robust, d, n, span, degree, nonparametric,
/// drop.square, sum.drop.sqr, cell, surf.stat, surface, parameter, a, xi,
/// vert, vval, diagonal, trL, one.delta, two.delta, setLf)`.
pub(crate) fn loess_raw(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    let mut a = Args::new(b);
    let y = &*a.real();
    let x = &*a.real();
    let weights = &*a.real();
    let robust = &*a.real();
    let d = a.int()[0];
    let n = a.int()[0];
    let span = a.real()[0];
    let degree = a.int()[0];
    let nonparametric = a.int()[0];
    let drop_square = &*a.int();
    let _sum_drop_sqr = a.int();
    let cell = a.real()[0];
    let surf = surf_text(a.chars());
    let surface = a.real();
    let parameter = a.int();
    let a_out = a.int();
    let xi = a.real();
    let vert = a.real();
    let vval = a.real();
    let diagonal = a.real();
    let tr_l = &mut a.real()[0];
    let one_delta = &mut a.real()[0];
    let two_delta = &mut a.real()[0];

    let weight_values = if surf.ends_with("/none") {
        robust
    } else {
        weights
    };
    let spec = FitSpec {
        y,
        x,
        weights: weight_values,
        span,
        degree,
        nonparametric,
        drop_square,
        d,
        n,
    };
    let cell_v = if span > 0.0 { cell / span } else { cell };
    let model = match spec.fit(
        weight_values,
        cell_v,
        surf.starts_with("interpolate"),
        surf.ends_with("/exact"),
        surf.contains("2.approx"),
    ) {
        Ok(model) => model,
        Err(message) => fail_loess(&message),
    };
    write_fit(surface, &model.fitted);
    let no_statistics = surf.ends_with("/none");
    *tr_l = if no_statistics { 0.0 } else { model.trace };
    if !no_statistics {
        *one_delta = model.delta1;
        *two_delta = model.delta2;
    }
    if matches!(
        surf.as_str(),
        "direct/approximate" | "direct/exact" | "interpolate/1.approx" | "interpolate/exact"
    ) {
        write_fit(
            diagonal,
            model
                .training_diagonal
                .as_deref()
                .unwrap_or_else(|| fail_loess("missing LOESS training diagonal")),
        );
    }
    if model.config.interpolate {
        let surface = super::loess::pruned::PrunedSurface::fit(
            &model,
            &super::loess::Execution::new(&|| Ok(())),
        )
        .unwrap_or_else(|message| fail_loess(&message));
        write_fit(parameter, &surface.parameter);
        write_fit(a_out, &surface.a);
        write_fit(xi, &surface.xi);
        write_fit(vert, &surface.vert);
        write_fit(vval, &surface.vval);
    }
    Ok(())
}

/// `.C(C_loess_dfit, y, x, x_evaluate, weights, span, degree, nonparametric,
/// drop.square, sum.drop.sqr, d, n, m, fit)`.
pub(crate) fn loess_dfit(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    let mut a = Args::new(b);
    let y = &*a.real();
    let x = &*a.real();
    let x_evaluate = &*a.real();
    let weights = &*a.real();
    let span = a.real()[0];
    let degree = a.int()[0];
    let nonparametric = a.int()[0];
    let drop_square = &*a.int();
    let _sum_drop_sqr = a.int();
    let d = a.int()[0];
    let n = a.int()[0];
    let m = a.int()[0];
    let fit = a.real();
    let spec = FitSpec {
        y,
        x,
        weights,
        span,
        degree,
        nonparametric,
        drop_square,
        d,
        n,
    };
    let model = match spec.fit(weights, 0.2, false, false, false) {
        Ok(model) => model,
        Err(message) => fail_loess(&message),
    };
    let queries = column_major_rows(x_evaluate, m as usize, d as usize);
    match predict_model(&model, &queries) {
        Ok(values) => write_fit(fit, &values),
        Err(message) => fail_loess(&message),
    }
    Ok(())
}

/// `.C(C_loess_ise, y, x, x_evaluate, weights, span, degree, nonparametric,
/// drop.square, sum.drop.sqr, cell, d, n, m, fit, L)`.
pub(crate) fn loess_ise(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    let mut a = Args::new(b);
    let y = &*a.real();
    let x = &*a.real();
    let x_evaluate = &*a.real();
    let weights = &*a.real();
    let span = a.real()[0];
    let degree = a.int()[0];
    let nonparametric = a.int()[0];
    let drop_square = &*a.int();
    let _sum_drop_sqr = a.int();
    let cell = a.real()[0];
    let d = a.int()[0];
    let n = a.int()[0];
    let m = a.int()[0];
    let fit = a.real();
    let leverages = a.real();
    // loess_ise rebuilds the k-d tree with v[1] = span*cell and computes
    // L by interpolating the vertex influence rows (lowesb + lowesl);
    // se.fit divides by the raw weights.
    let spec = FitSpec {
        y,
        x,
        weights,
        span,
        degree,
        nonparametric,
        drop_square,
        d,
        n,
    };
    engine_loess_se(&spec, x_evaluate, cell, true, None, m, fit, leverages);
    Ok(())
}

/// `.C(C_loess_dfitse, y, x, x_evaluate, weights, robust, family, span,
/// degree, nonparametric, drop.square, sum.drop.sqr, d, n, m, fit, L)`.
pub(crate) fn loess_dfitse(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    let mut a = Args::new(b);
    let y = &*a.real();
    let x = &*a.real();
    let x_evaluate = &*a.real();
    let weights = &*a.real();
    let robust = &*a.real();
    let family = a.int()[0];
    let span = a.real()[0];
    let degree = a.int()[0];
    let nonparametric = a.int()[0];
    let drop_square = &*a.int();
    let _sum_drop_sqr = a.int();
    let d = a.int()[0];
    let n = a.int()[0];
    let m = a.int()[0];
    let fit = a.real();
    let leverages = a.real();
    // GNU computes L with weights*robust, then refits point estimates
    // with robustness weights alone for the symmetric family.
    let symmetric = family != 1;
    let spec = FitSpec {
        y,
        x,
        weights,
        span,
        degree,
        nonparametric,
        drop_square,
        d,
        n,
    };
    engine_loess_se(
        &spec,
        x_evaluate,
        0.2,
        false,
        symmetric.then_some(robust),
        m,
        fit,
        leverages,
    );
    Ok(())
}

/// `.Fortran(C_lowesw, residuals, n, robust, iwork)`.
pub(crate) fn lowesw(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    let mut a = Args::new(b);
    let residuals = &*a.real();
    let n = a.int()[0] as usize;
    let robust = a.real();
    let iwork = a.int();
    let (values, work) =
        super::loess::robust::weights(&residuals[..n]).unwrap_or_else(|message| fail_loess(message));
    write_fit(robust, &values);
    write_fit(iwork, &work);
    Ok(())
}

/// `.Fortran(C_lowesp, n, y, fitted, weights, robust, iwork, pseudo)`.
pub(crate) fn lowesp(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    let mut a = Args::new(b);
    let n = a.int()[0] as usize;
    let y = &a.real()[..n];
    let fitted = &a.real()[..n];
    let weights = &a.real()[..n];
    let robust = &a.real()[..n];
    let iwork = a.int();
    let pseudo = a.real();
    let (values, work) = super::loess::robust::pseudovalues(y, fitted, weights, robust)
        .unwrap_or_else(|message| fail_loess(message));
    write_fit(pseudo, &values);
    write_fit(iwork, &work);
    Ok(())
}

pub(crate) struct LoessWorkspaceState {
    iv: Vec<c_int>,
    v: Vec<c_double>,
    liv: c_int,
    lv: c_int,
    tau: c_int,
}

impl Default for LoessWorkspaceState {
    fn default() -> Self {
        Self {
            iv: Vec::new(),
            v: Vec::new(),
            liv: 0,
            lv: 0,
            tau: 0,
        }
    }
}

impl LoessWorkspaceState {
    fn clear(&mut self) {
        self.iv = Vec::new();
        self.v = Vec::new();
        self.liv = 0;
        self.lv = 0;
    }

    fn allocate(&mut self, liv: c_int, lv: c_int) {
        self.liv = liv;
        self.lv = lv;
        self.iv = vec![0; liv as usize];
        self.v = vec![0.0; lv as usize];
    }
}

fn with_loess_workspace_state<R>(f: impl FnOnce(&mut LoessWorkspaceState) -> R) -> R {
    with_required_current_instance(|instance| f(unsafe { &mut (*instance).loess_workspace_state }))
}


#[cfg(test)]
fn loess_free() {
    with_loess_workspace_state(LoessWorkspaceState::clear);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loess_legacy_workspaces_are_runtime_local() {
        use crate::sexp::session::RSession;
        let first = RSession::new_for_gc_tests();
        let second = RSession::new_for_gc_tests();
        first.with_active(|| {
            with_loess_workspace_state(|state| {
                state.allocate(64, 64);
                state.iv[10] = 37;
                state.v[10] = 2.5;
            })
        });
        second.with_active(|| {
            with_loess_workspace_state(|state| {
                assert!(state.iv.is_empty());
                state.allocate(64, 64);
                state.iv[10] = 73;
            })
        });
        first.with_active(|| {
            with_loess_workspace_state(|state| {
                assert_eq!(state.iv[10], 37);
                assert_eq!(state.v[10], 2.5);
            })
        });
        drop(first);
        second.with_active(|| with_loess_workspace_state(|state| assert_eq!(state.iv[10], 73)));
    }

    #[test]
    fn loess_workspace_is_session_local_and_owned() {
        use crate::sexp::session::RSession;
        let first = RSession::new_for_gc_tests();
        let second = RSession::new_for_gc_tests();
        first.with_active(|| {
            with_loess_workspace_state(|state| {
                state.allocate(100, 100);
                state.tau = 6;
                assert!(!state.iv.is_empty());
                assert!(!state.v.is_empty());
                assert_eq!(state.tau, 6);
            });
        });
        second.with_active(|| {
            with_loess_workspace_state(|state| {
                assert!(state.iv.is_empty());
                assert!(state.v.is_empty());
                state.allocate(50, 50);
                assert!(!state.iv.is_empty());
                assert!(!state.v.is_empty());
            });
            loess_free();
            with_loess_workspace_state(|state| {
                assert!(state.iv.is_empty());
                assert!(state.v.is_empty());
            });
        });
        first.with_active(|| {
            with_loess_workspace_state(|state| {
                assert!(!state.iv.is_empty());
                assert!(!state.v.is_empty());
                assert_eq!(state.tau, 6);
            });
        });
    }
}

#[cfg(test)]
mod native_output_tests;
