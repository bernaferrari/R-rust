//! Checked native admission and invocation without raw projections.
#![forbid(unsafe_code)]
use super::BufferType::{Integer, Real};
use super::{
    BufferError, BufferInterface, BufferRoutine, NativeBuffer, dimension, minimum, scalar,
    scalar_real,
};

pub(crate) const ROUTINE: BufferRoutine = BufferRoutine::owned(
    "stats",
    BufferInterface::C,
    &[
        Real, Integer, Real, Real, Real, Integer, Integer, Integer, Integer, Integer, Real, Real,
        Real, Real, Real, Real, Real,
    ],
    shape,
    invoke,
);

fn holtwinters_parameters(
    b: &[NativeBuffer],
) -> Result<crate::library::stats::holtwinters::kernel::Parameters, BufferError> {
    use crate::library::stats::holtwinters::kernel::Parameters;
    Ok(Parameters {
        alpha: scalar_real(b, 2)?,
        beta: scalar_real(b, 3)?,
        gamma: scalar_real(b, 4)?,
        start_time: dimension(b, 5, 1)?,
        additive: scalar(b, 6)? == 1,
        period: dimension(b, 7, 0)?,
        trend: scalar(b, 8)? == 1,
        seasonal: scalar(b, 9)? == 1,
    })
}

fn shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let xl = dimension(b, 1, 0)?;
    minimum(b, 0, xl)?;
    let parameters = holtwinters_parameters(b)?;
    scalar_real(b, 10)?;
    scalar_real(b, 11)?;
    scalar_real(b, 13)?;
    parameters
        .validate(crate::library::stats::holtwinters::kernel::Lengths {
            x: xl,
            seed: b[12].len(),
            level: b[14].len(),
            trend: b[15].len(),
            season: b[16].len(),
        })
        .map_err(|error| BufferError::new(error.to_string()))
}

fn invoke(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    use crate::library::stats::holtwinters::kernel::{self, Initial, Output};
    // Recheck even a direct internal call before projecting any Rust slice.
    ROUTINE.validate_buffers(BufferInterface::C, b)?;
    let parameters = holtwinters_parameters(b)?;
    let xl = dimension(b, 1, 0)?;
    let [
        NativeBuffer::Real(x),
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        NativeBuffer::Real(a),
        NativeBuffer::Real(initial_trend),
        NativeBuffer::Real(seed),
        NativeBuffer::Real(sse),
        NativeBuffer::Real(level),
        NativeBuffer::Real(trend),
        NativeBuffer::Real(season),
    ] = b
    else {
        return Err(BufferError::new("invalid HoltWinters buffer types"));
    };
    kernel::filter(
        &x[..xl],
        parameters,
        Initial {
            level: a[0],
            trend: initial_trend[0],
            season: seed,
        },
        Output {
            sse: &mut sse[0],
            level,
            trend,
            season,
        },
    )
    .map_err(|error| BufferError::new(error.to_string()))
}
