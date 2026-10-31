//! Checked I calculation and original-owner diagnostic boundaries.
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
    work: &mut super::bessel_workspace::SegmentedWorkspace,
) -> SexpResult<f64> {
    access.require_active()?;
    // Pinned GNU has no large-order guard. Retain the existing defensive port
    // cap and its single callback; do not claim GNU cap-message fidelity.
    if x < 0.0 || order > crate::special::bessel_i::MAX_ALPHA_I {
        let value = crate::special::bessel_i::bessel_i_ex(x, order, scaling, &mut []);
        access.require_active()?;
        return Ok(value);
    }
    let result = if let Some(contiguous) = work.contiguous_mut() {
        crate::special::bessel_i::bessel_i_positive_status(x, order, scaling, contiguous)
    } else {
        crate::special::bessel_i::bessel_i_positive_status_with_workspace(x, order, scaling, work)
    }
    .ok_or_else(|| SexpError::EvaluationFailed {
        message: "invalid checked Bessel I calculation".into(),
    })?;
    if result.ncalc != result.count {
        let mut message = Text::new();
        if result.ncalc < 0 {
            write!(
                &mut message,
                "bessel_i({}): ncalc (={}) != nb (={}); alpha={}. Arg. out of range?",
                General(x),
                result.ncalc,
                result.count,
                General(result.fraction)
            )
            .map_err(|_| format_error())?;
        } else {
            write!(
                &mut message,
                "bessel_i({},nu={}): precision lost in result",
                General(x),
                General(result.fraction + f64::from(result.count) - 1.0)
            )
            .map_err(|_| format_error())?;
        }
        emit(access, &message)?;
    }
    Ok(result.value)
}
