use r_embed::RSession;

#[test]
fn formatc_fg_rounds_significant_digits_without_spurious_decimal_points() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r##"
                stopifnot(
                    identical(formatC(99.9, 1, format = "fg"), "100"),
                    identical(formatC(99.9, 2, format = "fg"), "100"),
                    identical(formatC(99.9, 3, format = "fg"), "99.9"),
                    identical(formatC(9.99, 1, format = "fg"), "1e+01"),
                    identical(formatC(9.99, 2, format = "fg"), " 10"),
                    identical(formatC(999, 1, format = "fg"), "1000"),
                    identical(formatC(0.999, 1, format = "fg"), " 1"),
                    identical(formatC(0.999, 2, format = "fg"), "  1"),
                    identical(formatC(0.000001234, 1, format = "fg"), "0.000001"),
                    identical(formatC(123.456, 2, format = "fg", flag = "#"), "123."),
                    identical(formatC(99.9, 2, format = "fg", flag = "#"), "100.")
                )
                cat("formatc-fg-rounding: PASS\n")
                "##
            )
            .unwrap(),
        "formatc-fg-rounding: PASS\n"
    );
}
