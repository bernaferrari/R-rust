//! FORBIDDEN: carrying a session-scoped `Sexp` handle out of the session
//! that owns it — here, out of session A and into session B.
//!
//! WHY: a handle issued by an `RSession` is typed with the borrow of that
//! session (`Sexp<'a>` where `'a` is tied to `&session_a`). The borrow
//! checker therefore rejects any escape of the handle past its owner's
//! scope: a value minted by session A cannot survive into session B's
//! code, no matter how the downstream API is shaped. (Crossing owners at
//! runtime is additionally rejected by the `SexpOwner` token checks, but
//! the lifetime discipline already stops the pattern at compile time.)
//!
//! Expected: error[E0597] — `session_a` does not live long enough.

use rmath::sexp::session::RSession;
use rmath::sexp::SEXPTYPE;

pub fn forbidden() {
    let session_b = RSession::new();
    let carried = {
        let mut session_a = RSession::new();
        let raw = session_a
            .with_arena(|arena| arena.alloc_vector_sexp(SEXPTYPE::INTSXP, 1).expect("arena allocation failed").as_raw())
            .expect("session should be active");
        let handle = session_a.sexp(raw).expect("value belongs to session");

        // Attempt to move the handle out of session A's scope so it can
        // be used with session B below. The handle's lifetime is tied to
        // the borrow of `session_a`, which ends with this block.
        handle
    }; //~ ERROR: E0597

    // Would type-check for any owner-scoped handle, but `carried` cannot
    // exist: its lifetime died with session A.
    let _ = session_b.define_var("carried", carried);
}
