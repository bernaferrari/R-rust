//! Checked GNU multivariate Burg buffers; no raw projections.
#![forbid(unsafe_code)]
use super::BufferType::{Integer, Real};
use super::{
    BufferError, BufferInterface, BufferRoutine, NativeBuffer, dimension, minimum, scalar,
};
use crate::library::stats::mar::multi_burg::{self, Parameters};

pub(crate) const ROUTINE: BufferRoutine = BufferRoutine::owned(
    "stats",
    BufferInterface::C,
    &[
        Integer, Real, Integer, Integer, Real, Real, Real, Real, Integer, Integer, Integer,
    ],
    shape,
    invoke,
);
fn parameters(b: &[NativeBuffer]) -> Result<Parameters, BufferError> {
    let max_order = dimension(b, 2, 0)?;
    // GNU burg0 never reads vmethod when no partial correlation is fitted.
    let method = if max_order > 0 { scalar(b, 10)? } else { 0 };
    Ok(Parameters {
        n: dimension(b, 0, 1)?,
        max_order,
        series: dimension(b, 3, 1)?,
        use_aic: scalar(b, 9)? != 0,
        method,
    })
}
fn shape(b: &[NativeBuffer]) -> Result<(), BufferError> {
    let (data, cube, lags) = parameters(b)?
        .lengths()
        .map_err(|e| BufferError::new(e.to_string()))?;
    minimum(b, 1, data)?;
    for index in [4, 5, 6] {
        minimum(b, index, cube)?;
    }
    minimum(b, 7, lags)?;
    minimum(b, 8, 1)
}
fn invoke(b: &mut [NativeBuffer]) -> Result<(), BufferError> {
    ROUTINE.validate_buffers(BufferInterface::C, b)?;
    let p = parameters(b)?;
    let [
        _,
        NativeBuffer::Real(x),
        _,
        _,
        NativeBuffer::Real(coef),
        NativeBuffer::Real(pacf),
        NativeBuffer::Real(var),
        NativeBuffer::Real(aic),
        NativeBuffer::Integer(order),
        _,
        _,
    ] = b
    else {
        return Err(BufferError::new("invalid multi_burg buffer types"));
    };
    let output = multi_burg::fit(p, x, pacf).map_err(|e| BufferError::new(e.to_string()))?;
    x[..output.residuals.len()].copy_from_slice(&output.residuals);
    coef[..output.coefficients.len()].copy_from_slice(&output.coefficients);
    pacf[..output.partial.len()].copy_from_slice(&output.partial);
    var[..output.variance.len()].copy_from_slice(&output.variance);
    aic[..output.aic.len()].copy_from_slice(&output.aic);
    order[0] = output.order;
    Ok(())
}
