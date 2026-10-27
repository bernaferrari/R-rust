use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn top_level_task_callbacks_warn_for_successful_nonlogical_results() {
    let mut session = RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"))
        .expect("filesystem-free runtime initializes");

    let output = session
        .eval_script(
            r#"
answer <- NA
addTaskCallback(function(...) answer, name="nonlogical")
{ answer <- logical(0); cat("logical0 done\n") }
{ answer <- NULL; cat("NULL done\n") }
{ answer <- list(1L); cat("list done\n") }
{ answer <- expression(1L); cat("expression done\n") }
removeTaskCallback("nonlogical")
"#,
        )
        .expect("default warning mode continues script execution")
        .output;

    let callback_header = "warning messages from top-level task callback 'nonlogical'\n";
    let warning_message = "top-level task callback did not return a logical value";
    assert_eq!(output.matches(callback_header).count(), 3, "{output:?}");
    assert_eq!(output.matches(warning_message).count(), 3, "{output:?}");
    assert!(output.contains("logical0 done\n"), "{output:?}");
    assert!(output.contains("NULL done\n"), "{output:?}");
    assert!(output.contains("list done\n"), "{output:?}");
    assert!(output.contains("expression done\n"), "{output:?}");
    let warning_positions = output
        .match_indices(callback_header)
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    let null_position = output.find("NULL done\n").unwrap();
    let list_position = output.find("list done\n").unwrap();
    let expression_position = output.find("expression done\n").unwrap();
    assert!(null_position < warning_positions[0], "{output:?}");
    assert!(warning_positions[0] < list_position, "{output:?}");
    assert!(list_position < warning_positions[1], "{output:?}");
    assert!(warning_positions[1] < expression_position, "{output:?}");
    assert!(expression_position < warning_positions[2], "{output:?}");

    let error = session
        .eval_script(
            r#"
options(warn=2)
answer <- NULL
addTaskCallback(function(...) answer, name="warn2")
"#,
        )
        .expect_err("warn=2 turns the callback warning into an error")
        .to_string();
    assert!(error.contains("converted from warning"), "{error}");
    assert!(error.contains(warning_message), "{error}");

    let recovered = session
        .eval_script("options(warn=0)\nremoveTaskCallback(\"warn2\")\n42L")
        .expect("the top-level callback guard recovers after warn=2");
    assert!(
        recovered
            .output
            .contains("warning messages from top-level task callback 'warn2'\n")
    );
    assert!(
        recovered.output.ends_with("[1] 42\n"),
        "{:?}",
        recovered.output
    );
}
