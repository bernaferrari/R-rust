#![forbid(unsafe_code)]
//! Copied numerical reads and original-runtime checks around scalar warnings.
use crate::sexp::{
    ffi::{NA_REAL, SEXPTYPE},
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};
use core::fmt::Write;

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Choose,
    LogChoose,
    PsiGamma,
}

pub(super) fn evaluate(
    access: &RuntimeAccess,
    a: Sexp<'static>,
    b: Sexp<'static>,
    operation: Operation,
) -> SexpResult<(Sexp<'static>, Option<Sexp<'static>>)> {
    access.require_active()?;
    let na = a.len();
    let nb = b.len();
    let length = if na == 0 || nb == 0 { 0 } else { na.max(nb) };
    let domain = access.domain();
    let result = access
        .allocator(&domain)?
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, length)))?;
    let mut output = SexpMut::try_from_checked(result)?;
    if length != 0 {
        // GNU Math2 materializes both coerced operands before scalar calls.
        // No payload view survives either provider or a warning callback.
        crate::sexp::altrep::force_materialization(&a)?;
        access.require_active()?;
        crate::sexp::altrep::force_materialization(&b)?;
        access.require_active()?;
        let mut nan = false;
        for i in 0..length {
            check_lengths(&a, &b, na, nb)?;
            let x = a.try_real_elt(i % na)?;
            access.require_active()?;
            check_lengths(&a, &b, na, nb)?;
            let k = b.try_real_elt(i % nb)?;
            access.require_active()?;
            check_lengths(&a, &b, na, nb)?;
            let value = if crate::sexp::ffi::R_IsNA(x) || crate::sexp::ffi::R_IsNA(k) {
                NA_REAL
            } else if x.is_nan() || k.is_nan() {
                f64::NAN
            } else {
                let value = match operation {
                    Operation::Choose | Operation::LogChoose => {
                        warn_rounding(k, crate::nmath::utils::r_forceint(k));
                        access.require_active()?;
                        check_lengths(&a, &b, na, nb)?;
                        match operation {
                            Operation::LogChoose => crate::special::choose::lchoose(x, k),
                            _ => crate::special::choose::choose(x, k),
                        }
                    }
                    Operation::PsiGamma => {
                        let n = crate::nmath::utils::r_forceint(k) as i32;
                        if n > crate::special::polygamma::PSIGAMMA_MAX_DERIV {
                            warn_derivative_limit(n);
                            access.require_active()?;
                            check_lengths(&a, &b, na, nb)?;
                            f64::NAN
                        } else {
                            crate::special::polygamma::psigamma(x, k)
                        }
                    }
                };
                nan |= value.is_nan();
                value
            };
            access.require_active()?;
            check_lengths(&a, &b, na, nb)?;
            output.try_set_real_elt(i, value)?;
        }
        if nan {
            crate::mainutils::errors::nmath_warning_hook("NaNs produced");
            access.require_active()?;
        }
    }
    access.require_active()?;
    check_lengths(&a, &b, na, nb)?;
    let source = if na == 0 || (length != 0 && na >= nb) {
        Some(a)
    } else if length != 0 {
        Some(b)
    } else {
        None
    };
    Ok((output.freeze(), source))
}
fn check_lengths(a: &Sexp<'_>, b: &Sexp<'_>, na: i64, nb: i64) -> SexpResult<()> {
    if a.len() != na || b.len() != nb {
        return Err(SexpError::EvaluationFailed {
            message: "mathematical input length changed during evaluation".to_owned(),
        });
    }
    Ok(())
}

// A warning is possible only when |k0| < 5e8: the rounding distance is
// at most 0.5 and the relative tolerance is 1e-9. Decimal formatting of
// these two finite numbers therefore fits this fixed stack buffer.
fn warn_rounding(original: f64, rounded: f64) {
    if !((rounded - original).abs() > 1e-9 * crate::nmath::utils::fmax2(1.0, original.abs())) {
        return;
    }
    let mut message = Message {
        bytes: [0; 128],
        len: 0,
    };
    write!(
        &mut message,
        "'k' ({original:.2}) must be integer, rounded to {rounded:.0}"
    )
    .expect("bounded choose rounding warning");
    crate::nmath::error::ml_warn_message(
        core::str::from_utf8(&message.bytes[..message.len]).expect("ASCII warning"),
    );
}

struct Message {
    bytes: [u8; 128],
    len: usize,
}
impl core::fmt::Write for Message {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(core::fmt::Error)?;
        let target = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        target.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn warn_derivative_limit(n: i32) {
    let mut message = Message {
        bytes: [0; 128],
        len: 0,
    };
    write!(
        &mut message,
        "deriv = {n} > {} (= n_max)",
        crate::special::polygamma::PSIGAMMA_MAX_DERIV
    )
    .expect("bounded derivative warning");
    crate::nmath::error::ml_warn_message(
        core::str::from_utf8(&message.bytes[..message.len]).expect("ASCII warning"),
    );
}
