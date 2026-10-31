//! Checked K calculation and original-owner diagnostic boundaries.
#![forbid(unsafe_code)]
use super::bessel_diagnostic::{General, Text, emit, format_error};
use crate::sexp::{
    object::{SexpError, SexpResult},
    owner::RuntimeAccess,
};
use core::fmt::Write;

pub(super) fn positive(
    access: &RuntimeAccess,
    x: f64,
    order: f64,
    scaling: f64,
    work: &mut [f64],
) -> SexpResult<f64> {
    access.require_active()?;
    let magnitude = order.abs();
    if x < 0.0 || magnitude > crate::special::bessel_k::MAX_ALPHA_K {
        let value = crate::special::bessel_k::bessel_k_ex(x, order, scaling, work);
        access.require_active()?;
        return Ok(value);
    }
    let result = crate::special::bessel_k::bessel_k_positive_status(x, magnitude, scaling, work)
        .ok_or_else(|| SexpError::EvaluationFailed {
            message: "invalid checked Bessel K calculation".into(),
        })?;
    if result.ncalc != result.count {
        let mut message = Text::new();
        if result.ncalc < 0 {
            write!(
                &mut message,
                "bessel_k({}): ncalc (={}) != nb (={}); alpha={}. Arg. out of range?",
                General(x),
                result.ncalc,
                result.count,
                General(result.fraction)
            )
            .map_err(|_| format_error())?;
        } else {
            write!(
                &mut message,
                "bessel_k({},nu={}): precision lost in result",
                General(x),
                General(result.fraction + f64::from(result.count) - 1.0)
            )
            .map_err(|_| format_error())?;
        }
        emit(access, &message)?;
    }
    Ok(result.value)
}
