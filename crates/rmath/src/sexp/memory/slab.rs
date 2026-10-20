//! Slab pages, backing store and node page registration.

use super::*;

/// Rust owns the node allocation and safe collector metadata. The `base`
/// address is used only by the legacy projection directory; slot pointers
/// always come from their own interior cell, never a sibling's pointer tag.
pub(super) struct SlabPage {
    pub(super) first_slot: usize,
    pub(super) storage: NodePage<SexprecCore>,
    pub(super) meta: Rc<PageMetadata>,
    pub(super) _registration: NodePageRegistration,
    pub(super) payloads: RefCell<super::slab_slots::SlabSlots>,
}

/// Canonical physical storage shared by the allocator facade and owning
/// values. The RefCell loans cover only local storage operations; callbacks
/// and deferred collection always run after those loans have ended.
pub(crate) struct ArenaBacking {
    pub(super) node_pages: RefCell<Vec<SlabPage>>,
    pub(super) page_cookies: RefCell<HashMap<u64, usize>>,
    pub(super) identity: HeapIdentity,
    pub(super) allocated_bytes: Rc<Cell<usize>>,
    pub(super) max_bytes: Rc<Cell<usize>>,
    pub(super) transient_bytes: Rc<Cell<usize>>,
}

impl ArenaBacking {
    pub(crate) fn node_payload(&self, id: &NodeId) -> Option<PayloadLease> {
        let pages = self.node_pages.borrow();
        let page = pages.get(id.page())?;
        let header = page.storage.copy_live(id)?;
        let lease = page.payloads.borrow().get(&id.slot())?.clone();
        lease.matches_header(&header).then_some(lease)
    }
    pub(crate) fn reserve_payload_bytes(
        &self,
        id: &NodeId,
        bytes: usize,
    ) -> Option<TransientReservation> {
        self.node_snapshot(id)?;
        Some(
            TransientReservation::new(
                self.max_bytes.get(),
                self.allocated_bytes.get(),
                0,
                &self.transient_bytes,
                bytes,
            )?
            .for_payload(self.identity.clone(), self.allocated_bytes.clone()),
        )
    }
    pub(super) fn admit_payload(&self, lease: &PayloadLease) -> Option<()> {
        if !lease.belongs_to(&self.identity) {
            let total = self
                .allocated_bytes
                .get()
                .checked_add(self.transient_bytes.get())?
                .checked_add(lease.storage_bytes())?;
            if self.max_bytes.get() != 0 && total > self.max_bytes.get() {
                return None;
            }
        }
        lease.bind_to_heap(&self.identity, &self.allocated_bytes)
    }
    pub(crate) fn publish_payload(
        &self,
        id: &NodeId,
        expected: PayloadLink,
        lease: &PayloadLease,
    ) -> Option<()> {
        let pages = self.node_pages.borrow();
        let page = pages.get(id.page())?;
        let mut header = page.storage.copy_live(id)?;
        if header.sxpinfo.type_of() == SEXPTYPE::CHARSXP || lease.is_immutable() {
            return None;
        }
        let mut payloads = page.payloads.borrow_mut();
        if header.payload != expected
            || payloads
                .get(&id.slot())
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
        self.admit_payload(lease)?;
        page.storage.replace_live(id, header).ok()?;
        payloads.insert(id.slot(), lease.clone());
        Some(())
    }
    pub(crate) fn node_snapshot(&self, id: &NodeId) -> Option<SexprecCore> {
        self.node_pages
            .borrow()
            .get(id.page())?
            .storage
            .copy_live(id)
    }

    pub(crate) fn node_projection(&self, id: &NodeId) -> Option<SEXP> {
        self.node_pages.borrow().get(id.page())?.storage.resolve(id)
    }

    pub(crate) fn resolve_link(&self, link: NodeLink) -> Option<(SEXP, CheckedNode)> {
        let page = *self.page_cookies.borrow().get(&link.page_cookie())?;
        self.node_pages
            .borrow()
            .get(page)?
            .storage
            .resolve_link(link)
    }
    pub(crate) fn replace_node(&self, id: &NodeId, value: SexprecCore) -> Option<()> {
        if !value.has_valid_shape() {
            return None;
        }
        if value.sxpinfo.type_of() == SEXPTYPE::CHARSXP && value.sxpinfo.alt() {
            return None;
        }
        let pages = self.node_pages.borrow();
        let page = pages.get(id.page())?;
        let current = page.storage.copy_live(id)?;
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
            match page.payloads.borrow().get(&id.slot()) {
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
        page.storage.replace_live(id, value).ok()?;
        if !matches!(value.data, NodeBody::Vector(_)) {
            page.payloads.borrow_mut().remove(&id.slot());
        }
        Some(())
    }
    pub(crate) fn attach_resource(&self, id: &NodeId, value: &Rc<dyn std::any::Any>) -> Option<()> {
        self.node_pages
            .borrow()
            .get(id.page())?
            .storage
            .attach_resource(id, value)
    }
    pub(crate) fn resource(&self, id: &NodeId) -> Option<Rc<dyn std::any::Any>> {
        self.node_pages
            .borrow()
            .get(id.page())?
            .storage
            .resource(id)
    }
    pub(crate) fn take_resource(&self, id: &NodeId) -> Option<Rc<dyn std::any::Any>> {
        self.node_pages
            .borrow()
            .get(id.page())?
            .storage
            .take_resource(id)
    }
}

#[derive(Clone)]
pub(super) struct SlabMetadata {
    base: usize,
    pub(super) meta: Rc<PageMetadata>,
    pub(super) projection: NodeProjection<SexprecCore>,
}

impl SlabMetadata {
    fn slot(&self, address: usize) -> Option<usize> {
        let offset = address.checked_sub(self.base)?;
        (offset < NODE_BYTES * self.meta.slots() && offset.is_multiple_of(NODE_BYTES))
            .then_some(offset / NODE_BYTES)
    }
}

thread_local! {
    pub(super) static SLAB_META: RefCell<BTreeMap<usize, SlabMetadata>> = RefCell::new(BTreeMap::new());
    /// Fast owned metadata lease for the most recently touched range.
    static SLAB_CACHE: RefCell<Option<SlabMetadata>> = const { RefCell::new(None) };
    pub(super) static GC_EPOCH: Cell<u32> = const { Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    static PROJECTION_DIRECTORY_PROBES: Cell<usize> = const { Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn projection_directory_probe_count() -> usize {
    PROJECTION_DIRECTORY_PROBES.with(Cell::get)
}

/// An owned directory entry for an actual owned header page. Drop removes
/// only this page's identity, so a reused address cannot evict its successor.
pub(crate) struct NodePageRegistration {
    base: usize,
    meta: Rc<PageMetadata>,
}

impl Drop for NodePageRegistration {
    fn drop(&mut self) {
        let _ = SLAB_META.try_with(|map| {
            let mut map = map.borrow_mut();
            if map
                .get(&self.base)
                .is_some_and(|entry| Rc::ptr_eq(&entry.meta, &self.meta))
            {
                map.remove(&self.base);
            }
        });
        let _ = SLAB_CACHE.try_with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache
                .as_ref()
                .is_some_and(|entry| entry.base == self.base && Rc::ptr_eq(&entry.meta, &self.meta))
            {
                *cache = None;
            }
        });
    }
}

pub(crate) fn register_node_page(page: &NodePage<SexprecCore>) -> NodePageRegistration {
    let base = page.raw_slot(0).expect("nonempty node page") as usize;
    let meta = page.metadata();
    let entry = SlabMetadata {
        base,
        meta: meta.clone(),
        projection: page.projection(),
    };
    SLAB_META.with(|map| {
        map.borrow_mut().insert(base, entry.clone());
    });
    SLAB_CACHE.with(|cache| {
        *cache.borrow_mut() = Some(entry);
    });
    NodePageRegistration { base, meta }
}

#[inline(always)]
pub(super) fn find_slab_entry(pointer: SEXP) -> Option<(SlabMetadata, usize)> {
    if pointer.is_null() {
        return None;
    }
    #[cfg(test)]
    PROJECTION_DIRECTORY_PROBES.with(|count| count.set(count.get() + 1));
    let address = pointer as usize;
    let cached = SLAB_CACHE
        .try_with(|cache| {
            cache
                .borrow()
                .as_ref()
                .and_then(|entry| entry.slot(address).map(|slot| (entry.clone(), slot)))
        })
        .ok()
        .flatten();
    if cached.is_some() {
        return cached;
    }
    let entry = SLAB_META
        .try_with(|map| {
            map.borrow()
                .range(..=address)
                .next_back()
                .map(|(_, entry)| entry.clone())
        })
        .ok()
        .flatten()?;
    let slot = entry.slot(address)?;
    let _ = SLAB_CACHE.try_with(|cache| {
        *cache.borrow_mut() = Some(entry.clone());
    });
    Some((entry, slot))
}

#[inline(always)]
pub(super) fn find_slab_slot(pointer: SEXP) -> Option<(Rc<PageMetadata>, usize)> {
    let (entry, slot) = find_slab_entry(pointer)?;
    Some((entry.meta, slot))
}
