#![forbid(unsafe_code)]
//! Collection-local work carries exact allocation identities, never addresses
//! alone. Metadata leases do not retain the physical header allocation.

use crate::sexp::{
    ffi::SEXP,
    heap::{CheckedNode, HeapIdentity},
    memory,
    session::immutable_singleton_projection,
};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TraceError {
    UnownedProjection(usize),
    ForeignHeap(usize),
    StaleAllocation(usize),
    ProjectionMismatch(usize),
    InvalidPayload(usize),
}

pub(super) struct TraceContext {
    heap: HeapIdentity,
    epoch: u32,
}
impl TraceContext {
    pub(super) fn new(heap: HeapIdentity, epoch: u32) -> Self {
        assert_ne!(epoch, 0, "collection tracing requires an active epoch");
        Self { heap, epoch }
    }

    fn project(&self, candidate: SEXP) -> Result<Option<TraceNode>, TraceError> {
        if candidate.is_null() || immutable_singleton_projection(candidate).is_some() {
            return Ok(None);
        }
        // Address membership cannot rehabilitate the caller's pointer tag.
        // Every task projects its header from the actual owning Cell instead.
        let (projection, token) = memory::checked_projection(candidate)
            .ok_or(TraceError::UnownedProjection(candidate as usize))?;
        let node = TraceNode { projection, token };
        node.validate(self)?;
        Ok(Some(node))
    }
}

/// Task construction recovers an owned Cell projection and its live token.
/// Retaining this task cannot turn a recycled address into a different node.
pub(super) struct TraceNode {
    projection: SEXP,
    token: CheckedNode,
}
impl TraceNode {
    fn validate(&self, context: &TraceContext) -> Result<(), TraceError> {
        if !self.token.belongs_to(&context.heap) {
            return Err(TraceError::ForeignHeap(self.projection as usize));
        }
        if !self.token.is_live() {
            return Err(TraceError::StaleAllocation(self.projection as usize));
        }
        Ok(())
    }

    /// The projection bridge revalidates immediately before copying a header.
    pub(super) fn projection(&self, context: &TraceContext) -> Result<SEXP, TraceError> {
        self.validate(context)?;
        Ok(self.projection)
    }
}

pub(super) struct TraceWorklist {
    context: Rc<TraceContext>,
    pending: Vec<TraceNode>,
}
impl TraceWorklist {
    pub(super) fn new(context: Rc<TraceContext>) -> Self {
        Self {
            context,
            pending: Vec::new(),
        }
    }

    pub(super) fn context(&self) -> &TraceContext {
        &self.context
    }

    pub(super) fn enqueue(&mut self, projection: SEXP) -> Result<(), TraceError> {
        if let Some(node) = self.context.project(projection)? {
            self.pending.push(node);
        }
        Ok(())
    }

    /// Root storage retains its original allocation token across collections.
    /// Check that token before resolving the current occupant of the address.
    pub(super) fn enqueue_checked(
        &mut self,
        candidate: SEXP,
        token: CheckedNode,
    ) -> Result<(), TraceError> {
        if !token.belongs_to(&self.context.heap) {
            return Err(TraceError::ForeignHeap(candidate as usize));
        }
        if !token.is_live() {
            return Err(TraceError::StaleAllocation(candidate as usize));
        }
        let (projection, current) = memory::checked_projection(candidate)
            .ok_or(TraceError::UnownedProjection(candidate as usize))?;
        if token != current {
            return Err(TraceError::ProjectionMismatch(candidate as usize));
        }
        self.pending.push(TraceNode { projection, token });
        Ok(())
    }

    pub(super) fn next_marked(&mut self) -> Result<Option<TraceNode>, TraceError> {
        while let Some(node) = self.pending.pop() {
            node.validate(&self.context)?;
            match node.token.mark(self.context.epoch) {
                Some(false) => return Ok(Some(node)),
                Some(true) => continue,
                None => return Err(TraceError::StaleAllocation(node.projection as usize)),
            }
        }
        Ok(None)
    }
}

thread_local! {
    static CONTEXT: RefCell<Option<Rc<TraceContext>>> = const { RefCell::new(None) };
}

/// The context owns only heap identity and epoch. No owner or node loan crosses
/// collection callbacks; the guard is dropped before notifications execute.
pub(super) struct TraceScope {
    previous: Option<Rc<TraceContext>>,
}
impl TraceScope {
    pub(super) fn enter(context: TraceContext) -> Self {
        let previous = CONTEXT.with(|slot| slot.replace(Some(Rc::new(context))));
        Self { previous }
    }

    pub(super) fn active() -> Rc<TraceContext> {
        CONTEXT.with(|slot| {
            slot.borrow()
                .clone()
                .expect("node tracing outside collection scope")
        })
    }
}
impl Drop for TraceScope {
    fn drop(&mut self) {
        CONTEXT.with(|slot| {
            slot.replace(self.previous.take());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{
        ffi::{SEXPTYPE, SexprecCore},
        heap::NodePage,
        memory::{NodePageRegistration, register_node_page},
    };

    fn registered_page(heap: HeapIdentity) -> (NodePage<SexprecCore>, NodePageRegistration, SEXP) {
        let page = NodePage::try_new(heap, 0, 1, || SexprecCore::new(SEXPTYPE::LISTSXP)).unwrap();
        page.metadata().activate(0, false).unwrap();
        let pointer = page.raw_slot(0).unwrap();
        let registration = register_node_page(&page);
        (page, registration, pointer)
    }

    #[test]
    fn unknown_projection_is_rejected_without_header_access() {
        let header = SexprecCore::new(SEXPTYPE::LISTSXP);
        let pointer = std::ptr::from_ref(&header).cast_mut();
        let context = Rc::new(TraceContext::new(HeapIdentity::new(), 1));
        let mut worklist = TraceWorklist::new(context);
        assert_eq!(
            worklist.enqueue(pointer),
            Err(TraceError::UnownedProjection(pointer as usize))
        );
        assert!(worklist.next_marked().unwrap().is_none());
    }

    #[test]
    fn foreign_heap_projection_is_rejected_without_marking() {
        let (page, _registration, pointer) = registered_page(HeapIdentity::new());
        let context = Rc::new(TraceContext::new(HeapIdentity::new(), 7));
        let mut worklist = TraceWorklist::new(context);
        assert_eq!(
            worklist.enqueue(pointer),
            Err(TraceError::ForeignHeap(pointer as usize))
        );
        assert_eq!(page.metadata().epoch(0), Some(0));
    }

    #[test]
    fn queued_identity_rejects_reused_address_without_marking_replacement() {
        let heap = HeapIdentity::new();
        let (page, _registration, pointer) = registered_page(heap.clone());
        let mut worklist = TraceWorklist::new(Rc::new(TraceContext::new(heap, 9)));
        worklist.enqueue(pointer).unwrap();
        let old = page.token(0).unwrap();
        assert!(page.metadata().release(old.id()));
        page.replace_inactive(0, SexprecCore::new(SEXPTYPE::LISTSXP))
            .unwrap();
        page.metadata().activate(0, false).unwrap();
        assert!(matches!(
            worklist.next_marked(),
            Err(TraceError::StaleAllocation(_))
        ));
        assert_eq!(page.metadata().epoch(0), Some(0));
    }

    #[test]
    fn checked_root_cannot_pair_another_nodes_projection_with_its_token() {
        let heap = HeapIdentity::new();
        let (first, _first_registration, _) = registered_page(heap.clone());
        let (second, _second_registration, second_pointer) = registered_page(heap.clone());
        let mut worklist = TraceWorklist::new(Rc::new(TraceContext::new(heap, 10)));
        assert_eq!(
            worklist.enqueue_checked(second_pointer, first.token(0).unwrap()),
            Err(TraceError::ProjectionMismatch(second_pointer as usize))
        );
        assert_eq!(first.metadata().epoch(0), Some(0));
        assert_eq!(second.metadata().epoch(0), Some(0));
    }

    #[test]
    fn queued_identity_rejects_destroyed_page() {
        let heap = HeapIdentity::new();
        let (page, _registration, pointer) = registered_page(heap.clone());
        let mut worklist = TraceWorklist::new(Rc::new(TraceContext::new(heap, 11)));
        worklist.enqueue(pointer).unwrap();
        drop(page);
        assert!(matches!(
            worklist.next_marked(),
            Err(TraceError::StaleAllocation(_))
        ));
    }

    #[test]
    fn repeated_edges_mark_exact_identity_once_per_epoch() {
        let heap = HeapIdentity::new();
        let (_page, _registration, pointer) = registered_page(heap.clone());
        let mut worklist = TraceWorklist::new(Rc::new(TraceContext::new(heap, 13)));
        worklist.enqueue(pointer).unwrap();
        worklist.enqueue(pointer).unwrap();
        assert!(worklist.next_marked().unwrap().is_some());
        assert!(worklist.next_marked().unwrap().is_none());
    }

    #[test]
    fn scope_restores_owned_context_after_nested_unwind() {
        let heap = HeapIdentity::new();
        let (_page, _registration, pointer) = registered_page(heap.clone());
        let _outer = TraceScope::enter(TraceContext::new(heap, 17));
        let result = std::panic::catch_unwind(|| {
            let _inner = TraceScope::enter(TraceContext::new(HeapIdentity::new(), 19));
            let mut worklist = TraceWorklist::new(TraceScope::active());
            assert_eq!(
                worklist.enqueue(pointer),
                Err(TraceError::ForeignHeap(pointer as usize))
            );
            panic!("exercise trace scope unwind");
        });
        assert!(result.is_err());
        let mut restored = TraceWorklist::new(TraceScope::active());
        restored.enqueue(pointer).unwrap();
        assert!(restored.next_marked().unwrap().is_some());
    }
}
