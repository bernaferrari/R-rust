use r_embed::{RSession, RuntimePathPolicy};
const CONTROLS: &str = r#"a <- c(1.26606587775200818,.56515910399248503,.13574766976703828)
b <- c(.46575960759364043,.20791041534970844,.04993877689422354)
stopifnot(all(abs(besselI(1,0:2)-a)<1e-14),all(abs(besselI(1,0:2,TRUE)-b)<1e-14))
stopifnot(isTRUE(all.equal(besselI(1,0,c(FALSE,TRUE,NA)),c(a[1],b[1],NA_real_),tolerance=1e-14)),isTRUE(all.equal(.Internal(besselI(1,0,c(1,2,NA_real_))),c(a[1],b[1],NA_real_),tolerance=1e-14)))
stopifnot(isTRUE(all.equal(besselI(1,0,c("FALSE","TRUE","bogus")),c(a[1],b[1],NA_real_),tolerance=1e-14)))
stopifnot(is.na(besselI(1,0,NaN)),is.nan(.Internal(besselI(1,0,NaN))))
for(n in c(0,1,2,10))stopifnot(identical(besselI(c(0,1,1000),n),besselI(c(0,1,1000),-n)))
stopifnot(is.infinite(besselI(1000,0)),is.finite(besselI(1000,0,TRUE)))
x <- structure(1:3,names=c("a","b","c"));stopifnot(identical(names(besselI(x,0)),names(x)))
nu <- structure(0:2,names=c("z","o","t"));stopifnot(identical(names(besselI(1,nu)),names(nu)))
sc <- structure(c(FALSE,TRUE),names=c("f","t"));stopifnot(is.null(names(besselI(1,0,sc))))
sc <- structure(c(1,2),names=c("f","t"));stopifnot(identical(names(.Internal(besselI(1,0,sc))),names(sc)))
stopifnot(identical(besselI(numeric(0),0),numeric(0)),identical(besselI(1,numeric(0)),numeric(0)),identical(besselI(1,0,logical(0)),numeric(0)))
as.logical.besseli_probe <- function(x,...) {gc();stopifnot(abs(besselI(1,0)-a[1])<1e-14);c(FALSE,TRUE)}
stopifnot(isTRUE(all.equal(besselI(1,0,structure(1,class="besseli_probe")),c(a[1],b[1]),tolerance=1e-14)))
events <- character()
z <- withCallingHandlers(besselI(-1,0),warning=function(w){stopifnot(identical(conditionCall(w),quote(besselI(-1,0))));events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(is.nan(z),identical(events,c("value out of range in 'bessel_i'","NaNs produced")))
old <- options(warn=2);z <- tryCatch(besselI(-1,0),error=function(e)conditionMessage(e));options(old);stopifnot(is.character(z),abs(besselI(1,0)-a[1])<1e-14)
for(f in list(function()besselI("x",0),function()besselI(1,"x"),function().Internal(besselI(1,0,"x")),function()besselI(1+1i,0)))stopifnot(is.character(tryCatch(f(),error=function(e)conditionMessage(e))))
stopifnot(all(abs(besselI(1,c(-1.5,-.5,.5))-c(-.29352532634747996,1.23120021459296747,.93767488824548761))<1e-14),all(abs(besselI(1,c(-1.5,-.5,.5),TRUE)-c(-.10798193302637615,.45293324691462078,.34495131388824463))<1e-14))
stopifnot(identical(besselI(0,0:2),c(1,0,0)),suppressWarnings(besselI(1,500))==0)
for(x in c(-0.0,0.0)){z <- besselI(x,c(0,1,2,-.5,-1.5));stopifnot(identical(z,c(1,0,0,Inf,-Inf)),identical(1/z[2:3],c(Inf,Inf)))}
stopifnot(identical(.Internal(besselI(c(NA_real_,NaN,1),c(NaN,0,NA_real_),1)),c(NA_real_,NaN,NA_real_)))
events <- character(); z <- withCallingHandlers(besselI(1,-500.5),warning=function(w){stopifnot(identical(conditionCall(w),quote(besselI(1,-500.5))));events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(z,Inf),length(events)==1)
cat("besseli-checked-contract-ok\n")
"#;
#[test]
fn public_besseli_checked() {
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("besseli-checked-contract-ok"), "{out}");
    }
}
