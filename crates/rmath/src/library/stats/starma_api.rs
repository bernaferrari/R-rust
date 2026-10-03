//! SEXP entry points for the ported starma Kalman filter.
//!
//! These follow `stats/src/pacf.c`: `setup_starma`, `free_starma`,
//! `Starma_method`, `arma0fa`, `get_s2`, `get_resid`, `set_trans`,
//! `Invtrans`, `Dotrans`, and `Gradtrans`.

use core::ffi::{c_double, c_int, c_void};
use std::{cell::RefCell, rc::Rc};

use crate::sexp::accessors::{INTEGER, REAL, SET_VECTOR_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_ScalarReal, Rf_allocVector3};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

use super::starma::{forecast_dimensions, forkal, karma, starma, starma_struct};

fn alloc_len(n: i32) -> usize {
    if n < 1 { 1 } else { n as usize }
}

/// Owns every native buffer referenced by the Kalman state. The cell supports
/// the existing mutable numerical entry points while Rc keeps the state alive
/// if a construction callback closes its external pointer.
struct StarmaState {
    state: RefCell<starma_struct>,
    _buffers: [Vec<f64>; 14],
}

#[cfg(test)]
thread_local! {
    static LIVE_STARMA_ALLOCATIONS: std::cell::Cell<(usize, usize)> = const {
        std::cell::Cell::new((0, 0))
    };
}

impl StarmaState {
    fn new(state: starma_struct, buffers: [Vec<f64>; 14]) -> Rc<Self> {
        let owned = Rc::new(Self { state: RefCell::new(state), _buffers: buffers });
        #[cfg(test)]
        LIVE_STARMA_ALLOCATIONS.with(|live| {
            let (states, buffers) = live.get();
            live.set((states + 1, buffers + 14));
        });
        owned
    }
}

impl Drop for StarmaState {
    fn drop(&mut self) {
        #[cfg(test)]
        LIVE_STARMA_ALLOCATIONS.with(|live| {
            let (states, buffers) = live.get();
            live.set((states - 1, buffers - 14));
        });
    }
}

/// Owns the tentative external-pointer reference until publication completes.
/// No owner/arena field is borrowed while allocation callbacks execute.
struct StarmaPublication {
    state: Rc<StarmaState>,
    published: Option<crate::sexp::heap::CheckedNode>,
}

impl StarmaPublication {
    fn complete(&mut self) {
        let node = self.published.as_ref().expect("initialized starma publication");
        let heap = node.heap_identity();
        let authenticated = heap.resource::<StarmaState>(node)
            .is_some_and(|state| Rc::ptr_eq(&state, &self.state));
        let valid = authenticated && heap.node_snapshot(node)
            .is_some_and(|header| matches!(header.data,
                crate::sexp::ffi::NodeBody::ExtPtr(body)
                    if body.address == Rc::as_ptr(&self.state) as *mut c_void));
        if !valid { starma_error("starma pointer closed during initialization"); }
        // Canonical node storage now owns the state. The address carries no
        // independent native ownership and can never authenticate its type.
        self.published = None;
    }
}

impl Drop for StarmaPublication {
    fn drop(&mut self) {
        let Some(node) = self.published.take() else { return; };
        let heap = node.heap_identity();
        if let Some(mut header) = heap.node_snapshot(&node) {
            if let crate::sexp::ffi::NodeBody::ExtPtr(body) = &mut header.data {
                if body.address == Rc::as_ptr(&self.state) as *mut c_void {
                    body.address = std::ptr::null_mut();
                    heap.replace_node(&node, header).expect("live starma publication cleanup");
                }
            }
        }
        // The detached Rc is dropped after every canonical storage loan ends.
        // Prior explicit free or retirement leaves nothing to release twice.
        drop(heap.take_resource(&node));
    }
}

unsafe fn alloc_result(kind: SEXPTYPE, length: i64) -> SEXP {
    let result = unsafe { Rf_allocVector3(kind, length) };
    if result.is_null() { starma_error("could not allocate starma result"); }
    result
}

unsafe fn scalar_result(value: f64) -> SEXP {
    let result = unsafe { Rf_ScalarReal(value) };
    if result.is_null() { starma_error("could not allocate starma result"); }
    result
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

fn starma_error(message: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError { message: message.to_string() })
}

fn dimension(value: SEXP, name: &str) -> i32 {
    unsafe {
        if value.is_null() || XLENGTH(value) != 1 {
            starma_error(&format!("invalid starma {name}"));
        }
        let number = match SEXPTYPE(TYPEOF(value)) {
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
                let number = *INTEGER(value);
                if number == crate::sexp::ffi::NA_INTEGER { starma_error(&format!("invalid starma {name}")); }
                f64::from(number)
            }
            SEXPTYPE::REALSXP => *REAL(value),
            _ => starma_error(&format!("invalid starma {name}")),
        };
        if !number.is_finite() || number.fract() != 0.0 || number < 0.0 || number > f64::from(i32::MAX) {
            starma_error(&format!("invalid starma {name}"));
        }
        number as i32
    }
}

fn checked_size(value: i64) -> i32 {
    i32::try_from(value).ok().filter(|value| *value >= 0)
        .unwrap_or_else(|| starma_error("starma dimensions are too large"))
}

unsafe fn require_reals(value: SEXP, needed: i32, name: &str) {
    if needed == 0 { return; }
    unsafe {
        if value.is_null() || TYPEOF(value) != SEXPTYPE::REALSXP || XLENGTH(value) < i64::from(needed) {
            starma_error(&format!("starma {name} is shorter than its dimensions"));
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

fn starma_state(ext: SEXP) -> Rc<StarmaState> {
    let (_, token) = crate::sexp::memory::checked_projection(ext)
        .unwrap_or_else(|| starma_error("bad starma pointer"));
    let heap = token.heap_identity();
    let header = heap.node_snapshot(&token).unwrap_or_else(|| starma_error("bad starma pointer"));
    if header.sxpinfo.type_of() != SEXPTYPE::EXTPTRSXP { starma_error("bad starma pointer"); }
    // The exact canonical allocation owns and authenticates the Rust type.
    // A copied address, unrelated EXTPTR, or recycled slot cannot forge it.
    let state = heap.resource::<StarmaState>(&token)
        .unwrap_or_else(|| starma_error("bad starma pointer"));
    if header.data.extptr().address != Rc::as_ptr(&state) as *mut c_void {
        starma_error("bad starma pointer");
    }
    state
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
        if na.is_null() || TYPEOF(na) != SEXPTYPE::INTSXP || XLENGTH(na) != 5 {
            starma_error("starma orders must be five nonnegative integers");
        }
        let orders = INTEGER(na);
        let [mp, mq, msp, msq, ns] = std::array::from_fn(|index| *orders.add(index));
        if [mp, mq, msp, msq, ns].iter().any(|order| *order < 0) {
            starma_error("starma orders must be five nonnegative integers");
        }
        let n = dimension(pn, "observation count");
        let m = dimension(pm, "regression count");
        let ncond = dimension(sncond, "conditioning count");
        if ncond > n { starma_error("starma conditioning count exceeds observation count"); }
        let ip = checked_size(i64::from(ns) * i64::from(msp) + i64::from(mp));
        let iq = checked_size(i64::from(ns) * i64::from(msq) + i64::from(mq));
        let ir = checked_size(i64::from(ip).max(i64::from(iq) + 1));
        let np = checked_size(i64::from(ir) * (i64::from(ir) + 1) / 2);
        let nrbar = checked_size((i64::from(np) * (i64::from(np) - 1) / 2).max(1));
        let npar = checked_size(i64::from(mp) + i64::from(mq) + i64::from(msp) + i64::from(msq) + i64::from(m));
        let reg_n = checked_size(i64::from(n) * i64::from(m));
        let reg_capacity = checked_size(i64::from(reg_n) + 1);
        require_reals(x, n, "observations");
        require_reals(xreg, reg_n, "regression input");
        let lengths = [npar, ir, ir, ir, np, np, np, np, np, nrbar, n, n, n, reg_capacity];
        // Completed buffers stay ordinary Rust owners even if a later
        // allocation or initialization fails.
        let mut buffers: [Vec<f64>; 14] = std::array::from_fn(|_| Vec::new());
        for (buffer, length) in buffers.iter_mut().zip(lengths) {
            let length = alloc_len(length);
            buffer.try_reserve_exact(length).unwrap_or_else(|_| starma_error("could not allocate starma workspace"));
            buffer.resize(length, 0.0);
        }
        let g = starma_struct {
            p: ip,
            q: iq,
            r: ir,
            np,
            nrbar,
            n,
            ncond,
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
            params: buffers[0].as_mut_ptr(),
            phi: buffers[1].as_mut_ptr(),
            theta: buffers[2].as_mut_ptr(),
            a: buffers[3].as_mut_ptr(),
            P: buffers[4].as_mut_ptr(),
            V: buffers[5].as_mut_ptr(),
            thetab: buffers[6].as_mut_ptr(),
            xnext: buffers[7].as_mut_ptr(),
            xrow: buffers[8].as_mut_ptr(),
            rbar: buffers[9].as_mut_ptr(),
            w: buffers[10].as_mut_ptr(),
            wkeep: buffers[11].as_mut_ptr(),
            resid: buffers[12].as_mut_ptr(),
            reg: buffers[13].as_mut_ptr(),
        };
        if n > 0 && !x.is_null() && TYPEOF(x) == SEXPTYPE::REALSXP {
            for i in 0..n as usize {
                let value = *REAL(x).add(i);
                *g.w.add(i) = value;
                *g.wkeep.add(i) = value;
            }
        }
        let reg_n = reg_n as usize;
        if reg_n > 0 && !xreg.is_null() && TYPEOF(xreg) == SEXPTYPE::REALSXP {
            for i in 0..reg_n {
                *g.reg.add(i) = *REAL(xreg).add(i);
            }
        }
        let state = StarmaState::new(g, buffers);
        let mut publication = StarmaPublication { state, published: None };
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| std::panic::panic_any(crate::sexp::context::RError {
                message: error.to_string(),
            }));
        let factory = owner.node_factory();
        let result = factory.allocate(|arena| {
            let node = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
            if node.is_null() { return None; }
            let token = arena.node_token(node).expect("fresh starma external pointer");
            let heap = arena.heap_identity();
            heap.attach_resource(&token, publication.state.clone())?;
            publication.published = Some(token);
            let address = Rc::as_ptr(&publication.state);
            *(*node).data.extptr_mut() = crate::sexp::ffi::ExtPtrBody {
                address: address as *mut c_void,
                protected: crate::sexp::heap::NodeLink::NULL,
                tag: crate::sexp::heap::NodeLink::NULL,
            };
            Some(node)
        }).unwrap_or_else(|error| std::panic::panic_any(crate::sexp::context::RError {
            message: format!("could not allocate starma pointer: {error}"),
        }));
        publication.complete();
        result.as_raw()
    }
}

pub unsafe extern "C-unwind" fn c_free_starma(pg: SEXP) -> SEXP {
    unsafe {
        let (_, token) = crate::sexp::memory::checked_projection(pg)
            .unwrap_or_else(|| starma_error("bad starma pointer"));
        let heap = token.heap_identity();
        let mut header = heap.node_snapshot(&token).unwrap_or_else(|| starma_error("bad starma pointer"));
        if header.sxpinfo.type_of() != SEXPTYPE::EXTPTRSXP { starma_error("bad starma pointer"); }
        if header.data.extptr().address.is_null() {
            // Generic external-pointer clearing changes the address, not the
            // canonical typed attachment. Release only authenticated STARMA
            // ownership here; an unrelated empty EXTPTR remains untouched.
            if heap.resource::<StarmaState>(&token).is_some() {
                drop(heap.take_resource(&token));
            }
            return R_NilValue();
        }
        // Pin and authenticate before taking the canonical node's attachment.
        let _state = starma_state(pg);
        header.data.extptr_mut().address = std::ptr::null_mut();
        heap.replace_node(&token, header).expect("live starma pointer close");
        drop(heap.take_resource(&token));
        R_NilValue()
    }
}

pub unsafe extern "C-unwind" fn c_starma_method(pg: SEXP, method: SEXP) -> SEXP {
    unsafe {
        let state = starma_state(pg);
        let method = as_i32(method);
        state.state.borrow_mut().method = method;
        R_NilValue()
    }
}

pub unsafe extern "C-unwind" fn c_set_trans(pg: SEXP, ptrans: SEXP) -> SEXP {
    unsafe {
        let state = starma_state(pg);
        let trans = as_i32(ptrans);
        state.state.borrow_mut().trans = trans;
        R_NilValue()
    }
}

pub unsafe extern "C-unwind" fn c_arma0fa(pg: SEXP, inparams: SEXP) -> SEXP {
    unsafe {
        let state = starma_state(pg);
        let npar = {
            let g = state.state.borrow();
            (g.mp + g.mq + g.msp + g.msq + g.m) as usize
        };
        let input = if npar > 0 && !inparams.is_null() && TYPEOF(inparams) == SEXPTYPE::REALSXP {
            if XLENGTH(inparams) < npar as i64 {
                std::panic::panic_any(crate::sexp::context::RError { message: "too few starma parameters".to_string() });
            }
            Some(std::slice::from_raw_parts(REAL(inparams), npar).to_vec())
        } else { None };
        let ans = {
        let mut state_loan = state.state.borrow_mut();
        let g = &mut *state_loan;
        if let Some(input) = input { dotrans(g, input.as_ptr(), g.params, g.trans); }
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
        ans
        };
        scalar_result(ans)
    }
}

pub unsafe extern "C-unwind" fn c_get_s2(pg: SEXP) -> SEXP {
    unsafe {
        let state = starma_state(pg);
        let s2 = state.state.borrow().s2;
        scalar_result(s2)
    }
}

pub unsafe extern "C-unwind" fn c_get_resid(pg: SEXP) -> SEXP {
    unsafe {
        let state = starma_state(pg);
        let values = {
            let g = state.state.borrow();
            std::slice::from_raw_parts(g.resid, g.n.max(0) as usize).to_vec()
        };
        let res = alloc_result(SEXPTYPE::REALSXP, values.len() as _);
        let _res = protect(res);
        if !values.is_empty() { std::ptr::copy_nonoverlapping(values.as_ptr(), REAL(res), values.len()); }
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
        let state = starma_state(pg);
        let dd = dimension(pd, "difference count");
        let sd = dimension(psd, "seasonal difference count");
        let il = dimension(nahead, "forecast count");
        let (ns_value, r, n) = {
            let g = state.state.borrow();
            (g.ns, g.r, g.n)
        };
        let d = checked_size(i64::from(dd) + i64::from(ns_value) * i64::from(sd));
        forecast_dimensions(r, n, d).unwrap_or_else(|error| starma_error(&error.to_string()));
        let length = checked_size(i64::from(d) + 1) as usize;
        let mut del = Vec::new();
        del.try_reserve_exact(length).unwrap_or_else(|_| starma_error("could not allocate starma forecast workspace"));
        del.resize(length, 0.0);
        let mut del2 = Vec::new();
        del2.try_reserve_exact(length).unwrap_or_else(|_| starma_error("could not allocate starma forecast workspace"));
        del2.resize(length, 0.0);
        if !del.is_empty() {
            del[0] = 1.0;
        }
        for _j in 0..dd {
            del2.copy_from_slice(&del);
            for i in 0..d as usize {
                del[i + 1] -= del2[i];
            }
        }
        let ns = ns_value.max(0) as usize;
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
        let (forecast, variance) = {
            let state_loan = state.state.borrow();
            forkal(&state_loan, &state._buffers, d, il, &del[1..])
                .unwrap_or_else(|error| starma_error(&error.to_string()))
        };
        // All numerical state loans end before any R result allocation or
        // materialization. The pinned Rc and owned output survive callbacks.
        let res = alloc_result(SEXPTYPE::VECSXP, 2);
        let _res = protect(res);
        let x = alloc_result(SEXPTYPE::REALSXP, il as i64);
        let _x = protect(x);
        let var = alloc_result(SEXPTYPE::REALSXP, il as i64);
        SET_VECTOR_ELT(res, 0, x);
        SET_VECTOR_ELT(res, 1, var);
        std::ptr::copy_nonoverlapping(forecast.as_ptr(), REAL(x), forecast.len());
        std::ptr::copy_nonoverlapping(variance.as_ptr(), REAL(var), variance.len());
        res
    }
}

pub unsafe extern "C-unwind" fn c_dotrans(pg: SEXP, x: SEXP) -> SEXP {
    unsafe {
        let state = starma_state(pg);
        let n = XLENGTH(x).max(0);
        let input = if n > 0 && TYPEOF(x) == SEXPTYPE::REALSXP {
            Some(std::slice::from_raw_parts(REAL(x), n as usize).to_vec())
        } else { None };
        let needed = { let g = state.state.borrow(); g.mp + g.mq + g.msp + g.msq + g.m };
        if input.as_ref().is_some_and(|input| input.len() < needed as usize) {
            starma_error("too few starma parameters");
        }
        let y = alloc_result(SEXPTYPE::REALSXP, n);
        let _y = protect(y);
        if let Some(input) = input {
            let output = REAL(y);
            dotrans(&state.state.borrow(), input.as_ptr(), output, 1);
        }
        y
    }
}

pub unsafe extern "C-unwind" fn c_invtrans(pg: SEXP, x: SEXP) -> SEXP {
    unsafe {
        let state = starma_state(pg);
        let n = XLENGTH(x).max(0);
        let input = if n > 0 && TYPEOF(x) == SEXPTYPE::REALSXP {
            Some(std::slice::from_raw_parts(REAL(x), n as usize).to_vec())
        } else { None };
        let needed = { let g = state.state.borrow(); g.mp + g.mq + g.msp + g.msq };
        if input.as_ref().is_some_and(|input| input.len() < needed as usize) {
            starma_error("too few starma parameters");
        }
        let y = alloc_result(SEXPTYPE::REALSXP, n);
        let _y = protect(y);
        let Some(input) = input else { return y; };
        let raw = input.as_ptr();
        let new = REAL(y);
        let g = state.state.borrow();
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
        let state = starma_state(pg);
        let (n, trans) = {
            let g = state.state.borrow();
            ((g.mp + g.mq + g.msp + g.msq + g.m).max(0), g.trans)
        };
        let y = alloc_result(SEXPTYPE::REALSXP, (n as i64) * (n as i64));
        let _y = protect(y);
        let dim = alloc_result(SEXPTYPE::INTSXP, 2);
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
        if n == 0 || x.is_null() || TYPEOF(x) != SEXPTYPE::REALSXP || trans == 0 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{Sexp, session::RSession};
    use crate::sexp::memory::{self, ArenaBudget};
    use std::{cell::Cell, panic::AssertUnwindSafe};

    fn live_allocations() -> (usize, usize) {
        LIVE_STARMA_ALLOCATIONS.with(Cell::get)
    }

    fn inputs(session: &RSession) -> [Sexp<'_>; 8] {
        let factory = session.owner_token().unwrap().node_factory();
        let integer = |values: &[i32]| factory.allocate(|arena| {
            let raw = arena.alloc_vector(SEXPTYPE::INTSXP, values.len() as _);
            if raw.is_null() { return None; }
            if !values.is_empty() {
                unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), INTEGER(raw), values.len()); }
            }
            Some(raw)
        }).unwrap();
        let real = |values: &[f64]| factory.allocate(|arena| {
            let raw = arena.alloc_vector(SEXPTYPE::REALSXP, values.len() as _);
            if raw.is_null() { return None; }
            if !values.is_empty() {
                unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), REAL(raw), values.len()); }
            }
            Some(raw)
        }).unwrap();
        [integer(&[1, 0, 0, 0, 0]), real(&[2.0, 4.0, 8.0]), integer(&[3]),
            real(&[]), integer(&[0]), real(&[-1.0]), integer(&[0]), integer(&[0])]
    }

    unsafe fn setup(values: &[Sexp<'_>; 8]) -> SEXP {
        unsafe {
            c_setup_starma(values[0].as_raw(), values[1].as_raw(), values[2].as_raw(),
                values[3].as_raw(), values[4].as_raw(), values[5].as_raw(),
                values[6].as_raw(), values[7].as_raw())
        }
    }

    fn integer_values<'a>(session: &'a RSession, values: &[i32]) -> Sexp<'a> {
        session.owner_token().unwrap().node_factory().allocate(|arena| {
            let raw = arena.alloc_vector(SEXPTYPE::INTSXP, values.len() as _);
            if raw.is_null() { return None; }
            unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), INTEGER(raw), values.len()); }
            Some(raw)
        }).unwrap()
    }

    fn real_values<'a>(session: &'a RSession, values: &[f64]) -> Sexp<'a> {
        session.owner_token().unwrap().node_factory().allocate(|arena| {
            let raw = arena.alloc_vector(SEXPTYPE::REALSXP, values.len() as _);
            if raw.is_null() { return None; }
            unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), REAL(raw), values.len()); }
            Some(raw)
        }).unwrap()
    }

    fn force_callback_collections(session: &RSession) {
        session.with_active_in(|owner| unsafe {
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
        });
    }

    #[test]
    fn starma_allocation_failure_releases_native_state_and_retry_succeeds() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let baseline = live_allocations();
            session.with_active_in(|owner| unsafe {
                let count = (*owner).arena.node_count();
                (*owner).arena.set_budget(ArenaBudget::new(0, count));
            });
            let failure = std::panic::catch_unwind(AssertUnwindSafe(|| unsafe { setup(&values) }))
                .expect_err("constrained actual arena must reject the external pointer");
            let error = failure.downcast_ref::<crate::sexp::context::RError>().unwrap();
            assert!(error.message.contains("could not allocate starma pointer"), "{}", error.message);
            assert_eq!(live_allocations(), baseline);
            session.with_active_in(|owner| unsafe { (*owner).arena.set_budget(ArenaBudget::unlimited()); });
            let pointer = unsafe { setup(&values) };
            let original = session.sexp(pointer).unwrap();
            assert_eq!(live_allocations(), (baseline.0 + 1, baseline.1 + 14));
            session.with_active_in(|owner| unsafe {
                let count = (*owner).arena.node_count();
                (*owner).arena.set_budget(ArenaBudget::new(0, count));
            });
            let failure = std::panic::catch_unwind(AssertUnwindSafe(|| unsafe {
                c_get_resid(original.as_raw());
            })).expect_err("constrained actual result allocation must fail safely");
            assert!(failure.is::<crate::sexp::context::RError>());
            assert_eq!(live_allocations(), (baseline.0 + 1, baseline.1 + 14));
            session.with_active_in(|owner| unsafe { (*owner).arena.set_budget(ArenaBudget::unlimited()); });
            let residual = unsafe { c_get_resid(original.as_raw()) };
            assert_eq!(unsafe { XLENGTH(residual) }, 3);
            unsafe {
                assert_eq!(starma_state(pointer).state.borrow().n, 3);
                assert_eq!(*starma_state(pointer).state.borrow().w.add(2), 8.0);
                c_free_starma(pointer);
                c_free_starma(pointer); // closing a live empty header is idempotent
                assert!((*pointer).data.extptr().address.is_null());
            }
            assert_eq!(live_allocations(), baseline);
        });
    }

    #[test]
    fn starma_callback_observes_initialized_state_surviving_full_collection() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let baseline = live_allocations();
            let heap = session.with_active_in(|owner| unsafe { (*owner).heap_identity.clone() });
            let notifications = Rc::new(Cell::new(0));
            let observed = notifications.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed.set(observed.get() + 1);
                let roots = memory::fresh_allocation_roots(&heap);
                let (pointer, token) = roots.iter().find(|(_, token)| {
                    heap.node_snapshot(token).unwrap().sxpinfo.type_of() == SEXPTYPE::EXTPTRSXP
                }).expect("actual setup result must be rooted during its callback");
                let pointer = *pointer;
                unsafe {
                    let owned = starma_state(pointer);
                    let state = owned.state.borrow();
                    assert_eq!((state.mp, state.n, state.m, state.method), (1, 3, 0, 0));
                    assert_eq!(std::slice::from_raw_parts(state.w, 3), &[2.0, 4.0, 8.0]);
                    assert_eq!(std::slice::from_raw_parts(state.wkeep, 3), &[2.0, 4.0, 8.0]);
                    assert!(!state.params.is_null() && !state.reg.is_null());
                }
                crate::sexp::gengc::full_gc();
                assert!(token.is_live());
                assert_eq!(unsafe { *starma_state(pointer).state.borrow().w.add(2) }, 8.0);
            }));
            force_callback_collections(&session);
            let pointer = unsafe { setup(&values) };
            assert_eq!(notifications.get(), 1);
            let root = session.sexp(pointer).unwrap();
            assert_eq!(root.typeof_(), SEXPTYPE::EXTPTRSXP);
            unsafe { c_free_starma(root.as_raw()); }
            assert_eq!(live_allocations(), baseline);
        });
    }

    #[test]
    fn starma_ar1_calculation_survives_collecting_and_mutating_result_callbacks() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let baseline = live_allocations();
            let pointer = unsafe { setup(&values) };
            let original = session.sexp(pointer).unwrap();
            let factory = session.owner_token().unwrap().node_factory();
            let method = factory.allocate(|arena| {
                let raw = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
                if raw.is_null() { return None; }
                unsafe { INTEGER(raw).write(1); }
                Some(raw)
            }).unwrap();
            let parameters = factory.allocate(|arena| {
                let raw = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
                if raw.is_null() { return None; }
                unsafe { REAL(raw).write(0.5); }
                Some(raw)
            }).unwrap();
            unsafe { c_starma_method(pointer, method.as_raw()); }
            let method_raw = method.as_raw();
            let notifications = Rc::new(Cell::new(0));
            let called = notifications.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                called.set(called.get() + 1);
                crate::sexp::gengc::full_gc();
                // A mutable callback must acquire a new RefCell loan; the
                // numerical operation's previous loan must already be gone.
                unsafe { c_starma_method(pointer, method_raw); }
            }));
            force_callback_collections(&session);
            let objective = unsafe { c_arma0fa(original.as_raw(), parameters.as_raw()) };
            let objective = session.sexp(objective).unwrap();
            let variance = unsafe { c_get_s2(original.as_raw()) };
            let variance = session.sexp(variance).unwrap();
            let residual = unsafe { c_get_resid(original.as_raw()) };
            let residual = session.sexp(residual).unwrap();
            assert!((objective.real_elt(0).unwrap() - 0.5 * (49.0_f64 / 3.0).ln()).abs() < 1e-12);
            assert!((variance.real_elt(0).unwrap() - 49.0 / 3.0).abs() < 1e-12);
            assert_eq!(residual.iter_real().collect::<Vec<_>>(), vec![2.0, 3.0, 6.0]);
            assert_eq!(notifications.get(), 3);
            unsafe { c_free_starma(original.as_raw()); }
            assert_eq!(live_allocations(), baseline);
        });
    }

    #[test]
    fn starma_full_gc_releases_unreachable_native_state() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let baseline = live_allocations();
            let pointer = unsafe { setup(&values) };
            let (_, token) = memory::checked_projection(pointer).unwrap();
            let native = Rc::downgrade(&starma_state(pointer));
            assert_eq!(live_allocations(), (baseline.0 + 1, baseline.1 + 14));
            crate::sexp::gengc::full_gc();
            assert!(!token.is_live());
            assert!(native.upgrade().is_none());
            assert_eq!(live_allocations(), baseline);
            crate::sexp::gengc::full_gc();
            assert_eq!(live_allocations(), baseline);
        });
    }

    #[test]
    fn starma_owner_close_releases_native_state_without_explicit_free() {
        let baseline = live_allocations();
        let session = RSession::new_for_gc_tests();
        let values = inputs(&session);
        let pointer = session.with_active(|| unsafe { setup(&values) });
        let (_, token) = memory::checked_projection(pointer).unwrap();
        let native = Rc::downgrade(&starma_state(pointer));
        assert_eq!(live_allocations(), (baseline.0 + 1, baseline.1 + 14));
        drop(values);
        drop(session);
        assert!(!token.is_live());
        assert!(native.upgrade().is_none());
        assert_eq!(live_allocations(), baseline);
    }

    #[test]
    fn starma_rejects_malformed_dimensions_and_short_input_before_native_allocation() {
        fn set_integer(value: &Sexp<'_>, index: i64, integer: i32) {
            crate::sexp::SexpMut::try_from_checked(value.clone()).unwrap()
                .try_set_integer_elt(index, integer).unwrap();
        }
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let baseline = live_allocations();
            for case in 0..8 {
                let mut values = inputs(&session);
                match case {
                    0 => values[0] = values[2].clone(), // one order, not five
                    1 => set_integer(&values[0], 0, -1),
                    2 => set_integer(&values[0], 0, crate::sexp::ffi::NA_INTEGER),
                    3 => set_integer(&values[2], 0, 4), // only 3 observations
                    4 => set_integer(&values[4], 0, 1), // missing regression column
                    5 => set_integer(&values[7], 0, 4), // conditioning > observations
                    6 => {
                        set_integer(&values[0], 2, i32::MAX);
                        set_integer(&values[0], 4, i32::MAX);
                    }, // seasonal product overflows the native kernel dimension
                    7 => set_integer(&values[0], 0, 100_000), // covariance triangle too large
                    _ => unreachable!(),
                }
                let failure = std::panic::catch_unwind(AssertUnwindSafe(|| unsafe { setup(&values) }))
                    .expect_err("malformed actual setup inputs must be rejected");
                assert!(failure.is::<crate::sexp::context::RError>());
                assert_eq!(live_allocations(), baseline);
            }
            let values = inputs(&session);
            let pointer = unsafe { setup(&values) };
            let original = session.sexp(pointer).unwrap();
            let failure = std::panic::catch_unwind(AssertUnwindSafe(|| unsafe {
                c_arma0fa(original.as_raw(), values[3].as_raw());
            })).expect_err("empty parameters cannot feed an AR(1) native state");
            assert_eq!(failure.downcast_ref::<crate::sexp::context::RError>().unwrap().message, "too few starma parameters");
            unsafe { c_free_starma(original.as_raw()); }
            assert_eq!(live_allocations(), baseline);
        });
    }

    #[test]
    fn starma_rejects_unrelated_external_pointer_with_same_opaque_address() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let baseline = live_allocations();
            let pointer = unsafe { setup(&values) };
            let original = session.sexp(pointer).unwrap();
            let address = unsafe { (*pointer).data.extptr().address };
            let factory = session.owner_token().unwrap().node_factory();
            let unrelated = factory.allocate(|arena| {
                let pointer = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
                if pointer.is_null() { return None; }
                unsafe { (*pointer).data.extptr_mut().address = address; }
                Some(pointer)
            }).unwrap();
            let failure = std::panic::catch_unwind(AssertUnwindSafe(|| unsafe {
                c_free_starma(unrelated.as_raw());
            })).expect_err("same opaque address cannot forge the original allocation capability");
            assert_eq!(failure.downcast_ref::<crate::sexp::context::RError>().unwrap().message, "bad starma pointer");
            assert_eq!(live_allocations(), (baseline.0 + 1, baseline.1 + 14));
            unsafe { c_free_starma(original.as_raw()); }
            assert_eq!(live_allocations(), baseline);
        });
    }

    #[test]
    fn starma_result_allocation_pins_native_state_during_reentrant_free() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let baseline = live_allocations();
            let pointer = unsafe { setup(&values) };
            let original = session.sexp(pointer).unwrap();
            let closed = Rc::new(Cell::new(false));
            let callback_closed = closed.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if callback_closed.replace(true) { return; }
                unsafe { c_free_starma(pointer); }
                crate::sexp::gengc::full_gc();
                // The operation owns a separate native lease across callback.
                assert_eq!(live_allocations(), (baseline.0 + 1, baseline.1 + 14));
            }));
            force_callback_collections(&session);
            let residual = unsafe { c_get_resid(original.as_raw()) };
            assert!(closed.get());
            assert_eq!(unsafe { XLENGTH(residual) }, 3);
            assert_eq!(unsafe { std::slice::from_raw_parts(REAL(residual), 3) }, &[0.0, 0.0, 0.0]);
            assert_eq!(live_allocations(), baseline);
            assert!(unsafe { (*original.as_raw()).data.extptr().address.is_null() });
        });
    }

    #[test]
    fn starma_callback_unwind_clears_tentative_pointer_and_retry_succeeds() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let baseline = live_allocations();
            let heap = session.with_active_in(|owner| unsafe { (*owner).heap_identity.clone() });
            let observed = Rc::new(Cell::new(None));
            let pointer_seen = observed.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if pointer_seen.get().is_some() { return; }
                let roots = memory::fresh_allocation_roots(&heap);
                let (pointer, _) = roots.iter().find(|(_, token)| {
                    heap.node_snapshot(token).unwrap().sxpinfo.type_of() == SEXPTYPE::EXTPTRSXP
                }).expect("actual tentative setup result");
                pointer_seen.set(Some(*pointer));
                crate::sexp::gengc::full_gc();
                panic!("injected starma allocation callback unwind");
            }));
            force_callback_collections(&session);
            std::panic::catch_unwind(AssertUnwindSafe(|| unsafe { setup(&values) }))
                .expect_err("the actual allocation callback must unwind setup");
            assert_eq!(live_allocations(), baseline);
            let pointer = observed.get().expect("callback saw the tentative pointer");
            let (_, token) = memory::checked_projection(pointer).unwrap();
            let header = token.heap_identity().node_snapshot(&token).unwrap();
            assert!(header.data.extptr().address.is_null());
            let retry = unsafe { setup(&values) };
            unsafe { c_free_starma(retry); }
            assert_eq!(live_allocations(), baseline);
        });
    }
    #[test]
    fn starma_incompatible_publication_is_rejected_without_changing_attached_state() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let baseline = live_allocations();
            let heap = session.with_active_in(|owner| unsafe { (*owner).heap_identity.clone() });
            let observed = Rc::new(RefCell::new(None));
            let saved = observed.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if saved.borrow().is_some() { return; }
                let roots = memory::fresh_allocation_roots(&heap);
                let (_, token) = roots.iter().find(|(_, token)| {
                    heap.resource::<StarmaState>(token).is_some()
                }).expect("tentative STARMA state is attached before callbacks");
                let state = heap.resource::<StarmaState>(token).unwrap();
                *saved.borrow_mut() = Some((token.clone(), Rc::downgrade(&state)));
                let original = heap.node_snapshot(token).unwrap();
                let mut incompatible = original;
                incompatible.sxpinfo.set_type(SEXPTYPE::S4SXP);
                assert!(heap.replace_node(token, incompatible).is_none());
                let current = heap.node_snapshot(token).unwrap();
                assert_eq!(current.sxpinfo.type_and_flags, original.sxpinfo.type_and_flags);
                assert_eq!(current.sxpinfo.rcount, original.sxpinfo.rcount);
                assert_eq!(current.data, original.data);
                assert_eq!(current.attrib, original.attrib);
                assert_eq!(current.gengc_next_node, original.gengc_next_node);
                assert_eq!(current.gengc_prev_node, original.gengc_prev_node);
                assert!(Rc::ptr_eq(&heap.resource::<StarmaState>(token).unwrap(), &state));
                crate::sexp::gengc::full_gc();
            }));
            force_callback_collections(&session);
            let pointer = unsafe { setup(&values) };
            let pointer = session.sexp(pointer).unwrap();
            let (token, native) = observed.borrow().as_ref().unwrap().clone();
            assert!(native.upgrade().is_some());
            assert_eq!(token.heap_identity().node_snapshot(&token).unwrap().sxpinfo.type_of(), SEXPTYPE::EXTPTRSXP);
            unsafe { c_free_starma(pointer.as_raw()); }
            assert!(native.upgrade().is_none());
            assert_eq!(live_allocations(), baseline);
        });
    }

    #[test]
    fn starma_publication_type_changes_release_state_on_rejection_and_unwind() {
        for callback_panics in [false, true] {
            let session = RSession::new_for_gc_tests();
            session.with_active(|| {
                let values = inputs(&session);
                let baseline = live_allocations();
                let heap = session.with_active_in(|owner| unsafe { (*owner).heap_identity.clone() });
                let observed = Rc::new(RefCell::new(None));
                let saved = observed.clone();
                crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                    if saved.borrow().is_some() { return; }
                    let roots = memory::fresh_allocation_roots(&heap);
                    let (_, token) = roots.iter().find(|(_, token)| {
                        heap.resource::<StarmaState>(token).is_some()
                    }).expect("tentative STARMA state is attached before callbacks");
                    let state = heap.resource::<StarmaState>(token).unwrap();
                    *saved.borrow_mut() = Some((token.clone(), Rc::downgrade(&state)));
                    drop(state);
                    crate::sexp::gengc::full_gc();
                    let mut header = heap.node_snapshot(token).unwrap();
                    header.sxpinfo.set_type(SEXPTYPE::S4SXP);
                    header.data = crate::sexp::ffi::NodeBody::for_kind(SEXPTYPE::S4SXP);
                    heap.replace_node(token, header).unwrap();
                    if callback_panics { panic!("changed STARMA publication callback unwind"); }
                }));
                force_callback_collections(&session);
                let failure = std::panic::catch_unwind(AssertUnwindSafe(|| unsafe { setup(&values) }))
                    .expect_err("a changed tentative object cannot publish native state");
                if callback_panics {
                    assert_eq!(failure.downcast_ref::<&str>(), Some(&"changed STARMA publication callback unwind"));
                } else {
                    assert!(failure.downcast_ref::<crate::sexp::context::RError>().unwrap()
                        .message.contains("closed during initialization"));
                }
                assert_eq!(live_allocations(), baseline);
                let (token, native) = observed.borrow().as_ref().unwrap().clone();
                assert!(native.upgrade().is_none());
                assert!(token.is_live());
                let header = token.heap_identity().node_snapshot(&token).unwrap();
                assert_eq!(header.sxpinfo.type_of(), SEXPTYPE::S4SXP);
                assert!(matches!(header.data, crate::sexp::ffi::NodeBody::Other));
                let retry = unsafe { setup(&values) };
                unsafe { c_free_starma(retry); }
                assert_eq!(live_allocations(), baseline);
                crate::sexp::gengc::full_gc();
                assert!(!token.is_live());
            });
        }
    }

    #[test]
    fn starma_forecasts_repeat_without_changing_fitted_state() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let baseline = live_allocations();
            let pointer = unsafe { setup(&values) };
            let original = session.sexp(pointer).unwrap();
            let parameters = real_values(&session, &[0.5]);
            let zero = integer_values(&session, &[0]);
            let one = integer_values(&session, &[1]);
            let two = integer_values(&session, &[2]);
            let ahead = integer_values(&session, &[3]);
            unsafe {
                c_starma_method(pointer, one.as_raw());
                c_arma0fa(pointer, parameters.as_raw());
            }
            let before = {
                let state = starma_state(pointer);
                let g = state.state.borrow();
                (g.n, g.a, g.P, g.s2, state._buffers.clone())
            };
            let calls = Rc::new(Cell::new(0));
            let counted = calls.clone();
            let method = one.as_raw();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                counted.set(counted.get() + 1);
                crate::sexp::gengc::full_gc();
                unsafe { c_starma_method(pointer, method); }
            }));
            force_callback_collections(&session);
            // Closed forms: AR(1) means are phi^h times the last observation;
            // variances sum squared innovation impulse coefficients. Integrated
            // AR(1) uses cumulative impulse coefficients [1, 1.5, 1.75],
            // twice-integrated AR(1) uses [1, 2.5, 4.25]. Stationary filtering
            // scales the first squared residual by 1-phi^2, giving variances
            // (3+9+36)/3=16, (3+9)/2=6 and 3 respectively.
            for (difference, means, variances) in [
                (&zero, [4.0, 2.0, 1.0], [16.0, 20.0, 21.0]),
                (&one, [10.0, 11.0, 11.5], [6.0, 19.5, 37.875]),
                (&two, [13.0, 18.5, 24.25], [3.0, 21.75, 75.9375]),
            ] {
                for _ in 0..2 {
                    let raw = unsafe { c_arma0_kfore(original.as_raw(), difference.as_raw(), zero.as_raw(), ahead.as_raw()) };
                    let result = session.sexp(raw).unwrap();
                    let mean = result.try_vector_elt(0).unwrap();
                    let variance = result.try_vector_elt(1).unwrap();
                    for i in 0..3 {
                        assert!((mean.real_elt(i).unwrap() - means[i as usize]).abs() < 1e-12);
                        assert!((variance.real_elt(i).unwrap() - variances[i as usize]).abs() < 1e-12);
                    }
                    let state = starma_state(pointer);
                    let g = state.state.borrow();
                    assert_eq!((g.n, g.a, g.P, g.s2), (before.0, before.1, before.2, before.3));
                    assert_eq!(state._buffers, before.4);
                }
            }
            let objective = unsafe { c_arma0fa(pointer, parameters.as_raw()) };
            let objective = session.sexp(objective).unwrap();
            let residual = unsafe { c_get_resid(pointer) };
            let residual = session.sexp(residual).unwrap();
            assert!((objective.real_elt(0).unwrap() - 0.5 * (49.0_f64 / 3.0).ln()).abs() < 1e-12);
            assert_eq!(residual.iter_real().collect::<Vec<_>>(), [2.0, 3.0, 6.0]);
            assert!(calls.get() >= 20);
            unsafe { c_free_starma(pointer); }
            assert_eq!(live_allocations(), baseline);
        });
    }

    #[test]
    fn starma_forecast_rejects_history_and_packed_overflow_before_workspaces() {
        use super::super::starma::ForecastError;
        // A packed product exceeds i32 even though d and n individually fit;
        // no observation or covariance allocation is needed to reject it.
        assert_eq!(forecast_dimensions(1, 50_002, 50_000), Err(ForecastError::PackedOverflow));
        assert_eq!(forecast_dimensions(i32::MAX, 3, 1), Err(ForecastError::PackedOverflow));
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let baseline = live_allocations();
            let pointer = unsafe { setup(&values) };
            let original = session.sexp(pointer).unwrap();
            let zero = integer_values(&session, &[0]);
            let ahead = integer_values(&session, &[1]);
            for difference in [3, 4, i32::MAX] {
                let d = integer_values(&session, &[difference]);
                let failure = std::panic::catch_unwind(AssertUnwindSafe(|| unsafe {
                    c_arma0_kfore(original.as_raw(), d.as_raw(), zero.as_raw(), ahead.as_raw());
                })).expect_err("differences require available history");
                let error = failure.downcast_ref::<crate::sexp::context::RError>().unwrap();
                assert!(error.message.contains("more observations"), "{}", error.message);
                assert_eq!(starma_state(pointer).state.borrow().n, 3);
            }
            unsafe { c_free_starma(pointer); }
            assert_eq!(live_allocations(), baseline);
        });
    }

    #[test]
    fn starma_forecast_errors_leave_state_available_for_retry() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let pointer = unsafe { setup(&values) };
            let original = session.sexp(pointer).unwrap();
            let parameters = real_values(&session, &[0.5]);
            let zero = integer_values(&session, &[0]);
            let one = integer_values(&session, &[1]);
            let ahead = integer_values(&session, &[3]);
            unsafe { c_arma0fa(pointer, parameters.as_raw()); }
            session.with_active_in(|owner| unsafe {
                let count = (*owner).arena.node_count();
                (*owner).arena.set_budget(ArenaBudget::new(0, count));
            });
            let failure = std::panic::catch_unwind(AssertUnwindSafe(|| unsafe {
                c_arma0_kfore(original.as_raw(), one.as_raw(), zero.as_raw(), ahead.as_raw());
            })).expect_err("actual forecast result allocation is constrained");
            assert!(failure.is::<crate::sexp::context::RError>());
            session.with_active_in(|owner| unsafe { (*owner).arena.set_budget(ArenaBudget::unlimited()); });
            let retry = unsafe { c_arma0_kfore(original.as_raw(), one.as_raw(), zero.as_raw(), ahead.as_raw()) };
            let retry = session.sexp(retry).unwrap();
            assert!((retry.try_vector_elt(0).unwrap().real_elt(0).unwrap() - 10.0).abs() < 1e-12);
            let notifications = Rc::new(Cell::new(0));
            let callback_count = notifications.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                let prior = callback_count.replace(callback_count.get() + 1);
                if prior == 0 { panic!("forecast output callback unwind"); }
            }));
            force_callback_collections(&session);
            let failure = std::panic::catch_unwind(AssertUnwindSafe(|| unsafe {
                c_arma0_kfore(original.as_raw(), one.as_raw(), zero.as_raw(), ahead.as_raw());
            })).expect_err("actual forecast callback unwinds");
            assert!(failure.is::<&str>());
            assert_eq!(notifications.get(), 1);
            let retry = unsafe { c_arma0_kfore(original.as_raw(), one.as_raw(), zero.as_raw(), ahead.as_raw()) };
            let retry = session.sexp(retry).unwrap();
            assert!((retry.try_vector_elt(0).unwrap().real_elt(0).unwrap() - 10.0).abs() < 1e-12);
            assert_eq!(starma_state(pointer).state.borrow().n, 3);
            unsafe { c_free_starma(pointer); }
        });
    }

    #[test]
    fn starma_explicit_free_releases_attachment_after_address_clear() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let values = inputs(&session);
            let baseline = live_allocations();
            let pointer = unsafe { setup(&values) };
            let original = session.sexp(pointer).unwrap();
            let (_, node) = memory::checked_projection(pointer).unwrap();
            let heap = node.heap_identity();
            let mut header = heap.node_snapshot(&node).unwrap();
            header.data.extptr_mut().address = std::ptr::null_mut();
            heap.replace_node(&node, header).unwrap();
            unsafe {
                c_free_starma(original.as_raw());
                c_free_starma(original.as_raw());
            }
            assert_eq!(live_allocations(), baseline);
            assert!(heap.resource::<StarmaState>(&node).is_none());
        });
    }

}
