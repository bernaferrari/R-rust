use r_embed::RSession;

fn render(command: &str) -> Vec<u8> {
    let mut session = RSession::new().unwrap();
    session.render_with_dimensions(&format!("par(xaxs='i',yaxs='i');plot.new();plot.window(c(1,100),c(1,100),log='xy');{command}"), 240, 180).unwrap()
}

#[test]
fn default_log_abline_matches_transformed_space_endpoints() {
    // GNU abline(a=1,b=.5) on log-log axes is y=10*x^.5.
    assert_eq!(
        render("abline(1,.5,col='red')"),
        render("segments(1,10,100,100,col='red')")
    );
}

#[test]
fn untransformed_log_abline_follows_original_coordinate_curve() {
    let curved = render("abline(1,1,untf=TRUE,col='red')");
    assert_ne!(curved, render("abline(1,1,col='red')"));
}

#[test]
fn nonfinite_abline_coefficients_error_and_session_recovers() {
    let mut session = RSession::new().unwrap();
    for code in ["plot(1:3);abline(NA,1)", "plot(1:3);abline(1,Inf)"] {
        let error = session.render_with_dimensions(code, 160, 120).unwrap_err();
        assert!(error.to_string().contains("must be finite"));
        assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
    }
}
