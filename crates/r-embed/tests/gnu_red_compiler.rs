// Beads: rport-uewq; broader bytecode execution: rport-bq7s.4.
use r_embed::{CancellationToken, RResourceLimits, RSession};

fn load(session: &mut RSession, bytes: &[u8]) {
    let raw = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    session
        .eval(&format!("f <- unserialize(as.raw(c({raw})))"))
        .unwrap();
}

fn encoded(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn unique_offset(bytes: &[u8], values: &[i32]) -> usize {
    let needle = encoded(values);
    let offsets = bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(i, window)| (window == needle).then_some(i))
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 1, "bytecode stream must occur exactly once");
    offsets[0]
}

#[test]
fn compiled_sqrt_opcode_executes_instead_of_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-red-compiler/sqrt.rds").to_vec();
    // GNU stream: version/BASEGUARD/GETVAR/SQRT. Replace SQRT with EXP while
    // retaining the original source expression (sqrt(x)).
    let offset = unique_offset(&bytes, &[12, 123, 0, 8, 20, 1, 49, 0, 1]);
    bytes[offset + 6 * 4..offset + 7 * 4].copy_from_slice(&50_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &bytes);
    assert_eq!(
        session.eval("round(f(4), 6)").unwrap().trim(),
        "[1] 54.59815"
    );
}

#[test]
fn compiled_math_opcode_uses_base_primitive_after_guard() {
    let mut bytes = include_bytes!("fixtures/gnu-red-compiler/sqrt.rds").to_vec();
    let offset = unique_offset(&bytes, &[12, 123, 0, 8, 20, 1, 49, 0, 1]);
    bytes[offset + 6 * 4..offset + 7 * 4].copy_from_slice(&50_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &bytes);
    assert_eq!(
        session
            .eval("exp <- function(x) -100; round(f(4), 6)")
            .unwrap()
            .trim(),
        "[1] 54.59815"
    );
}

#[test]
fn compiled_math_opcode_preserves_math_group_dispatch() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-red-compiler/sqrt.rds"),
    );
    assert_eq!(
        session
            .eval("Math.probe <- function(x, ...) 77; f(structure(4, class='probe'))")
            .unwrap()
            .trim(),
        "[1] 77"
    );
}

#[test]
fn compiled_baseguard_falls_back_when_guarded_function_is_rebound() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-red-compiler/sqrt.rds"),
    );
    assert_eq!(
        session.eval("sqrt <- function(x) 99; f(4)").unwrap().trim(),
        "[1] 99"
    );
}

#[test]
fn compiled_builtin_call_executes_through_the_guarded_builtin_path() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-red-compiler/abs.rds"),
    );
    assert_eq!(
        session.eval("identical(f(-4), 4)").unwrap().trim(),
        "[1] TRUE"
    );
}

#[test]
fn compiled_loop_comparison_opcode_executes_instead_of_retained_source() {
    let mut bytes = include_bytes!("fixtures/gnu-red-compiler/loop.rds").to_vec();
    // GNU loop stream contains LT.OP (53). Replace it with GT.OP (56): for
    // c(-1, 0, 1), the mutated program sums -1 instead of the source's 1.
    let offset = unique_offset(
        &bytes,
        &[
            12, 16, 1, 22, 2, 4, 20, 4, 11, 6, 5, 36, 20, 5, 16, 1, 53, 7,
        ],
    );
    bytes[offset + 16 * 4..offset + 17 * 4].copy_from_slice(&56_i32.to_be_bytes());

    let mut session = RSession::new().unwrap();
    load(&mut session, &bytes);
    assert_eq!(session.eval("f(c(-1L, 0L, 1L))").unwrap().trim(), "[1] -1");
}

#[test]
fn compiled_loop_handles_an_empty_sequence() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-red-compiler/loop.rds"),
    );
    assert_eq!(session.eval("f(integer())").unwrap().trim(), "[1] 0");
}

#[test]
fn compiled_loop_observes_cancellation_and_session_recovers() {
    let cancellation = CancellationToken::new();
    let worker_token = cancellation.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut session = RSession::new().unwrap();
        load(
            &mut session,
            include_bytes!("fixtures/gnu-red-compiler/loop.rds"),
        );
        session.eval("x <- rep(1L, 1000000L)").unwrap();
        ready_tx.send(()).unwrap();
        let error = session
            .eval_result_cancellable("f(x)", &worker_token)
            .expect_err("compiled loop should observe cancellation")
            .to_string();
        let recovered = session.eval("2 + 2").unwrap();
        (error, recovered)
    });

    ready_rx.recv().expect("worker should reach compiled loop");
    std::thread::sleep(std::time::Duration::from_millis(10));
    cancellation.cancel();

    let (error, recovered) = worker.join().expect("worker should not panic");
    assert!(error.contains("operation cancelled"));
    assert_eq!(recovered.trim(), "[1] 4");
}

#[test]
fn compiled_loop_observes_time_budget_and_session_recovers() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-red-compiler/loop.rds"),
    );
    session.eval("x <- rep(1L, 1000000L)").unwrap();
    session
        .set_resource_limits(RResourceLimits {
            max_eval_depth: 500,
            max_execution_time_ms: 1,
            max_alloc_bytes: 0,
            max_arena_nodes: 0,
        })
        .unwrap();
    let error = session
        .eval_result("f(x)")
        .expect_err("compiled loop should observe its execution-time budget");
    assert!(error.to_string().contains("time limit"));

    session
        .set_resource_limits(RResourceLimits {
            max_eval_depth: 500,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
            max_arena_nodes: 0,
        })
        .unwrap();
    assert_eq!(session.eval("2 + 2").unwrap().trim(), "[1] 4");
}

#[test]
fn compiled_for_visits_factor_labels_and_preserves_element_types() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-red-compiler/last.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(factor(c('a','b'))),'b')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(list(1L,c(2L,3L))),c(2L,3L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(f(as.raw(c(1,2))),as.raw(2))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn compiled_loop_can_return_before_endfor() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-red-compiler/early-return.rds"),
    );
    assert_eq!(session.eval("f(c(-1L,2L,3L))").unwrap().trim(), "[1] 2");
    assert_eq!(session.eval("f(integer())").unwrap().trim(), "[1] 0");
}
