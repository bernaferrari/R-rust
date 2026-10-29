use r_embed::{RSession, RuntimePathPolicy};

// Oracle: GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
x <- rbind(c(0, 0), c(3, 4))
for (p in list(0, -1, Inf, -Inf, NA_real_, NaN, NULL, numeric(0), FALSE)) {
    msg <- tryCatch(dist(x, method="minkowski", p=p), error=conditionMessage)
    stopifnot(identical(msg, "distance(): invalid p"))
}
stopifnot(identical(as.vector(dist(x, method="minkowski", p="2")), 5),
          identical(as.vector(dist(x, method="minkowski", p=TRUE)), 7),
          identical(as.vector(dist(x, method="euclidean", p=0)), 5))
warnings <- character()
z <- withCallingHandlers(dist(x, method="minkowski", p=2+1i), warning=function(w) {
    warnings <<- c(warnings, conditionMessage(w))
    gc()
    invokeRestart("muffleWarning")
})
stopifnot(identical(as.vector(z), 5),
          identical(warnings, "imaginary parts discarded in coercion"))
events <- character()
z <- withCallingHandlers(tryCatch(dist(x, method="minkowski", p="bad"), error=function(e) {
    events <<- c(events, paste("error", conditionMessage(e)))
    NULL
}), warning=function(w) {
    events <<- c(events, paste("warning", conditionMessage(w)))
    gc()
    invokeRestart("muffleWarning")
})
stopifnot(is.null(z), identical(events, c("warning NAs introduced by coercion",
          "error distance(): invalid p")))
warnings <- character()
z <- withCallingHandlers(dist(x, p="bad"), warning=function(w) {
    warnings <<- c(warnings, conditionMessage(w))
    gc()
    invokeRestart("muffleWarning")
})
stopifnot(identical(as.vector(z), 5), identical(warnings, "NAs introduced by coercion"))
empty <- matrix(numeric(0), nrow=0, ncol=2)
msg <- tryCatch(dist(empty, method="minkowski", p=0), error=conditionMessage)
stopifnot(identical(msg, "distance(): invalid p"))
native <- get("C_Cdist", asNamespace("stats"))
for (method in c(0L, 7L, NA_integer_)) {
    msg <- tryCatch(.Call(native, x, method, list(), 2), error=conditionMessage)
    stopifnot(identical(msg, "distance(): invalid distance"))
}
stopifnot(identical(.Call(native, x, "1", list(), 2), 5))
cat("dist-minkowski-contract-ok\n")
"#;

#[test]
fn public_distance_power_contract_matches_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(output.contains("dist-minkowski-contract-ok"), "{output}");
    }
}
