//! Public GNU vector/table/matrix contracts for the ordinary R barplot producer.
use crate::sexp::{RSession, ffi::SEXPTYPE, object::Sexp};

fn dimensions(value: &Sexp<'_>) -> Vec<i32> {
    let mut attributes = value.try_attrib().unwrap();
    while !attributes.is_nil() {
        if attributes.try_tag_name_eq(b"dim").unwrap() {
            let dim = attributes.try_car().unwrap();
            return (0..dim.len())
                .map(|index| dim.try_integer_elt(index).unwrap())
                .collect();
        }
        attributes = attributes.try_cdr().unwrap();
    }
    Vec::new()
}

fn midpoint_values(value: &Sexp<'_>) -> Vec<f64> {
    assert_eq!(value.typeof_(), SEXPTYPE::REALSXP);
    (0..value.len())
        .map(|index| value.try_real_elt(index).unwrap())
        .collect()
}

fn assert_midpoints(value: &Sexp<'_>, expected: &[f64]) {
    let actual = midpoint_values(value);
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            (actual - expected).abs() <= 1e-12,
            "midpoint {actual:.17} differs from pinned GNU {expected:.17}"
        );
    }
}

#[test]
fn owning_barplot_public_midpoint_shapes_match_gnu() {
    let mut session = RSession::new_without_default_packages();
    for (code, shape, midpoints) in [
        (
            "barplot(c(A=2,B=4,C=3),plot=FALSE)",
            vec![3, 1],
            vec![0.7, 1.9, 3.1],
        ),
        (
            "barplot(matrix(1:6,nrow=2),beside=TRUE,plot=FALSE)",
            vec![2, 3],
            vec![1.5, 2.5, 4.5, 5.5, 7.5, 8.5],
        ),
        (
            "barplot(matrix(1:6,nrow=2),plot=FALSE)",
            vec![],
            vec![0.7, 1.9, 3.1],
        ),
    ] {
        let result = session
            .eval_code_with_output_capture(code)
            .0
            .unwrap_or_else(|error| panic!("{code}: {}", error.message));
        assert_eq!(dimensions(&result), shape, "{code}");
        assert_midpoints(&result, &midpoints);
    }
}

#[cfg(feature = "renderplot-device")]
fn render(code: &str) -> Result<(RSession, Sexp<'static>, r_graphics_engine::Scene), String> {
    let mut session = RSession::new_without_default_packages();
    let mut scene = r_graphics_engine::Scene::new(320, 240);
    let backend: *mut dyn r_graphics_engine::DrawTarget = &mut scene;
    // The scene and original session outlive the scoped backend guard. We only
    // inspect the owned recording after evaluation and guard cleanup finish.
    let result =
        session.eval_script_with_output_capture_then_renderplot(code, backend, |result, _, _| {
            result.map(|value| value.into_owned().unwrap())
        });
    result
        .map(|value| (session, value, scene))
        .map_err(|error| error.message)
}

#[cfg(feature = "renderplot-device")]
fn text_labels(scene: &r_graphics_engine::Scene) -> Vec<&str> {
    scene
        .operations()
        .iter()
        .filter_map(|operation| match operation {
            r_graphics_engine::DrawOperation::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "renderplot-device")]
#[test]
fn owning_barplot_named_vector_and_table_render_all_labels() {
    for (code, labels) in [
        (
            "barplot(c(A=2,B=4,C=3),axes=FALSE,ann=FALSE)",
            ["A", "B", "C"],
        ),
        (
            "barplot(table(c('a','a','b','c','c','c')),axes=FALSE,ann=FALSE)",
            ["a", "b", "c"],
        ),
    ] {
        let (_session, value, scene) =
            render(code).unwrap_or_else(|error| panic!("{code}: {error}"));
        assert_eq!(dimensions(&value), [3, 1]);
        assert_midpoints(&value, &[0.7, 1.9, 3.1]);
        let actual = text_labels(&scene);
        for label in labels {
            assert!(actual.contains(&label), "{code}: missing label {label}");
        }
        scene.validate().unwrap();
    }
}

#[cfg(feature = "renderplot-device")]
#[test]
fn owning_barplot_one_label_and_grouped_matrix_use_gnu_axis_counts() {
    for (code, labels, shape) in [
        (
            "barplot(c(2,4,3),names.arg='group',axes=FALSE,ann=FALSE)",
            vec!["group"],
            vec![3, 1],
        ),
        (
            "barplot(matrix(1:6,nrow=2),beside=TRUE,names.arg=c('A','B','C'),axes=FALSE,ann=FALSE)",
            vec!["A", "B", "C"],
            vec![2, 3],
        ),
        (
            "barplot(matrix(1:6,nrow=2),beside=TRUE,names.arg=c('A','B','C','D','E','F'),axes=FALSE,ann=FALSE)",
            vec!["A", "B", "C", "D", "E", "F"],
            vec![2, 3],
        ),
    ] {
        let (_session, value, scene) =
            render(code).unwrap_or_else(|error| panic!("{code}: {error}"));
        assert_eq!(dimensions(&value), shape);
        let actual = text_labels(&scene);
        for label in labels {
            assert!(actual.contains(&label), "{code}: missing label {label}");
        }
        scene.validate().unwrap();
    }
}

#[cfg(feature = "renderplot-device")]
#[test]
fn owning_barplot_vector_preserves_per_bar_colors() {
    let (_session, value, scene) = render(
        "barplot(c(2,4,3),col=c('red','blue','green'),border=NA,axes=FALSE,axisnames=FALSE,ann=FALSE)",
    )
    .unwrap();
    assert_midpoints(&value, &[0.7, 1.9, 3.1]);
    let fills = scene
        .operations()
        .iter()
        .filter_map(|operation| match operation {
            r_graphics_engine::DrawOperation::Path(path) => Some(path.fill),
            _ => None,
        })
        .collect::<Vec<_>>();
    for color in [
        r_graphics_engine::Color::RED,
        r_graphics_engine::Color::BLUE,
        r_graphics_engine::Color {
            r: 0,
            g: 255,
            b: 0,
            a: 255,
        },
    ] {
        assert!(
            fills.contains(&color),
            "missing independently pinned color {color:?}"
        );
    }
    scene.validate().unwrap();
}

#[cfg(feature = "renderplot-device")]
#[test]
fn owning_barplot_wrong_name_count_errors_and_renderer_recovers() {
    let mut session = RSession::new_without_default_packages();
    let mut scene = r_graphics_engine::Scene::new(320, 240);
    let backend: *mut dyn r_graphics_engine::DrawTarget = &mut scene;
    let error = session.eval_script_with_output_capture_then_renderplot(
        "barplot(c(2,4,3),names.arg=c('A','B'))",
        backend,
        |result, _, _| result.unwrap_err(),
    );
    assert!(
        error.message.contains("incorrect number of names"),
        "{}",
        error.message
    );
    let backend: *mut dyn r_graphics_engine::DrawTarget = &mut scene;
    session.eval_script_with_output_capture_then_renderplot(
        "barplot(c(A=2,B=4,C=3),axes=FALSE,ann=FALSE)",
        backend,
        |result, _, _| {
            let value = result.expect("same session renderer recovers after name error");
            assert_midpoints(&value, &[0.7, 1.9, 3.1]);
        },
    );
    assert!(text_labels(&scene).contains(&"C"));
    scene.validate().unwrap();
}

#[cfg(feature = "renderplot-device")]
#[test]
fn owning_barplot_short_colors_recycle_in_gnu_flat_and_stacked_order() {
    use r_graphics_engine::{Color, DrawOperation};
    let green = Color {
        r: 0,
        g: 255,
        b: 0,
        a: 255,
    };
    for (code, expected) in [
        (
            "barplot(c(2,4,3),col=c('red','blue'),border=NA,axes=FALSE,axisnames=FALSE,ann=FALSE)",
            vec![Color::RED, Color::BLUE, Color::RED],
        ),
        (
            "barplot(matrix(1:6,nrow=2),beside=TRUE,col=c('red','blue','green'),border=NA,axes=FALSE,axisnames=FALSE,ann=FALSE)",
            vec![
                Color::RED,
                Color::BLUE,
                green,
                Color::RED,
                Color::BLUE,
                green,
            ],
        ),
        (
            "barplot(matrix(1:6,nrow=2),col=c('red','blue','green'),border=NA,axes=FALSE,axisnames=FALSE,ann=FALSE)",
            vec![
                Color::RED,
                Color::BLUE,
                Color::RED,
                Color::BLUE,
                Color::RED,
                Color::BLUE,
            ],
        ),
    ] {
        let (_session, _value, scene) =
            render(code).unwrap_or_else(|error| panic!("{code}: {error}"));
        let fills = scene
            .operations()
            .iter()
            .filter_map(|operation| match operation {
                DrawOperation::Path(path)
                    if [Color::RED, Color::BLUE, green].contains(&path.fill) =>
                {
                    Some(path.fill)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(fills, expected, "{code}");
        scene.validate().unwrap();
    }
}
