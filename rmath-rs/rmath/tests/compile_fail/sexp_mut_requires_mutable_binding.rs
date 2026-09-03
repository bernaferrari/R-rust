//! FORBIDDEN: constructing a `SexpMut` outside the crate.
//!
//! `SexpMut` is a crate-internal mutation guard, not a uniqueness proof:
//! consuming one clone cannot prove no other aliases exist. Its constructor
//! is therefore `pub(crate)` — external code cannot mint the guard at all.
//! In-crate mutation still requires a `mut` binding (`&mut self` setters).
//!
//! Expected: error[E0624] — `from_owned` is private.

use rmath::sexp::memory::RArena;
use rmath::sexp::object::SexpMut;
use rmath::sexp::SEXPTYPE;

pub fn forbidden() {
    let mut arena = RArena::new();
    let sexp = arena
        .alloc_vector_sexp(SEXPTYPE::INTSXP, 1)
        .expect("arena allocation failed");

    // External construction rejected: no guard, no mutation surface.
    let _guard = SexpMut::from_owned(sexp); //~ ERROR: E0624
}
