//! Authoritative default supersmoother admission; no PPR settings or raw aliases.
#![forbid(unsafe_code)]
use super::BufferType::{Integer, Real};
use super::{
    BufferError, BufferInterface, BufferRoutine, NativeBuffer, dimension, minimum, scalar,
    scalar_real,
};
use crate::library::stats::supsmu::{self, Input, Output, Parameters};
pub(crate) const ROUTINE: BufferRoutine = BufferRoutine::owned(
    "stats",
    BufferInterface::Fortran,
    &[
        Integer, Real, Real, Real, Integer, Real, Real, Real, Real, Real,
    ],
    shape,
    invoke,
);
fn parameters(b: &[NativeBuffer]) -> Result<Parameters, BufferError> {
    let n = dimension(b, 0, 1)?;
    minimum(b, 1, n)?;
    let NativeBuffer::Real(x) = &b[1] else {
        return Err(BufferError::new("invalid supersmoother abscissae"));
    };
    let constant = x[n - 1] <= x[0];
    let span = if constant { 0. } else { scalar_real(b, 5)? };
    Ok(Parameters {
        n,
        periodic: if constant { 1 } else { scalar(b, 4)? },
        span,
        alpha: if constant || span > 0. {
            0.
        } else {
            scalar_real(b, 6)?
        },
    })
}
fn shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let p = parameters(b)?;
    let [
        _,
        NativeBuffer::Real(x),
        NativeBuffer::Real(y),
        NativeBuffer::Real(weights),
        _,
        _,
        _,
        _,
        _,
        _,
    ] = b
    else {
        return Err(BufferError::new("invalid supersmoother input types"));
    };
    p.validate(
        Input { x, y, weights },
        [b[7].len(), b[8].len(), b[9].len()],
    )
    .map_err(|e| BufferError::new(e.to_string()))
}
fn invoke(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    ROUTINE.validate_buffers(BufferInterface::Fortran, b)?;
    let p = parameters(b)?;
    let [
        _,
        NativeBuffer::Real(x),
        NativeBuffer::Real(y),
        NativeBuffer::Real(weights),
        _,
        _,
        _,
        NativeBuffer::Real(smoothed),
        NativeBuffer::Real(scratch),
        NativeBuffer::Real(edf),
    ] = b
    else {
        return Err(BufferError::new("invalid supersmoother input types"));
    };
    supsmu::filter(
        Input { x, y, weights },
        p,
        Output {
            smoothed,
            scratch,
            edf,
        },
    )
    .map_err(|e| BufferError::new(e.to_string()))
}
