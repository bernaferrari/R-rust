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

use std::cmp;
use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};

use crate::sexp::ffi::*;
use crate::sexp::instance::with_required_current_instance;

const GAUSSIAN: c_int = 1;
const SYMMETRIC: c_int = 0;
use std::cell::{Cell, RefCell};

thread_local! {
    static PREDICT_MODEL: RefCell<Option<super::loess::Model>> = const { RefCell::new(None) };
    /// The pseudovalue refit after `lowesp` must not replace the model
    /// `predict.loess` interpolates. `simpleLoess` always calls it second.
    static SKIP_PREDICT_STORE: Cell<bool> = const { Cell::new(false) };
}

fn loess_median(mut values: Vec<f64>) -> f64 {
    values.retain(|v| v.is_finite());
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n % 2 == 0 {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    } else {
        values[n / 2]
    }
}

fn column_major_rows(x: *mut c_double, n: usize, d: usize) -> Vec<Vec<f64>> {
    unsafe {
        (0..n)
            .map(|i| (0..d).map(|j| *x.add(i + j * n)).collect())
            .collect()
    }
}

fn copy_vec(ptr: *mut c_double, n: usize) -> Vec<f64> {
    unsafe { (0..n).map(|i| *ptr.add(i)).collect() }
}

fn surf_text(surf_stat: *mut *mut c_char) -> String {
    unsafe {
        if surf_stat.is_null() || (*surf_stat).is_null() {
            return String::new();
        }
        CStr::from_ptr(*surf_stat).to_string_lossy().into_owned()
    }
}

fn fit_loess_model(
    y: *mut c_double,
    x: *mut c_double,
    weights: *mut c_double,
    d: c_int,
    n: c_int,
    span: f64,
    degree: c_int,
    nonparametric: c_int,
    drop_square: *mut c_int,
    cell: f64,
    interpolate: bool,
    exact: bool,
    approximate_trace: bool,
) -> Result<super::loess::Model, String> {
    let n = n as usize;
    let d = d as usize;
    if n == 0 || d == 0 || y.is_null() || x.is_null() || weights.is_null() {
        return Err("invalid LOESS arguments".into());
    }
    let np = (nonparametric as usize).min(d);
    let parametric = (0..d).map(|j| j >= np).collect();
    let drop = unsafe {
        (0..d)
            .map(|j| !drop_square.is_null() && *drop_square.add(j) == 1)
            .collect()
    };
    let config = super::loess::Config {
        span,
        degree: (degree as usize).min(2),
        normalize: false,
        parametric,
        drop_square: drop,
        interpolate,
        cell: if cell.is_finite() && cell > 0.0 { cell } else { 0.2 },
        iterations: 1,
        exact,
        approximate_trace,
    };
    super::loess::Model::fit_with_execution(
        column_major_rows(x, n, d),
        copy_vec(y, n),
        copy_vec(weights, n),
        config,
        &super::loess::Execution::new(&|| Ok(())),
    )
}

fn store_predict_model(model: super::loess::Model) {
    let skip = SKIP_PREDICT_STORE.with(|flag| flag.replace(false));
    if skip {
        return;
    }
    PREDICT_MODEL.with(|slot| *slot.borrow_mut() = Some(model));
}

fn write_fit(dest: *mut c_double, values: &[f64]) {
    unsafe {
        if dest.is_null() {
            return;
        }
        for (i, value) in values.iter().enumerate() {
            *dest.add(i) = *value;
        }
    }
}

fn same_rows(model: &super::loess::Model, queries: &[Vec<f64>]) -> bool {
    model.x.len() == queries.len()
        && model
            .x
            .iter()
            .zip(queries)
            .all(|(left, right)| left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a == b))
}

fn predict_model(model: &super::loess::Model, queries: &[Vec<f64>]) -> Result<Vec<f64>, String> {
    if same_rows(model, queries) {
        return Ok(model.fitted.clone());
    }
    model
        .predict_with_execution(queries, false, &super::loess::Execution::new(&|| Ok(())))
        .map(|(fitted, _)| fitted)
}
fn fail_loess(message: &str) -> ! {
    let mut bytes = message.as_bytes().to_vec();
    bytes.push(0);
    unsafe {
        crate::main::errors::Rf_error(bytes.as_ptr() as *const c_char);
        unreachable!();
    }
}

fn engine_loess_raw(
    y: *mut c_double,
    x: *mut c_double,
    weights: *mut c_double,
    robust: *mut c_double,
    d: *mut c_int,
    n: *mut c_int,
    span: *mut c_double,
    degree: *mut c_int,
    nonparametric: *mut c_int,
    drop_square: *mut c_int,
    cell: *mut c_double,
    surf_stat: *mut *mut c_char,
    surface: *mut c_double,
    parameter: *mut c_int,
    tr_l: *mut c_double,
    one_delta: *mut c_double,
    two_delta: *mut c_double,
) {
    unsafe {
        let surf = surf_text(surf_stat);
        let weight_ptr = if surf.ends_with("/none") { robust } else { weights };
        let span_v = *span;
        let cell_v = if span_v > 0.0 { *cell / span_v } else { *cell };
        let model = fit_loess_model(
            y,
            x,
            weight_ptr,
            *d,
            *n,
            span_v,
            *degree,
            *nonparametric,
            drop_square,
            cell_v,
            surf.starts_with("interpolate"),
            surf.ends_with("/exact"),
            surf.contains("2.approx"),
        );
        let model = match model {
            Ok(model) => model,
            Err(message) => fail_loess(&message),
        };
        write_fit(surface, &model.fitted);
        if !tr_l.is_null() {
            *tr_l = model.trace;
        }
        if !one_delta.is_null() {
            *one_delta = if model.delta1 == 0.0 { 1.0 } else { model.delta1 };
        }
        if !two_delta.is_null() {
            *two_delta = model.delta2;
        }
        if !parameter.is_null() {
            *parameter = *d;
            *parameter.add(1) = *n;
            *parameter.add(2) = 1;
            *parameter.add(3) = 1;
            *parameter.add(4) = 1;
            *parameter.add(5) = 1;
            *parameter.add(6) = 1;
        }
        store_predict_model(model);
    }
}

fn engine_loess_dfit(
    y: *mut c_double,
    x: *mut c_double,
    x_evaluate: *mut c_double,
    weights: *mut c_double,
    span: *mut c_double,
    degree: *mut c_int,
    nonparametric: *mut c_int,
    drop_square: *mut c_int,
    d: *mut c_int,
    n: *mut c_int,
    m: *mut c_int,
    fit: *mut c_double,
) {
    unsafe {
        let model = fit_loess_model(
            y,
            x,
            weights,
            *d,
            *n,
            *span,
            *degree,
            *nonparametric,
            drop_square,
            0.2,
            false,
            false,
            false,
        );
        let model = match model {
            Ok(model) => model,
            Err(message) => fail_loess(&message),
        };
        let queries = column_major_rows(x_evaluate, *m as usize, *d as usize);
        match predict_model(&model, &queries) {
            Ok(values) => write_fit(fit, &values),
            Err(message) => fail_loess(&message),
        }
    }
}

fn engine_loess_ifit(m: *mut c_int, x_evaluate: *mut c_double, fit: *mut c_double) {
    let model = PREDICT_MODEL.with(|slot| slot.borrow().clone());
    let Some(model) = model else {
        fail_loess("no LOESS model to interpolate");
    };
    unsafe {
        let d = model.x.first().map(Vec::len).unwrap_or(0);
        let queries = column_major_rows(x_evaluate, *m as usize, d);
        match predict_model(&model, &queries) {
            Ok(values) => write_fit(fit, &values),
            Err(message) => fail_loess(&message),
        }
    }
}

pub unsafe extern "C" fn c_lowesw(
    residuals: *mut std::ffi::c_void,
    n: *mut std::ffi::c_void,
    robust: *mut std::ffi::c_void,
    _iwork: *mut std::ffi::c_void,
) {
    unsafe {
        let residuals = residuals as *mut c_double;
        let robust = robust as *mut c_double;
        let n = *(n as *mut c_int) as usize;
        let abs: Vec<f64> = (0..n).map(|i| (*residuals.add(i)).abs()).collect();
        let cmad = 6.0 * loess_median(abs);
        for i in 0..n {
            let r = (*residuals.add(i)).abs();
            *robust.add(i) = if cmad < f64::MIN_POSITIVE || r <= cmad * 0.001 {
                1.0
            } else if r > cmad * 0.999 {
                0.0
            } else {
                (1.0 - (r / cmad).powi(2)).powi(2)
            };
        }
    }
}

pub unsafe extern "C" fn c_lowesp(
    n: *mut std::ffi::c_void,
    y: *mut std::ffi::c_void,
    fitted: *mut std::ffi::c_void,
    weights: *mut std::ffi::c_void,
    robust: *mut std::ffi::c_void,
    _iwork: *mut std::ffi::c_void,
    pseudo: *mut std::ffi::c_void,
) {
    unsafe {
        let n = *(n as *mut c_int) as usize;
        let y = y as *mut c_double;
        let fitted = fitted as *mut c_double;
        let weights = weights as *mut c_double;
        let robust = robust as *mut c_double;
        let pseudo = pseudo as *mut c_double;
        let residuals: Vec<f64> = (0..n).map(|i| *y.add(i) - *fitted.add(i)).collect();
        let mad = loess_median(
            residuals
                .iter()
                .enumerate()
                .map(|(i, r)| r.abs() * (*weights.add(i)).sqrt())
                .collect(),
        );
        let c = (6.0 * mad).powi(2) / 5.0;
        let scale = if c == 0.0 {
            1.0
        } else {
            n as f64
                / (0..n)
                    .map(|i| {
                        let r = residuals[i];
                        let w = *weights.add(i);
                        let rw = *robust.add(i);
                        (1.0 - r * r * w / c) * rw.sqrt()
                    })
                    .sum::<f64>()
                    .max(f64::MIN_POSITIVE)
        };
        for i in 0..n {
            *pseudo.add(i) = *fitted.add(i) + scale * *robust.add(i) * residuals[i];
        }
    }
    SKIP_PREDICT_STORE.with(|flag| flag.set(true));
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

    fn allocate(&mut self, liv: c_int, lv: c_int) -> (*mut c_int, *mut c_double) {
        self.liv = liv;
        self.lv = lv;
        self.iv = vec![0; liv as usize];
        self.v = vec![0.0; lv as usize];
        (self.iv.as_mut_ptr(), self.v.as_mut_ptr())
    }

    fn ptrs(&mut self) -> (*mut c_int, *mut c_double) {
        (self.iv.as_mut_ptr(), self.v.as_mut_ptr())
    }
}

fn with_loess_workspace_state<R>(f: impl FnOnce(&mut LoessWorkspaceState) -> R) -> R {
    with_required_current_instance(|instance| f(unsafe { &mut (*instance).loess_workspace_state }))
}

fn r_min<T: Ord>(a: T, b: T) -> T {
    if a < b { a } else { b }
}
fn r_max<T: Ord>(a: T, b: T) -> T {
    if a > b { a } else { b }
}

unsafe fn loess_free() {
    with_loess_workspace_state(LoessWorkspaceState::clear);
}

// These are R's LOESS routines, not LAPACK symbols. Neither numerical
// backend supplies them yet; fail explicitly instead of returning fake fits.
mod loess_stubs {
    use std::os::raw::{c_double, c_int};
    pub unsafe fn lowesd(
        _iv: *mut c_int,
        _liv: *mut c_int,
        _lv: *mut c_int,
        _v: *mut c_double,
        _d: *mut c_int,
        _n: *mut c_int,
        _f: *mut c_double,
        _ideg: *mut c_int,
        _nf: *mut c_int,
        _nvmax: *mut c_int,
        _setlf: *mut c_int,
    ) {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "LOESS native routines are not implemented".into(),
        });
    }
    pub unsafe fn lowesa(
        _trL: *mut c_double,
        _n: *mut c_int,
        _d: *mut c_int,
        _tau: *mut c_int,
        _nsing: *mut c_int,
        _one_delta: *mut c_double,
        _two_delta: *mut c_double,
    ) {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "LOESS native routines are not implemented".into(),
        });
    }
    pub unsafe fn lowesb(
        _x: *mut c_double,
        _y: *mut c_double,
        _robust: *mut c_double,
        _diagonal: *mut c_double,
        _i1: *mut c_int,
        _iv: *mut c_int,
        _v: *mut c_double,
    ) {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "LOESS native routines are not implemented".into(),
        });
    }
    pub unsafe fn lowese(
        _iv: *mut c_int,
        _v: *mut c_double,
        _n: *mut c_int,
        _x: *mut c_double,
        _surface: *mut c_double,
    ) {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "LOESS native routines are not implemented".into(),
        });
    }
    pub unsafe fn lowesf(
        _x: *mut c_double,
        _y: *mut c_double,
        _weights: *mut c_double,
        _iv: *mut c_int,
        _v: *mut c_double,
        _m: *mut c_int,
        _x_evaluate: *mut c_double,
        _diagonal: *mut c_double,
        _i2: *mut c_int,
        _surface: *mut c_double,
    ) {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "LOESS native routines are not implemented".into(),
        });
    }
    pub unsafe fn lowesl(
        _iv: *mut c_int,
        _v: *mut c_double,
        _m: *mut c_int,
        _x: *mut c_double,
        _L: *mut c_double,
    ) {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "LOESS native routines are not implemented".into(),
        });
    }
    pub unsafe fn lowesc(
        _n: *mut c_int,
        _hat_matrix: *mut c_double,
        _LL: *mut c_double,
        _trL: *mut c_double,
        _one_delta: *mut c_double,
        _two_delta: *mut c_double,
    ) {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "LOESS native routines are not implemented".into(),
        });
    }
    pub unsafe fn ehg169(
        _d: *mut c_int,
        _vc: *mut c_int,
        _nc: *mut c_int,
        _nc2: *mut c_int,
        _nv: *mut c_int,
        _nv2: *mut c_int,
        _vert: *mut c_double,
        _a: *mut c_int,
        _xi: *mut c_double,
        _lv1: *mut c_int,
        _lv2: *mut c_int,
        _lv3: *mut c_int,
    ) {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "LOESS native routines are not implemented".into(),
        });
    }
    pub unsafe fn ehg196(
        _tau: *mut c_int,
        _d: *mut c_int,
        _span: *mut c_double,
        _trL: *mut c_double,
    ) {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "LOESS native routines are not implemented".into(),
        });
    }
}
use loess_stubs::*;

fn R_pow_di(x: c_double, n: c_int) -> c_double {
    crate::nmath::special::mlutils::R_pow_di(x, n)
}

fn strcmp_c(s1: &str, s2: &str) -> bool {
    s1 == s2
}

pub unsafe extern "C" fn loess_raw(
    y: *mut c_double,
    x: *mut c_double,
    weights: *mut c_double,
    robust: *mut c_double,
    d: *mut c_int,
    n: *mut c_int,
    span: *mut c_double,
    degree: *mut c_int,
    nonparametric: *mut c_int,
    drop_square: *mut c_int,
    sum_drop_sqr: *mut c_int,
    cell: *mut c_double,
    surf_stat: *mut *mut c_char,
    surface: *mut c_double,
    parameter: *mut c_int,
    a: *mut c_int,
    xi: *mut c_double,
    vert: *mut c_double,
    vval: *mut c_double,
    diagonal: *mut c_double,
    trL: *mut c_double,
    one_delta: *mut c_double,
    two_delta: *mut c_double,
    setLf: *mut c_int,
) {
    unsafe {
        engine_loess_raw(
            y, x, weights, robust, d, n, span, degree, nonparametric, drop_square, cell,
            surf_stat, surface, parameter, trL, one_delta, two_delta,
        );
        return;
        use crate::main::errors::Rf_error;

        let mut i0: c_int = 0;
        let mut one: c_int = 1;
        let mut two: c_int = 2;
        let mut d0: c_double = 0.0;

        *trL = 0.0;

        loess_workspace(
            *d,
            *n,
            *span,
            *degree,
            *nonparametric,
            drop_square,
            *sum_drop_sqr,
            *setLf != 0,
        );
        let (iv, v, mut tau) = with_loess_workspace_state(|state| {
            let (iv, v) = state.ptrs();
            (iv, v, state.tau)
        });
        *v.add(1) = *cell;

        let surf = CStr::from_ptr(*surf_stat).to_str().unwrap_or("");

        if strcmp_c(surf, "interpolate/none") {
            lowesb(
                x,
                y,
                robust,
                std::ptr::addr_of_mut!(d0),
                std::ptr::addr_of_mut!(i0),
                iv,
                v,
            );
            lowese(iv, v, n, x, surface);
            loess_prune(parameter, a, xi, vert, vval);
        } else if strcmp_c(surf, "direct/none") {
            lowesf(
                x,
                y,
                robust,
                iv,
                v,
                n,
                x,
                std::ptr::addr_of_mut!(d0),
                std::ptr::addr_of_mut!(i0),
                surface,
            );
        } else if strcmp_c(surf, "interpolate/1.approx") {
            lowesb(x, y, weights, diagonal, std::ptr::addr_of_mut!(one), iv, v);
            lowese(iv, v, n, x, surface);
            let mut nsing = *iv.add(29);
            for i in 0..(*n as usize) {
                *trL = *trL + *diagonal.add(i);
            }
            lowesa(
                trL,
                n,
                d,
                &mut tau,
                std::ptr::addr_of_mut!(nsing),
                one_delta,
                two_delta,
            );
            loess_prune(parameter, a, xi, vert, vval);
        } else if strcmp_c(surf, "interpolate/2.approx") {
            lowesb(
                x,
                y,
                weights,
                std::ptr::addr_of_mut!(d0),
                std::ptr::addr_of_mut!(i0),
                iv,
                v,
            );
            lowese(iv, v, n, x, surface);
            let _nsing = *iv.add(29);
            ehg196(&mut tau, d, span, trL);
            let mut nsing = *iv.add(29);
            lowesa(
                trL,
                n,
                d,
                &mut tau,
                std::ptr::addr_of_mut!(nsing),
                one_delta,
                two_delta,
            );
            loess_prune(parameter, a, xi, vert, vval);
        } else if strcmp_c(surf, "direct/approximate") {
            lowesf(
                x,
                y,
                weights,
                iv,
                v,
                n,
                x,
                diagonal,
                std::ptr::addr_of_mut!(one),
                surface,
            );
            let mut nsing = *iv.add(29);
            for i in 0..(*n as usize) {
                *trL = *trL + *diagonal.add(i);
            }
            lowesa(
                trL,
                n,
                d,
                &mut tau,
                std::ptr::addr_of_mut!(nsing),
                one_delta,
                two_delta,
            );
        } else if strcmp_c(surf, "interpolate/exact") {
            let hat_matrix = vec![0.0f64; (*n as usize) * (*n as usize)];
            let mut ll = vec![0.0f64; (*n as usize) * (*n as usize)];
            lowesb(x, y, weights, diagonal, std::ptr::addr_of_mut!(one), iv, v);
            lowesl(iv, v, n, x, hat_matrix.as_ptr() as *mut c_double);
            lowesc(
                n,
                hat_matrix.as_ptr() as *mut c_double,
                ll.as_mut_ptr(),
                trL,
                one_delta,
                two_delta,
            );
            lowese(iv, v, n, x, surface);
            loess_prune(parameter, a, xi, vert, vval);
        } else if strcmp_c(surf, "direct/exact") {
            let mut hat_matrix = vec![0.0f64; (*n as usize) * (*n as usize)];
            let mut ll = vec![0.0f64; (*n as usize) * (*n as usize)];
            lowesf(
                x,
                y,
                weights,
                iv,
                v,
                n,
                x,
                hat_matrix.as_mut_ptr(),
                std::ptr::addr_of_mut!(two),
                surface,
            );
            lowesc(
                n,
                hat_matrix.as_mut_ptr(),
                ll.as_mut_ptr(),
                trL,
                one_delta,
                two_delta,
            );
            let k = (*n + 1) as usize;
            for i in 0..(*n as usize) {
                *diagonal.add(i) = *hat_matrix.as_ptr().add(i * k);
            }
        } else {
            Rf_error(b"invalid surface statistic type\0".as_ptr() as *const core::ffi::c_char);
        }
        with_loess_workspace_state(|state| state.tau = tau);
        loess_free();
    }
}

pub unsafe extern "C" fn loess_dfit(
    y: *mut c_double,
    x: *mut c_double,
    x_evaluate: *mut c_double,
    weights: *mut c_double,
    span: *mut c_double,
    degree: *mut c_int,
    nonparametric: *mut c_int,
    drop_square: *mut c_int,
    sum_drop_sqr: *mut c_int,
    d: *mut c_int,
    n: *mut c_int,
    m: *mut c_int,
    fit: *mut c_double,
) {
    unsafe {
        let _ = (sum_drop_sqr,);
        engine_loess_dfit(
            y, x, x_evaluate, weights, span, degree, nonparametric, drop_square, d, n, m, fit,
        );
        return;
        let mut i0: c_int = 0;
        let mut d0: c_double = 0.0;

        loess_workspace(
            *d,
            *n,
            *span,
            *degree,
            *nonparametric,
            drop_square,
            *sum_drop_sqr,
            false,
        );
        let (iv, v) = with_loess_workspace_state(LoessWorkspaceState::ptrs);
        lowesf(
            x,
            y,
            weights,
            iv,
            v,
            m,
            x_evaluate,
            std::ptr::addr_of_mut!(d0),
            std::ptr::addr_of_mut!(i0),
            fit,
        );
        loess_free();
    }
}

pub unsafe fn loess_dfitse(
    y: *mut c_double,
    x: *mut c_double,
    x_evaluate: *mut c_double,
    weights: *mut c_double,
    robust: *mut c_double,
    family: *mut c_int,
    span: *mut c_double,
    degree: *mut c_int,
    nonparametric: *mut c_int,
    drop_square: *mut c_int,
    sum_drop_sqr: *mut c_int,
    d: *mut c_int,
    n: *mut c_int,
    m: *mut c_int,
    fit: *mut c_double,
    L: *mut c_double,
) {
    unsafe {
        loess_workspace(
            *d,
            *n,
            *span,
            *degree,
            *nonparametric,
            drop_square,
            *sum_drop_sqr,
            false,
        );

        let mut i2: c_int = 2;
        let (iv, v) = with_loess_workspace_state(LoessWorkspaceState::ptrs);
        if *family == GAUSSIAN {
            lowesf(
                x,
                y,
                weights,
                iv,
                v,
                m,
                x_evaluate,
                L,
                std::ptr::addr_of_mut!(i2),
                fit,
            );
        } else if *family == SYMMETRIC {
            let mut i0: c_int = 0;
            let mut d0: c_double = 0.0;
            lowesf(
                x,
                y,
                weights,
                iv,
                v,
                m,
                x_evaluate,
                L,
                std::ptr::addr_of_mut!(i2),
                fit,
            );
            lowesf(
                x,
                y,
                robust,
                iv,
                v,
                m,
                x_evaluate,
                std::ptr::addr_of_mut!(d0),
                std::ptr::addr_of_mut!(i0),
                fit,
            );
        }
        loess_free();
    }
}

pub unsafe extern "C" fn loess_ifit(
    parameter: *mut c_int,
    a: *mut c_int,
    xi: *mut c_double,
    vert: *mut c_double,
    vval: *mut c_double,
    m: *mut c_int,
    x_evaluate: *mut c_double,
    fit: *mut c_double,
) {
    unsafe {
        let _ = (parameter, a, xi, vert, vval);
        engine_loess_ifit(m, x_evaluate, fit);
        return;
        loess_grow(parameter, a, xi, vert, vval);
        let (iv, v) = with_loess_workspace_state(LoessWorkspaceState::ptrs);
        lowese(iv, v, m, x_evaluate, fit);
        loess_free();
    }
}

pub unsafe fn loess_ise(
    y: *mut c_double,
    x: *mut c_double,
    x_evaluate: *mut c_double,
    weights: *mut c_double,
    span: *mut c_double,
    degree: *mut c_int,
    nonparametric: *mut c_int,
    drop_square: *mut c_int,
    sum_drop_sqr: *mut c_int,
    cell: *mut c_double,
    d: *mut c_int,
    n: *mut c_int,
    m: *mut c_int,
    fit: *mut c_double,
    L: *mut c_double,
) {
    unsafe {
        loess_workspace(
            *d,
            *n,
            *span,
            *degree,
            *nonparametric,
            drop_square,
            *sum_drop_sqr,
            true,
        );

        let mut i0: c_int = 0;
        let mut d0: c_double = 0.0;
        let (iv, v) = with_loess_workspace_state(LoessWorkspaceState::ptrs);
        *v.add(1) = *cell;
        lowesb(
            x,
            y,
            weights,
            std::ptr::addr_of_mut!(d0),
            std::ptr::addr_of_mut!(i0),
            iv,
            v,
        );
        lowesl(iv, v, m, x_evaluate, L);
        loess_free();
    }
}

/// Set per-instance tau/lv/liv and allocate workspace arrays v[1..lv], iv[1..liv].
unsafe fn loess_workspace(
    d: c_int,
    n: c_int,
    span: c_double,
    degree: c_int,
    nonparametric: c_int,
    drop_square: *const c_int,
    sum_drop_sqr: c_int,
    set_lf: bool,
) {
    unsafe {
        use crate::main::errors::Rf_error;

        let nvmax = r_max(200, n);
        let nf = r_min(n, (n as f64 * span + 1e-5).floor() as c_int);
        if nf <= 0 {
            Rf_error(b"span is too small\0".as_ptr() as *const core::ffi::c_char);
        }

        let tau0 = if degree > 1 {
            ((d + 2) * (d + 1)) / 2
        } else {
            d + 1
        };
        with_loess_workspace_state(|state| state.tau = tau0 - sum_drop_sqr);

        let dlv =
            50.0 + (3 * d + 3) as f64 * nvmax as f64 + n as f64 + (tau0 as f64 + 2.0) * nf as f64;
        let mut dliv = 50.0 + (R_pow_di(2.0, d) + 4.0) * nvmax as f64 + 2.0 * n as f64;

        let (new_lv, new_liv) = if set_lf {
            let dlv_extra = (d + 1) as f64 * nf as f64 * nvmax as f64;
            let dliv_extra = nf as f64 * nvmax as f64;
            let total_dlv = dlv + dlv_extra;
            let total_dliv = dliv + dliv_extra;

            if total_dlv < c_int::MAX as f64 && total_dliv < c_int::MAX as f64 {
                (total_dlv as c_int, total_dliv as c_int)
            } else {
                Rf_error(b"workspace required is too large\0".as_ptr() as *const core::ffi::c_char);
                unreachable!()
            }
        } else {
            if dlv < c_int::MAX as f64 && dliv < c_int::MAX as f64 {
                (dlv as c_int, dliv as c_int)
            } else {
                Rf_error(b"workspace required is too large\0".as_ptr() as *const core::ffi::c_char);
                unreachable!()
            }
        };

        let (iv, v) = with_loess_workspace_state(|state| state.allocate(new_liv, new_lv));

        let mut iset_lf = if set_lf { 1 } else { 0 };
        let mut d_out = d;
        let mut n_out = n;
        let mut span_out = span;
        let mut degree_out = degree;
        let mut nf_out = nf;
        let mut nvmax_out = nvmax;
        let mut liv_local = new_liv;
        let mut lv_local = new_lv;
        lowesd(
            iv,
            &mut liv_local,
            &mut lv_local,
            v,
            std::ptr::addr_of_mut!(d_out),
            std::ptr::addr_of_mut!(n_out),
            std::ptr::addr_of_mut!(span_out),
            std::ptr::addr_of_mut!(degree_out),
            std::ptr::addr_of_mut!(nf_out),
            std::ptr::addr_of_mut!(nvmax_out),
            std::ptr::addr_of_mut!(iset_lf),
        );
        with_loess_workspace_state(|state| {
            state.liv = liv_local;
            state.lv = lv_local;
        });
        *iv.add(32) = nonparametric;
        for i in 0..(d as usize) {
            *iv.add(40 + i) = *drop_square.add(i);
        }
    }
}

unsafe fn loess_prune(
    parameter: *mut c_int,
    a: *mut c_int,
    xi: *mut c_double,
    vert: *mut c_double,
    vval: *mut c_double,
) {
    unsafe {
        let (iv, v) = with_loess_workspace_state(LoessWorkspaceState::ptrs);
        let d = *iv.add(1);
        let vc = *iv.add(3) - 1;
        let nc = *iv.add(4);
        let nv = *iv.add(5);
        let a1 = *iv.add(6) - 1;
        let v1 = *iv.add(10) - 1;
        let xi1 = *iv.add(11) - 1;
        let vv1 = *iv.add(12) - 1;
        let nvmax = *iv.add(13);

        for i in 0..5 {
            *parameter.add(i) = *iv.add(1 + i);
        }
        *parameter.add(5) = *iv.add(21) - 1;
        *parameter.add(6) = *iv.add(14) - 1;

        for i in 0..(d as usize) {
            let k = nvmax as usize * i;
            *vert.add(i) = *v.add((v1 + k as c_int) as usize);
            *vert.add(i + d as usize) = *v.add((v1 + vc + k as c_int) as usize);
        }
        for i in 0..(nc as usize) {
            *xi.add(i) = *v.add(xi1 as usize + i);
            *a.add(i) = *iv.add(a1 as usize + i);
        }
        let k = (d + 1) * nv;
        for i in 0..(k as usize) {
            *vval.add(i) = *v.add(vv1 as usize + i);
        }
    }
}

unsafe fn loess_grow(
    parameter: *mut c_int,
    a: *mut c_int,
    xi: *mut c_double,
    vert: *mut c_double,
    vval: *mut c_double,
) {
    unsafe {
        let mut d = *parameter.add(0);
        let mut vc = *parameter.add(2);
        let mut nc = *parameter.add(3);
        let mut nv = *parameter.add(4);
        let new_liv = *parameter.add(5);
        let new_lv = *parameter.add(6);
        let (iv, v) = with_loess_workspace_state(|state| state.allocate(new_liv, new_lv));
        *iv.add(1) = d;
        *iv.add(2) = *parameter.add(1);
        *iv.add(3) = vc;
        *iv.add(5) = nv;
        *iv.add(13) = nv;
        *iv.add(4) = nc;
        *iv.add(16) = nc;
        *iv.add(6) = 50;
        *iv.add(7) = 50 + nc;
        *iv.add(8) = 50 + nc + vc * nc;
        *iv.add(9) = 50 + nc + vc * nc + nc;
        *iv.add(10) = 50;
        *iv.add(12) = 50 + nv * d;
        *iv.add(11) = 50 + nv * d + (d + 1) * nv;
        *iv.add(27) = 173;

        let v1 = *iv.add(10) - 1;
        let xi1 = *iv.add(11) - 1;
        let a1 = *iv.add(6) - 1;
        let vv1 = *iv.add(12) - 1;

        for i in 0..(d as usize) {
            let k = nv as usize * i;
            *v.add((v1 + k as c_int) as usize) = *vert.add(i);
            *v.add((v1 + vc - 1 + k as c_int) as usize) = *vert.add(i + d as usize);
        }
        for i in 0..(nc as usize) {
            *v.add(xi1 as usize + i) = *xi.add(i);
            *iv.add(a1 as usize + i) = *a.add(i);
        }
        let k = (d + 1) * nv;
        for i in 0..(k as usize) {
            *v.add(vv1 as usize + i) = *vval.add(i);
        }

        ehg169(
            std::ptr::addr_of_mut!(d),
            std::ptr::addr_of_mut!(vc),
            std::ptr::addr_of_mut!(nc),
            std::ptr::addr_of_mut!(nc),
            std::ptr::addr_of_mut!(nv),
            std::ptr::addr_of_mut!(nv),
            v.add(v1 as usize),
            iv.add(a1 as usize),
            v.add(xi1 as usize),
            iv.add(*iv.add(7) as usize - 1),
            iv.add(*iv.add(8) as usize - 1),
            iv.add(*iv.add(9) as usize - 1),
        );
    }
}

/* begin ehg's FORTRAN-callable C-codes */

pub unsafe fn loesswarn(i: *mut c_int) {
    unsafe {
        let msg = match *i {
            100 => "wrong version number in lowesd.   Probably typo in caller.",
            101 => "d>dMAX in ehg131.  Need to recompile with increased dimensions.",
            102 => "liv too small.    (Discovered by lowesd)",
            103 => "lv too small.     (Discovered by lowesd)",
            104 => "span too small.   fewer data values than degrees of freedom.",
            105 => "k>d2MAX in ehg136.  Need to recompile with increased dimensions.",
            106 => "lwork too small",
            107 => "invalid value for kernel",
            108 => "invalid value for ideg",
            109 => "lowstt only applies when kernel=1.",
            110 => "not enough extra workspace for robustness calculation",
            120 => "zero-width neighborhood. make span bigger",
            121 => "all data on boundary of neighborhood. make span bigger",
            122 => "extrapolation not allowed with blending",
            123 => "ihat=1 (diag L) in l2fit only makes sense if z=x (eval=data).",
            171 => "lowesd must be called first.",
            172 => "lowesf must not come between lowesb and lowese, lowesr, or lowesl.",
            173 => "lowesb must come before lowese, lowesr, or lowesl.",
            174 => "lowesb need not be called twice.",
            175 => "need setLf=.true. for lowesl.",
            180 => "nv>nvmax in cpvert.",
            181 => "nt>20 in eval.",
            182 => "svddc failed in l2fit.",
            183 => "didn't find edge in vleaf.",
            184 => "zero-width cell found in vleaf.",
            185 => "trouble descending to leaf in vleaf.",
            186 => "insufficient workspace for lowesf.",
            187 => "insufficient stack space",
            188 => "lv too small for computing explicit L",
            191 => "computed trace L was negative; something is wrong!",
            192 => "computed delta was negative; something is wrong!",
            193 => "workspace in loread appears to be corrupted",
            194 => "trouble in l2fit/l2tr",
            195 => "only constant, linear, or quadratic local models allowed",
            196 => "degree must be at least 1 for vertex influence matrix",
            999 => "not yet implemented",
            _ => {
                // snprintf(msg2, 50, "Assert failed; error code %d\n", *i);
                "Assert failed"
            }
        };
        crate::main::errors::Rf_warning(format!("{}\0", msg).as_ptr() as *const core::ffi::c_char);
    }
}

pub unsafe fn ehg183a(
    s: *mut c_char,
    nc: *mut c_int,
    i: *mut c_int,
    n: *mut c_int,
    inc: *mut c_int,
) {
    unsafe {
        let nnc = *nc as usize;
        let s_slice = std::slice::from_raw_parts(s as *const u8, nnc);
        let s_str = std::str::from_utf8_unchecked(s_slice);
        let mut mess = String::with_capacity(4000);
        mess.push_str(s_str);
        for j in 0..(*n as usize) {
            mess.push_str(&format!(" {}", *i.add(j * (*inc as usize))));
        }
        mess.push('\n');
        crate::main::errors::Rf_warning(format!("{}\0", mess).as_ptr() as *const core::ffi::c_char);
    }
}

pub unsafe fn ehg184a(
    s: *mut c_char,
    nc: *mut c_int,
    x: *mut c_double,
    n: *mut c_int,
    inc: *mut c_int,
) {
    unsafe {
        let nnc = *nc as usize;
        let s_slice = std::slice::from_raw_parts(s as *const u8, nnc);
        let s_str = std::str::from_utf8_unchecked(s_slice);
        let mut mess = String::with_capacity(4000);
        mess.push_str(s_str);
        for j in 0..(*n as usize) {
            mess.push_str(&format!(" {:.5}", *x.add(j * (*inc as usize))));
        }
        mess.push('\n');
        crate::main::errors::Rf_warning(format!("{}\0", mess).as_ptr() as *const core::ffi::c_char);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::instance::{RInstance, replace_current_instance};

    #[test]
    fn unported_loess_fails_explicitly() {
        let failure = std::panic::catch_unwind(|| unsafe {
            let p = std::ptr::null_mut();
            lowesd(
                p,
                p,
                p,
                std::ptr::null_mut(),
                p,
                p,
                std::ptr::null_mut(),
                p,
                p,
                p,
                p,
            );
        })
        .unwrap_err();
        assert!(
            failure
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("not implemented")
        );
    }

    #[test]
    fn loess_workspace_is_session_local_and_owned() {
        let mut first = RInstance::new();
        let mut second = RInstance::new();

        unsafe {
            let previous = replace_current_instance(Some(&mut first as *mut RInstance));
            with_loess_workspace_state(|state| {
                state.allocate(100, 100);
                state.tau = 6;
            });
            assert!(!first.loess_workspace_state.iv.is_empty());
            assert!(!first.loess_workspace_state.v.is_empty());
            assert_eq!(first.loess_workspace_state.tau, 6);
            replace_current_instance(previous);

            let previous = replace_current_instance(Some(&mut second as *mut RInstance));
            assert!(second.loess_workspace_state.iv.is_empty());
            assert!(second.loess_workspace_state.v.is_empty());
            with_loess_workspace_state(|state| state.allocate(50, 50));
            assert!(!second.loess_workspace_state.iv.is_empty());
            assert!(!second.loess_workspace_state.v.is_empty());
            loess_free();
            assert!(second.loess_workspace_state.iv.is_empty());
            assert!(second.loess_workspace_state.v.is_empty());
            replace_current_instance(previous);
        }

        assert!(!first.loess_workspace_state.iv.is_empty());
        assert!(!first.loess_workspace_state.v.is_empty());
        assert!(second.loess_workspace_state.iv.is_empty());
        assert!(second.loess_workspace_state.v.is_empty());
    }
}
