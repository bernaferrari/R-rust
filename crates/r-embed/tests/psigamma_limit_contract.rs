use r_embed::{RSession, RuntimePathPolicy};
const CONTROLS: &str = r#"
events <- character()
z <- withCallingHandlers(psigamma(1,c(101,101.5,100.5,NaN,NA_real_)),warning=function(w){stopifnot(identical(conditionCall(w),quote(psigamma(1,c(101,101.5,100.5,NaN,NA_real_)))));events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(events,c("deriv = 101 > 100 (= n_max)","deriv = 102 > 100 (= n_max)","NaNs produced")),all(is.nan(z[c(1,2,4)])),is.na(z[5]),is.finite(z[3]))
for(x in c(NA_real_,NaN)) {
 events <- character()
 z <- withCallingHandlers(psigamma(x,c(101,102)),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
 stopifnot(length(events)==0,all(is.na(z)))
}
old <- options(warn=2)
z <- tryCatch(psigamma(1,101),error=function(e)conditionMessage(e)); options(old)
stopifnot(is.character(z),abs(psigamma(1,1)-pi^2/6)<1e-14)
x <- structure(c(1,2),names=c("a","b")); stopifnot(identical(names(psigamma(x,0)),names(x)),identical(psigamma(numeric(0),101),numeric(0)))
for(f in list(function()psigamma("x",101),function()psigamma(1,"x"),function()psigamma(factor("a"),101))) stopifnot(is.character(tryCatch(f(),error=function(e)conditionMessage(e))))
cat("psigamma-limit-contract-ok\n")
"#;
#[test]
fn public_psigamma_limit_warning() {
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("psigamma-limit-contract-ok"), "{out}");
    }
}
