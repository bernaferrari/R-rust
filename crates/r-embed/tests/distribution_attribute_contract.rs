use r_embed::{RSession, RuntimePathPolicy};

// GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
x <- matrix(c(-1,0,1,2),2,dimnames=list(c("a","b"),c("u","v")))
attr(x,"note") <- list(k=17)
z <- dnorm(x)
stopifnot(identical(attributes(z),attributes(x)),max(abs(as.vector(z)-c(.24197072451914337,.3989422804014327,.24197072451914337,.053990966513188063)))<1e-14)
a <- structure(c(0,1),note="main",names=c("x0","x1"))
b <- structure(c(0,0),names=c("m0","m1"),note="mean")
c <- structure(c(1,1,1),names=c("s0","s1","s2"),note="sd")
stopifnot(identical(attributes(dnorm(a,b,1)),attributes(a)),
          identical(attributes(dnorm(0,b,1)),attributes(b)),
          identical(attributes(dnorm(a,0,c)),attributes(c)),
          is.null(attributes(dnorm(c(0,1),b,1))))
e <- structure(numeric(0),note=list(k=23),names=character(0))
stopifnot(identical(attributes(dnorm(e)),attributes(e)),
          identical(attributes(pexp(e)),attributes(e)),
          identical(attributes(dhyper(e,2,3,2)),attributes(e)),
          is.null(attributes(dnorm(a,sd=e))))
class(a) <- "dpq.contract"
z <- dnorm(a)
stopifnot(identical(attributes(z),attributes(a)),is.object(z))
attr(z,"note") <- "changed"
stopifnot(identical(attr(a,"note"),"main"))
for(f in list(function(x)dexp(x),function(x)pexp(x),function(x)dnorm(x),function(x)pnorm(x),function(x)dhyper(x,5,8,4),function(x)phyper(x,5,8,4))) {
    source <- structure(c(0,1),note=list(child=c(19,23)),names=c("first","second"))
    result <- f(source)
    stopifnot(identical(attributes(result),attributes(source)))
}
events <- character()
source <- structure(c(1,2),note=list(k=47),names=c("one","two"))
result <- withCallingHandlers(dnorm(source,sd=-1),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(all(is.nan(result)),identical(attributes(result),attributes(source)),identical(events,"NaNs produced"))
rm(source);gc()
stopifnot(identical(attr(result,"note"),list(k=47)),identical(names(result),c("one","two")))
for(f in list(function()dnorm(factor(c("a","b"))),function()dnorm(1,mean=factor("a")),function()dnorm(factor(character(0))))) {
    msg <- tryCatch(f(),error=function(e){gc();conditionMessage(e)})
    stopifnot(identical(msg,"Non-numeric argument to mathematical function"))
}
stopifnot(identical(dnorm(TRUE),dnorm(1)))
cat("distribution-attribute-contract-ok\n")
"#;
#[test]
fn public_distribution_attributes_match_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(
            output.contains("distribution-attribute-contract-ok"),
            "{output}"
        );
    }
}
