//! FORBIDDEN: mutating an R object through a handle while a shared slice
//! view of the same object is live.
//!
//! WHY: `Sexp` deliberately has no `&mut self` mutation surface, so the
//! classic E0502 shared/mutable-borrow conflict cannot even arise.
//! Instead the crate enforces exclusivity with move semantics: both the
//! slice views (`as_integer_slice`, `as_real_slice`, ...) and the mutating
//! accessors (`set_integer_elt`, `set_real_elt`, ...) consume the handle
//! (`self` by value). Once the live slice view is taken, the handle is
//! gone — a mutating call through the shared (non-`mut`) binding is a
//! use-after-move, and the only way to mutate again would be an explicit
//! `clone()` at the call site, making the new alias visible in review.
//!
//! Expected: error[E0382] — use of moved value `sexp`.

use rmath::sexp::memory::RArena;
use rmath::sexp::SEXPTYPE;

pub fn forbidden() {
    let mut arena = RArena::new();
    let sexp = arena
        .alloc_vector_sexp(SEXPTYPE::INTSXP, 3)
        .expect("arena allocation failed");

    // Taking the bulk view consumes the handle: the slice outlives any
    // further use of `sexp` as a value.
    let slice = sexp.as_integer_slice().expect("integer vector");

    // Mutating through the (still non-mut, shared) binding while the
    // slice view is live: rejected, `sexp` was moved into the view.
    sexp.set_integer_elt(0, 42); //~ ERROR: E0382

    // The view is provably live across the mutation window above.
    let _sum = slice[0] + slice[1] + slice[2];
}
