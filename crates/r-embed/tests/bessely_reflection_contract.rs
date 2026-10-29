use r_embed::{RSession, RuntimePathPolicy};
const CONTROLS: &str = r#"
z <- besselY(1,c(-2,-1.5,-.5,-.25));stopifnot(all(abs(z-c(-1.6506826068162548,-.24029783912342698,.671396707141803,.39443093639096749))<1e-14))
for(x in c(-0.0,0.0)){z <- besselY(x,c(0,1,2,-.5,-1.5));stopifnot(identical(z,c(-Inf,-Inf,-Inf,0,0)),identical(1/z[4:5],c(Inf,Inf)))}
stopifnot(identical(besselY(c(NA_real_,NaN,1),c(NaN,0,NA_real_)),c(NA_real_,NaN,NA_real_)))
events <- character();z <- withCallingHandlers(besselY(1,-500.25),warning=function(w){stopifnot(identical(conditionCall(w),quote(besselY(1,-500.25))));events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(z,-Inf),length(events)>=1,length(events)<=2)
old <- options(warn=2);z <- tryCatch(besselY(1,-500.25),error=function(e)conditionMessage(e));options(old);stopifnot(is.character(z),abs(besselY(1,0)-.08825696421567694)<1e-14)
cat("bessely-reflection-contract-ok\n")
"#;
#[test]
fn public_bessely_reflection() {
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("bessely-reflection-contract-ok"), "{out}");
    }
}
