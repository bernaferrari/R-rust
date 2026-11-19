use r_embed::{RSession, RuntimePathPolicy};

const INTERNAL_CHOL_CONTROLS: &str = r#"
x <- matrix(c(5, 1, 1, 3), 2, 2)
x_before <- x
r <- .Internal(La_chol(x, FALSE, -1))
stopifnot(is.matrix(r), all.equal(crossprod(r), x), identical(x, x_before))
i <- .Internal(La_chol2inv(r, 2))
stopifnot(is.matrix(i), all.equal(i, solve(x)), identical(x, x_before))

bad_pivot <- tryCatch(.Internal(La_chol(x, NA, -1)), error=function(e)
  c(class=class(e)[1], message=conditionMessage(e)))
stopifnot(identical(unname(bad_pivot["class"]), "simpleError"),
          grepl("invalid 'pivot' argument", bad_pivot["message"], fixed=TRUE))

singular <- crossprod(cbind(1:5, (1:5)^2, (1:5) + 3*(1:5)^2))
warned <- FALSE
withCallingHandlers(
  .Internal(La_chol(singular, TRUE, -1)),
  warning=function(w) {
    warned <<- grepl("rank-deficient or not positive definite",
                     conditionMessage(w), fixed=TRUE)
    gc()
    invokeRestart("muffleWarning")
  }
)
stopifnot(warned)
cat("chol-internal-controls\n")
"#;

#[test]
fn original_cholesky_internals_work_under_both_runtime_path_policies() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(INTERNAL_CHOL_CONTROLS).unwrap();
        assert!(output.contains("chol-internal-controls"), "{output}");
    }
}
