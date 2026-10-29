use r_embed::{RSession, RuntimePathPolicy};
const CONTROLS: &str = r#"
z <- besselJ(1,c(-2,-1.5,-.5,-.25));stopifnot(all(abs(z-c(.11490348493190049,-1.1024955751601793,.43109886801837616,.66938481726157439))<1e-14))
for(x in c(-0.0,0.0)){z <- besselJ(x,c(0,1,2,-.5,-1.5));stopifnot(identical(z,c(1,0,0,Inf,-Inf)),identical(1/z[2:3],c(Inf,Inf)))}
stopifnot(identical(besselJ(c(NA_real_,NaN,1),c(NaN,0,NA_real_)),c(NA_real_,NaN,NA_real_)))
events <- character();z <- withCallingHandlers(besselJ(1,-500.25),warning=function(w){stopifnot(identical(conditionCall(w),quote(besselJ(1,-500.25))));events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(z,Inf),length(events)>=1,length(events)<=2)
old <- options(warn=2);z <- tryCatch(besselJ(1,-500.25),error=function(e)conditionMessage(e));options(old);stopifnot(is.character(z),abs(besselJ(1,0)-.7651976865579666)<1e-14)
cat("besselj-reflection-contract-ok\n")
"#;
#[test]
fn public_besselj_reflection() {
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("besselj-reflection-contract-ok"), "{out}");
    }
}
