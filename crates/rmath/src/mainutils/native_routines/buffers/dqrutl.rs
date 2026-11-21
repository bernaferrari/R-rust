//! Port of R's src/appl/dqrutl.f — thin column loops over `dqrsl()`.
//!
//! GNU registers `dqrdc2`, `dqrcf`, `dqrqy`, `dqrqty`, `dqrrsd` and `dqrxb`
//! as base-package Fortran symbols (`.F_*` in the base namespace). The R-level
//! `qr.R` sources staged by the upstream loader reach them through
//! `.Fortran(.F_dqrrsd, ...)`; these kernels provide the same contracts over
//! the ported `appl::linpack_qr` routines.

use super::{Args, BufferError, NativeBuffer, dimension, finite, minimum, product};

/// `dqrdc2(x, ldx, n, p, tol, k, qraux, jpvt, work)` — pivoted Householder QR.
pub(crate) fn dqrdc2(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    let mut a = Args::new(b);
    let x = a.real();
    let ldx = a.int()[0];
    let n = a.int()[0];
    let p = a.int()[0];
    let tol = a.real()[0];
    let k = &mut a.int()[0];
    let qraux = a.real();
    let jpvt = a.int();
    let work = a.real();
    crate::appl::linpack_qr::dqrdc2_impl(x, ldx as usize, n as usize, p as usize, tol, k, qraux, jpvt, work);
    Ok(())
}

/// Shared shape admission for the `dqrsl` column loops:
/// `(x, n, k, qraux, y, ny, out)` plus optional `b` and `info` tails.
fn dqrsl_shape(b: &[NativeBuffer], out: usize, extra: &[usize]) -> Result<(), BufferError> {
    let n = dimension(b, 1, 0)?;
    let k = dimension(b, 2, 0)?;
    minimum(b, 0, product(&[n, k])?)?;
    minimum(b, 3, k)?;
    let ny = dimension(b, 5, 0)?;
    minimum(b, 4, product(&[n, ny])?)?;
    minimum(b, out, product(&[n, ny])?)?;
    for (index, length) in extra.iter().enumerate() {
        minimum(b, 6 + index, *length)?;
    }
    finite(b, 0)?;
    finite(b, 4)
}

pub(crate) fn dqrdc2_shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let ldx = dimension(b, 1, 1)?;
    let n = dimension(b, 2, 0)?;
    let p = dimension(b, 3, 0)?;
    if ldx < n {
        return Err(BufferError::new(
            "dqrdc2 leading dimension smaller than matrix",
        ));
    }
    minimum(b, 0, product(&[ldx, p])?)?;
    minimum(b, 4, 1)?;
    minimum(b, 5, 1)?;
    minimum(b, 6, p)?;
    minimum(b, 7, p)?;
    minimum(b, 8, product(&[2, p])?)?;
    finite(b, 0)?;
    finite(b, 4)
}

pub(crate) fn dqrqty_shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    dqrsl_shape(b, 6, &[])
}

pub(crate) fn dqrqy_shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    dqrsl_shape(b, 6, &[])
}

pub(crate) fn dqrrsd_shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    dqrsl_shape(b, 6, &[])
}

pub(crate) fn dqrxb_shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    dqrsl_shape(b, 6, &[])
}

pub(crate) fn dqrcf_shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let k = dimension(b, 2, 0)?;
    let ny = dimension(b, 5, 0)?;
    dqrsl_shape(b, 6, &[product(&[k, ny])?, 1])
}

/// Which `dqrsl` output a column loop publishes.
#[derive(Clone, Copy, PartialEq)]
enum Output {
    Qy,
    Qty,
    Rsd,
    Xb,
}

/// Run the selected QR operation per column. The residual and fitted-value
/// routines use each input column as the in-place qty workspace.
fn dqrsl_columns(b: &mut [NativeBuffer], output: Output) -> Result<(), BufferError> {
    use crate::appl::linpack_qr::dqrsl_impl;
    let mut a = Args::new(b);
    let x = a.real();
    let n = a.int()[0] as usize;
    let k = a.int()[0] as usize;
    let qraux = a.real();
    let y = a.real();
    let ny = a.int()[0] as usize;
    let out = a.real();
    if n == 0 {
        return Ok(());
    }
    let mut info = 0;
    for j in 0..ny {
        let range = j * n..(j + 1) * n;
        let y = &mut y[range.clone()];
        let out = &mut out[range];
        let (y, qy, qty, rsd, xb, flags): (_, &mut [f64], &mut [f64], &mut [f64], &mut [f64], _) = match output {
            Output::Qy => (Some(&*y), out, &mut [], &mut [], &mut [], [true, false, false, false, false]),
            Output::Qty => (Some(&*y), &mut [], out, &mut [], &mut [], [false, true, false, false, false]),
            Output::Rsd => (None, &mut [], y, out, &mut [], [false, true, false, true, false]),
            Output::Xb => (None, &mut [], y, &mut [], out, [false, true, false, false, true]),
        };
        dqrsl_impl(x, n, n, k, qraux, y, qy, qty, &mut [], rsd, xb, flags, &mut info);
    }
    Ok(())
}

/// `dqrqty(x, n, k, qraux, y, ny, qty)` — job 1000.
pub(crate) fn dqrqty(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    dqrsl_columns(b, Output::Qty)
}

/// `dqrqy(x, n, k, qraux, y, ny, qy)` — job 10000 (qty slot unused).
pub(crate) fn dqrqy(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    dqrsl_columns(b, Output::Qy)
}

/// `dqrrsd(x, n, k, qraux, y, ny, rsd)` — job 10.
pub(crate) fn dqrrsd(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    dqrsl_columns(b, Output::Rsd)
}

/// `dqrxb(x, n, k, qraux, y, ny, xb)` — job 1.
pub(crate) fn dqrxb(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    dqrsl_columns(b, Output::Xb)
}

/// `dqrcf(x, n, k, qraux, y, ny, b, info)` — job 100 per column, with the
/// column doubling as the `qty` slot as in dqrutl.f.
pub(crate) fn dqrcf(buffers: &mut [NativeBuffer]) -> Result<(), BufferError> {
    let mut a = Args::new(buffers);
    let x = a.real();
    let n = a.int()[0];
    let k = a.int()[0];
    let qraux = &*a.real();
    let y = a.real();
    let ny = a.int()[0];
    let b = a.real();
    let info = a.int();
    let (nu, ku) = (n as usize, k as usize);
    let mut column_info = 0;
    if nu != 0 {
        for j in 0..ny as usize {
            crate::appl::linpack_qr::dqrsl_impl(
                x, nu, nu, ku, qraux, None, &mut [],
                &mut y[j * nu..(j + 1) * nu],
                &mut b[j * ku..(j + 1) * ku],
                &mut [], &mut [], [false, true, true, false, false], &mut column_info,
            );
        }
    }
    info[0] = column_info;
    Ok(())
}
