use r_embed::{RSession, RuntimePathPolicy};
const CONTROLS: &str = r#"a <- c(.42102443824070834,.60190723019723458,1.62483889863517739)
b <- c(1.1444630798068949,1.6361534862632581,4.4167700523334110)
stopifnot(all(abs(besselK(1,0:2)-a)<1e-14),all(abs(besselK(1,0:2,TRUE)-b)<1e-14))
stopifnot(isTRUE(all.equal(besselK(1,0,c(FALSE,TRUE,NA)),c(a[1],b[1],NA_real_),tolerance=1e-14)),isTRUE(all.equal(.Internal(besselK(1,0,c(1,2,NA_real_))),c(a[1],b[1],NA_real_),tolerance=1e-14)))
stopifnot(isTRUE(all.equal(besselK(1,0,c("FALSE","TRUE","bogus")),c(a[1],b[1],NA_real_),tolerance=1e-14)))
stopifnot(is.na(besselK(1,0,NaN)),is.nan(.Internal(besselK(1,0,NaN))))
for(n in c(0,.5,1,2,10))stopifnot(identical(besselK(c(0,1,1000),n),besselK(c(0,1,1000),-n)))
stopifnot(besselK(1000,0)==0,is.finite(besselK(1000,0,TRUE)))
x <- structure(1:3,names=c("a","b","c"));stopifnot(identical(names(besselK(x,0)),names(x)))
nu <- structure(0:2,names=c("z","o","t"));stopifnot(identical(names(besselK(1,nu)),names(nu)))
sc <- structure(c(FALSE,TRUE),names=c("f","t"));stopifnot(is.null(names(besselK(1,0,sc))))
sc <- structure(c(1,2),names=c("f","t"));stopifnot(identical(names(.Internal(besselK(1,0,sc))),names(sc)))
stopifnot(identical(besselK(numeric(0),0),numeric(0)),identical(besselK(1,numeric(0)),numeric(0)),identical(besselK(1,0,logical(0)),numeric(0)))
as.logical.besselk_probe <- function(x,...) {gc();stopifnot(abs(besselK(1,0)-a[1])<1e-14);c(FALSE,TRUE)}
stopifnot(isTRUE(all.equal(besselK(1,0,structure(1,class="besselk_probe")),c(a[1],b[1]),tolerance=1e-14)))
events <- character()
z <- withCallingHandlers(besselK(-1,0),warning=function(w){stopifnot(identical(conditionCall(w),quote(besselK(-1,0))));events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(is.nan(z),identical(events,c("value out of range in 'bessel_k'","NaNs produced")))
old <- options(warn=2);z <- tryCatch(besselK(-1,0),error=function(e)conditionMessage(e));options(old);stopifnot(is.character(z),abs(besselK(1,0)-a[1])<1e-14)
for(n in c(1,2.25)) for(s in c(FALSE,TRUE)) {
  events <- character()
  z <- withCallingHandlers(besselK(1e-320,n,s),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
  expected <- if(n == 1) "bessel_k(9.99989e-321): ncalc (=-1) != nb (=2); alpha=0. Arg. out of range?" else "bessel_k(9.99989e-321): ncalc (=-1) != nb (=3); alpha=0.25. Arg. out of range?"
  stopifnot(length(z)==1, identical(events,expected))
}
old <- options(warn=2); z <- tryCatch(besselK(1e-320,1),error=function(e)conditionMessage(e));options(old)
stopifnot(is.character(z),grepl("ncalc",z,fixed=TRUE),abs(besselK(1,0)-a[1])<1e-14)
for(sc in c(0,3)) {
  events <- character(); z <- withCallingHandlers(.Internal(besselK(1,1,sc)),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
  stopifnot(length(z)==1, length(events)>=1, identical(events[1],"bessel_k(1): ncalc (=-2) != nb (=2); alpha=0. Arg. out of range?"))
}
for(f in list(function()besselK("x",0),function()besselK(1,"x"),function().Internal(besselK(1,0,"x")),function()besselK(1+1i,0)))stopifnot(is.character(tryCatch(f(),error=function(e)conditionMessage(e))))
# GNU reuses the initialized work array between internal vector elements.
prior <- suppressWarnings(.Internal(besselK(c(1,1),0,c(1,3))))
stopifnot(identical(prior[1],prior[2]))
events <- character()
prior_grown <- withCallingHandlers(.Internal(besselK(1,c(0,2,0,2),c(1,1,3,3))), warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
stopifnot(identical(prior_grown[1:2],prior_grown[3:4]),length(events)==2)
frac <- .Internal(besselK(1,0,c(1.1,1.9,2.1,2.9)))
canonical <- .Internal(besselK(1,0,c(1,1,2,2)))
stopifnot(identical(frac,canonical))
cat("besselk-checked-contract-ok\n")
"#;
#[test]
fn public_besselk_checked() {
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("besselk-checked-contract-ok"), "{out}");
    }
}
