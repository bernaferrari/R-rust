use r_embed::{RSession, RuntimePathPolicy};

// Oracle: GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
x <- matrix(1:4, 2, 2)
a <- loglin(x, list(1, 2), fit=TRUE, print=FALSE, eps=0.1, iter=20)
b <- loglin(x, list(1, 2), fit=TRUE, print=FALSE, eps="0.1", iter="20")
stopifnot(identical(a, b),
          isTRUE(all.equal(b$fit, matrix(c(1.2, 1.8, 2.8, 4.2), 2))),
          b$df == 1, abs(b$lrt - 0.0804348646096483) < 1e-12)
warnings <- character()
z <- withCallingHandlers(loglin(x, list(1, 2), fit=TRUE, print=FALSE,
    eps=0.1+1i, iter=20+1i), warning=function(w) {
        warnings <<- c(warnings, conditionMessage(w))
        invokeRestart("muffleWarning")
    })
stopifnot(identical(z, a),
          identical(warnings, rep("imaginary parts discarded in coercion", 2)))
events <- character()
z <- withCallingHandlers(tryCatch(loglin(x, list(1, 2), print=FALSE, iter="bad"),
    error=function(e) {
        events <<- c(events, paste("error", conditionMessage(e)))
        NULL
    }), warning=function(w) {
        events <<- c(events, paste("warning", conditionMessage(w)))
        invokeRestart("muffleWarning")
    })
stopifnot(is.null(z), identical(events, c("warning NAs introduced by coercion",
          "error negative length vectors are not allowed")))
msg <- tryCatch(loglin(x, list(1, 2), print=FALSE, iter=integer(0)), error=conditionMessage)
stopifnot(identical(msg, "negative length vectors are not allowed"))
cat("loglin-scalar-controls-ok\n")
"#;

#[test]
fn public_loglin_scalar_controls_match_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(output.contains("loglin-scalar-controls-ok"), "{output}");
    }
}
