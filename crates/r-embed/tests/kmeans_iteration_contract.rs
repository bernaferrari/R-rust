use r_embed::{RSession, RuntimePathPolicy};

// Oracle: GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
x <- matrix(0:9, ncol=1)
initial <- matrix(c(0, 1), ncol=1)
for (algorithm in c("Lloyd", "MacQueen")) {
    z <- kmeans(x, initial, algorithm=algorithm)
    expected_iter <- if (algorithm == "Lloyd") 5L else 2L
    stopifnot(identical(z$iter, expected_iter), is.null(z$ifault),
              identical(z$cluster, rep(1:2, each=5)),
              isTRUE(all.equal(as.vector(z$centers), c(2, 7))))
    warnings <- character()
    z <- withCallingHandlers(kmeans(x, initial, algorithm=algorithm, iter.max=1),
        warning=function(w) {
            warnings <<- c(warnings, conditionMessage(w))
            gc()
            invokeRestart("muffleWarning")
        })
    expected_centers <- if (algorithm == "Lloyd") c(0, 5) else c(2, 7)
    stopifnot(identical(z$iter, 2L), identical(z$ifault, 2L),
              identical(warnings, "did not converge in 1 iteration"),
              isTRUE(all.equal(as.vector(z$centers), expected_centers)))
    warnings <- character()
    z <- withCallingHandlers(kmeans(x, initial, algorithm=algorithm, iter.max=2),
        warning=function(w) {
            warnings <<- c(warnings, conditionMessage(w))
            gc()
            invokeRestart("muffleWarning")
        })
    if (algorithm == "Lloyd") {
        stopifnot(identical(z$iter, 3L), identical(z$ifault, 2L),
                  identical(warnings, "did not converge in 2 iterations"))
    } else {
        stopifnot(identical(z$iter, 2L), is.null(z$ifault), length(warnings) == 0)
    }
}
z <- kmeans(x, initial)
stopifnot(identical(z$iter, 1L), identical(z$ifault, 0L),
          identical(z$cluster, rep(1:2, each=5)))
x <- matrix(c(0, 1, 2, 8, 9, 10), ncol=1)
z <- kmeans(x, matrix(c(0, 10), ncol=1), algorithm="MacQueen")
stopifnot(identical(z$iter, 1L), identical(z$cluster, rep(1:2, each=3)))
cat("kmeans-iteration-contract-ok\n")
"#;

#[test]
fn public_kmeans_iteration_and_warning_contract_matches_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(output.contains("kmeans-iteration-contract-ok"), "{output}");
    }
}
