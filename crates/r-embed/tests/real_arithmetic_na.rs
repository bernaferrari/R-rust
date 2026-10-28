use r_embed::RSession;

#[test]
fn vector_real_arithmetic_preserves_operation_specific_na_payloads() {
    let mut session = RSession::new().unwrap();
    session.eval("bits <- function(x) writeBin(as.double(x), raw(), endian='little'); quiet <- bits(NA_real_ + 0); direct <- bits(NA_real_)").unwrap();
    for op in ["+", "-", "*", "/", "^"] {
        for (x, y) in [
            ("c(NA_real_, 2)", "1"),
            ("c(NA_integer_, 2L)", "1"),
            ("c(NaN, 2)", "NA_real_"),
            ("c(NA_real_, 2)", "NaN"),
        ] {
            let code = format!("identical(bits(({x} {op} {y})[1]), quiet)");
            assert_eq!(session.eval(&code).unwrap().trim(), "[1] TRUE", "{code}");
        }
    }
    for code in [
        "identical(bits(c(0, 2)^NA_real_)[1:8], direct)",
        "identical(c(NA_real_, NaN)^0, c(1, 1))",
        "identical(c(1, 1)^c(NA_real_, NaN), c(1, 1))",
        "identical(bits((c(NA_integer_, 2L)/1L)[1]), direct)",
        "identical(bits((c(NA_integer_, 2L)^2L)[1]), direct)",
        "identical(c(NA_integer_, 2L)+1L, c(NA_integer_, 3L))",
        "identical(bits(NA_real_), direct)",
        "f <- lm(c(1,NA,3,NaN,5) ~ 1, na.action=na.exclude); y <- covratio(f); identical(bits(y[2]), quiet) && identical(bits(y[4]), quiet)",
        "identical(names(setNames(c(NA_real_, 2), c('a','b'))/1), c('a','b'))",
        "identical(bits((setNames(c(NA_real_, 2), c('a','b'))/1)[1]), quiet)",
        "identical(dim(matrix(c(NA_real_, 2), 1)/1), c(1L, 2L))",
    ] {
        assert_eq!(session.eval(code).unwrap().trim(), "[1] TRUE", "{code}");
    }
}
