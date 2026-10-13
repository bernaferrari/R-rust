//! Kalman filter family and StructTS (models, like/run/forecast/smooth, trend/BSM MLE)

use super::*;

/// State-space model components for the Kalman routines (arima.c).
/// `z` and `a` have length p; matrices are p x p column-major.
struct KalmanModel {
    z: Vec<f64>,
    a: Vec<f64>,
    p: Vec<f64>,
    t: Vec<f64>,
    v: Vec<f64>,
    h: f64,
    pn: Vec<f64>,
}

unsafe fn real_vec(x: SEXP) -> Vec<f64> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        let n = XLENGTH(x) as usize;
        (0..n)
            .map(|i| {
                if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(i)
                } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                    *INTEGER(x).add(i) as f64
                } else {
                    f64::NAN
                }
            })
            .collect()
    }
}

unsafe fn read_kalman_model(model: SEXP) -> Option<KalmanModel> {
    unsafe {
        if model.is_null() || model == R_NilValue() {
            return None;
        }
        let m = KalmanModel {
            z: real_vec(named_list_elt(model, "Z")),
            a: real_vec(named_list_elt(model, "a")),
            p: real_vec(named_list_elt(model, "P")),
            t: real_vec(named_list_elt(model, "T")),
            v: real_vec(named_list_elt(model, "V")),
            h: elt_real_or(named_list_elt(model, "h"), 0.0),
            pn: real_vec(named_list_elt(model, "Pn")),
        };
        let p = m.a.len();
        if p == 0
            || m.z.len() != p
            || m.p.len() != p * p
            || m.t.len() != p * p
            || m.v.len() != p * p
            || m.pn.len() != p * p
        {
            return None;
        }
        Some(m)
    }
}

/// Write the final filter state back into a (duplicated) model.
unsafe fn write_kalman_model(model: SEXP, m: &KalmanModel) {
    unsafe {
        let pairs: [(&str, &Vec<f64>); 3] = [("a", &m.a), ("P", &m.p), ("Pn", &m.pn)];
        for (name, vals) in pairs {
            let elt = named_list_elt(model, name);
            if elt.is_null() || elt == R_NilValue() {
                continue;
            }
            if TYPEOF(elt) == SEXPTYPE::REALSXP {
                for (i, &v) in vals.iter().enumerate() {
                    if i < XLENGTH(elt) as usize {
                        *REAL(elt).add(i) = v;
                    }
                }
            }
        }
    }
}

struct KalmanOut {
    s2: f64,
    sumlog: f64,
    resid: Vec<f64>,
    states: Vec<f64>,
}

/// Port of `KalmanLike` in arima.c (one filter pass). `lop` selects the
/// KalmanRun variant (standardized residuals plus an n x p states matrix).
/// Like the C original, mutates `m.a`, `m.p` and `m.pn` in place.
unsafe fn kalman_like_pass(y: &[f64], m: &mut KalmanModel, s_up: i32, lop: bool) -> KalmanOut {
    unsafe {
        let n = y.len();
        let p = m.a.len();
        let mut anew = vec![0.0; p];
        let mut mm = vec![0.0; p * p];
        let mut gain_vec = vec![0.0; p];
        let mut resid = if lop { vec![f64::NAN; n] } else { Vec::new() };
        let mut states = if lop { vec![0.0; n * p] } else { Vec::new() };
        let mut sumlog = 0.0;
        let mut ssq = 0.0;
        let mut nu = 0.0f64;
        for l in 0..n {
            for i in 0..p {
                let mut tmp = 0.0;
                for (k, &ak) in m.a.iter().enumerate() {
                    tmp += m.t[i + p * k] * ak;
                }
                anew[i] = tmp;
            }
            if l as i32 > s_up {
                for i in 0..p {
                    for j in 0..p {
                        let mut tmp = 0.0;
                        for k in 0..p {
                            tmp += m.t[i + p * k] * m.p[k + p * j];
                        }
                        mm[i + p * j] = tmp;
                    }
                }
                for i in 0..p {
                    for j in 0..p {
                        let mut tmp = m.v[i + p * j];
                        for k in 0..p {
                            tmp += mm[i + p * k] * m.t[j + p * k];
                        }
                        m.pn[i + p * j] = tmp;
                    }
                }
            }
            if !y[l].is_nan() {
                nu += 1.0;
                let mut resid0 = y[l];
                for i in 0..p {
                    resid0 -= m.z[i] * anew[i];
                }
                let mut gain = m.h;
                for i in 0..p {
                    let mut tmp = 0.0;
                    for j in 0..p {
                        tmp += m.pn[i + j * p] * m.z[j];
                    }
                    gain_vec[i] = tmp;
                    gain += m.z[i] * tmp;
                }
                ssq += resid0 * resid0 / gain;
                sumlog += gain.ln();
                if lop {
                    resid[l] = resid0 / gain.sqrt();
                }
                for i in 0..p {
                    m.a[i] = anew[i] + gain_vec[i] * resid0 / gain;
                }
                for i in 0..p {
                    for j in 0..p {
                        m.p[i + j * p] = m.pn[i + j * p] - gain_vec[i] * gain_vec[j] / gain;
                    }
                }
            } else {
                for i in 0..p {
                    m.a[i] = anew[i];
                }
                for i in 0..p * p {
                    m.p[i] = m.pn[i];
                }
                if lop {
                    resid[l] = f64::NAN;
                }
            }
            if lop {
                for j in 0..p {
                    states[l + n * j] = m.a[j];
                }
            }
        }
        let (s2, sumlog) = if nu > 0.0 {
            (ssq / nu, sumlog / nu)
        } else {
            (f64::NAN, f64::NAN)
        };
        KalmanOut {
            s2,
            sumlog,
            resid,
            states,
        }
    }
}

unsafe fn logical_elt(x: SEXP) -> i32 {
    unsafe {
        if x.is_null() || x == R_NilValue() || XLENGTH(x) == 0 {
            return 0;
        }
        let ty = TYPEOF(x);
        if ty == SEXPTYPE::LGLSXP {
            *LOGICAL(x) as i32
        } else if ty == SEXPTYPE::INTSXP {
            *INTEGER(x)
        } else if ty == SEXPTYPE::REALSXP && *REAL(x) != 0.0 {
            1
        } else {
            0
        }
    }
}

unsafe fn attach_mod_attr(target: SEXP, model: SEXP, m: &KalmanModel) {
    unsafe {
        let dup = crate::mainutils::duplicate::Rf_duplicate(model);
        let _d = protect(dup);
        write_kalman_model(dup, m);
        let sym = crate::sexp::symbol::Rf_install(c"mod".as_ptr());
        crate::sexp::attrib_core::setAttrib(target, sym, dup);
    }
}

/// GNU `KalmanLike(y, mod)` — R-wrapper semantics: list(Lik, s2).
pub unsafe fn do_kalman_like(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(args);
        let model = CAR(CDR(args));
        let Some(mut m) = read_kalman_model(model) else {
            return R_NilValue();
        };
        let yv = real_vec(y);
        let out = kalman_like_pass(&yv, &mut m, 0, false);
        let lik = 0.5 * (out.s2.ln() + out.sumlog);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, Rf_ScalarReal(lik));
        SET_VECTOR_ELT(result, 1, Rf_ScalarReal(out.s2));
        crate::mainutils::essentials::set_string_names(
            result,
            &["Lik".to_string(), "s2".to_string()],
        );
        result
    }
}

/// GNU `KalmanRun(y, mod, nit = 0, update = FALSE)` — likelihood,
/// standardized residuals and filtered states (n x p matrix).
pub unsafe fn do_kalman_run(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(args);
        let model = CAR(CDR(args));
        let tail = CDR(CDR(args));
        let nit = if tail.is_null() || tail == R_NilValue() {
            0
        } else {
            elt_real_or(CAR(tail), 0.0) as i32
        };
        let update = {
            let t2 = if tail.is_null() || tail == R_NilValue() {
                tail
            } else {
                CDR(tail)
            };
            !t2.is_null() && t2 != R_NilValue() && logical_elt(CAR(t2)) == 1
        };
        let Some(mut m) = read_kalman_model(model) else {
            return R_NilValue();
        };
        let yv = real_vec(y);
        let n = yv.len();
        let out = kalman_like_pass(&yv, &mut m, nit, true);
        let lik = 0.5 * (out.s2.ln() + out.sumlog);
        let values = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _v = protect(values);
        *REAL(values) = lik;
        *REAL(values).add(1) = out.s2;
        crate::mainutils::essentials::set_string_names(
            values,
            &["Lik".to_string(), "s2".to_string()],
        );
        let resid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _e = protect(resid);
        for i in 0..n {
            *REAL(resid).add(i) = out.resid[i];
        }
        let p = m.a.len();
        let states = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            n as c_int,
            p as c_int,
        );
        let _s = protect(states);
        for (i, &v) in out.states.iter().enumerate() {
            *REAL(states).add(i) = v;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, values);
        SET_VECTOR_ELT(result, 1, resid);
        SET_VECTOR_ELT(result, 2, states);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "values".to_string(),
                "resid".to_string(),
                "states".to_string(),
            ],
        );
        if update {
            attach_mod_attr(result, model, &m);
        }
        result
    }
}

/// GNU `KalmanForecast(n.ahead, mod)` — iterate the prediction step.
pub unsafe fn do_kalman_forecast(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_ahead = elt_real_or(CAR(args), 0.0).round() as usize;
        let model = CAR(CDR(args));
        let tail = CDR(CDR(args));
        let update = !tail.is_null() && tail != R_NilValue() && logical_elt(CAR(tail)) == 1;
        if n_ahead == 0 || model.is_null() || model == R_NilValue() {
            return R_NilValue();
        }
        let Some(mut m) = read_kalman_model(model) else {
            return R_NilValue();
        };
        let p = m.a.len();
        let pred = Rf_allocVector3(SEXPTYPE::REALSXP, n_ahead as i64);
        let _p = protect(pred);
        let var = Rf_allocVector3(SEXPTYPE::REALSXP, n_ahead as i64);
        let _va = protect(var);
        let mut anew = vec![0.0; p];
        let mut mm = vec![0.0; p * p];
        for l in 0..n_ahead {
            let mut fc = 0.0;
            for i in 0..p {
                let mut tmp = 0.0;
                for (k, &ak) in m.a.iter().enumerate() {
                    tmp += m.t[i + p * k] * ak;
                }
                anew[i] = tmp;
                fc += tmp * m.z[i];
            }
            for i in 0..p {
                m.a[i] = anew[i];
            }
            *REAL(pred).add(l) = fc;
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = 0.0;
                    for k in 0..p {
                        tmp += m.t[i + p * k] * m.p[k + p * j];
                    }
                    mm[i + p * j] = tmp;
                }
            }
            let mut se = m.h;
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = m.v[i + p * j];
                    for k in 0..p {
                        tmp += mm[i + p * k] * m.t[j + p * k];
                    }
                    m.p[i + j * p] = tmp;
                    se += m.z[i] * m.z[j] * m.p[i + j * p];
                }
            }
            *REAL(var).add(l) = se;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, pred);
        SET_VECTOR_ELT(result, 1, var);
        crate::mainutils::essentials::set_string_names(
            result,
            &["pred".to_string(), "var".to_string()],
        );
        if update {
            attach_mod_attr(result, model, &m);
        }
        result
    }
}
pub unsafe extern "C-unwind" fn c_kalman_fore(n_ahead: SEXP, model: SEXP, _update: SEXP) -> SEXP {
    unsafe {
        let args = crate::sexp::constructors::Rf_cons(model, R_NilValue());
        let args = crate::sexp::constructors::Rf_cons(n_ahead, args);
        do_kalman_forecast(R_NilValue(), R_NilValue(), args, R_NilValue())
    }
}
pub unsafe extern "C-unwind" fn c_kalman_like(
    y: SEXP,
    model: SEXP,
    s_up: SEXP,
    op: SEXP,
    update: SEXP,
) -> SEXP {
    unsafe {
        // Port of KalmanLike in arima.c: `op` FALSE gives KalmanLike
        // (numeric c(ssq/nu, sumlog/nu)); TRUE gives KalmanRun's list.
        let lop = logical_elt(op) == 1;
        let upd = logical_elt(update) == 1;
        let nit = elt_real_or(s_up, 0.0) as i32;
        let Some(mut m) = read_kalman_model(model) else {
            return R_NilValue();
        };
        let yv = real_vec(y);
        let out = kalman_like_pass(&yv, &mut m, nit, lop);
        let res2 = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _r2 = protect(res2);
        *REAL(res2) = out.s2;
        *REAL(res2).add(1) = out.sumlog;
        if !lop {
            if upd {
                attach_mod_attr(res2, model, &m);
            }
            return res2;
        }
        let n = yv.len();
        let p = m.a.len();
        let resid = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _e = protect(resid);
        for i in 0..n {
            *REAL(resid).add(i) = out.resid[i];
        }
        let states = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            n as c_int,
            p as c_int,
        );
        let _s = protect(states);
        for (i, &v) in out.states.iter().enumerate() {
            *REAL(states).add(i) = v;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, res2);
        SET_VECTOR_ELT(result, 1, resid);
        SET_VECTOR_ELT(result, 2, states);
        crate::mainutils::essentials::set_string_names(
            result,
            &[
                "values".to_string(),
                "resid".to_string(),
                "states".to_string(),
            ],
        );
        if upd {
            attach_mod_attr(result, model, &m);
        }
        result
    }
}

pub unsafe extern "C-unwind" fn c_kalman_smooth(y: SEXP, model: SEXP, nit: SEXP) -> SEXP {
    unsafe {
        let args = crate::sexp::constructors::Rf_cons(nit, R_NilValue());
        let args = crate::sexp::constructors::Rf_cons(model, args);
        let args = crate::sexp::constructors::Rf_cons(y, args);
        do_kalman_smooth(R_NilValue(), R_NilValue(), args, R_NilValue())
    }
}

/// GNU `KalmanSmooth(y, mod, nit)` — Rauch–Tung–Striebel smoother, general
/// state dimension (port of KalmanSmooth in arima.c).
pub unsafe fn do_kalman_smooth(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(args);
        let model = CAR(CDR(args));
        let tail = CDR(CDR(args));
        let nit = if tail.is_null() || tail == R_NilValue() {
            0
        } else {
            elt_real_or(CAR(tail), 0.0) as i32
        };
        if y.is_null() || y == R_NilValue() || model.is_null() || model == R_NilValue() {
            return R_NilValue();
        }
        let Some(mut m) = read_kalman_model(model) else {
            return R_NilValue();
        };
        let yv = real_vec(y);
        let n = yv.len();
        if n == 0 {
            return R_NilValue();
        }
        let p = m.a.len();
        let mut anew = vec![0.0; p];
        let mut mm = vec![0.0; p * p];
        let mut gain_vec = vec![0.0; p];
        let mut big_l = vec![0.0; p * p];
        let mut at = vec![0.0; n * p];
        let mut pt = vec![0.0; n * p * p];
        let mut gains = vec![0.0; n];
        let mut resids = vec![0.0; n];
        let mut mt = vec![0.0; n * p];
        for l in 0..n {
            for i in 0..p {
                let mut tmp = 0.0;
                for (k, &ak) in m.a.iter().enumerate() {
                    tmp += m.t[i + p * k] * ak;
                }
                anew[i] = tmp;
            }
            if l as i32 > nit {
                for i in 0..p {
                    for j in 0..p {
                        let mut tmp = 0.0;
                        for k in 0..p {
                            tmp += m.t[i + p * k] * m.p[k + p * j];
                        }
                        mm[i + p * j] = tmp;
                    }
                }
                for i in 0..p {
                    for j in 0..p {
                        let mut tmp = m.v[i + p * j];
                        for k in 0..p {
                            tmp += mm[i + p * k] * m.t[j + p * k];
                        }
                        m.pn[i + p * j] = tmp;
                    }
                }
            }
            for i in 0..p {
                at[l + n * i] = anew[i];
            }
            for i in 0..p * p {
                pt[l + n * i] = m.pn[i];
            }
            if !yv[l].is_nan() {
                let mut resid0 = yv[l];
                for i in 0..p {
                    resid0 -= m.z[i] * anew[i];
                }
                let mut gain = m.h;
                for i in 0..p {
                    let mut tmp = 0.0;
                    for j in 0..p {
                        tmp += m.pn[i + j * p] * m.z[j];
                    }
                    mt[l + n * i] = tmp;
                    gain_vec[i] = tmp;
                    gain += m.z[i] * tmp;
                }
                gains[l] = gain;
                resids[l] = resid0;
                for i in 0..p {
                    m.a[i] = anew[i] + gain_vec[i] * resid0 / gain;
                }
                for i in 0..p {
                    for j in 0..p {
                        m.p[i + j * p] = m.pn[i + j * p] - gain_vec[i] * gain_vec[j] / gain;
                    }
                }
            } else {
                for i in 0..p {
                    m.a[i] = anew[i];
                    mt[l + n * i] = 0.0;
                }
                for i in 0..p * p {
                    m.p[i] = m.pn[i];
                }
                gains[l] = f64::NAN;
                resids[l] = f64::NAN;
            }
        }
        // Backward pass: rt holds r_{t-1}; Nt the smoothed covariances.
        let mut nt = vec![0.0; n * p * p];
        let mut rt = vec![0.0; n * p];
        for l in (0..n).rev() {
            let gn = if !gains[l].is_nan() && gains[l] != 0.0 {
                1.0 / gains[l]
            } else {
                0.0
            };
            if !gains[l].is_nan() {
                for i in 0..p {
                    rt[l + n * i] = m.z[i] * resids[l] * gn;
                }
            } else {
                for i in 0..p {
                    rt[l + n * i] = 0.0;
                }
            }
            for i in 0..p {
                for j in 0..p {
                    nt[l + n * i + n * p * j] = m.z[i] * m.z[j] * gn;
                }
            }
            if l < n - 1 {
                for i in 0..p {
                    for j in 0..p {
                        mm[i + p * j] =
                            (if i == j { 1.0 } else { 0.0 }) - mt[l + n * i] * m.z[j] * gn;
                    }
                }
                for i in 0..p {
                    for j in 0..p {
                        let mut tmp = 0.0;
                        for k in 0..p {
                            tmp += m.t[i + p * k] * mm[k + p * j];
                        }
                        big_l[i + p * j] = tmp;
                    }
                }
                for i in 0..p {
                    let mut tmp = 0.0;
                    for j in 0..p {
                        tmp += big_l[j + p * i] * rt[l + 1 + n * j];
                    }
                    rt[l + n * i] += tmp;
                }
                for i in 0..p {
                    for j in 0..p {
                        let mut tmp = 0.0;
                        for k in 0..p {
                            tmp += big_l[k + p * i] * nt[l + 1 + n * k + n * p * j];
                        }
                        mm[i + p * j] = tmp;
                    }
                }
                for i in 0..p {
                    for j in 0..p {
                        let mut tmp = 0.0;
                        for k in 0..p {
                            tmp += mm[i + p * k] * big_l[k + p * j];
                        }
                        nt[l + n * i + n * p * j] += tmp;
                    }
                }
            }
            for i in 0..p {
                let mut tmp = 0.0;
                for j in 0..p {
                    tmp += pt[l + n * i + n * p * j] * rt[l + n * j];
                }
                at[l + n * i] += tmp;
            }
        }
        for l in 0..n {
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = 0.0;
                    for k in 0..p {
                        tmp += pt[l + n * i + n * p * k] * nt[l + n * k + n * p * j];
                    }
                    mm[i + p * j] = tmp;
                }
            }
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = pt[l + n * i + n * p * j];
                    for k in 0..p {
                        tmp -= mm[i + p * k] * pt[l + n * k + n * p * j];
                    }
                    nt[l + n * i + n * p * j] = tmp;
                }
            }
        }
        let sm = crate::mainutils::array::allocMatrix(
            SEXPTYPE::REALSXP.as_c_int(),
            n as c_int,
            p as c_int,
        );
        let _sm = protect(sm);
        for (i, &v) in at.iter().enumerate() {
            *REAL(sm).add(i) = v;
        }
        let va = Rf_allocVector3(SEXPTYPE::REALSXP, (n * p * p) as i64);
        let _va = protect(va);
        for (i, &v) in nt.iter().enumerate() {
            *REAL(va).add(i) = v;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, sm);
        SET_VECTOR_ELT(result, 1, va);
        crate::mainutils::essentials::set_string_names(
            result,
            &["smooth".to_string(), "var".to_string()],
        );
        result
    }
}

fn local_level_lik(y: &[f64], q: f64, r: f64) -> (f64, f64) {
    if q < 0.0 || r < 0.0 || (q == 0.0 && r == 0.0) {
        return (f64::INFINITY, f64::NAN);
    }
    let n = y.len();
    let mut a = y[0];
    let mut p = 1e7;
    let mut s2 = 0.0;
    let mut sumlog = 0.0;
    let mut nlik = 0.0;
    for (i, &yi) in y.iter().enumerate() {
        let resid = yi - a;
        let f = p + r;
        if f > 0.0 {
            if i > 0 {
                s2 += resid * resid / f;
                sumlog += f.ln();
                nlik += 1.0;
            }
            let k = p / f;
            a += k * resid;
            p -= k * k * f;
        }
        p += q;
    }
    if nlik < 1.0 || s2 <= 0.0 {
        return (f64::INFINITY, f64::NAN);
    }
    let s2m = s2 / nlik;
    (0.5 * (s2m.ln() + sumlog / nlik), s2m)
}

fn struct_ts_type(args: SEXP) -> String {
    unsafe {
        let mut a = CDR(args);
        let mut pos = 1;
        while !a.is_null() && a != R_NilValue() {
            let tag = TAG(a);
            let named = if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP
            {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if named == "type" || (named.is_empty() && pos == 1) {
                let v = CAR(a);
                if !v.is_null() && TYPEOF(v) == SEXPTYPE::STRSXP && XLENGTH(v) > 0 {
                    return std::ffi::CStr::from_ptr(CHAR(STRING_ELT(v, 0)))
                        .to_string_lossy()
                        .into_owned();
                }
            }
            pos += 1;
            a = CDR(a);
        }
        "level".to_string()
    }
}

/// GNU KalmanLike for local linear trend (p=2). `P[] <- 1e6*vx`.
fn kalman_trend_like(y: &[f64], rel: [f64; 3], vx: f64) -> f64 {
    if y.is_empty() || rel.iter().all(|v| *v <= 0.0) {
        return 1000.0;
    }
    let ql = rel[0].max(0.0) * vx;
    let qs = rel[1].max(0.0) * vx;
    let h = rel[2].max(0.0) * vx;
    let p0 = 1e6 * vx;
    // T = matrix(c(1,0,1,1), 2, 2) column-major.
    let t = [1.0, 0.0, 1.0, 1.0];
    let v = [ql, 0.0, 0.0, qs];
    let z = [1.0, 0.0];
    let a0 = [y[0], 0.0];
    let pmat = [p0, p0, p0, p0];
    kalman_like_nd(y, &z, &a0, &pmat, &t, &v, h, -1)
}

fn optimize_trend_rel(y: &[f64], vx: f64) -> [f64; 3] {
    let starts = [
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.15, 0.0],
        [0.0, 0.0, 100.0],
    ];
    let mut best_rel = [1.0, 1.0, 1.0];
    let mut best_v = f64::INFINITY;
    for start in starts {
        let mut rel = start;
        for _ in 0..8 {
            for i in 0..3 {
                let nll = |logv: f64| {
                    let mut r = rel;
                    r[i] = 10f64.powf(logv);
                    kalman_trend_like(y, r, vx)
                };
                let mut lo = -8.0;
                let mut hi = 4.0;
                let gr = (5.0_f64.sqrt() - 1.0) / 2.0;
                let mut c = hi - gr * (hi - lo);
                let mut d = lo + gr * (hi - lo);
                let mut fc = nll(c);
                let mut fd = nll(d);
                for _ in 0..60 {
                    if fc < fd {
                        hi = d;
                        d = c;
                        fd = fc;
                        c = hi - gr * (hi - lo);
                        fc = nll(c);
                    } else {
                        lo = c;
                        c = d;
                        fc = fd;
                        d = lo + gr * (hi - lo);
                        fd = nll(d);
                    }
                }
                let mut cand = rel;
                cand[i] = 10f64.powf(0.5 * (lo + hi));
                let mut zero = rel;
                zero[i] = 0.0;
                let v1 = kalman_trend_like(y, cand, vx);
                let v0 = kalman_trend_like(y, zero, vx);
                rel = if v0 <= v1 { zero } else { cand };
            }
        }
        let v = kalman_trend_like(y, rel, vx);
        if v < best_v {
            best_v = v;
            best_rel = rel;
        }
    }
    best_rel
}

fn series_frequency(x: SEXP) -> i32 {
    unsafe {
        let tsp = getAttrib(x, crate::sexp::symbol::Rf_install(c"tsp".as_ptr()));
        if !tsp.is_null()
            && tsp != R_NilValue()
            && TYPEOF(tsp) == SEXPTYPE::REALSXP
            && XLENGTH(tsp) >= 3
        {
            return (*REAL(tsp).add(2)).round() as i32;
        }
        1
    }
}

fn kalman_bsm_like(y: &[f64], rel: [f64; 4], vx: f64, nf: usize) -> f64 {
    if y.is_empty() || nf < 2 || rel.iter().all(|v| *v <= 0.0) {
        return 1000.0;
    }
    let p = nf + 1;
    let mut t = vec![0.0; p * p];
    t[0] = 1.0;
    t[p] = 1.0;
    t[1 + p] = 1.0;
    for j in 2..p {
        t[2 + p * j] = -1.0;
    }
    if nf >= 3 {
        for k in 2..nf {
            t[(k + 1) + p * k] = 1.0;
        }
    }
    let mut z = vec![0.0; p];
    z[0] = 1.0;
    z[2] = 1.0;
    let mut a0 = vec![0.0; p];
    a0[0] = y[0];
    let p0v = 1e6 * vx;
    let pmat = vec![p0v; p * p];
    let mut v = vec![0.0; p * p];
    v[0] = rel[0].max(0.0) * vx;
    v[1 + p] = rel[1].max(0.0) * vx;
    v[2 + 2 * p] = rel[2].max(0.0) * vx;
    let h = rel[3].max(0.0) * vx;
    kalman_like_nd(y, &z, &a0, &pmat, &t, &v, h, -1)
}

fn optimize_bsm_rel(y: &[f64], vx: f64, nf: usize) -> [f64; 4] {
    let starts = [
        [1.0, 1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 0.0],
    ];
    let mut best_rel = [1.0, 1.0, 1.0, 1.0];
    let mut best_v = f64::INFINITY;
    for start in starts {
        let mut rel = start;
        for _ in 0..8 {
            for i in 0..4 {
                let nll = |logv: f64| {
                    let mut r = rel;
                    r[i] = 10f64.powf(logv);
                    kalman_bsm_like(y, r, vx, nf)
                };
                let mut lo = -8.0;
                let mut hi = 4.0;
                let gr = (5.0_f64.sqrt() - 1.0) / 2.0;
                let mut c = hi - gr * (hi - lo);
                let mut d = lo + gr * (hi - lo);
                let mut fc = nll(c);
                let mut fd = nll(d);
                for _ in 0..50 {
                    if fc < fd {
                        hi = d;
                        d = c;
                        fd = fc;
                        c = hi - gr * (hi - lo);
                        fc = nll(c);
                    } else {
                        lo = c;
                        c = d;
                        fc = fd;
                        d = lo + gr * (hi - lo);
                        fd = nll(d);
                    }
                }
                let mut cand = rel;
                cand[i] = 10f64.powf(0.5 * (lo + hi));
                let mut zero = rel;
                zero[i] = 0.0;
                let v1 = kalman_bsm_like(y, cand, vx, nf);
                let v0 = kalman_bsm_like(y, zero, vx, nf);
                rel = if v0 <= v1 { zero } else { cand };
            }
        }
        let v = kalman_bsm_like(y, rel, vx, nf);
        if v < best_v {
            best_v = v;
            best_rel = rel;
        }
    }
    best_rel
}

/// GNU `StructTS(x, type=)` — local-level or local-linear-trend MLE.
pub unsafe fn do_struct_ts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = XLENGTH(x) as usize;
        if n < 2 {
            return R_NilValue();
        }
        let typ = struct_ts_type(args);
        if typ == "BSM" && series_frequency(x) < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "frequency must be a positive integer >= 2 for BSM",
            );
        }
        let mut y = vec![0.0; n];
        for i in 0..n {
            y[i] = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i)
            } else {
                *INTEGER(x).add(i) as f64
            };
        }
        if typ == "BSM" {
            let nf = series_frequency(x) as usize;
            let mean = y.iter().sum::<f64>() / n as f64;
            let var = y.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n as f64 - 1.0);
            let vx = var / 100.0;
            let rel = optimize_bsm_rel(&y, vx, nf);
            let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 4);
            let _c = protect(coef);
            for i in 0..4 {
                *REAL(coef).add(i) = (rel[i] * vx).max(0.0);
            }
            crate::mainutils::essentials::set_string_names(
                coef,
                &[
                    "level".to_string(),
                    "slope".to_string(),
                    "seas".to_string(),
                    "epsilon".to_string(),
                ],
            );
            let data = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
            let _d = protect(data);
            for i in 0..n {
                *REAL(data).add(i) = y[i];
            }
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let _r = protect(result);
            SET_VECTOR_ELT(result, 0, coef);
            SET_VECTOR_ELT(result, 1, data);
            crate::mainutils::essentials::set_string_names(
                result,
                &["coef".to_string(), "data".to_string()],
            );
            let class = Rf_mkString(c"StructTS".as_ptr());
            let _cl = protect(class);
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
            return result;
        }
        if typ == "trend" {
            let mean = y.iter().sum::<f64>() / n as f64;
            let var = y.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n as f64 - 1.0);
            let vx = var / 100.0;
            let rel = optimize_trend_rel(&y, vx);
            let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
            let _c = protect(coef);
            *REAL(coef) = (rel[0] * vx).max(0.0);
            *REAL(coef).add(1) = (rel[1] * vx).max(0.0);
            *REAL(coef).add(2) = (rel[2] * vx).max(0.0);
            crate::mainutils::essentials::set_string_names(
                coef,
                &[
                    "level".to_string(),
                    "slope".to_string(),
                    "epsilon".to_string(),
                ],
            );
            let data = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
            let _d = protect(data);
            for i in 0..n {
                *REAL(data).add(i) = y[i];
            }
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let _r = protect(result);
            SET_VECTOR_ELT(result, 0, coef);
            SET_VECTOR_ELT(result, 1, data);
            crate::mainutils::essentials::set_string_names(
                result,
                &["coef".to_string(), "data".to_string()],
            );
            let class = Rf_mkString(c"StructTS".as_ptr());
            let _cl = protect(class);
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
            return result;
        }
        let mean = y.iter().sum::<f64>() / n as f64;
        let vx = y.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n as f64 - 1.0);
        let mut cands = vec![0.0, 1.0, vx];
        if n >= 2 {
            let mut md = 0.0;
            let mut vd = 0.0;
            for i in 1..n {
                let d = y[i] - y[i - 1];
                md += d;
                vd += d * d;
            }
            md /= (n - 1) as f64;
            vd = vd / (n - 1) as f64 - md * md;
            cands.push(md.abs());
            cands.push(md * md);
            cands.push(vd.max(0.0));
        }
        for k in -4..=0 {
            cands.push(vx * 10f64.powi(k));
        }
        let mut best_q = 0.0;
        let mut best_r = vx;
        let mut best_lik = f64::INFINITY;
        let mut best_scale = f64::INFINITY;
        for &q in &cands {
            for &r in &cands {
                let (lik, s2m) = local_level_lik(&y, q, r);
                let scale = (s2m - 1.0).abs();
                if lik < best_lik - 1e-10 || (lik < best_lik + 1e-10 && scale < best_scale) {
                    best_lik = lik;
                    best_scale = scale;
                    best_q = q;
                    best_r = r;
                }
            }
        }
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _c = protect(coef);
        *REAL(coef) = best_q;
        *REAL(coef).add(1) = best_r;
        crate::mainutils::essentials::set_string_names(
            coef,
            &["level".to_string(), "epsilon".to_string()],
        );
        let data = Rf_allocVector3(SEXPTYPE::REALSXP, n as i64);
        let _d = protect(data);
        for i in 0..n {
            *REAL(data).add(i) = y[i];
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, coef);
        SET_VECTOR_ELT(result, 1, data);
        crate::mainutils::essentials::set_string_names(
            result,
            &["coef".to_string(), "data".to_string()],
        );
        let class = Rf_mkString(c"StructTS".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// GNU `tsSmooth(StructTS)` — Kalman smooth of the fitted local level.
pub unsafe fn do_ts_smooth(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let data = named_list_elt(obj, "data");
        let coef = named_list_elt(obj, "coef");
        if data == R_NilValue() || coef == R_NilValue() || XLENGTH(data) == 0 {
            return R_NilValue();
        }
        let q = elt_real_or(coef, 0.0);
        let r = if XLENGTH(coef) > 1 {
            *REAL(coef).add(1)
        } else {
            elt_real_or(coef, 0.0)
        };
        let a0 = *REAL(data);
        let p0 = if r > 0.0 { 1e4 * r } else { 1e4 };
        let z = Rf_ScalarReal(1.0);
        let _z = protect(z);
        let a = Rf_ScalarReal(a0);
        let _a = protect(a);
        let p = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _p = protect(p);
        *REAL(p) = 0.0;
        let t = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _t = protect(t);
        *REAL(t) = 1.0;
        let v = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _v = protect(v);
        *REAL(v) = q;
        let h = Rf_ScalarReal(r);
        let _h = protect(h);
        let pn = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), 1, 1);
        let _pn = protect(pn);
        *REAL(pn) = p0;
        let model = Rf_allocVector3(SEXPTYPE::VECSXP, 7);
        let _m = protect(model);
        SET_VECTOR_ELT(model, 0, z);
        SET_VECTOR_ELT(model, 1, a);
        SET_VECTOR_ELT(model, 2, p);
        SET_VECTOR_ELT(model, 3, t);
        SET_VECTOR_ELT(model, 4, v);
        SET_VECTOR_ELT(model, 5, h);
        SET_VECTOR_ELT(model, 6, pn);
        crate::mainutils::essentials::set_string_names(
            model,
            &[
                "Z".to_string(),
                "a".to_string(),
                "P".to_string(),
                "T".to_string(),
                "V".to_string(),
                "h".to_string(),
                "Pn".to_string(),
            ],
        );
        let ks_args = Rf_cons(data, Rf_cons(model, R_NilValue()));
        let _ka = protect(ks_args);
        let ks = do_kalman_smooth(call, op, ks_args, rho);
        let _ks = protect(ks);
        named_list_elt(ks, "smooth")
    }
}
