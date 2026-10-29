use r_embed::{RSession, RuntimePathPolicy};
// Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5 r90451.
const CONTROLS: &str = r#"
for(f in list(choose,lchoose)) {
 events <- character()
 z <- withCallingHandlers(f(c(5,NA_real_,NaN,6,-1),c(1.2,2.3,3.4,2.6,-1.2)),warning=function(w){events <<- c(events,conditionMessage(w));gc();stopifnot(identical(choose(6,2),15));invokeRestart("muffleWarning")})
 target <- if(identical(f,choose))c(5,NA_real_,NaN,20,0)else c(log(5),NA_real_,NaN,log(20),-Inf)
 stopifnot(isTRUE(all.equal(z,target,tolerance=1e-14)),identical(events,c("'k' (1.20) must be integer, rounded to 1","'k' (2.60) must be integer, rounded to 3","'k' (-1.20) must be integer, rounded to -1")))
 events <- character()
 z <- withCallingHandlers(f(rep(5,8),c(.5,1.5,2.5,-.5,-1.5,1+1e-10,1+2e-9,Inf)),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
 stopifnot(identical(events,c("'k' (0.50) must be integer, rounded to 0","'k' (1.50) must be integer, rounded to 2","'k' (2.50) must be integer, rounded to 2","'k' (-0.50) must be integer, rounded to -0","'k' (-1.50) must be integer, rounded to -2","'k' (1.00) must be integer, rounded to 1")))
 events <- character()
 z <- withCallingHandlers(f(c(Inf,5),c(Inf,1.2)),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
 stopifnot(is.nan(z[1]),identical(events,c("'k' (1.20) must be integer, rounded to 1","NaNs produced")))
 stopifnot(identical(f(numeric(0),1.2),numeric(0)),identical(f(5,numeric(0)),numeric(0)))
}
call.text <- ""
withCallingHandlers(choose(5,1.2),warning=function(w){call.text <<- paste(deparse(conditionCall(w)),collapse=" ");invokeRestart("muffleWarning")})
stopifnot(identical(call.text,"choose(5, 1.2)"))
x <- structure(c(5,6),names=c("a","b"),class="foo")
z <- suppressWarnings(choose(x,c(1.2,2.6)));gc()
stopifnot(identical(z,structure(c(5,20),names=c("a","b"),class="foo")))
old <- options(warn=2)
message <- tryCatch(choose(5,1.2),error=conditionMessage)
options(old)
stopifnot(identical(message,"(converted from warning) 'k' (1.20) must be integer, rounded to 1"),identical(choose(6,2),15))
cat("choose-rounding-warning-contract-ok\n")
"#;
#[test]
fn public_choose_rounding_warnings() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("choose-rounding-warning-contract-ok"), "{out}");
    }
}
