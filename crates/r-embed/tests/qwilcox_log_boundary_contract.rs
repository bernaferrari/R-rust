use r_embed::{RSession, RuntimePathPolicy};
// GNU R bac583951b728e97b9786804d3b4081f0fe18df5 (r90451).
const CONTROLS: &str = r#"
for(lower in c(TRUE,FALSE)) {
    expected <- if(lower) 12 else 0
    stopifnot(identical(qwilcox(0,3,4,lower.tail=lower,log.p=TRUE),expected),
              identical(qwilcox(-0,3,4,lower.tail=lower,log.p=TRUE),expected),
              identical(qwilcox(log(.5),3,4,lower.tail=lower,log.p=TRUE),6),
              identical(qwilcox(log(.99),3,4,lower.tail=lower,log.p=TRUE),expected))
    expected.ordinary <- if(lower)c(0,6,12)else c(12,6,0)
    stopifnot(identical(qwilcox(c(0,.5,1),3,4,lower.tail=lower),expected.ordinary),
              identical(qwilcox(0,.51,4,lower.tail=lower,log.p=TRUE),if(lower)4 else 0))
    for(p in c(-Inf,Inf,.1)) {
        events <- character()
        value <- withCallingHandlers(qwilcox(p,3,4,lower.tail=lower,log.p=TRUE),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
        stopifnot(is.nan(value),identical(events,"NaNs produced"))
    }
    for(m in c(0,-1,.49)) {
        events <- character()
        value <- withCallingHandlers(qwilcox(0,m,4,lower.tail=lower,log.p=TRUE),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
        stopifnot(is.nan(value),identical(events,"NaNs produced"))
    }
    events <- character()
    value <- withCallingHandlers(qwilcox(0,c(NA_real_,NaN),4,lower.tail=lower,log.p=TRUE),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
    stopifnot(is.na(value[1]),!is.nan(value[1]),is.nan(value[2]),length(events)==0)
}
cat("qwilcox-log-boundary-contract-ok\n")
"#;
#[test]
fn public_qwilcox_log_boundaries_match_gnu() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(
            output.contains("qwilcox-log-boundary-contract-ok"),
            "{output}"
        );
    }
}
