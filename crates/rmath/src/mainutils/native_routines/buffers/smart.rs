//! Authoritative SMART (projection-pursuit regression) admission and exact
//! checked invocation for `.Fortran(C_setppr, ...)` and `.Fortran(C_smart, ...)`.
#![forbid(unsafe_code)]
use super::BufferType::{Integer, Real};
use super::{
    BufferError, BufferInterface, BufferRoutine, NativeBuffer, dimension, finite, minimum,
};
use crate::library::stats::smart;

pub(crate) const SETPPR_ROUTINE: BufferRoutine = BufferRoutine::owned(
    "stats",
    BufferInterface::Fortran,
    &[Real, Real, Integer, Integer, Real, Real],
    setppr_shape,
    setppr_invoke,
);
fn setppr_shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    for i in 0..6 {
        minimum(b, i, 1)?;
    }
    finite(b, 0)?;
    finite(b, 1)?;
    finite(b, 4)?;
    finite(b, 5)?;
    Ok(())
}
fn setppr_invoke(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    SETPPR_ROUTINE.validate_buffers(BufferInterface::Fortran, b)?;
    let span1 = b[0].reals()?[0];
    let alpha1 = b[1].reals()?[0];
    let optlevel = b[2].integers()?[0];
    let ism = b[3].integers()?[0];
    let df1 = b[4].reals()?[0];
    let gcvpen1 = b[5].reals()?[0];
    smart::setppr(span1, alpha1, optlevel, ism, df1, gcvpen1);
    Ok(())
}

pub(crate) const SMART_ROUTINE: BufferRoutine = BufferRoutine::owned(
    "stats",
    BufferInterface::Fortran,
    &[
        Integer, Integer, Integer, Integer, Integer, // m mu p q n
        Real, Real, Real, Real, // w x y ww
        Real, Integer, // smod nsmod
        Real, Integer, // sp nsp
        Real, Integer, // dp ndp
        Real,    // edf
    ],
    smart_shape,
    smart_invoke,
);
fn checked_mul(a: usize, b: usize) -> Result<usize, BufferError> {
    a.checked_mul(b)
        .filter(|v| *v <= i32::MAX as usize)
        .ok_or_else(|| BufferError::new("native dimension product overflow"))
}
fn checked_add(a: usize, b: usize) -> Result<usize, BufferError> {
    a.checked_add(b)
        .ok_or_else(|| BufferError::new("native dimension sum overflow"))
}
fn smart_shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let m = dimension(b, 0, 1)?;
    let mu = dimension(b, 1, 1)?;
    let p = dimension(b, 2, 1)?;
    let q = dimension(b, 3, 1)?;
    let n = dimension(b, 4, 1)?;
    if mu > m {
        return Err(BufferError::new("number of ppr terms exceeds max.terms"));
    }
    minimum(b, 5, n)?; // w
    minimum(b, 6, checked_mul(p, n)?)?; // x(p,n)
    minimum(b, 7, checked_mul(q, n)?)?; // y(q,n)
    minimum(b, 8, q)?; // ww
    // smod: q+6 header/means/scale, a,b,f,t blocks, asr(15) (GNU's msmod tail
    // covers asr(2..1+m), so only min(15, m+1) doubles are ever touched).
    let smod_need = checked_add(
        q + 7 + checked_mul(m, checked_add(checked_add(p, q)?, checked_mul(2, n)?)?)?,
        15.min(m + 1),
    )?;
    minimum(b, 9, smod_need)?;
    // sp: r(q*n) + sc(n,15) + bt(q) + g(p,3)
    let sp_need = checked_add(
        checked_add(checked_add(checked_mul(q, n)?, checked_mul(15, n)?)?, q)?,
        checked_mul(3, p)?,
    )?;
    minimum(b, 11, sp_need)?;
    // dp: conjugate-gradient workspace p*(p+1)/2 + 6p
    minimum(b, 13, checked_add(p * (p + 1) / 2, checked_mul(6, p)?)?)?;
    minimum(b, 15, m)?; // edf
    finite(b, 5)?;
    finite(b, 6)?;
    finite(b, 7)?;
    finite(b, 8)?;
    Ok(())
}
fn smart_invoke(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    SMART_ROUTINE.validate_buffers(BufferInterface::Fortran, b)?;
    let m = b[0].integers()?[0] as usize;
    let mu = b[1].integers()?[0] as usize;
    let p = b[2].integers()?[0] as usize;
    let q = b[3].integers()?[0] as usize;
    let n = b[4].integers()?[0] as usize;
    let [
        _,
        _,
        _,
        _,
        _,
        NativeBuffer::Real(w),
        NativeBuffer::Real(x),
        NativeBuffer::Real(y),
        NativeBuffer::Real(ww),
        NativeBuffer::Real(smod),
        _,
        NativeBuffer::Real(sp),
        _,
        NativeBuffer::Real(dp),
        _,
        NativeBuffer::Real(edf),
    ] = b
    else {
        return Err(BufferError::new("invalid SMART buffer types"));
    };
    smart::smart(m, mu, p, q, n, w, x, y, ww, smod, sp, dp, edf).map_err(BufferError::new)
}
