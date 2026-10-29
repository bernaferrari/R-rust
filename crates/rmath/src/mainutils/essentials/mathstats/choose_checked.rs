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
    BesselJ,
    BesselY,
    BesselK,
    BesselI,
}

pub(super) fn evaluate(
    access: &RuntimeAccess,
    a: Sexp<'static>,
    b: Sexp<'static>,
    c: Option<Sexp<'static>>,
    operation: Operation,
) -> SexpResult<(Sexp<'static>, Option<Sexp<'static>>)> {
    access.require_active()?;
    let na = a.len();
    let nb = b.len();
    let nc = c.as_ref().map_or(1, |c| c.len());
    let check = || {
        check_lengths(&a, &b, na, nb)?;
        if c.as_ref().is_some_and(|c| c.len() != nc) {
            return Err(SexpError::EvaluationFailed {
                message: "mathematical input length changed during evaluation".into(),
            });
        }
        Ok(())
    };
    let length = if na == 0 || nb == 0 || nc == 0 {
        0
    } else {
        na.max(nb).max(nc)
    };
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
        if let Some(c) = &c {
            crate::sexp::altrep::force_materialization(c)?;
            access.require_active()?;
        }
        let mut nan = false;
        for i in 0..length {
            check()?;
            let x = a.try_real_elt(i % na)?;
            access.require_active()?;
            check()?;
            let k = b.try_real_elt(i % nb)?;
            access.require_active()?;
            check()?;
            let scaling = if let Some(c) = &c {
                let value = c.try_real_elt(i % nc)?;
                access.require_active()?;
                check()?;
                value
            } else {
                0.0
            };
            let value = if crate::sexp::ffi::R_IsNA(x)
                || crate::sexp::ffi::R_IsNA(k)
                || crate::sexp::ffi::R_IsNA(scaling)
            {
                NA_REAL
            } else if x.is_nan() || k.is_nan() || scaling.is_nan() {
                f64::NAN
            } else {
                let value = match operation {
                    Operation::Choose | Operation::LogChoose => {
                        warn_rounding(k, crate::nmath::utils::r_forceint(k));
                        access.require_active()?;
                        check()?;
                        match operation {
                            Operation::LogChoose => crate::special::choose::lchoose(x, k),
                            _ => crate::special::choose::choose(x, k),
                        }
                    }
                    Operation::BesselJ
                    | Operation::BesselY
                    | Operation::BesselK
                    | Operation::BesselI => {
                        // Oversized orders and negative x return before the
                        // kernel reads its array. Other orders need floor(|nu|)+1.
                        let magnitude = k.abs();
                        // Negative fractional Y reflects through J as well.
                        // Integer Y orders only require Y's smaller limit.
                        let limit = match operation {
                            Operation::BesselK => crate::special::bessel_k::MAX_ALPHA_K,
                            Operation::BesselI if k < 0.0 && k != k.floor() => {
                                crate::special::bessel_i::MAX_ALPHA_I
                                    .max(crate::special::bessel_k::MAX_ALPHA_K)
                            }
                            Operation::BesselI => crate::special::bessel_i::MAX_ALPHA_I,
                            Operation::BesselY if k >= 0.0 || k == k.floor() => {
                                crate::special::bessel_y::MAX_ALPHA_Y
                            }
                            _ => super::bessel_j_checked::MAX_ORDER,
                        };
                        let length = if x < 0.0 || magnitude > limit {
                            0
                        } else {
                            (magnitude.floor() as usize).checked_add(1).ok_or_else(|| {
                                SexpError::EvaluationFailed {
                                    message: "Bessel workspace dimension overflow".into(),
                                }
                            })?
                        };
                        let mut workspace =
                            super::bessel_workspace::Workspace::new(access, length)?;
                        let value = match operation {
                            // These branches return before scalar allocation;
                            // retain existing shared range-warning text.
                            Operation::BesselJ if x >= 0.0 && k < 0.0 => {
                                // Keep the half-integer shortcut and J-then-Y
                                // arithmetic order, checking before reflection.
                                let first = if k - k.floor() == 0.5 {
                                    0.0
                                } else {
                                    let value = super::bessel_j_checked::positive(
                                        access,
                                        x,
                                        -k,
                                        workspace.values(),
                                    )?;
                                    access.require_active()?;
                                    check()?;
                                    value * crate::special::cospi::cospi(k)
                                };
                                let reflected = if k == k.floor() {
                                    0.0
                                } else {
                                    super::bessel_y_checked::positive(
                                        access,
                                        x,
                                        -k,
                                        workspace.values(),
                                    )? * crate::special::cospi::sinpi(k)
                                };
                                access.require_active()?;
                                check()?;
                                first + reflected
                            }
                            Operation::BesselI if x >= 0.0 && k < 0.0 && k != k.floor() => {
                                // Preserve I-then-K reflection and arithmetic order,
                                // checking authority before a second kernel can warn.
                                let first = crate::special::bessel_i::bessel_i_ex(
                                    x,
                                    -k,
                                    scaling,
                                    workspace.values(),
                                );
                                access.require_active()?;
                                check()?;
                                let reflected = crate::special::bessel_k::bessel_k_ex(
                                    x,
                                    -k,
                                    scaling,
                                    workspace.values(),
                                );
                                access.require_active()?;
                                check()?;
                                first
                                    + reflected
                                        * if scaling as i32 == 1 {
                                            2.0
                                        } else {
                                            2.0 * libm::exp(-2.0 * x)
                                        }
                                        / std::f64::consts::PI
                                        * crate::special::cospi::sinpi(-k)
                            }
                            Operation::BesselI => crate::special::bessel_i::bessel_i_ex(
                                x,
                                k,
                                scaling,
                                workspace.values(),
                            ),
                            Operation::BesselK => crate::special::bessel_k::bessel_k_ex(
                                x,
                                k,
                                scaling,
                                workspace.values(),
                            ),
                            Operation::BesselY if x >= 0.0 && k < 0.0 => {
                                let first = if k - k.floor() == 0.5 {
                                    0.0
                                } else {
                                    let value = super::bessel_y_checked::positive(
                                        access,
                                        x,
                                        -k,
                                        workspace.values(),
                                    )?;
                                    access.require_active()?;
                                    check()?;
                                    value * crate::special::cospi::cospi(k)
                                };
                                let reflected = if k == k.floor() {
                                    0.0
                                } else {
                                    let value = super::bessel_j_checked::positive(
                                        access,
                                        x,
                                        -k,
                                        workspace.values(),
                                    )?;
                                    access.require_active()?;
                                    check()?;
                                    value * crate::special::cospi::sinpi(k)
                                };
                                first - reflected
                            }
                            Operation::BesselY => {
                                super::bessel_y_checked::positive(access, x, k, workspace.values())?
                            }
                            _ => {
                                super::bessel_j_checked::positive(access, x, k, workspace.values())?
                            }
                        };
                        access.require_active()?;
                        check()?;
                        value
                    }
                    Operation::PsiGamma => {
                        let n = crate::nmath::utils::r_forceint(k) as i32;
                        if n > crate::special::polygamma::PSIGAMMA_MAX_DERIV {
                            warn_derivative_limit(n);
                            access.require_active()?;
                            check()?;
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
            check()?;
            output.try_set_real_elt(i, value)?;
        }
        if nan {
            crate::mainutils::errors::nmath_warning_hook("NaNs produced");
            access.require_active()?;
        }
    }
    access.require_active()?;
    check()?;
    let source = if na == 0 || (length != 0 && na == length) {
        Some(a)
    } else if length != 0 && nb == length {
        Some(b)
    } else if length != 0 {
        c
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
