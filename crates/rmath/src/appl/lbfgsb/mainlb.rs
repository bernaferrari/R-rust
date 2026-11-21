#![allow(
    clippy::self_assignment,
    clippy::never_loop,
    clippy::redundant_closure_call,
    clippy::needless_late_init
)]
#![allow(clippy::comparison_chain)]
#![allow(clippy::manual_memcpy, clippy::comparison_to_empty)]
use super::*;

#[inline(always)]
pub(crate) fn daxpy(n: i32, da: f64, dx: &[f64], incx: i32, dy: &mut [f64], incy: i32) {
    {
        if n <= 0 || da == 0.0 {
            return;
        }
        if incx == 1 && incy == 1 {
            for i in 0..n as usize {
                dy[i] += da * dx[i];
            }
        } else {
            let (mut ix, mut iy) = (0usize, 0usize);
            for _ in 0..n {
                dy[iy] += da * dx[ix];
                ix += incx as usize;
                iy += incy as usize;
            }
        }
    }
}

#[inline(always)]
pub(crate) fn dcopy(n: i32, dx: &[f64], incx: i32, dy: &mut [f64], incy: i32) {
    {
        if n <= 0 {
            return;
        }
        if incx == 1 && incy == 1 {
            for i in 0..n as usize {
                dy[i] = dx[i];
            }
        } else {
            let (mut ix, mut iy) = (0usize, 0usize);
            for _ in 0..n {
                dy[iy] = dx[ix];
                ix += incx as usize;
                iy += incy as usize;
            }
        }
    }
}

/// `dcopy` between two regions of one array, element by element in the
/// Fortran order, so overlapping regions behave exactly as in the C original.
#[inline(always)]
pub(crate) fn dcopy_within(n: i32, a: &mut [f64], dx: usize, incx: i32, dy: usize, incy: i32) {
    if n <= 0 {
        return;
    }
    let (mut ix, mut iy) = (dx, dy);
    for _ in 0..n {
        a[iy] = a[ix];
        ix += incx as usize;
        iy += incy as usize;
    }
}

#[inline(always)]
pub(crate) fn ddot(n: i32, dx: &[f64], incx: i32, dy: &[f64], incy: i32) -> f64 {
    {
        if n <= 0 {
            return 0.0;
        }
        let mut s = 0.0_f64;
        if incx == 1 && incy == 1 {
            for i in 0..n as usize {
                s += dx[i] * dy[i];
            }
        } else {
            let (mut ix, mut iy) = (0usize, 0usize);
            for _ in 0..n {
                s += dx[ix] * dy[iy];
                ix += incx as usize;
                iy += incy as usize;
            }
        }
        s
    }
}

#[inline(always)]
pub(crate) fn dscal(n: i32, da: f64, dx: &mut [f64], incx: i32) {
    {
        if n <= 0 || da == 1.0 {
            return;
        }
        if incx == 1 {
            for i in 0..n as usize {
                dx[i] *= da;
            }
        } else {
            let mut ix = 0usize;
            for _ in 0..n {
                dx[ix] *= da;
                ix += incx as usize;
            }
        }
    }
}

// =====================================================================
// Inline LINPACK replacements
// =====================================================================

/// dtrsl: solve triangular systems with Fortran dtrsl job semantics.
/// Element (row r, col c) [0-based] lives at offset r + c*lda.
///   job 00 (0):  solve T*x = b, T lower triangular (forward)
///   job 01 (1):  solve T*x = b, T upper triangular (backward)
///   job 10 (10): solve T'*x = b, T lower triangular (backward)
///   job 11 (11): solve T'*x = b, T upper triangular (forward)
#[inline(always)]
pub(crate) fn dtrsl(a: &[f64], lda: i32, n: i32, x: &mut [f64], job: i32) -> i32 {
    {
        let kase1 = if job % 10 != 0 { 1 } else { 0 };
        let kase2 = if (job / 10) % 10 != 0 { 1 } else { 0 };
        let kase = 1 + kase1 + 2 * kase2;
        let n_u = n as usize;
        let lda_u = lda as usize;
        // dtrsl.f scans all diagonal elements first and returns the index
        // of the first zero one without touching b.
        for j in 0..n_u {
            if a[j + j * lda_u] == 0.0 {
                return (j + 1) as i32;
            }
        }
        match kase {
            1 => {
                // solve T*x = b, T lower triangular: forward
                for j in 0..n_u {
                    let mut tmp = x[j];
                    for c in 0..j {
                        tmp -= a[j + c * lda_u] * x[c];
                    }
                    let ajj = a[j + j * lda_u];
                    x[j] = tmp / ajj;
                }
            }
            2 => {
                // solve T*x = b, T upper triangular: backward
                for j in (0..n_u).rev() {
                    let mut tmp = x[j];
                    for c in (j + 1)..n_u {
                        tmp -= a[j + c * lda_u] * x[c];
                    }
                    let ajj = a[j + j * lda_u];
                    x[j] = tmp / ajj;
                }
            }
            3 => {
                // solve T'*x = b, T lower triangular: backward.
                // dtrsl.f: b(j) = b(j) - ddot(rows j+1..n, column j, b)
                for j in (0..n_u).rev() {
                    let mut dot = 0.0_f64;
                    for r in (j + 1)..n_u {
                        dot += a[r + j * lda_u] * x[r];
                    }
                    let ajj = a[j + j * lda_u];
                    x[j] = (x[j] - dot) / ajj;
                }
            }
            _ => {
                // solve T'*x = b, T upper triangular: forward.
                // dtrsl.f: b(j) = b(j) - ddot(rows 1..j-1, column j, b)
                for j in 0..n_u {
                    let mut dot = 0.0_f64;
                    for r in 0..j {
                        dot += a[r + j * lda_u] * x[r];
                    }
                    let ajj = a[j + j * lda_u];
                    x[j] = (x[j] - dot) / ajj;
                }
            }
        }
        0
    }
}

/// dpofa: Cholesky factorization of symmetric positive definite matrix.
/// Upper triangle stored column-major. On exit: U where A = U'*U.
pub(crate) fn dpofa(a: &mut [f64], lda: i32, n: i32) -> i32 {
    {
        for j in 0..n as usize {
            let mut s = 0.0_f64;
            for i in 0..j {
                let aij = a[j * lda as usize + i];
                s += aij * aij;
            }
            let ajj = a[j * lda as usize + j] - s;
            // R's vendored dpofa.f fails when s <= eps*|a(j,j)| (eps = 1e-14),
            // not only when the pivot is non-positive.
            if ajj <= 1.0e-14 * (a[j * lda as usize + j]).abs() {
                return (j + 1) as i32;
            }
            a[j * lda as usize + j] = sqrt(ajj);
            let ajj = a[j * lda as usize + j];
            for i in (j + 1)..n as usize {
                let mut s2 = 0.0_f64;
                for k in 0..j {
                    s2 += a[j * lda as usize + k] * a[i * lda as usize + k];
                }
                a[i * lda as usize + j] = (a[i * lda as usize + j] - s2) / ajj;
            }
        }
        0
    }
}

// =====================================================================
// C string helpers
// =====================================================================

#[inline(always)]
pub(crate) fn cstrncmp(s: &[c_char], prefix: &[u8], n: usize) -> bool {
    {
        for i in 0..n {
            if i >= prefix.len() {
                return false;
            }
            if s[i] == 0 {
                return false;
            }
            if s[i] as u8 != prefix[i] {
                return false;
            }
        }
        true
    }
}

#[inline(always)]
pub(crate) fn cstrcpy(dst: &mut [c_char], src: &[u8]) {
    {
        for i in 0..src.len() {
            dst[i] = src[i] as c_char;
        }
        dst[src.len()] = 0;
    }
}

// =====================================================================
// Persistent state for mainlb (replaces C static locals)
// =====================================================================

pub(crate) struct LbfgsbState {
    pub(crate) prjctd: i32,
    pub(crate) cnstnd: i32,
    pub(crate) boxed: i32,
    pub(crate) updatd: i32,
    pub(crate) nintol: i32,
    pub(crate) iback: i32,
    pub(crate) nskip: i32,
    pub(crate) head: i32,
    pub(crate) col: i32,
    pub(crate) itail: i32,
    pub(crate) iter: i32,
    pub(crate) iupdat: i32,
    pub(crate) nint: i32,
    pub(crate) nfgv: i32,
    pub(crate) info: i32,
    pub(crate) ifun: i32,
    pub(crate) iword: i32,
    pub(crate) nfree: i32,
    pub(crate) nact: i32,
    pub(crate) ileave: i32,
    pub(crate) nenter: i32,
    pub(crate) theta: f64,
    pub(crate) fold: f64,
    pub(crate) tol: f64,
    pub(crate) dnorm: f64,
    pub(crate) epsmch: f64,
    pub(crate) gd: f64,
    pub(crate) stpmx: f64,
    pub(crate) sbgnrm: f64,
    pub(crate) stp: f64,
    pub(crate) gdold: f64,
    pub(crate) dtd: f64,
    pub(crate) xstep: f64,
    pub(crate) word: [u8; 4],
    pub(crate) dcsrch_stage: i32,
    pub(crate) dcsrch_brackt: i32,
    pub(crate) dcsrch_ginit: f64,
    pub(crate) dcsrch_gtest: f64,
    pub(crate) dcsrch_gx: f64,
    pub(crate) dcsrch_gy: f64,
    pub(crate) dcsrch_finit: f64,
    pub(crate) dcsrch_fx: f64,
    pub(crate) dcsrch_fy: f64,
    pub(crate) dcsrch_stx: f64,
    pub(crate) dcsrch_sty: f64,
    pub(crate) dcsrch_stmin: f64,
    pub(crate) dcsrch_stmax: f64,
    pub(crate) dcsrch_width: f64,
    pub(crate) dcsrch_width1: f64,
    pub(crate) lws: i32,
    pub(crate) lwy: i32,
    pub(crate) lsy: i32,
    pub(crate) lss: i32,
    pub(crate) lwt: i32,
    pub(crate) lwn: i32,
    pub(crate) lsnd: i32,
    pub(crate) lz: i32,
    pub(crate) lr: i32,
    pub(crate) ld: i32,
    pub(crate) lt: i32,
    pub(crate) lwa: i32,
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

impl Default for LbfgsbState {
    fn default() -> Self {
        Self::new()
    }
}

// =====================================================================
// Public API: lbfgsb (= setulb)
// =====================================================================

/// L-BFGS-B represents every offset in its work arrays as a C int. Admit
/// the whole layout before the translated routine constructs any pointer.
pub(crate) fn workspace_lengths(n: c_int, m: c_int) -> Option<(c_int, c_int)> {
    let integer_words = n.checked_mul(3)?;
    let matrix_words = m.checked_mul(m)?.checked_mul(11)?;
    let history_words = m.checked_mul(n)?.checked_mul(2)?;
    let real_words = matrix_words
        .checked_add(history_words)?
        .checked_add(n.checked_mul(5)?)?
        .checked_add(m.checked_mul(8)?)?
        .checked_add(1)?;
    Some((real_words, integer_words))
}

pub(crate) fn lbfgsb(
    st: &mut LbfgsbState,
    n: c_int,
    m: c_int,
    x: &mut [f64],
    l: &[f64],
    u: &[f64],
    nbd: &[c_int],
    f: &mut f64,
    g: &mut [f64],
    factr: f64,
    pgtol: &f64,
    wa: &mut [f64],
    iwa: &mut [c_int],
    task: &mut [c_char],
    iprint: c_int,
    isave: &mut [c_int],
) {
    {
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

        // The Fortran work-array layout, carved into disjoint sections at
        // the offsets fixed by the START call.
        let (ws, rest) = wa[(st.lws - 1) as usize..].split_at_mut((st.lwy - st.lws) as usize);
        let (wy, rest) = rest.split_at_mut((st.lsy - st.lwy) as usize);
        let (sy, rest) = rest.split_at_mut((st.lss - st.lsy) as usize);
        let (ss, rest) = rest.split_at_mut((st.lwt - st.lss) as usize);
        let (wt, rest) = rest.split_at_mut((st.lwn - st.lwt) as usize);
        let (wn, rest) = rest.split_at_mut((st.lsnd - st.lwn) as usize);
        let (snd, rest) = rest.split_at_mut((st.lz - st.lsnd) as usize);
        let (z, rest) = rest.split_at_mut((st.lr - st.lz) as usize);
        let (r, rest) = rest.split_at_mut((st.ld - st.lr) as usize);
        let (d, rest) = rest.split_at_mut((st.lt - st.ld) as usize);
        let (t, wa) = rest.split_at_mut((st.lwa - st.lt) as usize);
        let (indx, rest) = iwa.split_at_mut(n as usize);
        let (iwhere, indx2) = rest.split_at_mut(n as usize);
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
            ws,
            wy,
            sy,
            ss,
            wt,
            wn,
            snd,
            z,
            r,
            d,
            t,
            wa,
            indx,
            iwhere,
            indx2,
            task,
            iprint,
            &mut csave,
            isave,
            st,
        );
        // GNU mainlb saves nfgv into isave at every exit (isave[12]
        // of the rearranged size-21 layout); our caller reads the
        // original-layout slot 33 (Fortran isave(34)).
        isave[33] = st.nfgv;
    }
}

// =====================================================================
// mainlb
// =====================================================================

fn mainlb(
    n: i32,
    m: i32,
    x: &mut [f64],
    l: &[f64],
    u: &[f64],
    nbd: &[i32],
    f: &mut f64,
    g: &mut [f64],
    factr: f64,
    pgtol: &f64,
    ws: &mut [f64],
    wy: &mut [f64],
    sy: &mut [f64],
    ss: &mut [f64],
    wt: &mut [f64],
    wn: &mut [f64],
    snd: &mut [f64],
    z: &mut [f64],
    r: &mut [f64],
    d: &mut [f64],
    t: &mut [f64],
    wa: &mut [f64],
    indx: &mut [i32],
    iwhere: &mut [i32],
    indx2: &mut [i32],
    task: &mut [c_char],
    iprint: i32,
    csave: &mut [c_char],
    isave: &mut [i32],
    st: &mut LbfgsbState,
) {
    {
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
                    &st.word,
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
                g,
                d,
                r,
                t,
                z,
                task,
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
                        &st.word,
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
                    // GNU's mainlb does `goto L222` here: restart the
                    // iteration inside this call instead of returning to
                    // the driver. Fall through to the L222 loop below.
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
                    &st.word,
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
                    &st.word,
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
                    &st.word,
                    st.iback,
                    st.stp,
                    st.xstep,
                    0,
                );
                return;
            }
            // Compute r = newg - oldg, rr = y'y, dr = y's
            for i in 0..n as usize {
                r[i] = g[i] - r[i];
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
                    &st.word,
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
            if cstrncmp(&mut task[6..], b"CPU", 3) {
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
                &st.word,
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
                let (wa_p, rest) = wa.split_at_mut((2 * m) as usize);
                let (wa_c, rest) = rest.split_at_mut((2 * m) as usize);
                let (wa_wbp, wa_v) = rest.split_at_mut((2 * m) as usize);
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
                    wa_p,
                    wa_c,
                    wa_wbp,
                    wa_v,
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
                d[i] = z[i] - x[i];
            }

            // L666
            lnsrlb(
                n,
                l,
                u,
                nbd,
                x,
                f,
                g,
                d,
                r,
                t,
                z,
                task,
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
                        &st.word,
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
                    // GNU: `goto L222` — restart the iteration inside
                    // this call; do not return to the driver.
                    continue;
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
                    &st.word,
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

fn active(
    n: i32,
    l: &[f64],
    u: &[f64],
    nbd: &[i32],
    x: &mut [f64],
    iwhere: &mut [i32],
    iprint: i32,
    prjctd: &mut i32,
    cnstnd: &mut i32,
    boxed: &mut i32,
) {
    {
        let mut nbdd = 0i32;
        *prjctd = 0;
        *cnstnd = 0;
        *boxed = 1;
        for i in 0..n as usize {
            if nbd[i] > 0 {
                if nbd[i] <= 2 && x[i] <= l[i] {
                    if x[i] < l[i] {
                        *prjctd = 1;
                        x[i] = l[i];
                    }
                    nbdd += 1;
                } else if nbd[i] >= 2 && x[i] >= u[i] {
                    if x[i] > u[i] {
                        *prjctd = 1;
                        x[i] = u[i];
                    }
                    nbdd += 1;
                }
            }
        }
        for i in 0..n as usize {
            if nbd[i] != 2 {
                *boxed = 0;
            }
            if nbd[i] == 0 {
                iwhere[i] = -1;
            } else {
                *cnstnd = 1;
                if nbd[i] == 2 && u[i] - l[i] <= 0.0 {
                    iwhere[i] = 3;
                } else {
                    iwhere[i] = 0;
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

pub(crate) fn bmv(
    m: i32,
    sy: &[f64],
    wt: &[f64],
    col: &i32,
    v: &[f64],
    p: &mut [f64],
    info: &mut i32,
) {
    {
        let col_v = *col;
        if col_v == 0 {
            return;
        }
        let col_i = col_v as usize;
        let m_i = m as usize;

        // PART I
        p[col_i] = v[col_i];
        for i in 1..col_i {
            let i2 = col_i + i;
            let mut sum = 0.0_f64;
            for k in 0..i as usize {
                sum += sy[i + k * m_i] * v[k] / sy[k + k * m_i];
            }
            p[i2] = v[i2] + sum;
        }
        *info = dtrsl(wt, m, col_v, &mut p[col_i..], 11);
        if *info != 0 {
            return;
        }

        for i in 0..col_i {
            p[i] = v[i] / sqrt(sy[i + i * m_i]);
        }

        // PART II
        *info = dtrsl(wt, m, col_v, &mut p[col_i..], 1);
        if *info != 0 {
            return;
        }

        for i in 0..col_i {
            p[i] = -p[i] / sqrt(sy[i + i * m_i]);
        }
        for i in 0..col_i {
            let mut sum = 0.0_f64;
            for k in (i + 1)..col_i {
                sum += sy[k + i * m_i] * p[col_i + k] / sy[i + i * m_i];
            }
            p[i] += sum;
        }
    }
}

// =====================================================================
// cauchy
// =====================================================================
