//! Multivariate Burg estimation with owned, checked numerical workspaces.
//! Derived from GNU R mAR.c (Martyn Plummer and the R Core Team).
#![forbid(unsafe_code)]
mod algebra;
use algebra::Matrix;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Overflow,
    Storage,
    Allocation,
    Dimensions,
    Method,
    SingularQr,
    SingularDet,
    Convergence,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Overflow => "multi_burg workspace size overflow",
            Self::Storage => "multi_burg buffer too short",
            Self::Allocation => "cannot allocate multi_burg workspace",
            Self::Dimensions => "invalid multi_burg dimensions",
            Self::Method => "Invalid vmethod",
            Self::SingularQr => "Singular matrix in qr_solve",
            Self::SingularDet => "Singular matrix in ldet",
            Self::Convergence => "Burg's algorithm failed to find partial correlation",
        })
    }
}
type Result<T> = std::result::Result<T, Error>;
fn zeros(n: usize) -> Result<Vec<f64>> {
    if n > isize::MAX as usize / size_of::<f64>() {
        return Err(Error::Overflow);
    }
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| Error::Allocation)?;
    v.resize(n, 0.);
    Ok(v)
}
#[derive(Clone, Copy)]
pub(crate) struct Parameters {
    pub n: usize,
    pub series: usize,
    pub max_order: usize,
    pub use_aic: bool,
    pub method: i32,
}
impl Parameters {
    pub(crate) fn lengths(self) -> Result<(usize, usize, usize)> {
        if self.n == 0 || self.series == 0 || self.max_order >= self.n {
            return Err(Error::Dimensions);
        }
        if self.max_order > 0 && self.method != 1 && self.method != 2 {
            return Err(Error::Method);
        }
        let square = self
            .series
            .checked_mul(self.series)
            .ok_or(Error::Overflow)?;
        let fourth = square.checked_mul(square).ok_or(Error::Overflow)?;
        let cube = self
            .max_order
            .checked_add(1)
            .and_then(|n| n.checked_mul(square))
            .ok_or(Error::Overflow)?;
        let data = self.n.checked_mul(self.series).ok_or(Error::Overflow)?;
        for count in [fourth, cube, data] {
            if count > isize::MAX as usize / size_of::<f64>() {
                return Err(Error::Overflow);
            }
        }
        Ok((data, cube, self.max_order + 1))
    }
}
pub(crate) struct Output {
    pub residuals: Vec<f64>,
    pub coefficients: Vec<f64>,
    pub partial: Vec<f64>,
    pub variance: Vec<f64>,
    pub aic: Vec<f64>,
    pub order: i32,
}
fn block(data: &[f64], lag: usize, series: usize) -> Result<Matrix> {
    let width = series * series;
    Matrix::from_slice(series, series, &data[lag * width..(lag + 1) * width])
}
fn partial(ff: &Matrix, bb: &Matrix, fb: &Matrix, e: &Matrix) -> Result<(Matrix, Matrix)> {
    let n = e.rows;
    let square = n * n;
    let bf = fb.transpose()?;
    let ef = e.solve(&bf)?;
    let f = e.solve(fb)?;
    let g = e.solve(&e.solve(bb)?.transpose()?)?;
    let h = e.solve(&e.solve(ff)?.transpose()?)?;
    let mut theta = Matrix::zero(n, n)?;
    let mut theta_vec = Matrix::zero(square, 1)?;
    for _ in 0..20 {
        let ka = e.solve(&theta.transpose()?)?.transpose()?;
        let kb = e.solve(&theta)?.transpose()?;
        let mut s = ff.copy()?;
        let tmp = ka.product(&bf, false, false)?;
        s.add(&tmp, true)?;
        s.add(&tmp.transpose()?, true)?;
        s.add(
            &ka.product(&bb.product(&ka, false, true)?, false, false)?,
            false,
        )?;
        s.add(bb, false)?;
        let tmp = kb.product(fb, false, false)?;
        s.add(&tmp, true)?;
        s.add(&tmp.transpose()?, true)?;
        s.add(
            &kb.product(&ff.product(&kb, false, true)?, false, false)?,
            false,
        )?;
        let mut d1 = s.product(&f, false, false)?;
        d1.add(&ef.product(&s, true, false)?, false)?;
        let sg = s.product(&g, false, false)?;
        let sh = s.product(&h, false, false)?;
        let mut d2 = Matrix::zero(square, square)?;
        for i in 0..n {
            for j in 0..n {
                for k in 0..n {
                    for l in 0..n {
                        d2.set(
                            n * i + j,
                            n * k + l,
                            (if i == k { sg.get(j, l) } else { 0. })
                                + (if j == l { sh.get(i, k) } else { 0. }),
                        );
                    }
                }
            }
        }
        let old = theta_vec;
        theta_vec = d2.solve(&Matrix::from_slice(square, 1, &d1.data)?)?;
        theta.data.copy_from_slice(&theta_vec.data);
        let mut diff = old;
        diff.add(&theta_vec, true)?;
        let objective = diff
            .product(&d2.product(&diff, false, false)?, true, false)?
            .data[0];
        if objective < 1e-8 {
            return Ok((ka, kb));
        }
    }
    Err(Error::Convergence)
}
pub(crate) fn fit(p: Parameters, x: &[f64], original_partial: &[f64]) -> Result<Output> {
    let (data, cube, lags) = p.lengths()?;
    if x.len() < data || original_partial.len() < cube {
        return Err(Error::Storage);
    }
    let n = p.series;
    let width = n * n;
    let source = Matrix::from_slice(n, p.n, &x[..data])?;
    let mut forward = source.copy()?;
    let mut backward = source.copy()?;
    let mut a = zeros(cube)?;
    let mut b = zeros(cube)?;
    let identity = Matrix::identity(n)?;
    a[..width].copy_from_slice(&identity.data);
    b[..width].copy_from_slice(&identity.data);
    let mut best = zeros(cube)?;
    best.copy_from_slice(&a);
    let mut partial_out = zeros(cube)?;
    partial_out.copy_from_slice(&original_partial[..cube]);
    let mut variance = zeros(cube)?;
    let mut e = forward.product(&forward, false, true)?;
    e.divide(p.n as f64);
    variance[..width].copy_from_slice(&e.data);
    let mut aic = zeros(lags)?;
    // Evaluate order zero first; no caller output is written before every order succeeds.
    let mut determinant_error = None;
    let mut criterion = |covariance: &Matrix, lag: usize| match covariance.log_determinant() {
        Ok(log) => p.n as f64 * log + 2. * lag as f64 * n as f64 * n as f64,
        Err(error) => {
            determinant_error.get_or_insert(error);
            f64::NAN
        }
    };
    aic[0] = criterion(&e, 0);
    let mut selected = 0usize;
    let mut minimum = aic[0];
    for m in 0..p.max_order {
        for i in 0..n {
            for j in (m + 1..p.n).rev() {
                backward.set(i, j, backward.get(i, j - 1));
            }
            forward.set(i, m, 0.);
            backward.set(i, m, 0.);
        }
        let ff = forward.product(&forward, false, true)?;
        let bb = backward.product(&backward, false, true)?;
        let fb = forward.product(&backward, false, true)?;
        let (ka, kb) = partial(&ff, &bb, &fb, &e)?;
        let mut next_a = zeros(cube)?;
        let mut next_b = zeros(cube)?;
        for lag in 0..=m + 1 {
            let mut av = block(&a, lag, n)?;
            av.add(
                &ka.product(&block(&b, m + 1 - lag, n)?, false, false)?,
                true,
            )?;
            let mut bv = block(&b, lag, n)?;
            bv.add(
                &kb.product(&block(&a, m + 1 - lag, n)?, false, false)?,
                true,
            )?;
            next_a[lag * width..(lag + 1) * width].copy_from_slice(&av.data);
            next_b[lag * width..(lag + 1) * width].copy_from_slice(&bv.data);
        }
        a = next_a;
        b = next_b;
        let forward_delta = ka.product(&backward, false, false)?;
        let backward_delta = kb.product(&forward, false, false)?;
        forward.add(&forward_delta, true)?;
        backward.add(&backward_delta, true)?;
        if p.method == 1 {
            let mut update = identity.copy()?;
            update.add(&ka.product(&kb, false, false)?, true)?;
            e = update.product(&e, false, false)?;
        } else {
            e = forward.product(&forward, false, true)?;
            e.add(&backward.product(&backward, false, true)?, false)?;
            e.divide(2. * (p.n - m - 1) as f64);
        }
        variance[(m + 1) * width..(m + 2) * width].copy_from_slice(&e.data);
        partial_out[(m + 1) * width..(m + 2) * width].copy_from_slice(&ka.data);
        aic[m + 1] = criterion(&e, m + 1);
        if !p.use_aic || aic[m + 1] < minimum {
            selected = m + 1;
            minimum = aic[m + 1];
            best.copy_from_slice(&a);
        }
    }
    // GNU completes all partial-correlation solves before evaluating ldet.
    // Defer determinant errors so a later Burg solve retains error precedence.
    if let Some(error) = determinant_error {
        return Err(error);
    }
    if p.use_aic {
        forward = Matrix::zero(n, p.n)?;
        let mut shifted = Matrix::zero(n, p.n)?;
        for lag in 0..=selected {
            for i in 0..n {
                for j in 0..p.n - selected {
                    shifted.set(i, j + selected, source.get(i, j + selected - lag));
                }
            }
            shifted = block(&best, lag, n)?.product(&shifted, false, false)?;
            forward.add(&shifted, false)?;
        }
    }
    Ok(Output {
        residuals: forward.data,
        coefficients: best,
        partial: partial_out,
        variance,
        aic,
        order: i32::try_from(selected).map_err(|_| Error::Overflow)?,
    })
}
