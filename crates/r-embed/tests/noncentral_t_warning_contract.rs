use r_embed::{RSession, RuntimePathPolicy};

const CONTROLS: &str = r#"
stopifnot(abs(pt(1,10,ncp=2)-0.15851505953749501)<1e-12)
stopifnot(abs(pt(1,10,ncp=2,lower.tail=FALSE)-0.84148494046250499)<1e-12)
stopifnot(abs(pt(1,10,ncp=2,log.p=TRUE)-(-1.8419056773233791))<1e-12)
events <- character()
z <- withCallingHandlers(pt(2022.5067499243628,3015.4821127636951,ncp=.0025750311311942531), warning=function(w) {
    events <<- c(events,conditionMessage(w)); gc(); invokeRestart('muffleWarning')
})
stopifnot(abs(z-0.99999999999999989)<1e-14,identical(events,"full precision may not have been achieved in 'pnt{final}'"))
old <- options(warn=2)
message <- tryCatch(pt(2022.5067499243628,3015.4821127636951,ncp=.0025750311311942531),error=function(e) conditionMessage(e))
options(old)
stopifnot(identical(message,"(converted from warning) full precision may not have been achieved in 'pnt{final}'"),abs(pt(1,10,ncp=2)-0.15851505953749501)<1e-12)
cat('noncentral-t-warning-contract-ok\n')
"#;

#[test]
fn noncentral_t_warning_and_recovery() {
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(
            output.contains("noncentral-t-warning-contract-ok"),
            "{output}"
        );
    }
}
