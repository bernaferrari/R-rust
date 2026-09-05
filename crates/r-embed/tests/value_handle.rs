//! Session-scoped ValueHandle / ReadGuard / WriteGuard boundary.
//!
//! Contract under test (docs/rust-r-port-architecture.md safe-embedding
//! rules): a handle is a `Copy` id with no reference into the R arena;
//! all access is validated at use time and mediated by guards that
//! exclusively borrow the session. No `SEXP` appears in the public API.

use r_embed::RSession;

fn real(x: &[f64]) -> rmath::android::RValue {
    rmath::android::RValue::RealVector(x.iter().copied().map(Some).collect())
}

#[test]
fn define_read_roundtrip_survives_gc_and_evals() {
    let mut session = RSession::new().expect("session");
    let handle = session
        .define_handle("c(1, 2, 3) + 0.5")
        .expect("define");

    session.eval("x <- rnorm(100); gc(); rm(x); gc()").expect("noise");
    let guard = session.read_handle(&handle).expect("read after gc");
    assert_eq!(*guard, real(&[1.5, 2.5, 3.5]));
    // The guard exclusively borrows the session: session_id() must be
    // read from the guard itself, not the session (that borrow is what
    // the boundary rejects at compile time).
    assert_eq!(guard.session_id(), guard.session_id());
    drop(guard);

    // R-side mutation is visible through the same handle.
    session
        .eval("..rport_handles..$h0 <- c(9, 9)")
        .expect("r-side assign");
    let guard = session.read_handle(&handle).expect("second read");
    assert_eq!(*guard, real(&[9.0, 9.0]));
}

#[test]
fn write_guard_set_and_update() {
    let mut session = RSession::new().expect("session");
    let handle = session.define_handle("c(1, 2, 3)").expect("define");
    {
        let mut writer = session.write_handle(&handle).expect("write guard");
        writer.set("c(4, 5)").expect("set");
    }
    let guard = session.read_handle(&handle).expect("read after set");
    assert_eq!(*guard, real(&[4.0, 5.0]));
    drop(guard);
    {
        let mut writer = session.write_handle(&handle).expect("write guard 2");
        writer.update(". <- sum(.) + 100; . * 2").expect("update");
    }
    let guard = session.read_handle(&handle).expect("read after update");
    // R collapses the length-1 result to a scalar.
    assert_eq!(*guard, rmath::android::RValue::Real(Some(218.0)));
}
#[test]
fn failed_set_keeps_previous_binding() {
    let mut session = RSession::new().expect("session");
    let handle = session.define_handle("42").expect("define");
    {
        let mut writer = session.write_handle(&handle).expect("guard");
        assert!(writer.set("stop('boom')").is_err());
    }
    let guard = session.read_handle(&handle).expect("still readable");
    assert_eq!(*guard, rmath::android::RValue::Real(Some(42.0)));
}

#[test]
fn remove_invalidates_every_handle_to_the_slot() {
    let mut session = RSession::new().expect("session");
    let handle = session.define_handle("1:4").expect("define");
    let copy = handle; // Copy id: both alias the slot.
    session.remove_handle(&handle).expect("remove");
    for h in [&handle, &copy] {
        let err = session.read_handle(h).unwrap_err();
        assert!(
            err.to_string().contains("stale"),
            "expected stale, got: {err:?}"
        );
        let err = session.write_handle(h).unwrap_err();
        assert!(err.to_string().contains("stale"));
    }
    // Double-remove is also stale.
    assert!(session.remove_handle(&handle).is_err());
}

#[test]
fn foreign_session_handle_is_rejected() {
    let mut a = RSession::new().expect("a");
    let mut b = RSession::new().expect("b");
    let handle = a.define_handle("1").expect("define on a");
    let err = b.read_handle(&handle).unwrap_err();
    assert!(
        err.to_string().contains("belongs to session"),
        "expected foreign-session rejection, got: {err:?}"
    );
    assert!(b.write_handle(&handle).is_err());
    // The handle still works on its own session.
    let guard = a.read_handle(&handle).expect("still valid on a");
    assert_eq!(*guard, rmath::android::RValue::Real(Some(1.0)));
}

#[test]
fn define_failure_yields_no_handle_slot() {
    let mut session = RSession::new().expect("session");
    assert!(session.define_handle("stop('nope')").is_err());
    // The failed slot was never published: no binding exists.
    let exists = session
        .eval("exists(\"h0\", envir = ..rport_handles..)")
        .expect("exists probe");
    assert_eq!(exists.trim(), "[1] FALSE");
    // A later define reuses nothing: fresh handle works.
    let handle = session.define_handle("11").expect("second define");
    let guard = session.read_handle(&handle).expect("read");
    assert_eq!(*guard, rmath::android::RValue::Real(Some(11.0)));
}

#[test]
fn multi_statement_define_and_null_values_roundtrip() {
    let mut session = RSession::new().expect("session");
    let handle = session
        .define_handle("a <- 6; a * 7")
        .expect("multi-statement define");
    let guard = session.read_handle(&handle).expect("read");
    assert_eq!(*guard, rmath::android::RValue::Real(Some(42.0)));
    drop(guard);

    let null_handle = session.define_handle("NULL").expect("null define");
    let guard = session.read_handle(&null_handle).expect("null read");
    assert_eq!(*guard, rmath::android::RValue::Null);
}

#[test]
fn handle_env_is_hidden_from_global_binding_names() {
    let mut session = RSession::new().expect("session");
    session.define_handle("1").expect("define");
    let names = session.global_binding_names().expect("names");
    assert!(
        !names.iter().any(|n| n == "..rport_handles.."),
        "engine-internal env leaked into host binding list: {names:?}"
    );
}
