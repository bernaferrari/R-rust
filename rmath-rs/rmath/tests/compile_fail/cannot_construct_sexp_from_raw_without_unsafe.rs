//! FORBIDDEN: constructing a `Sexp` handle directly from a raw `SEXP`
//! pointer in safe code.
//!
//! WHY: a raw `SEXP` carries no lifetime and no provenance, so a handle
//! minted from one could outlive its memory or cross owners invisibly.
//! Every safe handle must therefore be issued by a checked owner (an
//! `RArena` allocation or an `RSession`), which ties the `Sexp<'a>`
//! lifetime to the owning memory. The raw-pointer constructors
//! (`from_raw`, `try_from_raw`, `from_arena_raw`, `from_session_raw`) are
//! `pub(crate)`: outside the crate the names do not resolve at all, so no
//! safe external code can fabricate a handle. Crossing the legacy FFI
//! boundary requires the explicitly `unsafe` unchecked wrapper instead.
//!
//! Expected: error[E0624] — associated function `from_raw` is private.

use rmath::sexp::memory::RArena;
use rmath::sexp::{Sexp, SEXPTYPE};

pub fn forbidden() {
    let mut arena = RArena::new();
    let raw = arena
        .alloc_vector_sexp(SEXPTYPE::INTSXP, 1)
        .expect("arena allocation failed")
        .as_raw();

    // Both raw-pointer constructors are crate-private:
    let _handle = Sexp::from_raw(raw); //~ ERROR: E0624
    let _other = Sexp::try_from_raw(raw); //~ ERROR: E0624
}
