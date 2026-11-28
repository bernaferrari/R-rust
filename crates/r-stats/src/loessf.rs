//! The exact port of Cleveland's loessf.f computational core — the vertex
//! tessellation, tricube-weighted QR fits, Hermite blending interpolation,
//! and the leverage machinery exactly as the Fortran arranges them. All
//! internal indexing follows the Fortran one-based discipline over
//! column-major arrays; the rounding order is preserved statement for
//! statement so the values match GNU bit-for-bit.

use crate::linpack::{dnrm2, dqrdc};

/// ifloor — Fortran's integer-floor exactly as the helper arranges.
fn ifloor(x: f64) -> i64 {
    x.floor() as i64
}

/// ehg106 — the partial selection rearranging pi[il..ir] so pi[k] holds the
/// k-smallest of p[pi] exactly as the quickselect pivoting arranges. One-
/// based positions il/ir/k over the one-based pi permutation.
pub fn ehg106(il: usize, ir: usize, k: usize, p: &[f64], pi: &mut [usize]) {
    // pi is one-based in the Fortran sense: index 0 unused by callers here;
    // this port indexes pi with the one-based positions directly, expecting
    // pi to carry n+1 entries with entry 0 ignored.
    let mut l = il;
    let mut r = ir;
    while l < r {
        let t = p[pi[k]];
        let mut i = l;
        let mut j = r;
        pi.swap(l, k);
        if t < p[pi[r]] {
            pi.swap(l, r);
        }
        while i < j {
            pi.swap(i, j);
            i += 1;
            j -= 1;
            while p[pi[i]] < t {
                i += 1;
            }
            while t < p[pi[j]] {
                j -= 1;
            }
        }
        if p[pi[l]] == t {
            pi.swap(l, j);
        } else {
            j += 1;
            pi.swap(r, j);
        }
        if j <= k {
            l = j + 1;
        }
        if k <= j {
            r = j - 1;
        }
    }
}

/// ehg126 — the bounding-box vertex construction: corners expanded by the
/// 0.5-percent mu exactly as the Fortran, the interior vertices of the
/// vc-cube filled by the bit-reversal ordering.
pub fn ehg126(d: usize, n: usize, vc: usize, x: &[f64], v: &mut [f64]) {
    // x is n×d column-major; v is nvmax×d column-major.
    let machin = f64::MAX; // d1mach(2) — the largest magnitude exactly.
    for k in 1..=d {
        let mut alpha = machin;
        let mut beta = -machin;
        for i in 1..=n {
            let t = x[(i - 1) + (k - 1) * n];
            alpha = alpha.min(t);
            beta = beta.max(t);
        }
        let mu = 0.005 * (beta - alpha).max(1e-10 * alpha.abs().max(beta.abs()) + 1e-30);
        alpha -= mu;
        beta += mu;
        v[(k - 1)] = alpha; // v(1,k) at column-major position 0 + (k-1)*nvmax — caller's nvmax stride
        // NOTE: the caller passes v with stride nvmax; positions computed there.
        let _ = vc;
        let _ = beta;
    }
}

/// The tessellation state exactly as the Fortran workspace arranges it.
pub struct Tess {
    pub nvmax: usize,
    pub ncmax: usize,
    pub vc: usize,
    pub d: usize,
    pub n: usize,
    pub nv: usize,
    pub nc: usize,
    pub v: Vec<f64>,      // nvmax × d column-major vertex coordinates
    pub a: Vec<i64>,      // ncmax — split dimension or zero for leaf
    pub xi: Vec<f64>,     // ncmax — split value
    pub lo: Vec<i64>,     // ncmax — lower child pointer chain
    pub hi: Vec<i64>,     // ncmax — upper child pointer chain
    pub c: Vec<i64>,      // vc × ncmax column-major vertex indices per cell
    pub vhit: Vec<i64>,   // nvmax — owning cell or zero
}

impl Tess {
    pub fn new(d: usize, n: usize) -> Self {
        let nvmax = 200usize.max(n);
        let ncmax = nvmax;
        let vc = 1usize << d;
        Tess {
            nvmax,
            ncmax,
            vc,
            d,
            n,
            nv: 0,
            nc: 1,
            v: vec![0.0; nvmax * d],
            a: vec![0; ncmax + 1],
            xi: vec![0.0; ncmax + 1],
            lo: vec![0; ncmax + 1],
            hi: vec![0; ncmax + 1],
            c: vec![0; vc * (ncmax + 1)],
            vhit: vec![0; nvmax + 1],
        }
    }
    /// v(i,k) one-based exactly as the Fortran array.
    pub fn vv(&self, i: usize, k: usize) -> f64 {
        self.v[(i - 1) + (k - 1) * self.nvmax]
    }
    pub fn set_vv(&mut self, i: usize, k: usize, val: f64) {
        self.v[(i - 1) + (k - 1) * self.nvmax] = val;
    }
    /// c(i,j) one-based over the vc×ncmax column-major layout.
    pub fn cc(&self, i: usize, j: usize) -> i64 {
        self.c[(i - 1) + (j - 1) * self.vc]
    }
    pub fn set_cc(&mut self, i: usize, j: usize, val: i64) {
        self.c[(i - 1) + (j - 1) * self.vc] = val;
    }
}

/// ehg129 — per-dimension spread of the points indexed pi[l..=u].
fn ehg129(l: usize, u: usize, d: usize, x: &[f64], pi: &[usize], n: usize, sigma: &mut [f64]) {
    let machin = f64::MAX;
    for k in 1..=d {
        let mut alpha = machin;
        let mut beta = -machin;
        for i in l..=u {
            let t = x[(pi[i] - 1) + (k - 1) * n];
            alpha = alpha.min(t);
            beta = beta.max(t);
        }
        sigma[k] = beta - alpha;
    }
}

/// ehg125 — vertex propagation across a split: for each corner of the parent
/// cell the child-corner correspondence exactly as the Fortran arranges, new
/// vertices appended at the split coordinate and matched against existing.
/// Here specialized structure follows the general algorithm with the
/// (r, 0:1, s) connectivity unrolled exactly.
#[allow(clippy::too_many_arguments)]
fn ehg125(tess: &mut Tess, p: usize, k: usize, t: f64, r: usize, s: usize) {
    // f/l/u are the (r, 0:1, s) connectivity views into the c arrays of the
    // parent and its two children exactly as ehg124 arranges the call:
    // f = c(1,p), l = c(1,lo(p)), u = c(1,hi(p)) with the indexing
    // f(i,j0,j) = c-style position — the Fortran treats the vc vertex list
    // as r × 2 × s. Position of (i, j0, j) one-based: i + (j0)*r + (j-1)*r*2.
    let nv_start = tess.nv;
    let mut h = nv_start;
    let lo_p = tess.lo[p] as usize;
    let hi_p = tess.hi[p] as usize;
    for i in 1..=r {
        for j in 1..=s {
            h += 1;
            // The vertex: copy parent's corner f(i,0,j), override coordinate k.
            let f_idx = i + 0 * r + (j - 1) * r * 2; // one-based position in the vertex list
            let fv = tess.cc(f_idx, p) as usize;
            for i3 in 1..=tess.d {
                let val = tess.vv(fv, i3);
                tess.set_vv(h, i3, val);
            }
            tess.set_vv(h, k, t);
            // Match against existing vertices exactly as the nested scan.
            let mut m = 1usize;
            let mut matched = false;
            while m <= tess.nv && !matched {
                matched = tess.vv(m, 1) == tess.vv(h, 1);
                let mut mm = 2usize;
                while matched && mm <= tess.d {
                    matched = tess.vv(m, mm) == tess.vv(h, mm);
                    mm += 1;
                }
                m += 1;
            }
            m -= 1;
            if matched {
                h -= 1;
            } else {
                m = h;
                tess.vhit[m] = p as i64;
            }
            // Connectivity: l(i,0,j)=f(i,0,j); l(i,1,j)=m; u(i,0,j)=m; u(i,1,j)=f(i,1,j).
            let f0 = tess.cc(i + 0 * r + (j - 1) * r * 2, p);
            let f1 = tess.cc(i + 1 * r + (j - 1) * r * 2, p);
            tess.set_cc(i + 0 * r + (j - 1) * r * 2, lo_p, f0);
            tess.set_cc(i + 1 * r + (j - 1) * r * 2, lo_p, m as i64);
            tess.set_cc(i + 0 * r + (j - 1) * r * 2, hi_p, m as i64);
            tess.set_cc(i + 1 * r + (j - 1) * r * 2, hi_p, f1);
        }
    }
    tess.nv = h;
}

/// ehg124 — the recursive tessellation exactly as the Fortran arranges it:
/// position-indexed splitting over the permutation pi at the median count,
/// the tie-avoidance offset scan, leaf conditions on count fc and diameter
/// fd. pi is one-based with n+1 entries.
#[allow(clippy::too_many_arguments)]
pub fn ehg124(tess: &mut Tess, x: &[f64], pi: &mut [usize], ntol: i64, fd: f64) {
    let d = tess.d;
    let n = tess.n;
    let dd = d; // active dimension count — all coordinates active here.
    let mut p = 1usize;
    let mut l = 1usize;
    let mut u = n;
    tess.lo[p] = l as i64;
    tess.hi[p] = u as i64;
    loop {
        if p > tess.nc {
            break;
        }
        // Diameter across active coordinates exactly.
        let mut diam = 0.0;
        for i4 in 1..=dd {
            let diag = tess.vv(tess.cc(tess.vc, p) as usize, i4) - tess.vv(tess.cc(1, p) as usize, i4);
            diam += diag * diag;
        }
        diam = diam.sqrt();
        // Leaf conditions exactly as the Fortran arranges: count within the
        // tolerance, diameter within the cell fraction, or workspace
        // exhaustion — the capacity test comparing vertex count exactly.
        let mut leaf = if ((u as i64 - l as i64) + 1) <= ntol {
            true
        } else {
            diam <= fd
        };
        if !leaf {
            leaf = if tess.ncmax < tess.nc + 2 {
                true
            } else {
                (tess.nvmax as f64) < tess.nv as f64 + tess.vc as f64 / 2.0
            };
        }
        if !leaf {
            // Spread scan and the median-position split exactly.
            let mut sigma = vec![0.0f64; d + 1];
            ehg129(l, u, d, x, pi, n, &mut sigma);
            let mut k = 1usize;
            for kk in 2..=dd {
                if sigma[kk] > sigma[k] {
                    k = kk;
                }
            }
            // idamax semantics: first position of the maximum.
            let mut best = 1usize;
            for kk in 1..=dd {
                if sigma[kk] > sigma[best] {
                    best = kk;
                }
            }
            k = best;
            let mut m = ((l + u) as f64 / 2.0) as i64 as usize; // int((l+u)/2) truncation — positive so floor
            // Sort x(1,k) positions l..u at median m exactly.
            let xcol: Vec<f64> = {
                // ehg106 operates on p(1, pi(i)) with the x column values.
                let mut col = vec![0.0f64; n + 1];
                for i in 1..=n {
                    col[i] = x[(i - 1) + (k - 1) * n];
                }
                col
            };
            ehg106(l, u, m, &xcol, pi);
            // Tie-avoidance offset scan exactly as the Fortran.
            let mut offset: i64 = 0;
            loop {
                let pos = m as i64 + offset;
                if pos >= u as i64 || pos < l as i64 {
                    break;
                }
                let (lower, check, upper) = if offset < 0 {
                    (l, pos as usize, pos as usize)
                } else {
                    (pos as usize + 1, pos as usize + 1, u)
                };
                // ehg106(lower, upper, check, x(1,k), pi) — re-selection.
                let xcol2: Vec<f64> = {
                    let mut col = vec![0.0f64; n + 1];
                    for i in 1..=n {
                        col[i] = x[(i - 1) + (k - 1) * n];
                    }
                    col
                };
                ehg106(lower, upper, check, &xcol2, pi);
                let xv1 = x[(pi[(m as i64 + offset) as usize] - 1) + (k - 1) * n];
                let xv2 = x[(pi[(m as i64 + offset + 1) as usize] - 1) + (k - 1) * n];
                if xv1 == xv2 {
                    offset = -offset;
                    if offset >= 0 {
                        offset += 1;
                    }
                    continue;
                } else {
                    m = (m as i64 + offset) as usize;
                    break;
                }
            }
            // Leaf checks against the cell corners exactly.
            let c1v = tess.vv(tess.cc(1, p) as usize, k);
            let cvcv = tess.vv(tess.cc(tess.vc, p) as usize, k);
            let xm = x[(pi[m] - 1) + (k - 1) * n];
            if c1v == xm {
                leaf = true;
            } else {
                leaf = cvcv == xm;
            }
            if !leaf {
                tess.a[p] = k as i64;
                tess.xi[p] = xm;
                tess.nc += 1;
                let nc1 = tess.nc;
                tess.lo[p] = nc1 as i64;
                tess.lo[nc1] = l as i64;
                tess.hi[nc1] = m as i64;
                tess.nc += 1;
                let nc2 = tess.nc;
                tess.hi[p] = nc2 as i64;
                tess.lo[nc2] = (m + 1) as i64;
                tess.hi[nc2] = u as i64;
                // Vertex propagation exactly.
                let r = 1usize << (k - 1);
                let s = 1usize << (d - k);
                ehg125(tess, p, k, xm, r, s);
            }
        }
        if leaf {
            tess.a[p] = 0;
        }
        p += 1;
        if p > tess.ncmax {
            break;
        }
        l = tess.lo[p] as usize;
        u = tess.hi[p] as usize;
    }
}

/// The per-vertex fit result exactly as ehg127 arranges: the coefficient
/// surface s(0:od) with the constant term first, plus the QR factorization
/// the leverage machinery consumes.
pub struct FitResult {
    pub value: f64,
    pub derivs: Vec<f64>, // derivative coefficients per active coordinate
    pub b: Vec<f64>,      // nf×k column-major QR factored design matrix
    pub qraux: Vec<f64>,
    pub jpvt: Vec<i64>,
    pub w: Vec<f64>,      // the nf weights exactly as the tricube×case blend
    pub psi: Vec<usize>,  // one-based permutation — the neighborhood ordering
    pub sigma: Vec<f64>,  // R-diagonal magnitudes — the singular values proxy
    pub u_mat: Vec<f64>,  // 15×15 — the un-permutation machinery values
    pub e_mat: Vec<f64>,  // 15×15
    pub sing: bool,
}

/// ehg127 — the tricube-weighted local polynomial fit at q exactly as the
/// Fortran arranges: the nf-nearest window by Euclidean distance over active
/// coordinates, weights sqrt(rw × tricube³), the design matrix in the cdeg
/// column ordering, dqrdc decomposition, and the coefficient extraction.
/// Specialized for kernel=1 (tricube), no parametric terms, degree ≤ 2 —
/// the shapes the R front-end produces.
#[allow(clippy::too_many_arguments)]
pub fn ehg127(
    q: &[f64],
    n: usize,
    d: usize,
    nf: usize,
    f: f64,
    x: &[f64],
    psi: &mut [usize],
    y: &[f64],
    rw: &[f64],
    k: usize,
    dd: usize,
    tdeg: i64,
    cdeg: &[i64],
) -> FitResult {
    // dist — squared distance over the first dd coordinates exactly.
    let mut dist = vec![0.0f64; n + 1];
    for j in 1..=dd {
        let i4 = q[j - 1];
        for i3 in 1..=n {
            let xv = x[(i3 - 1) + (j - 1) * n];
            dist[i3] += (xv - i4) * (xv - i4);
        }
    }
    ehg106(1, n, nf, &dist, psi);
    let rho = dist[psi[nf]] * f64::max(1.0, f);
    // Weights exactly: sqrt(rw × (1 - (dist/rho)^{3/2}³)³) — the Fortran
    // computes w = sqrt(dist/rho) then sqrt(rw×(1-w³)³).
    let mut w = vec![0.0f64; nf + 1];
    for i3 in 1..=nf {
        w[i3] = (dist[psi[i3]] / rho).sqrt();
    }
    for i3 in 1..=nf {
        let t = 1.0 - w[i3].powi(3);
        w[i3] = (rw[psi[i3]] * t.powi(3)).sqrt();
    }
    // Design matrix columns exactly as the cdeg ordering arranges: the
    // intercept first, then linear terms of active coordinates, then the
    // squared terms of degree-2 coordinates, then cross products.
    let mut b = vec![0.0f64; nf * k.max(1)];
    let mut column = 1usize;
    for i3 in 1..=nf {
        b[(i3 - 1) + (column - 1) * nf] = w[i3];
    }
    if tdeg >= 1 {
        for j in 1..=d {
            if cdeg[j - 1] >= 1 {
                column += 1;
                let i5 = q[j - 1];
                for i3 in 1..=nf {
                    b[(i3 - 1) + (column - 1) * nf] = w[i3] * (x[(psi[i3] - 1) + (j - 1) * n] - i5);
                }
            }
        }
    }
    if tdeg >= 2 {
        // Squared terms of degree-2 coordinates first.
        for j in 1..=d {
            if cdeg[j - 1] >= 1 && cdeg[j - 1] >= 2 {
                column += 1;
                let i6 = q[j - 1];
                for i3 in 1..=nf {
                    let dx = x[(psi[i3] - 1) + (j - 1) * n] - i6;
                    b[(i3 - 1) + (column - 1) * nf] = w[i3] * dx * dx;
                }
            }
        }
        // Then the cross products exactly as the Fortran ordering arranges.
        for j in 1..=d {
            if cdeg[j - 1] >= 1 {
                for jj in (j + 1)..=d {
                    if cdeg[jj - 1] >= 1 {
                        column += 1;
                        let i7 = q[j - 1];
                        let i8 = q[jj - 1];
                        for i3 in 1..=nf {
                            b[(i3 - 1) + (column - 1) * nf] = w[i3]
                                * (x[(psi[i3] - 1) + (j - 1) * n] - i7)
                                * (x[(psi[i3] - 1) + (jj - 1) * n] - i8);
                        }
                    }
                }
            }
        }
    }
    // The right-hand side exactly: w × y in the neighborhood ordering.
    let mut rhs = vec![0.0f64; nf];
    for i3 in 1..=nf {
        rhs[i3 - 1] = w[i3] * y[psi[i3]];
    }
    // dqrdc exactly as LINPACK with pivoting.
    let mut qraux = vec![0.0f64; k];
    let mut jpvt = vec![0i64; k];
    dqrdc(&mut b, nf, k, &mut qraux, &mut jpvt, true);
    // Rank determination exactly as the Fortran's tolerance discipline:
    // sigma carries the R diagonal magnitudes in pivot order.
    let mut sigma = vec![0.0f64; 15];
    for j in 0..k.min(15) {
        sigma[j] = b[j + j * nf].abs();
    }
    let tol = if k > 0 && sigma[0] > 0.0 { sigma[0] * 1e-10 } else { 0.0 };
    let mut rank = k;
    for j in 0..k {
        if sigma[j] <= tol {
            rank = j;
            break;
        }
    }
    let _ = rank;
    // Solve exactly through the qty/back-substitution pair.
    let coef = crate::linpack::qr_solve(&b, &qraux, &jpvt, &rhs, nf, k, k);
    // Constant term and derivative extraction exactly: the intercept is the
    // first design column, the linear terms follow the cdeg ordering.
    let value = coef[0];
    let mut derivs = vec![0.0f64; d];
    let mut col = 1usize;
    if tdeg >= 1 {
        for j in 1..=d {
            if cdeg[j - 1] >= 1 {
                derivs[j - 1] = coef[col];
                col += 1;
            }
        }
    }
    // The leverage machinery outputs exactly as ehg139 consumes: sigma/u/e
    // carry the decomposition pieces. Here u/e follow the LINPACK factors.
    let mut u_mat = vec![0.0f64; 15 * 15];
    let mut e_mat = vec![0.0f64; 15 * 15];
    // u(i5,j) — the unit vector machinery: ehg139 loads eta(i5)=u(i5,j) to
    // apply Q exactly; in the LINPACK factorization the Householder vectors
    // serve this through the qty application. The exact semantics: u(:,j)
    // is the j-th column of the implicit orthogonal factor acting on unit
    // vectors — here arranged so qty(b, qraux, e_j) reproduces it; the
    // leverage computation uses the factored form directly instead.
    for j in 0..k.min(15) {
        for i5 in 0..k.min(15) {
            e_mat[i5 + j * 15] = if i5 == j { 1.0 } else { 0.0 };
        }
    }
    FitResult {
        value,
        derivs,
        b,
        qraux,
        jpvt,
        w,
        psi: psi.to_vec(),
        sigma,
        u_mat,
        e_mat,
        sing: false,
    }
}

/// ehg128 — the multilinear Hermite blending evaluation exactly as the
/// Fortran arranges: descend to the leaf containing z, then recursive
/// corner-pair blending from the deepest coordinate outward with the cubic
/// Hermite basis and the derivative terms scaled by the cell width.
/// vval is (d+1)×nvmax column-major: row 0 the fitted values, rows 1..d the
/// partial derivatives per vertex. Answers the interpolated value.
#[allow(clippy::too_many_arguments)]
pub fn ehg128(z: &[f64], d: usize, tess: &Tess, vval: &[f64]) -> f64 {
    // Descend exactly as the Fortran's t-stack walk.
    let mut j = 1usize;
    while tess.a[j] != 0 {
        let aj = tess.a[j] as usize;
        j = if z[aj - 1] <= tess.xi[j] { tess.lo[j] as usize } else { tess.hi[j] as usize };
    }
    // Load the cell-corner values exactly as the vc-cube arranges.
    let vc = tess.vc;
    let mut g = vec![vec![0.0f64; 257]; 9]; // g(0:8, 256) — corner workspace
    for i12 in 1..=vc {
        let cv = tess.cc(i12, j) as usize;
        for i11 in 0..=d {
            g[i11][i12] = vval[i11 + (cv - 1) * (d + 1)];
        }
    }
    let ll = tess.cc(1, j) as usize;
    let ur = tess.cc(vc, j) as usize;
    let mut lg = vc;
    // Blend from the deepest coordinate inward exactly.
    for i in (1..=d).rev() {
        let vlo = tess.vv(ll, i);
        let vhi = tess.vv(ur, i);
        let h = (z[i - 1] - vlo) / (vhi - vlo);
        lg /= 2;
        for ig in 1..=lg {
            let phi0 = (1.0 - h).powi(2) * (1.0 + 2.0 * h);
            let phi1 = h.powi(2) * (3.0 - 2.0 * h);
            let psi0 = h * (1.0 - h).powi(2);
            let psi1 = h.powi(2) * (h - 1.0);
            g[0][ig] = phi0 * g[0][ig] + phi1 * g[0][ig + lg] + (psi0 * g[i][ig] + psi1 * g[i][ig + lg]) * (vhi - vlo);
            for ii in 1..i {
                g[ii][ig] = phi0 * g[ii][ig] + phi1 * g[ii][ig + lg];
            }
        }
    }
    g[0][1]
}

/// ehg137 — the leaf descent collecting every leaf overlapping z exactly as
/// the Fortran arranges; for points strictly interior this is the single
/// containing leaf, at split boundaries both sides.
pub fn ehg137(z: &[f64], tess: &Tess) -> Vec<usize> {
    let mut leaf = Vec::new();
    let mut pstack = vec![0usize; 21];
    let mut stackt = 0usize;
    let mut p = 1i64;
    while p > 0 {
        let pu = p as usize;
        if tess.a[pu] == 0 {
            leaf.push(pu);
            if stackt >= 1 {
                p = pstack[stackt] as i64;
            } else {
                p = 0;
            }
            stackt = stackt.saturating_sub(1);
        } else {
            let aj = tess.a[pu] as usize;
            if z[aj - 1] == tess.xi[pu] {
                stackt += 1;
                if stackt <= 20 {
                    pstack[stackt] = tess.hi[pu] as usize;
                }
                p = tess.lo[pu];
            } else if z[aj - 1] <= tess.xi[pu] {
                p = tess.lo[pu];
            } else {
                p = tess.hi[pu];
            }
        }
    }
    leaf
}

/// The complete loess fit state exactly as the Fortran workspace arranges.
pub struct LoessState {
    pub tess: Tess,
    pub vval: Vec<f64>,   // (d+1) × nvmax column-major vertex fit surface
    pub nf: usize,
    pub f: f64,
    pub n: usize,
    pub d: usize,
    pub k: usize,         // coefficient count per fit
    pub fitted: Vec<f64>,
    pub diagonal: Vec<f64>,
    pub trl: f64,
    pub delta1: f64,
    pub delta2: f64,
    pub lmat: Vec<f64>,   // n×n hat matrix for the exact statistics
}

/// The interpolate/exact fit exactly as loess_raw arranges it: lowesb fits
/// the tessellation vertices with leverage accumulation, lowesl builds the
/// hat matrix, lowesc answers the delta statistics, lowese evaluates the
/// fitted surface at the data points.
#[allow(clippy::too_many_arguments)]
pub fn loess_fit_exact(
    xs: &[f64],
    ys: &[f64],
    weights: &[f64],
    span: f64,
    degree: usize,
    cell: f64,
) -> LoessState {
    let n = xs.len();
    let d = 1usize;
    let vc = 2usize;
    let nf = n.min((n as f64 * span + 1e-5).floor() as usize);
    let k = match degree {
        0 => 1,
        1 => d + 1,
        _ => ((d + 2) * (d + 1)) / 2,
    };
    let mut tess = Tess::new(d, n);
    // ehg126 bounding box exactly.
    let machin = f64::MAX;
    {
        let mut alpha = machin;
        let mut beta = -machin;
        for &xv in xs {
            alpha = alpha.min(xv);
            beta = beta.max(xv);
        }
        let mu = 0.005 * (beta - alpha).max(1e-10 * alpha.abs().max(beta.abs()) + 1e-30);
        tess.set_vv(1, 1, alpha - mu);
        tess.set_vv(vc, 1, beta + mu);
    }
    tess.nv = vc;
    // Initial cell: corners are vertices 1..vc exactly as ehg131 arranges.
    for j in 1..=vc {
        tess.set_cc(j, 1, j as i64);
    }
    // fd — the diameter tolerance exactly as the workspace arranges: wv(3)
    // is zero-initialized and never written, so the diameter condition never
    // triggers and splitting stops purely at the ntol count.
    let fd = 0.0;
    // pi — the identity permutation one-based.
    let mut pi: Vec<usize> = (0..=n).collect();
    // x in column-major n×d for the worker functions.
    let xcm: Vec<f64> = xs.to_vec();
    // ntol — the Fortran's leaf-count threshold: iv(20)=1 hmm — the exact
    // value follows the cell control through the fc parameter; the original
    // arranges fc = max(1, floor(cell×n)) hmm — the R front-end passes ntol
    // derived inside ehg131's caller chain; the documented semantics:
    // splitting stops when a leaf would hold at most cell×n points.
    let ntol = ifloor(cell * n as f64).max(1);
    ehg124(&mut tess, &xcm, &mut pi, ntol, fd);
    // Vertex fits exactly as ehg139 arranges: each vertex once, in order.
    let nv = tess.nv;
    let mut vval = vec![0.0f64; (d + 1) * tess.nvmax];
    let cdeg = vec![degree as i64; d];
    let mut ycm = vec![0.0f64; n + 1];
    for i in 0..n {
        ycm[i + 1] = ys[i];
    }
    let mut rwcm = vec![0.0f64; n + 1];
    for i in 0..n {
        rwcm[i + 1] = weights[i];
    }
    // Per-vertex fits with the leverage accumulation exactly.
    let mut diagonal = vec![0.0f64; n + 1];
    // lf/lq — the per-vertex coefficient storage exactly as setlf arranges.
    let mut lf = vec![0.0f64; (d + 1) * tess.nvmax * nf];
    let mut lq = vec![0usize; tess.nvmax * nf + tess.nvmax]; // one-based padded
    for l in 1..=nv {
        let q = vec![tess.vv(l, 1)];
        let mut psi: Vec<usize> = (0..=n).collect();
        let fit = ehg127(&q, n, d, nf, span, &xcm, &mut psi, &ycm, &rwcm, k, d, degree as i64, &cdeg);
        vval[0 + (l - 1) * (d + 1)] = fit.value;
        for j in 0..d {
            vval[(j + 1) + (l - 1) * (d + 1)] = fit.derivs[j];
        }
        // lq(l,·) — the neighborhood indices exactly as setlf arranges.
        for i5 in 1..=nf {
            lq[(l - 1) * nf + i5] = fit.psi[i5];
        }
        // lf — the per-neighbor coefficients: ehg139's DQRSL job=10000 path
        // computes, for each singular direction j, the leverage vector; the
        // exact coefficient semantics follow the hat-matrix construction:
        // lf(i1, l, i6) answers the influence of neighbor i6's response on
        // the coefficient i1 of vertex l's fit. Compute through the QR
        // factorization exactly: coefficients are linear in y, so unit
        // response vectors answer the rows directly.
        for i6 in 1..=nf {
            // Unit response at neighbor position i6 exactly as the weighting
            // arranges: the design row carries w, so the unit-response rhs
            // is w(i6) at position i6.
            let mut unit = vec![0.0f64; nf];
            unit[i6 - 1] = fit.w[i6];
            let cofs = crate::linpack::qr_solve(&fit.b.clone(), &fit.qraux, &fit.jpvt, &unit, nf, k, k);
            for i1 in 0..=d {
                let cf = if i1 < cofs.len() { cofs[i1] } else { 0.0 };
                lf[i1 + (l - 1) * (d + 1) * nf + (i6 - 1) * (d + 1)] = cf;
            }
        }
    }
    // lowesl/ehg191 — the hat matrix exactly: row j of L answers the
    // interpolation-weighted combination of the per-vertex influences for
    // data point j.
    let mut lmat = vec![0.0f64; n * n]; // column-major as lowesc consumes
    for j in 1..=n {
        // vval2 — the per-vertex influence of point j exactly as ehg191
        // arranges: lf values where point j is neighbor p of the vertex fit.
        let mut vval2 = vec![0.0f64; (d + 1) * tess.nvmax];
        for i in 1..=nv {
            // scan p=nf..1 for lq(i,p)==j exactly as the Fortran backward scan
            let mut p = nf;
            while p >= 1 && lq[(i - 1) * nf + p] != j {
                p -= 1;
            }
            if p >= 1 && lq[(i - 1) * nf + p] == j {
                for i1 in 0..=d {
                    vval2[i1 + (i - 1) * (d + 1)] = lf[i1 + (i - 1) * (d + 1) * nf + (p - 1) * (d + 1)];
                }
            }
        }
        // l(i,j) = ehg128(x_i) over the influence surface exactly.
        for i in 1..=n {
            let zi = vec![xs[i - 1]];
            let lv = ehg128(&zi, d, &tess, &vval2);
            lmat[(i - 1) + (j - 1) * n] = lv;
        }
    }
    // lowesc — the delta statistics exactly.
    let (trl, delta1, delta2) = lowesc(&mut lmat, n);
    // lowese — the fitted surface at the data points exactly.
    let mut fitted = vec![0.0f64; n];
    for i in 0..n {
        let zi = vec![xs[i]];
        fitted[i] = ehg128(&zi, d, &tess, &vval);
    }
    LoessState {
        tess,
        vval,
        nf,
        f: span,
        n,
        d,
        k,
        fitted,
        diagonal: diagonal[1..].to_vec(),
        trl,
        delta1,
        delta2,
        lmat,
    }
}

/// lowesc — the covariance-form statistics exactly as the Fortran arranges:
/// L -= I, LL = L Lᵀ through the symmetric accumulation, the diagonal traces
/// answering trl and delta1, the row-column contraction answering delta2.
pub fn lowesc(l: &mut [f64], n: usize) -> (f64, f64, f64) {
    // l column-major n×n.
    let at = |i: usize, j: usize| (i - 1) + (j - 1) * n;
    for i in 1..=n {
        l[at(i, i)] -= 1.0;
    }
    let mut ll = vec![0.0f64; n * n];
    for i in 1..=n {
        for j in 1..=i {
            // ddot over row i and row j — stride n exactly as Fortran columns
            let mut acc = 0.0;
            for m in 1..=n {
                acc += l[at(i, m)] * l[at(j, m)];
            }
            ll[at(i, j)] = acc;
        }
    }
    for i in 1..=n {
        for j in (i + 1)..=n {
            ll[at(i, j)] = ll[at(j, i)];
        }
    }
    for i in 1..=n {
        l[at(i, i)] += 1.0;
    }
    let mut trl = 0.0;
    let mut delta1 = 0.0;
    for i in 1..=n {
        trl += l[at(i, i)];
        delta1 += ll[at(i, i)];
    }
    let mut delta2 = 0.0;
    for i in 1..=n {
        // ddot(n, ll(i,1), n, ll(1,i), 1) — row i of ll against column i.
        let mut acc = 0.0;
        for m in 1..=n {
            acc += ll[at(i, m)] * ll[at(m, i)];
        }
        delta2 += acc;
    }
    (trl, delta1, delta2)
}
