#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Bytecode interpreter stack management.
//!
//! Provides the R_bcstack_t structure and stack operations used by bcEval.

use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::instance::{RInstance, with_required_current_instance};

// ---------------------------------------------------------------------------
// R_bcstack_t — bytecode interpreter stack
// ---------------------------------------------------------------------------

/// The bytecode interpreter stack.
///
/// This is a simple dynamic array used by the bytecode interpreter
/// to store intermediate values during evaluation.
pub struct R_bcstack_t {
    /// Stack items (SEXP pointers).
    items: Vec<SEXP>,
    /// Current stack depth.
    depth: usize,
}

impl R_bcstack_t {
    /// Create a new bytecode stack with the given initial capacity.
    pub fn new(capacity: usize) -> Self {
        R_bcstack_t {
            items: Vec::with_capacity(capacity),
            depth: 0,
        }
    }

    /// Push a value onto the stack.
    #[inline]
    pub unsafe fn push(&mut self, val: SEXP) {
        if self.depth >= self.items.len() {
            self.items.push(val);
        } else {
            self.items[self.depth] = val;
        }
        self.depth += 1;
    }

    /// Pop a value from the stack.
    #[inline]
    pub unsafe fn pop(&mut self) -> SEXP {
        if self.depth == 0 {
            return ptr::null_mut();
        }
        self.depth -= 1;
        self.items[self.depth]
    }

    /// Peek at the top of the stack without popping.
    #[inline]
    pub unsafe fn top(&self) -> SEXP {
        if self.depth == 0 {
            return ptr::null_mut();
        }
        self.items[self.depth - 1]
    }

    /// Get the current stack depth.
    #[inline]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Set the stack depth (for restoring after a call).
    #[inline]
    pub unsafe fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    /// Get a value at a given index.
    #[inline]
    pub unsafe fn at(&self, index: usize) -> SEXP {
        self.items.get(index).copied().unwrap_or(ptr::null_mut())
    }

    /// Set a value at a given index.
    #[inline]
    pub unsafe fn set(&mut self, index: usize, val: SEXP) {
        if index < self.items.len() {
            self.items[index] = val;
        }
    }

    /// Check if the stack is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.depth == 0
    }
}

impl Default for R_bcstack_t {
    fn default() -> Self {
        Self::new(256)
    }
}

/// Get a reference to the current bytecode stack.
pub fn with_bc_stack<F, R>(f: F) -> R
where
    F: FnOnce(&mut R_bcstack_t) -> R,
{
    with_required_current_instance(|inst| with_bc_stack_in(inst, f))
}

pub(crate) fn with_bc_stack_in<F, R>(inst: &mut RInstance, f: F) -> R
where
    F: FnOnce(&mut R_bcstack_t) -> R,
{
    f(&mut inst.eval_state.bc_stack)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::instance::RInstance;
    use crate::sexp::session::RSession;

    use super::*;

    #[test]
    fn test_bc_stack_push_pop() {
        let mut stack = R_bcstack_t::new(4);
        unsafe {
            let a = 0x1 as SEXP;
            let b = 0x2 as SEXP;
            let c = 0x3 as SEXP;

            stack.push(a);
            stack.push(b);
            stack.push(c);

            assert_eq!(stack.depth(), 3);
            assert_eq!(stack.pop(), c);
            assert_eq!(stack.pop(), b);
            assert_eq!(stack.pop(), a);
            assert_eq!(stack.depth(), 0);
        }
    }

    #[test]
    fn test_bc_stack_peek() {
        let mut stack = R_bcstack_t::new(4);
        unsafe {
            stack.push(0x1 as SEXP);
            assert_eq!(stack.top(), 0x1 as SEXP);
            assert_eq!(stack.depth(), 1);
        }
    }

    #[test]
    fn test_bc_stack_at_set() {
        let mut stack = R_bcstack_t::new(4);
        unsafe {
            stack.push(0x1 as SEXP);
            stack.push(0x2 as SEXP);
            stack.push(0x3 as SEXP);

            assert_eq!(stack.at(1), 0x2 as SEXP);
            stack.set(1, 0x99 as SEXP);
            assert_eq!(stack.at(1), 0x99 as SEXP);
        }
    }

    #[test]
    fn test_bc_stack_pop_empty() {
        let mut stack = R_bcstack_t::new(4);
        unsafe {
            assert_eq!(stack.pop(), ptr::null_mut());
        }
    }

    #[test]
    fn test_session_bc_stacks_are_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        left.with_arena(|_| unsafe {
            with_bc_stack(|stack| {
                stack.push(0x1 as SEXP);
                assert_eq!(stack.depth(), 1);
            });
        })
        .unwrap();

        right
            .with_arena(|_| unsafe {
                with_bc_stack(|stack| {
                    assert_eq!(stack.depth(), 0);
                    stack.push(0x2 as SEXP);
                    assert_eq!(stack.pop(), 0x2 as SEXP);
                });
            })
            .unwrap();

        left.with_arena(|_| unsafe {
            with_bc_stack(|stack| {
                assert_eq!(stack.depth(), 1);
                assert_eq!(stack.pop(), 0x1 as SEXP);
            });
        })
        .unwrap();
    }

    #[test]
    fn test_bc_stack_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        with_bc_stack_in(&mut left, |stack| unsafe {
            stack.push(0x1 as SEXP);
            assert_eq!(stack.depth(), 1);
        });

        with_bc_stack_in(&mut right, |stack| {
            assert_eq!(stack.depth(), 0);
        });

        with_bc_stack_in(&mut left, |stack| unsafe {
            assert_eq!(stack.pop(), 0x1 as SEXP);
            assert_eq!(stack.depth(), 0);
        });
    }
}
