// Port of R's src/appl/lbfgsb.c to Rust
//
// L-BFGS-B (version 2.3) - A limited memory algorithm for bound constrained optimization
//
// Original Fortran by Ciyou Zhu, in collaboration with R.H. Byrd, P. Lu-Chen and J. Nocedal.
// f2c translation, then hand-edited.
//
// Byrd, R. H., Lu, P., Nocedal, J. and Zhu, C. (1995) A limited
// memory algorithm for bound constrained optimization.
// SIAM J. Scientific Computing, 16, 1190--1208.

use libm::*;
use std::cell::RefCell;
use std::os::raw::{c_char, c_int};

// =====================================================================
// Inline BLAS replacements
// =====================================================================

#[inline(always)]
unsafe fn daxpy(n: i32, da: f64, dx: *const f64, incx: i32, dy: *mut f64, incy: i32) {
    unsafe {
        if n <= 0 || da == 0.0 {
            return;
        }
        if incx == 1 && incy == 1 {
            for i in 0..n as usize {
                *dy.add(i) += da * *dx.add(i);
            }
        } else {
            let (mut ix, mut iy) = (0usize, 0usize);
            for _ in 0..n {
                *dy.add(iy) += da * *dx.add(ix);
                ix += incx as usize;
                iy += incy as usize;
            }
        }
    }
}

#[inline(always)]
unsafe fn dcopy(n: i32, dx: *const f64, incx: i32, dy: *mut f64, incy: i32) {
    unsafe {
        if n <= 0 {
            return;
        }
        if incx == 1 && incy == 1 {
            for i in 0..n as usize {
                *dy.add(i) = *dx.add(i);
            }
        } else {
            let (mut ix, mut iy) = (0usize, 0usize);
            for _ in 0..n {
                *dy.add(iy) = *dx.add(ix);
                ix += incx as usize;
                iy += incy as usize;
            }
        }
    }
}

#[inline(always)]
unsafe fn ddot(n: i32, dx: *const f64, incx: i32, dy: *const f64, incy: i32) -> f64 {
    unsafe {
        if n <= 0 {
            return 0.0;
        }
        let mut s = 0.0_f64;
        if incx == 1 && incy == 1 {
            for i in 0..n as usize {
                s += *dx.add(i) * *dy.add(i);
            }
        } else {
            let (mut ix, mut iy) = (0usize, 0usize);
            for _ in 0..n {
                s += *dx.add(ix) * *dy.add(iy);
                ix += incx as usize;
                iy += incy as usize;
            }
        }
        s
    }
}

#[inline(always)]
unsafe fn dscal(n: i32, da: f64, dx: *mut f64, incx: i32) {
    unsafe {
        if n <= 0 || da == 1.0 {
            return;
        }
        if incx == 1 {
            for i in 0..n as usize {
                *dx.add(i) *= da;
            }
        } else {
            let mut ix = 0usize;
            for _ in 0..n {
                *dx.add(ix) *= da;
                ix += incx as usize;
            }
        }
    }
}

// =====================================================================
// Inline LINPACK replacements
// =====================================================================

/// dtrsl: solve triangular system. a stored column-major, lower triangle.
/// job: 0 = L*x=b (forward), 11 = L'*x=b (backward)
#[inline(always)]
unsafe fn dtrsl(a: *const f64, lda: i32, n: i32, x: *mut f64, job: i32) -> i32 {
    unsafe {
        if job == 0 {
            for j in 0..n as usize {
                let ajj = *a.add(j * lda as usize + j);
                if ajj == 0.0 {
                    return (j + 1) as i32;
                }
                *x.add(j) /= ajj;
                let tmp = *x.add(j);
                for i in (j + 1)..n as usize {
                    *x.add(i) -= tmp * *a.add(j * lda as usize + i);
                }
            }
        } else {
            for j in (0..n as usize).rev() {
                let mut tmp = *x.add(j);
                for i in (j + 1)..n as usize {
                    tmp -= *a.add(j * lda as usize + i) * *x.add(i);
                }
                let ajj = *a.add(j * lda as usize + j);
                if ajj == 0.0 {
                    return (j + 1) as i32;
                }
                *x.add(j) = tmp / ajj;
            }
        }
        0
    }
}

/// dpofa: Cholesky factorization of symmetric positive definite matrix.
/// Upper triangle stored column-major. On exit: U where A = U'*U.
unsafe fn dpofa(a: *mut f64, lda: i32, n: i32) -> i32 {
    unsafe {
        for j in 0..n as usize {
            let mut s = 0.0_f64;
            for i in 0..j {
                let aij = *a.add(j * lda as usize + i);
                s += aij * aij;
            }
            let ajj = *a.add(j * lda as usize + j) - s;
            if ajj <= 0.0 {
                return (j + 1) as i32;
            }
            *a.add(j * lda as usize + j) = sqrt(ajj);
            let ajj = *a.add(j * lda as usize + j);
            for i in (j + 1)..n as usize {
                let mut s2 = 0.0_f64;
                for k in 0..j {
                    s2 += *a.add(j * lda as usize + k) * *a.add(i * lda as usize + k);
                }
                *a.add(i * lda as usize + j) = (*a.add(i * lda as usize + j) - s2) / ajj;
            }
        }
        0
    }
}

// =====================================================================
// C string helpers
// =====================================================================

#[inline(always)]
unsafe fn cstrncmp(s: *const c_char, prefix: &[u8], n: usize) -> bool {
    unsafe {
        for i in 0..n {
            if i >= prefix.len() {
                return false;
            }
            if *s.add(i) == 0 {
                return false;
            }
            if *s.add(i) as u8 != prefix[i] {
                return false;
            }
        }
        true
    }
}

#[inline(always)]
unsafe fn cstrcpy(dst: *mut c_char, src: &[u8]) {
    unsafe {
        for i in 0..src.len() {
            *dst.add(i) = src[i] as c_char;
        }
        *dst.add(src.len()) = 0;
    }
}

// =====================================================================
// Persistent state for mainlb (replaces C static locals)
// =====================================================================

struct LbfgsbState {
    prjctd: i32,
    cnstnd: i32,
    boxed: i32,
    updatd: i32,
    nintol: i32,
    iback: i32,
    nskip: i32,
    head: i32,
    col: i32,
    itail: i32,
    iter: i32,
    iupdat: i32,
    nint: i32,
    nfgv: i32,
    info: i32,
    ifun: i32,
    iword: i32,
    nfree: i32,
    nact: i32,
    ileave: i32,
    nenter: i32,
    theta: f64,
    fold: f64,
    tol: f64,
    dnorm: f64,
    epsmch: f64,
    gd: f64,
    stpmx: f64,
    sbgnrm: f64,
    stp: f64,
    gdold: f64,
    dtd: f64,
    xstep: f64,
    word: [u8; 4],
    dcsrch_stage: i32,
    dcsrch_brackt: i32,
    dcsrch_ginit: f64,
    dcsrch_gtest: f64,
    dcsrch_gx: f64,
    dcsrch_gy: f64,
    dcsrch_finit: f64,
    dcsrch_fx: f64,
    dcsrch_fy: f64,
    dcsrch_stx: f64,
    dcsrch_sty: f64,
    dcsrch_stmin: f64,
    dcsrch_stmax: f64,
    dcsrch_width: f64,
    dcsrch_width1: f64,
    lws: i32,
    lwy: i32,
    lsy: i32,
    lss: i32,
    lwt: i32,
    lwn: i32,
    lsnd: i32,
    lz: i32,
    lr: i32,
    ld: i32,
    lt: i32,
    lwa: i32,
}

impl LbfgsbState {
    fn new() -> Self {
        LbfgsbState {
            prjctd: 0,
            cnstnd: 0,
            boxed: 0,
            updatd: 0,
            nintol: 0,
            iback: 0,
            nskip: 0,
            head: 1,
            col: 0,
            itail: 0,
            iter: 0,
            iupdat: 0,
            nint: 0,
            nfgv: 0,
            info: 0,
            ifun: 0,
            iword: 0,
            nfree: 0,
            nact: 0,
            ileave: 0,
            nenter: 0,
            theta: 1.0,
            fold: 0.0,
            tol: 0.0,
            dnorm: 0.0,
            epsmch: f64::EPSILON,
            gd: 0.0,
            stpmx: 0.0,
            sbgnrm: 0.0,
            stp: 0.0,
            gdold: 0.0,
            dtd: 0.0,
            xstep: 0.0,
            word: [b'-', b'-', b'-', 0],
            dcsrch_stage: 0,
            dcsrch_brackt: 0,
            dcsrch_ginit: 0.0,
            dcsrch_gtest: 0.0,
            dcsrch_gx: 0.0,
            dcsrch_gy: 0.0,
            dcsrch_finit: 0.0,
            dcsrch_fx: 0.0,
            dcsrch_fy: 0.0,
            dcsrch_stx: 0.0,
            dcsrch_sty: 0.0,
            dcsrch_stmin: 0.0,
            dcsrch_stmax: 0.0,
            dcsrch_width: 0.0,
            dcsrch_width1: 0.0,
            lws: 0,
            lwy: 0,
            lsy: 0,
            lss: 0,
            lwt: 0,
            lwn: 0,
            lsnd: 0,
            lz: 0,
            lr: 0,
            ld: 0,
            lt: 0,
            lwa: 0,
        }
    }
}

thread_local! {
    static LBFGSB_STATE: RefCell<Option<Box<LbfgsbState>>> = RefCell::new(None);
}

// =====================================================================
// Public API: lbfgsb (= setulb)
// =====================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lbfgsb(
    n: c_int,
    m: c_int,
    x: *mut f64,
    l: *mut f64,
    u: *mut f64,
    nbd: *mut c_int,
    f: *mut f64,
    g: *mut f64,
    factr: f64,
    pgtol: *mut f64,
    wa: *mut f64,
    iwa: *mut c_int,
    task: *mut c_char,
    iprint: c_int,
    isave: *mut c_int,
) {
    unsafe {
        LBFGSB_STATE.with(|s| {
            let mut sr = s.borrow_mut();
            if sr.is_none() {
                *sr = Some(Box::new(LbfgsbState::new()));
            }
            let st = sr.as_mut().unwrap();
            let mut csave: [c_char; 60] = [0; 60];

            if cstrncmp(task, b"START", 5) {
                st.lws = 1;
                st.lwy = st.lws + m * n;
                st.lsy = st.lwy + m * n;
                st.lss = st.lsy + m * m;
                st.lwt = st.lss + m * m;
                st.lwn = st.lwt + m * m;
                st.lsnd = st.lwn + (m * m * 4);
                st.lz = st.lsnd + (m * m * 4);
                st.lr = st.lz + n;
                st.ld = st.lr + n;
                st.lt = st.ld + n;
                st.lwa = st.lt + n;
            }

            mainlb(
                n,
                m,
                x,
                l,
                u,
                nbd,
                f,
                g,
                factr,
                pgtol,
                wa.add((st.lws - 1) as usize),
                wa.add((st.lwy - 1) as usize),
                wa.add((st.lsy - 1) as usize),
                wa.add((st.lss - 1) as usize),
                wa.add((st.lwt - 1) as usize),
                wa.add((st.lwn - 1) as usize),
                wa.add((st.lsnd - 1) as usize),
                wa.add((st.lz - 1) as usize),
                wa.add((st.lr - 1) as usize),
                wa.add((st.ld - 1) as usize),
                wa.add((st.lt - 1) as usize),
                wa.add((st.lwa - 1) as usize),
                iwa,
                iwa.add(n as usize),
                iwa.add((2 * n) as usize),
                task,
                iprint,
                csave.as_mut_ptr(),
                isave,
                st,
            );
        });
    }
}

// =====================================================================
// mainlb
// =====================================================================

unsafe fn mainlb(
    n: i32,
    m: i32,
    x: *mut f64,
    l: *const f64,
    u: *const f64,
    nbd: *const i32,
    f: *mut f64,
    g: *mut f64,
    factr: f64,
    pgtol: *const f64,
    ws: *mut f64,
    wy: *mut f64,
    sy: *mut f64,
    ss: *mut f64,
    wt: *mut f64,
    wn: *mut f64,
    snd: *mut f64,
    z: *mut f64,
    r: *mut f64,
    d: *mut f64,
    t: *mut f64,
    wa: *mut f64,
    indx: *mut i32,
    iwhere: *mut i32,
    indx2: *mut i32,
    task: *mut c_char,
    iprint: i32,
    csave: *mut c_char,
    isave: *mut i32,
    st: &mut LbfgsbState,
) {
    unsafe {
        // All arrays 0-based.
        // ws, wy: n x m col-major => ws[i + j*n]
        // sy, ss, wt: m x m col-major => sy[i + j*m]
        // wn, snd: 2m x 2m col-major => wn[i + j*(2*m)]

        let mut k: i32 = 0;
        let mut wrk: i32 = 0;
        let mut dr: f64 = 0.0;
        let mut rr: f64 = 0.0;
        let mut ddum: f64;

        if cstrncmp(task, b"START", 5) {
            st.epsmch = f64::EPSILON;
            st.fold = 0.0;
            st.dnorm = 0.0;
            st.gd = 0.0;
            st.sbgnrm = 0.0;
            st.stp = 0.0;
            st.xstep = 0.0;
            st.stpmx = 0.0;
            st.gdold = 0.0;
            st.dtd = 0.0;
            st.col = 0;
            st.head = 1;
            st.theta = 1.0;
            st.iupdat = 0;
            st.updatd = 0;
            st.iback = 0;
            st.itail = 0;
            st.ifun = 0;
            st.iword = 0;
            st.nact = 0;
            st.ileave = 0;
            st.nenter = 0;
            st.iter = 0;
            st.nfgv = 0;
            st.nint = 0;
            st.nintol = 0;
            st.nskip = 0;
            st.nfree = n;
            st.tol = factr * st.epsmch;
            st.word = [b'-', b'-', b'-', 0];
            st.info = 0;

            errclb(n, m, factr, l, u, nbd, task, &mut st.info, &mut k);
            if cstrncmp(task, b"ERROR", 5) {
                prn3lb(
                    n,
                    x,
                    f,
                    task,
                    iprint,
                    st.info,
                    st.iter,
                    st.nfgv,
                    st.nintol,
                    st.nskip,
                    st.nact,
                    st.sbgnrm,
                    st.nint,
                    st.word.as_ptr() as *const c_char,
                    st.iback,
                    st.stp,
                    st.xstep,
                    k,
                );
                return;
            }
            prn1lb(n, m, l, u, x, iprint, st.epsmch);
            active(
                n,
                l,
                u,
                nbd,
                x,
                iwhere,
                iprint,
                &mut st.prjctd,
                &mut st.cnstnd,
                &mut st.boxed,
            );
            cstrcpy(task, b"FG_START");
            return;
        }

        // Dispatch on re-entry task
        if cstrncmp(task, b"FG_LN", 5) {
            // L666: re-enter line search
            lnsrlb(
                n,
                l,
                u,
                nbd,
                x,
                f,
                &mut st.fold,
                &mut st.gd,
                &mut st.gdold,
                g,
                d,
                r,
                t,
                z,
                &mut st.stp,
                &mut st.dnorm,
                &mut st.dtd,
                &mut st.xstep,
                &mut st.stpmx,
                &st.iter,
                &mut st.ifun,
                &mut st.iback,
                &mut st.nfgv,
                &mut st.info,
                task,
                &st.boxed,
                &st.cnstnd,
                csave,
                st,
            );
            if st.info != 0 || st.iback >= 20 {
                dcopy(n, t, 1, x, 1);
                dcopy(n, r, 1, g, 1);
                *f = st.fold;
                if st.col == 0 {
                    if st.info == 0 {
                        st.info = -9;
                        st.nfgv -= 1;
                        st.ifun -= 1;
                        st.iback -= 1;
                    }
                    cstrcpy(task, b"ERROR: ABNORMAL_TERMINATION_IN_LNSRCH");
                    st.iter += 1;
                    prn3lb(
                        n,
                        x,
                        f,
                        task,
                        iprint,
                        st.info,
                        st.iter,
                        st.nfgv,
                        st.nintol,
                        st.nskip,
                        st.nact,
                        st.sbgnrm,
                        st.nint,
                        st.word.as_ptr() as *const c_char,
                        st.iback,
                        st.stp,
                        st.xstep,
                        0,
                    );
                    return;
                } else {
                    if iprint >= 1 {
                        eprintln!(
                            "{}\n{}",
                            "Bad direction in the line search;",
                            "   refresh the lbfgs memory and restart the iteration."
                        );
                    }
                    if st.info == 0 {
                        st.nfgv -= 1;
                    }
                    st.info = 0;
                    st.col = 0;
                    st.head = 1;
                    st.theta = 1.0;
                    st.iupdat = 0;
                    st.updatd = 0;
                    cstrcpy(task, b"RESTART_FROM_LNSRCH");
                    return;
                }
            } else if cstrncmp(task, b"FG_LN", 5) {
                return;
            } else {
                // task = NEW_X
                st.iter += 1;
                projgr(n, l, u, nbd, x, g, &mut st.sbgnrm);
                prn2lb(
                    n,
                    x,
                    f,
                    g,
                    iprint,
                    st.iter,
                    st.nfgv,
                    st.nact,
                    st.sbgnrm,
                    st.nint,
                    st.word.as_ptr() as *const c_char,
                    st.iword,
                    st.iback,
                    st.stp,
                    st.xstep,
                );
                return;
            }
        }

        if cstrncmp(task, b"NEW_X", 5) {
            // L777: test for termination and do update
            if st.sbgnrm <= *pgtol {
                cstrcpy(task, b"CONVERGENCE: NORM OF PROJECTED GRADIENT <= PGTOL");
                prn3lb(
                    n,
                    x,
                    f,
                    task,
                    iprint,
                    st.info,
                    st.iter,
                    st.nfgv,
                    st.nintol,
                    st.nskip,
                    st.nact,
                    st.sbgnrm,
                    st.nint,
                    st.word.as_ptr() as *const c_char,
                    st.iback,
                    st.stp,
                    st.xstep,
                    0,
                );
                return;
            }
            ddum = fabs(st.fold).max(fabs(*f)).max(1.0);
            if st.fold - *f <= st.tol * ddum {
                cstrcpy(task, b"CONVERGENCE: REL_REDUCTION_OF_F <= FACTR*EPSMCH");
                if st.iback >= 10 {
                    st.info = -5;
                }
                prn3lb(
                    n,
                    x,
                    f,
                    task,
                    iprint,
                    st.info,
                    st.iter,
                    st.nfgv,
                    st.nintol,
                    st.nskip,
                    st.nact,
                    st.sbgnrm,
                    st.nint,
                    st.word.as_ptr() as *const c_char,
                    st.iback,
                    st.stp,
                    st.xstep,
                    0,
                );
                return;
            }
            // Compute r = newg - oldg, rr = y'y, dr = y's
            for i in 0..n as usize {
                *r.add(i) = *g.add(i) - *r.add(i);
            }
            rr = ddot(n, r, 1, r, 1);
            if st.stp == 1.0 {
                dr = st.gd - st.gdold;
                ddum = -st.gdold;
            } else {
                dr = (st.gd - st.gdold) * st.stp;
                dscal(n, st.stp, d, 1);
                ddum = -st.gdold * st.stp;
            }
            if dr <= st.epsmch * ddum {
                st.nskip += 1;
                st.updatd = 0;
                if iprint >= 1 {
                    eprintln!("ys={:10.3e}  -gs={:10.3e}, BFGS update SKIPPED", dr, ddum);
                }
                // fall through to L222
            } else {
                st.updatd = 1;
                st.iupdat += 1;
                matupd(
                    n,
                    m,
                    ws,
                    wy,
                    sy,
                    ss,
                    d,
                    r,
                    &mut st.itail,
                    &st.iupdat,
                    &mut st.col,
                    &mut st.head,
                    &mut st.theta,
                    &rr,
                    &dr,
                    &st.stp,
                    &st.dtd,
                );
                formt(m, wt, sy, ss, &st.col, &st.theta, &mut st.info);
                if st.info != 0 {
                    if iprint >= 0 {
                        eprintln!(
                            "{}\n{}",
                            "Nonpositive definiteness in Cholesky factorization in formt();",
                            "   refresh the lbfgs memory and restart the iteration."
                        );
                    }
                    st.info = 0;
                    st.col = 0;
                    st.head = 1;
                    st.theta = 1.0;
                    st.iupdat = 0;
                    st.updatd = 0;
                    // fall through to L222
                }
            }
            // L888 -> L222
        }

        // Handle FG_ST or fall through to L222
        if cstrncmp(task, b"FG_ST", 5) {
            // L111
            st.nfgv = 1;
            projgr(n, l, u, nbd, x, g, &mut st.sbgnrm);
            if iprint >= 1 {
                eprintln!(
                    "At iterate {:5}  f= {:12.5e}  |proj g|= {:12.5e}",
                    st.iter, *f, st.sbgnrm
                );
            }
            if st.sbgnrm <= *pgtol {
                cstrcpy(task, b"CONVERGENCE: NORM OF PROJECTED GRADIENT <= PGTOL");
                prn3lb(
                    n,
                    x,
                    f,
                    task,
                    iprint,
                    st.info,
                    st.iter,
                    st.nfgv,
                    st.nintol,
                    st.nskip,
                    st.nact,
                    st.sbgnrm,
                    st.nint,
                    st.word.as_ptr() as *const c_char,
                    st.iback,
                    st.stp,
                    st.xstep,
                    0,
                );
                return;
            }
            // fall through to L222
        }

        if cstrncmp(task, b"STOP", 4) {
            if cstrncmp(task.add(6), b"CPU", 3) {
                dcopy(n, t, 1, x, 1);
                dcopy(n, r, 1, g, 1);
                *f = st.fold;
            }
            prn3lb(
                n,
                x,
                f,
                task,
                iprint,
                st.info,
                st.iter,
                st.nfgv,
                st.nintol,
                st.nskip,
                st.nact,
                st.sbgnrm,
                st.nint,
                st.word.as_ptr() as *const c_char,
                st.iback,
                st.stp,
                st.xstep,
                0,
            );
            return;
        }

        // L222: main loop
        loop {
            if iprint >= 99 {
                eprintln!("Iteration {:5}", st.iter);
            }
            st.iword = -1;

            if st.cnstnd == 0 && st.col > 0 {
                dcopy(n, x, 1, z, 1);
                wrk = st.updatd;
                st.nint = 0;
            } else {
                cauchy(
                    n,
                    x,
                    l,
                    u,
                    nbd,
                    g,
                    indx2,
                    iwhere,
                    t,
                    d,
                    z,
                    m,
                    wy,
                    ws,
                    sy,
                    wt,
                    &st.theta,
                    &st.col,
                    &st.head,
                    wa,
                    wa.add((2 * m) as usize),
                    wa.add((4 * m) as usize),
                    wa.add((6 * m) as usize),
                    &mut st.nint,
                    iprint,
                    &st.sbgnrm,
                    &mut st.info,
                    &st.epsmch,
                );
                if st.info != 0 {
                    if iprint >= 1 {
                        eprintln!(
                            "{}\n{}",
                            "Singular triangular system detected;",
                            "   refresh the lbfgs memory and restart the iteration."
                        );
                    }
                    st.info = 0;
                    st.col = 0;
                    st.head = 1;
                    st.theta = 1.0;
                    st.iupdat = 0;
                    st.updatd = 0;
                    continue;
                }
                st.nintol += st.nint;
                freev(
                    n,
                    &mut st.nfree,
                    indx,
                    &mut st.nenter,
                    &mut st.ileave,
                    indx2,
                    iwhere,
                    &mut wrk,
                    &st.updatd,
                    &st.cnstnd,
                    iprint,
                    &st.iter,
                );
                st.nact = n - st.nfree;
            }

            // L333
            if st.nfree == 0 || st.col == 0 {
                // skip subspace minimization
            } else {
                if wrk != 0 {
                    formk(
                        n,
                        &mut st.nfree,
                        indx,
                        &st.nenter,
                        &st.ileave,
                        indx2,
                        &st.iupdat,
                        &st.updatd,
                        wn,
                        snd,
                        m,
                        ws,
                        wy,
                        sy,
                        &st.theta,
                        &st.col,
                        &st.head,
                        &mut st.info,
                    );
                }
                if st.info != 0 {
                    if iprint >= 0 {
                        eprintln!(
                            "{}\n{}",
                            "Nonpositive definiteness in Cholesky factorization in formk;",
                            "   refresh the lbfgs memory and restart the iteration."
                        );
                    }
                    st.info = 0;
                    st.col = 0;
                    st.head = 1;
                    st.theta = 1.0;
                    st.iupdat = 0;
                    st.updatd = 0;
                    continue;
                }
                cmprlb(
                    n,
                    m,
                    x,
                    g,
                    ws,
                    wy,
                    sy,
                    wt,
                    z,
                    r,
                    wa,
                    indx,
                    &st.theta,
                    &st.col,
                    &st.head,
                    &st.nfree,
                    &st.cnstnd,
                    &mut st.info,
                );
                if st.info != 0 {
                    if iprint >= 1 {
                        eprintln!(
                            "{}\n{}",
                            "Singular triangular system detected;",
                            "   refresh the lbfgs memory and restart the iteration."
                        );
                    }
                    st.info = 0;
                    st.col = 0;
                    st.head = 1;
                    st.theta = 1.0;
                    st.iupdat = 0;
                    st.updatd = 0;
                    continue;
                }
                subsm(
                    n,
                    m,
                    &st.nfree,
                    indx,
                    l,
                    u,
                    nbd,
                    z,
                    r,
                    ws,
                    wy,
                    &st.theta,
                    &st.col,
                    &st.head,
                    &mut st.iword,
                    wa,
                    wn,
                    iprint,
                    &mut st.info,
                );
                if st.info != 0 {
                    if iprint >= 1 {
                        eprintln!(
                            "{}\n{}",
                            "Singular triangular system detected;",
                            "   refresh the lbfgs memory and restart the iteration."
                        );
                    }
                    st.info = 0;
                    st.col = 0;
                    st.head = 1;
                    st.theta = 1.0;
                    st.iupdat = 0;
                    st.updatd = 0;
                    continue;
                }
            }

            // L555: line search
            for i in 0..n as usize {
                *d.add(i) = *z.add(i) - *x.add(i);
            }

            // L666
            lnsrlb(
                n,
                l,
                u,
                nbd,
                x,
                f,
                &mut st.fold,
                &mut st.gd,
                &mut st.gdold,
                g,
                d,
                r,
                t,
                z,
                &mut st.stp,
                &mut st.dnorm,
                &mut st.dtd,
                &mut st.xstep,
                &mut st.stpmx,
                &st.iter,
                &mut st.ifun,
                &mut st.iback,
                &mut st.nfgv,
                &mut st.info,
                task,
                &st.boxed,
                &st.cnstnd,
                csave,
                st,
            );

            if st.info != 0 || st.iback >= 20 {
                dcopy(n, t, 1, x, 1);
                dcopy(n, r, 1, g, 1);
                *f = st.fold;
                if st.col == 0 {
                    if st.info == 0 {
                        st.info = -9;
                        st.nfgv -= 1;
                        st.ifun -= 1;
                        st.iback -= 1;
                    }
                    cstrcpy(task, b"ERROR: ABNORMAL_TERMINATION_IN_LNSRCH");
                    st.iter += 1;
                    prn3lb(
                        n,
                        x,
                        f,
                        task,
                        iprint,
                        st.info,
                        st.iter,
                        st.nfgv,
                        st.nintol,
                        st.nskip,
                        st.nact,
                        st.sbgnrm,
                        st.nint,
                        st.word.as_ptr() as *const c_char,
                        st.iback,
                        st.stp,
                        st.xstep,
                        0,
                    );
                    return;
                } else {
                    if iprint >= 1 {
                        eprintln!(
                            "{}\n{}",
                            "Bad direction in the line search;",
                            "   refresh the lbfgs memory and restart the iteration."
                        );
                    }
                    if st.info == 0 {
                        st.nfgv -= 1;
                    }
                    st.info = 0;
                    st.col = 0;
                    st.head = 1;
                    st.theta = 1.0;
                    st.iupdat = 0;
                    st.updatd = 0;
                    cstrcpy(task, b"RESTART_FROM_LNSRCH");
                    return;
                }
            } else if cstrncmp(task, b"FG_LN", 5) {
                return;
            } else {
                st.iter += 1;
                projgr(n, l, u, nbd, x, g, &mut st.sbgnrm);
                prn2lb(
                    n,
                    x,
                    f,
                    g,
                    iprint,
                    st.iter,
                    st.nfgv,
                    st.nact,
                    st.sbgnrm,
                    st.nint,
                    st.word.as_ptr() as *const c_char,
                    st.iword,
                    st.iback,
                    st.stp,
                    st.xstep,
                );
                return;
            }
        }
    }
}

// =====================================================================
// active
// =====================================================================

unsafe fn active(
    n: i32,
    l: *const f64,
    u: *const f64,
    nbd: *const i32,
    x: *mut f64,
    iwhere: *mut i32,
    iprint: i32,
    prjctd: *mut i32,
    cnstnd: *mut i32,
    boxed: *mut i32,
) {
    unsafe {
        let mut nbdd = 0i32;
        *prjctd = 0;
        *cnstnd = 0;
        *boxed = 1;
        for i in 0..n as usize {
            if *nbd.add(i) > 0 {
                if *nbd.add(i) <= 2 && *x.add(i) <= *l.add(i) {
                    if *x.add(i) < *l.add(i) {
                        *prjctd = 1;
                        *x.add(i) = *l.add(i);
                    }
                    nbdd += 1;
                } else if *nbd.add(i) >= 2 && *x.add(i) >= *u.add(i) {
                    if *x.add(i) > *u.add(i) {
                        *prjctd = 1;
                        *x.add(i) = *u.add(i);
                    }
                    nbdd += 1;
                }
            }
        }
        for i in 0..n as usize {
            if *nbd.add(i) != 2 {
                *boxed = 0;
            }
            if *nbd.add(i) == 0 {
                *iwhere.add(i) = -1;
            } else {
                *cnstnd = 1;
                if *nbd.add(i) == 2 && *u.add(i) - *l.add(i) <= 0.0 {
                    *iwhere.add(i) = 3;
                } else {
                    *iwhere.add(i) = 0;
                }
            }
        }
        if iprint >= 0 {
            if *prjctd != 0 {
                eprintln!("The initial X is infeasible.  Restart with its projection.");
            }
            if *cnstnd == 0 {
                eprintln!("This problem is unconstrained.");
            }
        }
        if iprint > 0 {
            eprintln!("At X0, {} variables are exactly at the bounds", nbdd);
        }
    }
}

// =====================================================================
// bmv
// =====================================================================

unsafe fn bmv(
    m: i32,
    sy: *const f64,
    wt: *const f64,
    col: *const i32,
    v: *const f64,
    p: *mut f64,
    info: *mut i32,
) {
    unsafe {
        let col_v = *col;
        if col_v == 0 {
            return;
        }
        let col_i = col_v as usize;
        let m_i = m as usize;

        // PART I
        *p.add(col_i) = *v.add(col_i);
        for i in 1..col_i {
            let i2 = col_i + i;
            let mut sum = 0.0_f64;
            for k in 0..i as usize {
                sum += *sy.add(i + k * m_i) * *v.add(k) / *sy.add(k + k * m_i);
            }
            *p.add(i2) = *v.add(i2) + sum;
        }
        *info = dtrsl(wt, m, col_v, p.add(col_i), 11);
        if *info != 0 {
            return;
        }

        for i in 0..col_i {
            *p.add(i) = *v.add(i) / sqrt(*sy.add(i + i * m_i));
        }

        // PART II
        *info = dtrsl(wt, m, col_v, p.add(col_i), 0);
        if *info != 0 {
            return;
        }

        for i in 0..col_i {
            *p.add(i) = -*p.add(i) / sqrt(*sy.add(i + i * m_i));
        }
        for i in 0..col_i {
            let mut sum = 0.0_f64;
            for k in (i + 1)..col_i {
                sum += *sy.add(k + i * m_i) * *p.add(col_i + k) / *sy.add(i + i * m_i);
            }
            *p.add(i) += sum;
        }
    }
}

// =====================================================================
// cauchy
// =====================================================================

unsafe fn cauchy(
    n: i32,
    x: *const f64,
    l: *const f64,
    u: *const f64,
    nbd: *const i32,
    g: *const f64,
    iorder: *mut i32,
    iwhere: *mut i32,
    t: *mut f64,
    d: *mut f64,
    xcp: *mut f64,
    m: i32,
    wy: *const f64,
    ws: *const f64,
    sy: *const f64,
    wt: *const f64,
    theta: *const f64,
    col: *const i32,
    head: *const i32,
    p: *mut f64,
    c: *mut f64,
    wbp: *mut f64,
    v: *mut f64,
    nint: *mut i32,
    iprint: i32,
    sbgnrm: *const f64,
    info: *mut i32,
    epsmch: *const f64,
) {
    unsafe {
        let n_i = n as usize;
        let m_i = m as usize;
        let col2 = *col * 2;
        let mut bkmin = 0.0_f64;
        let mut f1 = 0.0_f64;
        let mut f2 = 0.0_f64;
        let mut dibp = 0.0_f64;
        let mut zibp = 0.0_f64;
        let mut neggi = 0.0_f64;
        let mut tsum = 0.0_f64;
        let mut dt = 0.0_f64;
        let mut tj = 0.0_f64;
        let mut tj0 = 0.0_f64;
        let mut dtm = 0.0_f64;
        let mut wmc = 0.0_f64;
        let mut wmp = 0.0_f64;
        let mut wmw = 0.0_f64;
        let mut ibp = 0i32;
        let mut iter = 0i32;
        let mut nfree = n + 1;
        let mut nbreak = 0i32;
        let mut nleft = 0i32;
        let mut ibkmin = 0i32;
        let mut bnded = 1i32;

        if *sbgnrm <= 0.0 {
            if iprint >= 0 {
                eprintln!("Subgnorm = 0.  GCP = X.");
            }
            dcopy(n, x, 1, xcp, 1);
            return;
        }
        if iprint >= 99 {
            eprintln!("\n---------------- CAUCHY entered-------------------\n");
        }

        for i in 0..col2 as usize {
            *p.add(i) = 0.0;
        }

        for i in 0..n_i {
            neggi = -*g.add(i);
            let iw = *iwhere.add(i);
            if iw != 3 && iw != -1 {
                let tl = if *nbd.add(i) <= 2 {
                    *x.add(i) - *l.add(i)
                } else {
                    0.0
                };
                let tu = if *nbd.add(i) >= 2 {
                    *u.add(i) - *x.add(i)
                } else {
                    0.0
                };
                let xlower = *nbd.add(i) <= 2 && tl <= 0.0;
                let xupper = *nbd.add(i) >= 2 && tu <= 0.0;
                *iwhere.add(i) = 0;
                if xlower {
                    if neggi <= 0.0 {
                        *iwhere.add(i) = 1;
                    }
                } else if xupper {
                    if neggi >= 0.0 {
                        *iwhere.add(i) = 2;
                    }
                } else {
                    if fabs(neggi) <= 0.0 {
                        *iwhere.add(i) = -3;
                    }
                }
            }

            let mut pointr = (*head - 1) as usize;
            let iw = *iwhere.add(i);
            if iw != 0 && iw != -1 {
                *d.add(i) = 0.0;
            } else {
                *d.add(i) = neggi;
                f1 -= neggi * neggi;
                for j in 0..*col as usize {
                    *p.add(j) += *wy.add(i + pointr * n_i) * neggi;
                    *p.add(*col as usize + j) += *ws.add(i + pointr * n_i) * neggi;
                    pointr = (pointr + 1) % m_i;
                }
                if *nbd.add(i) <= 2 && *nbd.add(i) != 0 && neggi < 0.0 {
                    nbreak += 1;
                    *iorder.add((nbreak - 1) as usize) = (i + 1) as i32;
                    let tl = *x.add(i) - *l.add(i);
                    *t.add((nbreak - 1) as usize) = tl / (-neggi);
                    if nbreak == 1 || *t.add((nbreak - 1) as usize) < bkmin {
                        bkmin = *t.add((nbreak - 1) as usize);
                        ibkmin = nbreak;
                    }
                } else if *nbd.add(i) >= 2 && neggi > 0.0 {
                    nbreak += 1;
                    *iorder.add((nbreak - 1) as usize) = (i + 1) as i32;
                    let tu = *u.add(i) - *x.add(i);
                    *t.add((nbreak - 1) as usize) = tu / neggi;
                    if nbreak == 1 || *t.add((nbreak - 1) as usize) < bkmin {
                        bkmin = *t.add((nbreak - 1) as usize);
                        ibkmin = nbreak;
                    }
                } else {
                    nfree -= 1;
                    *iorder.add((nfree - 1) as usize) = (i + 1) as i32;
                    if fabs(neggi) > 0.0 {
                        bnded = 0;
                    }
                }
            }
        }

        if *theta != 1.0 {
            dscal(*col, *theta, p.add(*col as usize), 1);
        }
        dcopy(n, x, 1, xcp, 1);
        if nbreak == 0 && nfree == n + 1 {
            if iprint > 100 {
                eprint!("Cauchy X =  ");
                for i in 0..n_i {
                    eprint!("{} ", *xcp.add(i));
                }
                eprintln!();
            }
            return;
        }

        for j in 0..col2 as usize {
            *c.add(j) = 0.0;
        }
        f2 = -*theta * f1;
        let f2_org = f2;
        if *col > 0 {
            bmv(m, sy, wt, col, v, p, info);
            if *info != 0 {
                return;
            }
            f2 -= ddot(col2, v, 1, p, 1);
        }
        dtm = -f1 / f2;
        tsum = 0.0;
        *nint = 1;
        if iprint >= 99 {
            eprintln!("There are {}  breakpoints", nbreak);
        }

        if nbreak == 0 {
            // goto L888
        } else {
            nleft = nbreak;
            iter = 1;
            tj = 0.0;
            loop {
                tj0 = tj;
                if iter == 1 {
                    tj = bkmin;
                    ibp = *iorder.add((ibkmin - 1) as usize);
                } else {
                    if iter == 2 && ibkmin != nbreak {
                        *t.add((ibkmin - 1) as usize) = *t.add((nbreak - 1) as usize);
                        *iorder.add((ibkmin - 1) as usize) = *iorder.add((nbreak - 1) as usize);
                    }
                    hpsolb(nleft, t, iorder, iter - 2);
                    tj = *t.add((nleft - 1) as usize);
                    ibp = *iorder.add((nleft - 1) as usize);
                }
                dt = tj - tj0;
                if dt != 0.0 && iprint >= 100 {
                    eprintln!(
                        "\nPiece    {:3} f1, f2 at start point {:11.4e} {:11.4e}",
                        *nint, f1, f2
                    );
                    eprintln!("Distance to the next break point =  {:11.4e}", dt);
                    eprintln!("Distance to the stationary point =  {:11.4e}", dtm);
                }
                if dtm < dt {
                    break;
                }

                tsum += dt;
                nleft -= 1;
                iter += 1;
                dibp = *d.add((ibp - 1) as usize);
                *d.add((ibp - 1) as usize) = 0.0;
                if dibp > 0.0 {
                    zibp = *u.add((ibp - 1) as usize) - *x.add((ibp - 1) as usize);
                    *xcp.add((ibp - 1) as usize) = *u.add((ibp - 1) as usize);
                    *iwhere.add((ibp - 1) as usize) = 2;
                } else {
                    zibp = *l.add((ibp - 1) as usize) - *x.add((ibp - 1) as usize);
                    *xcp.add((ibp - 1) as usize) = *l.add((ibp - 1) as usize);
                    *iwhere.add((ibp - 1) as usize) = 1;
                }
                if iprint >= 100 {
                    eprintln!("Variable  {}  is fixed.", ibp);
                }
                if nleft == 0 && nbreak == n {
                    dtm = dt;
                    break;
                }

                *nint += 1;
                let dibp2 = dibp * dibp;
                f1 += dt * f2 + dibp2 - *theta * dibp * zibp;
                f2 -= *theta * dibp2;
                if *col > 0 {
                    daxpy(col2, dt, p, 1, c, 1);
                    let mut pointr = (*head - 1) as usize;
                    for j in 0..*col as usize {
                        *wbp.add(j) = *wy.add((ibp - 1) as usize + pointr * n_i);
                        *wbp.add(*col as usize + j) =
                            *theta * *ws.add((ibp - 1) as usize + pointr * n_i);
                        pointr = (pointr + 1) % m_i;
                    }
                    bmv(m, sy, wt, col, wbp, v, info);
                    if *info != 0 {
                        return;
                    }
                    wmc = ddot(col2, c, 1, v, 1);
                    wmp = ddot(col2, p, 1, v, 1);
                    wmw = ddot(col2, wbp, 1, v, 1);
                    daxpy(col2, -dibp, wbp, 1, p, 1);
                    f1 += dibp * wmc;
                    f2 += 2.0 * dibp * wmp - dibp2 * wmw;
                }
                let f2_floor = *epsmch * f2_org;
                if f2 < f2_floor {
                    f2 = f2_floor;
                }
                if nleft > 0 {
                    dtm = -f1 / f2;
                    // continue
                } else if bnded != 0 {
                    f1 = 0.0;
                    f2 = 0.0;
                    dtm = 0.0;
                    break;
                } else {
                    dtm = -f1 / f2;
                    break;
                }
            }
        }

        // L888
        if iprint >= 99 {
            eprintln!("\nGCP found in this segment");
            eprintln!(
                "Piece    {:3} f1, f2 at start point {:11.4e} {:11.4e}",
                *nint, f1, f2
            );
            eprintln!("Distance to the stationary point =  {:11.4e}", dtm);
        }
        if dtm <= 0.0 {
            dtm = 0.0;
        }
        tsum += dtm;
        daxpy(n, tsum, d, 1, xcp, 1);
        // L999
        if *col > 0 {
            daxpy(col2, dtm, p, 1, c, 1);
        }
        if iprint >= 100 {
            eprint!("Cauchy X =  ");
            for i in 0..n_i {
                eprint!("{} ", *xcp.add(i));
            }
            eprintln!();
        }
        if iprint >= 99 {
            eprintln!("\n---------------- exit CAUCHY----------------------\n");
        }
    }
}

// =====================================================================
// cmprlb
// =====================================================================

unsafe fn cmprlb(
    n: i32,
    m: i32,
    x: *const f64,
    g: *const f64,
    ws: *const f64,
    wy: *const f64,
    sy: *const f64,
    wt: *const f64,
    z: *const f64,
    r: *mut f64,
    wa: *mut f64,
    indx: *const i32,
    theta: *const f64,
    col: *const i32,
    head: *const i32,
    nfree: *const i32,
    cnstnd: *const i32,
    info: *mut i32,
) {
    unsafe {
        let n_i = n as usize;
        let m_i = m as usize;
        let col_v = *col;
        if *cnstnd == 0 && col_v > 0 {
            for i in 0..n_i {
                *r.add(i) = -*g.add(i);
            }
        } else {
            let n_f = *nfree;
            for i in 0..n_f as usize {
                let k = (*indx.add(i) - 1) as usize;
                *r.add(i) = -*theta * (*z.add(k) - *x.add(k)) - *g.add(k);
            }
            bmv(m, sy, wt, col, wa.add((2 * m) as usize), wa, info);
            if *info != 0 {
                *info = -8;
                return;
            }
            let mut pointr = (*head - 1) as usize;
            for j in 0..col_v as usize {
                let a1 = *wa.add(j);
                let a2 = *theta * *wa.add(col_v as usize + j);
                for i in 0..n_f as usize {
                    let k = (*indx.add(i) - 1) as usize;
                    *r.add(i) += *wy.add(k + pointr * n_i) * a1 + *ws.add(k + pointr * n_i) * a2;
                }
                pointr = (pointr + 1) % m_i;
            }
        }
    }
}

// =====================================================================
// errclb
// =====================================================================

unsafe fn errclb(
    n: i32,
    m: i32,
    factr: f64,
    l: *const f64,
    u: *const f64,
    nbd: *const i32,
    task: *mut c_char,
    info: *mut i32,
    k: *mut i32,
) {
    unsafe {
        if n <= 0 {
            cstrcpy(task, b"ERROR: N .LE. 0");
        }
        if m <= 0 {
            cstrcpy(task, b"ERROR: M .LE. 0");
        }
        if factr < 0.0 {
            cstrcpy(task, b"ERROR: FACTR .LT. 0");
        }
        for i in 0..n as usize {
            if *nbd.add(i) < 0 || *nbd.add(i) > 3 {
                cstrcpy(task, b"ERROR: INVALID NBD");
                *info = -6;
                *k = (i + 1) as i32;
            }
            if *nbd.add(i) == 2 && *l.add(i) > *u.add(i) {
                cstrcpy(task, b"ERROR: NO FEASIBLE SOLUTION");
                *info = -7;
                *k = (i + 1) as i32;
            }
        }
    }
}

// =====================================================================
// formk
// =====================================================================

unsafe fn formk(
    n: i32,
    nsub: *mut i32,
    ind: *const i32,
    nenter: *const i32,
    ileave: *const i32,
    indx2: *const i32,
    iupdat: *const i32,
    updatd: *const i32,
    wn: *mut f64,
    wn1: *mut f64,
    m: i32,
    ws: *const f64,
    wy: *const f64,
    sy: *const f64,
    theta: *const f64,
    col: *const i32,
    head: *const i32,
    info: *mut i32,
) {
    unsafe {
        let m2 = 2 * m;
        let m2i = m2 as usize;
        let mi = m as usize;
        let ni = n as usize;
        let mut upcl: i32;

        if *updatd != 0 {
            if *iupdat > m {
                for jy in 0..(m - 1) as usize {
                    let js = mi + jy;
                    let cnt = m - 1 - jy as i32;
                    dcopy(
                        cnt,
                        wn1.add((jy + 1) + (jy + 1) * m2i),
                        1,
                        wn1.add(jy + jy * m2i),
                        1,
                    );
                    dcopy(
                        cnt,
                        wn1.add((js + 1) + (js + 1) * m2i),
                        1,
                        wn1.add(js + js * m2i),
                        1,
                    );
                    dcopy(
                        m - 1,
                        wn1.add((m as usize + 1) + (jy + 1) * m2i),
                        1,
                        wn1.add(mi + jy * m2i),
                        1,
                    );
                }
            }
            let pbegin = 0;
            let pend = *nsub - 1;
            let dbegin = *nsub;
            let dend = n - 1;
            let iy = (*col - 1) as usize;
            let is_ = mi + (*col - 1) as usize;
            let mut ipntr = (*head + *col - 2) as usize;
            if ipntr >= mi {
                ipntr -= mi;
            }
            let mut jpntr = (*head - 1) as usize;
            for jy in 0..*col as usize {
                let js = mi + jy;
                let (mut t1, mut t2, mut t3) = (0.0_f64, 0.0_f64, 0.0_f64);
                for k in pbegin as usize..=pend as usize {
                    let k1 = (*ind.add(k) - 1) as usize;
                    t1 += *wy.add(k1 + ipntr * ni) * *wy.add(k1 + jpntr * ni);
                }
                for k in dbegin as usize..=dend as usize {
                    let k1 = (*ind.add(k) - 1) as usize;
                    t2 += *ws.add(k1 + ipntr * ni) * *ws.add(k1 + jpntr * ni);
                    t3 += *ws.add(k1 + ipntr * ni) * *wy.add(k1 + jpntr * ni);
                }
                *wn1.add(iy + jy * m2i) = t1;
                *wn1.add(is_ + js * m2i) = t2;
                *wn1.add(is_ + jy * m2i) = t3;
                jpntr = (jpntr + 1) % mi;
            }
            let jy = (*col - 1) as usize;
            let mut jpntr = (*head + *col - 2) as usize;
            if jpntr >= mi {
                jpntr -= mi;
            }
            let mut ipntr = (*head - 1) as usize;
            for i in 0..*col as usize {
                let is_ = mi + i;
                let mut t3 = 0.0_f64;
                for k in pbegin as usize..=pend as usize {
                    let k1 = (*ind.add(k) - 1) as usize;
                    t3 += *ws.add(k1 + ipntr * ni) * *wy.add(k1 + jpntr * ni);
                }
                ipntr = (ipntr + 1) % mi;
                *wn1.add(is_ + jy * m2i) = t3;
            }
            upcl = *col - 1;
        } else {
            upcl = *col;
        }

        // modify blocks (1,1) and (2,2)
        let mut ipntr = (*head - 1) as usize;
        for iy in 0..upcl as usize {
            let is_ = mi + iy;
            let mut jpntr = (*head - 1) as usize;
            for jy in 0..=iy {
                let js = mi + jy;
                let (mut t1, mut t2, mut t3, mut t4) = (0.0, 0.0, 0.0, 0.0);
                for k in 0..*nenter as usize {
                    let k1 = (*indx2.add(k) - 1) as usize;
                    t1 += *wy.add(k1 + ipntr * ni) * *wy.add(k1 + jpntr * ni);
                    t2 += *ws.add(k1 + ipntr * ni) * *ws.add(k1 + jpntr * ni);
                }
                for k in (*ileave - 1) as usize..ni {
                    let k1 = (*indx2.add(k) - 1) as usize;
                    t3 += *wy.add(k1 + ipntr * ni) * *wy.add(k1 + jpntr * ni);
                    t4 += *ws.add(k1 + ipntr * ni) * *ws.add(k1 + jpntr * ni);
                }
                *wn1.add(iy + jy * m2i) += t1 - t3;
                *wn1.add(is_ + js * m2i) += -t2 + t4;
                jpntr = (jpntr + 1) % mi;
            }
            ipntr = (ipntr + 1) % mi;
        }

        // modify block (2,1)
        let mut ipntr = (*head - 1) as usize;
        for is_ in mi..(mi + upcl as usize) {
            let mut jpntr = (*head - 1) as usize;
            for jy in 0..upcl as usize {
                let (mut t1, mut t3) = (0.0_f64, 0.0_f64);
                for k in 0..*nenter as usize {
                    let k1 = (*indx2.add(k) - 1) as usize;
                    t1 += *ws.add(k1 + ipntr * ni) * *wy.add(k1 + jpntr * ni);
                }
                for k in (*ileave - 1) as usize..ni {
                    let k1 = (*indx2.add(k) - 1) as usize;
                    t3 += *ws.add(k1 + ipntr * ni) * *wy.add(k1 + jpntr * ni);
                }
                if is_ <= jy + mi {
                    *wn1.add(is_ + jy * m2i) += t1 - t3;
                } else {
                    *wn1.add(is_ + jy * m2i) += -t1 + t3;
                }
                jpntr = (jpntr + 1) % mi;
            }
            ipntr = (ipntr + 1) % mi;
        }

        // Form upper triangle of WN
        for iy in 0..*col as usize {
            let is_ = *col as usize + iy;
            let is1 = mi + iy;
            for jy in 0..=iy {
                let js = *col as usize + jy;
                let js1 = mi + jy;
                *wn.add(jy + iy * m2i) = *wn1.add(iy + jy * m2i) / *theta;
                *wn.add(js + is_ * m2i) = *wn1.add(is1 + js1 * m2i) * *theta;
            }
            for jy in (iy + 1)..*col as usize {
                *wn.add(jy + iy * m2i) = -*wn1.add(is1 + jy * m2i);
            }
            *wn.add(iy + is_ * m2i) = 0.0;
            for jy in (iy + 1)..*col as usize {
                *wn.add(is_ + jy * m2i) = *wn1.add(is1 + jy * m2i);
            }
            *wn.add(is_ + iy * m2i) += *sy.add(iy + iy * mi);
        }

        // Cholesky (1,1)
        *info = dpofa(wn, m2, *col);
        if *info != 0 {
            *info = -1;
            return;
        }

        // L^-1 in (1,2)
        let col2 = *col * 2;
        for js in *col..col2 {
            let r = dtrsl(wn, m2, *col, wn.add(js as usize * m2i), 11);
            if r != 0 {
                return;
            }
        }

        // Complete (2,2)
        for is_ in *col as usize..col2 as usize {
            for js in is_..col2 as usize {
                *wn.add(is_ + js * m2i) += ddot(*col, wn.add(is_ * m2i), 1, wn.add(js * m2i), 1);
            }
        }

        // Cholesky (2,2)
        *info = dpofa(wn.add(*col as usize + *col as usize * m2i), m2, *col);
        if *info != 0 {
            *info = -2;
        }
    }
}

// =====================================================================
// formt
// =====================================================================

unsafe fn formt(
    m: i32,
    wt: *mut f64,
    sy: *const f64,
    ss: *const f64,
    col: *const i32,
    theta: *const f64,
    info: *mut i32,
) {
    unsafe {
        let mi = m as usize;
        for j in 0..*col as usize {
            *wt.add(j * mi) = *theta * *ss.add(j * mi);
        }
        for i in 1..*col as usize {
            for j in i..*col as usize {
                let k1 = if i < j { i } else { j } - 1;
                let mut ddum = 0.0_f64;
                for k in 0..k1 as usize {
                    ddum += *sy.add(i + k * mi) * *sy.add(j + k * mi) / *sy.add(k + k * mi);
                }
                *wt.add(i + j * mi) = ddum + *theta * *ss.add(i + j * mi);
            }
        }
        *info = dpofa(wt, m, *col);
        if *info != 0 {
            *info = -3;
        }
    }
}

// =====================================================================
// freev
// =====================================================================

unsafe fn freev(
    n: i32,
    nfree: *mut i32,
    indx: *mut i32,
    nenter: *mut i32,
    ileave: *mut i32,
    indx2: *mut i32,
    iwhere: *const i32,
    wrk: *mut i32,
    updatd: *const i32,
    cnstnd: *const i32,
    iprint: i32,
    iter: *const i32,
) {
    unsafe {
        *nenter = 0;
        *ileave = n + 1;
        if *iter > 0 && *cnstnd != 0 {
            for i in 0..*nfree as usize {
                let k = *indx.add(i);
                if *iwhere.add((k - 1) as usize) > 0 {
                    *ileave -= 1;
                    *indx2.add((*ileave - 1) as usize) = k;
                    if iprint >= 100 {
                        eprintln!("Variable {} leaves the set of free variables", k);
                    }
                }
            }
            for i in *nfree as usize..n as usize {
                let k = *indx.add(i);
                if *iwhere.add((k - 1) as usize) <= 0 {
                    *nenter += 1;
                    *indx2.add((*nenter - 1) as usize) = k;
                    if iprint >= 100 {
                        eprintln!("Variable {} enters the set of free variables", k);
                    }
                }
                if iprint >= 100 {
                    eprintln!(
                        "{} variables leave; {} variables enter",
                        n + 1 - *ileave,
                        *nenter
                    );
                }
            }
        }
        *wrk = if *ileave < n + 1 || *nenter > 0 || *updatd != 0 {
            1
        } else {
            0
        };
        *nfree = 0;
        let mut iact = n + 1;
        for i in 0..n as usize {
            if *iwhere.add(i) <= 0 {
                *nfree += 1;
                *indx.add((*nfree - 1) as usize) = (i + 1) as i32;
            } else {
                iact -= 1;
                *indx.add((iact - 1) as usize) = (i + 1) as i32;
            }
        }
        if iprint >= 99 {
            eprintln!(
                "{}  variables are free at GCP on iteration {}",
                *nfree,
                *iter + 1
            );
        }
    }
}

// =====================================================================
// hpsolb
// =====================================================================

unsafe fn hpsolb(n: i32, t: *mut f64, iorder: *mut i32, iheap: i32) {
    unsafe {
        // 0-based indexing
        if iheap == 0 {
            for k in 1..n as usize {
                let ddum = *t.add(k);
                let indxin = *iorder.add(k);
                let mut i = k;
                loop {
                    if i <= 0 {
                        break;
                    }
                    let j = (i - 1) / 2;
                    if ddum < *t.add(j) {
                        *t.add(i) = *t.add(j);
                        *iorder.add(i) = *iorder.add(j);
                        i = j;
                    } else {
                        break;
                    }
                }
                *t.add(i) = ddum;
                *iorder.add(i) = indxin;
            }
        }
        if n > 1 {
            let mut i = 0usize;
            let out = *t.add(0);
            let indxou = *iorder.add(0);
            let ddum = *t.add((n - 1) as usize);
            let indxin = *iorder.add((n - 1) as usize);
            loop {
                let mut j = i + i + 1;
                if j <= (n - 2) as usize {
                    if *t.add(j + 1) < *t.add(j) {
                        j += 1;
                    }
                    if *t.add(j) < ddum {
                        *t.add(i) = *t.add(j);
                        *iorder.add(i) = *iorder.add(j);
                        i = j;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            *t.add(i) = ddum;
            *iorder.add(i) = indxin;
            *t.add((n - 1) as usize) = out;
            *iorder.add((n - 1) as usize) = indxou;
        }
    }
}

// =====================================================================
// lnsrlb
// =====================================================================

unsafe fn lnsrlb(
    n: i32,
    l: *const f64,
    u: *const f64,
    nbd: *const i32,
    x: *mut f64,
    f: *mut f64,
    fold: *mut f64,
    gd: *mut f64,
    gdold: *mut f64,
    g: *mut f64,
    d: *mut f64,
    r: *mut f64,
    t: *mut f64,
    z: *const f64,
    stp: *mut f64,
    dnorm: *mut f64,
    dtd: *mut f64,
    xstep: *mut f64,
    stpmx: *mut f64,
    iter: *const i32,
    ifun: *mut i32,
    iback: *mut i32,
    nfgv: *mut i32,
    info: *mut i32,
    task: *mut c_char,
    boxed: *const i32,
    cnstnd: *const i32,
    csave: *mut c_char,
    st: &mut LbfgsbState,
) {
    unsafe {
        const STPMIN: f64 = 0.0;
        const FTOL: f64 = 0.001;
        const GTOL: f64 = 0.9;
        const XTOL: f64 = 0.1;

        if cstrncmp(task, b"FG_LN", 5) {
            // goto L556
        } else {
            *dtd = ddot(n, d, 1, d, 1);
            *dnorm = sqrt(*dtd);
            *stpmx = 1e10;
            if *cnstnd != 0 {
                if *iter == 0 {
                    *stpmx = 1.0;
                } else {
                    for i in 0..n as usize {
                        let a1 = *d.add(i);
                        if *nbd.add(i) != 0 {
                            if a1 < 0.0 && *nbd.add(i) <= 2 {
                                let a2 = *l.add(i) - *x.add(i);
                                if a2 >= 0.0 {
                                    *stpmx = 0.0;
                                } else if a1 * *stpmx < a2 {
                                    *stpmx = a2 / a1;
                                }
                            } else if a1 > 0.0 && *nbd.add(i) >= 2 {
                                let a2 = *u.add(i) - *x.add(i);
                                if a2 <= 0.0 {
                                    *stpmx = 0.0;
                                } else if a1 * *stpmx > a2 {
                                    *stpmx = a2 / a1;
                                }
                            }
                        }
                    }
                }
            }
            if *iter == 0 && *boxed == 0 {
                let d1 = 1.0 / *dnorm;
                *stp = d1.min(*stpmx);
            } else {
                *stp = 1.0;
            }
            dcopy(n, x as *const f64, 1, t, 1);
            dcopy(n, g as *const f64, 1, r, 1);
            *fold = *f;
            *ifun = 0;
            *iback = 0;
            cstrcpy(csave, b"START");
        }

        // L556
        *gd = ddot(n, g, 1, d, 1);
        if *ifun == 0 {
            *gdold = *gd;
            if *gd >= 0.0 {
                *info = -4;
                return;
            }
        }
        dcsrch(f, gd, stp, FTOL, GTOL, XTOL, STPMIN, *stpmx, csave, st);
        *xstep = *stp * *dnorm;
        if !cstrncmp(csave, b"CONV", 4) && !cstrncmp(csave, b"WARN", 4) {
            cstrcpy(task, b"FG_LNSRCH");
            *ifun += 1;
            *nfgv += 1;
            *iback = *ifun - 1;
            if *stp == 1.0 {
                dcopy(n, z, 1, x, 1);
            } else {
                for i in 0..n as usize {
                    *x.add(i) = *stp * *d.add(i) + *t.add(i);
                }
            }
        } else {
            cstrcpy(task, b"NEW_X");
        }
    }
}

// =====================================================================
// matupd
// =====================================================================

unsafe fn matupd(
    n: i32,
    m: i32,
    ws: *mut f64,
    wy: *mut f64,
    sy: *mut f64,
    ss: *mut f64,
    d: *const f64,
    r: *const f64,
    itail: *mut i32,
    iupdat: *const i32,
    col: *mut i32,
    head: *mut i32,
    theta: *mut f64,
    rr: *const f64,
    dr: *const f64,
    stp: *const f64,
    dtd: *const f64,
) {
    unsafe {
        let mi = m as usize;
        let ni = n as usize;
        if *iupdat <= m {
            *col = *iupdat;
            *itail = (*head + *iupdat - 2) % m + 1;
        } else {
            *itail = *itail % m + 1;
            *head = *head % m + 1;
        }
        // itail and head are 1-based; convert to 0-based for array access
        dcopy(n, d, 1, ws.add((*itail - 1) as usize * ni), 1);
        dcopy(n, r, 1, wy.add((*itail - 1) as usize * ni), 1);
        *theta = *rr / *dr;

        if *iupdat > m {
            for j in 0..(*col - 1) as usize {
                dcopy(
                    j as i32,
                    ss.add((j + 1) + (j + 1) * mi),
                    1,
                    ss.add(j + j * mi),
                    1,
                );
                let cnt = *col - (j as i32 + 1);
                dcopy(
                    cnt,
                    sy.add((j + 1) + (j + 1) * mi),
                    1,
                    sy.add(j + j * mi),
                    1,
                );
            }
        }

        let mut pointr = (*head - 1) as usize;
        for j in 0..(*col - 1) as usize {
            *sy.add(*col as usize + j * mi) = ddot(n, d, 1, wy.add(pointr * ni), 1);
            *ss.add(j + *col as usize * mi) = ddot(n, ws.add(pointr * ni), 1, d, 1);
            pointr = (pointr + 1) % mi;
        }
        if *stp == 1.0 {
            *ss.add(*col as usize + *col as usize * mi) = *dtd;
        } else {
            *ss.add(*col as usize + *col as usize * mi) = *stp * *stp * *dtd;
        }
        *sy.add(*col as usize + *col as usize * mi) = *dr;
    }
}

// =====================================================================
// projgr
// =====================================================================

unsafe fn projgr(
    n: i32,
    l: *const f64,
    u: *const f64,
    nbd: *const i32,
    x: *const f64,
    g: *const f64,
    sbgnrm: *mut f64,
) {
    unsafe {
        // Note: C code uses 0-based here already!
        *sbgnrm = 0.0;
        for i in 0..n as usize {
            let mut gi = *g.add(i);
            if *nbd.add(i) != 0 {
                if gi < 0.0 {
                    if *nbd.add(i) >= 2 {
                        let d1 = *x.add(i) - *u.add(i);
                        if gi < d1 {
                            gi = d1;
                        }
                    }
                } else {
                    if *nbd.add(i) <= 2 {
                        let d1 = *x.add(i) - *l.add(i);
                        if gi > d1 {
                            gi = d1;
                        }
                    }
                }
            }
            let abs_gi = fabs(gi);
            if *sbgnrm < abs_gi {
                *sbgnrm = abs_gi;
            }
        }
    }
}

// =====================================================================
// subsm
// =====================================================================

unsafe fn subsm(
    n: i32,
    m: i32,
    nsub: *const i32,
    ind: *const i32,
    l: *const f64,
    u: *const f64,
    nbd: *const i32,
    x: *mut f64,
    d: *mut f64,
    ws: *const f64,
    wy: *const f64,
    theta: *const f64,
    col: *const i32,
    head: *const i32,
    iword: *mut i32,
    wv: *mut f64,
    wn: *const f64,
    iprint: i32,
    info: *mut i32,
) {
    unsafe {
        let m2 = 2 * m;
        let m2i = m2 as usize;
        let mi = m as usize;
        let ni = n as usize;
        let col2 = *col * 2;
        let ns = *nsub;
        if ns <= 0 {
            return;
        }

        let mut pointr = (*head - 1) as usize;
        for i in 0..*col as usize {
            let (mut t1, mut t2) = (0.0_f64, 0.0_f64);
            for j in 0..ns as usize {
                let k = (*ind.add(j) - 1) as usize;
                t1 += *wy.add(k + pointr * ni) * *d.add(j);
                t2 += *ws.add(k + pointr * ni) * *d.add(j);
            }
            *wv.add(i) = t1;
            *wv.add(*col as usize + i) = *theta * t2;
            pointr = (pointr + 1) % mi;
        }

        *info = dtrsl(wn, m2, col2, wv, 11);
        if *info != 0 {
            return;
        }
        for i in 0..*col as usize {
            *wv.add(i) = -*wv.add(i);
        }
        *info = dtrsl(wn, m2, col2, wv, 0);
        if *info != 0 {
            return;
        }

        let mut pointr = (*head - 1) as usize;
        for jy in 0..*col as usize {
            let js = *col as usize + jy;
            for i in 0..ns as usize {
                let k = (*ind.add(i) - 1) as usize;
                *d.add(i) += *wy.add(k + pointr * ni) * *wv.add(jy) / *theta
                    + *ws.add(k + pointr * ni) * *wv.add(js);
            }
            pointr = (pointr + 1) % mi;
        }
        for i in 0..ns as usize {
            *d.add(i) /= *theta;
        }

        // Backtrack to feasible region
        let mut alpha = 1.0_f64;
        let mut ibd = 0i32;
        for i in 0..ns as usize {
            let k = (*ind.add(i) - 1) as usize;
            let dk = *d.add(i);
            if *nbd.add(k) != 0 {
                if dk < 0.0 && *nbd.add(k) <= 2 {
                    let temp2 = *l.add(k) - *x.add(k);
                    if temp2 >= 0.0 {
                        alpha = 0.0;
                    } else if dk * alpha < temp2 {
                        alpha = temp2 / dk;
                    }
                } else if dk > 0.0 && *nbd.add(k) >= 2 {
                    let temp2 = *u.add(k) - *x.add(k);
                    if temp2 <= 0.0 {
                        alpha = 0.0;
                    } else if dk * alpha > temp2 {
                        alpha = temp2 / dk;
                    }
                }
                if alpha < 1.0 {
                    ibd = i as i32 + 1;
                } // 1-based
            }
        }
        if alpha < 1.0 {
            let dk = *d.add((ibd - 1) as usize);
            let k = (*ind.add((ibd - 1) as usize) - 1) as usize;
            if dk > 0.0 {
                *x.add(k) = *u.add(k);
                *d.add((ibd - 1) as usize) = 0.0;
            } else if dk < 0.0 {
                *x.add(k) = *l.add(k);
                *d.add((ibd - 1) as usize) = 0.0;
            }
        }
        for i in 0..ns as usize {
            let k = (*ind.add(i) - 1) as usize;
            *x.add(k) += alpha * *d.add(i);
        }
        *iword = if alpha < 1.0 { 1 } else { 0 };
    }
}

// =====================================================================
// dcsrch
// =====================================================================

unsafe fn dcsrch(
    f: *mut f64,
    g: *mut f64,
    stp: *mut f64,
    ftol: f64,
    gtol: f64,
    xtol: f64,
    stpmin: f64,
    stpmax: f64,
    task: *mut c_char,
    st: &mut LbfgsbState,
) {
    unsafe {
        let ftest: f64;
        let mut fm: f64;
        let mut gm: f64;
        let mut fxm: f64;
        let mut fym: f64;
        let mut gxm: f64;
        let mut gym: f64;

        if cstrncmp(task, b"START", 5) {
            if *stp < stpmin {
                cstrcpy(task, b"ERROR: STP .LT. STPMIN");
            }
            if *stp > stpmax {
                cstrcpy(task, b"ERROR: STP .GT. STPMAX");
            }
            if *g >= 0.0 {
                cstrcpy(task, b"ERROR: INITIAL G .GE. ZERO");
            }
            if ftol < 0.0 {
                cstrcpy(task, b"ERROR: FTOL .LT. ZERO");
            }
            if gtol < 0.0 {
                cstrcpy(task, b"ERROR: GTOL .LT. ZERO");
            }
            if xtol < 0.0 {
                cstrcpy(task, b"ERROR: XTOL .LT. ZERO");
            }
            if stpmin < 0.0 {
                cstrcpy(task, b"ERROR: STPMIN .LT. ZERO");
            }
            if stpmax < stpmin {
                cstrcpy(task, b"ERROR: STPMAX .LT. STPMIN");
            }
            if cstrncmp(task, b"ERROR", 5) {
                return;
            }

            st.dcsrch_brackt = 0;
            st.dcsrch_stage = 1;
            st.dcsrch_finit = *f;
            st.dcsrch_ginit = *g;
            st.dcsrch_gtest = ftol * st.dcsrch_ginit;
            st.dcsrch_width = stpmax - stpmin;
            st.dcsrch_width1 = st.dcsrch_width / 0.5;
            st.dcsrch_stx = 0.0;
            st.dcsrch_fx = st.dcsrch_finit;
            st.dcsrch_gx = st.dcsrch_ginit;
            st.dcsrch_sty = 0.0;
            st.dcsrch_fy = st.dcsrch_finit;
            st.dcsrch_gy = st.dcsrch_ginit;
            st.dcsrch_stmin = 0.0;
            st.dcsrch_stmax = *stp + *stp * 4.0;
            cstrcpy(task, b"FG");
            return;
        }

        ftest = st.dcsrch_finit + *stp * st.dcsrch_gtest;
        if st.dcsrch_stage == 1 && *f <= ftest && *g >= 0.0 {
            st.dcsrch_stage = 2;
        }

        if st.dcsrch_brackt != 0 && (*stp <= st.dcsrch_stmin || *stp >= st.dcsrch_stmax) {
            cstrcpy(task, b"WARNING: ROUNDING ERRORS PREVENT PROGRESS");
        }
        if st.dcsrch_brackt != 0 && st.dcsrch_stmax - st.dcsrch_stmin <= xtol * st.dcsrch_stmax {
            cstrcpy(task, b"WARNING: XTOL TEST SATISFIED");
        }
        if *stp == stpmax && *f <= ftest && *g <= st.dcsrch_gtest {
            cstrcpy(task, b"WARNING: STP = STPMAX");
        }
        if *stp == stpmin && (*f > ftest || *g >= st.dcsrch_gtest) {
            cstrcpy(task, b"WARNING: STP = STPMIN");
        }
        if *f <= ftest && fabs(*g) <= gtol * (-st.dcsrch_ginit) {
            cstrcpy(task, b"CONVERGENCE");
        }
        if cstrncmp(task, b"WARN", 4) || cstrncmp(task, b"CONV", 4) {
            return;
        }

        if st.dcsrch_stage == 1 && *f <= st.dcsrch_fx && *f > ftest {
            fm = *f - *stp * st.dcsrch_gtest;
            fxm = st.dcsrch_fx - st.dcsrch_stx * st.dcsrch_gtest;
            fym = st.dcsrch_fy - st.dcsrch_sty * st.dcsrch_gtest;
            gm = *g - st.dcsrch_gtest;
            gxm = st.dcsrch_gx - st.dcsrch_gtest;
            gym = st.dcsrch_gy - st.dcsrch_gtest;
            dcstep(
                &mut st.dcsrch_stx,
                &mut fxm,
                &mut gxm,
                &mut st.dcsrch_sty,
                &mut fym,
                &mut gym,
                stp,
                &mut fm,
                &mut gm,
                &mut st.dcsrch_brackt,
                &mut st.dcsrch_stmin,
                &mut st.dcsrch_stmax,
            );
            st.dcsrch_fx = fxm + st.dcsrch_stx * st.dcsrch_gtest;
            st.dcsrch_fy = fym + st.dcsrch_sty * st.dcsrch_gtest;
            st.dcsrch_gx = gxm + st.dcsrch_gtest;
            st.dcsrch_gy = gym + st.dcsrch_gtest;
        } else {
            dcstep(
                &mut st.dcsrch_stx,
                &mut st.dcsrch_fx,
                &mut st.dcsrch_gx,
                &mut st.dcsrch_sty,
                &mut st.dcsrch_fy,
                &mut st.dcsrch_gy,
                stp,
                f,
                g,
                &mut st.dcsrch_brackt,
                &mut st.dcsrch_stmin,
                &mut st.dcsrch_stmax,
            );
        }

        if st.dcsrch_brackt != 0 {
            if fabs(st.dcsrch_sty - st.dcsrch_stx) >= st.dcsrch_width1 * 0.66 {
                *stp = st.dcsrch_stx + (st.dcsrch_sty - st.dcsrch_stx) * 0.5;
            }
            st.dcsrch_width1 = st.dcsrch_width;
            st.dcsrch_width = fabs(st.dcsrch_sty - st.dcsrch_stx);
        }

        if st.dcsrch_brackt != 0 {
            st.dcsrch_stmin = st.dcsrch_stx.min(st.dcsrch_sty);
            st.dcsrch_stmax = st.dcsrch_stx.max(st.dcsrch_sty);
        } else {
            st.dcsrch_stmin = *stp + (*stp - st.dcsrch_stx) * 1.1;
            st.dcsrch_stmax = *stp + (*stp - st.dcsrch_stx) * 4.0;
        }
        if *stp < stpmin {
            *stp = stpmin;
        }
        if *stp > stpmax {
            *stp = stpmax;
        }

        if (st.dcsrch_brackt != 0 && (*stp <= st.dcsrch_stmin || *stp >= st.dcsrch_stmax))
            || (st.dcsrch_brackt != 0
                && (st.dcsrch_stmax - st.dcsrch_stmin <= xtol * st.dcsrch_stmax))
        {
            *stp = st.dcsrch_stx;
        }
        cstrcpy(task, b"FG");
    }
}

// =====================================================================
// dcstep
// =====================================================================

unsafe fn dcstep(
    stx: *mut f64,
    fx: *mut f64,
    dx: *mut f64,
    sty: *mut f64,
    fy: *mut f64,
    dy: *mut f64,
    stp: *mut f64,
    fp: *mut f64,
    dp: *mut f64,
    brackt: *mut i32,
    stpmin: *mut f64,
    stpmax: *mut f64,
) {
    unsafe {
        let sgnd = *dp * (*dx / fabs(*dx));
        let mut theta: f64;
        let mut s: f64;
        let mut gamm: f64;
        let mut p: f64;
        let mut q: f64;
        let mut r__: f64;
        let mut stpc: f64;
        let mut stpf: f64;
        let mut stpq: f64;

        if *fp > *fx {
            theta = (*fx - *fp) * 3.0 / (*stp - *stx) + *dx + *dp;
            s = fabs(theta).max(fabs(*dx)).max(fabs(*dp));
            gamm = s * sqrt((theta / s).powi(2) - *dx / s * (*dp / s));
            if *stp < *stx {
                gamm = -gamm;
            }
            p = gamm - *dx + theta;
            q = gamm - *dx + gamm + *dp;
            r__ = p / q;
            stpc = *stx + r__ * (*stp - *stx);
            stpq = *stx + *dx / ((*fx - *fp) / (*stp - *stx) + *dx) / 2.0 * (*stp - *stx);
            stpf = if fabs(stpc - *stx) < fabs(stpq - *stx) {
                stpc
            } else {
                stpc + (stpq - stpc) / 2.0
            };
            *brackt = 1;
        } else if sgnd < 0.0 {
            theta = (*fx - *fp) * 3.0 / (*stp - *stx) + *dx + *dp;
            s = fabs(theta).max(fabs(*dx)).max(fabs(*dp));
            gamm = s * sqrt((theta / s).powi(2) - *dx / s * (*dp / s));
            if *stp > *stx {
                gamm = -gamm;
            }
            p = gamm - *dp + theta;
            q = gamm - *dp + gamm + *dx;
            r__ = p / q;
            stpc = *stp + r__ * (*stx - *stp);
            stpq = *stp + *dp / (*dp - *dx) * (*stx - *stp);
            stpf = if fabs(stpc - *stp) > fabs(stpq - *stp) {
                stpc
            } else {
                stpq
            };
            *brackt = 1;
        } else if fabs(*dp) < fabs(*dx) {
            theta = (*fx - *fp) * 3.0 / (*stp - *stx) + *dx + *dp;
            s = fabs(theta).max(fabs(*dx)).max(fabs(*dp));
            let d1 = (theta / s).powi(2) - *dx / s * (*dp / s);
            gamm = if d1 < 0.0 { 0.0 } else { s * sqrt(d1) };
            if *stp > *stx {
                gamm = -gamm;
            }
            p = gamm - *dp + theta;
            q = gamm + (*dx - *dp) + gamm;
            r__ = p / q;
            if r__ < 0.0 && gamm != 0.0 {
                stpc = *stp + r__ * (*stx - *stp);
            } else if *stp > *stx {
                stpc = *stpmax;
            } else {
                stpc = *stpmin;
            }
            stpq = *stp + *dp / (*dp - *dx) * (*stx - *stp);
            if *brackt != 0 {
                stpf = if fabs(stpc - *stp) < fabs(stpq - *stp) {
                    stpc
                } else {
                    stpq
                };
                let d1 = *stp + (*sty - *stp) * 0.66;
                stpf = if *stp > *stx {
                    d1.min(stpf)
                } else {
                    d1.max(stpf)
                };
            } else {
                stpf = if fabs(stpc - *stp) > fabs(stpq - *stp) {
                    stpc
                } else {
                    stpq
                };
                stpf = (*stpmax).min(stpf);
                stpf = (*stpmin).max(stpf);
            }
        } else {
            if *brackt != 0 {
                theta = (*fp - *fy) * 3.0 / (*sty - *stp) + *dy + *dp;
                s = fabs(theta).max(fabs(*dy)).max(fabs(*dp));
                gamm = s * sqrt((theta / s).powi(2) - *dy / s * (*dp / s));
                if *stp > *sty {
                    gamm = -gamm;
                }
                p = gamm - *dp + theta;
                q = gamm - *dp + gamm + *dy;
                r__ = p / q;
                stpc = *stp + r__ * (*sty - *stp);
                stpf = stpc;
            } else if *stp > *stx {
                stpf = *stpmax;
            } else {
                stpf = *stpmin;
            }
        }

        // Update interval
        if *fp > *fx {
            *sty = *stp;
            *fy = *fp;
            *dy = *dp;
        } else {
            if sgnd < 0.0 {
                *sty = *stx;
                *fy = *fx;
                *dy = *dx;
            }
            *stx = *stp;
            *fx = *fp;
            *dx = *dp;
        }
        *stp = stpf;
    }
}

// =====================================================================
// Print routines
// =====================================================================

unsafe fn pvector(title: &[u8], x: *const f64, n: i32) {
    unsafe {
        let title_str = std::str::from_utf8_unchecked(title);
        eprint!("{} ", title_str);
        for i in 0..n as usize {
            eprint!("{} ", *x.add(i));
        }
        eprintln!();
    }
}

unsafe fn prn1lb(
    n: i32,
    m: i32,
    l: *const f64,
    u: *const f64,
    x: *const f64,
    iprint: i32,
    epsmch: f64,
) {
    unsafe {
        if iprint >= 0 {
            eprintln!("N = {}, M = {} machine precision = {}", n, m, epsmch);
            if iprint >= 100 {
                pvector(b"L =", l, n);
                pvector(b"X0 =", x, n);
                pvector(b"U =", u, n);
            }
        }
    }
}

unsafe fn prn2lb(
    n: i32,
    x: *const f64,
    f: *const f64,
    g: *const f64,
    iprint: i32,
    iter: i32,
    nfgv: i32,
    nact: i32,
    sbgnrm: f64,
    nint: i32,
    word: *const c_char,
    iword: i32,
    iback: i32,
    stp: f64,
    xstep: f64,
) {
    unsafe {
        if iprint >= 99 {
            eprintln!("LINE SEARCH {} times; norm of step = {}", iback, xstep);
            if iprint > 100 {
                pvector(b"X =", x, n);
                pvector(b"G =", g, n);
            }
        } else if iprint > 0 && iter % iprint == 0 {
            eprintln!(
                "At iterate {:5}  f = {:12.5e}  |proj g|=  {:12.5e}",
                iter, *f, sbgnrm
            );
        }
    }
}

unsafe fn prn3lb(
    n: i32,
    x: *const f64,
    f: *const f64,
    task: *const c_char,
    iprint: i32,
    info: i32,
    iter: i32,
    nfgv: i32,
    nintol: i32,
    nskip: i32,
    nact: i32,
    sbgnrm: f64,
    nint: i32,
    word: *const c_char,
    iback: i32,
    stp: f64,
    xstep: f64,
    k: i32,
) {
    unsafe {
        if cstrncmp(task, b"CONV", 4) {
            if iprint >= 0 {
                eprintln!(
                    "\niterations {}\nfunction evaluations {}\nsegments explored during Cauchy searches {}\nBFGS updates skipped {}\nactive bounds at final generalized Cauchy point {}\nnorm of the final projected gradient {}\nfinal function value {}\n\n",
                    iter, nfgv, nintol, nskip, nact, sbgnrm, *f
                );
            }
            if iprint >= 100 {
                pvector(b"X =", x, n);
            }
            if iprint >= 1 {
                eprintln!("F = {}", *f);
            }
        }
        if iprint >= 0 {
            match info {
                -1 => eprintln!("Matrix in 1st Cholesky factorization in formk is not Pos. Def."),
                -2 => eprintln!("Matrix in 2st Cholesky factorization in formk is not Pos. Def."),
                -3 => eprintln!("Matrix in the Cholesky factorization in formt is not Pos. Def."),
                -4 => eprintln!("Derivative >= 0, backtracking line search impossible."),
                -5 => eprintln!(
                    "Warning:  more than 10 function and gradient evaluations\n   in the last line search"
                ),
                -6 => eprintln!("Input nbd({}) is invalid", k),
                -7 => eprintln!("l({}) > u({}).  No feasible solution", k, k),
                -8 => eprintln!("The triangular system is singular."),
                -9 => eprintln!(
                    "{}\n{}",
                    "Line search cannot locate an adequate point after 20 function",
                    "and gradient evaluations"
                ),
                _ => {}
            }
        }
    }
}
