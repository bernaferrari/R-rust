use r_embed::RSession;

#[test]
fn chol_pivot_warning_keeps_gnu_method_call_in_direct_nested_and_caught_paths() {
    let mut session = RSession::new().unwrap();
    let output = session
        .eval(
            r#"
            x <- matrix(c(1:5, (1:5)^2), 5, 2)
            x <- cbind(x, x[, 1] + 3*x[, 2])
            m <- crossprod(x)
            if (!exists("chol.default")) {
              chol.default <- function(x, pivot = FALSE) chol(x, pivot = pivot)
            }
            cat("DIRECT METHOD\n")
            invisible(chol.default(m, pivot = TRUE))
            cat("DIRECT GENERIC\n")
            invisible(chol(m, pivot = TRUE))
            cat("ALIAS\n")
            alias_chol <- chol
            invisible(alias_chol(m, pivot = TRUE))
            nested_chol <- function(z) invisible(chol(z, pivot = TRUE))
            cat("NESTED\n")
            nested_chol(m)
            cat("CAUGHT\n")
            tryCatch(nested_chol(m), warning = function(w) {
              cat("conditionCall:", paste(deparse(conditionCall(w)), collapse = " "), "\n")
            })
            cat("CALLING HANDLER\n")
            withCallingHandlers(nested_chol(m), warning = function(w) {
              gc()
              cat("conditionCall:", paste(deparse(conditionCall(w)), collapse = " "), "\n")
              invokeRestart("muffleWarning")
            })
            cat("CALL-LESS\n")
            warning("call-less", call. = FALSE)
            "#,
        )
        .unwrap();
    assert_eq!(
        output,
        concat!(
            "DIRECT METHOD\n",
            "Warning message:\n",
            "In chol.default(m, pivot = TRUE) :\n",
            "  the matrix is either rank-deficient or not positive definite\n",
            "DIRECT GENERIC\n",
            "Warning message:\n",
            "In chol.default(m, pivot = TRUE) :\n",
            "  the matrix is either rank-deficient or not positive definite\n",
            "ALIAS\n",
            "Warning message:\n",
            "In chol.default(m, pivot = TRUE) :\n",
            "  the matrix is either rank-deficient or not positive definite\n",
            "NESTED\n",
            "Warning message:\n",
            "In chol.default(z, pivot = TRUE) :\n",
            "  the matrix is either rank-deficient or not positive definite\n",
            "CAUGHT\n",
            "conditionCall: chol.default(z, pivot = TRUE) \n",
            "CALLING HANDLER\n",
            "conditionCall: chol.default(z, pivot = TRUE) \n",
            "CALL-LESS\n",
            "Warning message:\n",
            "call-less \n",
        )
    );
}
