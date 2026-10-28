use r_embed::RSession;

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G']) && bytes.len() > 256
}

#[test]
fn render_wrapper_uses_lazy_renderplot_device_and_restores_state() {
    let mut session = RSession::new().expect("session");

    let blank = session
        .render_with_dimensions("1", 120, 100)
        .expect("non-plot render");
    assert!(blank.starts_with(&[0x89, b'P', b'N', b'G']));
    assert_eq!(session.eval("unname(grDevices::dev.cur())").unwrap(), "[1] 1\n");

    let plot = session
        .render_with_dimensions(
            "plot(1, 1); render_device_during_plot <- as.character(.Device)",
            120,
            100,
        )
        .expect("plot render");
    assert!(is_png(&plot));
    assert_eq!(
        session
            .eval("identical(render_device_during_plot, 'renderplot')")
            .unwrap(),
        "[1] TRUE\n"
    );
    assert_eq!(session.eval("unname(grDevices::dev.cur())").unwrap(), "[1] 1\n");

    let error = session
        .render_with_dimensions("plot(1, 1); stop('render cleanup probe')", 120, 100)
        .expect_err("render error should surface");
    assert!(error.to_string().contains("render cleanup probe"));
    assert_eq!(session.eval("unname(grDevices::dev.cur())").unwrap(), "[1] 1\n");

    session
        .eval(
            concat!(
                "render_saved_device_option <- getOption('device'); ",
                "render_custom_path <- tempfile(fileext='.pdf'); ",
                "render_custom_called <- FALSE; ",
                "options(device=function(...) { ",
                "render_custom_called <<- TRUE; ",
                "grDevices::pdf(file=render_custom_path, ...) })"
            ),
        )
        .expect("install custom device option");
    let custom_plot = session
        .render_with_dimensions("plot(1, 1)", 120, 100)
        .expect("custom device render");
    assert!(is_png(&custom_plot));
    assert_eq!(session.eval("render_custom_called").unwrap(), "[1] TRUE\n");
    assert_eq!(session.eval("unname(grDevices::dev.cur())").unwrap(), "[1] 1\n");

    session
        .eval(
            concat!(
                "render_custom_called <- FALSE; ",
                "render_previous_path <- tempfile(fileext='.pdf'); ",
                "grDevices::pdf(file=render_previous_path); ",
                "render_previous_device <- grDevices::dev.cur()"
            ),
        )
        .expect("open prior device");
    let restored_plot = session
        .render_with_dimensions("plot(1, 1)", 120, 100)
        .expect("render with prior device open");
    assert!(is_png(&restored_plot));
    assert_eq!(session.eval("render_custom_called").unwrap(), "[1] TRUE\n");
    assert_eq!(
        session
            .eval("identical(grDevices::dev.cur(), render_previous_device)")
            .unwrap(),
        "[1] TRUE\n"
    );

    session
        .eval(
            concat!(
                "grDevices::dev.off(); options(device=render_saved_device_option); ",
                "unlink(c(render_custom_path, render_previous_path))"
            ),
        )
        .expect("restore device option and close files");
    assert_eq!(session.eval("unname(grDevices::dev.cur())").unwrap(), "[1] 1\n");
}
