use r_embed::{RSession, RuntimePathPolicy};
// Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
expected <- 1.1125369292535661e-308
for(s in c(.125,.5,1,2)) {
 z <- qbeta(-1e300,s,s,log.p=TRUE)
 stopifnot(z>0,abs(z/expected-1)<1e-14,identical(qbeta(-1e300,s,s,lower.tail=FALSE,log.p=TRUE),1))
}
stopifnot(identical(qbeta(c(-Inf,0),.5,.5,log.p=TRUE),c(0,1)),abs(qbeta(-1,.5,.5,log.p=TRUE)-.298373474295751)<1e-14)
p <- structure(c(-1e300,NA_real_,NaN,-100),names=c("boundary","missing","nan","interior"))
z <- qbeta(p,.5,.5,log.p=TRUE)
gc()
stopifnot(identical(names(z),names(p)),z[1]>0,is.na(z[2]),!is.nan(z[2]),is.nan(z[3]),abs(z[4]/3.41462781273329e-87-1)<1e-13)
events <- character()
z <- withCallingHandlers(qbeta(c(-1e300,1),.5,.5,log.p=TRUE),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(z[1]>0,is.nan(z[2]),identical(events,"NaNs produced"))
cat("qbeta-boundary-contract-ok\n")
"#;
#[test]
fn public_qbeta_preserves_boundary_convergence() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("qbeta-boundary-contract-ok"), "{out}");
    }
}
