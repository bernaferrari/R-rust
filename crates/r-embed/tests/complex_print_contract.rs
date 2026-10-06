use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn complex_public_printing_matches_gnu_under_both_package_policies() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        for (name, source, expected) in [
            (
                "original complex FFT cat",
                include_str!("../../../tests/conformance/cases/1094_cat_complex_fft.R"),
                include_str!("fixtures/complex-original-1094_cat_complex_fft.out"),
            ),
            (
                "original constructor, format, str",
                include_str!(
                    "../../../tests/conformance/cases/544_complex_constructor_format_str_parity.R"
                ),
                include_str!(
                    "fixtures/complex-original-544_complex_constructor_format_str_parity.out"
                ),
            ),
            (
                "original round and signif",
                include_str!("../../../tests/conformance/cases/551_round_signif_complex_parity.R"),
                include_str!("fixtures/complex-original-551_round_signif_complex_parity.out"),
            ),
            (
                "missing values and scalar formatting controls",
                include_str!("fixtures/complex-print-public-contract.R"),
                include_str!("fixtures/complex-print-public-contract.out"),
            ),
        ] {
            assert_eq!(
                session
                    .eval(source)
                    .unwrap_or_else(|error| panic!("{name}, portable={portable}: {error}")),
                expected,
                "{name}, portable={portable}"
            );
            assert_eq!(session.eval("1 + 1").unwrap(), "[1] 2\n");
        }
    }
}
