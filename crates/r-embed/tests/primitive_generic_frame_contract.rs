use r_embed::{RSession, RuntimePathPolicy};

const CONTROLS: &str = r#"
events <- character()
as.logical.frame_probe_a <- function(x, ...) {
    calls <- tail(sys.calls(), 2)
    stopifnot(identical(vapply(calls, function(z) as.character(z[[1]]), ''), c('as.logical','as.logical.frame_probe_a')))
    stopifnot(identical(sys.call(sys.parent()),quote(frame_probe_outer())))
    on.exit(events <<- c(events,'exit'))
    gc()
    stopifnot(identical(as.logical(structure(1,class='frame_probe_nested')),TRUE))
    events <<- c(events,'first')
    warning('frame warning')
    NextMethod()
}
as.logical.frame_probe_b <- function(x, ...) { events <<- c(events,'second'); TRUE }
as.logical.frame_probe_nested <- function(x, ...) { gc(); TRUE }
frame_probe_outer <- function() as.logical(structure(1,class=c('frame_probe_a','frame_probe_b')))
result <- withCallingHandlers(frame_probe_outer(),warning=function(w) { stopifnot(identical(conditionMessage(w),'frame warning')); events <<- c(events,'warning'); gc(); invokeRestart('muffleWarning') })
stopifnot(identical(result,TRUE),identical(events,c('first','warning','second','exit')))
cat('primitive-generic-frame-contract-ok\n')
"#;

#[test]
fn primitive_generic_frames_and_nextmethod() {
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap(),
    ] {
        let output = session.eval(CONTROLS).unwrap();
        assert!(
            output.contains("primitive-generic-frame-contract-ok"),
            "{output}"
        );
    }
}
