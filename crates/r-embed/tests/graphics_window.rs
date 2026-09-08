use r_embed::RSession;

#[test]
fn plot_window_matches_positional_and_named_limits() {
    let mut session = RSession::new().unwrap();
    for call in [
        "plot.window(c(2,8), c(10,20))",
        "plot.window(ylim=c(10,20), c(2,8))",
        "plot.window(c(2,8), ylim=c(10,20))",
    ] {
        session
            .render_with_dimensions(
                &format!("par(xaxs='i',yaxs='i'); plot.new(); {call}"),
                160,
                120,
            )
            .unwrap();
        assert_eq!(
            session.eval("identical(par('usr'), c(2,8,10,20))").unwrap(),
            "[1] TRUE"
        );
    }
    session
        .render_with_dimensions("plot.new(); plot.window(c(1,100), c(10,20), 'x')", 160, 120)
        .unwrap();
    assert_eq!(session.eval("par('xlog')").unwrap(), "[1] TRUE");
}
