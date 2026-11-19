//! Fallible bytecode proof buffers charged to the input's original heap.
#![forbid(unsafe_code)]

use crate::sexp::memory::{MemoryLedger, WorkspaceReservation};
use std::cell::{Cell, RefCell};

pub(super) struct ProofWorkspace {
    reservation: RefCell<WorkspaceReservation>,
    limit: Option<usize>,
    exhausted: Cell<bool>,
    admission_denied: Cell<bool>,
    work: Cell<usize>,
}

fn denied() -> String {
    "bytecode validation workspace exceeds memory limit".into()
}

impl ProofWorkspace {
    pub(super) fn new(ledger: &MemoryLedger, work: usize) -> Result<Self, String> {
        Ok(Self {
            reservation: RefCell::new(ledger.reserve(0).ok_or_else(denied)?),
            limit: None,
            exhausted: Cell::new(false),
            admission_denied: Cell::new(false),
            work: Cell::new(work),
        })
    }
    pub(super) fn bounded(
        ledger: &MemoryLedger,
        limit: usize,
        work: usize,
    ) -> Result<Self, String> {
        let mut workspace = Self::new(ledger, work)?;
        workspace.limit = Some(limit);
        Ok(workspace)
    }
    pub(super) fn exhausted(&self) -> bool {
        self.exhausted.get()
    }
    pub(super) fn admission_denied(&self) -> bool {
        self.admission_denied.get()
    }
    pub(super) fn spend_work(&self) -> Result<(), String> {
        let Some(remaining) = self.work.get().checked_sub(1) else {
            self.exhausted.set(true);
            return Err("bytecode proof work budget exhausted".into());
        };
        self.work.set(remaining);
        Ok(())
    }
    fn add(&self, bytes: usize) -> Result<(), String> {
        if bytes == 0 {
            return Ok(());
        }
        let mut ticket = self.reservation.borrow_mut();
        let total = ticket.bytes().checked_add(bytes).ok_or_else(denied)?;
        if self.limit.is_some_and(|limit| total > limit) {
            self.exhausted.set(true);
            return Err(denied());
        }
        if ticket.try_resize(total).is_none() {
            self.admission_denied.set(true);
            return Err(denied());
        }
        Ok(())
    }
    fn release(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let mut ticket = self.reservation.borrow_mut();
        let total = ticket
            .bytes()
            .checked_sub(bytes)
            .expect("proof buffer charge balance");
        ticket
            .try_resize(total)
            .expect("release original proof admission");
    }
}

pub(super) struct ProofBuffer<'workspace, T> {
    values: Vec<T>,
    workspace: &'workspace ProofWorkspace,
}

impl<'workspace, T> ProofBuffer<'workspace, T> {
    pub(super) fn new(workspace: &'workspace ProofWorkspace) -> Result<Self, String> {
        Ok(Self {
            values: Vec::new(),
            workspace,
        })
    }
    pub(super) fn try_push(&mut self, value: T) -> Result<(), String> {
        let needed = self.values.len().checked_add(1).ok_or_else(denied)?;
        if needed > self.values.capacity() {
            self.reserve(needed.checked_next_power_of_two().unwrap_or(needed))?;
        }
        self.values.push(value);
        Ok(())
    }
    pub(super) fn pop(&mut self) -> Option<T> {
        self.values.pop()
    }
    pub(super) fn filled(
        workspace: &'workspace ProofWorkspace,
        length: usize,
        mut element: impl FnMut() -> T,
    ) -> Result<Self, String> {
        let mut buffer = Self::new(workspace)?;
        buffer.reserve(length)?;
        buffer.values.resize_with(length, &mut element);
        Ok(buffer)
    }
    fn reserve(&mut self, capacity: usize) -> Result<(), String> {
        if capacity <= self.values.capacity() {
            return Ok(());
        }
        let requested = capacity
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(denied)?;
        self.workspace.add(requested)?;
        let mut replacement = Vec::new();
        if replacement.try_reserve_exact(capacity).is_err() {
            self.workspace.release(requested);
            return Err(denied());
        }
        let actual = replacement.capacity().checked_mul(std::mem::size_of::<T>());
        let funded = actual
            .and_then(|bytes| bytes.checked_sub(requested))
            .ok_or_else(denied)
            .and_then(|extra| self.workspace.add(extra));
        if let Err(error) = funded {
            drop(replacement);
            self.workspace.release(requested);
            return Err(error);
        }
        let old_bytes = self
            .values
            .capacity()
            .checked_mul(std::mem::size_of::<T>())
            .expect("admitted old proof capacity");
        replacement.append(&mut self.values);
        drop(std::mem::replace(&mut self.values, replacement));
        self.workspace.release(old_bytes);
        Ok(())
    }
}
impl<T> Drop for ProofBuffer<'_, T> {
    fn drop(&mut self) {
        let bytes = self
            .values
            .capacity()
            .checked_mul(std::mem::size_of::<T>())
            .expect("admitted proof capacity");
        drop(std::mem::take(&mut self.values));
        self.workspace.release(bytes);
    }
}
impl<T> std::ops::Deref for ProofBuffer<'_, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}
impl<T> std::ops::DerefMut for ProofBuffer<'_, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}
impl<T: PartialEq> PartialEq for ProofBuffer<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}

/// Canonical immutable stack heads. Reusing (parent,value) nodes makes
/// complete marker-stack equality constant-time without per-PC Vec headers.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(super) struct MarkerStack(usize);
struct MarkerNode {
    parent: MarkerStack,
    value: usize,
    depth: usize,
}
pub(super) struct MarkerArena<'a> {
    nodes: ProofBuffer<'a, MarkerNode>,
}
impl<'a> MarkerArena<'a> {
    pub(super) fn new(workspace: &'a ProofWorkspace) -> Result<Self, String> {
        Ok(Self {
            nodes: ProofBuffer::new(workspace)?,
        })
    }
    pub(super) fn push(
        &mut self,
        parent: MarkerStack,
        value: usize,
    ) -> Result<MarkerStack, String> {
        self.nodes.workspace.spend_work()?;
        for (index, node) in self.nodes.iter().enumerate() {
            self.nodes.workspace.spend_work()?;
            if node.parent == parent && node.value == value {
                return Ok(MarkerStack(index + 1));
            }
        }
        let depth = self.len(parent).checked_add(1).ok_or_else(denied)?;
        self.nodes.try_push(MarkerNode {
            parent,
            value,
            depth,
        })?;
        Ok(MarkerStack(self.nodes.len()))
    }
    pub(super) fn last(&self, stack: MarkerStack) -> Option<usize> {
        stack.0.checked_sub(1).map(|index| self.nodes[index].value)
    }
    pub(super) fn len(&self, stack: MarkerStack) -> usize {
        stack
            .0
            .checked_sub(1)
            .map_or(0, |index| self.nodes[index].depth)
    }
    pub(super) fn pop(&self, stack: &mut MarkerStack) -> Option<usize> {
        let node = &self.nodes[stack.0.checked_sub(1)?];
        *stack = node.parent;
        Some(node.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proof_buffer_charges_actual_capacity_and_preserves_backing_on_growth_denial() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let ledger = session.with_active(|| {
            session
                .owner_token()
                .unwrap()
                .with_arena(|arena| {
                    let ledger = arena.heap_identity().byte_ledger();
                    let mut budget = arena.budget();
                    budget.max_bytes = ledger.allocated_bytes() + 64;
                    arena.set_budget(budget);
                    ledger
                })
                .unwrap()
        });
        let before = ledger.workspace_bytes_for_test();
        let workspace = ProofWorkspace::new(&ledger, 65_536).unwrap();
        let mut values = ProofBuffer::new(&workspace).unwrap();
        for x in 0usize..4 {
            values.try_push(x).unwrap();
        }
        assert_eq!(
            ledger.workspace_bytes_for_test() - before,
            values.values.capacity() * std::mem::size_of::<usize>()
        );
        // Old32 + replacement64 exceeds64 bytes of headroom.
        assert!(values.try_push(4).is_err());
        assert_eq!(&*values, &[0, 1, 2, 3]);
        assert_eq!(
            ledger.workspace_bytes_for_test() - before,
            values.values.capacity() * std::mem::size_of::<usize>()
        );
        drop(values);
        assert_eq!(ledger.workspace_bytes_for_test(), before);
    }
    #[test]
    fn proof_marker_heads_are_canonical_and_budget_exhaustion_never_publishes_a_state() {
        let ledger = MemoryLedger::new();
        let workspace = ProofWorkspace::new(&ledger, 64).unwrap();
        let mut markers = MarkerArena::new(&workspace).unwrap();
        let empty = MarkerStack::default();
        let first = markers.push(empty, 0).unwrap();
        let bytes = ledger.workspace_bytes_for_test();
        assert_eq!(markers.push(empty, 0).unwrap(), first);
        assert_eq!(ledger.workspace_bytes_for_test(), bytes);
        let mut nested = markers.push(first, 9).unwrap();
        assert_eq!(markers.len(nested), 2);
        assert_eq!(markers.last(nested), Some(9));
        assert_eq!(markers.pop(&mut nested), Some(9));
        assert_eq!(nested, first);
        assert_eq!(markers.pop(&mut nested), Some(0));
        assert_eq!(nested, empty);
        drop(markers);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        let workspace = ProofWorkspace::new(&ledger, 1).unwrap();
        let mut markers = MarkerArena::new(&workspace).unwrap();
        markers.push(empty, 0).unwrap();
        assert!(markers.push(empty, 1).is_err());
        assert!(workspace.exhausted());
        assert!(!workspace.admission_denied());
        assert_eq!(markers.nodes.len(), 1);
    }
}
