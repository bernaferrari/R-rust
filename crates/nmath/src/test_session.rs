//! Minimal test-only R-session shim.
//!
//! The upstream rmath port's session-locality tests drive two independent
//! sessions via `rmath::sexp::RSession`. This crate has no interpreter
//! dependency, so tests use this tiny stand-in: it installs fresh
//! [`MathState`] and [`RngState`](crate::rng::RngState) for its lifetime,
//! restoring whatever was installed before.

use crate::rng::{detach_rng, install_rng};
use crate::state::{
    MathState, detach_state, install_state, replace_rng, replace_state, restore_rng, restore_state,
};

/// A scoped pair of math + RNG state, mirroring the parts of the host
/// session that nmath code observes. Create one per logical "session".
pub(crate) struct TestSession {
    math: Box<MathState>,
    rng: Box<crate::rng::RngState>,
}

impl TestSession {
    /// Install this session's state as current for the thread.
    pub(crate) fn new() -> Self {
        let mut session = TestSession {
            math: Box::new(MathState::default()),
            rng: Box::new(crate::rng::RngState::default()),
        };
        unsafe {
            install_state(&mut *session.math as *mut MathState);
            install_rng(&mut *session.rng as *mut crate::rng::RngState);
        }
        session
    }

    /// Run `f` with this session's state installed.
    ///
    /// Mirrors `RSession::with_protected` from the host runtime so the moved
    /// test bodies keep their shape.
    pub(crate) fn with_protected<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let prev_math = replace_state(&mut *self.math as *mut MathState);
        let prev_rng = replace_rng(&mut *self.rng);
        let value = f();
        restore_state(prev_math);
        restore_rng(prev_rng);
        value
    }
}

impl Drop for TestSession {
    fn drop(&mut self) {
        detach_state(&*self.math);
        detach_rng(&*self.rng);
    }
}
