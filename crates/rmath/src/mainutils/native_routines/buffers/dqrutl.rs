//! Port of R's src/appl/dqrutl.f — thin column loops over `dqrsl()`.
//!
//! GNU registers `dqrdc2`, `dqrcf`, `dqrqy`, `dqrqty`, `dqrrsd` and `dqrxb`
//! as base-package Fortran symbols (`.F_*` in the base namespace). The R-level
//! `qr.R` sources staged by the upstream loader reach them through
//! `.Fortran(.F_dqrrsd, ...)`; these kernels provide the same contracts over
//! the ported `appl::linpack_qr` routines.

use std::os::raw::{c_int, c_void};

use super::{BufferError, NativeBuffer, dimension, finite, minimum, product};

/// `dqrdc2(x, ldx, n, p, tol, k, qraux, jpvt, work)` — pivoted Householder QR.
pub(crate) unsafe extern "C-unwind" fn dqrdc2(
    x: *mut c_void,
    ldx: *mut c_void,
    n: *mut c_void,
    p: *mut c_void,
    tol: *mut c_void,
    k: *mut c_void,
    qraux: *mut c_void,
    jpvt: *mut c_void,
    work: *mut c_void,
) {
    unsafe {
        crate::appl::linpack_qr::dqrdc2(
            x.cast(),
            *ldx.cast::<c_int>(),
            *n.cast::<c_int>(),
            *p.cast::<c_int>(),
            *tol.cast::<f64>(),
            k.cast(),
            qraux.cast(),
            jpvt.cast(),
            work.cast(),
        );
    }
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

/// One `dqrsl` call per column, mirroring dqrutl.f exactly: `y` doubles as
/// the `qty` slot except for dqrqty itself, which writes `qty(1,j)`.
unsafe fn dqrsl_columns(
    x: *const f64,
    n: c_int,
    k: c_int,
    qraux: *const f64,
    y: *mut f64,
    ny: c_int,
    qy: *mut f64,
    qty: *mut f64,
    b: *mut f64,
    rsd: *mut f64,
    xb: *mut f64,
    job: c_int,
) {
    unsafe {
        let mut column_info: c_int = 0;
        for j in 0..ny as isize {
            let column = y.offset(j * n as isize);
            let offset = |out: *mut f64, stride: isize| {
                if out.is_null() {
                    std::ptr::null_mut()
                } else {
                    out.offset(j * stride)
                }
            };
            crate::appl::linpack_qr::dqrsl(
                x.cast_mut(),
                n,
                n,
                k,
                qraux,
                column,
                offset(qy, n as isize),
                if qty.is_null() {
                    column
                } else {
                    offset(qty, n as isize)
                },
                offset(b, k as isize),
                offset(rsd, n as isize),
                offset(xb, n as isize),
                job,
                &mut column_info,
            );
        }
    }
}

/// `dqrqty(x, n, k, qraux, y, ny, qty)` — job 1000.
pub(crate) unsafe extern "C-unwind" fn dqrqty(
    x: *mut c_void,
    n: *mut c_void,
    k: *mut c_void,
    qraux: *mut c_void,
    y: *mut c_void,
    ny: *mut c_void,
    qty: *mut c_void,
) {
    unsafe {
        dqrsl_columns(
            x.cast(),
            *n.cast::<c_int>(),
            *k.cast::<c_int>(),
            qraux.cast(),
            y.cast(),
            *ny.cast::<c_int>(),
            std::ptr::null_mut(),
            qty.cast(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            1000,
        );
    }
}

/// `dqrqy(x, n, k, qraux, y, ny, qy)` — job 10000 (qty slot unused).
pub(crate) unsafe extern "C-unwind" fn dqrqy(
    x: *mut c_void,
    n: *mut c_void,
    k: *mut c_void,
    qraux: *mut c_void,
    y: *mut c_void,
    ny: *mut c_void,
    qy: *mut c_void,
) {
    unsafe {
        dqrsl_columns(
            x.cast(),
            *n.cast::<c_int>(),
            *k.cast::<c_int>(),
            qraux.cast(),
            y.cast(),
            *ny.cast::<c_int>(),
            qy.cast(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            10000,
        );
    }
}

/// `dqrrsd(x, n, k, qraux, y, ny, rsd)` — job 10.
pub(crate) unsafe extern "C-unwind" fn dqrrsd(
    x: *mut c_void,
    n: *mut c_void,
    k: *mut c_void,
    qraux: *mut c_void,
    y: *mut c_void,
    ny: *mut c_void,
    rsd: *mut c_void,
) {
    unsafe {
        dqrsl_columns(
            x.cast(),
            *n.cast::<c_int>(),
            *k.cast::<c_int>(),
            qraux.cast(),
            y.cast(),
            *ny.cast::<c_int>(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            rsd.cast(),
            std::ptr::null_mut(),
            10,
        );
    }
}

/// `dqrxb(x, n, k, qraux, y, ny, xb)` — job 1.
pub(crate) unsafe extern "C-unwind" fn dqrxb(
    x: *mut c_void,
    n: *mut c_void,
    k: *mut c_void,
    qraux: *mut c_void,
    y: *mut c_void,
    ny: *mut c_void,
    xb: *mut c_void,
) {
    unsafe {
        dqrsl_columns(
            x.cast(),
            *n.cast::<c_int>(),
            *k.cast::<c_int>(),
            qraux.cast(),
            y.cast(),
            *ny.cast::<c_int>(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            xb.cast(),
            1,
        );
    }
}

/// `dqrcf(x, n, k, qraux, y, ny, b, info)` — job 100 per column.
pub(crate) unsafe extern "C-unwind" fn dqrcf(
    x: *mut c_void,
    n: *mut c_void,
    k: *mut c_void,
    qraux: *mut c_void,
    y: *mut c_void,
    ny: *mut c_void,
    b: *mut c_void,
    info: *mut c_void,
) {
    unsafe {
        let mut column_info = 0 as c_int;
        let n_val = *n.cast::<c_int>();
        let k_val = *k.cast::<c_int>();
        let ny_val = *ny.cast::<c_int>();
        for j in 0..ny_val as isize {
            let column = (y as *mut f64).offset(j * n_val as isize);
            crate::appl::linpack_qr::dqrsl(
                x.cast(),
                n_val,
                n_val,
                k_val,
                qraux.cast(),
                column,
                std::ptr::null_mut(),
                column,
                (b as *mut f64).offset(j * k_val as isize),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                100,
                &mut column_info,
            );
        }
        if !info.is_null() {
            *(info as *mut c_int) = column_info;
        }
    }
}
