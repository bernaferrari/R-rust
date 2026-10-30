use r_embed::{RSession, RuntimePathPolicy};
const CONTROLS: &str = r#"events<-character()
z<-withCallingHandlers(choose(5,1.2),warning=function(w){
 stopifnot(identical(conditionCall(w),quote(choose(5,1.2))));events<<-c(events,conditionMessage(w));gc()
 inner<-withCallingHandlers(besselJ(1,500),warning=function(v){stopifnot(identical(conditionCall(v),quote(besselJ(1,500))));events<<-c(events,conditionMessage(v));gc();invokeRestart("muffleWarning")})
 stopifnot(identical(inner,0));invokeRestart("muffleWarning")
})
stopifnot(identical(z,5),identical(events,c("'k' (1.20) must be integer, rounded to 1","bessel_j(1,nu=500): precision lost in result")))
events<-character();z<-withCallingHandlers(besselY(1,-500.25),warning=function(w){events<<-c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(z,-Inf),identical(events,c("bessel_y(1,nu=500.25): precision lost in result","bessel_j(1,nu=500.25): precision lost in result")))
for(i in 1:2){f<-if(i==1)besselI else besselK;events<-character();z<-withCallingHandlers(f(-1,0),warning=function(w){events<<-c(events,conditionMessage(w));invokeRestart("muffleWarning")});name<-if(i==1)"bessel_i" else "bessel_k";stopifnot(is.nan(z),identical(events,c(paste0("value out of range in '",name,"'"),"NaNs produced")))}
old<-options(warn=2);z<-tryCatch(choose(5,1.2),error=function(e)conditionMessage(e));options(old)
stopifnot(identical(z,"(converted from warning) 'k' (1.20) must be integer, rounded to 1"),identical(choose(6,2),15))
old<-options(warn=2);z<-tryCatch(choose(5,1.2),warning=function(w)conditionMessage(w));options(old)
stopifnot(identical(z,"'k' (1.20) must be integer, rounded to 1"),identical(suppressWarnings(choose(5,1.2)),5))
cat("nmath-warning-bridge-contract-ok\n")
"#;
#[test]
fn public_nmath_warning_bridge() {
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let out = session.eval(CONTROLS).unwrap();
        assert!(out.contains("nmath-warning-bridge-contract-ok"), "{out}");
    }
}
