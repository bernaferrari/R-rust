use r_embed::RSession;

#[test]
fn function_body_accepts_r_power_and_unary_precedence() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval(
            r#"
            f <- function(x) exp(-x^2)
            stopifnot(all.equal(f(2), exp(-4)))
            stopifnot(2^-2 == 0.25)
            stopifnot(-2^2 == -4)
            stopifnot(--2 == 2, -+2 == -2, +-2 == -2)
            stopifnot(!-2^2 == FALSE)
            x <- c(2, 3)
            stopifnot(-x[1]^2 == -4)
            f(2)
            "#,
        )
        .expect("unary exponent expression");
    assert!(
        result.contains("0.01831564"),
        "unexpected f(2) result: {result}"
    );
}
