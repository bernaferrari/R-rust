//! Checked J calculation and original-owner diagnostic boundaries.
#![forbid(unsafe_code)]
use super::bessel_diagnostic::{General, Text, emit, format_error};
use crate::sexp::{
    object::{SexpError, SexpResult},
    owner::RuntimeAccess,
};
use core::fmt::Write;
/// Pinned GNU's public/preallocated J cap; shared legacy scalar cap is separate.
pub(super) const MAX_ORDER: f64 = 1e7;
pub(super) fn positive(
    access: &RuntimeAccess,
    x: f64,
    order: f64,
    work: &mut [f64],
) -> SexpResult<f64> {
    access.require_active()?;
    if x < 0.0 {
        let value = crate::special::bessel_j::bessel_j_ex(x, order, work);
        access.require_active()?;
        return Ok(value);
    }
    if order > MAX_ORDER {
        let mut message = Text::new();
        write!(
            &mut message,
            "besselJ(x, nu): nu={} > 1e7; too large for bessel_j() algorithm",
            General(order)
        )
        .map_err(|_| format_error())?;
        emit(access, &message)?;
        return Ok(f64::NAN);
    }
    let result =
        crate::special::bessel_j::bessel_j_positive_status(x, order, work).ok_or_else(|| {
            SexpError::EvaluationFailed {
                message: "invalid checked Bessel J calculation".into(),
            }
        })?;
    if result.large_x {
        // Preserve J_bessel's existing range text; this bridge can invoke handlers.
        crate::nmath::error::ml_warning(crate::nmath::constants::ME_RANGE, "J_bessel");
        access.require_active()?;
    } else if result.ncalc != result.count {
        let mut message = Text::new();
        if result.ncalc < 0 {
            write!(
                &mut message,
                "bessel_j({}): ncalc (={}) != nb (={}); alpha={}. Arg. out of range?",
                General(x),
                result.ncalc,
                result.count,
                General(result.fraction)
            )
            .map_err(|_| format_error())?;
        } else {
            write!(
                &mut message,
                "bessel_j({},nu={}): precision lost in result",
                General(x),
                General(result.fraction + f64::from(result.count) - 1.0)
            )
            .map_err(|_| format_error())?;
        }
        emit(access, &message)?;
    }
    Ok(result.value)
}
