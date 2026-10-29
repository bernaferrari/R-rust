use r_embed::{RSession, RuntimePathPolicy};

// GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
stopifnot(abs(dnorm(0,log="1")+.91893853320467278)<1e-14,
          abs(pnorm(1,lower.tail="0")-.15865525393145705)<1e-14,
          abs(qnorm(.25,lower.tail="0")-.67448975019608171)<1e-14,
          identical(dnorm(0,log=NULL),dnorm(0,log=TRUE)),
          identical(pnorm(1,log.p=NULL),pnorm(1,log.p=TRUE)))
for(flag in list(character(0),numeric(0),integer(0),list(1),"1",1.5)) {
    stopifnot(identical(dnorm(c(0,1),log=flag),dnorm(c(0,1),log=TRUE)))
}
for(flag in list("0",0L,.5,-.5)) {
    stopifnot(identical(dnorm(c(0,1),log=flag),dnorm(c(0,1),log=FALSE)))
}
events <- character()
z <- withCallingHandlers(dnorm(c(0,1),log="bad"),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(z,dnorm(c(0,1),log=TRUE)),identical(events,"NAs introduced by coercion"))
events <- character()
z <- withCallingHandlers(pnorm(1,lower.tail=4+1i,log.p="bad"),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(z,pnorm(1,lower.tail=TRUE,log.p=TRUE)),identical(events,c("imaginary parts discarded in coercion","NAs introduced by coercion")))
events <- character()
z <- withCallingHandlers(pnorm(1,lower.tail=numeric(0),log.p="bad"),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(z,pnorm(1,log.p=TRUE)),identical(events,"NAs introduced by coercion"))
for(z in list(function()dnorm(numeric(0),log="bad"),function()dnorm(1,sd=numeric(0),log="bad"),function()pnorm(numeric(0),lower.tail="bad",log.p="bad"),function()dexp(1,rate=numeric(0),log="bad"),function()dnorm(numeric(0),log=function()0))) {
    events <- character()
    value <- withCallingHandlers(z(),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
    stopifnot(identical(value,numeric(0)),length(events)==0L)
}
stopifnot(identical(dchisq(2,3,log="1"),dchisq(2,3,log=TRUE)),
          identical(dhyper(2,5,8,4,log="1"),dhyper(2,5,8,4,log=TRUE)),
          identical(pnbinom(2,size=3,mu=4,lower.tail="0",log.p="1"),pnbinom(2,size=3,mu=4,lower.tail=FALSE,log.p=TRUE)))
cat("distribution-flag-contract-ok\n")
"#;

#[test]
fn public_distribution_flags_match_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(output.contains("distribution-flag-contract-ok"), "{output}");
    }
}
