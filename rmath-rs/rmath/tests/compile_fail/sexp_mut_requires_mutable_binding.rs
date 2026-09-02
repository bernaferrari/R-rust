//! FORBIDDEN: mutating through a `SexpMut` that is not behind `mut`.
//!
//! `SexpMut` is the exclusive mutation guard for the safe API: every write
//! takes `&mut self`, so the binding itself must be declared `mut`. A
//! shared binding can still read (through `Deref` to `Sexp`) but can never
//! write.
//!
//! Expected: error[E0596] — cannot borrow `guard` as mutable.

use rmath::sexp::memory::RArena;
use rmath::sexp::object::SexpMut;
use rmath::sexp::SEXPTYPE;

pub fn forbidden() {
    let mut arena = RArena::new();
    let sexp = arena
        .alloc_vector_sexp(SEXPTYPE::INTSXP, 1)
        .expect("arena allocation failed");

    // No `mut` on the binding: reads work, writes are rejected.
    let guard = SexpMut::from_owned(sexp);
    guard.set_integer_elt(0, 42); //~ ERROR: E0596
}
