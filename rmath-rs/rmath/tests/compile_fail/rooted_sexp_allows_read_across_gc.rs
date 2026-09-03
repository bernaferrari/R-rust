//! ALLOWED: a `RootedSexp` root keeps a value readable across GC.
//!
//! This is the positive half of the safety model: handles are not
//! immortal by themselves (the GC may collect unrooted young objects),
//! but rooting a clone of the handle on the protect stack pins the value,
//! so reads through the root remain valid after a collection pass. The
//! compile-fail harness compiles this file as a binary and runs it;
//! process exit status 0 is the pass condition.

use rmath::sexp::protect::RootedSexp;
use rmath::sexp::session::RSession;
use rmath::sexp::SEXPTYPE;

fn main() {
    let mut session = RSession::new();

    // Allocate and initialize through the session's arena; only the raw
    // pointer leaves the `with_arena` closure.
    let raw = session
        .with_arena(|arena| {
            let value = arena
                .alloc_vector_sexp(SEXPTYPE::INTSXP, 3)
                .expect("arena allocation failed");
            assert!(value.clone().set_integer_elt(0, 10));
            assert!(value.clone().set_integer_elt(1, 20));
            assert!(value.clone().set_integer_elt(2, 30));
            value.as_raw()
        })
        .expect("session should be active");
    let value = session.sexp(raw).expect("value belongs to session");

    session.with_protected(|| {
        let root = RootedSexp::root(value.clone());

        // Collect. The root pins the value on the protect stack, so the
        // handle stays valid and readable after collection.
        session.gc();

        // Read through the checked `get()` path and clone to keep a value.
        let readback = root.get().expect("root is live").clone();
        assert_eq!(readback.clone().len(), 3);
        assert_eq!(readback.clone().integer_elt(0), Some(10));
        assert_eq!(readback.clone().integer_elt(1), Some(20));
        assert_eq!(readback.clone().integer_elt(2), Some(30));
        // The root still names the same R object.
        assert_eq!(readback.as_raw(), value.as_raw());
    });

    println!("rooted_sexp_allows_read_across_gc: ok");
}
