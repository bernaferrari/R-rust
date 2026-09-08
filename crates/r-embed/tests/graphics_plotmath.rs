use r_embed::RSession;

#[test]
fn gaussian_density_website_expression_renders_natively() {
    let mut session = RSession::new().unwrap();
    session
        .render_with_dimensions(
            r#"mu <- 0; sigma <- 1; x <- seq(-4,4,length.out=300); y <- dnorm(x,mean=mu,sd=sigma); plot(x,y,type='n',ylim=c(0,.62)); inside <- seq(-sigma,sigma,length.out=100)+mu; polygon(c(inside[1],inside,inside[length(inside)]),c(0,dnorm(inside,mu,sigma),0),col='#93bfae',border=NA); lines(x,y,col='#43877b',lwd=3); text(0,.52,expression(f(x)==frac(1,sigma*sqrt(2*pi))*e^(-frac((x-mu)^2,2*sigma^2))),cex=1.3,col='#bc8060'); probability <- pnorm(mu+sigma,mu,sigma)-pnorm(mu-sigma,mu,sigma); text(0,.13,paste(round(100*probability,1),'%'),cex=1.6,col='#244e44')"#,
            640,
            480,
        )
        .unwrap();
}

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
fn invalid_math_arity_fails_and_session_recovers() {
    let mut session = RSession::new().unwrap();
    let error = session
        .render_with_dimensions("plot.new();text(.5,.5,expression(frac(x)))", 320, 240)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("plotmath 'frac' requires 2 arguments"),
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
    assert_eq!(alpha.2.font_face, FontFace::Plain);
    assert_eq!(sub.2.font_face, FontFace::Plain);
    assert_eq!(beta.2.font_face, FontFace::Plain);
    assert!(
        scene
            .operations()
            .iter()
            .any(|op| matches!(op,DrawOperation::Path(path) if path.stroke.width>0.))
    );
}

#[test]
fn accents_operators_and_fixed_groups_draw_owned_geometry() {
    use r_graphics_engine::{DrawOperation, Scene};
    let mut session = rmath::android::RSession::new();
    let mut scene = Scene::new(500, 300);
    let result=session.eval_script_with_renderplot_backend("plot.new();text(c(.2,.5,.8),c(.5,.5,.5),expression(bar(x)+underline(y),sum(x[i],i==1,n),group('(',hat(theta),' )')))",&mut scene);
    assert!(
        !matches!(result.typed, rmath::android::RValue::Error(_)),
        "{}",
        result.output
    );
    assert!(
        scene
            .operations()
            .iter()
            .filter(|op| matches!(op, DrawOperation::Path(_)))
            .count()
            >= 2
    );
    assert!(
        scene
            .operations()
            .iter()
            .any(|op| matches!(op,DrawOperation::Text{text,..} if text=="∑"))
    );
}

#[test]
fn latin_variables_default_to_plain_and_explicit_faces_work() {
    use r_graphics_engine::{DrawOperation, FontFace, Scene};
    let mut session = rmath::android::RSession::new();
    let mut scene = Scene::new(300, 200);
    let result = session.eval_script_with_renderplot_backend(
        "plot.new();text(.5,.5,expression(x+italic(y)+bold(z)))",
        &mut scene,
    );
    assert!(
        !matches!(result.typed, rmath::android::RValue::Error(_)),
        "{}",
        result.output
    );
    for (name, face) in [
        ("x", FontFace::Plain),
        ("y", FontFace::Italic),
        ("z", FontFace::Bold),
    ] {
        assert!(scene.operations().iter().any(|op|matches!(op,DrawOperation::Text{text,params,..} if text==name && params.font_face==face)));
    }
}

#[test]
fn tall_math_main_fits_above_plot_without_canvas_clipping() {
    use r_graphics_engine::{DrawOperation, DrawTarget, Scene};
    let mut session = rmath::android::RSession::new();
    let mut scene = Scene::new(640, 480);
    let result = session.eval_script_with_renderplot_backend(
        "plot(1:3,axes=FALSE,xlab='',ylab='',main=expression(frac(alpha[1]^2,sqrt(beta))))",
        &mut scene,
    );
    assert!(
        !matches!(result.typed, rmath::android::RValue::Error(_)),
        "{}",
        result.output
    );
    for op in scene.operations() {
        if let DrawOperation::Text {
            text,
            position,
            params,
        } = op
            && !text.is_empty()
        {
            let metrics = scene.measure_math_text(text, params);
            assert!(
                position.y - metrics.ascent >= 3.9,
                "title glyph '{text}' crosses canvas top"
            );
            assert!(
                position.y + metrics.descent <= 40.1,
                "title glyph '{text}' touches plot rectangle"
            );
        }
    }
}

#[test]
fn stretchy_groups_wide_accents_and_display_limits_emit_owned_geometry() {
    use r_graphics_engine::{DrawOperation, Scene};
    let mut session = rmath::android::RSession::new();
    let mut scene = Scene::new(640, 360);
    let result = session.eval_script_with_renderplot_backend(
        "plot.new();text(.5,.5,expression(bgroup('(',frac(alpha+beta,gamma+delta),')')+widehat(alpha+beta)+sum(x[i],i==1,n)))",
        &mut scene,
    );
    assert!(
        !matches!(result.typed, rmath::android::RValue::Error(_)),
        "{}",
        result.output
    );
    let paths = scene
        .operations()
        .iter()
        .filter(|op| matches!(op, DrawOperation::Path(_)))
        .count();
    assert!(paths >= 3, "fraction and widehat should emit paths");
    for glyph in ["⎛", "⎝", "⎞", "⎠"] {
        assert!(
            scene
                .operations()
                .iter()
                .any(|op| matches!(op, DrawOperation::Text { text, .. } if text == glyph)),
            "missing delimiter piece {glyph}"
        );
    }
    assert!(
        scene
            .operations()
            .iter()
            .any(|op| matches!(op, DrawOperation::Text { text, .. } if text == "∑"))
    );
}

#[test]
fn standard_plotmath_symbol_and_relation_catalog_renders() {
    use r_graphics_engine::{DrawOperation, Scene};
    let mut session = rmath::android::RSession::new();
    let mut scene = Scene::new(800, 320);
    let result = session.eval_script_with_renderplot_backend(
        "plot.new();text(.5,.5,expression(infinity + partialdiff + nabla + degree + arrowleft + arrowright + plusminus + notequal + lessequal + greaterequal + intersection + union + therefore + x %subset% y %notin% z %<->% w))",
        &mut scene,
    );
    assert!(
        !matches!(result.typed, rmath::android::RValue::Error(_)),
        "{}",
        result.output
    );
    for expected in [
        "∞", "∂", "∇", "°", "←", "→", "±", "≠", "≤", "≥", "∩", "∪", "∴", "⊂", "∉", "↔",
    ] {
        assert!(
            scene
                .operations()
                .iter()
                .any(|op| matches!(op, DrawOperation::Text { text, .. } if text == expected)),
            "missing plotmath symbol {expected}"
        );
    }
}

#[test]
fn named_limit_operators_use_display_layout() {
    use r_graphics_engine::{DrawOperation, Scene};
    let mut session = rmath::android::RSession::new();
    let mut scene = Scene::new(640, 320);
    let result = session.eval_script_with_renderplot_backend(
        "plot.new();text(.5,.5,expression(lim(x,x==0)+inf(x,i==1,n)+max(x,i==1,n)))",
        &mut scene,
    );
    assert!(
        !matches!(result.typed, rmath::android::RValue::Error(_)),
        "{}",
        result.output
    );
    for expected in ["lim", "inf", "max"] {
        assert!(
            scene
                .operations()
                .iter()
                .any(|op| matches!(op, DrawOperation::Text { text, .. } if text == expected))
        );
    }
    let ys: Vec<f32> = scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            DrawOperation::Text { text, params, .. }
                if ["0", "1", "n"].contains(&text.as_str()) =>
            {
                Some(params.font_size)
            }
            _ => None,
        })
        .collect();
    assert!(ys.iter().any(|size| *size < 12.), "limits use script size");
}
