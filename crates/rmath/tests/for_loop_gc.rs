use rmath::android::{RSession, RValue};
#[test]
fn for_loop_sum_survives_per_iteration_gc() {
    let mut session = RSession::new();
    let result = session.eval("s <- 0; for (i in 1:5000) s <- s + i; s");
    assert_eq!(result.typed, RValue::Real(Some(12_502_500.0)));
}
