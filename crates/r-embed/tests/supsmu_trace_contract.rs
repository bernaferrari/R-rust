use r_embed::RSession;

const DATA: &str = r#"
x <- seq(0, 1, length.out=8)
y <- c(0, 1, 0, 2, 1, 3, 2, 4)
"#;

#[test]
fn fixed_trace_matches_gnu_and_automatic_trace_keeps_all_eight_events() {
    let mut session = RSession::new().unwrap();

    let fixed = session
        .eval_script(&format!(
            "{DATA}\nexpected <- stats::supsmu(x, y, span=.25, trace=FALSE); actual <- stats::supsmu(x, y, span=.25, trace=TRUE); stopifnot(identical(actual, expected)); invisible(NULL)"
        ))
        .expect("fixed-span supersmoother call");
    assert_eq!(
        fixed.output,
        "smooth(span=0.25, iper=+1) -> (var, cvar) = (0.204082, 0.714286)\n"
    );

    let automatic = session
        .eval_script(&format!(
            "{DATA}\nexpected <- stats::supsmu(x, y, trace=FALSE); actual <- stats::supsmu(x, y, trace=TRUE); stopifnot(identical(actual, expected)); invisible(NULL)"
        ))
        .expect("automatic-span supersmoother call");
    let events: Vec<_> = automatic.output.lines().collect();
    assert_eq!(events.len(), 8, "automatic trace event count");
    let expected = [
        "smooth(span=0.05, iper=+1) -> (var, cvar) = (0.204082, 0.714286)\n",
        "smooth(span= 0.2, iper=-1) -> (var, cvar) = (0.204082, 0.290816)\n",
        "smooth(span= 0.2, iper=+1) -> (var, cvar) = (0.204082, 0.714286)\n",
        "smooth(span= 0.2, iper=-1) -> (var, cvar) = (0.204082, 0.290816)\n",
        "smooth(span= 0.5, iper=+1) -> (var, cvar) = (0.204082, 0.714286)\n",
        "smooth(span= 0.2, iper=-1) -> (var, cvar) = (0.204082, 0.290816)\n",
        "smooth(span= 0.2, iper=-1) -> (var, cvar) = (0.204082, -2.08167e-17)\n",
        "smooth(span=0.05, iper=-1) -> (var, cvar) = (0.204082, 0.757143)\n",
    ];
    for (event, expected) in events.iter().zip(expected.iter()) {
        assert_eq!(*event, expected.trim_end());
    }
}

#[test]
fn periodic_and_trace_disabled_calls_keep_owner_output_isolated() {
    let mut traced = RSession::new().unwrap();
    let mut silent = RSession::new().unwrap();

    let periodic = traced
        .eval_script(&format!(
            "{DATA}\nexpected <- stats::supsmu(x, y, span=.25, periodic=TRUE, trace=FALSE); actual <- stats::supsmu(x, y, span=.25, periodic=TRUE, trace=TRUE); stopifnot(identical(actual, expected)); invisible(NULL)"
        ))
        .expect("periodic supersmoother call");
    assert_eq!(
        periodic.output,
        "smooth(span=0.25, iper=+2) -> (var, cvar) = (0.106122, -0.428571)\n"
    );

    let quiet = silent
        .eval_script(&format!(
            "{DATA}\ninvisible(stats::supsmu(x, y, span=.25, trace=FALSE))"
        ))
        .expect("trace-disabled call in the other runtime");
    assert!(quiet.output.is_empty());

    let again = traced
        .eval_script(&format!(
            "{DATA}\ninvisible(stats::supsmu(x, y, span=.25, periodic=TRUE, trace=TRUE))"
        ))
        .expect("original runtime remains usable");
    assert_eq!(again.output, periodic.output);
}
