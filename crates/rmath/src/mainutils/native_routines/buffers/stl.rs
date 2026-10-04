//! Authoritative typed admission for GNU's seventeen STL buffers.
#![forbid(unsafe_code)]
use super::BufferType::{Integer, Real};
use super::{BufferError, BufferInterface, BufferRoutine, NativeBuffer, dimension, scalar};
use crate::library::stats::stl::kernel::{self, Lengths, Output, Parameters};
pub(crate) const ROUTINE: BufferRoutine = BufferRoutine::owned(
    "stats",
    BufferInterface::Fortran,
    &[
        Real, Integer, Integer, Integer, Integer, Integer, Integer, Integer, Integer, Integer,
        Integer, Integer, Integer, Integer, Real, Real, Real,
    ],
    shape,
    invoke,
);
fn parameters(b: &[NativeBuffer]) -> Result<Parameters, BufferError> {
    Ok(Parameters::new(
        dimension(b, 1, 0)?,
        scalar(b, 2)?,
        [scalar(b, 3)?, scalar(b, 4)?, scalar(b, 5)?],
        [scalar(b, 6)?, scalar(b, 7)?, scalar(b, 8)?],
        [scalar(b, 9)?, scalar(b, 10)?, scalar(b, 11)?],
        scalar(b, 12)?,
        scalar(b, 13)?,
    ))
}
fn shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    parameters(b)?
        .validate(Lengths {
            y: b[0].len(),
            weights: b[14].len(),
            season: b[15].len(),
            trend: b[16].len(),
        })
        .map(|_| ())
        .map_err(|e| BufferError::new(e.to_string()))
}
fn invoke(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    ROUTINE.validate_buffers(BufferInterface::Fortran, b)?;
    let p = parameters(b)?;
    let [
        NativeBuffer::Real(y),
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        NativeBuffer::Real(weights),
        NativeBuffer::Real(season),
        NativeBuffer::Real(trend),
    ] = b
    else {
        return Err(BufferError::new("invalid STL buffer types"));
    };
    kernel::filter(
        y,
        p,
        Output {
            weights,
            season,
            trend,
        },
    )
    .map_err(|e| BufferError::new(e.to_string()))
}
