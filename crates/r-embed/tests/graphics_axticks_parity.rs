use r_embed::{RSession, RuntimePathPolicy};

const AX_TICKS_SCRIPT: &str = r##"
    plot(1:100, log="y", ylim=c(100,10))
    stopifnot(identical(par("yaxp"), c(100,10,3)))
    stopifnot(identical(axTicks(2), 10*c(10,5,2,1)))

    plot(1:100, log="y")
    stopifnot(identical(par("yaxp"), c(1,100,3)))
    stopifnot(identical(axTicks(2), c(1,2,5,10,20,50,100)))

    plot(1:100, log="x", xlim=c(100,1))
    stopifnot(identical(par("xaxp"), c(100,1,3)))
    stopifnot(identical(axTicks(1), c(100,50,20,10,5,2,1)))

    plot(1:100)
    stopifnot(identical(par("yaxp"), c(0,100,5)))
    stopifnot(identical(axTicks(2), c(0,20,40,60,80,100)))

    plot(1:3, rep(10,3), log="y")
    stopifnot(identical(par("yaxp"), c(5,25,-4)))
    stopifnot(identical(axTicks(2), c(5,10,15,20,25)))

    plot(1:3, rep(10,3))
    stopifnot(identical(par("yaxp"), c(6,14,4)))
    stopifnot(identical(axTicks(2), c(6,8,10,12,14)))

    nonfinite <- tryCatch(plot(1:3, ylim=c(NA_real_,1)), error=function(error) error)
    stopifnot(inherits(nonfinite, "error"))
    stopifnot(identical(conditionMessage(nonfinite), "need finite 'ylim' values"))
    cat("graphics-axticks-parity: PASS\n")
    "##;

fn check_ax_ticks(session: &mut RSession) {
    session
        .render_with_dimensions(AX_TICKS_SCRIPT, 320, 240)
        .expect("graphics axis state renders and matches GNU");
}

#[test]
fn native_plot_installs_gnu_log_and_linear_axis_parameters() {
    let mut session = RSession::new().unwrap();
    check_ax_ticks(&mut session);
}

#[test]
fn portable_plot_installs_gnu_log_and_linear_axis_parameters() {
    let mut session = RSession::new_with_path_policy(RuntimePathPolicy::new(
        Vec::new(),
        std::env::temp_dir(),
    ))
    .unwrap();
    check_ax_ticks(&mut session);
}
