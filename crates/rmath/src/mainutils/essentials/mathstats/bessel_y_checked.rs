//! Checked Y diagnostics; shared formatting is bounded stack storage.
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
    work: &mut [f64],
) -> SexpResult<f64> {
    access.require_active()?;
    if x < 0.0 {
        let value = crate::special::bessel_y::bessel_y_ex(x, order, work);
        access.require_active()?;
        return Ok(value);
    }
    if order > crate::special::bessel_y::MAX_ALPHA_Y {
        let mut message = Text::new();
        write!(
            &mut message,
            "besselY(x, nu): nu={} too large for bessel_y() algorithm",
            General(order)
        )
        .map_err(|_| format_error())?;
        emit(access, &message)?;
        return Ok(f64::NAN);
    }
    let result =
        crate::special::bessel_y::bessel_y_positive_status(x, order, work).ok_or_else(|| {
            SexpError::EvaluationFailed {
                message: "invalid checked Bessel Y calculation".into(),
            }
        })?;
    if result.ncalc != result.count && result.ncalc != -1 {
        let mut message = Text::new();
        if result.ncalc < -1 {
            write!(
                &mut message,
                "bessel_y({}): ncalc (={}) != nb (={}); alpha={}. Arg. out of range?",
                General(x),
                result.ncalc,
                result.count,
                General(result.fraction)
            )
            .map_err(|_| format_error())?;
        } else {
            write!(
                &mut message,
                "bessel_y({},nu={}): precision lost in result",
                General(x),
                General(result.fraction + f64::from(result.count) - 1.0)
            )
            .map_err(|_| format_error())?;
        }
        emit(access, &message)?;
    }
    Ok(result.value)
}
