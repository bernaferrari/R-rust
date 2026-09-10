//! GNU behavior probes: expected results recorded in fixtures/gnu-differential-wave9/oracle.json.
//! Normal assertions intentionally expose unresolved parity gaps; Beads: rport-tte8.

use r_embed::RSession;

fn raw_expression(bytes: &[u8]) -> String {
    format!(
        "as.raw(c({}))",
        bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn load(session: &mut RSession, bytes: &[u8]) {
    session
        .eval(&format!("f <- unserialize({})", raw_expression(bytes)))
        .unwrap();
}

fn check(fixture: &[u8], code: &str) {
    let mut session = RSession::new().unwrap();
    load(&mut session, fixture);
    let output = session
        .eval(code)
        .unwrap_or_else(|error| panic!("{code}: {error}"));
    assert_eq!(output.trim(), "[1] TRUE", "{code}");
}

const NAMESPACE: &[u8] = include_bytes!("fixtures/gnu-bytecode-checkfun/base-abs.rds");
const COMPUTED: &[u8] = include_bytes!("fixtures/gnu-bytecode-checkfun/callable-argument.rds");

#[test]
fn namespace_call_scalar() {
    check(NAMESPACE, "identical(f(-4L),4L)");
}
#[test]
fn namespace_call_named_argument() {
    check(NAMESPACE, "identical(f(x=-4L),4L)");
}
#[test]
fn namespace_call_na() {
    check(NAMESPACE, "is.na(f(NA_integer_))");
}
#[test]
fn namespace_call_vector() {
    check(NAMESPACE, "identical(f(c(-2L,3L)),c(2L,3L))");
}
#[test]
fn computed_call_builtin_control() {
    check(COMPUTED, "identical(f(abs,-4L),4L)");
}
#[test]
fn computed_call_closure_control() {
    check(COMPUTED, "h<-function(z)z*2L;identical(f(h,5L),10L)");
}
