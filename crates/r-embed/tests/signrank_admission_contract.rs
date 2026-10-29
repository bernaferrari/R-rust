use r_embed::{RSession, RuntimePathPolicy};
// GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
expected <- c(1,1,1,2,2,3,3,3,3,3,3,2,2,1,1,1)/32
stopifnot(all(abs(dsignrank(0:15,5)-expected)<1e-14),all(abs(psignrank(0:15,5)-cumsum(expected))<1e-14),
          all(abs(psignrank(0:15,5,lower.tail=FALSE)-(1-cumsum(expected)))<1e-14),
          all(abs(dsignrank(0:15,5,log=TRUE)-log(expected))<1e-14))
p <- c(0,.001,.25,.5,.75,.999,1)
stopifnot(identical(qsignrank(p,5),c(0,0,5,7,10,15,15)),identical(qsignrank(p,5,lower.tail=FALSE),c(15,15,10,7,5,0,0)))
for(n in c(1,2,5,10,5,1)) {
    x <- c(-1,0,n*(n+1)/2,n*(n+1)/2+1)
    stopifnot(identical(dsignrank(x,n),c(0,2^-n,2^-n,0)),identical(psignrank(c(-1,n*(n+1)/2),n),c(0,1)))
}
x <- structure(c(0,1),note=list(k=19),names=c("a","b"))
stopifnot(identical(attributes(dsignrank(x,5)),attributes(x)),identical(dsignrank(numeric(0),5),numeric(0)),
          identical(psignrank(numeric(0),5,lower.tail="bad"),numeric(0)),identical(qsignrank(0,5,log.p=TRUE),15))
events <- character()
z <- withCallingHandlers(psignrank(5,10,lower.tail="0",log.p="bad"),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(identical(z,psignrank(5,10,lower.tail=FALSE,log.p=TRUE)),identical(events,"NAs introduced by coercion"))
events <- character()
z <- withCallingHandlers(qsignrank(-Inf,5,log.p=TRUE),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(is.nan(z),identical(events,"NaNs produced"))
cat("signrank-admission-contract-ok\n")
"#;
// Port-only size admission: do not execute overflowing native dimensions in GNU.
const ADMISSION: &str = r#"
for(f in list(function()dsignrank(0,65536),function()psignrank(0,65536),function()qsignrank(.5,65536))) {
    msg <- tryCatch(f(),error=conditionMessage)
    stopifnot(identical(msg,"signed-rank sample size is too large"))
}
stopifnot(abs(dsignrank(5,10)-3/1024)<1e-14,identical(qsignrank(.75,10),34))
cat("signrank-size-admission-ok\n")
"#;
#[test]
fn public_signed_rank_counts_match_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(
            output.contains("signrank-admission-contract-ok"),
            "{output}"
        );
        let output = session.eval(ADMISSION).unwrap();
        assert!(output.contains("signrank-size-admission-ok"), "{output}");
    }
}
