use r_embed::{RSession, RuntimePathPolicy};

// Oracle: GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
x <- cbind(a=c(1, 2, NA, 4), b=c(10, NA, 30, 40))
z <- cov(x)
stopifnot(all(is.na(z)), identical(dimnames(z), list(c("a", "b"), c("a", "b"))))
z <- cov(x, use="complete.obs")
stopifnot(isTRUE(all.equal(unname(z), matrix(c(4.5, 45, 45, 450), 2))))
z <- cov(x, use="pairwise.complete.obs")
stopifnot(isTRUE(all.equal(unname(z), matrix(c(7/3, 45, 45, 700/3), 2))))
stopifnot(identical(cov(matrix(1:3, 3, 1)), matrix(1, 1, 1)))
msg <- tryCatch(cov(1:3, 1:2), error=conditionMessage)
stopifnot(identical(msg, "incompatible dimensions"))
stopifnot(identical(cov(1:3, 1:3, method="kendall"), 6),
          identical(cov(c(1, 1, 3), c(3, 2, 1), method="kendall"), -4))
stopifnot(identical(var(x, na.rm=TRUE), cov(x, use="complete.obs")))
z <- cov(matrix(1:3, 3, 1, dimnames=list(NULL, "only")))
stopifnot(identical(dim(z), c(1L, 1L)),
          identical(dimnames(z), list("only", "only")))
msg <- tryCatch(cov(x, use="all.obs"), error=conditionMessage)
stopifnot(identical(msg, "missing observations in cov/cor"))
missing <- matrix(NA_real_, 3, 2)
msg <- tryCatch(cov(missing, use="complete.obs"), error=conditionMessage)
stopifnot(identical(msg, "no complete element pairs"),
          all(is.na(cov(missing, use="na.or.complete"))))
z <- withCallingHandlers(cov(1:3, rep(7, 3)),
    warning=function(w) stop("covariance of a constant must not warn"))
stopifnot(identical(z, 0))
k <- cbind(a=1:3, b=c(3, 1, 2))
stopifnot(isTRUE(all.equal(unname(cov(k, method="kendall")),
                          matrix(c(6, -2, -2, 6), 2))))
stopifnot(identical(cov(c(1, NA), c(2, NA), use="complete.obs", method="kendall"), NA_real_))
cat("covariance-public-policies-ok\n")
"#;

#[test]
fn covariance_public_policies_match_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(output.contains("covariance-public-policies-ok"), "{output}");
    }
}
