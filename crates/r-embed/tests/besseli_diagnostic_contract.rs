use r_embed::{RSession, RuntimePathPolicy};
const CONTROLS: &str = r#"for(sc in c(FALSE,TRUE)){
 for(n in c(500,-500)){
  events<-character();z<-withCallingHandlers(besselI(1,n,sc),warning=function(w){events<<-c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
  stopifnot(identical(z,0),identical(events,"bessel_i(1,nu=500): precision lost in result"))
 }
 for(n in c(-500.25,-500.5)){
  events<-character();z<-withCallingHandlers(besselI(1,n,sc),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")})
  nu<-if(n== -500.25)"500.25" else "500.5"
  stopifnot(identical(z,Inf),identical(events,paste0("bessel_i(1,nu=",nu,"): precision lost in result")))
 }
}
xs<-c(1e-8,9.999995e-5,12.3456789);text<-c("1e-08","0.0001","12.3457")
for(i in seq_along(xs)){
 events<-character();z<-withCallingHandlers(besselI(xs[i],500.25),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")})
 stopifnot(identical(z,0),identical(events,paste0("bessel_i(",text[i],",nu=500.25): precision lost in result")))
}
events<-character();z<-withCallingHandlers(.Internal(besselI(1,c(500,501),c(1,2))),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")});stopifnot(identical(z,c(0,0)),identical(events,c("bessel_i(1,nu=500): precision lost in result","bessel_i(1,nu=501): precision lost in result")))
old<-options(warn=2);events<-character();z<-tryCatch(withCallingHandlers(besselI(1,-500.25),warning=function(w){stopifnot(identical(conditionCall(w),quote(besselI(1,-500.25))));events<<-c(events,conditionMessage(w))}),error=function(e)conditionMessage(e));options(old)
stopifnot(identical(events,"bessel_i(1,nu=500.25): precision lost in result"),identical(z,"(converted from warning) bessel_i(1,nu=500.25): precision lost in result"),abs(besselI(1,0)-1.2660658777520082)<1e-14)
for(x in c(-0.0,0.0)){events<-character();z<-withCallingHandlers(besselI(x,c(0,1,-500)),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")});stopifnot(identical(z,c(1,0,0)),identical(1/z[2:3],c(Inf,Inf)),length(events)==0)}
events<-character();z<-withCallingHandlers(besselI(c(NA_real_,NaN),500),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")});stopifnot(identical(z,c(NA_real_,NaN)),length(events)==0)
stopifnot(identical(besselI(Inf,0),Inf),identical(besselI(Inf,0,TRUE),0))
stopifnot(abs(.Internal(besselI(1,0,1.9))-besselI(1,0))<1e-14,abs(.Internal(besselI(1,0,2.9))-besselI(1,0,TRUE))<1e-14)
cat("besseli-diagnostic-contract-ok\n")
"#;
// Invalid-scale public GNU values are indeterminate; these diagnostic controls
// are corroborated with an initialized, bounded direct _ex work buffer.
const INTERNAL_CONTROLS: &str = r#"# Invalid-scale GNU work-array values are indeterminate: compare diagnostics only.
for(sc in c(0,3)){events<-character();z<-withCallingHandlers(.Internal(besselI(1,0,sc)),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")});stopifnot(length(z)==1,identical(events,"bessel_i(1): ncalc (=-1) != nb (=1); alpha=0. Arg. out of range?"))}
"#;
#[test]
fn public_besseli_diagnostic() {
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let out = session.eval(CONTROLS).unwrap();
        session.eval(INTERNAL_CONTROLS).unwrap();
        assert!(out.contains("besseli-diagnostic-contract-ok"), "{out}");
    }
}
