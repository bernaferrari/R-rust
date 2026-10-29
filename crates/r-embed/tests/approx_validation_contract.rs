use r_embed::{RSession, RuntimePathPolicy};

// Oracle: GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
for (f in list(-1, 2, NA_real_, NaN, Inf, -Inf)) {
    msg <- tryCatch(approx(1:2, c(10, 20), xout=1.5, method="constant", f=f), error=conditionMessage)
    stopifnot(identical(msg, "approx(): invalid f value"))
    msg <- tryCatch(approxfun(1:2, c(10, 20), method="constant", f=f), error=conditionMessage)
    stopifnot(identical(msg, "approx(): invalid f value"))
}
msg <- tryCatch(approx(c(1, NA, 3), c(10, 20, 30), xout=2, na.rm=FALSE), error=function(e) { gc(); conditionMessage(e) })
stopifnot(identical(msg, "approx(x,y, .., na.rm=FALSE): NA values in x are not allowed"))
msg <- tryCatch(approxfun(c(1, NA, 3), c(10, 20, 30), na.rm=FALSE), error=conditionMessage)
stopifnot(identical(msg, "approx(x,y, .., na.rm=FALSE): NA values in x are not allowed"))
stopifnot(identical(approx(1:2, c(10, 20), xout=1.5, method="constant", f="0.5")$y, 15),
          identical(approxfun(1:2, c(10, 20), method="constant", f=TRUE)(1.5), 20),
          identical(approx(1:2, c(10, 20), xout=1.5, f=NA_real_)$y, 15))
expected <- c(10, NA_real_, NA_real_, NA_real_, 30)
stopifnot(identical(approx(1:3, c(10, NA, 30), xout=c(1,1.5,2,2.5,3), na.rm=FALSE)$y, expected))
expected <- c(10,10,NA_real_,NA_real_,30)
stopifnot(identical(approxfun(1:3, c(10,NA,30), method="constant", na.rm=FALSE)(c(1,1.5,2,2.5,3)), expected))
events <- character()
z <- withCallingHandlers(tryCatch(approx(1:2, c(10,20), xout=1.5, method="constant", f="bad"), error=function(e) {
    events <<- c(events, paste("error", conditionMessage(e)))
    NULL
}), warning=function(w) {
    events <<- c(events, paste("warning", conditionMessage(w)))
    gc()
    invokeRestart("muffleWarning")
})
stopifnot(is.null(z), identical(events, c("warning NAs introduced by coercion", "error approx(): invalid f value")))
warnings <- character()
z <- withCallingHandlers(approx(1:2, c(10,20), xout=1.5, method="constant", f=0.5+1i), warning=function(w) {
    warnings <<- c(warnings, conditionMessage(w))
    gc()
    invokeRestart("muffleWarning")
})
stopifnot(identical(z$y, 15),
          identical(warnings, rep("imaginary parts discarded in coercion", 2)))
stopifnot(identical(approx(1:2, c(10,20), xout=numeric(0))$y, numeric(0)),
          identical(approxfun(1:2, c(10,20))(numeric(0)), numeric(0)))
native <- get("C_ApproxTest", asNamespace("stats"))
msg <- tryCatch(.Call(native, c(1,2), c(10,20), 0L, 0, TRUE), error=conditionMessage)
stopifnot(identical(msg, "approx(): invalid interpolation method"))
msg <- tryCatch(.Call(native, c(1,2), c(10,NA_real_), 1L, 0, TRUE), error=conditionMessage)
stopifnot(identical(msg, "approx(): attempted to interpolate NA values"))
stopifnot(is.null(.Call(native, numeric(0), numeric(0), 1L, 0, TRUE)))
cat("approx-validation-contract-ok\n")
"#;

// Malformed unequal buffers violate GNU's native precondition: do not execute
// those cases in GNU, where they could read beyond a buffer. The port rejects
// both short and surplus y before any indexed read.
const ADMISSION: &str = r#"
test_native <- get("C_ApproxTest", asNamespace("stats"))
eval_native <- get("C_Approx", asNamespace("stats"))
x <- c(1,2); y <- c(10,20)
for (bad_y in list(10, c(10,20,30))) {
    msg <- tryCatch(.Call(test_native, x, bad_y, 1L, 0, TRUE), error=conditionMessage)
    stopifnot(identical(msg, "'x' and 'y' lengths differ"))
    msg <- tryCatch(.Call(eval_native, x, bad_y, 1.5, 1L, NA_real_, NA_real_, 0, TRUE), error=conditionMessage)
    stopifnot(identical(msg, "'x' and 'y' lengths differ"))
}
for (bad in list(1:2, c(TRUE,FALSE), c("1","2"), list(1,2), NULL)) {
    expected <- paste0("REAL() can only be applied to a 'numeric', not a '", typeof(bad), "'")
    expected_x <- if (is.null(bad)) "LENGTH or similar applied to NULL object" else expected
    msg <- tryCatch(.Call(test_native, bad, y, 1L, 0, TRUE), error=conditionMessage)
    stopifnot(identical(msg, expected_x))
    msg <- tryCatch(.Call(test_native, x, bad, 1L, 0, TRUE), error=conditionMessage)
    stopifnot(identical(msg, expected))
    msg <- tryCatch(.Call(eval_native, bad, y, 1.5, 1L, NA_real_, NA_real_, 0, TRUE), error=conditionMessage)
    stopifnot(identical(msg, expected_x))
    msg <- tryCatch(.Call(eval_native, x, bad, 1.5, 1L, NA_real_, NA_real_, 0, TRUE), error=conditionMessage)
    stopifnot(identical(msg, expected))
}
stopifnot(identical(.Call(eval_native, x, y, 1:2, 1L, NA_real_, NA_real_, 0, TRUE), y),
          is.nan(.Call(eval_native, numeric(0), numeric(0), 1.5, 1L, NA_real_, NA_real_, 0, TRUE)))
cat("approx-native-admission-ok\n")
"#;

#[test]
fn public_approx_validation_contract_matches_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(output.contains("approx-validation-contract-ok"), "{output}");
        let output = session.eval(ADMISSION).unwrap();
        assert!(output.contains("approx-native-admission-ok"), "{output}");
    }
}
