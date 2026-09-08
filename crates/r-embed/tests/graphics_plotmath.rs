use r_embed::RSession;

fn pixels(png: &[u8]) -> (Vec<u8>, usize, usize) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut bytes).unwrap();
    bytes.truncate(info.buffer_size());
    (bytes, info.width as usize, info.height as usize)
}
#[test]
fn expression_labels_typeset_fraction_scripts_and_radicals() {
    let mut session = RSession::new().unwrap();
    let png=session.render_with_dimensions("plot.new(); plot.window(xlim=c(0,1), ylim=c(0,1)); text(.5,.5,expression(frac(alpha[1]^2, sqrt(beta))),cex=2)",320,240).unwrap();
    let (rgba, width, height) = pixels(&png);
    let mut ink = 0;
    let mut min_y = height;
    let mut max_y = 0;
    for y in 0..height {
        for x in 0..width {
            let p = &rgba[(y * width + x) * 4..][..4];
            if p[0] < 200 && p[1] < 200 && p[2] < 200 {
                ink += 1;
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    assert!(
        ink > 50,
        "math must contain rendered glyphs and fraction/radical paths"
    );
    assert!(
        max_y - min_y > 25,
        "fraction must stack numerator and denominator"
    );
}
#[test]
fn expressions_work_in_titles_axes_and_replay() {
    let mut session = RSession::new().unwrap();
    session.render_with_dimensions("plot(1:3,axes=FALSE,main=expression(alpha^2),xlab=expression(sqrt(x)),ylab=expression(frac(a,b))); axis(1,at=1:3,labels=expression(alpha,beta,gamma)); title(sub=expression(bold(x)+italic(y))); p <- recordPlot()",400,300).unwrap();
    let replay = session
        .render_with_dimensions("replayPlot(p)", 400, 300)
        .unwrap();
    assert!(replay.len() > 1000);
}
#[test]
fn unsupported_math_fails_and_session_recovers() {
    let mut session = RSession::new().unwrap();
    let error = session
        .render_with_dimensions(
            "plot.new();text(.5,.5,expression(unsupported_math(x)))",
            320,
            240,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("unsupported plotmath operator"),
        "{error}"
    );
    session
        .render_with_dimensions("plot.new();text(.5,.5,expression(alpha))", 320, 240)
        .unwrap();
}

#[test]
fn expression_scene_contains_greek_and_independently_positioned_scripts() {
    use r_graphics_engine::{DrawOperation, FontFace, Scene};
    let mut session = rmath::android::RSession::new();
    let mut scene = Scene::new(320, 240);
    let result = session.eval_script_with_renderplot_backend(
        "plot.new();text(.5,.5,expression(frac(bold(alpha[1]^2),beta)))",
        &mut scene,
    );
    assert!(
        !matches!(result.typed, rmath::android::RValue::Error(_)),
        "{}",
        result.output
    );
    let texts: Vec<_> = scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            DrawOperation::Text {
                text,
                position,
                params,
            } => Some((text, position, params)),
            _ => None,
        })
        .collect();
    let alpha = texts
        .iter()
        .find(|(t, _, _)| t.as_str() == "α")
        .expect("Greek alpha");
    let beta = texts
        .iter()
        .find(|(t, _, _)| t.as_str() == "β")
        .expect("Greek beta");
    let sub = texts.iter().find(|(t, _, _)| t.as_str() == "1").unwrap();
    let sup = texts.iter().find(|(t, _, _)| t.as_str() == "2").unwrap();
    assert!(alpha.1.y < beta.1.y);
    assert!(sub.1.y > alpha.1.y && sup.1.y < alpha.1.y);
    assert!(sub.2.font_size < alpha.2.font_size);
    assert_eq!(alpha.2.font_face, FontFace::Bold);
    assert_eq!(beta.2.font_face, FontFace::Plain);
    assert!(
        scene
            .operations()
            .iter()
            .any(|op| matches!(op,DrawOperation::Path(path) if path.stroke.width>0.))
    );
}
