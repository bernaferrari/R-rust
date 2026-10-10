//! Independent GNU FixupCol zero-length and rectangle foreground contracts.
use crate::sexp::{RSession, ffi::SEXPTYPE};
use r_graphics_engine::{Color, DrawOperation, Path, Scene};

fn render(session: &mut RSession, body: &str) -> Scene {
    let mut scene = Scene::new(320, 240);
    let target: *mut dyn r_graphics_engine::DrawTarget = &mut scene;
    let code = format!("par(fg='red');plot.new();plot.window(c(0,1),c(0,1));{body}");
    session.eval_script_with_output_capture_then_renderplot(&code, target, |result, _, _| {
        result.unwrap_or_else(|error| panic!("{body}: {}", error.message));
    });
    scene.validate().unwrap();
    scene
}
fn rectangle(scene: &Scene) -> &Path {
    let paths = scene
        .operations()
        .iter()
        .filter_map(|operation| match operation {
            DrawOperation::Path(path) => Some(path),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        paths.len(),
        1,
        "one actual rectangle, not an omitted operation"
    );
    paths[0]
}
#[test]
fn owning_rect_empty_fill_and_border_follow_transparent_and_foreground_defaults() {
    let mut session = RSession::new_without_default_packages();
    for value in [
        "character()",
        "integer()",
        "double()",
        "logical()",
        "list()",
        "raw()",
    ] {
        let scene = render(
            &mut session,
            &format!("rect(.2,.2,.8,.8,col={value},border={value})"),
        );
        let path = rectangle(&scene);
        assert_eq!(
            path.fill.a, 0,
            "empty {value} must use transparent rectangle fill"
        );
        assert_eq!(
            path.stroke.color,
            Color::RED,
            "empty {value} must use current foreground, not black"
        );
    }
}
#[test]
fn owning_rect_null_border_uses_modified_foreground_and_na_stays_transparent() {
    let mut session = RSession::new_without_default_packages();
    let scene = render(&mut session, "rect(.2,.2,.8,.8,col='blue',border=NULL)");
    let path = rectangle(&scene);
    assert_eq!(path.fill, Color::BLUE);
    assert_eq!(path.stroke.color, Color::RED);
    let scene = render(
        &mut session,
        "rect(.2,.2,.8,.8,col=NA_character_,border=NA_character_)",
    );
    let path = rectangle(&scene);
    assert_eq!(path.fill.a, 0);
    assert_eq!(path.stroke.color.a, 0);
}
#[test]
fn owning_rect_invalid_color_recovers_to_empty_defaults() {
    let mut session = RSession::new_without_default_packages();
    let mut scene = Scene::new(320, 240);
    let target: *mut dyn r_graphics_engine::DrawTarget = &mut scene;
    let error = session.eval_script_with_output_capture_then_renderplot(
        "plot.new();plot.window(c(0,1),c(0,1));rect(.2,.2,.8,.8,col='not_an_actual_color')",
        target,
        |result, _, _| result.unwrap_err(),
    );
    assert!(
        error.message.contains("invalid color name"),
        "{}",
        error.message
    );
    let scene = render(
        &mut session,
        "rect(.2,.2,.8,.8,col=character(),border=character())",
    );
    assert_eq!(rectangle(&scene).fill.a, 0);
    assert_eq!(rectangle(&scene).stroke.color, Color::RED);
}
#[test]
fn owning_color_zero_length_types_keep_the_caller_default_nonempty() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    for tag in [
        SEXPTYPE::STRSXP,
        SEXPTYPE::INTSXP,
        SEXPTYPE::REALSXP,
        SEXPTYPE::LGLSXP,
        SEXPTYPE::VECSXP,
        SEXPTYPE::RAWSXP,
    ] {
        let value = factory
            .allocate(|arena| arena.alloc_vector_sexp(tag, 0).map(|value| value.as_raw()))
            .unwrap()
            .into_owned()
            .unwrap();
        assert_eq!(
            unsafe { super::colors(value.as_raw(), Color::BLUE) },
            vec![Color::BLUE],
            "{tag:?}"
        );
    }
}
