use rmath::sexp::session::RSession;

fn real_value(sexp: rmath::sexp::ffi::SEXP) -> f64 {
    unsafe { *((*sexp).gengc_next_node as *const f64) }
}

#[test]
fn for_loop_sum_survives_per_iteration_gc() {
    let mut session = RSession::new();
    let script = "s <- 0\nfor (i in 1:5000) s <- s + i\ns";
    let (result, _, _) = session.eval_script_with_output_capture(script);
    let sexp = result.expect("eval should succeed");
    assert_eq!(
        real_value(sexp.as_raw()),
        12_502_500.0,
        "sum 1..5000"
    );
}