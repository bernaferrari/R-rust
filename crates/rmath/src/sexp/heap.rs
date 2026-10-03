#![forbid(unsafe_code)]
//! Owned, non-moving node pages and checked allocation identities.
//!
//! The translated runtime may project a raw pointer from a `Cell`; page
//! ownership, allocation identity and collector metadata stay ordinary Rust.
//! IDs never keep a dead node alive and cannot select a reused or foreign slot.

use super::{
    ffi::{EdgeField, NodeBody, SEXP, SEXPTYPE, SexprecCore},
    globals::{SingletonKind, SingletonLease},
};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::HashMap,
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
    sync::atomic::{AtomicU32, Ordering},
};

/// Exact, nonowning graph identity. Zero heap/page identify null or an
/// original immutable lease; managed nodes use nonzero heap/page cookies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(align(8))]
pub(crate) struct NodeLink {
    heap_cookie: u32,
    slot: u32,
    page_cookie: u64,
    generation: u64,
}
impl NodeLink {
    pub(crate) const NULL: Self = Self {
        heap_cookie: 0,
        slot: 0,
        page_cookie: 0,
        generation: 0,
    };
    pub(crate) const fn null() -> Self {
        Self::NULL
    }
    pub(crate) fn is_null(self) -> bool {
        self == Self::NULL
    }
    pub(crate) fn page_cookie(self) -> u64 {
        self.page_cookie
    }
    fn singleton(kind: SingletonKind, cookie: u64) -> Option<Self> {
        (cookie != 0).then_some(Self {
            heap_cookie: 0,
            slot: kind.link_tag(),
            page_cookie: 0,
            generation: cookie,
        })
    }
    fn singleton_identity(self) -> Option<(SingletonKind, u64)> {
        if self.heap_cookie != 0 || self.page_cookie != 0 || self.generation == 0 {
            return None;
        }
        Some((SingletonKind::from_link_tag(self.slot)?, self.generation))
    }
}

#[derive(Debug)]
pub(crate) enum ResolvedLink {
    Null,
    Node {
        projection: SEXP,
        allocation: CheckedNode,
    },
    Singleton(SingletonLease),
}

static NEXT_HEAP_COOKIE: AtomicU32 = AtomicU32::new(1);
fn issue_heap_cookie(counter: &AtomicU32) -> Option<u32> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            if next == 0 { None } else { next.checked_add(1) }
        })
        .ok()
        .filter(|cookie| *cookie != 0)
}
fn issue_page_cookie(counter: &Cell<u64>) -> Option<u64> {
    let next = counter.get();
    let successor = next.checked_add(1)?;
    if next == 0 {
        return None;
    }
    counter.set(successor);
    Some(next)
}

/// Child identities are captured by their caller, never refreshed from an
/// address while a graph edge is being written.
#[derive(Clone, Copy)]
pub(crate) enum ReferenceChild<'a> {
    Null,
    Node(&'a CheckedNode),
    Singleton(&'a SingletonLease),
}

/// The shared domain retains physical storage through one ownership bag.
/// Only the weak link lives here: pages contain this identity themselves.
struct HeapState {
    cookie: u32,
    next_page_cookie: Cell<u64>,
    backing: RefCell<Weak<HeapBackingOwners>>,
}

/// Closed physical owners of the existing canonical pages and payloads.
/// This bag contains no copied headers or alternative node authority.
enum PhysicalBacking {
    Arena(Rc<super::memory::ArenaBacking>),
    Persistent(Rc<super::instance::persistent::PersistentBacking>),
}
impl std::fmt::Debug for PhysicalBacking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Arena(store) => f.debug_tuple("Arena").field(&Rc::as_ptr(store)).finish(),
            Self::Persistent(store) => f
                .debug_tuple("Persistent")
                .field(&Rc::as_ptr(store))
                .finish(),
        }
    }
}

/// A temporary retirement queue, not a registry of attached ownership.
pub(crate) type RetiredResources = Rc<RefCell<Vec<Rc<dyn Any>>>>;

/// Facades and automatic values share this bag, so values also retain any
/// physical stores subsequently added to their heap domain. Stores contain
/// only a HeapIdentity with a weak bag link, never a strong link back here.
pub(crate) struct HeapBackingOwners {
    stores: RefCell<Vec<PhysicalBacking>>,
    singletons: RefCell<Vec<SingletonLease>>,
    retired_resources: RetiredResources,
}
impl HeapBackingOwners {
    /// Transient detached resources only; attached state remains on NodePage.
    pub(crate) fn resource_drop_queue(&self) -> RetiredResources {
        self.retired_resources.clone()
    }
}
impl std::fmt::Debug for HeapBackingOwners {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeapBackingOwners")
            .field("stores", &self.stores.borrow())
            .finish()
    }
}

#[derive(Clone)]
pub(crate) struct HeapIdentity(Rc<HeapState>);
impl std::fmt::Debug for HeapIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HeapIdentity")
            .field(&Rc::as_ptr(&self.0))
            .finish()
    }
}
impl HeapIdentity {
    pub(crate) fn new() -> Self {
        Self(Rc::new(HeapState {
            cookie: issue_heap_cookie(&NEXT_HEAP_COOKIE).expect("heap identity space exhausted"),
            next_page_cookie: Cell::new(1),
            backing: RefCell::new(Weak::new()),
        }))
    }
    fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    fn retained_backing(&self) -> Option<Rc<HeapBackingOwners>> {
        self.0.backing.borrow().upgrade()
    }
    fn retain(&self, store: PhysicalBacking) -> Rc<HeapBackingOwners> {
        let owners = self.retained_backing().unwrap_or_else(|| {
            let owners = Rc::new(HeapBackingOwners {
                stores: RefCell::new(Vec::new()),
                singletons: RefCell::new(Vec::new()),
                retired_resources: Rc::new(RefCell::new(Vec::new())),
            });
            *self.0.backing.borrow_mut() = Rc::downgrade(&owners);
            owners
        });
        owners.stores.borrow_mut().push(store);
        owners
    }
    pub(crate) fn retain_arena(
        &self,
        backing: Rc<super::memory::ArenaBacking>,
    ) -> Rc<HeapBackingOwners> {
        self.retain(PhysicalBacking::Arena(backing))
    }
    pub(crate) fn retain_persistent(
        &self,
        backing: Rc<super::instance::persistent::PersistentBacking>,
    ) -> Rc<HeapBackingOwners> {
        self.retain(PhysicalBacking::Persistent(backing))
    }

    pub(crate) fn retained_singleton(&self, pointer: SEXP) -> Option<SingletonLease> {
        let owners = self.retained_backing()?;
        let result = owners
            .singletons
            .borrow()
            .iter()
            .find(|lease| lease.projection() == pointer)
            .cloned();
        result
    }

    pub(crate) fn node_snapshot(&self, node: &CheckedNode) -> Option<SexprecCore> {
        if !node.belongs_to(self) || !node.is_live() {
            return None;
        }
        let owners = self.retained_backing()?;
        let result = owners.stores.borrow().iter().find_map(|store| match store {
            PhysicalBacking::Arena(store) => store.node_snapshot(node.id()),
            PhysicalBacking::Persistent(store) => store.node_snapshot(node.id()),
        });
        result
    }

    pub(crate) fn node_projection(&self, node: &CheckedNode) -> Option<SEXP> {
        if !node.belongs_to(self) || !node.is_live() {
            return None;
        }
        let owners = self.retained_backing()?;
        let result = owners.stores.borrow().iter().find_map(|store| match store {
            PhysicalBacking::Arena(store) => store.node_projection(node.id()),
            PhysicalBacking::Persistent(store) => store.node_projection(node.id()),
        });
        result
    }

    /// A producer boundary may capture a current projection once. Readers
    /// and collectors resolve saved links instead of calling this bridge.
    pub(crate) fn link_from_projection(&self, pointer: SEXP) -> Option<NodeLink> {
        if pointer.is_null() {
            return Some(NodeLink::NULL);
        }
        if let Some(node) = super::memory::checked_node(pointer) {
            return self.capture_child(ReferenceChild::Node(&node));
        }
        let singleton = self
            .retained_singleton(pointer)
            .or_else(|| super::globals::immutable_singleton_lease(pointer))?;
        self.capture_child(ReferenceChild::Singleton(&singleton))
    }
    fn validate_child(&self, child: ReferenceChild<'_>) -> Option<NodeLink> {
        match child {
            ReferenceChild::Null => Some(NodeLink::NULL),
            ReferenceChild::Node(node) => {
                self.node_projection(node)?;
                node.link()
            }
            ReferenceChild::Singleton(lease) => {
                NodeLink::singleton(lease.kind(), lease.identity_cookie())
            }
        }
    }
    fn retain_children(&self, children: &[ReferenceChild<'_>]) -> Option<()> {
        if !children
            .iter()
            .any(|child| matches!(child, ReferenceChild::Singleton(_)))
        {
            return Some(());
        }
        let owners = self.retained_backing()?;
        let mut retained = owners.singletons.borrow_mut();
        for child in children {
            if let ReferenceChild::Singleton(lease) = child {
                if !retained.iter().any(|owned| {
                    owned.kind() == lease.kind()
                        && owned.identity_cookie() == lease.identity_cookie()
                }) {
                    retained.push((*lease).clone());
                }
            }
        }
        Some(())
    }
    pub(crate) fn capture_child(&self, child: ReferenceChild<'_>) -> Option<NodeLink> {
        let link = self.validate_child(child)?;
        self.retain_children(&[child])?;
        Some(link)
    }
    pub(crate) fn resolve_link(&self, link: NodeLink) -> Option<ResolvedLink> {
        if link.is_null() {
            return Some(ResolvedLink::Null);
        }
        let owners = self.retained_backing()?;
        if let Some((kind, cookie)) = link.singleton_identity() {
            let lease = owners
                .singletons
                .borrow()
                .iter()
                .find(|lease| lease.kind() == kind && lease.identity_cookie() == cookie)
                .cloned()?;
            return Some(ResolvedLink::Singleton(lease));
        }
        if link.heap_cookie != self.0.cookie || link.page_cookie == 0 || link.generation == 0 {
            return None;
        }
        let (projection, allocation) =
            owners
                .stores
                .borrow()
                .iter()
                .find_map(|store| match store {
                    PhysicalBacking::Arena(store) => store.resolve_link(link),
                    PhysicalBacking::Persistent(store) => store.resolve_link(link),
                })?;
        Some(ResolvedLink::Node {
            projection,
            allocation,
        })
    }
    pub(crate) fn projection_of_link(&self, link: NodeLink) -> Option<SEXP> {
        match self.resolve_link(link)? {
            ResolvedLink::Null => Some(std::ptr::null_mut()),
            ResolvedLink::Node { projection, .. } => Some(projection),
            ResolvedLink::Singleton(lease) => Some(lease.projection()),
        }
    }
    pub(crate) fn replace_node(&self, node: &CheckedNode, value: SexprecCore) -> Option<()> {
        if !node.belongs_to(self) || !node.is_live() {
            return None;
        }
        let owners = self.retained_backing()?;
        let result = owners.stores.borrow().iter().find_map(|store| match store {
            PhysicalBacking::Arena(store) => store.replace_node(node.id(), value),
            PhysicalBacking::Persistent(store) => store.replace_node(node.id(), value),
        });
        result
    }
    /// Own native state at the exact canonical external-pointer allocation.
    /// State must not retain owning values or this heap, which would form cycles.
    /// Existing attachments are rejected without replacing or dropping them.
    pub(crate) fn attach_resource(&self, node: &CheckedNode, value: Rc<dyn Any>) -> Option<()> {
        if self.node_snapshot(node)?.sxpinfo.type_of() != SEXPTYPE::EXTPTRSXP {
            return None;
        }
        let owners = self.retained_backing()?;
        let result = owners.stores.borrow().iter().find_map(|store| match store {
            PhysicalBacking::Arena(store) => store.attach_resource(node.id(), &value),
            PhysicalBacking::Persistent(store) => store.attach_resource(node.id(), &value),
        });
        result
    }
    pub(crate) fn resource_erased(&self, node: &CheckedNode) -> Option<Rc<dyn Any>> {
        if self.node_snapshot(node)?.sxpinfo.type_of() != SEXPTYPE::EXTPTRSXP {
            return None;
        }
        let owners = self.retained_backing()?;
        let result = owners.stores.borrow().iter().find_map(|store| match store {
            PhysicalBacking::Arena(store) => store.resource(node.id()),
            PhysicalBacking::Persistent(store) => store.resource(node.id()),
        });
        result
    }
    pub(crate) fn resource<T: Any>(&self, node: &CheckedNode) -> Option<Rc<T>> {
        self.resource_erased(node)?.downcast().ok()
    }
    pub(crate) fn take_resource(&self, node: &CheckedNode) -> Option<Rc<dyn Any>> {
        // Cleanup follows the original live allocation, even if a callback
        // changes its semantic type or body before publication completes.
        self.node_snapshot(node)?;
        let owners = self.retained_backing()?;
        let result = owners.stores.borrow().iter().find_map(|store| match store {
            PhysicalBacking::Arena(store) => store.take_resource(node.id()),
            PhysicalBacking::Persistent(store) => store.take_resource(node.id()),
        });
        result
    }

    pub(crate) fn edge(&self, parent: &CheckedNode, field: EdgeField) -> Option<NodeLink> {
        self.node_snapshot(parent)?.edge(field)
    }
    pub(crate) fn set_edge(
        &self,
        parent: &CheckedNode,
        field: EdgeField,
        child: ReferenceChild<'_>,
    ) -> Option<()> {
        let mut header = self.node_snapshot(parent)?;
        header.edge(field)?;
        let link = self.capture_child(child)?;
        header.set_edge(field, link)?;
        self.replace_node(parent, header)
    }

    /// Header kind, exact generation and typed allocation capacity are all
    /// checked before a graph operation can select a payload cell.
    fn reference_payload(&self, parent: &CheckedNode) -> Option<(*mut u8, usize)> {
        let header = self.node_snapshot(parent)?;
        if !matches!(
            header.sxpinfo.type_of(),
            SEXPTYPE::STRSXP | SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP | SEXPTYPE::BCODESXP
        ) {
            return None;
        }
        let NodeBody::Vector(vector) = header.data else {
            return None;
        };
        let length = usize::try_from(vector.length).ok()?;
        let pointer = header.gengc_next_node.cast::<u8>();
        if length == 0 && pointer.is_null() {
            // Ordinary empty vectors need no allocation. An ALTREP provider
            // with no registered payload has not committed typed storage.
            return (!header.sxpinfo.alt()).then_some((pointer, length));
        }
        let owners = self.retained_backing()?;
        let capacity = owners
            .stores
            .borrow()
            .iter()
            .find_map(|store| match store {
                PhysicalBacking::Arena(store) => store.reference_payload_capacity(pointer),
                PhysicalBacking::Persistent(store) => store.reference_payload_capacity(pointer),
            })?;
        (length <= capacity).then_some((pointer, length))
    }

    /// Clone the actual reference allocation after validating the exact
    /// parent and its logical bounds. Empty/null headers have no allocation.
    pub(crate) fn reference_payload_lease(
        &self,
        parent: &CheckedNode,
    ) -> Option<super::payload::ReferencePayloadLease> {
        let (pointer, _) = self.reference_payload(parent)?;
        let owners = self.retained_backing()?;
        let result = owners.stores.borrow().iter().find_map(|store| match store {
            PhysicalBacking::Arena(store) => store.reference_payload_lease(pointer),
            PhysicalBacking::Persistent(store) => store.reference_payload_lease(pointer),
        });
        result
    }

    pub(crate) fn reference_link_elt(
        &self,
        parent: &CheckedNode,
        index: usize,
    ) -> Option<NodeLink> {
        let (pointer, length) = self.reference_payload(parent)?;
        if index >= length {
            return None;
        }
        let owners = self.retained_backing()?;
        let result = owners.stores.borrow().iter().find_map(|store| match store {
            PhysicalBacking::Arena(store) => store.reference_payload_elt(pointer, index),
            PhysicalBacking::Persistent(store) => store.reference_payload_elt(pointer, index),
        });
        result
    }

    pub(crate) fn reference_links(&self, parent: &CheckedNode) -> Option<Vec<NodeLink>> {
        let (pointer, length) = self.reference_payload(parent)?;
        if length == 0 {
            return Some(Vec::new());
        }
        self.copy_reference_links(pointer, length)
    }
    pub(crate) fn reference_elt(&self, parent: &CheckedNode, index: usize) -> Option<SEXP> {
        self.projection_of_link(self.reference_link_elt(parent, index)?)
    }
    pub(crate) fn reference_elements(&self, parent: &CheckedNode) -> Option<Vec<SEXP>> {
        self.reference_links(parent)?
            .into_iter()
            .map(|link| self.projection_of_link(link))
            .collect()
    }

    pub(crate) fn set_reference_elt(
        &self,
        parent: &CheckedNode,
        index: usize,
        child: ReferenceChild<'_>,
    ) -> Option<()> {
        self.replace_reference_elements(parent, index, &[child])
    }

    /// Resolve every capability and bound before writing any element. Store
    /// loans remain local, and singleton retention owns the actual referent.
    pub(crate) fn replace_reference_elements(
        &self,
        parent: &CheckedNode,
        start: usize,
        children: &[ReferenceChild<'_>],
    ) -> Option<()> {
        let (pointer, length) = self.reference_payload(parent)?;
        if start.checked_add(children.len())? > length {
            return None;
        }
        let values = children
            .iter()
            .copied()
            .map(|child| self.validate_child(child))
            .collect::<Option<Vec<_>>>()?;
        self.retain_children(children)?;
        if children.is_empty() {
            return Some(());
        }
        let owners = self.retained_backing()?;
        owners
            .stores
            .borrow()
            .iter()
            .find_map(|store| match store {
                PhysicalBacking::Arena(store) => {
                    store.replace_reference_payload(pointer, start, &values)
                }
                PhysicalBacking::Persistent(store) => {
                    store.replace_reference_payload(pointer, start, &values)
                }
            })?;
        Some(())
    }
    /// Copy exact reference links from canonical typed owners in this
    /// exact heap domain. The supplied address only selects registered
    /// storage; it is never dereferenced or interpreted as a byte buffer.
    pub(crate) fn copy_reference_links(
        &self,
        pointer: *mut u8,
        length: usize,
    ) -> Option<Vec<NodeLink>> {
        let owners = self.retained_backing()?;
        let result = owners.stores.borrow().iter().find_map(|store| match store {
            PhysicalBacking::Arena(store) => store.copy_reference_payload(pointer, length),
            PhysicalBacking::Persistent(store) => store.copy_reference_payload(pointer, length),
        });
        result
    }
    pub(crate) fn copy_reference_payload(
        &self,
        pointer: *mut u8,
        length: usize,
    ) -> Option<Vec<SEXP>> {
        self.copy_reference_links(pointer, length)?
            .into_iter()
            .map(|link| self.projection_of_link(link))
            .collect()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NodeId {
    heap: HeapIdentity,
    page: usize,
    page_cookie: u64,
    slot: usize,
    generation: u64,
}
impl NodeId {
    pub(crate) fn page(&self) -> usize {
        self.page
    }
    pub(crate) fn page_cookie(&self) -> u64 {
        self.page_cookie
    }
    pub(crate) fn slot(&self) -> usize {
        self.slot
    }
    pub(crate) fn link(&self) -> NodeLink {
        NodeLink {
            heap_cookie: self.heap.0.cookie,
            slot: u32::try_from(self.slot).expect("checked page slot fits link"),
            page_cookie: self.page_cookie,
            generation: self.generation,
        }
    }
}
impl PartialEq for NodeId {
    fn eq(&self, other: &Self) -> bool {
        self.heap.same(&other.heap)
            && self.page == other.page
            && self.page_cookie == other.page_cookie
            && self.slot == other.slot
            && self.generation == other.generation
    }
}
impl Eq for NodeId {}
impl Hash for NodeId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash(Rc::as_ptr(&self.heap.0), state);
        self.page.hash(state);
        self.page_cookie.hash(state);
        self.slot.hash(state);
        self.generation.hash(state);
    }
}

/// An owned metadata lease validates liveness without borrowing the arena.
/// It retains no header storage; page destruction invalidates all its IDs.
#[derive(Clone)]
pub(crate) struct CheckedNode {
    metadata: Rc<PageMetadata>,
    id: NodeId,
}
impl std::fmt::Debug for CheckedNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckedNode")
            .field("id", &self.id)
            .field("live", &self.is_live())
            .finish_non_exhaustive()
    }
}
impl CheckedNode {
    pub(crate) fn new(metadata: Rc<PageMetadata>, id: NodeId) -> Option<Self> {
        metadata.validates(&id).then_some(Self { metadata, id })
    }
    pub(crate) fn is_live(&self) -> bool {
        self.metadata.validates(&self.id)
    }
    pub(crate) fn same_heap(&self, other: &Self) -> bool {
        self.id.heap.same(&other.id.heap)
    }
    pub(crate) fn belongs_to(&self, heap: &HeapIdentity) -> bool {
        self.id.heap.same(heap)
    }
    pub(crate) fn mark(&self, epoch: u32) -> Option<bool> {
        if !self.is_live() {
            return None;
        }
        self.metadata.mark(self.id.slot, epoch)
    }
    pub(crate) fn id(&self) -> &NodeId {
        &self.id
    }
    pub(crate) fn link(&self) -> Option<NodeLink> {
        self.is_live().then(|| self.id.link())
    }
    pub(crate) fn heap_identity(&self) -> HeapIdentity {
        self.id.heap.clone()
    }
    /// Root this exact allocation without borrowing its header or owner.
    /// Cloning the returned Rc shares this single counted lease.
    pub(crate) fn root_lease(&self) -> Option<Rc<NodeRootLease>> {
        self.metadata.acquire_root(&self.id)?;
        Some(Rc::new(NodeRootLease {
            allocation: self.clone(),
            _backing: self.id.heap.retained_backing(),
        }))
    }
    pub(crate) fn root_count(&self) -> usize {
        self.metadata.root_count(&self.id)
    }
}
impl PartialEq for CheckedNode {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for CheckedNode {}

/// An automatic root for one exact allocation generation. Metadata Cells
/// remain accessible during a heap lend and after the physical page closes.
#[derive(Debug)]
pub(crate) struct NodeRootLease {
    allocation: CheckedNode,
    _backing: Option<Rc<HeapBackingOwners>>,
}
impl NodeRootLease {
    pub(crate) fn allocation(&self) -> &CheckedNode {
        &self.allocation
    }
}
impl Drop for NodeRootLease {
    fn drop(&mut self) {
        self.allocation.metadata.release_root(self.allocation.id());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeapError {
    Allocation,
    InvalidSlot,
    LiveSlot,
    RetiredSlot,
    IdentityExhausted,
}

fn cells<T>(len: usize, mut initial: impl FnMut() -> T) -> Result<Box<[Cell<T>]>, HeapError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| HeapError::Allocation)?;
    values.resize_with(len, || Cell::new(initial()));
    Ok(values.into_boxed_slice())
}

/// Shared collector metadata contains no node pointer or node borrow.
pub(crate) struct PageMetadata {
    heap: HeapIdentity,
    page: usize,
    cookie: u64,
    slots: usize,
    retired: Cell<bool>,
    live: Box<[Cell<u64>]>,
    old: Box<[Cell<u64>]>,
    rooted: Box<[Cell<u64>]>,
    roots: Box<[Cell<usize>]>,
    epochs: Box<[Cell<u32>]>,
    generations: Box<[Cell<u64>]>,
}
impl PageMetadata {
    pub(crate) fn try_new(
        heap: HeapIdentity,
        page: usize,
        slots: usize,
    ) -> Result<Self, HeapError> {
        if slots > u32::MAX as usize {
            return Err(HeapError::Allocation);
        }
        let cookie =
            issue_page_cookie(&heap.0.next_page_cookie).ok_or(HeapError::IdentityExhausted)?;
        let words = slots.checked_add(63).ok_or(HeapError::Allocation)? / 64;
        Ok(Self {
            heap,
            page,
            cookie,
            slots,
            retired: Cell::new(false),
            live: cells(words, || 0)?,
            old: cells(words, || 0)?,
            rooted: cells(words, || 0)?,
            roots: cells(slots, || 0)?,
            epochs: cells(slots, || 0)?,
            generations: cells(slots, || 0)?,
        })
    }
    pub(crate) fn page(&self) -> usize {
        self.page
    }
    pub(crate) fn cookie(&self) -> u64 {
        self.cookie
    }
    fn id_from_link(&self, link: NodeLink) -> Option<NodeId> {
        if link.heap_cookie != self.heap.0.cookie || link.page_cookie != self.cookie {
            return None;
        }
        let id = NodeId {
            heap: self.heap.clone(),
            page: self.page,
            page_cookie: self.cookie,
            slot: link.slot as usize,
            generation: link.generation,
        };
        self.validates(&id).then_some(id)
    }
    pub(crate) fn slots(&self) -> usize {
        self.slots
    }
    pub(crate) fn invalidate_all(&self) {
        self.retired.set(true);
        for word in &self.live {
            word.set(0);
        }
        for word in &self.old {
            word.set(0);
        }
        for word in &self.rooted {
            word.set(0);
        }
        for count in &self.roots {
            count.set(0);
        }
        self.clear_epochs();
    }
    pub(crate) fn belongs_to(&self, heap: &HeapIdentity) -> bool {
        self.heap.same(heap)
    }
    pub(crate) fn is_live(&self, slot: usize) -> bool {
        !self.retired.get()
            && slot < self.slots
            && self.live[slot >> 6].get() & (1 << (slot & 63)) != 0
    }
    pub(crate) fn reusable(&self, slot: usize) -> bool {
        !self.retired.get()
            && slot < self.slots
            && !self.is_live(slot)
            && self.generations[slot].get() != u64::MAX
    }
    fn bit(&self, old: bool, slot: usize, on: bool) {
        let words = if old { &self.old } else { &self.live };
        let word = &words[slot >> 6];
        let mask = 1_u64 << (slot & 63);
        word.set(if on {
            word.get() | mask
        } else {
            word.get() & !mask
        });
    }
    pub(crate) fn activate(&self, slot: usize, old: bool) -> Result<NodeId, HeapError> {
        if slot >= self.slots {
            return Err(HeapError::InvalidSlot);
        }
        if self.retired.get() {
            return Err(HeapError::RetiredSlot);
        }
        if self.is_live(slot) {
            return Err(HeapError::LiveSlot);
        }
        let generation = self.generations[slot]
            .get()
            .checked_add(1)
            .ok_or(HeapError::RetiredSlot)?;
        self.generations[slot].set(generation);
        self.epochs[slot].set(0);
        self.roots[slot].set(0);
        self.root_bit(slot, false);
        self.bit(false, slot, true);
        self.bit(true, slot, old);
        Ok(NodeId {
            heap: self.heap.clone(),
            page: self.page,
            page_cookie: self.cookie,
            slot,
            generation,
        })
    }
    pub(crate) fn current_id(&self, slot: usize) -> Option<NodeId> {
        self.is_live(slot).then(|| NodeId {
            heap: self.heap.clone(),
            page: self.page,
            page_cookie: self.cookie,
            slot,
            generation: self.generations[slot].get(),
        })
    }
    pub(crate) fn validates(&self, id: &NodeId) -> bool {
        self.heap.same(&id.heap)
            && self.page == id.page
            && self.cookie == id.page_cookie
            && self.is_live(id.slot)
            && self.generations[id.slot].get() == id.generation
    }
    fn root_bit(&self, slot: usize, on: bool) {
        let word = &self.rooted[slot >> 6];
        let mask = 1_u64 << (slot & 63);
        word.set(if on {
            word.get() | mask
        } else {
            word.get() & !mask
        });
    }
    fn acquire_root(&self, id: &NodeId) -> Option<()> {
        if !self.validates(id) {
            return None;
        }
        let count = self.roots[id.slot].get().checked_add(1)?;
        self.roots[id.slot].set(count);
        self.root_bit(id.slot, true);
        Some(())
    }
    fn release_root(&self, id: &NodeId) {
        // Forced release or owner teardown invalidates the original identity.
        // Such a lease must never decrement a replacement allocation's roots.
        if !self.validates(id) {
            return;
        }
        let previous = self.roots[id.slot].get();
        let count = previous
            .checked_sub(1)
            .expect("live automatic root has a counted metadata lease");
        self.roots[id.slot].set(count);
        if count == 0 {
            self.root_bit(id.slot, false);
        }
    }
    fn root_count(&self, id: &NodeId) -> usize {
        if self.validates(id) {
            self.roots[id.slot].get()
        } else {
            0
        }
    }
    /// Enumerate current rooted IDs using the root bitmap, avoiding a scan
    /// over every unrooted slot on each allocation-time collection.
    pub(crate) fn rooted_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.rooted
            .iter()
            .enumerate()
            .flat_map(move |(index, word)| {
                let mut bits = word.get();
                std::iter::from_fn(move || {
                    while bits != 0 {
                        let bit = bits.trailing_zeros() as usize;
                        bits &= bits - 1;
                        let slot = index * 64 + bit;
                        if let Some(id) = self.current_id(slot) {
                            if self.root_count(&id) > 0 {
                                return Some(id);
                            }
                        }
                    }
                    None
                })
            })
    }
    /// Dense original generations for an iterator transaction. Inactive or
    /// ineligible slots are zero; rootless generation metadata is not a lease.
    /// Snapshotting releases all canonical storage loans before iteration.
    pub(crate) fn snapshot_eligible(&self, old_only: bool) -> (Box<[u64]>, Box<[u64]>) {
        let words = if old_only { &self.old } else { &self.live };
        let bitmap: Box<[u64]> = words.iter().map(Cell::get).collect();
        let generations = self
            .generations
            .iter()
            .enumerate()
            .map(|(slot, value)| {
                if self.is_live(slot) && bitmap[slot >> 6] & (1 << (slot & 63)) != 0 {
                    value.get()
                } else {
                    0
                }
            })
            .collect();
        (bitmap, generations)
    }
    /// Invalidate the current identity before its storage is made reusable.
    /// Generation exhaustion permanently retires the slot instead of wrapping.
    pub(crate) fn release(&self, id: &NodeId) -> bool {
        if !self.validates(id) {
            return false;
        }
        self.bit(false, id.slot, false);
        self.bit(true, id.slot, false);
        self.roots[id.slot].set(0);
        self.root_bit(id.slot, false);
        self.epochs[id.slot].set(0);
        let generation = &self.generations[id.slot];
        generation.set(generation.get().saturating_add(1));
        true
    }
    pub(crate) fn set_old(&self, slot: usize, old: bool) {
        if self.is_live(slot) {
            self.bit(true, slot, old);
        }
    }
    pub(crate) fn epoch(&self, slot: usize) -> Option<u32> {
        self.is_live(slot).then(|| self.epochs[slot].get())
    }
    pub(crate) fn mark(&self, slot: usize, epoch: u32) -> Option<bool> {
        if epoch == 0 || !self.is_live(slot) {
            return None;
        }
        let previous = self.epochs[slot].replace(epoch);
        Some(previous == epoch)
    }
    pub(crate) fn clear_epochs(&self) {
        for epoch in &self.epochs {
            epoch.set(0);
        }
    }
    fn word(&self, old: bool, word: usize) -> u64 {
        let bits = if old { &self.old } else { &self.live };
        bits[word].get()
    }
    pub(crate) fn next_run(&self, old: bool, mut slot: usize) -> Option<(usize, usize)> {
        while slot < self.slots {
            let word_index = slot >> 6;
            let bit = slot & 63;
            let word = self.word(old, word_index);
            if word >> bit == 0 {
                slot = (word_index + 1) << 6;
                continue;
            }
            let start = slot + (word >> bit).trailing_zeros() as usize;
            let mut end = start;
            while end < self.slots {
                let index = end >> 6;
                let bit = end & 63;
                let word = self.word(old, index);
                if bit == 0 && word == u64::MAX {
                    end += 64;
                    continue;
                }
                let ones = (!(word >> bit)).trailing_zeros() as usize;
                if ones == 0 {
                    break;
                }
                end += ones;
                if bit + ones < 64 {
                    break;
                }
            }
            return Some((start, end.min(self.slots)));
        }
        None
    }
    #[cfg(test)]
    fn force_generation(&self, slot: usize, value: u64) {
        self.generations[slot].set(value);
    }
}

struct AttachedResource {
    generation: u64,
    value: Rc<dyn Any>,
}

/// Owned pages expose only interior cells. Forming an arena reference never
/// forms an exclusive reference over previously projected legacy node bytes.
pub(crate) struct NodePage<T> {
    // Rc ownership permits moving the page after legacy pointers have been
    // projected. A movable Box would retag these bytes uniquely on each move.
    values: Rc<[Cell<T>]>,
    metadata: Rc<PageMetadata>,
    resources: RefCell<HashMap<usize, AttachedResource>>,
}

/// A projection directory retains metadata and a weak reference to the one
/// canonical Cell allocation. It cannot prolong header ownership or turn an
/// address-only caller pointer into a pointer with fabricated provenance.
pub(crate) struct NodeProjection<T> {
    values: Weak<[Cell<T>]>,
    metadata: Rc<PageMetadata>,
}
impl<T> Clone for NodeProjection<T> {
    fn clone(&self) -> Self {
        Self {
            values: self.values.clone(),
            metadata: self.metadata.clone(),
        }
    }
}
impl<T> NodeProjection<T> {
    pub(crate) fn resolve_slot(&self, slot: usize) -> Option<(*mut T, CheckedNode)> {
        let token = CheckedNode::new(self.metadata.clone(), self.metadata.current_id(slot)?)?;
        let values = self.values.upgrade()?;
        let pointer = values.get(slot)?.as_ptr();
        Some((pointer, token))
    }
    /// Project only the allocation generation captured by an iterator.
    /// Never refresh an original identity from a reused slot's address.
    pub(crate) fn resolve_generation(
        &self,
        slot: usize,
        generation: u64,
    ) -> Option<(*mut T, CheckedNode)> {
        let id = NodeId {
            heap: self.metadata.heap.clone(),
            page: self.metadata.page,
            page_cookie: self.metadata.cookie,
            slot,
            generation,
        };
        let token = CheckedNode::new(self.metadata.clone(), id)?;
        let values = self.values.upgrade()?;
        Some((values.get(slot)?.as_ptr(), token))
    }
}
impl<T: Copy> NodeProjection<T> {
    pub(crate) fn copy_live(&self, id: &NodeId) -> Option<T> {
        if !self.metadata.validates(id) {
            return None;
        }
        let values = self.values.upgrade()?;
        values.get(id.slot).map(Cell::get)
    }
}
impl<T> Drop for NodePage<T> {
    fn drop(&mut self) {
        self.metadata.invalidate_all();
    }
}
impl<T> NodePage<T> {
    pub(crate) fn try_new(
        heap: HeapIdentity,
        page: usize,
        slots: usize,
        initial: impl FnMut() -> T,
    ) -> Result<Self, HeapError> {
        let metadata = Rc::new(PageMetadata::try_new(heap, page, slots)?);
        Ok(Self {
            values: Rc::from(cells(slots, initial)?),
            metadata,
            resources: RefCell::new(HashMap::new()),
        })
    }
    pub(crate) fn attach_resource(&self, id: &NodeId, value: &Rc<dyn Any>) -> Option<()> {
        if !self.metadata.validates(id) {
            return None;
        }
        let mut resources = self.resources.borrow_mut();
        if resources.contains_key(&id.slot) {
            return None;
        }
        resources.insert(
            id.slot,
            AttachedResource {
                generation: id.generation,
                value: value.clone(),
            },
        );
        Some(())
    }
    pub(crate) fn resource(&self, id: &NodeId) -> Option<Rc<dyn Any>> {
        if !self.metadata.validates(id) {
            return None;
        }
        let resources = self.resources.borrow();
        let resource = resources.get(&id.slot)?;
        (resource.generation == id.generation).then(|| resource.value.clone())
    }
    pub(crate) fn take_resource(&self, id: &NodeId) -> Option<Rc<dyn Any>> {
        if !self.metadata.validates(id) {
            return None;
        }
        let mut resources = self.resources.borrow_mut();
        if resources.get(&id.slot)?.generation != id.generation {
            return None;
        }
        resources.remove(&id.slot).map(|resource| resource.value)
    }

    pub(crate) fn metadata(&self) -> Rc<PageMetadata> {
        self.metadata.clone()
    }
    pub(crate) fn projection(&self) -> NodeProjection<T> {
        NodeProjection {
            values: Rc::downgrade(&self.values),
            metadata: self.metadata.clone(),
        }
    }
    pub(crate) fn resolve_link(&self, link: NodeLink) -> Option<(*mut T, CheckedNode)> {
        let id = self.metadata.id_from_link(link)?;
        let token = CheckedNode::new(self.metadata.clone(), id)?;
        Some((self.values.get(token.id.slot)?.as_ptr(), token))
    }
    pub(crate) fn token(&self, slot: usize) -> Option<CheckedNode> {
        CheckedNode::new(self.metadata.clone(), self.metadata.current_id(slot)?)
    }
    pub(crate) fn raw_slot(&self, slot: usize) -> Option<*mut T> {
        self.values.get(slot).map(Cell::as_ptr)
    }
    pub(crate) fn replace_inactive(&self, slot: usize, value: T) -> Result<*mut T, HeapError> {
        if !self.metadata.reusable(slot) || self.resources.borrow().contains_key(&slot) {
            return Err(HeapError::RetiredSlot);
        }
        let cell = self.values.get(slot).ok_or(HeapError::InvalidSlot)?;
        drop(cell.replace(value));
        Ok(cell.as_ptr())
    }
    /// Update canonical storage only while the original exact allocation
    /// remains live. A stale or foreign identity cannot overwrite a successor.
    pub(crate) fn replace_live(&self, id: &NodeId, value: T) -> Result<(), HeapError> {
        if !self.metadata.validates(id) {
            return Err(HeapError::InvalidSlot);
        }
        self.values
            .get(id.slot)
            .ok_or(HeapError::InvalidSlot)?
            .set(value);
        Ok(())
    }
    pub(crate) fn resolve(&self, id: &NodeId) -> Option<*mut T> {
        self.metadata
            .validates(id)
            .then(|| self.values[id.slot].as_ptr())
    }
}
impl<T: Copy> NodePage<T> {
    pub(crate) fn copy_live(&self, id: &NodeId) -> Option<T> {
        self.metadata
            .validates(id)
            .then(|| self.values[id.slot].get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cookies_exhaust_without_wrapping_or_issuing_reserved_zero() {
        let heap_counter = AtomicU32::new(u32::MAX - 1);
        assert_eq!(issue_heap_cookie(&heap_counter), Some(u32::MAX - 1));
        assert_eq!(issue_heap_cookie(&heap_counter), None);
        assert_eq!(heap_counter.load(Ordering::Relaxed), u32::MAX);
        let reserved = AtomicU32::new(0);
        assert_eq!(issue_heap_cookie(&reserved), None);
        assert_eq!(reserved.load(Ordering::Relaxed), 0);
        let page_counter = Cell::new(u64::MAX - 1);
        assert_eq!(issue_page_cookie(&page_counter), Some(u64::MAX - 1));
        assert_eq!(issue_page_cookie(&page_counter), None);
        assert_eq!(page_counter.get(), u64::MAX);
        let reserved = Cell::new(0);
        assert_eq!(issue_page_cookie(&reserved), None);
        assert_eq!(reserved.get(), 0);
    }

    #[test]
    fn link_identity_rejects_recreated_page_ordinal_and_generation_reuse() {
        let heap = HeapIdentity::new();
        let first = NodePage::try_new(heap.clone(), 0, 1, || 7).unwrap();
        let id = first.metadata().activate(0, false).unwrap();
        let old = first.token(0).unwrap().link().unwrap();
        assert_eq!(first.resolve_link(old).unwrap().1.link(), Some(old));
        assert!(first.metadata().release(&id));
        first.replace_inactive(0, 8).unwrap();
        first.metadata().activate(0, false).unwrap();
        let next = first.token(0).unwrap().link().unwrap();
        assert_ne!(old, next);
        assert!(first.resolve_link(old).is_none());
        assert!(first.resolve_link(next).is_some());
        drop(first);
        let second = NodePage::try_new(heap.clone(), 0, 1, || 9).unwrap();
        second.metadata().activate(0, false).unwrap();
        let recreated = second.token(0).unwrap().link().unwrap();
        assert_ne!(next.page_cookie(), recreated.page_cookie());
        assert!(second.resolve_link(next).is_none());
        // Metadata-only pages never create a second physical heap authority.
        assert!(heap.resolve_link(recreated).is_none());
        assert_eq!(std::mem::size_of::<NodeLink>(), 24);
        assert_eq!(std::mem::align_of::<NodeLink>(), 8);
    }
    #[test]
    fn live_replacement_rejects_stale_and_foreign_exact_allocations() {
        let page = NodePage::try_new(HeapIdentity::new(), 0, 1, || 0).unwrap();
        let original = page.metadata.activate(0, false).unwrap();
        page.replace_live(&original, 7).unwrap();
        assert_eq!(page.copy_live(&original), Some(7));
        let foreign = NodePage::try_new(HeapIdentity::new(), 0, 1, || 0).unwrap();
        let foreign_id = foreign.metadata.activate(0, false).unwrap();
        assert_eq!(
            page.replace_live(&foreign_id, 99),
            Err(HeapError::InvalidSlot)
        );
        assert_eq!(page.copy_live(&original), Some(7));
        assert!(page.metadata.release(&original));
        page.replace_inactive(0, 11).unwrap();
        let replacement = page.metadata.activate(0, false).unwrap();
        assert_eq!(
            page.replace_live(&original, 99),
            Err(HeapError::InvalidSlot)
        );
        assert_eq!(page.copy_live(&replacement), Some(11));
        page.replace_live(&replacement, 13).unwrap();
        assert_eq!(page.copy_live(&replacement), Some(13));
    }

    #[test]
    fn automatic_root_clones_share_one_lease_and_last_drop_unroots() {
        let page = NodePage::try_new(HeapIdentity::new(), 0, 130, || 0).unwrap();
        for slot in [0, 64, 129] {
            page.metadata.activate(slot, false).unwrap();
        }
        let node = page.token(64).unwrap();
        let lease = node.root_lease().unwrap();
        let clone = lease.clone();
        assert_eq!(node.root_count(), 1);
        assert_eq!(lease.allocation(), &node);
        assert_eq!(
            page.metadata.rooted_ids().collect::<Vec<_>>(),
            [node.id().clone()]
        );
        drop(lease);
        assert_eq!(node.root_count(), 1);
        let independent = node.root_lease().unwrap();
        assert_eq!(node.root_count(), 2);
        drop(clone);
        assert_eq!(node.root_count(), 1);
        drop(independent);
        assert_eq!(node.root_count(), 0);
        assert!(page.metadata.rooted_ids().next().is_none());
    }

    #[test]
    fn stale_automatic_root_drop_cannot_unroot_replacement_or_revive_closed_page() {
        let page = NodePage::try_new(HeapIdentity::new(), 0, 1, || 0).unwrap();
        let old_id = page.metadata.activate(0, false).unwrap();
        let old = page.token(0).unwrap();
        let old_lease = old.root_lease().unwrap();
        assert!(page.metadata.release(&old_id));
        page.replace_inactive(0, 7).unwrap();
        page.metadata.activate(0, false).unwrap();
        let replacement = page.token(0).unwrap();
        let replacement_lease = replacement.root_lease().unwrap();
        assert!(old.root_lease().is_none());
        drop(old_lease);
        assert_eq!(replacement.root_count(), 1);
        assert_eq!(
            page.metadata.rooted_ids().collect::<Vec<_>>(),
            [replacement.id().clone()]
        );
        let metadata = page.metadata();
        drop(page);
        assert!(replacement.root_lease().is_none());
        assert_eq!(replacement.root_count(), 0);
        assert!(metadata.rooted_ids().next().is_none());
        drop(replacement_lease);
        assert!(metadata.rooted_ids().next().is_none());
    }

    #[test]
    fn automatic_root_overflow_fails_without_changing_the_count() {
        let page = NodePage::try_new(HeapIdentity::new(), 0, 1, || 0).unwrap();
        page.metadata.activate(0, false).unwrap();
        let node = page.token(0).unwrap();
        let lease = node.root_lease().unwrap();
        page.metadata.roots[0].set(usize::MAX);
        assert!(node.root_lease().is_none());
        assert_eq!(node.root_count(), usize::MAX);
        assert_eq!(page.metadata.rooted[0].get(), 1);
        page.metadata.roots[0].set(1);
        drop(lease);
    }

    #[test]
    fn page_drop_invalidates_surviving_metadata_leases() {
        let page = NodePage::try_new(HeapIdentity::new(), 0, 2, || 0).unwrap();
        let metadata = page.metadata();
        page.replace_inactive(0, 9).unwrap();
        metadata.activate(0, false).unwrap();
        let token = page.token(0).unwrap();
        let projection = page.projection();
        assert!(projection.resolve_slot(0).is_some());
        assert_eq!(projection.copy_live(token.id()), Some(9));
        assert!(token.is_live());
        drop(page);
        assert!(!token.is_live());
        assert!(projection.resolve_slot(0).is_none());
        assert_eq!(projection.copy_live(token.id()), None);
        assert!(!metadata.reusable(0));
        assert!(!metadata.reusable(1));
        assert_eq!(metadata.activate(1, false), Err(HeapError::RetiredSlot));
    }

    #[test]
    fn same_heap_pages_with_equal_numbers_have_distinct_identities() {
        let heap = HeapIdentity::new();
        let first = NodePage::try_new(heap.clone(), 0, 1, || 0).unwrap();
        let second = NodePage::try_new(heap, 0, 1, || 0).unwrap();
        let first_id = first.metadata.activate(0, false).unwrap();
        let second_id = second.metadata.activate(0, false).unwrap();
        assert_ne!(first_id, second_id);
        assert!(first.token(0).unwrap().same_heap(&second.token(0).unwrap()));
        assert_eq!(first.resolve(&second_id), None);
        assert_eq!(second.resolve(&first_id), None);
        assert_eq!(first.copy_live(&second_id), None);
        assert_eq!(first.projection().copy_live(&second_id), None);
    }

    #[test]
    fn reused_slots_reject_stale_and_foreign_heap_ids() {
        let identity = HeapIdentity::new();
        let page = NodePage::try_new(identity.clone(), 0, 4, || 0_i32).unwrap();
        let pointer = page.replace_inactive(0, 7).unwrap();
        let old = page.metadata.activate(0, false).unwrap();
        assert_eq!(page.resolve(&old), Some(pointer));
        assert_eq!(page.copy_live(&old), Some(7));
        let projection = page.projection();
        assert_eq!(projection.copy_live(&old), Some(7));
        assert!(page.metadata.release(&old));
        assert_eq!(page.resolve(&old), None);
        assert_eq!(page.copy_live(&old), None);
        assert_eq!(projection.copy_live(&old), None);
        assert_eq!(page.replace_inactive(0, 11).unwrap(), pointer);
        let new = page.metadata.activate(0, false).unwrap();
        assert_ne!(old, new);
        assert_eq!(page.resolve(&old), None);
        assert_eq!(page.resolve(&new), Some(pointer));
        assert_eq!(page.copy_live(&new), Some(11));
        assert_eq!(projection.copy_live(&new), Some(11));
        assert_eq!(page.token(0).unwrap().mark(17), Some(false));
        assert_eq!(page.token(0).unwrap().mark(17), Some(true));
        let stale = CheckedNode {
            metadata: page.metadata(),
            id: old,
        };
        assert_eq!(stale.mark(19), None);
        assert_eq!(page.metadata.epoch(0), Some(17));
        let foreign = NodePage::try_new(HeapIdentity::new(), 0, 4, || 0).unwrap();
        foreign.replace_inactive(0, 3).unwrap();
        let foreign_id = foreign.metadata.activate(0, false).unwrap();
        assert_eq!(page.resolve(&foreign_id), None);
        assert_eq!(page.copy_live(&foreign_id), None);
        assert_eq!(projection.copy_live(&foreign_id), None);
    }
    #[test]
    fn generation_exhaustion_retires_storage_without_aliasing_old_ids() {
        let page = NodePage::try_new(HeapIdentity::new(), 0, 1, || 0).unwrap();
        page.metadata.force_generation(0, u64::MAX - 1);
        page.replace_inactive(0, 7).unwrap();
        let last = page.metadata.activate(0, false).unwrap();
        assert!(page.metadata.release(&last));
        assert_eq!(page.resolve(&last), None);
        assert_eq!(
            page.metadata.activate(0, false),
            Err(HeapError::RetiredSlot)
        );
        assert_eq!(page.replace_inactive(0, 11), Err(HeapError::RetiredSlot));
    }
    #[test]
    fn collector_runs_follow_live_and_old_slots_and_clear_reused_marks() {
        let meta = PageMetadata::try_new(HeapIdentity::new(), 0, 130).unwrap();
        let mut ids = Vec::new();
        for slot in 0..130 {
            ids.push(meta.activate(slot, slot % 2 == 0).unwrap());
        }
        assert_eq!(meta.next_run(false, 0), Some((0, 130)));
        assert_eq!(meta.next_run(true, 0), Some((0, 1)));
        assert_eq!(meta.next_run(true, 1), Some((2, 3)));
        assert_eq!(meta.mark(64, 17), Some(false));
        assert_eq!(meta.mark(64, 17), Some(true));
        assert!(meta.release(&ids[64]));
        assert_eq!(meta.next_run(false, 0), Some((0, 64)));
        assert_eq!(meta.next_run(false, 64), Some((65, 130)));
        let replacement = meta.activate(64, false).unwrap();
        assert_ne!(replacement, ids[64]);
        assert_eq!(meta.epoch(64), Some(0));
        assert_eq!(meta.mark(64, 17), Some(false));
        meta.clear_epochs();
        assert_eq!(meta.epoch(64), Some(0));
    }
}
