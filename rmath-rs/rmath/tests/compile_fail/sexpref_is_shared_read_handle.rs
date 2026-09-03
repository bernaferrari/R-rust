//! ALLOWED: `SexpRef` is the shared-borrow read handle.
//!
//! `SexpRef` is an alias for `Sexp`: a freely reborrowable shared handle
//! with no mutation surface. Shared reads (`len`, `typeof_`,
//! `integer_elt`, predicates) coexist through clones and reborrows, while
//! in-place mutation goes through `SexpMut::from_owned(..)` -> `set_*` ->
//! `freeze()`. The compile-fail harness compiles this file as a binary and
//! runs it; process exit status 0 is the pass condition.

use rmath::sexp::SEXPTYPE;
use rmath::sexp::SexpRef;
use rmath::sexp::memory::RArena;

fn main() {
    let mut arena = RArena::new();
    let sexp = arena
        .alloc_vector_sexp(SEXPTYPE::INTSXP, 3)
        .expect("arena allocation failed");

    // Shared handle binding: reads never move the alias.
    let r: SexpRef<'_> = sexp.clone();
    assert_eq!(r.len(), 3);
    assert_eq!(r.typeof_(), SEXPTYPE::INTSXP);
    assert!(r.is_vector());

    // Second shared reborrow coexists with the first.
    let r2: SexpRef<'_> = r.clone();
    assert_eq!(r2.integer_elt(0), Some(0));
    assert_eq!(r.integer_elt(0), Some(0));
    assert_eq!(r.len(), r2.len());

    // The original handle is untouched and still fully usable.
    assert_eq!(sexp.len(), 3);

    println!("sexpref_is_shared_read_handle: ok");
}
