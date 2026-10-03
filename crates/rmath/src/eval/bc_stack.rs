#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! The bytecode operand stack owns every live value. Popping transfers the
//! physical heap lease to the caller; truncation drops discarded values.

use crate::sexp::context::RError;
use crate::sexp::ffi::SEXP;
use crate::sexp::instance::{RInstance, with_required_current_instance};
use crate::sexp::object::Sexp;

fn stack_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

/// Validate a native projection and acquire its actual original heap lease.
///
/// # Safety
/// `value` must be a live native projection at this boundary.
pub(super) unsafe fn own_operand(value: SEXP) -> Sexp<'static> {
    unsafe { crate::sexp::owner::OwnerToken::current() }
        .and_then(|owner| owner.sexp(value))
        .and_then(Sexp::into_owned)
        .unwrap_or_else(|error| stack_error(format!("invalid bytecode operand: {error}")))
}

pub struct R_bcstack_t {
    items: Vec<Sexp<'static>>,
}

impl R_bcstack_t {
    pub fn new(capacity: usize) -> Self {
        let mut items = Vec::new();
        items
            .try_reserve(capacity)
            .unwrap_or_else(|_| stack_error("cannot reserve bytecode operand stack"));
        Self { items }
    }

    /// Native instruction adapter. Ownership is acquired before publication.
    pub unsafe fn push(&mut self, value: SEXP) {
        self.push_owned(unsafe { own_operand(value) });
    }

    pub fn push_owned(&mut self, value: Sexp<'static>) {
        self.items
            .try_reserve(1)
            .unwrap_or_else(|_| stack_error("cannot grow bytecode operand stack"));
        self.items.push(value);
    }

    pub fn pop_owned(&mut self) -> Sexp<'static> {
        self.items
            .pop()
            .unwrap_or_else(|| stack_error("bytecode stack underflow"))
    }

    pub fn top_owned(&self) -> Sexp<'static> {
        self.items
            .last()
            .cloned()
            .unwrap_or_else(|| stack_error("bytecode stack underflow"))
    }

    pub fn at_owned(&self, index: usize) -> Sexp<'static> {
        self.items
            .get(index)
            .cloned()
            .unwrap_or_else(|| stack_error(format!("bytecode stack slot {index} is missing")))
    }

    /// Short native projection; the actual entry remains owned by this stack.
    pub fn top(&self) -> SEXP {
        self.items
            .last()
            .map_or_else(|| stack_error("bytecode stack underflow"), Sexp::as_raw)
    }
    pub fn at(&self, index: usize) -> SEXP {
        self.items.get(index).map_or_else(
            || stack_error(format!("bytecode stack slot {index} is missing")),
            Sexp::as_raw,
        )
    }
    pub fn depth(&self) -> usize {
        self.items.len()
    }

    pub fn set_depth(&mut self, depth: usize) {
        if depth > self.items.len() {
            stack_error(format!(
                "cannot restore bytecode stack depth {depth} from {}",
                self.items.len()
            ));
        }
        self.items.truncate(depth);
    }

    pub unsafe fn set(&mut self, index: usize, value: SEXP) {
        let value = unsafe { own_operand(value) };
        self.set_owned(index, value);
    }

    pub fn set_owned(&mut self, index: usize, value: Sexp<'static>) {
        let slot = self
            .items
            .get_mut(index)
            .unwrap_or_else(|| stack_error(format!("bytecode stack slot {index} is missing")));
        *slot = value;
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for R_bcstack_t {
    fn default() -> Self {
        Self::new(256)
    }
}

/// # Safety
/// The closure is a local stack operation and cannot reenter R or collect.
pub unsafe fn with_bc_stack<F, R>(f: F) -> R
where
    F: FnOnce(&mut R_bcstack_t) -> R,
{
    with_required_current_instance(|inst| unsafe { with_bc_stack_in(inst, f) })
}

/// # Safety
/// The original instance must stay physically pinned for this strictly local
/// stack operation. `f` may not allocate, evaluate, or reenter the interpreter.
pub(crate) unsafe fn with_bc_stack_in<F, R>(inst: *mut RInstance, f: F) -> R
where
    F: FnOnce(&mut R_bcstack_t) -> R,
{
    f(unsafe { &mut (*inst).eval_state.bc_stack })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::INTEGER;
    use crate::sexp::constructors::Rf_ScalarInteger;
    use crate::sexp::session::RSession;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn owned_bytecode_stack_transfers_popped_values_and_releases_truncation() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let mut stack = R_bcstack_t::new(2);
            let a = Rf_ScalarInteger(11);
            let b = Rf_ScalarInteger(22);
            stack.push(a);
            stack.push(b);
            let popped = stack.pop_owned();
            stack.set_depth(0);
            crate::sexp::owner::OwnerToken::current()
                .unwrap()
                .full_gc()
                .unwrap();
            assert_eq!(*INTEGER(popped.as_raw()), 22);
            assert!(stack.is_empty());
            assert!(crate::sexp::memory::checked_projection(a).is_none());
        });
    }

    #[test]
    fn owned_bytecode_stack_rejects_invalid_depth_slots_and_unwinds_without_stale_roots() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let mut stack = R_bcstack_t::new(1);
            stack.push(Rf_ScalarInteger(3));
            for op in [0, 1, 2] {
                let error = catch_unwind(AssertUnwindSafe(|| match op {
                    0 => stack.set_depth(2),
                    1 => {
                        stack.at_owned(1);
                    }
                    _ => {
                        stack.set_owned(1, stack.top_owned());
                    }
                }))
                .unwrap_err();
                assert!(error.downcast_ref::<RError>().is_some());
                assert_eq!(stack.depth(), 1);
                assert_eq!(*INTEGER(stack.top()), 3);
            }
            let value = stack.top();
            let error = catch_unwind(AssertUnwindSafe(|| {
                let _owned_stack = stack;
                stack_error("forced bytecode unwind");
            }))
            .unwrap_err();
            assert!(error.downcast_ref::<RError>().is_some());
            crate::sexp::owner::OwnerToken::current()
                .unwrap()
                .full_gc()
                .unwrap();
            assert!(crate::sexp::memory::checked_projection(value).is_none());
        });
    }

    #[test]
    fn owned_bytecode_stacks_are_session_local() {
        let left = RSession::new_for_gc_tests();
        let right = RSession::new_for_gc_tests();
        left.with_active(|| unsafe {
            let value = Rf_ScalarInteger(17);
            with_bc_stack(|stack| stack.push(value));
        });
        right.with_active(|| unsafe { with_bc_stack(|stack| assert!(stack.is_empty())) });
        left.with_active(|| unsafe {
            with_bc_stack(|stack| {
                assert_eq!(stack.depth(), 1);
                let value = stack.pop_owned();
                assert_eq!(unsafe { *INTEGER(value.as_raw()) }, 17);
            })
        });
    }
}
