use rmath::android::{RSession, RValue, RuntimePathPolicy, TopLevelEvaluationMode};

#[test]
fn batch_scripts_preserve_user_last_value_and_console_publication_under_both_policies() {
    for portable in [false, true] {
        for drawing in [false, true] {
            let mut session = if portable {
                RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
            } else {
                RSession::new()
            };
            session.set_top_level_evaluation_mode(TopLevelEvaluationMode::Script);
            let source = include_str!("../../../tests/conformance/cases/154_ls_all_names.R");
            let result = if drawing {
                let mut device = r_graphics_engine::Scene::new(320, 240);
                session.eval_script_with_renderplot_backend(source, &mut device)
            } else {
                session.eval_script(source)
            };
            assert_eq!(
                result.output,
                include_str!("../../../tests/conformance/golden/154_ls_all_names.out"),
                "portable={portable}, drawing={drawing}"
            );
            let result = session.eval_script(
                ".Last.value <- 11L; x <- 2L; 3L; stopifnot(identical(.Last.value, 11L)); TRUE",
            );
            assert_eq!(result.output, "[1] 3\n[1] TRUE\n");
            let result = session.eval_script(
                "rm('.Last.value'); calls <- 0L; makeActiveBinding('.Last.value', function(value) { calls <<- calls + 1L; gc(); if (!missing(value)) stop('automatic last-value write'); 42L }, globalenv()); 7L; 8L; stopifnot(calls == 0L); .Last.value",
            );
            assert_eq!(result.output, "[1] 7\n[1] 8\n[1] 42\n");
            assert_eq!(session.eval_script("calls").output, "[1] 1\n");
            session.set_top_level_evaluation_mode(TopLevelEvaluationMode::Console);
            let error = session.eval_script("23L");
            assert!(matches!(error.typed, RValue::Error(_)));
            assert!(error.output.contains("automatic last-value write"));
            session.set_top_level_evaluation_mode(TopLevelEvaluationMode::Script);
            session.eval_script("rm('.Last.value')");
            session.set_top_level_evaluation_mode(TopLevelEvaluationMode::Console);
            assert_eq!(session.eval_script("23L").output, "[1] 23\n");
            assert_eq!(session.eval_script(".Last.value").output, "[1] 23\n");
            session.close();
        }
    }
}
