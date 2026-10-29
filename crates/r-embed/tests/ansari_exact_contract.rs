use r_embed::{RSession, RuntimePathPolicy};

// GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
x <- c(1,2,9,10); y <- 3:8
for(a in c("two.sided","less","greater")) {
    expected <- switch(a,"two.sided"=1/105,"less"=1,"greater"=1/210)
    z <- ansari.test(x,y,alternative=a,exact=TRUE)
    stopifnot(identical(unname(z$statistic),6), abs(z$p.value-expected)<1e-14)
    z <- ansari.test(-x,-y,alternative=a,exact=TRUE)
    stopifnot(abs(z$p.value-expected)<1e-14)
}
native <- get("C_pAnsari",asNamespace("stats"))
q <- c(5:19,6-5e-8,6-2e-7)
expected <- c(0,1,5,14,30,56,88,122,154,180,196,205,209,210,210,1,0)/210
p <- .Call(native,q,4L,6L)
stopifnot(all(abs(p-expected)<1e-14), all(diff(p[1:15])>=0))
qnative <- get("C_qAnsari",asNamespace("stats"))
stopifnot(identical(.Call(qnative,c(0,1/210,5/210,.5,209/210,1),4L,6L),c(6,6,7,12,17,18)),
          identical(.Call(native,c(NA_real_,NaN,-Inf,Inf),4L,6L),c(0,0,0,1)),
          abs(.Call(native,"6","4","6")-1/210)<1e-14,
          identical(.Call(native,numeric(0),4L,6L),numeric(0)),
          identical(.Call(qnative,numeric(0),4L,6L),numeric(0)))
for(d in list(c(0L,0L),c(4L,0L),c(0L,6L))) {
    expected <- if(d[1]==4L) 6 else 0
    stopifnot(identical(.Call(qnative,c(0,.5,1),d[1],d[2]),rep(expected,3)))
}
warnings <- character()
z <- withCallingHandlers(.Call(native,6,4+1i,6),warning=function(w){warnings <<- c(warnings,conditionMessage(w));gc();invokeRestart("muffleWarning")})
stopifnot(abs(z-1/210)<1e-14,identical(warnings,"imaginary parts discarded in coercion"))
for(a in c("two.sided","less","greater")) {
    z <- ansari.test(c(-3,-1,1,3),c(-2.5,-1.5,-.5,.5,1.5,2.5),alternative=a,exact=TRUE,conf.int=TRUE)
    expected.p <- switch(a,"two.sided"=8/15,"less"=6/7,"greater"=4/15)
    expected.ci <- switch(a,"two.sided"=c(.4,6),"less"=c(0,6),"greater"=c(.4,Inf))
    stopifnot(abs(z$p.value-expected.p)<1e-14,identical(as.numeric(z$conf.int),expected.ci),abs(z$estimate-1.6)<1e-14)
}
cat("ansari-exact-contract-ok\n")
"#;

// Port-only safe admission controls; malformed native dimensions/NaN quantiles
// can overflow or fail to terminate in GNU and are not executed there.
const ADMISSION: &str = r#"
pnative <- get("C_pAnsari",asNamespace("stats")); qnative <- get("C_qAnsari",asNamespace("stats"))
for(routine in list(pnative,qnative)) {
    for(bad in list(-1,NA_real_,Inf,numeric(0),.Machine$integer.max)) {
        msg <- suppressWarnings(tryCatch(.Call(routine,.5,bad,6L),error=conditionMessage))
        stopifnot(is.character(msg),grepl("Ansari",msg))
        msg <- suppressWarnings(tryCatch(.Call(routine,.5,4L,bad),error=conditionMessage))
        stopifnot(is.character(msg),grepl("Ansari",msg))
    }
}
for(bad in c(-.1,1.1,NaN,NA_real_,Inf,-Inf)) {
    msg <- tryCatch(.Call(qnative,bad,4L,6L),error=conditionMessage)
    stopifnot(identical(msg,"probabilities outside [0,1] in qansari()"))
}
stopifnot(identical(.Call(qnative,.5,4L,6L),12),abs(.Call(pnative,6,4L,6L)-1/210)<1e-14)
cat("ansari-native-admission-ok\n")
"#;

#[test]
fn public_ansari_exact_p_values_match_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(output.contains("ansari-exact-contract-ok"), "{output}");
        let output = session.eval(ADMISSION).unwrap();
        assert!(output.contains("ansari-native-admission-ok"), "{output}");
    }
}
