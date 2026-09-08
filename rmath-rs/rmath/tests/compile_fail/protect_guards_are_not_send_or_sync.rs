// The translated runtime is crate-private; embedding uses owned values.
//~ ERROR: module `sexp` is private
//! FORBIDDEN: sending or sharing protection guards across threads.
//!
//! WHY: guards release into their OWNING instance's protection storages
//! (the legacy LIFO stack / the generational root table), which are guarded
//! only by `RefCell` and dispatched through the thread-local
//! current-instance machinery. A guard that crossed threads could be
//! dropped against a foreign thread-local session (or a dead instance).
//! Every guard type therefore carries a `PhantomData<*mut ()>` confinement
//! marker: `ProtectGuard`, `IndexedProtectGuard`, and `PreserveGuard` are
//! `!Send + !Sync` by construction.
//!
//! Expected: error[E0277] — `*mut ()` cannot be sent/shared between
//! threads safely (the raw-pointer marker opts the guards out of the
//! auto traits).

use rmath::sexp::protect::{IndexedProtectGuard, PreserveGuard, ProtectGuard};
use rmath::sexp::session::RSession;

pub fn forbidden() {
    let session = RSession::new();
    session.with_protected(|| {
        let guard = rmath::sexp::protect::protect_sexp(rmath::sexp::Sexp::nil());
        let indexed = rmath::sexp::protect::protect_sexp_with_index(rmath::sexp::Sexp::nil());
        let preserved = rmath::sexp::protect::preserve_sexp(rmath::sexp::Sexp::nil());

        // Moving any guard to another thread is rejected.
        let _send: Box<dyn Send> = Box::new(guard); 
        let _send_indexed: Box<dyn Send> = Box::new(indexed); 
        let _send_preserved: Box<dyn Send> = Box::new(preserved); 

        // Naming the auto traits directly fails the same way.
        fn require_send<T: Send>() {}
        fn require_sync<T: Sync>() {}
        require_send::<ProtectGuard>(); 
        require_sync::<ProtectGuard>(); 
        require_send::<IndexedProtectGuard>(); 
        require_sync::<IndexedProtectGuard>(); 
        require_send::<PreserveGuard>(); 
        require_sync::<PreserveGuard>(); 
    });
}
