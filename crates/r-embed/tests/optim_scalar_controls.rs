use r_embed::{RSession, RuntimePathPolicy};

// Oracle: GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
f <- function(x) sum(x*x)
a <- optim(c(3, 4), f, method="BFGS", control=list(maxit=0, fnscale=2))
b <- optim(c(3, 4), f, method="BFGS", control=list(maxit="0", fnscale="2"))
stopifnot(identical(a, b), identical(b$par, c(3, 4)), identical(b$value, 25))
a <- optim(c(3, 4), f, method="BFGS", control=list(maxit=20, reltol=1e-8, fnscale=2))
b <- optim(c(3, 4), f, method="BFGS", control=list(maxit="20", reltol="1e-8", fnscale="2"))
stopifnot(identical(a, b), max(abs(b$par)) < 1e-6, b$convergence == 0)
h <- optimHess(c(3, 4), f, control=list(fnscale="2"))
stopifnot(isTRUE(all.equal(h, diag(2, 2), tolerance=1e-7)))
warnings <- character()
z <- withCallingHandlers(optim(c(3, 4), f, method="BFGS",
    control=list(maxit=0+1i, fnscale=2+1i)), warning=function(w) {
        warnings <<- c(warnings, conditionMessage(w))
        invokeRestart("muffleWarning")
    })
stopifnot(identical(z$par, c(3, 4)), identical(z$value, 25),
          identical(warnings, rep("imaginary parts discarded in coercion", 2)))
warnings <- character()
z <- withCallingHandlers(optim(c(3, 4), f, method="BFGS",
    control=list(maxit=0, fnscale="bad")), warning=function(w) {
        warnings <<- c(warnings, conditionMessage(w))
        invokeRestart("muffleWarning")
    })
stopifnot(is.na(z$value), identical(warnings, "NAs introduced by coercion"))
z <- optim(c(3, 4), f, method="BFGS", control=list(maxit=0, fnscale=numeric(0)))
stopifnot(is.na(z$value))
cat("optim-scalar-controls-ok\n")
"#;

#[test]
fn public_optimizer_scalar_controls_match_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(output.contains("optim-scalar-controls-ok"), "{output}");
    }
}
