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

fn plan(
    b: &[NativeBuffer],
) -> Result<
    (
        crate::library::stats::holtwinters::kernel::Parameters,
        usize,
        usize,
    ),
    BufferError,
> {
    use crate::library::stats::holtwinters::kernel::Parameters;
    let xl = dimension(b, 1, 0)?;
    let start_time = dimension(b, 5, 1)?;
    let trend = scalar(b, 8)? == 1;
    let seasonal = scalar(b, 9)? == 1;
    let steps = xl.saturating_sub(start_time - 1);
    let parameters = Parameters {
        alpha: if steps > 0 { scalar_real(b, 2)? } else { 0. },
        beta: if steps > 0 && trend {
            scalar_real(b, 3)?
        } else {
            0.
        },
        gamma: if steps > 0 && seasonal {
            scalar_real(b, 4)?
        } else {
            0.
        },
        start_time,
        additive: if steps > 0 { scalar(b, 6)? == 1 } else { false },
        period: if steps > 0 || seasonal {
            dimension(b, 7, 0)?
        } else {
            0
        },
        trend,
        seasonal,
    };
    Ok((parameters, xl, steps))
}
fn shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let (parameters, xl, steps) = plan(b)?;
    if steps > 0 {
        minimum(b, 0, xl)?;
    }
    scalar_real(b, 10)?;
    if parameters.trend {
        scalar_real(b, 11)?;
    }
    parameters
        .validate(crate::library::stats::holtwinters::kernel::Lengths {
            x: xl,
            sse: b[13].len(),
            seed: b[12].len(),
            level: b[14].len(),
            trend: b[15].len(),
            season: b[16].len(),
        })
        .map_err(|e| BufferError::new(e.to_string()))
}

fn invoke(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    use crate::library::stats::holtwinters::kernel::{self, Initial, Output};
    // Recheck even a direct internal call before projecting any Rust slice.
    ROUTINE.validate_buffers(BufferInterface::C, b)?;
    let (parameters, xl, steps) = plan(b)?;
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
        if steps > 0 { &x[..xl] } else { &[] },
        parameters,
        Initial {
            level: a[0],
            trend: if parameters.trend {
                initial_trend[0]
            } else {
                0.
            },
            season: seed,
        },
        Output {
            sse,
            level,
            trend,
            season,
        },
    )
    .map_err(|error| BufferError::new(error.to_string()))
}
