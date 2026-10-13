use rmath::android::{RSession, RuntimePathPolicy, TopLevelEvaluationMode};

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
            // GNU bootstrap leaves a locked NULL `.Last.value` binding on
            // the base symbol cell (R_ReplFile over the base sources, then
            // base sealing), so a fresh session must show it BEFORE any
            // top-level expression replaces the placeholder value. Script
            // mode then publishes per expression exactly like GNU
            // R_ReplFile (main.c:99): the acceptance pair from
            // reg-tests-1c holds without console tagging.
            let placeholder = session.eval_script(
                "stopifnot(exists('.Last.value', envir=baseenv()), is.null(get('.Last.value', envir=baseenv())), bindingIsLocked('.Last.value', baseenv()))",
            );
            assert_eq!(placeholder.output, "");
            let result = session.eval_script(
                "0x110p-5L\nstopifnot(.Last.value == 8.5, identical(get('.Last.value', envir=baseenv()), 8.5))\n",
            );
            assert_eq!(
                result.output,
                "[1] 8.5\nWarning message:\nnon-integer value 0x110p-5L qualified with L; using numeric value \n"
            );
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
            // GNU main.c SET_SYMVALUE bypasses active bindings on the write
            // side: `23L` succeeds and only reads invoke the function.
            let published = session.eval_script("23L");
            assert_eq!(published.output, "[1] 23\n");
            assert_eq!(session.eval_script(".Last.value").output, "[1] 42\n");
            assert_eq!(session.eval_script("calls").output, "[1] 2\n");
            session.set_top_level_evaluation_mode(TopLevelEvaluationMode::Script);
            session.eval_script("rm('.Last.value')");
            session.set_top_level_evaluation_mode(TopLevelEvaluationMode::Console);
            assert_eq!(session.eval_script("23L").output, "[1] 23\n");
            assert_eq!(session.eval_script(".Last.value").output, "[1] 23\n");
            session.close();
        }
    }
}
