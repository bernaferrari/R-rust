use r_embed::{RSession,RuntimePathPolicy};
const CONTROLS: &str = r#"
z <- besselJ(1,c(0,1,2));stopifnot(length(z)==3,all(abs(z-c(.76519768655796661,.44005058574493355,.11490348493190049))<1e-14))
x <- structure(1:3,names=c("a","b","c"));z <- besselJ(x,c(0,1,2));stopifnot(identical(names(z),names(x)),all(abs(z-c(.76519768655796661,.57672480775687340,.48609126058589108))<1e-14))
stopifnot(identical(besselJ(numeric(0),1),numeric(0)),identical(besselJ(1,numeric(0)),numeric(0)))
z <- suppressWarnings(besselJ(1,c(-2,-1.5,-.5,30,100,500))); target <- c(.11490348493190049,-1.1024955751601793,.43109886801837616,3.4828697942514824e-42,8.4318287896267246e-189,0)
stopifnot(all(abs(z-target)<=1e-14*abs(target)+1e-300),identical(besselJ(c(1,NA_real_,NaN),c(NA_real_,0,1)),c(NA_real_,NA_real_,NaN)))
events <- character()
z <- withCallingHandlers(besselJ(c(-1,1),c(0,NaN)),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(all(is.nan(z)),identical(events,c("value out of range in 'bessel_j'","NaNs produced")))
cat("besselj-vector-contract-ok\n")
"#;
#[test]
fn public_besselj_vector_orders(){for mut s in [RSession::new().unwrap(),RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(),std::env::temp_dir())).unwrap()]{let out=s.eval(CONTROLS).unwrap();assert!(out.contains("besselj-vector-contract-ok"),"{out}");}}
