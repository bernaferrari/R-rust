use r_embed::{RSession, RuntimePathPolicy};
// GNU R bac583951b728e97b9786804d3b4081f0fe18df5 r90451.
const CONTROLS: &str = r#"
for(lower in c(TRUE,FALSE)) {
 expected.t <- if(lower)c(-Inf,Inf)else c(Inf,-Inf)
 expected.f <- if(lower)c(0,Inf)else c(Inf,0)
 for(df in c(.5,1,10,Inf))stopifnot(identical(qt(c(-Inf,0),df,lower.tail=lower,log.p=TRUE),expected.t))
 for(d in list(c(1,1),c(1,Inf),c(Inf,1),c(Inf,Inf)))stopifnot(identical(qf(c(-Inf,0),d[1],d[2],lower.tail=lower,log.p=TRUE),expected.f))
}
stopifnot(abs(qt(-1,1,log.p=TRUE)+.44067109149597056)<1e-14,abs(qf(-1,1,1,log.p=TRUE)-.42525968355637889)<1e-14)
x <- structure(c(-Inf,0,NA_real_,NaN),names=c("zero","one","missing","nan"))
for(f in list(function(p)qt(p,1,log.p=TRUE),function(p)qf(p,1,1,log.p=TRUE))) {
 z <- f(x);gc()
 stopifnot(identical(names(z),names(x)),is.na(z[3]),!is.nan(z[3]),is.nan(z[4]))
 events <- character()
 z <- withCallingHandlers(f(c(-Inf,1)),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
 stopifnot(is.nan(z[2]),identical(events,"NaNs produced"))
}
cat("quantile-log-endpoints-ok\n")
"#;
#[test]
fn public_log_quantile_endpoints() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("quantile-log-endpoints-ok"), "{out}");
    }
}
