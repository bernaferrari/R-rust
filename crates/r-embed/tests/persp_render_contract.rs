use r_embed::RSession;
use r_graphics_engine::{DrawOperation, Scene};

#[test]
fn persp_returns_gnu_view_projection_and_records_an_owned_scene() {
    let mut session = RSession::new().unwrap();
    let mut scene = Scene::new(504, 504);
    let mut regression_scene = Scene::new(504, 504);
    let mut color_scene = Scene::new(504, 504);

    // Keep the original reg-tests-1a expression literal and render it alone
    // so the numeric axis label can be inspected on its own device.
    session
        .render_to(
            "regression <- persp(1:2, 1:2, matrix(1:4, 2), xlab=1)",
            &mut regression_scene,
        )
        .unwrap();

    session
        .render_to(
            r#"
                persp(
                    1:3, 1:3, matrix(c(1, 2, 1, 2, 3, 2, 1, 2, 1), 3),
                    xlab = "x", ylab = "y", zlab = "z",
                    theta = 45, phi = 25, col = c("red", "blue"),
                    border = c("black", "green"), shade = 0.6
                )
            "#,
            &mut color_scene,
        )
        .unwrap();

    session
        .render_to(
            r#"
                view <- persp(
                    1:2, 1:2, matrix(1:4, 2), xlab = 1, ylab = "Y",
                    zlab = "height", theta = 30, phi = 20,
                    box = TRUE, axes = TRUE, ticktype = "detailed",
                    col = c("red", "blue"), border = "black"
                )
            "#,
            &mut scene,
        )
        .unwrap();

    // Values captured from the pinned R 4.7.0 (r-source bac583951b728e97b9786804d3b4081f0fe18df5)
    // persp() and trans3d() implementations. Check both public results after
    // the native draw callback has run.
    assert_eq!(
        session
            .eval(
                r#"
                stopifnot(
                    is.matrix(regression), identical(dim(regression), c(4L, 4L)),
                    all(is.finite(regression)),
                    is.matrix(view), identical(dim(view), c(4L, 4L)),
                    max(abs(
                        c(view[1,1], view[2,2], view[4,4],
                          view[1,4], view[2,4], view[3,4]) -
                        c(1.73205080756887742, 0.59239626545204782,
                          3.27023026690906615, -0.93969262078590832,
                          1.62759536269874761, -0.22801342888377915)
                    )) < 1e-12
                )
                projection <- trans3d(c(1, 2), c(1, 2), c(1, 4), view)
                stopifnot(max(abs(c(projection$x, projection$y) -
                    c(-0.36621490933732959, 0.36583609425732205,
                      -0.28548164717731700, 0.28518634310787905))) < 1e-12)
                cat("persp-view-projection: PASS\n")
                "#
            )
            .unwrap(),
        "persp-view-projection: PASS\n"
    );

    let regression_labels: Vec<_> = regression_scene
        .operations()
        .iter()
        .filter_map(|operation| match operation {
            DrawOperation::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        regression_labels.contains(&"1"),
        "numeric xlab from the original case: {regression_labels:?}"
    );

    let paths: Vec<_> = scene
        .operations()
        .iter()
        .filter_map(|operation| match operation {
            DrawOperation::Path(path) => Some(path),
            _ => None,
        })
        .collect();
    assert!(paths.len() > 20, "persp should record facets, box and axes");
    let color_fills: Vec<_> = color_scene
        .operations()
        .iter()
        .filter_map(|operation| match operation {
            DrawOperation::Path(path) => Some(path.fill),
            _ => None,
        })
        .collect();
    assert!(
        color_fills
            .iter()
            .any(|color| color.r > color.g && color.r > color.b),
        "a facet should retain the red-dominant fill: {color_fills:?}"
    );
    assert!(
        color_fills
            .iter()
            .any(|color| color.b > color.r && color.b > color.g),
        "a facet should retain the blue-dominant fill: {color_fills:?}"
    );

    let labels: Vec<_> = scene
        .operations()
        .iter()
        .filter_map(|operation| match operation {
            DrawOperation::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(labels.contains(&"Y"), "Y axis label: {labels:?}");
    assert!(labels.contains(&"height"), "z axis label: {labels:?}");

    // GNU omits NA integer/real labels but prints an ordinary NaN label.
    let mut missing_label_scene = Scene::new(504, 504);
    session
        .render_to(
            "persp(1:2, 1:2, matrix(1:4, 2), xlab=NA_integer_, ylab=NA_real_, zlab=NaN)",
            &mut missing_label_scene,
        )
        .unwrap();
    let missing_labels: Vec<_> = missing_label_scene
        .operations()
        .iter()
        .filter_map(|operation| match operation {
            DrawOperation::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !missing_labels.contains(&"NA"),
        "numeric NA labels are omitted: {missing_labels:?}"
    );
    assert!(
        missing_labels.contains(&"NaN"),
        "ordinary NaN is a printable label: {missing_labels:?}"
    );

    let operation_count = scene.operations().len();
    session.close();
    assert_eq!(scene.operations().len(), operation_count);
    assert!(scene.operations().iter().any(|operation| matches!(
        operation,
        DrawOperation::Text { text, .. } if text == "height"
    )));
}
