use r_embed::{RSession, RuntimePathPolicy};
use r_graphics_engine::DrawOperation;

#[test]
fn native_raster_draws_recycled_placements_and_recovers_after_bad_payloads() {
    // The pinned GNU oracle draws three actual PDF images for the valid
    // two-placement request plus retry, and reports invalid color names.
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap(),
    ] {
        let scene = session.record_scene(r#"
            plot.new(); plot.window(c(0,1),c(0,1))
            image <- matrix(c('red','blue','green','white'),2)
            v <- withVisible(.External.graphics('C_raster',image,c(0.1,0.6),0.1,c(0.4,0.9),0.4,c(0,30),FALSE))
            stopifnot(is.null(v$value),v$visible)
        "#,320,240).unwrap();
        let images: Vec<_> = scene
            .operations()
            .iter()
            .filter_map(|op| match op {
                DrawOperation::DrawImage {
                    image,
                    transform,
                    interpolate,
                } => Some((image, transform, interpolate)),
                _ => None,
            })
            .collect();
        assert_eq!(images.len(), 2, "native provider must record real images");
        assert_eq!(images[0].0.width(), 2);
        assert_eq!(images[0].0.height(), 2);
        assert_eq!(
            images[0].0.pixels(),
            &[
                255, 0, 0, 255, 0, 0, 255, 255, 0, 255, 0, 255, 255, 255, 255, 255
            ]
        );
        assert_eq!(images[0].0, images[1].0);
        assert_ne!(images[0].1, images[1].1);
        assert!(!images[0].2 && !images[1].2);

        // Malformed native dimensions must be rejected before reading pixels;
        // GNU's raw bridge does not provide a safe malformed-buffer oracle.
        for image in [
            "c('red','blue')",
            "matrix(character(),0,2)",
            "matrix('not-a-color',2,2)",
        ] {
            let code = format!(
                "plot.new();plot.window(c(0,1),c(0,1));.External.graphics('C_raster',{image},0,0,1,1,0,FALSE)"
            );
            assert!(session.record_scene(&code, 320, 240).is_err(), "{image}");
        }
        let retry=session.record_scene("plot.new();plot.window(c(0,1),c(0,1));.External.graphics('C_raster',matrix('red',2,2),0,0,1,1,0,FALSE)",320,240).unwrap();
        assert_eq!(
            retry
                .operations()
                .iter()
                .filter(|op| matches!(op, DrawOperation::DrawImage { .. }))
                .count(),
            1
        );
    }
}

#[test]
fn incomplete_native_graphics_operations_cannot_report_silent_success() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap();
    for (name, payload) in [
        ("C_filledcontour", "1,1,1,1,1"),
        ("C_convertX", "1,1,1"),
        ("C_convertY", "1,1,1"),
        ("C_persp", ""),
        ("C_clip", ""),
        ("C_dend", ""),
        ("C_dendwindow", ""),
        ("C_erase", ""),
        ("C_path", ""),
        ("C_symbols", ""),
        ("C_xspline", ""),
        ("C_locator", ""),
        ("C_identify", ""),
    ] {
        let comma = if payload.is_empty() { "" } else { "," };
        let code = format!(".External.graphics('{name}'{comma}{payload})");
        let error = session
            .record_scene(&code, 320, 240)
            .expect_err(name)
            .to_string();
        assert!(
            error.contains(name) && error.contains("not implemented"),
            "{error}"
        );
        assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
    }
}

#[test]
fn native_text_payload_reaches_owned_plotmath_and_style_drawing() {
    for mut session in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap(),
    ] {
        let scene = session.record_scene("plot.new();plot.window(c(0,1),c(0,1));.External.graphics('C_text',xy.coords(.5,.5),expression(alpha^2),NULL,NULL,.5,NULL,1,'red',1)",320,240).unwrap();
        assert!(scene.operations().iter().any(|op| matches!(op,DrawOperation::Text{text,params,..} if text=="α" && params.text_color==r_graphics_engine::Color::RED)));
        assert!(
            scene
                .operations()
                .iter()
                .any(|op| matches!(op,DrawOperation::Text{text,..} if text=="2"))
        );
    }
}
