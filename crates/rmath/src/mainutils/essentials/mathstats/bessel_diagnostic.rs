//! Private checked Bessel diagnostics; formatting uses bounded stack storage.
#![forbid(unsafe_code)]
pub(super) use crate::nmath::diagnostics::{General, Text};
use crate::sexp::{
    object::{SexpError, SexpResult},
    owner::RuntimeAccess,
};
#[cfg(test)]
use core::fmt::Write;
pub(super) fn emit(access: &RuntimeAccess, message: &Text<256>) -> SexpResult<()> {
    crate::nmath::error::ml_warn_message(message.text());
    access.require_active()
}
pub(super) fn format_error() -> SexpError {
    SexpError::EvaluationFailed {
        message: "Bessel diagnostic formatting failed".into(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bessely_diagnostic_general_matches_pinned_c_format() {
        for (value, expected) in [
            (0.0, "0"),
            (-0.0, "-0"),
            (1e-300, "1e-300"),
            (1e-8, "1e-08"),
            (0.00123456789, "0.00123457"),
            (12.3456789, "12.3457"),
            (1e7 + 1.0, "1e+07"),
            (1.23456789e8, "1.23457e+08"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
            (f64::NAN, "nan"),
            (0.0001, "0.0001"),
            (0.0000999999, "9.99999e-05"),
            (999999.0, "999999"),
            (1000000.0, "1e+06"),
            (9.999999e-5, "0.0001"),
            (999999.9, "1e+06"),
            (9.999995e-5, "0.0001"),
            (0.001234565, "0.00123456"),
            (0.001234575, "0.00123457"),
            (10000050.0, "1e+07"),
            (10000150.0, "1.00002e+07"),
        ] {
            let mut text = Text::<32>::new();
            write!(&mut text, "{}", General(value)).unwrap();
            assert_eq!(text.text(), expected);
        }
    }
}
