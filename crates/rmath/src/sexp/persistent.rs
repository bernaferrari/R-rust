#![forbid(unsafe_code)]
//! Owned permanent headers share the arena's checked allocation identity.
//! Raw addresses are compatibility projections; Rust cells own every header.

use crate::sexp::{
    ffi::{NodeBody, SEXP, SEXPTYPE, SexprecCore, Vecsxp},
    heap::{CheckedNode, HeapBackingOwners, HeapError, HeapIdentity, NodeId, NodeLink, NodePage},
    memory::{NodePageRegistration, register_node_page},
    payload::{PayloadLease, PayloadLink},
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

fn empty_payload_shape(header: &SexprecCore) -> bool {
    if !header.payload.is_empty() || header.sxpinfo.type_of() == SEXPTYPE::CHARSXP {
        return false;
    }
    match header.data {
        NodeBody::Vector(vector) => {
            vector.length == 0 || (header.sxpinfo.alt() && vector.length >= 0)
        }
        _ => true,
    }
}
fn scalar_cells<T>(value: T) -> Result<Rc<[Cell<T>]>, HeapError> {
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(1)
        .map_err(|_| HeapError::Allocation)?;
    cells.push(Cell::new(value));
    Ok(Rc::from(cells.into_boxed_slice()))
}
struct PersistentAllocation {
    _registration: NodePageRegistration,
    header: NodePage<SexprecCore>,
    // Fixed interior cells keep a native character projection stable without
    // handing Rust ownership of the bytes to a raw allocation or header.
    payload: Option<PayloadLease>,
}

/// The single physical owner of permanent headers and their payloads.
/// Automatic values retain this same store through the heap backing bag.
pub(crate) struct PersistentBacking {
    nodes: RefCell<HashMap<usize, PersistentAllocation>>,
    // Allocation lookup only: the existing NodePage remains the authority.
    pages: RefCell<HashMap<u64, usize>>,
    identity: HeapIdentity,
    allocated_bytes: Rc<Cell<usize>>,
    transient_bytes: Rc<Cell<usize>>,
}
impl PersistentBacking {
    pub(crate) fn reserve_payload_bytes(
        &self,
        id: &NodeId,
        bytes: usize,
    ) -> Option<crate::sexp::memory::TransientReservation> {
        self.node_snapshot(id)?;
        Some(
            crate::sexp::memory::TransientReservation::new(
                0,
                self.allocated_bytes.get(),
                0,
                &self.transient_bytes,
                bytes,
            )?
            .for_payload(self.identity.clone(), self.allocated_bytes.clone()),
        )
    }
    pub(crate) fn node_payload(&self, id: &NodeId) -> Option<PayloadLease> {
        let key = *self.pages.borrow().get(&id.page_cookie())?;
        let nodes = self.nodes.borrow();
        let allocation = nodes.get(&key)?;
        let header = allocation.header.copy_live(id)?;
        let lease = allocation.payload.as_ref()?;
        lease.matches_header(&header).then(|| lease.clone())
    }
    pub(crate) fn publish_payload(
        &self,
        id: &NodeId,
        expected: PayloadLink,
        lease: &PayloadLease,
    ) -> Option<()> {
        let key = *self.pages.borrow().get(&id.page_cookie())?;
        let mut nodes = self.nodes.borrow_mut();
        let allocation = nodes.get_mut(&key)?;
        let mut header = allocation.header.copy_live(id)?;
        if header.sxpinfo.type_of() == SEXPTYPE::CHARSXP || lease.is_immutable() {
            return None;
        }
        if header.payload != expected
            || allocation
                .payload
                .as_ref()
                .map(PayloadLease::link)
                .unwrap_or(PayloadLink::EMPTY)
                != expected
        {
            return None;
        }
        header.payload = lease.link();
        if !lease.matches_header(&header) {
            return None;
        }
        lease.bind_to_heap(&self.identity, &self.allocated_bytes)?;
        allocation.header.replace_live(id, header).ok()?;
        allocation.payload = Some(lease.clone());
        Some(())
    }
    pub(crate) fn node_snapshot(&self, id: &NodeId) -> Option<SexprecCore> {
        let key = *self.pages.borrow().get(&id.page_cookie())?;
        self.nodes.borrow().get(&key)?.header.copy_live(id)
    }
    pub(crate) fn node_projection(&self, id: &NodeId) -> Option<SEXP> {
        let key = *self.pages.borrow().get(&id.page_cookie())?;
        self.nodes.borrow().get(&key)?.header.resolve(id)
    }
    pub(crate) fn resolve_link(&self, link: NodeLink) -> Option<(SEXP, CheckedNode)> {
        let key = *self.pages.borrow().get(&link.page_cookie())?;
        self.nodes.borrow().get(&key)?.header.resolve_link(link)
    }
    pub(crate) fn replace_node(&self, id: &NodeId, value: SexprecCore) -> Option<()> {
        if !value.has_valid_shape() {
            return None;
        }
        if value.sxpinfo.type_of() == SEXPTYPE::CHARSXP && value.sxpinfo.alt() {
            return None;
        }
        let key = *self.pages.borrow().get(&id.page_cookie())?;
        let mut nodes = self.nodes.borrow_mut();
        let allocation = nodes.get_mut(&key)?;
        let current = allocation.header.copy_live(id)?;
        if (current.sxpinfo.type_of() == SEXPTYPE::CHARSXP
            && (value.sxpinfo.type_of() != SEXPTYPE::CHARSXP
                || value.data.vector().length != current.data.vector().length
                || value.payload != current.payload))
            || (value.sxpinfo.type_of() == SEXPTYPE::CHARSXP
                && current.sxpinfo.type_of() != SEXPTYPE::CHARSXP)
        {
            return None;
        }
        if current.sxpinfo.alt() && current.sxpinfo.type_of() != value.sxpinfo.type_of() {
            return None;
        }
        if matches!(value.data, NodeBody::Vector(_)) {
            if value.payload != current.payload {
                return None;
            }
            match &allocation.payload {
                Some(lease)
                    if lease.matches_header(&value)
                        && (value.sxpinfo.type_of() != SEXPTYPE::CHARSXP
                            || lease.is_immutable()) => {}
                None if empty_payload_shape(&value) => {}
                _ => return None,
            }
        } else if !value.payload.is_empty() {
            return None;
        }
        allocation.header.replace_live(id, value).ok()?;
        if !matches!(value.data, NodeBody::Vector(_)) {
            allocation.payload = None;
        }
        Some(())
    }
    pub(crate) fn attach_resource(&self, id: &NodeId, value: &Rc<dyn std::any::Any>) -> Option<()> {
        self.nodes
            .borrow()
            .get(self.pages.borrow().get(&id.page_cookie())?)?
            .header
            .attach_resource(id, value)
    }
    pub(crate) fn resource(&self, id: &NodeId) -> Option<Rc<dyn std::any::Any>> {
        self.nodes
            .borrow()
            .get(self.pages.borrow().get(&id.page_cookie())?)?
            .header
            .resource(id)
    }
    pub(crate) fn take_resource(&self, id: &NodeId) -> Option<Rc<dyn std::any::Any>> {
        self.nodes
            .borrow()
            .get(self.pages.borrow().get(&id.page_cookie())?)?
            .header
            .take_resource(id)
    }
}

pub(crate) struct PersistentHeap {
    identity: HeapIdentity,
    next_page: usize,
    backing: Rc<PersistentBacking>,
    _owners: Rc<HeapBackingOwners>,
    retired_resources: crate::sexp::heap::RetiredResources,
}
impl PersistentHeap {
    pub(crate) fn new(identity: HeapIdentity) -> Self {
        let backing = Rc::new(PersistentBacking {
            nodes: RefCell::new(HashMap::new()),
            pages: RefCell::new(HashMap::new()),
            identity: identity.clone(),
            allocated_bytes: Rc::new(Cell::new(0)),
            transient_bytes: Rc::new(Cell::new(0)),
        });
        let owners = identity.retain_persistent(backing.clone());
        let retired_resources = owners.resource_drop_queue();
        Self {
            identity,
            next_page: 0,
            backing,
            _owners: owners,
            retired_resources,
        }
    }
    pub(crate) fn heap_identity(&self) -> HeapIdentity {
        self.identity.clone()
    }
    pub(crate) fn link_from_projection(&self, pointer: SEXP) -> Option<NodeLink> {
        self.identity.link_from_projection(pointer)
    }
    fn allocate(
        &mut self,
        mut header: SexprecCore,
        mut payload: Option<PayloadLease>,
    ) -> Result<SEXP, HeapError> {
        if !header.has_valid_shape() {
            return Err(HeapError::InvalidShape);
        }
        if header.sxpinfo.type_of() == SEXPTYPE::CHARSXP && header.sxpinfo.alt() {
            return Err(HeapError::InvalidShape);
        }
        if header.sxpinfo.type_of() == SEXPTYPE::CHARSXP && payload.is_none() {
            if !header.payload.is_empty() || header.data.vector().length != 0 {
                return Err(HeapError::InvalidShape);
            }
            let bytes = crate::sexp::payload::OwnedPayload::characters(b"")
                .map_err(|_| HeapError::Allocation)?;
            let lease = PayloadLease::from_owned(bytes).map_err(|_| HeapError::Allocation)?;
            header.payload = lease.link();
            payload = Some(lease);
        }
        self.backing
            .nodes
            .borrow_mut()
            .try_reserve(1)
            .map_err(|_| HeapError::Allocation)?;
        self.backing
            .pages
            .borrow_mut()
            .try_reserve(1)
            .map_err(|_| HeapError::Allocation)?;
        match &payload {
            Some(lease)
                if lease.matches_header(&header)
                    && (!lease.is_immutable() || header.sxpinfo.type_of() == SEXPTYPE::CHARSXP) =>
            {
                lease
                    .bind_to_heap(&self.identity, &self.backing.allocated_bytes)
                    .ok_or(HeapError::InvalidShape)?;
            }
            None if empty_payload_shape(&header) => {}
            _ => return Err(HeapError::InvalidShape),
        }
        let next_page = self.next_page.checked_add(1).ok_or(HeapError::Allocation)?;
        let mut initial = Some(header);
        let page = NodePage::try_new(self.identity.clone(), self.next_page, 1, || {
            initial
                .take()
                .expect("one header per persistent allocation")
        })?;
        page.metadata().activate(0, true)?;
        let pointer = page.raw_slot(0).ok_or(HeapError::InvalidSlot)?;
        let registration = register_node_page(&page);
        let page_cookie = page.metadata().cookie();
        let character_payload = (header.sxpinfo.type_of() == SEXPTYPE::CHARSXP).then(|| {
            payload
                .as_ref()
                .expect("character allocation has bytes")
                .clone()
        });
        self.backing.nodes.borrow_mut().insert(
            pointer as usize,
            PersistentAllocation {
                _registration: registration,
                header: page,
                payload,
            },
        );
        if let Some(lease) = character_payload {
            lease.make_immutable();
        }
        self.backing
            .pages
            .borrow_mut()
            .insert(page_cookie, pointer as usize);
        self.next_page = next_page;
        Ok(pointer)
    }
    pub(crate) fn allocate_header(&mut self, header: SexprecCore) -> Result<SEXP, HeapError> {
        self.allocate(header, None)
    }
    pub(crate) fn allocate_chars(&mut self, bytes: &[u8]) -> Result<SEXP, HeapError> {
        let length = i64::try_from(bytes.len()).map_err(|_| HeapError::Allocation)?;
        let capacity = bytes.len().checked_add(1).ok_or(HeapError::Allocation)?;
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(capacity)
            .map_err(|_| HeapError::Allocation)?;
        cells.extend(bytes.iter().copied().map(Cell::new));
        cells.push(Cell::new(0));
        let cells: Rc<[Cell<u8>]> = Rc::from(cells.into_boxed_slice());
        let mut header = SexprecCore::new(SEXPTYPE::CHARSXP);
        header.data = NodeBody::Vector(Vecsxp {
            length,
            truelength: 0,
        });
        let payload = PayloadLease::from_byte_cells(cells).map_err(|_| HeapError::Allocation)?;
        header.payload = payload.link();
        self.allocate(header, Some(payload))
    }
    fn allocate_vector(
        &mut self,
        kind: SEXPTYPE,
        payload: PayloadLease,
    ) -> Result<SEXP, HeapError> {
        let mut header = SexprecCore::new_vector(kind, 1);
        header.payload = payload.link();
        self.allocate(header, Some(payload))
    }
    pub(crate) fn allocate_integer(
        &mut self,
        value: i32,
        logical: bool,
    ) -> Result<SEXP, HeapError> {
        self.allocate_vector(
            if logical {
                SEXPTYPE::LGLSXP
            } else {
                SEXPTYPE::INTSXP
            },
            PayloadLease::from_integer_cells(scalar_cells(value)?)
                .map_err(|_| HeapError::Allocation)?,
        )
    }
    pub(crate) fn allocate_real(&mut self, value: f64) -> Result<SEXP, HeapError> {
        self.allocate_vector(
            SEXPTYPE::REALSXP,
            PayloadLease::from_real_cells(scalar_cells(value)?)
                .map_err(|_| HeapError::Allocation)?,
        )
    }
    pub(crate) fn allocate_string(&mut self, value: SEXP) -> Result<SEXP, HeapError> {
        let link = self
            .link_from_projection(value)
            .ok_or(HeapError::InvalidSlot)?;
        self.allocate_vector(
            SEXPTYPE::STRSXP,
            PayloadLease::from_reference_cells(scalar_cells(link)?)
                .map_err(|_| HeapError::Allocation)?,
        )
    }
    pub(crate) fn token(&self, pointer: SEXP) -> Option<CheckedNode> {
        self.backing
            .nodes
            .borrow()
            .get(&(pointer as usize))?
            .header
            .token(0)
    }
    /// Recover the allocation's own cell projection, rather than trusting the
    /// provenance of an address supplied by a native caller.
    pub(crate) fn canonical_projection(&self, pointer: SEXP) -> Option<SEXP> {
        let nodes = self.backing.nodes.borrow();
        let allocation = nodes.get(&(pointer as usize))?;
        allocation.header.token(0)?;
        allocation.header.raw_slot(0)
    }
    pub(crate) fn contains(&self, pointer: SEXP) -> bool {
        self.token(pointer).is_some()
    }
    pub(crate) fn remove(&mut self, pointer: SEXP) -> bool {
        let Some(node) = self.token(pointer) else {
            return false;
        };
        if let Some(resource) = self.backing.take_resource(node.id()) {
            self.retired_resources.borrow_mut().push(resource);
        }
        let Some(allocation) = self.backing.nodes.borrow_mut().remove(&(pointer as usize)) else {
            return false;
        };
        if let Some(token) = allocation.header.token(0) {
            self.backing
                .pages
                .borrow_mut()
                .remove(&token.id().page_cookie());
        }
        true
    }
    pub(crate) fn projections(&self) -> impl Iterator<Item = SEXP> {
        // Release the store loan before tracing can allocate or reenter.
        self.backing
            .nodes
            .borrow()
            .values()
            .filter_map(|owned| owned.header.raw_slot(0))
            .collect::<Vec<_>>()
            .into_iter()
    }
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.backing.nodes.borrow().len()
    }
}

impl Drop for PersistentHeap {
    fn drop(&mut self) {
        drop(crate::sexp::memory::ResourceDropGuard::standalone(
            self.retired_resources.clone(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detached_permanent_payload_snapshot_pins_the_actual_cells_and_charge() {
        let mut permanent = PersistentHeap::new(HeapIdentity::new());
        let heap = permanent.heap_identity();
        let pointer = permanent.allocate_integer(63, false).unwrap();
        let node = permanent.token(pointer).unwrap();
        let lease = heap.payload_lease(&node).unwrap();
        let charge = permanent.backing.allocated_bytes.clone();
        assert_eq!(charge.get(), 4);
        assert!(permanent.remove(pointer));
        assert!(heap.payload_lease(&node).is_none());
        assert_eq!(lease.integer_elt(0), Some(63));
        assert_eq!(charge.get(), 4);
        drop(permanent);
        assert_eq!(lease.integer_elt(0), Some(63));
        drop(lease);
        assert_eq!(charge.get(), 0);
    }

    #[test]
    fn permanent_characters_are_sealed_and_cannot_replace_their_original_storage() {
        let mut permanent = PersistentHeap::new(HeapIdentity::new());
        let heap = permanent.heap_identity();
        let pointer = permanent.allocate_chars(b"stable").unwrap();
        let node = permanent.token(pointer).unwrap();
        let original = heap.node_snapshot(&node).unwrap();
        let lease = heap.payload_lease(&node).unwrap();
        assert!(lease.is_immutable());
        assert!(lease.set_byte_elt(0, b'u').is_none());
        let replacement = PayloadLease::from_owned(
            crate::sexp::payload::OwnedPayload::characters(b"mutate").unwrap(),
        )
        .unwrap();
        let bytes = permanent.backing.allocated_bytes.get();
        assert!(
            heap.publish_payload(&node, original.payload, &replacement)
                .is_none()
        );
        assert!(
            permanent
                .backing
                .publish_payload(node.id(), original.payload, &replacement)
                .is_none()
        );
        let mut raw = original;
        raw.sxpinfo.set_type(SEXPTYPE::RAWSXP);
        assert!(permanent.backing.replace_node(node.id(), raw).is_none());
        let mut lazy = original;
        lazy.sxpinfo.set_alt(true);
        assert!(heap.replace_node(&node, lazy).is_none());
        let mut nil = SexprecCore::new(SEXPTYPE::NILSXP);
        nil.attrib = original.attrib;
        assert!(heap.replace_node(&node, nil).is_none());
        assert_eq!(permanent.backing.allocated_bytes.get(), bytes);
        assert_eq!(heap.node_snapshot(&node).unwrap().payload, original.payload);
        assert!(permanent.remove(pointer));
        assert!(!node.is_live());
        assert_eq!(lease.byte_elt(0), Some(b's'));

        let empty = permanent
            .allocate_header(SexprecCore::new(SEXPTYPE::CHARSXP))
            .unwrap();
        let empty = permanent.token(empty).unwrap();
        let empty = heap.payload_lease(&empty).unwrap();
        assert!(empty.is_immutable());
        assert_eq!(empty.capacity(), 1);
        assert_eq!(empty.byte_elt(0), Some(0));
        let mut lazy = SexprecCore::new(SEXPTYPE::CHARSXP);
        lazy.sxpinfo.set_alt(true);
        let before = permanent.len();
        assert_eq!(
            permanent.allocate_header(lazy),
            Err(HeapError::InvalidShape)
        );
        assert_eq!(permanent.len(), before);
    }

    #[test]
    fn invalid_permanent_shapes_publish_nothing_and_retype_uses_actual_payload() {
        let mut permanent = PersistentHeap::new(HeapIdentity::new());
        let heap = permanent.identity.clone();
        let mut invalid = SexprecCore::new(SEXPTYPE::LISTSXP);
        invalid.data = NodeBody::Other;
        let next_page = permanent.next_page;
        assert_eq!(
            permanent.allocate_header(invalid),
            Err(HeapError::InvalidShape)
        );
        assert_eq!(permanent.len(), 0);
        assert_eq!(permanent.next_page, next_page);
        assert!(permanent.backing.pages.borrow().is_empty());
        assert_eq!(permanent.backing.allocated_bytes.get(), 0);

        let pointer = permanent.allocate_integer(42, true).unwrap();
        let node = permanent.token(pointer).unwrap();
        let original = heap.node_snapshot(&node).unwrap();
        let mut invalid = original;
        invalid.data = NodeBody::Other;
        assert!(heap.replace_node(&node, invalid).is_none());
        assert!(permanent.backing.replace_node(node.id(), invalid).is_none());
        assert_eq!(heap.node_snapshot(&node).unwrap().data, original.data);
        heap.retype_node(&node, SEXPTYPE::INTSXP).unwrap();
        assert!(heap.retype_node(&node, SEXPTYPE::REALSXP).is_none());
        assert_eq!(heap.node_snapshot(&node).unwrap().payload, original.payload);
    }

    #[test]
    fn automatic_lease_retains_permanent_storage_and_releases_it_on_last_drop() {
        let mut heap = PersistentHeap::new(HeapIdentity::new());
        let pointer = heap.allocate_chars(b"owned permanent bytes").unwrap();
        let token = heap.token(pointer).unwrap();
        let lease = token.root_lease().unwrap();
        let backing = Rc::downgrade(&heap.backing);
        drop(heap);
        assert!(token.is_live());
        assert!(backing.upgrade().is_some());
        assert!(crate::sexp::memory::checked_projection(pointer).is_some());
        drop(lease);
        assert!(backing.upgrade().is_none());
        assert!(!token.is_live());
        assert!(crate::sexp::memory::checked_projection(pointer).is_none());
    }

    #[test]
    fn existing_lease_retains_later_stores_in_the_same_heap_domain() {
        let identity = HeapIdentity::new();
        let mut first = PersistentHeap::new(identity.clone());
        let pointer = first.allocate_integer(7, false).unwrap();
        let lease = first.token(pointer).unwrap().root_lease().unwrap();
        let mut later = PersistentHeap::new(identity);
        let later_pointer = later.allocate_chars(b"later").unwrap();
        let later_token = later.token(later_pointer).unwrap();
        let backing = Rc::downgrade(&later.backing);
        drop(first);
        drop(later);
        assert!(later_token.is_live());
        assert!(backing.upgrade().is_some());
        drop(lease);
        assert!(backing.upgrade().is_none());
        assert!(!later_token.is_live());
    }

    #[test]
    fn persistent_tokens_share_heap_and_expire_when_storage_is_removed() {
        let identity = HeapIdentity::new();
        let mut left = PersistentHeap::new(identity.clone());
        let mut right = PersistentHeap::new(identity);
        let first = left
            .allocate_header(SexprecCore::new(SEXPTYPE::LISTSXP))
            .unwrap();
        let token = left.token(first).unwrap();
        let second = right.allocate_chars(b"persistent").unwrap();
        let other = right.token(second).unwrap();
        assert!(token.same_heap(&other));
        assert_ne!(token, other);
        assert!(right.token(first).is_none());
        assert!(left.remove(first));
        assert!(!token.is_live());
        let next = left
            .allocate_header(SexprecCore::new(SEXPTYPE::LISTSXP))
            .unwrap();
        assert_ne!(token, left.token(next).unwrap());
        drop(right);
        // Another facade retains the entire shared physical heap domain.
        assert!(other.is_live());
        drop(left);
        assert!(!other.is_live());
    }
}
