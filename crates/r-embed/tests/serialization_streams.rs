use r_embed::{RSession, RValue, RuntimePathPolicy};

fn check(program: &str) {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap();
    session.enable_browser_files();
    assert_eq!(
        session.eval_result(program).unwrap().value,
        RValue::Logical(Some(true))
    );
}

#[test]
fn sequential_serialization_formats_leave_the_next_object_unread() {
    check(include_str!(
        "../../rmath/src/mainutils/serialize/sequential-contract.R"
    ));
}

#[test]
fn restoration_hooks_observe_gnu_stream_cursors() {
    check(include_str!(
        "../../rmath/src/mainutils/serialize/hook-cursor-contract.R"
    ));
}

#[test]
fn restoration_hooks_can_read_next_object_and_close_exhausted_stream() {
    check(include_str!(
        "../../rmath/src/mainutils/serialize/hook-reentry-contract.R"
    ));
}

#[test]
fn compressed_streams_are_decoded_by_the_connection_and_preserve_the_next_object() {
    let bytes = include_bytes!("fixtures/gnu-sequential-v2.rds.gz");
    let raw = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    check(&format!(
        "compressed <- as.raw(c({raw}));{}",
        include_str!("../../rmath/src/mainutils/serialize/compressed-stream-contract.R")
    ));
}

#[test]
fn compiled_closure_restoration_observes_each_gnu_hook_cursor() {
    let bytes = include_bytes!("fixtures/gnu-bytecode-stream-hooks-v2.rds");
    let raw = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    check(&format!(
        "compiled <- as.raw(c({raw}));{}",
        include_str!("../../rmath/src/mainutils/serialize/compiled-hook-contract.R")
    ));
}
