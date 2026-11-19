use r_embed::RSession;

const GNU_EN_US: &str = r#"
    expected <- c(
        decimal_point = ".", thousands_sep = "", grouping = "",
        int_curr_symbol = "USD ", currency_symbol = "$",
        mon_decimal_point = ".", mon_thousands_sep = ",",
        mon_grouping = "\003", positive_sign = "", negative_sign = "-",
        int_frac_digits = "2", frac_digits = "2", p_cs_precedes = "1",
        p_sep_by_space = "0", n_cs_precedes = "1", n_sep_by_space = "0",
        p_sign_posn = "1", n_sign_posn = "1"
    )
    original_locale <- Sys.getlocale("LC_ALL")
    actual <- Sys.localeconv()
    stopifnot(identical(Sys.getlocale("LC_ALL"), original_locale))
    if (!identical(actual, expected)) {
        stop(paste("localeconv:", paste(names(actual), actual, sep = "=", collapse = "|")))
    }
    "GNU en_US locale profile matches"
"#;

#[test]
fn localeconv_matches_native_environment_with_c_numeric_formatting() {
    let mut native = RSession::new().unwrap();
    let native_output = native
        .eval(GNU_EN_US)
        .unwrap_or_else(|error| panic!("LC_ALL={:?}; {error}", std::env::var("LC_ALL")));
    assert_eq!(
        native_output.trim_end(),
        "[1] \"GNU en_US locale profile matches\""
    );
}
