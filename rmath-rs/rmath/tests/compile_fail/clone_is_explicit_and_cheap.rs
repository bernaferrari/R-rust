//! ALLOWED: cloning a `Sexp` handle is explicit, cheap, and yields an
//! independent handle over the SAME R object.
//!
//! `Clone` (unlike `Copy`) makes every aliasing decision visible at the
//! call site. The clone shares the underlying R object — pointer identity
//! proves no deep copy touched R's heap — while remaining an independent
//! Rust value that can be moved and consumed without surrendering the
//! original handle. The compile-fail harness compiles this file as a
//! binary and runs it; process exit status 0 is the pass condition.

use rmath::sexp::memory::RArena;
use rmath::sexp::SEXPTYPE;

fn main() {
    let mut arena = RArena::new();
    let sexp = arena
        .alloc_vector_sexp(SEXPTYPE::INTSXP, 2)
        .expect("arena allocation failed");

    // Explicit, cheap clone: a second handle over the same SEXP.
    let alias = sexp.clone();
    assert_eq!(
        alias.clone().as_raw(),
        sexp.clone().as_raw(),
        "clone must alias the same R object, not deep-copy it"
    );
    // PartialEq compares by reference: no move, no clone needed here.
    assert_eq!(alias, sexp, "handle equality is pointer identity");

    // The clone is an independent Rust value: it moves and is consumed by
    // a by-value accessor without affecting the original handle.
    let moved = alias;
    assert_eq!(moved.len(), 2);

    // The original handle is untouched and still fully usable.
    assert_eq!(sexp.clone().len(), 2);
    assert!(sexp.clone().is_vector());
    println!("clone_is_explicit_and_cheap: ok");
}
