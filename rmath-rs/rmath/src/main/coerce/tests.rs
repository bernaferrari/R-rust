#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

#[cfg(test)]
mod tests {
    use super::super::*;
    use std::ffi::CString;

    #[test]
    fn test_logical_from_integer() {
        assert_eq!(unsafe { LogicalFromInteger(0, std::ptr::null_mut()) }, 0);
        assert_eq!(unsafe { LogicalFromInteger(1, std::ptr::null_mut()) }, 1);
        assert_eq!(unsafe { LogicalFromInteger(42, std::ptr::null_mut()) }, 1);
        assert_eq!(unsafe { LogicalFromInteger(-1, std::ptr::null_mut()) }, 1);
        assert_eq!(
            unsafe { LogicalFromInteger(NA_INTEGER, std::ptr::null_mut()) },
            NA_LOGICAL
        );
    }

    #[test]
    fn test_logical_from_real() {
        assert_eq!(unsafe { LogicalFromReal(0.0, std::ptr::null_mut()) }, 0);
        assert_eq!(unsafe { LogicalFromReal(1.0, std::ptr::null_mut()) }, 1);
        assert_eq!(unsafe { LogicalFromReal(-0.5, std::ptr::null_mut()) }, 1);
        assert_eq!(
            unsafe { LogicalFromReal(f64::NAN, std::ptr::null_mut()) },
            NA_LOGICAL
        );
    }

    #[test]
    fn test_logical_from_complex() {
        assert_eq!(
            unsafe { LogicalFromComplex(Rcomplex { r: 0.0, i: 0.0 }, std::ptr::null_mut()) },
            0
        );
        assert_eq!(
            unsafe { LogicalFromComplex(Rcomplex { r: 1.0, i: 0.0 }, std::ptr::null_mut()) },
            1
        );
        assert_eq!(
            unsafe { LogicalFromComplex(Rcomplex { r: 0.0, i: 1.0 }, std::ptr::null_mut()) },
            1
        );
        assert_eq!(
            unsafe {
                LogicalFromComplex(
                    Rcomplex {
                        r: f64::NAN,
                        i: 0.0,
                    },
                    std::ptr::null_mut(),
                )
            },
            NA_LOGICAL
        );
    }

    #[test]
    fn test_integer_from_logical() {
        assert_eq!(unsafe { IntegerFromLogical(0, std::ptr::null_mut()) }, 0);
        assert_eq!(unsafe { IntegerFromLogical(1, std::ptr::null_mut()) }, 1);
        assert_eq!(
            unsafe { IntegerFromLogical(NA_LOGICAL, std::ptr::null_mut()) },
            NA_INTEGER
        );
    }

    #[test]
    fn test_integer_from_real() {
        assert_eq!(unsafe { IntegerFromReal(3.7, std::ptr::null_mut()) }, 3);
        assert_eq!(unsafe { IntegerFromReal(-2.1, std::ptr::null_mut()) }, -2);
        assert_eq!(
            unsafe { IntegerFromReal(f64::NAN, std::ptr::null_mut()) },
            NA_INTEGER
        );

        let mut warn: c_int = 0;
        let result = unsafe { IntegerFromReal(1e20, &mut warn) };
        assert_eq!(result, NA_INTEGER);
        assert!(warn & WARN_INT_NA != 0);
    }

    #[test]
    fn test_integer_from_complex() {
        let mut warn: c_int = 0;
        let result = unsafe { IntegerFromComplex(Rcomplex { r: 3.0, i: 2.0 }, &mut warn) };
        assert_eq!(result, 3);
        assert!(warn & WARN_IMAG != 0);
    }

    #[test]
    fn test_real_from_logical() {
        assert_eq!(unsafe { RealFromLogical(0, std::ptr::null_mut()) }, 0.0);
        assert_eq!(unsafe { RealFromLogical(1, std::ptr::null_mut()) }, 1.0);
        let result = unsafe { RealFromLogical(NA_LOGICAL, std::ptr::null_mut()) };
        assert!(result.is_nan());
    }

    #[test]
    fn test_real_from_integer() {
        assert_eq!(unsafe { RealFromInteger(42, std::ptr::null_mut()) }, 42.0);
        let result = unsafe { RealFromInteger(NA_INTEGER, std::ptr::null_mut()) };
        assert!(result.is_nan());
    }

    #[test]
    fn test_complex_from_logical() {
        let z = unsafe { ComplexFromLogical(1, std::ptr::null_mut()) };
        assert_eq!(z.r, 1.0);
        assert_eq!(z.i, 0.0);

        let z_na = unsafe { ComplexFromLogical(NA_LOGICAL, std::ptr::null_mut()) };
        assert!(z_na.r.is_nan());
    }

    #[test]
    fn test_complex_from_integer() {
        let z = unsafe { ComplexFromInteger(42, std::ptr::null_mut()) };
        assert_eq!(z.r, 42.0);
        assert_eq!(z.i, 0.0);
    }

    #[test]
    fn test_complex_from_real() {
        let z = unsafe { ComplexFromReal(3.14, std::ptr::null_mut()) };
        assert_eq!(z.r, 3.14);
        assert_eq!(z.i, 0.0);

        // R's specific NA -> both parts NA
        let z_na = unsafe { ComplexFromReal(R_NA_REAL(), std::ptr::null_mut()) };
        assert!(z_na.r.is_nan());
        assert!(z_na.i.is_nan());
    }

    #[test]
    fn test_complex_from_string_c() {
        let s = CString::new("3+2i").unwrap();
        let z = unsafe { ComplexFromStringC(s.as_ptr(), std::ptr::null_mut()) };
        assert_eq!(z.r, 3.0);
        assert_eq!(z.i, 2.0);

        let s2 = CString::new("5i").unwrap();
        let z2 = unsafe { ComplexFromStringC(s2.as_ptr(), std::ptr::null_mut()) };
        assert_eq!(z2.r, 0.0);
        assert_eq!(z2.i, 5.0);

        let s3 = CString::new("3-4i").unwrap();
        let z3 = unsafe { ComplexFromStringC(s3.as_ptr(), std::ptr::null_mut()) };
        assert_eq!(z3.r, 3.0);
        assert_eq!(z3.i, -4.0);

        let s4 = CString::new("42").unwrap();
        let z4 = unsafe { ComplexFromStringC(s4.as_ptr(), std::ptr::null_mut()) };
        assert_eq!(z4.r, 42.0);
        assert_eq!(z4.i, 0.0);
    }

    // New tests for SEXP-based conversions

    #[test]
    fn test_logical_from_string() {
        // Test with null (no CHARSXP available in test without init)
        let result = unsafe { LogicalFromString(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert_eq!(result, NA_LOGICAL);
    }

    #[test]
    fn test_string_from_logical() {
        let s = unsafe { StringFromLogical(0) };
        assert!(!s.is_null());

        let s_true = unsafe { StringFromLogical(1) };
        assert!(!s_true.is_null());

        let s_na = unsafe { StringFromLogical(NA_LOGICAL) };
        assert!(!s_na.is_null());
    }

    #[test]
    fn test_string_from_integer() {
        let s = unsafe { StringFromInteger(42, std::ptr::null_mut()) };
        assert!(!s.is_null());

        let s_na = unsafe { StringFromInteger(NA_INTEGER, std::ptr::null_mut()) };
        assert!(!s_na.is_null());
    }

    #[test]
    fn test_string_from_raw() {
        let s = unsafe { StringFromRaw(255, std::ptr::null_mut()) };
        assert!(!s.is_null());

        let s0 = unsafe { StringFromRaw(0, std::ptr::null_mut()) };
        assert!(!s0.is_null());
    }

    #[test]
    fn test_string_from_complex() {
        let z = Rcomplex { r: 3.0, i: 4.0 };
        let s = unsafe { StringFromComplex(z, std::ptr::null_mut()) };
        assert!(!s.is_null());

        let z_na = Rcomplex {
            r: R_NA_REAL(),
            i: 0.0,
        };
        let s_na = unsafe { StringFromComplex(z_na, std::ptr::null_mut()) };
        assert!(!s_na.is_null());
    }

    #[test]
    fn test_string_from_real() {
        let s = unsafe { StringFromReal_impl(3.14, std::ptr::null_mut()) };
        assert!(!s.is_null());

        let s_na = unsafe { StringFromReal_impl(R_NA_REAL(), std::ptr::null_mut()) };
        assert!(!s_na.is_null());
    }

    #[test]
    fn test_warn_constants() {
        // Verify warning constants match R's C defines
        assert_eq!(WARN_NA, 1);
        assert_eq!(WARN_INT_NA, 2);
        assert_eq!(WARN_IMAG, 4);
        assert_eq!(WARN_RAW, 8);
    }

    #[test]
    fn test_coercion_warning_flags() {
        let mut warn: c_int = 0;
        unsafe { IntegerFromReal(1e20, &mut warn) };
        assert_ne!(warn & WARN_INT_NA, 0);

        warn = 0;
        let mut warn2: c_int = 0;
        unsafe { IntegerFromComplex(Rcomplex { r: 3.0, i: 2.0 }, &mut warn2) };
        assert_ne!(warn2 & WARN_IMAG, 0);
    }

    #[test]
    fn test_r_isna() {
        assert!(R_IsNA(R_NA_REAL()));
        assert!(!R_IsNA(f64::NAN)); // regular NaN is NOT R's NA
        assert!(!R_IsNA(0.0));
        assert!(!R_IsNA(1.0));
    }

    #[test]
    fn test_r_isnan() {
        assert!(!R_IsNaN(R_NA_REAL())); // R's NA is NOT a "pure" NaN
        assert!(R_IsNaN(f64::NAN)); // regular NaN IS a pure NaN
        assert!(!R_IsNaN(0.0));
        assert!(!R_IsNaN(f64::INFINITY));
    }

    #[test]
    fn test_r_finite() {
        assert!(R_FINITE(0.0));
        assert!(R_FINITE(1.0));
        assert!(R_FINITE(-1.0));
        assert!(!R_FINITE(f64::INFINITY));
        assert!(!R_FINITE(f64::NEG_INFINITY));
        assert!(!R_FINITE(f64::NAN));
        assert!(!R_FINITE(R_NA_REAL()));
    }

    #[test]
    fn test_integer_from_string() {
        // Test with null (no CHARSXP available in test)
        let result = unsafe { IntegerFromString(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert_eq!(result, NA_INTEGER);
    }

    #[test]
    fn test_real_from_string() {
        let result = unsafe { RealFromString(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert!(result.is_nan());
    }

    #[test]
    fn test_complex_from_string() {
        let z = unsafe { ComplexFromString(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert!(z.r.is_nan());
        assert!(z.i.is_nan());
    }

    #[test]
    fn test_as_logical_null() {
        let result = unsafe { asLogical(std::ptr::null_mut()) };
        assert_eq!(result, NA_LOGICAL);
    }

    #[test]
    fn test_as_integer_null() {
        let result = unsafe { asInteger(std::ptr::null_mut()) };
        assert_eq!(result, NA_INTEGER);
    }

    #[test]
    fn test_as_real_null() {
        let result = unsafe { asReal(std::ptr::null_mut()) };
        assert!(result.is_nan());
    }

    #[test]
    fn test_as_complex_null() {
        let z = unsafe { asComplex(std::ptr::null_mut()) };
        assert!(z.r.is_nan());
        assert!(z.i.is_nan());
    }

    #[test]
    fn test_coerce_vector_same_type() {
        // coerceVector should return the same pointer if types match
        // We can't easily create real SEXP objects in tests without init,
        // but we can test the null case
        let result = unsafe { coerceVector(std::ptr::null_mut(), SEXPTYPE::LGLSXP.0) };
        assert!(result.is_null());
    }
}
