#![forbid(unsafe_code)]
//! Collection-local work carries exact allocation identities, never addresses
//! alone. Metadata leases do not retain the physical header allocation.

use crate::sexp::{
    ffi::{SEXP, SexprecCore},
    heap::{CheckedNode, HeapIdentity, NodeLink, ResolvedLink},
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
    InvalidLink(NodeLink),
    WorklistAllocation,
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

    pub(super) fn reference_links(&self, node: &TraceNode) -> Option<Vec<NodeLink>> {
        self.heap.reference_links(&node.token)
    }

    fn project_link(&self, link: NodeLink) -> Result<Option<TraceNode>, TraceError> {
        match self
            .heap
            .resolve_link(link)
            .ok_or(TraceError::InvalidLink(link))?
        {
            ResolvedLink::Null | ResolvedLink::Singleton(_) => Ok(None),
            ResolvedLink::Node {
                projection,
                allocation,
            } => {
                let node = TraceNode {
                    projection,
                    token: allocation,
                };
                node.validate(self)?;
                Ok(Some(node))
            }
        }
    }

    fn project(&self, candidate: SEXP) -> Result<Option<TraceNode>, TraceError> {
        if candidate.is_null()
            || self.heap.retained_singleton(candidate).is_some()
            || immutable_singleton_projection(candidate).is_some()
        {
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

    /// Copy the owning Cell only after validating the original allocation.
    /// No header loan or caller-supplied pointer is used to read storage.
    pub(super) fn snapshot(&self, context: &TraceContext) -> Result<SexprecCore, TraceError> {
        self.validate(context)?;
        memory::checked_snapshot(self.projection, &self.token)
            .ok_or(TraceError::ProjectionMismatch(self.projection as usize))
    }

    pub(super) fn address(&self) -> usize {
        self.projection as usize
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
            self.admit(node)?;
        }
        Ok(())
    }

    /// Canonical graph edges retain the saved generation. Resolution never
    /// refreshes an edge from the allocation currently occupying an address.
    pub(super) fn enqueue_link(&mut self, link: NodeLink) -> Result<(), TraceError> {
        if let Some(node) = self.context.project_link(link)? {
            self.admit(node)?;
        }
        Ok(())
    }

    /// Root storage retains its original allocation token across collections.
    /// Project that exact token from its owning store before comparing addresses.
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
        let projection = self
            .context
            .heap
            .node_projection(&token)
            .ok_or(TraceError::UnownedProjection(candidate as usize))?;
        if projection.addr() != candidate.addr() {
            return Err(TraceError::ProjectionMismatch(candidate as usize));
        }
        self.admit(TraceNode { projection, token })
    }

    /// Callers validate the exact heap, generation and projection before this
    /// epoch check. Even already marked input must satisfy those obligations.
    /// A successful collection drains every admitted task before sweeping;
    /// allocation failure aborts tracing, never commits a partial collection.
    fn admit(&mut self, node: TraceNode) -> Result<(), TraceError> {
        match node.token.mark(self.context.epoch) {
            Some(false) => {
                self.pending
                    .try_reserve(1)
                    .map_err(|_| TraceError::WorklistAllocation)?;
                self.pending.push(node);
            }
            Some(true) => {}
            None => return Err(TraceError::StaleAllocation(node.projection as usize)),
        }
        Ok(())
    }

    pub(super) fn next_marked(&mut self) -> Result<Option<TraceNode>, TraceError> {
        if let Some(node) = self.pending.pop() {
            // Admission does not keep physical storage alive. Retirement or
            // address reuse while queued must still reject the saved token.
            node.validate(&self.context)?;
            return Ok(Some(node));
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
        let mut arena = memory::RArena::new();
        let first = arena.alloc_node(SEXPTYPE::LISTSXP);
        let second = arena.alloc_node(SEXPTYPE::LISTSXP);
        let first_token = memory::checked_projection(first).unwrap().1;
        let second_token = memory::checked_projection(second).unwrap().1;
        let mut worklist =
            TraceWorklist::new(Rc::new(TraceContext::new(arena.heap_identity(), 10)));
        assert_eq!(
            worklist.enqueue_checked(second, first_token.clone()),
            Err(TraceError::ProjectionMismatch(second as usize))
        );
        assert_eq!(first_token.mark(10), Some(false));
        assert_eq!(second_token.mark(10), Some(false));
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
    fn repeated_root_admission_is_bounded_by_unique_allocation_identities() {
        let mut arena = memory::RArena::new();
        let pointer = arena.alloc_node(SEXPTYPE::LISTSXP);
        let token = memory::checked_projection(pointer).unwrap().1;
        let link = token.link().unwrap();
        let mut worklist =
            TraceWorklist::new(Rc::new(TraceContext::new(arena.heap_identity(), 29)));
        for _ in 0..1000 {
            worklist.enqueue(pointer).unwrap();
            worklist.enqueue_link(link).unwrap();
            worklist.enqueue_checked(pointer, token.clone()).unwrap();
        }
        assert_eq!(
            worklist.pending.len(),
            1,
            "repeated roots must not allocate repeated tasks"
        );
        assert!(worklist.next_marked().unwrap().is_some());
        assert!(worklist.next_marked().unwrap().is_none());
    }

    #[test]
    fn already_marked_foreign_identity_still_rejects_before_admission() {
        let mut foreign = memory::RArena::new();
        let pointer = foreign.alloc_node(SEXPTYPE::LISTSXP);
        let token = memory::checked_projection(pointer).unwrap().1;
        assert_eq!(token.mark(31), Some(false));
        let mut worklist = TraceWorklist::new(Rc::new(TraceContext::new(HeapIdentity::new(), 31)));
        assert_eq!(
            worklist.enqueue(pointer),
            Err(TraceError::ForeignHeap(pointer as usize))
        );
        assert_eq!(
            worklist.enqueue_checked(pointer, token.clone()),
            Err(TraceError::ForeignHeap(pointer as usize))
        );
        assert_eq!(
            worklist.enqueue_link(token.link().unwrap()),
            Err(TraceError::InvalidLink(token.link().unwrap()))
        );
        assert!(worklist.next_marked().unwrap().is_none());
    }

    #[test]
    fn queued_stale_identity_rejects_after_replacement_is_already_marked() {
        let heap = HeapIdentity::new();
        let (page, _registration, pointer) = registered_page(heap.clone());
        let original = page.token(0).unwrap();
        let mut worklist = TraceWorklist::new(Rc::new(TraceContext::new(heap, 37)));
        worklist.enqueue(pointer).unwrap();
        assert!(page.metadata().release(original.id()));
        page.replace_inactive(0, SexprecCore::new(SEXPTYPE::LISTSXP))
            .unwrap();
        page.metadata().activate(0, false).unwrap();
        let current = page.token(0).unwrap();
        assert_ne!(original.link(), current.link());
        worklist.enqueue(pointer).unwrap();
        assert!(worklist.next_marked().unwrap().is_some());
        assert!(matches!(
            worklist.next_marked(),
            Err(TraceError::StaleAllocation(_))
        ));
        assert_eq!(current.mark(37), Some(true));
    }

    #[test]
    fn canonical_saved_link_rejects_same_address_reuse_without_marking_replacement() {
        let mut session = crate::sexp::session::RSession::new_for_gc_tests();
        let child = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::LISTSXP))
            .unwrap();
        let original = memory::checked_projection(child).unwrap().1;
        let saved = original.link().unwrap();
        super::super::full_gc();
        assert!(!original.is_live());
        let replacement = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::LISTSXP))
            .unwrap();
        assert_eq!(child.addr(), replacement.addr());
        let current = memory::checked_projection(replacement).unwrap().1;
        assert_ne!(saved, current.link().unwrap());
        let mut worklist =
            TraceWorklist::new(Rc::new(TraceContext::new(current.heap_identity(), 23)));
        assert_eq!(
            worklist.enqueue_link(saved),
            Err(TraceError::InvalidLink(saved))
        );
        assert!(worklist.next_marked().unwrap().is_none());
        assert_eq!(current.mark(23), Some(false));
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
