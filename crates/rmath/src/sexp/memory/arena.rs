//! RArena core: allocation, lending, reservations, with_arena*.

use super::slab_slots::SlotOffset;
use super::*;

/// An arena allocator for R objects.
///
/// Allocates SexprecCore nodes and their associated vector data.
/// Collection retires individual allocation identities and reuses their slots.
/// Owning values retain these pages and payloads after the facade drops.
pub struct RArena {
    /// Nonowning ASCII hints, capped at 4096 entries and 256 KiB of keys.
    pub(super) character_cache: characters::CharacterCache,
    pub(super) bytecode_string_cache: bytecode_strings::BytecodeStringCache,
    /// Owned pages of interior cells keep legacy header projections stable.
    /// Node identities and collector metadata are independent of raw headers.
    pub(super) backing: Rc<ArenaBacking>,
    retired_resources: crate::sexp::heap::RetiredResources,
    _backing_owners: Rc<HeapBackingOwners>,
    heap_identity: HeapIdentity,
    /// Current page index for allocation (last page usually).
    slab_page: usize,
    /// Current offset within the current page's admitted header capacity.
    pub(super) slab_offset: usize,
    /// Counts and a local logical head; all per-slot links are already funded.
    live_count: usize,
    reclaimed_count: usize,
    reclaimed_head: Option<SlotOffset>,
    /// Total bytes allocated for tracking.
    pub(super) allocated_bytes: Rc<Cell<usize>>,
    /// Bytes reserved by temporary native workspaces which live outside the
    /// arena (for example, a numerical transform's scratch Vecs).
    pub(super) transient_bytes: Rc<Cell<usize>>,
    /// Deferred alloc-time GC hook state (see `with_arena_in`): arena
    /// methods cannot touch instance state without aliasing the live
    /// borrow, so hooks record their firings here instead.
    pub(super) alloc_gc_torture_ticks: u32,
    alloc_gc_collect_requested: bool,
    /// Live size after the last collection. Triggers use *growth* since
    /// then so a large live heap does not GC on every `{`.
    pub(crate) nodes_at_last_gc: usize,
    bytes_at_last_gc: usize,
    /// Optional budget to limit arena growth.
    pub(super) budget: ArenaBudget,
    /// Host-level node guard: cumulative node admissions (fresh slab slot
    /// or free-list reuse) charged while `guard_max_nodes > 0`. Unlike
    /// `budget.max_nodes` (live-node cap for raw arena clients), the guard
    /// is never refunded by collection. This optional work quota is separate
    /// from live-node admission; app hosts use execution time and cancellation
    /// to bound work without denying reuse of collected nodes.
    pub(super) guard_max_nodes: usize,
    pub(super) guard_nodes_admitted: usize,
}

impl RArena {
    fn current_page_capacity(&self) -> usize {
        self.backing
            .node_pages
            .borrow()
            .get(self.slab_page)
            .map_or(0, |page| page.meta.slots())
    }

    /// Admit a complete initialized header buffer before allocating it. Small
    /// budgets use smaller pages; unused slots and reuse are already prepaid.
    fn alloc_new_page(&mut self) -> bool {
        let first_slot = self
            .backing
            .node_pages
            .borrow()
            .last()
            .map_or(Some(0), |page| {
                page.first_slot.checked_add(page.meta.slots())
            });
        let Some(first_slot) = first_slot else {
            return false;
        };
        let Some((descriptor_capacity, descriptor_bytes)) =
            self.backing.node_pages.borrow().growth()
        else {
            return false;
        };
        let slots = if self.budget.max_bytes == 0 {
            NODE_PAGE_SIZE
        } else {
            let Some(remaining) = self
                .budget
                .max_bytes
                .checked_sub(self.allocated_bytes.get())
                .and_then(|n| n.checked_sub(self.transient_bytes.get()))
                .and_then(|n| n.checked_sub(descriptor_bytes))
            else {
                return false;
            };
            // Do not consume a constrained heap with speculative empty
            // headers: leave room for later payloads. A one-header budget
            // must still admit its single node.
            let available = remaining / NODE_BYTES;
            (available / 2)
                .max(usize::from(available != 0))
                .min(NODE_PAGE_SIZE)
        };
        if slots == 0 || first_slot.checked_add(slots).is_none() {
            return false;
        }
        let Some(bytes) = slots.checked_mul(NODE_BYTES) else {
            return false;
        };
        let Some(admission) = self.reserve_workspace(bytes) else {
            return false;
        };
        let Some(index_bytes) = super::slab_slots::SlabSlots::requested_bytes(slots) else {
            return false;
        };
        let Some(index_admission) = self.reserve_workspace(index_bytes) else {
            return false;
        };
        let prepared = if descriptor_bytes == 0 {
            None
        } else {
            let Some(admission) = self.reserve_workspace(descriptor_bytes) else {
                return false;
            };
            let Some(prepared) =
                super::page_store::PageStore::prepare_growth(descriptor_capacity, admission)
            else {
                return false;
            };
            Some(prepared)
        };
        let Ok(mut storage) = NodePage::try_new(
            self.heap_identity.clone(),
            self.backing.node_pages.borrow().len(),
            slots,
            || SexprecCore::new(SEXPTYPE::NILSXP),
        ) else {
            return false;
        };
        let Some(payloads) = super::slab_slots::SlabSlots::try_new(slots, index_admission) else {
            return false;
        };
        let Some(charge) = admission.commit_allocation() else {
            return false;
        };
        if storage.attach_header_charge(charge).is_none() {
            return false;
        }
        let meta = storage.metadata();
        let registration = register_node_page(&storage);
        let mut pages = self.backing.node_pages.borrow_mut();
        if pages
            .commit_page(
                SlabPage {
                    first_slot,
                    storage,
                    meta,
                    _registration: registration,
                    payloads: RefCell::new(payloads),
                },
                prepared,
            )
            .is_none()
        {
            return false;
        }
        self.slab_page = pages.len() - 1;
        self.slab_offset = 0;
        true
    }

    /// Allocate a core node into the current slab page (creating new page if needed).
    /// Returns the raw SEXP ptr. Updates accounting and active tracking.
    /// Used by scalar/vector/CHARS allocs to avoid duplication (elegance + maintainability).
    #[inline(always)]
    pub(super) fn allocate_core_in_slab<F>(&mut self, ctor: F) -> SEXP
    where
        F: FnOnce() -> SexprecCore,
    {
        self.allocate_core_with_payload(ctor(), None)
    }

    fn allocate_core_with_payload(
        &mut self,
        mut header: SexprecCore,
        mut payload: Option<PayloadLease>,
    ) -> SEXP {
        if !header.has_valid_shape() {
            return ptr::null_mut();
        }
        if header.sxpinfo.type_of() == SEXPTYPE::CHARSXP && header.sxpinfo.alt() {
            return ptr::null_mut();
        }
        if header.sxpinfo.type_of() == SEXPTYPE::CHARSXP && payload.is_none() {
            let empty_storage = OwnedPayload::requested_byte_storage_bytes(1)
                .expect("empty character chunk layout");
            if !header.payload.is_empty()
                || header.data.vector().length != 0
                || !self.can_allocate_node_with_payload(empty_storage)
            {
                return ptr::null_mut();
            }
            let Ok(bytes) = OwnedPayload::characters(b"") else {
                return ptr::null_mut();
            };
            let Ok(lease) = PayloadLease::from_owned(bytes) else {
                return ptr::null_mut();
            };
            header.payload = lease.link();
            payload = Some(lease);
        }
        let payload_growth = payload
            .as_ref()
            .filter(|lease| !lease.belongs_to(&self.heap_identity))
            .map_or(0, PayloadLease::storage_bytes);
        if !self.can_allocate_node_with_payload(payload_growth) {
            return ptr::null_mut();
        }
        match &payload {
            Some(lease)
                if lease.matches_header(&header)
                    && (!lease.is_immutable() || header.sxpinfo.type_of() == SEXPTYPE::CHARSXP) =>
            {
                if self.backing.admit_payload(lease).is_none() {
                    return ptr::null_mut();
                }
            }
            None if empty_payload_shape(&header) => {}
            _ => return ptr::null_mut(),
        }
        while let Some((page, slot)) = self.pop_reclaimed_slot() {
            if !self.backing.node_pages.borrow()[page].meta.reusable(slot) {
                continue;
            }
            let ptr = self.backing.node_pages.borrow()[page]
                .storage
                .replace_inactive(slot, header)
                .expect("reusable arena slot");
            if let Some(lease) = payload {
                self.backing.node_pages.borrow()[page]
                    .payloads
                    .borrow_mut()
                    .insert(slot, lease.clone());
                if header.sxpinfo.type_of() == SEXPTYPE::CHARSXP {
                    lease.make_immutable();
                }
            }
            return self.register_new_node(ptr);
        }
        if self.slab_offset >= self.current_page_capacity() && !self.alloc_new_page() {
            return ptr::null_mut();
        }
        let ptr = self.backing.node_pages.borrow()[self.slab_page]
            .storage
            .replace_inactive(self.slab_offset, header)
            .expect("fresh inactive arena slot");
        if let Some(lease) = payload {
            self.backing.node_pages.borrow()[self.slab_page]
                .payloads
                .borrow_mut()
                .insert(self.slab_offset, lease.clone());
            if header.sxpinfo.type_of() == SEXPTYPE::CHARSXP {
                lease.make_immutable();
            }
        }
        self.slab_offset += 1;
        self.register_new_node(ptr)
    }

    /// Create a new empty arena with an unlimited budget.
    pub fn new() -> Self {
        Self::with_budget(ArenaBudget::unlimited())
    }

    /// Create a new empty arena with the given budget.
    pub fn with_budget(budget: ArenaBudget) -> Self {
        Self::with_budget_and_identity(budget, HeapIdentity::new())
    }

    /// Reset an arena fixture without changing the session's persistent heap
    /// domain. New page identities still invalidate all old arena handles.
    #[cfg(test)]
    pub(crate) fn fresh_with_identity(identity: HeapIdentity) -> Self {
        Self::with_budget_and_identity(ArenaBudget::unlimited(), identity)
    }

    fn with_budget_and_identity(budget: ArenaBudget, heap_identity: HeapIdentity) -> Self {
        let super::ledger::ArenaCounters {
            allocated: allocated_bytes,
            transient: transient_bytes,
            limit,
        } = heap_identity.byte_ledger().counters();
        limit.set(budget.max_bytes);
        let backing = Rc::new(ArenaBacking {
            node_pages: RefCell::new(super::page_store::PageStore::new()),
            identity: heap_identity.clone(),
            allocated_bytes: allocated_bytes.clone(),
            max_bytes: limit,
            transient_bytes: transient_bytes.clone(),
        });
        let backing_owners = heap_identity.retain_arena(backing.clone());
        let retired_resources = backing_owners.resource_drop_queue();
        RArena {
            character_cache: characters::CharacterCache::default(),
            bytecode_string_cache: bytecode_strings::BytecodeStringCache::default(),
            backing,
            _backing_owners: backing_owners,
            retired_resources,
            heap_identity,
            slab_page: 0,
            slab_offset: NODE_PAGE_SIZE,
            live_count: 0,
            reclaimed_count: 0,
            reclaimed_head: None,
            allocated_bytes,
            transient_bytes,
            alloc_gc_torture_ticks: 0,
            alloc_gc_collect_requested: false,
            nodes_at_last_gc: 0,
            bytes_at_last_gc: 0,
            budget,
            guard_max_nodes: 0,
            guard_nodes_admitted: 0,
        }
    }

    fn track_node_active(&mut self, ptr: SEXP) {
        if ptr.is_null() {
            return;
        }
        let Some((meta, slot)) = find_slab_slot(ptr) else {
            debug_assert!(ptr.is_null(), "active node is not in a slab page");
            return;
        };
        let id = meta
            .activate(slot, false)
            .expect("inactive nonretired arena node");
        let header = self.backing.node_pages.borrow()[meta.page()]
            .storage
            .copy_live(&id)
            .expect("initialized owned arena node");
        meta.set_old(slot, header.sxpinfo.gcgen() == 1);
        self.live_count = self
            .live_count
            .checked_add(1)
            .expect("arena live node count");
        let allocation = CheckedNode::new(meta, id).expect("new live arena allocation");
        note_lend_allocation(std::ptr::from_ref(self).addr(), ptr, allocation);
    }

    fn local_slot(&self, pointer: SEXP) -> Option<(usize, usize)> {
        let (metadata, slot) = find_slab_slot(pointer)?;
        let pages = self.backing.node_pages.borrow();
        let page = pages.get(metadata.page())?;
        Rc::ptr_eq(&page.meta, &metadata).then_some((metadata.page(), slot))
    }

    fn track_node_freed(&mut self, pointer: SEXP) {
        let (page, slot) = self.local_slot(pointer).expect("original live arena slot");
        let pages = self.backing.node_pages.borrow();
        let meta = &pages[page].meta;
        let id = meta
            .current_id(slot)
            .expect("retiring original live generation");
        meta.release(&id);
        self.live_count = self
            .live_count
            .checked_sub(1)
            .expect("arena live node balance");
    }

    fn reclaimed_location(&self, offset: SlotOffset) -> Option<(usize, usize)> {
        let index = offset.index();
        let pages = self.backing.node_pages.borrow();
        let page_index = pages
            .partition_point(|p| p.first_slot <= index)
            .checked_sub(1)?;
        let page = &pages[page_index];
        let slot = index.checked_sub(page.first_slot)?;
        (slot < page.meta.slots()).then_some((page_index, slot))
    }

    fn pop_reclaimed_slot(&mut self) -> Option<(usize, usize)> {
        let head = self.reclaimed_head?;
        let (page, slot) = self
            .reclaimed_location(head)
            .expect("reclamation link belongs to original arena");
        let pages = self.backing.node_pages.borrow();
        let mut records = pages[page].payloads.borrow_mut();
        self.reclaimed_head = records.next_free(slot).expect("admitted reclamation slot");
        records
            .set_next_free(slot, None)
            .expect("admitted reclamation slot");
        self.reclaimed_count = self
            .reclaimed_count
            .checked_sub(1)
            .expect("reclamation slot balance");
        Some((page, slot))
    }

    fn has_reusable_slot(&self) -> bool {
        let mut cursor = self.reclaimed_head;
        for _ in 0..self.reclaimed_count {
            let Some(head) = cursor else {
                break;
            };
            let Some((page, slot)) = self.reclaimed_location(head) else {
                return false;
            };
            let pages = self.backing.node_pages.borrow();
            if pages[page].meta.reusable(slot) {
                return true;
            }
            cursor = pages[page].payloads.borrow().next_free(slot).flatten();
        }
        false
    }

    #[cfg(test)]
    pub(super) fn reclaimed_snapshot(&self) -> Vec<SEXP> {
        let mut result = Vec::new();
        let mut cursor = self.reclaimed_head;
        for _ in 0..self.reclaimed_count {
            let head = cursor.expect("exact reclamation count");
            let (page, slot) = self
                .reclaimed_location(head)
                .expect("original reclamation slot");
            let pages = self.backing.node_pages.borrow();
            result.push(
                pages[page]
                    .storage
                    .raw_slot(slot)
                    .expect("original header projection"),
            );
            cursor = pages[page]
                .payloads
                .borrow()
                .next_free(slot)
                .expect("admitted reclamation slot");
        }
        assert!(cursor.is_none());
        result.reverse();
        result
    }

    /// Minimum complete backing for a new one-slot arena page, excluding
    /// the vector/character payload itself. Existing slots are already prepaid.
    pub(crate) fn minimum_page_bytes() -> usize {
        Self::minimum_slot_page_bytes() + super::page_store::PageStore::minimum_bytes()
    }

    fn minimum_slot_page_bytes() -> usize {
        NODE_BYTES
            + PageMetadata::requested_array_bytes(1).expect("one-slot metadata layout")
            + super::slab_slots::SlabSlots::requested_bytes(1).expect("one-slot index layout")
    }

    fn minimum_fresh_backing_bytes(&self) -> usize {
        if self.slab_offset < self.current_page_capacity() || self.has_reusable_slot() {
            0
        } else {
            self.backing
                .node_pages
                .borrow()
                .growth()
                .and_then(|(_, bytes)| Self::minimum_slot_page_bytes().checked_add(bytes))
                .unwrap_or(usize::MAX)
        }
    }

    fn register_new_node(&mut self, ptr: SEXP) -> SEXP {
        self.guard_nodes_admitted = self.guard_nodes_admitted.saturating_add(1);
        self.track_node_active(ptr);
        ptr
    }

    /// Return the current arena budget.
    pub fn budget(&self) -> ArenaBudget {
        self.budget
    }

    /// Set a new budget. Does not retroactively reject existing allocations.
    pub fn set_budget(&mut self, budget: ArenaBudget) {
        self.budget = budget;
        self.backing.max_bytes.set(budget.max_bytes);
        note_lend_budget(std::ptr::from_ref(self) as usize, budget.max_bytes);
    }

    /// Reserve native scratch space against this session's byte budget.
    ///
    /// The returned guard releases the reservation on every exit path,
    /// including an R error or cancellation unwind. The allocation itself is
    /// owned by the caller; this only accounts for its peak workspace.
    pub(crate) fn try_reserve_transient(&mut self, bytes: usize) -> Option<TransientReservation> {
        TransientReservation::new(
            self.budget.max_bytes,
            self.allocated_bytes.get(),
            0,
            &self.transient_bytes,
            bytes,
        )
    }

    pub(crate) fn workspace_bytes(&self) -> usize {
        self.transient_bytes.get()
    }

    pub(crate) fn reserve_workspace(&mut self, bytes: usize) -> Option<WorkspaceReservation> {
        Some(WorkspaceReservation {
            reservation: self.try_reserve_transient(bytes)?,
            allocated: self.allocated_bytes.clone(),
            limit: self.backing.max_bytes.clone(),
        })
    }

    fn add_accounted_bytes(&mut self, bytes: usize) {
        self.allocated_bytes.set(
            self.allocated_bytes
                .get()
                .checked_add(bytes)
                .expect("arena byte ledger overflow"),
        );
    }
    fn sub_accounted_bytes(&mut self, bytes: usize) {
        self.allocated_bytes.set(
            self.allocated_bytes
                .get()
                .checked_sub(bytes)
                .expect("arena byte ledger balance"),
        );
    }
    #[cfg(test)]
    pub(crate) fn tracks_altrep_test_buffer(&self, pointer: *mut u8) -> bool {
        self.backing.node_pages.borrow().iter().any(|page| {
            page.payloads
                .borrow()
                .values()
                .any(|lease| lease.native_projection() == pointer)
        })
    }
    #[cfg(test)]
    pub(crate) fn tracks_payload_link(&self, link: PayloadLink) -> bool {
        !link.is_empty()
            && self.backing.node_pages.borrow().iter().any(|page| {
                page.payloads
                    .borrow()
                    .values()
                    .any(|lease| lease.link() == link)
            })
    }
    /// Share a payload between checked live headers in this arena. No callback
    /// runs during the lend. Type, shape, ownership and publication are checked
    /// together; neither header can free storage still retained by the other.
    pub(crate) fn share_vector_payload(
        &mut self,
        source: &Sexp<'_>,
        target: &Sexp<'_>,
    ) -> super::super::object::SexpResult<()> {
        use super::super::object::SexpError;
        let error = || SexpError::Altrep {
            reason: "invalid shared vector payload",
        };
        let source_node = source.allocation()?;
        let target_node = target.allocation()?;
        if !source_node.belongs_to(&self.heap_identity)
            || !target_node.belongs_to(&self.heap_identity)
        {
            return Err(error());
        }
        let src = self
            .backing
            .node_snapshot(source_node.id())
            .ok_or_else(error)?;
        let dst = self
            .backing
            .node_snapshot(target_node.id())
            .ok_or_else(error)?;
        let same_length = match (src.data, dst.data) {
            (NodeBody::Vector(source), NodeBody::Vector(target)) => source.length == target.length,
            _ => false,
        };
        if src.sxpinfo.type_of() != dst.sxpinfo.type_of() || !same_length || !dst.payload.is_empty()
        {
            return Err(error());
        }
        if empty_payload_shape(&src) && src.payload.is_empty() {
            return Ok(());
        }
        let lease = self
            .backing
            .node_payload(source_node.id())
            .ok_or_else(error)?;
        self.backing
            .publish_payload(target_node.id(), dst.payload, &lease)
            .ok_or_else(error)?;

        Ok(())
    }

    fn can_activate_node(&self) -> bool {
        if self.budget.max_nodes != 0 && self.node_count() >= self.budget.max_nodes {
            return false;
        }
        self.guard_max_nodes == 0 || self.guard_nodes_admitted < self.guard_max_nodes
    }

    /// Arm an optional cumulative allocation-work guard. Charging starts
    /// from current live nodes; zero disables it independently of live caps.
    pub fn set_node_guard(&mut self, max_nodes: usize) {
        self.guard_max_nodes = max_nodes;
        self.guard_nodes_admitted = self.node_count();
    }

    pub(super) fn can_grow_bytes_by(&self, bytes: usize) -> bool {
        self.budget.max_bytes == 0
            || self
                .allocated_bytes
                .get()
                .checked_add(self.transient_bytes.get())
                .and_then(|total| total.checked_add(bytes))
                .is_some_and(|total| total <= self.budget.max_bytes)
    }

    fn can_allocate_node_with_payload(&self, bytes: usize) -> bool {
        self.can_activate_node()
            && bytes
                .checked_add(self.minimum_fresh_backing_bytes())
                .is_some_and(|total| self.can_grow_bytes_by(total))
    }

    /// Allocate a scalar SexprecCore node using slab pages.
    ///
    /// Returns a raw SEXP pointer to the allocated node.
    /// The pointer is valid for the lifetime of the arena.
    /// Uses page from slab (Vec with exact reserve) to avoid per-node Box overhead
    /// and improve locality (hard problem from review: one alloc per node was bad).
    #[inline(always)]
    pub(crate) fn alloc_node(&mut self, sexptype: SEXPTYPE) -> SEXP {
        if !sexptype.is_header_kind() {
            return ptr::null_mut();
        }
        self.alloc_gc_torture_ticks = self.alloc_gc_torture_ticks.wrapping_add(1);
        if !self.can_activate_node() {
            return ptr::null_mut();
        }

        if self.growth_warrants_gc() {
            self.alloc_gc_collect_requested = true;
        }

        if !self.can_allocate_node_with_payload(0) {
            return ptr::null_mut();
        }

        self.allocate_core_in_slab(|| SexprecCore::new(sexptype))
    }

    /// Allocate a scalar node and return an arena-scoped safe wrapper.
    pub fn alloc_node_sexp(&mut self, sexptype: SEXPTYPE) -> Option<Sexp<'_>> {
        let ptr = self.alloc_node(sexptype);
        self.sexp(ptr)
    }

    /// Allocate a vector SexprecCore node with associated data buffer.
    ///
    /// For INTSXP with length n: allocates n * 4 bytes.
    /// For REALSXP with length n: allocates n * 8 bytes.
    /// For STRSXP/VECSXP with length n: allocates n * sizeof(SEXP) bytes.
    ///
    /// Returns null if allocation fails (OOM safety).
    #[inline(always)]
    pub(crate) fn alloc_vector(&mut self, sexptype: SEXPTYPE, length: R_xlen_t) -> SEXP {
        let Ok(layout) = vector_storage_layout(sexptype, length) else {
            return ptr::null_mut();
        };

        self.alloc_gc_torture_ticks = self.alloc_gc_torture_ticks.wrapping_add(1);

        if self.growth_warrants_gc() {
            self.alloc_gc_collect_requested = true;
        }

        let total_bytes = layout.size();

        if !self.can_allocate_node_with_payload(total_bytes) {
            return ptr::null_mut();
        }

        let data = if total_bytes > 0 {
            let Ok(data) = zeroed_vector_payload(sexptype, length) else {
                return ptr::null_mut();
            };
            let Ok(lease) = PayloadLease::from_owned(data) else {
                return ptr::null_mut();
            };
            Some(lease)
        } else {
            None
        };
        let mut header = SexprecCore::new_vector(sexptype, length);
        if let Some(lease) = &data {
            header.payload = lease.link();
        }
        self.allocate_core_with_payload(header, data)
    }

    /// Allocate a vector node and return an arena-scoped safe wrapper.
    pub fn alloc_vector_sexp(&mut self, sexptype: SEXPTYPE, length: R_xlen_t) -> Option<Sexp<'_>> {
        let ptr = self.alloc_vector(sexptype, length);
        self.sexp(ptr)
    }

    /// Allocate a vector SexprecCore node with associated data buffer,
    /// returning `Result` instead of a raw pointer.
    ///
    /// Checks the arena budget before allocating and returns a descriptive
    /// error if the budget would be exceeded.
    pub(crate) fn alloc_vector_checked(
        &mut self,
        sexptype: SEXPTYPE,
        length: R_xlen_t,
    ) -> Result<SEXP, ArenaError> {
        let layout = vector_storage_layout(sexptype, length)?;

        self.alloc_gc_torture_ticks = self.alloc_gc_torture_ticks.wrapping_add(1);

        // Check node budget
        if self.budget.max_nodes > 0 {
            let active = self.node_count();
            if active >= self.budget.max_nodes {
                return Err(ArenaError::NodeBudgetExceeded {
                    limit: self.budget.max_nodes,
                    requested: active + 1,
                });
            }
        }

        let data_bytes = layout.size();
        let total_increase = data_bytes
            .checked_add(self.minimum_fresh_backing_bytes())
            .ok_or(ArenaError::InvalidLength)?;

        // Check byte budget
        if self.budget.max_bytes > 0 {
            let new_total = self
                .allocated_bytes
                .get()
                .checked_add(self.transient_bytes.get())
                .and_then(|total| total.checked_add(total_increase))
                .ok_or(ArenaError::ByteBudgetExceeded {
                    limit: self.budget.max_bytes,
                    requested: usize::MAX,
                })?;
            if new_total > self.budget.max_bytes {
                return Err(ArenaError::ByteBudgetExceeded {
                    limit: self.budget.max_bytes,
                    requested: new_total,
                });
            }
        }

        // Run GC if approaching thresholds (same as alloc_vector)
        if self.growth_warrants_gc() {
            self.alloc_gc_collect_requested = true;
        }

        let data = if data_bytes > 0 {
            Some(
                PayloadLease::from_owned(zeroed_vector_payload(sexptype, length)?)
                    .map_err(|_| ArenaError::OutOfMemory)?,
            )
        } else {
            None
        };

        let mut header = SexprecCore::new_vector(sexptype, length);
        if let Some(lease) = &data {
            header.payload = lease.link();
        }
        let node_ptr = self.allocate_core_with_payload(header, data);
        if node_ptr.is_null() {
            return Err(ArenaError::OutOfMemory);
        }

        Ok(node_ptr)
    }

    /// Allocate a vector node and return an arena-scoped safe wrapper with a
    /// descriptive allocation error.
    pub fn alloc_vector_checked_sexp(
        &mut self,
        sexptype: SEXPTYPE,
        length: R_xlen_t,
    ) -> Result<Sexp<'_>, ArenaError> {
        let ptr = self.alloc_vector_checked(sexptype, length)?;
        self.sexp(ptr).ok_or(ArenaError::OutOfMemory)
    }

    /// Allocate a CHARSXP with inline string data.
    ///
    /// Returns null if allocation fails (OOM safety).
    pub(crate) fn alloc_charsxp(&mut self, s: &[u8]) -> SEXP {
        if let Some(value) = self.cached_ascii_character(s) {
            return value;
        }
        self.alloc_gc_torture_ticks = self.alloc_gc_torture_ticks.wrapping_add(1);
        let len = s.len() as R_xlen_t;
        let total_bytes = match (len as usize).checked_add(1) {
            Some(n) => n,
            None => return ptr::null_mut(),
        };
        let Ok(total_bytes) = OwnedPayload::requested_byte_storage_bytes(total_bytes) else {
            return ptr::null_mut();
        };
        if !self.can_allocate_node_with_payload(total_bytes) {
            return ptr::null_mut();
        }

        #[cfg(test)]
        note_buffer_allocation_attempt();
        let Ok(data) = OwnedPayload::characters(s) else {
            return ptr::null_mut();
        };
        let Ok(data) = PayloadLease::from_owned(data) else {
            return ptr::null_mut();
        };

        // Character shape uses the same initialized vector record as the
        // character accessor; both length fields are ordinary Rust values.
        let header = {
            let mut c = SexprecCore::new(SEXPTYPE::CHARSXP);
            c.data = NodeBody::Vector(super::super::ffi::Vecsxp {
                length: len,
                truelength: 0,
                metadata: crate::sexp::ffi::VectorMetadata::None,
            });
            // GNU CHARSXP gp bits: ASCII (1<<6) and/or UTF8 (1<<3).
            // Serialization writes these via PackFlags(LEVELS(s)).
            let mut gp = 0u16;
            if s.is_ascii() {
                gp |= 1 << 6;
            } else if std::str::from_utf8(s).is_ok() {
                gp |= 1 << 3;
            }
            c.sxpinfo.set_gp(gp);
            c.payload = data.link();
            c
        };
        let value = self.allocate_core_with_payload(header, Some(data));
        self.cache_ascii_character(s, value);
        value
    }

    /// Allocate a CHARSXP and return an arena-scoped safe wrapper.
    pub fn alloc_charsxp_sexp(&mut self, s: &[u8]) -> Option<Sexp<'_>> {
        let ptr = self.alloc_charsxp(s);
        self.sexp(ptr)
    }

    /// Allocate a cons cell (LISTSXP).
    ///
    /// # Safety
    /// Each non-null child is initialized and remains live in this owner
    /// (or immutable storage) for the lifetime of the resulting graph.
    pub(crate) unsafe fn cons(&mut self, car: SEXP, cdr: SEXP, tag: SEXP) -> SEXP {
        let Some(car) = self.link_from_projection(car) else {
            return ptr::null_mut();
        };
        let Some(cdr) = self.link_from_projection(cdr) else {
            return ptr::null_mut();
        };
        let Some(tag) = self.link_from_projection(tag) else {
            return ptr::null_mut();
        };
        let mut header = SexprecCore::new(SEXPTYPE::LISTSXP);
        header.data.list_mut().carval = car;
        header.data.list_mut().cdrval = cdr;
        header.data.list_mut().tagval = tag;
        let pointer = self.alloc_node(SEXPTYPE::LISTSXP);
        if pointer.is_null() {
            return pointer;
        }
        let parent = self
            .node_token(pointer)
            .expect("fresh canonical cons allocation");
        self.heap_identity
            .replace_node(&parent, header)
            .expect("fresh cons header remains live");
        pointer
    }

    /// Check a graph child before storing it in this arena.
    pub(crate) fn accepts_child(&self, value: &Sexp<'_>) -> bool {
        let ptr = value.clone().as_raw();
        self.contains(ptr) || super::super::session::is_immutable_singleton(ptr)
    }

    /// Allocate a cons cell from safe wrappers and return an arena-scoped
    /// wrapper for the new cell.
    pub fn cons_sexp<'a>(
        &'a mut self,
        car: Sexp<'_>,
        cdr: Sexp<'_>,
        tag: Option<Sexp<'_>>,
    ) -> Option<Sexp<'a>> {
        if !self.accepts_child(&car)
            || !self.accepts_child(&cdr)
            || tag.as_ref().is_some_and(|tag| !self.accepts_child(tag))
        {
            return None;
        }
        // SAFETY: all children belong to this arena or immutable storage.
        // Keep the source handles live until the parent is fully initialized.
        let ptr = unsafe {
            self.cons(
                car.clone().as_raw(),
                cdr.clone().as_raw(),
                tag.as_ref()
                    .map_or(ptr::null_mut(), |tag| tag.clone().as_raw()),
            )
        };
        self.sexp(ptr)
    }

    /// Allocate a nil-terminated pairlist chain of n elements.
    ///
    /// Matches GNU `allocList`: `n <= 0` yields `R_NilValue`, and the final
    /// CDR is `R_NilValue` (not a null pointer). `serialize` already maps a
    /// null CDR to `NILVALUE_SXP`, but `identical()` and most list walks treat
    /// null and Nil as distinct without this terminator.
    pub(crate) fn alloc_list_chain(&mut self, n: i32) -> SEXP {
        if n <= 0 {
            return unsafe { crate::sexp::globals::R_NilValue() };
        }
        let mut result: SEXP = unsafe { crate::sexp::globals::R_NilValue() };
        for _ in 0..n {
            // SAFETY: each predecessor was allocated in this arena; no GC runs during this lend.
            result = unsafe { self.cons(ptr::null_mut(), result, ptr::null_mut()) };
            if result.is_null() {
                return ptr::null_mut();
            }
        }
        result
    }

    /// Add an existing node (for legacy compat in some paths). Pushes into current slab page
    /// (assumes caller ensures no overflow; for hard perf problem we prefer alloc_node).
    /// # Safety
    /// Payload pointers and graph children are valid and owned by this arena
    /// or immutable storage; transferring the core must not duplicate payload ownership.
    pub(crate) unsafe fn add_node(&mut self, node: Box<SexprecCore>) -> SEXP {
        if !self.can_allocate_node_with_payload(0) {
            return ptr::null_mut();
        }

        self.allocate_core_in_slab(|| *node)
    }

    /// Get the number of nodes allocated in this arena.
    pub fn node_count(&self) -> usize {
        self.live_count
    }

    /// Amortize tracing against the graph retained by the previous collection.
    /// The byte trigger and admission budgets remain independent of node growth.
    pub(crate) fn growth_warrants_gc(&self) -> bool {
        let node_growth = self.node_count().saturating_sub(self.nodes_at_last_gc);
        let mut node_threshold = GC_TRIGGER_THRESHOLD.max(self.nodes_at_last_gc / 4);
        let mut byte_threshold = GC_BYTE_THRESHOLD;
        // Use stable headroom from the completed collection. Testing current
        // pressure on every allocation would repeatedly trace an unreclaimable
        // live graph without allowing useful work between collections.
        if self.budget.max_nodes != 0 {
            node_threshold = node_threshold
                .min((self.budget.max_nodes.saturating_sub(self.nodes_at_last_gc) / 2).max(1));
        }
        if self.budget.max_bytes != 0 {
            byte_threshold = byte_threshold
                .min((self.budget.max_bytes.saturating_sub(self.bytes_at_last_gc) / 2).max(1));
        }
        node_growth > node_threshold
            || self
                .allocated_bytes
                .get()
                .saturating_sub(self.bytes_at_last_gc)
                > byte_threshold
    }

    /// At an already due safe point, reclaim old garbage before a bounded
    /// workload runs out of admission space. Zero means an unlimited budget.
    pub(crate) fn budget_pressure_warrants_full_gc(&self) -> bool {
        let near_limit = |used: usize, limit: usize| limit != 0 && used >= limit - limit / 10;
        near_limit(self.node_count(), self.budget.max_nodes)
            || near_limit(
                self.allocated_bytes
                    .get()
                    .saturating_add(self.transient_bytes.get()),
                self.budget.max_bytes,
            )
    }

    pub(crate) fn note_gc_completed(&mut self) {
        self.nodes_at_last_gc = self.node_count();
        self.bytes_at_last_gc = self.allocated_bytes.get();
    }

    /// Return true if this pointer is one of the arena's active nodes.
    pub(crate) fn contains(&self, ptr: SEXP) -> bool {
        if ptr.is_null() {
            return false;
        }
        self.node_token(ptr).is_some()
    }

    pub(crate) fn link_from_projection(&self, pointer: SEXP) -> Option<NodeLink> {
        self.heap_identity.link_from_projection(pointer)
    }
    pub(crate) fn heap_identity(&self) -> HeapIdentity {
        self.heap_identity.clone()
    }

    /// Constructor bridge: resolve each current projection once while the
    /// allocator is lent, then mutate only the canonical typed cells.
    pub(crate) fn set_reference_element(
        &self,
        parent: SEXP,
        index: usize,
        child: SEXP,
    ) -> Option<()> {
        use super::super::heap::ReferenceChild;
        let parent = self.node_token(parent)?;
        if child.is_null() {
            return self
                .heap_identity
                .set_reference_elt(&parent, index, ReferenceChild::Null);
        }
        if let Some(child) = checked_node(child) {
            return self.heap_identity.set_reference_elt(
                &parent,
                index,
                ReferenceChild::Node(&child),
            );
        }
        if let Some(child) = self.heap_identity.retained_singleton(child) {
            return self.heap_identity.set_reference_elt(
                &parent,
                index,
                ReferenceChild::Singleton(&child),
            );
        }
        let pool = super::super::globals::immutable_singleton_pool();
        let child = pool.lease(child)?;
        self.heap_identity
            .set_reference_elt(&parent, index, ReferenceChild::Singleton(&child))
    }

    pub(crate) fn node_token(&self, pointer: SEXP) -> Option<CheckedNode> {
        let (page, slot) = self.local_slot(pointer)?;
        self.backing.node_pages.borrow()[page].storage.token(slot)
    }

    pub(crate) fn node_projection(&self, pointer: SEXP) -> Option<(SEXP, CheckedNode)> {
        let node = self.node_token(pointer)?;
        let canonical = self.backing.node_pages.borrow()[node.id().page()]
            .storage
            .resolve(node.id())?;
        Some((canonical, node))
    }

    /// Identity of the currently live allocation at a legacy projection.
    /// Capturing an address after reuse identifies its new allocation; callers
    /// retain the returned ID to reject stale handles on later access.
    pub(crate) fn node_id(&self, pointer: SEXP) -> Option<NodeId> {
        self.node_token(pointer).map(|token| token.id().clone())
    }

    /// Resolve an unforgeable identity only while its exact allocation lives.
    pub(crate) fn resolve_node(&self, id: &NodeId) -> Option<SEXP> {
        self.backing
            .node_pages
            .borrow()
            .get(id.page())?
            .storage
            .resolve(id)
    }

    /// Wrap an active arena-owned pointer in a safe `Sexp`.
    ///
    /// Unlike raw construction, this checks that the pointer belongs to this
    /// arena and is not currently on the free list, tying the wrapper lifetime
    /// to the arena borrow.
    pub(crate) fn sexp(&self, ptr: SEXP) -> Option<Sexp<'_>> {
        Sexp::from_arena_raw(ptr, self).ok()
    }

    /// Iterate over initialized arena headers, including reusable slots.
    /// Only a local storage loan is used to capture these projections.
    pub(crate) fn nodes(&self) -> impl Iterator<Item = SEXP> + '_ {
        let pointers: Vec<SEXP> = self
            .backing
            .node_pages
            .borrow()
            .iter()
            .enumerate()
            .flat_map(|(page_idx, page)| {
                let used = if page_idx == self.slab_page {
                    self.slab_offset
                } else {
                    page.meta.slots()
                };
                (0..used).map(move |i| page.storage.raw_slot(i).expect("allocated page slot"))
            })
            .collect();
        pointers.into_iter()
    }

    /// Live nodes in slab order, using exact original generations.
    pub(crate) fn active_nodes(&self) -> SlotIter {
        self.slot_iter(false)
    }

    /// Old-generation nodes in slab order, using exact original generations.
    pub(crate) fn old_nodes(&self) -> SlotIter {
        self.slot_iter(true)
    }

    fn slot_iter(&self, old_only: bool) -> SlotIter {
        let pages = self
            .backing
            .node_pages
            .borrow()
            .iter()
            .map(|page| {
                let (eligible, generations) = page.meta.snapshot_eligible(old_only);
                PageSnapshot {
                    projection: page.storage.projection(),
                    eligible,
                    generations,
                }
            })
            .collect();
        SlotIter {
            pages,
            _backing_owners: self._backing_owners.clone(),
            page: 0,
            word: 0,
            bits: 0,
        }
    }

    /// Free a node by adding it to the free list for reuse.
    #[inline(always)]
    /// # Safety
    /// No reachable graph edge or Rust payload borrow may refer to this node.
    /// Unrooted checked metadata handles become stale and reject later access.
    pub(crate) unsafe fn free_node(&mut self, ptr: SEXP) {
        if ptr.is_null() {
            return;
        }
        let Some(node) = self.node_token(ptr) else {
            return;
        };
        if self.backing.node_snapshot(node.id()).is_none() {
            return;
        }
        let Some(pointer) = self.backing.node_projection(node.id()) else {
            return;
        };
        if let Some(resource) = self.backing.take_resource(node.id()) {
            // Detach before retiring the generation; release after this arena
            // lend ends, so a resource destructor may safely reenter.
            self.retired_resources.borrow_mut().push(resource);
        }
        // Retirement detaches the actual slot owner. A snapshot lease may
        // still pin its bytes and accounting after this generation dies.
        self.backing.node_pages.borrow()[node.id().page()]
            .payloads
            .borrow_mut()
            .remove(&node.id().slot());
        let mut header = SexprecCore::new(SEXPTYPE::NILSXP);
        header.attrib = NodeLink::NULL;
        header.sxpinfo.set_mark(false);
        header
            .sxpinfo
            .set_gcgen(crate::sexp::gengc::Generation::Old as u8);
        self.backing.node_pages.borrow()[node.id().page()]
            .storage
            .replace_live(node.id(), header)
            .expect("retiring exact live node");
        let ptr = pointer;

        let page = node.id().page();
        let slot = node.id().slot();
        let pages = self.backing.node_pages.borrow();
        let offset = SlotOffset::from_index(
            pages[page]
                .first_slot
                .checked_add(slot)
                .expect("admitted slot offset"),
        )
        .expect("admitted slot offset");
        pages[page]
            .payloads
            .borrow_mut()
            .set_next_free(slot, self.reclaimed_head)
            .expect("admitted reclamation slot");
        drop(pages);
        self.reclaimed_head = Some(offset);
        self.reclaimed_count = self
            .reclaimed_count
            .checked_add(1)
            .expect("reclamation slot count");
        self.track_node_freed(ptr);
    }

    /// Get the fragmentation ratio among allocated node slots.
    pub fn fragmentation_ratio(&self) -> f64 {
        let total = self.live_count + self.reclaimed_count;
        if total == 0 {
            0.0
        } else {
            self.reclaimed_count as f64 / total as f64
        }
    }

    /// Rebuild a unique logical worklist from canonical initialized slots.
    /// Spare capacity was never allocated and must not become a free node.
    pub(crate) fn normalize_free_list(&mut self) {
        let pages = self.backing.node_pages.borrow();
        let mut head = None;
        let mut count = 0usize;
        for (page_index, page) in pages.iter().enumerate() {
            let used = if page_index == self.slab_page {
                self.slab_offset
            } else {
                page.meta.slots()
            };
            let mut records = page.payloads.borrow_mut();
            for slot in 0..used {
                if page.meta.reusable(slot) {
                    records
                        .set_next_free(slot, head)
                        .expect("admitted reclamation slot");
                    head = Some(
                        SlotOffset::from_index(
                            page.first_slot
                                .checked_add(slot)
                                .expect("admitted slot offset"),
                        )
                        .expect("admitted slot offset"),
                    );
                    count = count.checked_add(1).expect("reclamation slot count");
                } else {
                    records
                        .set_next_free(slot, None)
                        .expect("admitted reclamation slot");
                }
            }
        }
        self.reclaimed_head = head;
        self.reclaimed_count = count;
    }

    /// Get the number of free slots available for reuse.
    pub fn free_count(&self) -> usize {
        self.reclaimed_count
    }

    /// Get total bytes allocated by this arena.
    pub fn total_bytes_allocated(&self) -> usize {
        self.allocated_bytes.get()
    }

    /// Verify arena invariants (debug only).
    fn verify_invariants(&self) {
        debug_assert!({
            for page in self.backing.node_pages.borrow().iter() {
                for (slot, lease) in page.payloads.borrow().iter() {
                    debug_assert!(page.storage.token(slot).is_some());
                    debug_assert!(lease.belongs_to(&self.heap_identity));
                }
            }
            let mut cursor = self.reclaimed_head;
            for _ in 0..self.reclaimed_count {
                let head = cursor.expect("exact reclamation count");
                let (page, slot) = self
                    .reclaimed_location(head)
                    .expect("original reclamation slot");
                cursor = self.backing.node_pages.borrow()[page]
                    .payloads
                    .borrow()
                    .next_free(slot)
                    .expect("admitted reclamation slot");
            }
            debug_assert!(cursor.is_none());
            true
        });
    }
}

/// Resizable host workspace admission sharing the arena's actual byte ledger.
/// Resizing never allocates, enters R, or borrows a runtime or payload.
pub(crate) struct WorkspaceReservation {
    reservation: TransientReservation,
    allocated: Rc<Cell<usize>>,
    limit: Rc<Cell<usize>>,
}

impl WorkspaceReservation {
    pub(super) fn from_counters(
        allocated: Rc<Cell<usize>>,
        transient: Rc<Cell<usize>>,
        limit: Rc<Cell<usize>>,
        bytes: usize,
    ) -> Option<Self> {
        Some(Self {
            reservation: TransientReservation::new(
                limit.get(),
                allocated.get(),
                0,
                &transient,
                bytes,
            )?,
            allocated,
            limit,
        })
    }
    pub(crate) fn bind_payload(&mut self, heap: &HeapIdentity, lease: &PayloadLease) -> Option<()> {
        if lease.belongs_to(heap) {
            return lease.bind_to_heap(heap, &self.allocated);
        }
        let remaining = self.bytes().checked_sub(lease.storage_bytes())?;
        lease.bind_to_heap(heap, &self.allocated)?;
        self.try_resize(remaining)
    }
    pub(crate) fn commit_allocation(self) -> Option<super::AllocationCharge> {
        let bytes = self.bytes();
        self.allocated.set(self.allocated.get().checked_add(bytes)?);
        Some(super::AllocationCharge {
            allocated: self.allocated.clone(),
            bytes,
        })
    }
    pub(crate) fn bytes(&self) -> usize {
        self.reservation.bytes
    }

    pub(crate) fn try_resize(&mut self, bytes: usize) -> Option<()> {
        let others = self
            .reservation
            .counter
            .get()
            .checked_sub(self.reservation.bytes)?;
        let reserved = others.checked_add(bytes)?;
        let total = self.allocated.get().checked_add(reserved)?;
        if bytes > self.reservation.bytes && self.limit.get() != 0 && total > self.limit.get() {
            return None;
        }
        self.reservation.counter.set(reserved);
        self.reservation.bytes = bytes;
        Some(())
    }
}

/// RAII release token for a native workspace reservation.
pub(crate) struct TransientReservation {
    counter: Rc<Cell<usize>>,
    bytes: usize,
    payload_target: Option<(HeapIdentity, Rc<Cell<usize>>)>,
}

impl TransientReservation {
    pub(crate) const fn bytes(&self) -> usize {
        self.bytes
    }
    pub(crate) fn new(
        max_bytes: usize,
        accounted: usize,
        pending: usize,
        counter: &Rc<Cell<usize>>,
        bytes: usize,
    ) -> Option<Self> {
        let reserved = counter.get().checked_add(bytes)?;
        let total = accounted.checked_add(pending)?.checked_add(reserved)?;
        if max_bytes != 0 && total > max_bytes {
            return None;
        }
        counter.set(reserved);
        Some(Self {
            counter: Rc::clone(counter),
            bytes,
            payload_target: None,
        })
    }
    pub(crate) fn for_payload(mut self, heap: HeapIdentity, total: Rc<Cell<usize>>) -> Self {
        self.payload_target = Some((heap, total));
        self
    }
    /// Convert admission into the allocation lifetime charge before exposing
    /// its cloneable lease to an initializer. Escaping clones stay accounted.
    fn bind_payload(self, lease: &PayloadLease) -> Option<()> {
        if lease.storage_bytes() != self.bytes {
            return None;
        }
        let (heap, total) = self.payload_target.as_ref()?;
        lease.bind_to_heap(heap, total)
    }
}

impl Drop for TransientReservation {
    fn drop(&mut self) {
        // The shared counter can outlive or move independently of the arena.
        // Rc also confines this token to its creating thread.
        self.counter.set(
            self.counter
                .get()
                .checked_sub(self.bytes)
                .expect("reservation byte ledger balance"),
        );
    }
}

impl Default for RArena {
    fn default() -> Self {
        Self::new()
    }
}

// Registered buffers and slab pages own their allocations through RAII.
// RArena needs no custom deallocator: field destruction releases each once.

// ---------------------------------------------------------------------------
// Instance evaluation arena
// ---------------------------------------------------------------------------

/// Access the active instance evaluation arena.
///
/// Allocation is intentionally scoped to an `RInstance`: unscoped arena
/// fallback would let objects escape the session that owns evaluator state,
/// which breaks Android multi-instance isolation.
///
/// The arena view is derived through `with_current_instance`, which guards
/// ambient mutable instance access with the borrow-depth monitor instead of
/// fabricating an independent `&mut RArena` from the raw current-instance
/// pointer (which could alias a live outer `&mut RInstance`).
///
/// # Safety
/// The active owner is live and exclusively available for the arena lend.
/// The callback must not reenter R or mutate/read the arena through an alias.
pub unsafe fn with_arena<F, R>(f: F) -> R
where
    F: FnOnce(&mut RArena) -> R,
{
    super::super::instance::with_required_current_instance(|inst| unsafe { with_arena_in(inst, f) })
}

thread_local! {
    static LENT_ARENAS: std::cell::RefCell<std::collections::HashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

pub(super) struct LendLedger {
    instance: usize,
    arena: usize,
    max_bytes: usize,
    total: Rc<Cell<usize>>,
    transient: Rc<Cell<usize>>,
    fresh: Vec<(SEXP, CheckedNode)>,
}

thread_local! {
    /// One entry per `with_arena_in`, including a nested call that is about
    /// to be rejected. Popping restores the outer lend's snapshot.
    static LEND_LEDGER: RefCell<Vec<LendLedger>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn install_lend_ledger(inst: *mut super::super::instance::RInstance) {
    let ledger = unsafe {
        let arena = &(*inst).arena;
        LendLedger {
            instance: inst as usize,
            arena: std::ptr::from_ref(arena) as usize,
            max_bytes: arena.budget.max_bytes,
            total: arena.allocated_bytes.clone(),
            transient: Rc::clone(&arena.transient_bytes),
            fresh: Vec::new(),
        }
    };
    LEND_LEDGER.with(|slot| slot.borrow_mut().push(ledger));
}

/// Keep the exact allocation alive until its lend finishes dispatching
/// deferred collection and callbacks. The arena supplies its own identity;
/// no instance field is accessed while the mutable arena borrow is live.
pub(super) fn note_lend_allocation(arena: usize, projection: SEXP, allocation: CheckedNode) {
    LEND_LEDGER.with(|slot| {
        if let Some(ledger) = slot
            .borrow_mut()
            .iter_mut()
            .rev()
            .find(|ledger| ledger.arena == arena)
        {
            ledger.fresh.push((projection, allocation));
        }
    });
}

/// Snapshot temporary roots for the collector's heap domain. The original
/// tokens reject freed/reused slots; lookup never refreshes them by address.
/// The owned snapshot releases its TLS borrow before tracing or callbacks.
pub(crate) fn fresh_allocation_roots(
    identity: &HeapIdentity,
) -> super::root_snapshots::CheckedRootSnapshot {
    let count = fresh_allocation_roots_for_marking(identity).count();
    crate::sexp::protect::RootSnapshot::try_collect(
        &identity.byte_ledger(),
        count,
        fresh_allocation_roots_for_marking(identity),
    )
    .unwrap_or_else(|error| super::root_snapshots::snapshot_error(error))
}

/// Each copied root ends its TLS loan before the collector traces it.
pub(crate) fn fresh_allocation_roots_for_marking(
    identity: &HeapIdentity,
) -> impl Iterator<Item = (SEXP, CheckedNode)> + use<> {
    FreshRoots {
        identity: identity.clone(),
        ledger: 0,
        root: 0,
    }
}

struct FreshRoots {
    identity: HeapIdentity,
    ledger: usize,
    root: usize,
}

impl Iterator for FreshRoots {
    type Item = (SEXP, CheckedNode);
    fn next(&mut self) -> Option<Self::Item> {
        LEND_LEDGER
            .try_with(|ledgers| {
                let ledgers = ledgers.borrow();
                while let Some(ledger) = ledgers.get(self.ledger) {
                    while let Some((pointer, allocation)) = ledger.fresh.get(self.root) {
                        self.root += 1;
                        if allocation.belongs_to(&self.identity) && allocation.is_live() {
                            return Some((*pointer, allocation.clone()));
                        }
                    }
                    self.ledger += 1;
                    self.root = 0;
                }
                None
            })
            .ok()
            .flatten()
    }
}

/// Reserve transient bytes without borrowing an already-lent arena again.
///
/// # Safety
/// `inst` is a live owner. Any outstanding arena borrow is registered by
/// `with_arena_in`; no other instance field borrow overlaps this operation.
pub(crate) unsafe fn reserve_transient_in(
    inst: *mut super::super::instance::RInstance,
    bytes: usize,
) -> Option<TransientReservation> {
    if is_arena_lent(inst) {
        LEND_LEDGER.with(|slot| {
            let ledgers = slot.borrow();
            let ledger = ledgers
                .iter()
                .rev()
                .find(|ledger| ledger.instance == inst as usize)?;
            TransientReservation::new(
                ledger.max_bytes,
                ledger.total.get(),
                0,
                &ledger.transient,
                bytes,
            )
        })
    } else {
        // Strictly local accounting: no evaluation, allocation hooks or GC.
        unsafe { (*inst).arena.try_reserve_transient(bytes) }
    }
}

pub(super) fn note_lend_budget(arena: usize, max_bytes: usize) {
    LEND_LEDGER.with(|slot| {
        if let Some(ledger) = slot
            .borrow_mut()
            .iter_mut()
            .rev()
            .find(|ledger| ledger.arena == arena)
        {
            ledger.max_bytes = max_bytes;
        }
    });
}

pub(super) struct ClearLendLedger;

impl Drop for ClearLendLedger {
    fn drop(&mut self) {
        let _ = LEND_LEDGER.try_with(|slot| {
            slot.borrow_mut().pop();
        });
    }
}

/// Zero-fill `bytes` and attach the pointer as `node`'s vector payload.
///
/// Admission and publication use the original retained canonical store, even
/// during a lend. A rejected preparation returns null without changing the header.
///
/// # Safety
/// `node` is a live vector header owned by the active instance, with no prior
/// payload. `bytes > 0` is its complete payload size; the caller roots the node.
pub(crate) unsafe fn attach_zeroed_data_buffer(node: SEXP, bytes: usize) -> *mut u8 {
    unsafe { attach_initialized_data_buffer(node, bytes, |_| {}) }
}

/// Construct and initialize a typed payload before publishing it to any
/// header. Initialization failure leaves the lazy header unchanged; any
/// escaped allocation leases continue owning and accounting for their bytes.
///
/// # Safety
/// `node` is a rooted live vector in the active owner, without an existing
/// payload or overlapping payload borrow. `initialize` writes only valid
/// elements of this vector's complete `bytes` region; it does not retain the
/// pointer or reenter R while an arena lend is active.
pub(crate) unsafe fn attach_initialized_data_buffer(
    node: SEXP,
    bytes: usize,
    initialize: impl FnOnce(*mut u8),
) -> *mut u8 {
    let Some((_, token)) = checked_projection(node) else {
        return ptr::null_mut();
    };
    let heap = token.heap_identity();
    let Some(original) = heap.node_snapshot(&token) else {
        return ptr::null_mut();
    };
    let Ok(layout) = vector_layout(original.sxpinfo.type_of(), original.vecsxp_length()) else {
        return ptr::null_mut();
    };
    if bytes == 0 || layout.size() != bytes {
        return ptr::null_mut();
    }
    attach_initialized_payload(&token, |lease| {
        initialize(lease.native_projection());
        Some(())
    })
    .map_or(ptr::null_mut(), |lease| lease.native_projection())
}

/// Prepare the actual typed allocation, release all store loans for the
/// initializer, then publish only into the original unchanged vector shape.
pub(crate) fn attach_initialized_payload(
    node: &CheckedNode,
    initialize: impl FnOnce(&PayloadLease) -> Option<()>,
) -> Option<PayloadLease> {
    let heap = node.heap_identity();
    let original = heap.node_snapshot(node)?;
    if !original.payload.is_empty() || !original.sxpinfo.alt() {
        return None;
    }
    let NodeBody::Vector(vector) = original.data else {
        return None;
    };
    let layout = vector_storage_layout(original.sxpinfo.type_of(), vector.length).ok()?;
    // Reserve via the independent ledger even during an existing arena lend.
    let reservation = heap.reserve_payload_bytes(node, layout.size())?;
    let lease = PayloadLease::from_owned(
        zeroed_vector_payload(original.sxpinfo.type_of(), vector.length).ok()?,
    )
    .ok()?;
    reservation.bind_payload(&lease)?;
    initialize(&lease)?;
    let current = heap.node_snapshot(node)?;
    if current.sxpinfo.type_of() != original.sxpinfo.type_of()
        || current.data != original.data
        || current.payload != original.payload
        || !current.sxpinfo.alt()
    {
        return None;
    }
    heap.publish_payload(node, original.payload, &lease)?;
    Some(lease)
}

pub(crate) unsafe fn vector_payload_is_tracked(node: SEXP) -> bool {
    checked_projection(node)
        .and_then(|(_, token)| token.heap_identity().payload_lease(&token))
        .is_some()
}

/// Detached state releases only after every arena lend has ended.
/// The guard owns the queue independently of the runtime pointer.
pub(crate) struct ResourceDropGuard {
    queue: Option<crate::sexp::heap::RetiredResources>,
    instance: Option<usize>,
}

impl ResourceDropGuard {
    pub(crate) fn standalone(queue: crate::sexp::heap::RetiredResources) -> Self {
        Self {
            queue: Some(queue),
            instance: None,
        }
    }
}

impl Drop for ResourceDropGuard {
    fn drop(&mut self) {
        if self
            .instance
            .is_some_and(|key| LENT_ARENAS.with(|lent| lent.borrow().contains(&key)))
        {
            return;
        }
        let Some(queue) = &self.queue else {
            return;
        };
        loop {
            let detached = std::mem::take(&mut *queue.borrow_mut());
            if detached.is_empty() {
                break;
            }
            drop(detached);
        }
    }
}

/// Capture before a GC/arena operation; Drop must outlive the exclusive lend.
/// # Safety
/// The owner is live and its arena is not mutably lent during capture.
pub(crate) unsafe fn resource_drop_guard_in(
    inst: *mut super::super::instance::RInstance,
) -> ResourceDropGuard {
    let queue = if is_arena_lent(inst) {
        None
    } else {
        // SAFETY: copy this field before any exclusive arena loan is formed.
        Some(unsafe { (&*ptr::addr_of!((*inst).arena.retired_resources)).clone() })
    };
    ResourceDropGuard {
        queue,
        instance: Some(inst.addr()),
    }
}

impl Drop for RArena {
    fn drop(&mut self) {
        let guard = ResourceDropGuard {
            queue: Some(self.retired_resources.clone()),
            instance: None,
        };
        drop(guard);
    }
}

pub(super) struct ArenaLend(usize);

impl ArenaLend {
    fn new(instance: *mut super::super::instance::RInstance) -> Self {
        let key = instance.addr();
        assert!(
            LENT_ARENAS.with(|lent| lent.borrow_mut().insert(key)),
            "reentrant mutable arena access; release the arena before calling the interpreter"
        );
        Self(key)
    }
}

pub(crate) fn is_arena_lent(instance: *mut super::super::instance::RInstance) -> bool {
    LENT_ARENAS.with(|lent| lent.borrow().contains(&instance.addr()))
}

impl Drop for ArenaLend {
    fn drop(&mut self) {
        LENT_ARENAS.with(|lent| {
            lent.borrow_mut().remove(&self.0);
        });
    }
}

/// # Safety
/// `inst` is a live writable owner pointer; no overlapping arena/instance
/// borrow exists. The callback must not reenter R through an alias.
pub(crate) unsafe fn with_arena_in<F, R>(inst: *mut super::super::instance::RInstance, f: F) -> R
where
    F: FnOnce(&mut RArena) -> R,
{
    // Reject through TLS identity alone, before even snapshotting a field:
    // another call may currently hold a protected exclusive arena reference.
    assert!(
        !is_arena_lent(inst),
        "reentrant mutable arena access; release the arena before calling the interpreter"
    );
    // P1: the `&mut RArena` lend below is arena-local by construction —
    // arena methods defer their GC firings (alloc_gc_torture_ticks /
    // alloc_gc_collect_requested) instead of touching instance state, so
    // nothing reenters the interpreter while the lend is live. The
    // deferred firings are processed only after it is released.
    unsafe {
        // Snapshot the budget before the `&mut RArena` lend. Compact-sequence
        // expansion during the callback reserves through this ledger instead
        // of borrowing the arena again.
        let liveness = super::super::instance::instance_liveness(inst);
        install_lend_ledger(inst);
        let _clear_ledger = ClearLendLedger;
        // Declared after the ledger so fresh result roots remain installed
        // through resource-destructor reentry, including unwind.
        let _resource_drops = resource_drop_guard_in(inst);
        let result = {
            // Drop flushes after the lend ends, including when `f` unwinds,
            // and before deferred collection below. The ledger outlives the
            // flush so reserved bytes stay in step with the arena.
            let result = {
                let _lend = ArenaLend::new(inst);
                f(&mut (*inst).arena)
            };
            result
        };
        let torture_ticks = std::mem::take(&mut (*inst).arena.alloc_gc_torture_ticks);
        let collect_requested = std::mem::take(&mut (*inst).arena.alloc_gc_collect_requested);
        crate::eval::parser::flush_literal_warnings();
        if liveness.is_live() {
            crate::sexp::gengc::process_deferred_alloc_gc_in(
                inst,
                torture_ticks,
                collect_requested,
            );
        }
        result
    }
}

/// This is identical to with_arena but named for clarity in GC context.
pub unsafe fn with_arena_for_gc<F, R>(f: F) -> R
where
    F: FnOnce(&mut RArena) -> R,
{
    unsafe { with_arena(f) }
}
