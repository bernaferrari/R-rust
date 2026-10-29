use r_embed::{RSession, RuntimePathPolicy};
// Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5 r90451.
const CONTROLS: &str = r#"
x <- c(-Inf,-1,-.5,0,1,2,Inf,NA_real_,NaN)
for(d in c(NaN,NA_real_)) {
 events <- character()
 z <- withCallingHandlers(psigamma(x,d),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
 target <- if(is.nan(d))c(rep(NaN,7),NA_real_,NaN)else rep(NA_real_,9)
 stopifnot(identical(z,target),identical(events,character(0)))
}
events <- character()
z <- withCallingHandlers(psigamma(NaN,c(-1,0,1,2,Inf)),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
stopifnot(identical(z,rep(NaN,5)),identical(events,character(0)))
stopifnot(all(abs(psigamma(c(1,2,3),0)-digamma(c(1,2,3)))<1e-14),all(abs(psigamma(c(1,2,3),1)-trigamma(c(1,2,3)))<1e-14))
stopifnot(all(abs(psigamma(1,c(0,.5,1.5,2))-c(-.5772156649015329,-.5772156649015329,-2.4041138063191885,-2.4041138063191885))<1e-13))
x <- structure(c(1,2,NA_real_,NaN),names=c("one","two","missing","nan"))
z <- psigamma(x,NaN);gc()
stopifnot(identical(z,structure(c(NaN,NaN,NA_real_,NaN),names=names(x))))
for(f in list(beta,lbeta)) {
 z <- f(c(-1,0,1,Inf,NA_real_),NaN)
 stopifnot(identical(z,c(NaN,NaN,NaN,NaN,NA_real_)))
}
stopifnot(all(abs(beta(c(.5,1,2),c(.5,2,3))-c(pi,.5,1/12))<1e-14),all(abs(lbeta(c(.5,1,2),c(.5,2,3))-log(c(pi,.5,1/12)))<1e-14))
events <- character()
z <- withCallingHandlers(psigamma(c(1,1),c(NaN,-1)),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(z,c(NaN,NaN)),identical(events,"NaNs produced"))
cat("psigamma-nan-contract-ok\n")
"#;
#[test]
fn public_psigamma_nan_derivatives() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("psigamma-nan-contract-ok"), "{out}");
    }
}
