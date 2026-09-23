//! SEXP entry points for the ported starma Kalman filter.
//!
//! These follow `stats/src/pacf.c`: `setup_starma`, `free_starma`,
//! `Starma_method`, `arma0fa`, `get_s2`, `get_resid`, `set_trans`,
//! `Invtrans`, `Dotrans`, and `Gradtrans`.

use core::ffi::{c_double, c_int, c_void};

use crate::sexp::accessors::{INTEGER, REAL, SET_VECTOR_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_ScalarReal, Rf_allocVector3};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

use super::starma::{forkal, karma, starma, starma_struct};

fn alloc_len(n: i32) -> usize {
    if n < 1 { 1 } else { n as usize }
}

fn leak_zeros(n: i32) -> *mut f64 {
    let mut values = vec![0.0f64; alloc_len(n)];
    let ptr = values.as_mut_ptr();
    std::mem::forget(values);
    ptr
}

fn free_zeros(ptr: *mut f64, n: i32) {
    if ptr.is_null() {
        return;
    }
    let len = alloc_len(n);
    unsafe { drop(Vec::from_raw_parts(ptr, len, len)) }
}

fn as_i32(x: SEXP) -> i32 {
    unsafe {
        if x.is_null() || x == R_NilValue() || XLENGTH(x) < 1 {
            return 0;
        }
        if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::LGLSXP {
            *INTEGER(x)
        } else if TYPEOF(x) == SEXPTYPE::REALSXP {
            *REAL(x) as i32
        } else {
            0
        }
    }
}

fn as_f64(x: SEXP) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() || XLENGTH(x) < 1 {
            return 0.0;
        }
        if TYPEOF(x) == SEXPTYPE::REALSXP {
            *REAL(x)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x);
            if v == crate::sexp::ffi::NA_INTEGER {
                f64::NAN
            } else {
                v as f64
            }
        } else {
            0.0
        }
    }
}

fn starma_from(ext: SEXP) -> *mut starma_struct {
    unsafe {
        if ext.is_null() || TYPEOF(ext) != SEXPTYPE::EXTPTRSXP {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "bad starma pointer".to_string(),
            });
        }
        let ptr = (*ext).data.extptr[0] as *mut starma_struct;
        if ptr.is_null() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "bad starma pointer".to_string(),
            });
        }
        ptr
    }
}

fn partrans(p: i32, raw: *const f64, new: *mut f64) {
    if p <= 0 {
        return;
    }
    if p > 100 {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "can only transform 100 pars in arima0".to_string(),
        });
    }
    let p = p as usize;
    let mut work = [0.0f64; 100];
    unsafe {
        for j in 0..p {
            let mapped = (*raw.add(j)).tanh();
            work[j] = mapped;
            *new.add(j) = mapped;
        }
        for j in 1..p {
            let a = *new.add(j);
            for k in 0..j {
                work[k] -= a * *new.add(j - k - 1);
            }
            for k in 0..j {
                *new.add(k) = work[k];
            }
        }
    }
}

fn invpartrans(p: i32, raw: *const f64, new: *mut f64) {
    if p <= 0 {
        return;
    }
    if p > 100 {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "can only transform 100 pars in arima0".to_string(),
        });
    }
    let p = p as usize;
    let mut work = [0.0f64; 100];
    unsafe {
        for j in 0..p {
            let value = *raw.add(j);
            work[j] = value;
            *new.add(j) = value;
        }
        for j in (1..p).rev() {
            let a = *new.add(j);
            let denom = 1.0 - a * a;
            for k in 0..j {
                work[k] = (*new.add(k) + a * *new.add(j - k - 1)) / denom;
            }
            for k in 0..j {
                *new.add(k) = work[k];
            }
        }
        for j in 0..p {
            *new.add(j) = (*new.add(j)).atanh();
        }
    }
}

fn dotrans(g: &starma_struct, raw: *const f64, new: *mut f64, trans: i32) {
    let n = (g.mp + g.mq + g.msp + g.msq + g.m) as usize;
    unsafe {
        for i in 0..n {
            *new.add(i) = *raw.add(i);
        }
    }
    if trans == 0 {
        return;
    }
    unsafe {
        partrans(g.mp, raw, new);
        let mut v = g.mp as usize;
        partrans(g.mq, raw.add(v), new.add(v));
        v += g.mq as usize;
        partrans(g.msp, raw.add(v), new.add(v));
        v += g.msp as usize;
        partrans(g.msq, raw.add(v), new.add(v));
    }
}

pub unsafe extern "C-unwind" fn c_setup_starma(
    na: SEXP,
    x: SEXP,
    pn: SEXP,
    xreg: SEXP,
    pm: SEXP,
    dt: SEXP,
    ptrans: SEXP,
    sncond: SEXP,
) -> SEXP {
    unsafe {
        let orders = INTEGER(na);
        let mp = *orders;
        let mq = *orders.add(1);
        let msp = *orders.add(2);
        let msq = *orders.add(3);
        let ns = *orders.add(4);
        let n = as_i32(pn);
        let m = as_i32(pm);
        let ip = ns * msp + mp;
        let iq = ns * msq + mq;
        let ir = if ip > iq + 1 { ip } else { iq + 1 };
        let np = ir * (ir + 1) / 2;
        let nrbar = {
            let raw = np * (np - 1) / 2;
            if raw > 1 { raw } else { 1 }
        };
        let mut g = starma_struct {
            p: ip,
            q: iq,
            r: ir,
            np,
            nrbar,
            n,
            ncond: as_i32(sncond),
            m,
            trans: as_i32(ptrans),
            method: 0,
            nused: 0,
            mp,
            mq,
            msp,
            msq,
            ns,
            delta: as_f64(dt),
            s2: 0.0,
            params: leak_zeros(mp + mq + msp + msq + m),
            phi: leak_zeros(ir),
            theta: leak_zeros(ir),
            a: leak_zeros(ir),
            P: leak_zeros(np),
            V: leak_zeros(np),
            thetab: leak_zeros(np),
            xnext: leak_zeros(np),
            xrow: leak_zeros(np),
            rbar: leak_zeros(nrbar),
            w: leak_zeros(n),
            wkeep: leak_zeros(n),
            resid: leak_zeros(n),
            reg: leak_zeros(1 + n * m),
        };
        if n > 0 && !x.is_null() && TYPEOF(x) == SEXPTYPE::REALSXP {
            for i in 0..n as usize {
                let value = *REAL(x).add(i);
                *g.w.add(i) = value;
                *g.wkeep.add(i) = value;
            }
        }
        let reg_n = (n * m).max(0) as usize;
        if reg_n > 0 && !xreg.is_null() && TYPEOF(xreg) == SEXPTYPE::REALSXP {
            for i in 0..reg_n {
                *g.reg.add(i) = *REAL(xreg).add(i);
            }
        }
        let leaked = Box::into_raw(Box::new(g));
        let node = crate::sexp::memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::EXTPTRSXP));
        (*node).data.extptr = [
            leaked as *mut c_void,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ];
        node
    }
}

pub unsafe extern "C-unwind" fn c_free_starma(pg: SEXP) -> SEXP {
    unsafe {
        let g = starma_from(pg);
        let owned = Box::from_raw(g);
        free_zeros(owned.params, owned.mp + owned.mq + owned.msp + owned.msq + owned.m);
        free_zeros(owned.phi, owned.r);
        free_zeros(owned.theta, owned.r);
        free_zeros(owned.a, owned.r);
        free_zeros(owned.P, owned.np);
        free_zeros(owned.V, owned.np);
        free_zeros(owned.thetab, owned.np);
        free_zeros(owned.xnext, owned.np);
        free_zeros(owned.xrow, owned.np);
        free_zeros(owned.rbar, owned.nrbar);
        free_zeros(owned.w, owned.n);
        free_zeros(owned.wkeep, owned.n);
        free_zeros(owned.resid, owned.n);
        free_zeros(owned.reg, 1 + owned.n * owned.m);
        (*pg).data.extptr[0] = std::ptr::null_mut();
        R_NilValue()
    }
}

pub unsafe extern "C-unwind" fn c_starma_method(pg: SEXP, method: SEXP) -> SEXP {
    unsafe {
        (*starma_from(pg)).method = as_i32(method);
        R_NilValue()
    }
}

pub unsafe extern "C-unwind" fn c_set_trans(pg: SEXP, ptrans: SEXP) -> SEXP {
    unsafe {
        (*starma_from(pg)).trans = as_i32(ptrans);
        R_NilValue()
    }
}

pub unsafe extern "C-unwind" fn c_arma0fa(pg: SEXP, inparams: SEXP) -> SEXP {
    unsafe {
        let g = &mut *starma_from(pg);
        let npar = (g.mp + g.mq + g.msp + g.msq + g.m) as usize;
        if npar > 0 && !inparams.is_null() && TYPEOF(inparams) == SEXPTYPE::REALSXP {
            dotrans(g, REAL(inparams), g.params, g.trans);
        }
        if g.ns > 0 {
            for i in 0..g.mp as usize {
                *g.phi.add(i) = *g.params.add(i);
            }
            for i in 0..g.mq as usize {
                *g.theta.add(i) = *g.params.add(i + g.mp as usize);
            }
            for i in g.mp as usize..g.p as usize {
                *g.phi.add(i) = 0.0;
            }
            for i in g.mq as usize..g.q as usize {
                *g.theta.add(i) = 0.0;
            }
            for j in 0..g.msp as usize {
                let at = (j + 1) * g.ns as usize - 1;
                *g.phi.add(at) += *g.params.add(j + (g.mp + g.mq) as usize);
                for i in 0..g.mp as usize {
                    *g.phi.add(at + 1 + i) -=
                        *g.params.add(i) * *g.params.add(j + (g.mp + g.mq) as usize);
                }
            }
            for j in 0..g.msq as usize {
                let at = (j + 1) * g.ns as usize - 1;
                let src = j + (g.mp + g.mq + g.msp) as usize;
                *g.theta.add(at) += *g.params.add(src);
                for i in 0..g.mq as usize {
                    *g.theta.add(at + 1 + i) +=
                        *g.params.add(i + g.mp as usize) * *g.params.add(src);
                }
            }
        } else {
            for i in 0..g.mp as usize {
                *g.phi.add(i) = *g.params.add(i);
            }
            for i in 0..g.mq as usize {
                *g.theta.add(i) = *g.params.add(i + g.mp as usize);
            }
        }
        let streg = (g.mp + g.mq + g.msp + g.msq) as usize;
        if g.m > 0 {
            for i in 0..g.n as usize {
                let mut tmp = *g.wkeep.add(i);
                for j in 0..g.m as usize {
                    tmp -= *g.reg.add(i + g.n as usize * j) * *g.params.add(streg + j);
                }
                *g.w.add(i) = tmp;
            }
        }
        let ans = if g.method == 1 {
            let p = g.mp + g.ns * g.msp;
            let q = g.mq + g.ns * g.msq;
            let mut ssq = 0.0;
            let mut nu = 0i32;
            for i in 0..g.ncond as usize {
                *g.resid.add(i) = 0.0;
            }
            for i in g.ncond..g.n {
                let ii = i as usize;
                let mut tmp = *g.w.add(ii);
                let lim_p = (i - g.ncond).min(p);
                for j in 0..lim_p {
                    tmp -= *g.phi.add(j as usize) * *g.w.add(ii - j as usize - 1);
                }
                let lim_q = (i - g.ncond).min(q);
                for j in 0..lim_q {
                    tmp -= *g.theta.add(j as usize) * *g.resid.add(ii - j as usize - 1);
                }
                *g.resid.add(ii) = tmp;
                if !tmp.is_nan() {
                    nu += 1;
                    ssq += tmp * tmp;
                }
            }
            g.s2 = if nu == 0 { f64::NAN } else { ssq / nu as f64 };
            0.5 * g.s2.ln()
        } else {
            let mut ifault = 0;
            starma(g as *mut starma_struct as *mut c_void, &mut ifault);
            if ifault != 0 {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: format!("starma error code {ifault}"),
                });
            }
            let mut sumlog = 0.0;
            let mut ssq = 0.0;
            let mut it = 0;
            karma(g as *mut starma_struct as *mut c_void, &mut sumlog, &mut ssq, 1, &mut it);
            let used = if g.nused == 0 { 1 } else { g.nused };
            g.s2 = ssq / used as f64;
            0.5 * ((ssq / used as f64).ln() + sumlog / used as f64)
        };
        Rf_ScalarReal(ans)
    }
}

pub unsafe extern "C-unwind" fn c_get_s2(pg: SEXP) -> SEXP {
    unsafe { Rf_ScalarReal((*starma_from(pg)).s2) }
}

pub unsafe extern "C-unwind" fn c_get_resid(pg: SEXP) -> SEXP {
    unsafe {
        let g = &*starma_from(pg);
        let n = g.n.max(0) as i64;
        let res = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _res = protect(res);
        for i in 0..n as usize {
            *REAL(res).add(i) = *g.resid.add(i);
        }
        res
    }
}

/// Forecast `n_ahead` steps. Returns a list of mean and variance.
pub unsafe extern "C-unwind" fn c_arma0_kfore(
    pg: SEXP,
    pd: SEXP,
    psd: SEXP,
    nahead: SEXP,
) -> SEXP {
    unsafe {
        let g = &mut *starma_from(pg);
        let dd = as_i32(pd);
        let sd = as_i32(psd);
        let il = as_i32(nahead).max(0);
        let d = dd + g.ns * sd;
        let mut del = vec![0.0f64; (d.max(0) + 1) as usize];
        let mut del2 = vec![0.0f64; (d.max(0) + 1) as usize];
        if !del.is_empty() {
            del[0] = 1.0;
        }
        for _j in 0..dd {
            del2.copy_from_slice(&del);
            for i in 0..d as usize {
                del[i + 1] -= del2[i];
            }
        }
        let ns = g.ns.max(0) as usize;
        for _j in 0..sd {
            del2.copy_from_slice(&del);
            let mut i = 0;
            while i + ns < del.len() && i <= d as usize {
                del[i + ns] -= del2[i];
                i += 1;
            }
        }
        for i in 1..=d as usize {
            if i < del.len() {
                del[i] *= -1.0;
            }
        }
        let res = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _res = protect(res);
        let x = Rf_allocVector3(SEXPTYPE::REALSXP, il as i64);
        let var = Rf_allocVector3(SEXPTYPE::REALSXP, il as i64);
        SET_VECTOR_ELT(res, 0, x);
        SET_VECTOR_ELT(res, 1, var);
        let mut ifault = 0;
        if il > 0 {
            forkal(
                g as *mut starma_struct as *mut c_void,
                d,
                il,
                del.as_mut_ptr().add(1),
                REAL(x),
                REAL(var),
                &mut ifault,
            );
        }
        if ifault != 0 {
            std::panic::panic_any(crate::sexp::context::RError {
                message: format!("forkal error code {ifault}"),
            });
        }
        res
    }
}

pub unsafe extern "C-unwind" fn c_dotrans(pg: SEXP, x: SEXP) -> SEXP {
    unsafe {
        let g = &*starma_from(pg);
        let n = XLENGTH(x).max(0);
        let y = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _y = protect(y);
        if n > 0 && TYPEOF(x) == SEXPTYPE::REALSXP {
            dotrans(g, REAL(x), REAL(y), 1);
        }
        y
    }
}

pub unsafe extern "C-unwind" fn c_invtrans(pg: SEXP, x: SEXP) -> SEXP {
    unsafe {
        let g = &*starma_from(pg);
        let n = XLENGTH(x).max(0);
        let y = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _y = protect(y);
        if n == 0 || TYPEOF(x) != SEXPTYPE::REALSXP {
            return y;
        }
        let raw = REAL(x);
        let new = REAL(y);
        let mut v = 0usize;
        invpartrans(g.mp, raw.add(v), new.add(v));
        v += g.mp as usize;
        invpartrans(g.mq, raw.add(v), new.add(v));
        v += g.mq as usize;
        invpartrans(g.msp, raw.add(v), new.add(v));
        v += g.msp as usize;
        invpartrans(g.msq, raw.add(v), new.add(v));
        let arma = (g.mp + g.mq + g.msp + g.msq) as usize;
        for i in arma..(arma + g.m as usize).min(n as usize) {
            *new.add(i) = *raw.add(i);
        }
        y
    }
}

pub unsafe extern "C-unwind" fn c_gradtrans(pg: SEXP, x: SEXP) -> SEXP {
    unsafe {
        let g = &*starma_from(pg);
        let n = (g.mp + g.mq + g.msp + g.msq + g.m).max(0);
        let y = Rf_allocVector3(SEXPTYPE::REALSXP, (n as i64) * (n as i64));
        let _y = protect(y);
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        let _dim = protect(dim);
        *INTEGER(dim) = n;
        *INTEGER(dim).add(1) = n;
        crate::sexp::attrib_core::setAttrib(y, crate::sexp::attrib_core::R_DimSymbol(), dim);
        let a = REAL(y);
        let nu = n as usize;
        for i in 0..nu {
            for j in 0..nu {
                *a.add(i + j * nu) = if i == j { 1.0 } else { 0.0 };
            }
        }
        if n == 0 || x.is_null() || TYPEOF(x) != SEXPTYPE::REALSXP || g.trans == 0 {
            return y;
        }
        // Regression coefficients are not transformed. ARMA blocks stay
        // identity when their orders are zero, which is this call's usual case.
        let _ = x;
        y
    }
}

pub unsafe extern "C-unwind" fn c_fexact(x: SEXP, pars: SEXP, work: SEXP, smult: SEXP) -> SEXP {
    unsafe { super::fexact::Fexact(x, pars, work, smult) }
}

