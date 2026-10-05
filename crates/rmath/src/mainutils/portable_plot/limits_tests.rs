//! Exact GNU axis-limit admission through the actual portable plotting entry.
use crate::sexp::RSession;
use r_graphics_engine::Scene;

const CONTRACT: &str = include_str!("limits-contract.tsv");

fn render(session: &mut RSession, code: &str) -> Result<Scene, String> {
    let mut scene = Scene::new(320, 240);
    let target: *mut dyn r_graphics_engine::DrawTarget = &mut scene;
    session.eval_script_with_output_capture_then_renderplot(code, target, |result, _, _| {
        result.map(|_| ()).map_err(|error| error.message)
    })?;
    scene.validate().map_err(|error| error.to_string())?;
    Ok(scene)
}

#[test]
fn plot_limit_errors_preserve_gnu_axis_type_and_length_admission() {
    let mut session = RSession::new_without_default_packages();
    for row in CONTRACT.lines() {
        let (code, expected) = row.split_once('\t').unwrap();
        if expected == "OK" {
            continue;
        }
        let error = render(&mut session, code).expect_err(code);
        assert_eq!(error, expected, "{code}");
        render(&mut session, "plot(1:2)").expect("same session recovers after admission error");
    }
}

#[test]
fn plot_limit_explicit_finite_overrides_and_reversed_axes_remain_valid() {
    let mut session = RSession::new_without_default_packages();
    let mut checked = 0;
    for row in CONTRACT.lines() {
        let (code, expected) = row.split_once('\t').unwrap();
        if expected == "OK" {
            render(&mut session, code).unwrap_or_else(|error| panic!("{code}: {error}"));
            checked += 1;
        }
    }
    assert_eq!(checked, 5);
}
