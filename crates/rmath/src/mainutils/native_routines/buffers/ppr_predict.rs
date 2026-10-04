//! Authoritative packed PPR prediction admission and exact safe slice invocation.
#![forbid(unsafe_code)]
use super::BufferType::{Integer, Real};
use super::{BufferError, BufferInterface, BufferRoutine, NativeBuffer, dimension};
use crate::library::stats::ppr_predict::{self, Shape};
pub(crate) const ROUTINE: BufferRoutine = BufferRoutine::owned(
    "stats",
    BufferInterface::Fortran,
    &[Integer, Real, Real, Real, Real],
    shape,
    invoke,
);
fn shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let np = dimension(b, 0, 0)?;
    let model = b[2].reals()?;
    let shape = Shape::read(model).map_err(|e| BufferError::new(e.to_string()))?;
    shape
        .validate(np, b[1].len(), b[3].len(), b[4].len())
        .map_err(|e| BufferError::new(e.to_string()))?;
    shape
        .validate_projections(np, b[1].reals()?, model)
        .map_err(|e| BufferError::new(e.to_string()))
}
fn invoke(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    ROUTINE.validate_buffers(BufferInterface::Fortran, b)?;
    let np = dimension(b, 0, 0)?;
    let [
        _,
        NativeBuffer::Real(x),
        NativeBuffer::Real(model),
        NativeBuffer::Real(y),
        NativeBuffer::Real(scratch),
    ] = b
    else {
        return Err(BufferError::new("invalid PPR prediction types"));
    };
    ppr_predict::predict(np, x, model, y, scratch).map_err(|e| BufferError::new(e.to_string()))
}
