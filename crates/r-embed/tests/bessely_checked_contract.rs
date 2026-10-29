use r_embed::{RSession, RuntimePathPolicy};
const CONTROLS: &str = r#"
stopifnot(identical(besselY(numeric(0),1),numeric(0)),identical(besselY(1,numeric(0)),numeric(0)))
x <- structure(1:3,names=c("a","b","c")); z <- besselY(x,c(0,1,2));stopifnot(identical(names(z),names(x)))
z <- besselY(1,c(0,1,2));stopifnot(length(z)==3,all(abs(z-c(.08825696421567697,-.7812128213002887,-1.6506826068162543))<1e-14))
z <- suppressWarnings(besselY(1,c(-2,-1.5,-.5,30,100,500,-500.5)));target <- c(-1.6506826068162548,-.24029783912342698,.671396707141803,-3.0481287832256433e39,-3.7752878101105259e185,-Inf,0)
stopifnot(all(abs(z[1:5]/target[1:5]-1)<1e-14),identical(z[6:7],c(-Inf,0)))
stopifnot(identical(besselY(c(1,NA_real_,NaN),c(NA_real_,0,1)),c(NA_real_,NA_real_,NaN)))
events <- character()
z <- withCallingHandlers(besselY(-1,0),warning=function(w){stopifnot(identical(conditionCall(w),quote(besselY(-1,0))));events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(is.nan(z),identical(events,c("value out of range in 'bessel_y'","NaNs produced")))
old <- options(warn=2); z <- tryCatch(besselY(-1,0),error=function(e)conditionMessage(e));options(old);stopifnot(is.character(z),abs(besselY(1,0)-.08825696421567694)<1e-14)
for(f in list(function()besselY("x",1),function()besselY(1,"x"),function()besselY(factor("a"),1)))stopifnot(is.character(tryCatch(f(),error=function(e)conditionMessage(e))))
cat("bessely-checked-contract-ok\n")
"#;
#[test]
fn public_bessely_checked() {
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("bessely-checked-contract-ok"), "{out}");
    }
}
