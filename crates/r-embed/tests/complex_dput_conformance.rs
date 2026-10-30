use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn complex_missing_deparse_matches_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let cases = [
            "identical(deparse(c(1+2i,NA_complex_,3-4i)), 'c(1+2i, NA, 3-4i)')",
            "identical(deparse(c(NA_complex_,NA_complex_)), 'c(NA_complex_, NA_complex_)')",
            "identical(deparse(NA_complex_), 'NA_complex_')",
            "identical(deparse(c(complex(real=NaN,imaginary=NaN),NA_complex_)), 'c(complex(real=NaN, imaginary=NaN), NA)')",
            "identical(deparse(c(complex(real=NA_real_,imaginary=2),NA_complex_)), 'c(complex(real=NA, imaginary=2), NA_complex_)')",
            "identical(deparse(c(1+2i,NA_complex_),control=character()), 'c(1+2i, NA)')",
            "identical(capture.output(dput(c(1+2i,NA_complex_,3-4i))), 'c(1+2i, NA, 3-4i)')",
            "identical(deparse(setNames(c(1+2i,NA_complex_),c('á x','β y'))), 'c(\"á x\" = 1+2i, \"β y\" = NA)')",
            "identical(deparse(c(NaN+0i,NA_complex_)), 'c(complex(real=NaN, imaginary=0), NA)')",
            "identical(deparse(c(1+2i,NA_complex_),control='S_compat'), 'c(1+2i, NA)')",
        ];
        for expression in cases {
            let value = session
                .eval(expression)
                .unwrap_or_else(|e| format!("ERROR: {e}"));
            assert_eq!(value, "[1] TRUE\n", "portable={portable}; {expression}");
        }
    }
}
