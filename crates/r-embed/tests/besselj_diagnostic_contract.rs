use r_embed::{RSession, RuntimePathPolicy};
const CONTROLS: &str = r#"xs<-c(1,1e-300,1e-8,9.999995e-5,.001234565,12.3456789)
text<-c("1","1e-300","1e-08","0.0001","0.00123456","12.3457")
for(i in seq_along(xs)){
 events<-character();z<-withCallingHandlers(besselJ(xs[i],500.25),warning=function(w){events<<-c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
 stopifnot(identical(z,0),identical(events,paste0("bessel_j(",text[i],",nu=500.25): precision lost in result")))
}
for(n in c(500,-500)){
 events<-character();z<-withCallingHandlers(besselJ(1,n),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")})
 stopifnot(identical(z,0),identical(events,"bessel_j(1,nu=500): precision lost in result"))
}
events<-character();z<-withCallingHandlers(besselJ(1,-500.25),warning=function(w){stopifnot(identical(conditionCall(w),quote(besselJ(1,-500.25))));events<<-c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(z,Inf),identical(events,c("bessel_j(1,nu=500.25): precision lost in result","bessel_y(1,nu=500.25): precision lost in result")))
events<-character();z<-withCallingHandlers(besselY(1,-500.25),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")})
stopifnot(identical(z,-Inf),identical(events,c("bessel_y(1,nu=500.25): precision lost in result","bessel_j(1,nu=500.25): precision lost in result")))
events<-character();z<-withCallingHandlers(besselJ(1,-500.5),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")})
stopifnot(identical(z,Inf),identical(events,"bessel_y(1,nu=500.5): precision lost in result"))
old<-options(warn=2);events<-character();z<-tryCatch(withCallingHandlers(besselJ(1,-500.25),warning=function(w)events<<-c(events,conditionMessage(w))),error=function(e)conditionMessage(e));options(old)
stopifnot(identical(events,"bessel_j(1,nu=500.25): precision lost in result"),identical(z,"(converted from warning) bessel_j(1,nu=500.25): precision lost in result"),abs(besselJ(1,0)-.7651976865579666)<1e-14)
for(x in c(-0.0,0.0)){events<-character();z<-withCallingHandlers(besselJ(x,c(0,1,500)),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")});stopifnot(identical(z,c(1,0,0)),length(events)==0,identical(1/z[2:3],c(Inf,Inf)))}
events<-character();z<-withCallingHandlers(besselJ(c(NA_real_,NaN),500),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")});stopifnot(identical(z,c(NA_real_,NaN)),length(events)==0)
events<-character();z<-withCallingHandlers(besselJ(Inf,0),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")});stopifnot(identical(z,0),identical(events,"value out of range in 'J_bessel'"))
cat("besselj-diagnostic-contract-ok\n")
"#;
// Large-order public GNU probes intentionally avoided: these cap controls are
// pinned-source expectations corroborated by guarded direct _ex probes (>1e9).
const CAP_CONTROLS: &str = r#"for(n in c(1e7+1,1e9+1,Inf,-Inf)){
 events<-character();z<-withCallingHandlers(besselJ(1,n),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")})
 nu<-if(is.finite(n))if(abs(n)>1e9)"1e+09" else "1e+07" else "inf"
 stopifnot(is.nan(z),identical(events,c(paste0("besselJ(x, nu): nu=",nu," > 1e7; too large for bessel_j() algorithm"),"NaNs produced")))
}
for(half in c(FALSE,TRUE)){
 n<--1e7-if(half).5 else .25
 events<-character();z<-withCallingHandlers(besselJ(1,n),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")})
 j<-"besselJ(x, nu): nu=1e+07 > 1e7; too large for bessel_j() algorithm";y<-"besselY(x, nu): nu=1e+07 too large for bessel_y() algorithm"
 stopifnot(is.nan(z),identical(events,if(half)c(y,"NaNs produced")else c(j,y,"NaNs produced")))
 events<-character();z<-withCallingHandlers(besselY(1,n),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")})
 stopifnot(is.nan(z),identical(events,if(half)c(j,"NaNs produced")else c(y,j,"NaNs produced")))
}
events<-character();z<-withCallingHandlers(besselJ(1,c(500,1e7+1,501)),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")});stopifnot(identical(z,c(0,NaN,0)),identical(events,c("bessel_j(1,nu=500): precision lost in result","besselJ(x, nu): nu=1e+07 > 1e7; too large for bessel_j() algorithm","bessel_j(1,nu=501): precision lost in result","NaNs produced")))
"#;
#[test]
fn public_besselj_diagnostic() {
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let out = session.eval(CONTROLS).unwrap();
        session.eval(CAP_CONTROLS).unwrap();
        assert!(out.contains("besselj-diagnostic-contract-ok"), "{out}");
    }
}
