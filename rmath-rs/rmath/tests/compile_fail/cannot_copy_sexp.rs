//! FORBIDDEN: `Copy` on `Sexp` handles — neither via a derived wrapper nor
//! by implicit copying of the handle itself.
//!
//! WHY: a `Copy` handle would let a stale alias legally survive an
//! in-place mutation of the same R object reached through another handle
//! (`SET_*` element writes, attribute assignment, environment rebinding) —
//! exactly the aliasing-undefined-behavior class this crate forbids (see
//! the "Intentionally Not `Copy`" section of the `Sexp` docs). Handles
//! therefore move by default; `clone()` is the explicit, reviewable way
//! to request a second aliasing handle.
//!
//! Expected: error[E0204] — `Copy` requires all fields to be `Copy`
//! (`Sexp` is not); error[E0382] — the moved handle is gone.

use rmath::sexp::memory::RArena;
use rmath::sexp::{Sexp, SEXPTYPE};

/// Deriving `Copy` on a local wrapper around `Sexp` is rejected: every
/// field of a `Copy` struct must itself be `Copy`, and `Sexp` is not.
#[derive(Clone, Copy)] //~ ERROR: E0204
struct Wrapper<'a> {
    handle: Sexp<'a>,
}

pub fn forbidden() {
    let mut arena = RArena::new();
    let sexp = arena
        .alloc_vector_sexp(SEXPTYPE::INTSXP, 3)
        .expect("arena allocation failed");

    // A plain binding MOVES the handle. Were `Sexp: Copy`, `sexp` would
    // still be usable below; since it is not, this is use-after-move.
    let alias = sexp;
    let _ = sexp.len(); //~ ERROR: E0382

    // The moved-to alias is a legal handle in its own right.
    let _ = alias.len();
    let _ = Wrapper { handle: alias };
}
