use r_embed::{RSession, RuntimePathPolicy};
const CONTROLS: &str = r#"xs <- c(1e-300,1e-8,9.999995e-5,.001234565,.001234575,1.23456789,12.3456789)
text <- c("1e-300","1e-08","0.0001","0.00123456","0.00123457","1.23457","12.3457")
for(i in seq_along(xs)){
 events <- character();x <- xs[i]
 z <- withCallingHandlers(besselY(x,500.25),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
 stopifnot(identical(z,-Inf),identical(events,paste0("bessel_y(",text[i],",nu=500.25): precision lost in result")))
}
for(n in c(500,-500)){
 events <- character();z <- withCallingHandlers(besselY(1,n),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
 stopifnot(identical(z,-Inf),identical(events,"bessel_y(1,nu=500): precision lost in result"))
}
orders <- c(1e7+1,1e8+1,1.23456789e8,Inf,-Inf)
text <- c("1e+07","1e+08","1.23457e+08","inf","inf")
for(i in seq_along(orders)){
 events <- character();z <- withCallingHandlers(besselY(1,orders[i]),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
 stopifnot(is.nan(z),identical(events,c(paste0("besselY(x, nu): nu=",text[i]," too large for bessel_y() algorithm"),"NaNs produced")))
}
events <- character();z <- withCallingHandlers(besselY(1,-500.25),warning=function(w){stopifnot(identical(conditionCall(w),quote(besselY(1,-500.25))));events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(z,-Inf),length(events)==2,identical(events[1],"bessel_y(1,nu=500.25): precision lost in result"))
events <- character();z <- withCallingHandlers(besselJ(1,-500.25),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
stopifnot(identical(z,Inf),length(events)==2,identical(events[2],"bessel_y(1,nu=500.25): precision lost in result"))
old <- options(warn=2);events <- character()
z <- tryCatch(withCallingHandlers(besselY(1,-500.25),warning=function(w){events <<- c(events,conditionMessage(w))}),error=function(e)conditionMessage(e));options(old)
stopifnot(identical(events,"bessel_y(1,nu=500.25): precision lost in result"),identical(z,"(converted from warning) bessel_y(1,nu=500.25): precision lost in result"),abs(besselY(1,0)-.08825696421567694)<1e-14)
events <- character();z <- withCallingHandlers(besselY(c(NA_real_,NaN),1e8+1),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")});stopifnot(identical(z,c(NA_real_,NaN)),length(events)==0)
stopifnot(identical(besselY(0,0:2),rep(-Inf,3)))
events <- character();z <- withCallingHandlers(besselY(c(1,NA_real_),NaN),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")});stopifnot(identical(z,c(NaN,NA_real_)),length(events)==0)
events <- character();z <- withCallingHandlers(besselY(1,c(500,1e8+1,501)),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")});stopifnot(identical(z,c(-Inf,NaN,-Inf)),identical(events,c("bessel_y(1,nu=500): precision lost in result","besselY(x, nu): nu=1e+08 too large for bessel_y() algorithm","bessel_y(1,nu=501): precision lost in result","NaNs produced")))
cat("bessely-diagnostic-contract-ok\n")
"#;
#[test]
fn public_bessely_diagnostic() {
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("bessely-diagnostic-contract-ok"), "{out}");
    }
}
