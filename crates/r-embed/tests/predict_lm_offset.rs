use r_embed::RSession;

/// GNU R 4.6.1 `tests/reg-tests-1e.R` PR#18456. An offset-only `lm` keeps
/// the formula offset once, and `predict` with unrelated newdata reuses it.
#[test]
fn predict_lm_offset_uses_the_formula_offset_once() {
    let mut session = RSession::new().unwrap();
    let value = session
        .eval(
            r#"
            local({
              y <- rep(0, 10)
              x <- rep(c(0, 1), each = 5)
              mod <- list(lm(y ~ x), lm(y ~ offset(x)))
              p1 <- identical(predict(mod[[1]], newdata = data.frame(z = 1:10)),
                              setNames(rep(0, 10), as.character(1:10)))
              p2 <- identical(predict(mod[[2]], newdata = data.frame(z = 1:10)),
                              setNames(rep(c(-0.5, 0.5), each = 5), as.character(1:10)))
              off <- identical(mod[[2]]$offset, x)
              p1 && p2 && off
            })
            "#,
        )
        .unwrap();
    assert_eq!(value.trim(), "[1] TRUE");
}
