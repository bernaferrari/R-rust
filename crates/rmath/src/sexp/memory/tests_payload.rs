//! Payload/resource/scheduling tests extracted from the former inline `mod tests`.

pub(super) fn native_payload(pointer: super::SEXP) -> *mut u8 {
    let (_, node) = super::checked_projection(pointer).expect("fixture allocation");
    node.heap_identity()
        .payload_lease(&node)
        .expect("fixture attached payload")
        .native_projection()
}

#[test]
pub(super) fn dense_payload_sharing_preserves_lazy_target_bookkeeping() {
    let session = super::super::session::RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let source = factory
            .allocate(|arena| Some(arena.alloc_vector(super::SEXPTYPE::INTSXP, 3)))
            .unwrap();
        let target = factory
            .allocate(|arena| Some(arena.alloc_vector(super::SEXPTYPE::INTSXP, 0)))
            .unwrap();
        let source_node = source.allocation().unwrap();
        let target_node = target.allocation().unwrap();
        let heap = target_node.heap_identity();
        let mut header = heap.node_snapshot(target_node).unwrap();
        header.sxpinfo.set_alt(true);
        header.sxpinfo.set_gp(19);
        heap.replace_node(target_node, header).unwrap();
        heap.publish_lazy_length(target_node, 3).unwrap();
        let lease = heap.payload_lease(source_node).unwrap();
        lease.set_integer_elt(2, 81).unwrap();
        assert_eq!(
            heap.node_snapshot(source_node)
                .unwrap()
                .data
                .vector()
                .truelength,
            3
        );
        owner
            .with_arena(|arena| arena.share_vector_payload(&source, &target))
            .unwrap()
            .unwrap();
        let current = heap.node_snapshot(target_node).unwrap();
        assert_eq!(current.data.vector().truelength, 0);
        assert_eq!(current.data.vector().length, 3);
        assert_eq!(current.sxpinfo.gp(), 19);
        assert!(current.sxpinfo.alt());
        assert!(
            heap.payload_lease(target_node)
                .unwrap()
                .same_allocation(&lease)
        );
        assert_eq!(
            heap.payload_lease(target_node).unwrap().integer_elt(2),
            Some(81)
        );
    });
}

#[test]
pub(super) fn pinned_reference_snapshot_keeps_actual_bytes_charged_until_last_drop() {
    let mut arena = super::RArena::with_budget(super::ArenaBudget::new(
        super::NODE_BYTES + 2 * std::mem::size_of::<super::NodeLink>(),
        0,
    ));
    let heap = arena.heap_identity();
    let pointer = arena.alloc_vector(super::SEXPTYPE::VECSXP, 2);
    let node = arena.node_token(pointer).unwrap();
    let allocation = heap.payload_lease(&node).unwrap();
    let references = allocation.reference_lease().unwrap();
    let charge = arena.allocated_bytes.clone();
    unsafe {
        arena.free_node(pointer);
    }
    assert!(heap.payload_lease(&node).is_none());
    assert!(arena.alloc_vector(super::SEXPTYPE::INTSXP, 1).is_null());
    drop(allocation);
    assert_eq!(
        charge.get(),
        super::NODE_BYTES + 2 * std::mem::size_of::<super::NodeLink>()
    );
    assert_eq!(references.snapshot(2), Some(vec![super::NodeLink::NULL; 2]));
    assert!(arena.alloc_vector(super::SEXPTYPE::INTSXP, 1).is_null());
    drop(references);
    assert_eq!(charge.get(), super::NODE_BYTES);
    assert!(!arena.alloc_vector(super::SEXPTYPE::INTSXP, 1).is_null());
}

#[test]
pub(super) fn checked_payload_publication_rejects_foreign_wrong_storage_and_budget_bypass() {
    let mut arena = super::RArena::with_budget(super::ArenaBudget::new(super::NODE_BYTES + 12, 0));
    let heap = arena.heap_identity();
    let parent = arena.alloc_vector(super::SEXPTYPE::INTSXP, 0);
    let node = arena.node_token(parent).unwrap();
    let mut lazy = heap.node_snapshot(&node).unwrap();
    lazy.sxpinfo.set_alt(true);
    heap.replace_node(&node, lazy).unwrap();
    heap.publish_lazy_length(&node, 3).unwrap();
    let mut foreign = super::RArena::new();
    let other = foreign.alloc_vector(super::SEXPTYPE::INTSXP, 3);
    let other = foreign.node_token(other).unwrap();
    let foreign_lease = other.heap_identity().payload_lease(&other).unwrap();
    let before = foreign.total_bytes_allocated();
    assert!(
        heap.publish_payload(&node, super::PayloadLink::EMPTY, &foreign_lease)
            .is_none()
    );
    assert_eq!(foreign.total_bytes_allocated(), before);
    let wrong = super::PayloadLease::zeroed(super::SEXPTYPE::REALSXP, 3).unwrap();
    assert!(
        heap.publish_payload(&node, super::PayloadLink::EMPTY, &wrong)
            .is_none()
    );
    let oversized = super::PayloadLease::zeroed(super::SEXPTYPE::INTSXP, 4).unwrap();
    assert!(
        heap.publish_payload(&node, super::PayloadLink::EMPTY, &oversized)
            .is_none()
    );
    assert!(!oversized.belongs_to(&heap));
    assert_eq!(arena.total_bytes_allocated(), super::NODE_BYTES);
    assert!(heap.node_snapshot(&node).unwrap().payload.is_empty());
    let valid = super::PayloadLease::zeroed(super::SEXPTYPE::INTSXP, 3).unwrap();
    heap.publish_payload(&node, super::PayloadLink::EMPTY, &valid)
        .unwrap();
    assert_eq!(arena.total_bytes_allocated(), super::NODE_BYTES + 12);
    assert!(heap.payload_lease(&node).unwrap().same_allocation(&valid));
    let mut changed_provider = heap.node_snapshot(&node).unwrap();
    changed_provider.sxpinfo.set_type(super::SEXPTYPE::LGLSXP);
    assert!(heap.replace_node(&node, changed_provider).is_none());
    assert_eq!(
        heap.node_snapshot(&node).unwrap().sxpinfo.type_of(),
        super::SEXPTYPE::INTSXP
    );
}

#[test]
pub(super) fn original_heap_initializer_reentry_rejects_changed_shape_and_accounts_escaped_lease() {
    let mut arena = super::RArena::new();
    let heap = arena.heap_identity();
    let pointer = arena.alloc_vector(super::SEXPTYPE::INTSXP, 0);
    let node = arena.node_token(pointer).unwrap();
    let root = node.root_lease().unwrap();
    let mut header = heap.node_snapshot(&node).unwrap();
    header.sxpinfo.set_alt(true);
    heap.replace_node(&node, header).unwrap();
    heap.publish_lazy_length(&node, 3).unwrap();
    let charge = arena.allocated_bytes.clone();
    let transient = arena.transient_bytes.clone();
    let header_bytes = super::tests_physical_headers::header_backing_bytes(&arena);
    drop(arena);
    let mut escaped = None;
    assert!(
        super::attach_initialized_payload(&node, |lease| {
            escaped = Some(lease.clone());
            lease.set_integer_elt(0, 91)?;
            heap.publish_lazy_length(&node, 2)?;
            Some(())
        })
        .is_none()
    );
    assert_eq!(heap.node_snapshot(&node).unwrap().vecsxp_length(), 2);
    assert!(heap.node_snapshot(&node).unwrap().payload.is_empty());
    assert_eq!(transient.get(), 0);
    assert_eq!(charge.get(), header_bytes + 12);
    assert_eq!(escaped.as_ref().unwrap().integer_elt(0), Some(91));
    drop(escaped);
    assert_eq!(charge.get(), header_bytes);
    let lease =
        super::attach_initialized_payload(&node, |lease| lease.set_integer_elt(1, 74)).unwrap();
    assert_eq!(lease.integer_elt(1), Some(74));
    assert!(heap.payload_lease(&node).unwrap().same_allocation(&lease));
    assert_eq!(charge.get(), header_bytes + 8);
    drop(root);
    assert!(!node.is_live());
    assert_eq!(lease.integer_elt(1), Some(74));
    drop(lease);
    // Both the original headers and the final pinned typed allocation
    // have been released and credited exactly once.
    assert_eq!(charge.get(), 0);
}

#[test]
pub(super) fn invalid_initial_shapes_preserve_arena_budget_and_reusable_slots() {
    let mut arena = super::RArena::with_budget(super::ArenaBudget::new(0, 1));
    let mut invalid = super::SexprecCore::new(super::SEXPTYPE::LISTSXP);
    invalid.data = super::NodeBody::Other;
    let original_bytes = arena.allocated_bytes.get();
    let original_offset = arena.slab_offset;
    assert!(arena.allocate_core_in_slab(|| invalid).is_null());
    assert!(arena.alloc_node(super::SEXPTYPE::FUNSXP).is_null());
    assert_eq!(arena.node_count(), 0);
    assert_eq!(arena.allocated_bytes.get(), original_bytes);
    assert_eq!(arena.slab_offset, original_offset);
    assert_eq!(arena.alloc_gc_torture_ticks, 0);

    let first = arena.alloc_node(super::SEXPTYPE::LISTSXP);
    assert!(!first.is_null());
    let bytes = arena.allocated_bytes.get();
    assert!(arena.alloc_node(super::SEXPTYPE::LISTSXP).is_null());
    assert_eq!(arena.node_count(), 1);
    assert_eq!(arena.allocated_bytes.get(), bytes);
    // SAFETY: this isolated header has no graph or payload users.
    unsafe {
        arena.free_node(first);
    }
    let free_slots = arena.free_list.clone();
    assert!(arena.allocate_core_in_slab(|| invalid).is_null());
    assert_eq!(arena.free_list, free_slots);
    assert_eq!(arena.allocated_bytes.get(), bytes);
    assert_eq!(arena.alloc_node(super::SEXPTYPE::LISTSXP), first);
}

#[test]
pub(super) fn allocation_callback_rejects_invalid_shape_without_changing_rooted_parent() {
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let heap = session
            .owner_token()
            .unwrap()
            .with_arena(|arena| arena.heap_identity())
            .unwrap();
        let callbacks = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = callbacks.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            let roots = super::fresh_allocation_roots(&heap);
            let (_, node) = roots
                .iter()
                .find(|(_, node)| {
                    heap.node_snapshot(node).unwrap().sxpinfo.type_of() == super::SEXPTYPE::LISTSXP
                })
                .unwrap();
            let original = heap.node_snapshot(node).unwrap();
            let mut invalid = original;
            invalid.data = super::NodeBody::Other;
            assert!(heap.replace_node(node, invalid).is_none());
            let current = heap.node_snapshot(node).unwrap();
            assert_eq!(current.data, original.data);
            assert_eq!(
                current.sxpinfo.type_and_flags,
                original.sxpinfo.type_and_flags
            );
            assert!(node.root_count() > 0);
            crate::sexp::gengc::full_gc();
            assert!(node.is_live());
            observed.set(observed.get() + 1);
        }));
        session.with_active_in(|owner| unsafe {
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
        });
        let value = factory
            .allocate(|arena| Some(arena.alloc_node(super::SEXPTYPE::LISTSXP)))
            .unwrap();
        assert_eq!(callbacks.get(), 1);
        assert_eq!(value.typeof_(), super::SEXPTYPE::LISTSXP);
    });
}

#[test]
pub(super) fn canonical_shape_rejection_is_atomic_and_retype_checks_actual_storage() {
    let mut arena = super::RArena::new();
    let heap = arena.heap_identity();
    let pointer = arena.alloc_vector(super::SEXPTYPE::INTSXP, 2);
    let node = arena.node_token(pointer).unwrap();
    let original = heap.node_snapshot(&node).unwrap();
    let mut invalid = original;
    invalid.data = super::NodeBody::Other;
    assert!(heap.replace_node(&node, invalid).is_none());
    assert!(arena.backing.replace_node(node.id(), invalid).is_none());
    let current = heap.node_snapshot(&node).unwrap();
    assert_eq!(current.data, original.data);
    assert_eq!(
        current.sxpinfo.type_and_flags,
        original.sxpinfo.type_and_flags
    );
    assert_eq!(current.payload, original.payload);

    heap.retype_node(&node, super::SEXPTYPE::LGLSXP).unwrap();
    assert_eq!(heap.node_snapshot(&node).unwrap().payload, original.payload);
    heap.retype_node(&node, super::SEXPTYPE::INTSXP).unwrap();
    assert!(heap.retype_node(&node, super::SEXPTYPE::REALSXP).is_none());
    let real = arena.alloc_vector(super::SEXPTYPE::REALSXP, 2);
    let real = arena.node_token(real).unwrap();
    let mut forged_payload = original;
    forged_payload.payload = heap.node_snapshot(&real).unwrap().payload;
    // A copied ID cannot replace the canonical slot attachment.
    assert!(heap.replace_node(&node, forged_payload).is_none());
    assert_eq!(heap.node_snapshot(&node).unwrap().payload, original.payload);
    heap.replace_node(&node, original).unwrap();

    let references = arena.alloc_vector(super::SEXPTYPE::VECSXP, 2);
    let references = arena.node_token(references).unwrap();
    for kind in [
        super::SEXPTYPE::STRSXP,
        super::SEXPTYPE::EXPRSXP,
        super::SEXPTYPE::BCODESXP,
    ] {
        heap.retype_node(&references, kind).unwrap();
    }
    let weak = arena.alloc_node(super::SEXPTYPE::WEAKREFSXP);
    let weak = arena.node_token(weak).unwrap();
    assert!(heap.retype_node(&weak, super::SEXPTYPE::LISTSXP).is_none());
    let pending = arena.alloc_node(super::SEXPTYPE::LGLSXP);
    let pending = arena.node_token(pending).unwrap();
    let mut header = heap.node_snapshot(&pending).unwrap();
    header.sxpinfo.set_alt(true);
    heap.replace_node(&pending, header).unwrap();
    assert!(
        heap.retype_node(&pending, super::SEXPTYPE::INTSXP)
            .is_none()
    );
}

#[test]
pub(super) fn published_characters_keep_exact_sealed_bytes_through_metadata_and_retirement() {
    let mut arena = super::RArena::new();
    let heap = arena.heap_identity();
    let pointer = arena.alloc_charsxp(b"a\0z");
    let node = arena.node_token(pointer).unwrap();
    let original = heap.node_snapshot(&node).unwrap();
    let lease = heap.payload_lease(&node).unwrap();
    assert!(lease.is_immutable());
    assert!(lease.set_byte_elt(0, b'b').is_none());
    assert!(lease.set_byte_elt(3, b'x').is_none());
    let replacement =
        super::PayloadLease::from_owned(super::OwnedPayload::characters(b"b\0z").unwrap()).unwrap();
    let mut replaced = original;
    replaced.payload = replacement.link();
    assert!(replacement.matches_header(&replaced));
    let before_bytes = arena.total_bytes_allocated();
    assert!(
        heap.publish_payload(&node, original.payload, &replacement)
            .is_none()
    );
    assert!(
        arena
            .backing
            .publish_payload(node.id(), original.payload, &replacement)
            .is_none()
    );
    assert!(heap.replace_node(&node, replaced).is_none());
    let mut shorter = original;
    shorter.data.vector_mut().length = 1; // Even a valid earlier NUL cannot change the declared length.
    assert!(lease.matches_header(&shorter));
    assert!(heap.replace_node(&node, shorter).is_none());
    let mut raw = original;
    raw.sxpinfo.set_type(super::SEXPTYPE::RAWSXP);
    assert!(heap.replace_node(&node, raw).is_none());
    assert!(arena.backing.replace_node(node.id(), raw).is_none());
    assert!(heap.retype_node(&node, super::SEXPTYPE::RAWSXP).is_none());
    let mut lazy = original;
    lazy.sxpinfo.set_alt(true);
    assert!(heap.replace_node(&node, lazy).is_none());
    let mut nil = super::SexprecCore::new(super::SEXPTYPE::NILSXP);
    nil.attrib = original.attrib;
    assert!(heap.replace_node(&node, nil).is_none());
    assert_eq!(heap.node_snapshot(&node).unwrap().data, original.data);
    assert_eq!(heap.node_snapshot(&node).unwrap().payload, original.payload);
    assert_eq!(arena.total_bytes_allocated(), before_bytes);

    let mut metadata = original;
    metadata.sxpinfo.set_named(2);
    metadata.sxpinfo.set_gp(1 << 3);
    metadata.data.vector_mut().truelength = 99;
    heap.replace_node(&node, metadata).unwrap();
    assert_eq!(heap.node_snapshot(&node).unwrap().data, metadata.data);
    assert_eq!(lease.byte_elt(0), Some(b'a'));
    unsafe { arena.free_node(pointer) };
    assert!(!node.is_live());
    let reused = arena.alloc_charsxp(b"new");
    assert_eq!(reused, pointer);
    let new_node = arena.node_token(reused).unwrap();
    assert!(heap.payload_lease(&new_node).unwrap().is_immutable());
    assert_eq!(lease.byte_elt(0), Some(b'a'));

    // An existing writable byte allocation cannot acquire character semantics.
    let raw = arena.alloc_vector(super::SEXPTYPE::RAWSXP, 2);
    let raw = arena.node_token(raw).unwrap();
    let mut character = heap.node_snapshot(&raw).unwrap();
    character.sxpinfo.set_type(super::SEXPTYPE::CHARSXP);
    character.data.vector_mut().length = 1;
    assert!(heap.payload_lease(&raw).unwrap().matches_header(&character));
    assert!(heap.replace_node(&raw, character).is_none());
    let raw_header = heap.node_snapshot(&raw).unwrap();
    assert!(
        heap.publish_payload(&raw, raw_header.payload, &lease)
            .is_none()
    );
    assert!(
        arena
            .backing
            .publish_payload(raw.id(), raw_header.payload, &lease)
            .is_none()
    );
    assert_eq!(
        heap.payload_lease(&raw).unwrap().set_byte_elt(0, b'w'),
        Some(())
    );
}

#[test]
pub(super) fn generic_empty_character_allocation_is_sealed_and_budgeted() {
    let mut denied = super::RArena::with_budget(super::ArenaBudget::new(super::NODE_BYTES, 0));
    assert!(denied.alloc_node(super::SEXPTYPE::CHARSXP).is_null());
    assert_eq!(denied.total_bytes_allocated(), 0);
    let mut arena = super::RArena::new();
    let pointer = arena.alloc_node(super::SEXPTYPE::CHARSXP);
    let node = arena.node_token(pointer).unwrap();
    let lease = arena.heap_identity().payload_lease(&node).unwrap();
    assert_eq!(lease.capacity(), 1);
    assert_eq!(lease.byte_elt(0), Some(0));
    assert!(lease.is_immutable());
    assert_eq!(
        arena.total_bytes_allocated(),
        super::tests_physical_headers::header_backing_bytes(&arena) + 1
    );
    let mut lazy = super::SexprecCore::new(super::SEXPTYPE::CHARSXP);
    lazy.sxpinfo.set_alt(true);
    let before = arena.total_bytes_allocated();
    assert!(arena.allocate_core_in_slab(|| lazy).is_null());
    assert_eq!(arena.total_bytes_allocated(), before);
}

#[test]
pub(super) fn character_allocation_callbacks_observe_only_sealed_initialized_bytes() {
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let heap = session
            .owner_token()
            .unwrap()
            .with_arena(|arena| arena.heap_identity())
            .unwrap();
        let observed = std::rc::Rc::new(std::cell::Cell::new(0));
        let callbacks = observed.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            let (_, node) = super::fresh_allocation_roots(&heap)
                .into_iter()
                .find(|(_, node)| {
                    heap.node_snapshot(node).unwrap().sxpinfo.type_of() == super::SEXPTYPE::CHARSXP
                })
                .unwrap();
            let header = heap.node_snapshot(&node).unwrap();
            let lease = heap.payload_lease(&node).unwrap();
            assert!(lease.is_immutable());
            let length = usize::try_from(header.data.vector().length).unwrap();
            assert_eq!(lease.byte_elt(length), Some(0));
            assert!(lease.set_byte_elt(0, b'x').is_none());
            assert!(
                heap.publish_payload(&node, header.payload, &lease)
                    .is_none()
            );
            crate::sexp::gengc::full_gc();
            assert!(node.is_live());
            assert!(lease.matches_header(&heap.node_snapshot(&node).unwrap()));
            callbacks.set(callbacks.get() + 1);
        }));
        session.with_active_in(|owner| unsafe {
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
        });
        let text = factory.character("µ").unwrap();
        let header = text.header();
        let payload = header.payload_lease().unwrap();
        assert_eq!(payload.byte_elt(0), Some(0xc2));
        assert_eq!(payload.byte_elt(1), Some(0xb5));
        let empty = factory
            .allocate(|arena| Some(arena.alloc_node(super::SEXPTYPE::CHARSXP)))
            .unwrap();
        assert_eq!(empty.header().payload_lease().unwrap().byte_elt(0), Some(0));
        assert_eq!(observed.get(), 2);
    });
}

pub(super) struct NativeResourceSpy {
    drops: std::rc::Rc<std::cell::Cell<usize>>,
    callback: Option<Box<dyn Fn()>>,
}

impl Drop for NativeResourceSpy {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        if let Some(callback) = &self.callback {
            callback();
        }
    }
}

#[test]
pub(super) fn canonical_resources_reject_foreign_types_and_retired_generations() {
    let mut arena = super::RArena::new();
    let mut foreign = super::RArena::new();
    let heap = arena.heap_identity();
    let pointer = arena.alloc_node(crate::sexp::ffi::SEXPTYPE::EXTPTRSXP);
    let node = arena.node_token(pointer).unwrap();
    let wrong_kind = arena.alloc_node(crate::sexp::ffi::SEXPTYPE::INTSXP);
    let wrong_kind = arena.node_token(wrong_kind).unwrap();
    let foreign_pointer = foreign.alloc_node(crate::sexp::ffi::SEXPTYPE::EXTPTRSXP);
    let foreign = foreign.node_token(foreign_pointer).unwrap();
    let state = std::rc::Rc::new(String::from("canonical state"));
    assert!(heap.attach_resource(&wrong_kind, state.clone()).is_none());
    assert!(heap.attach_resource(&foreign, state.clone()).is_none());
    heap.attach_resource(&node, state.clone()).unwrap();
    assert!(
        heap.attach_resource(&node, std::rc::Rc::new(7_u32))
            .is_none()
    );
    assert!(heap.resource::<u32>(&node).is_none());
    assert!(std::rc::Rc::ptr_eq(
        &state,
        &heap.resource::<String>(&node).unwrap()
    ));
    let erased = heap.take_resource(&node).unwrap();
    assert!(heap.resource::<String>(&node).is_none());
    assert!(std::rc::Rc::ptr_eq(
        &state,
        &erased.downcast::<String>().unwrap()
    ));
    heap.attach_resource(&node, state.clone()).unwrap();
    let address_only = std::ptr::without_provenance_mut(pointer.addr());
    // SAFETY: no graph/payload reference uses this allocation; only its
    // checked identity remains to verify exact retirement.
    unsafe {
        arena.free_node(address_only);
    }
    assert!(!node.is_live());
    assert!(heap.resource::<String>(&node).is_none());
    assert!(heap.take_resource(&node).is_none());
    let replacement = arena.alloc_node(crate::sexp::ffi::SEXPTYPE::EXTPTRSXP);
    assert_eq!(pointer.addr(), replacement.addr());
    let fresh = arena.node_token(replacement).unwrap();
    assert!(heap.resource::<String>(&fresh).is_none());
    assert!(heap.attach_resource(&node, state.clone()).is_none());
    heap.attach_resource(&fresh, state.clone()).unwrap();
    assert!(heap.resource::<String>(&fresh).is_some());
}

#[test]
pub(super) fn resource_cleanup_uses_exact_identity_after_type_or_body_mutation() {
    use crate::sexp::ffi::{SEXPTYPE, SexprecCore};

    for replace_body in [false, true] {
        let mut arena = super::RArena::new();
        let foreign = super::RArena::new();
        let heap = arena.heap_identity();
        let pointer = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
        let node = arena.node_token(pointer).unwrap();
        let drops = std::rc::Rc::new(std::cell::Cell::new(0));
        let state = std::rc::Rc::new(NativeResourceSpy {
            drops: drops.clone(),
            callback: None,
        });
        let original = std::rc::Rc::downgrade(&state);
        heap.attach_resource(&node, state).unwrap();

        let changed = if replace_body {
            SexprecCore::new(SEXPTYPE::LISTSXP)
        } else {
            let mut header = heap.node_snapshot(&node).unwrap();
            header.sxpinfo.set_type(SEXPTYPE::LISTSXP);
            header
        };
        if replace_body {
            heap.replace_node(&node, changed).unwrap();
            assert!(heap.resource_erased(&node).is_none());
        } else {
            assert!(heap.replace_node(&node, changed).is_none());
            assert_eq!(
                heap.node_snapshot(&node).unwrap().sxpinfo.type_of(),
                SEXPTYPE::EXTPTRSXP
            );
            assert!(heap.resource::<NativeResourceSpy>(&node).is_some());
        }
        assert!(
            heap.attach_resource(&node, std::rc::Rc::new(7_u32))
                .is_none()
        );
        assert!(foreign.heap_identity().take_resource(&node).is_none());
        assert_eq!(drops.get(), 0);

        let detached = heap.take_resource(&node).unwrap();
        let detached = detached.downcast::<NativeResourceSpy>().unwrap();
        assert!(std::rc::Rc::ptr_eq(&original.upgrade().unwrap(), &detached));
        assert!(heap.take_resource(&node).is_none());
        assert_eq!(drops.get(), 0);
        drop(detached);
        assert_eq!(drops.get(), 1);
        assert!(original.upgrade().is_none());

        // SAFETY: no graph or payload reference uses this allocation.
        unsafe {
            arena.free_node(pointer);
        }
        let replacement = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
        assert_eq!(pointer.addr(), replacement.addr());
        let fresh = arena.node_token(replacement).unwrap();
        heap.attach_resource(
            &fresh,
            std::rc::Rc::new(NativeResourceSpy {
                drops: drops.clone(),
                callback: None,
            }),
        )
        .unwrap();
        assert!(!node.is_live());
        assert!(heap.take_resource(&node).is_none());
        assert!(heap.resource::<NativeResourceSpy>(&fresh).is_some());
        assert_eq!(drops.get(), 1);
        drop(heap.take_resource(&fresh).unwrap());
        assert_eq!(drops.get(), 2);
    }
}

#[test]
pub(super) fn retired_resources_reenter_only_after_normal_or_unwinding_arena_lends() {
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    let drops = std::rc::Rc::new(std::cell::Cell::new(0));
    let callbacks = std::rc::Rc::new(std::cell::Cell::new(0));
    for unwind in [false, true] {
        let expected = drops.get() + 1;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.with_active_in(|owner| unsafe {
                super::with_arena_in(owner, |arena| {
                    let pointer = arena.alloc_node(crate::sexp::ffi::SEXPTYPE::EXTPTRSXP);
                    let node = arena.node_token(pointer).unwrap();
                    let callbacks = callbacks.clone();
                    let state = std::rc::Rc::new(NativeResourceSpy {
                        drops: drops.clone(),
                        callback: Some(Box::new(move || {
                            assert!(!super::is_arena_lent(owner));
                            // SAFETY: the enclosing session remains live.
                            // The resource guard releases the old lend first.
                            super::with_arena_in(owner, |arena| {
                                arena.alloc_node(crate::sexp::ffi::SEXPTYPE::INTSXP);
                            });
                            callbacks.set(callbacks.get() + 1);
                        })),
                    });
                    node.heap_identity().attach_resource(&node, state).unwrap();
                    arena.free_node(pointer);
                    assert_eq!(drops.get(), expected - 1);
                    if unwind {
                        panic!("resource unwind fixture");
                    }
                });
            });
        }));
        assert_eq!(outcome.is_err(), unwind);
        assert_eq!(drops.get(), expected);
        assert_eq!(callbacks.get(), expected);
    }
}

#[test]
pub(super) fn permanent_resource_removal_defers_release_and_preserves_typed_borrowers() {
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    let drops = std::rc::Rc::new(std::cell::Cell::new(0));
    session.with_active_in(|owner| unsafe {
        let pointer = (*owner)
            .persistent_nodes
            .allocate_header(crate::sexp::ffi::SexprecCore::new(
                crate::sexp::ffi::SEXPTYPE::EXTPTRSXP,
            ))
            .unwrap();
        let heap = (*owner).heap_identity.clone();
        let node = super::checked_projection(pointer).unwrap().1;
        heap.attach_resource(
            &node,
            std::rc::Rc::new(NativeResourceSpy {
                drops: drops.clone(),
                callback: Some(Box::new(move || {
                    assert!(!super::is_arena_lent(owner));
                    super::with_arena_in(owner, |arena| {
                        arena.alloc_node(crate::sexp::ffi::SEXPTYPE::INTSXP);
                    });
                })),
            }),
        )
        .unwrap();
        let borrower = heap.resource::<NativeResourceSpy>(&node).unwrap();
        super::with_arena_in(owner, |_| {
            // Only the disjoint persistent facade is lent here. Removal
            // detaches its state into the same post-arena-lend queue.
            assert!((*owner).persistent_nodes.remove(pointer));
            assert_eq!(drops.get(), 0);
            assert!(!node.is_live());
            assert!(heap.resource::<NativeResourceSpy>(&node).is_none());
        });
        assert_eq!(drops.get(), 0, "typed borrower independently owns state");
        drop(borrower);
        assert_eq!(drops.get(), 1);
        let fresh = (*owner)
            .persistent_nodes
            .allocate_header(crate::sexp::ffi::SexprecCore::new(
                crate::sexp::ffi::SEXPTYPE::EXTPTRSXP,
            ))
            .unwrap();
        let fresh = super::checked_projection(fresh).unwrap().1;
        assert_ne!(node.link(), fresh.link());
        assert!(heap.resource::<NativeResourceSpy>(&fresh).is_none());
    });
}

#[test]
pub(super) fn canonical_resource_lives_until_final_physical_page_owner_drops() {
    let drops = std::rc::Rc::new(std::cell::Cell::new(0));
    let (heap, node, root) = {
        let mut arena = super::RArena::new();
        let pointer = arena.alloc_node(crate::sexp::ffi::SEXPTYPE::EXTPTRSXP);
        let node = arena.node_token(pointer).unwrap();
        let heap = arena.heap_identity();
        heap.attach_resource(
            &node,
            std::rc::Rc::new(NativeResourceSpy {
                drops: drops.clone(),
                callback: None,
            }),
        )
        .unwrap();
        let root = node.root_lease().unwrap();
        (heap, node, root)
    };
    assert_eq!(drops.get(), 0);
    assert!(heap.resource::<NativeResourceSpy>(&node).is_some());
    drop(root);
    assert_eq!(drops.get(), 1);
    assert!(!node.is_live());
    assert!(heap.resource::<NativeResourceSpy>(&node).is_none());
}

#[test]
pub(super) fn standalone_arena_teardown_releases_detached_resources_once() {
    let drops = std::rc::Rc::new(std::cell::Cell::new(0));
    {
        let mut arena = super::RArena::new();
        let pointer = arena.alloc_node(crate::sexp::ffi::SEXPTYPE::EXTPTRSXP);
        let node = arena.node_token(pointer).unwrap();
        node.heap_identity()
            .attach_resource(
                &node,
                std::rc::Rc::new(NativeResourceSpy {
                    drops: drops.clone(),
                    callback: None,
                }),
            )
            .unwrap();
        unsafe {
            arena.free_node(pointer);
        }
        assert_eq!(drops.get(), 0);
    }
    assert_eq!(drops.get(), 1);
}

#[test]
pub(super) fn canonical_scalar_and_attribute_links_never_refresh_reused_child_addresses() {
    use super::super::{ffi::EdgeField, heap::ReferenceChild};
    let mut arena = super::RArena::new();
    let heap = arena.heap_identity();
    let parent = arena.alloc_node(super::SEXPTYPE::LISTSXP);
    let parent_token = arena.node_token(parent).unwrap();
    let child = arena.alloc_node(super::SEXPTYPE::LISTSXP);
    let child_token = arena.node_token(child).unwrap();
    let original = child_token.link().unwrap();
    for field in [EdgeField::ListCar, EdgeField::Attribute] {
        heap.set_edge(&parent_token, field, ReferenceChild::Node(&child_token))
            .unwrap();
        assert_eq!(heap.edge(&parent_token, field), Some(original));
    }
    assert!(
        heap.set_edge(&parent_token, EdgeField::SymbolName, ReferenceChild::Null)
            .is_none()
    );
    let mut foreign = super::RArena::new();
    let foreign_child = foreign.alloc_node(super::SEXPTYPE::LISTSXP);
    let foreign_token = foreign.node_token(foreign_child).unwrap();
    assert!(
        heap.set_edge(
            &parent_token,
            EdgeField::Attribute,
            ReferenceChild::Node(&foreign_token)
        )
        .is_none()
    );
    unsafe {
        arena.free_node(child);
    }
    let replacement = arena.alloc_node(super::SEXPTYPE::LISTSXP);
    assert_eq!(replacement, child);
    let replacement_link = arena.node_token(replacement).unwrap().link().unwrap();
    assert_ne!(original, replacement_link);
    for field in [EdgeField::ListCar, EdgeField::Attribute] {
        assert_eq!(heap.edge(&parent_token, field), Some(original));
        assert!(
            heap.projection_of_link(heap.edge(&parent_token, field).unwrap())
                .is_none()
        );
        assert!(
            heap.set_edge(&parent_token, field, ReferenceChild::Node(&child_token))
                .is_none()
        );
    }
    assert_eq!(heap.projection_of_link(replacement_link), Some(replacement));
}

#[test]
pub(super) fn canonical_singleton_links_keep_two_original_banks_distinct_after_tls_close() {
    use super::super::{
        globals,
        heap::{ReferenceChild, ResolvedLink},
    };
    let mut arena = super::RArena::new();
    let heap = arena.heap_identity();
    let parent = arena.alloc_vector(super::SEXPTYPE::VECSXP, 2);
    let token = arena.node_token(parent).unwrap();
    let root = token.root_lease().unwrap();
    let first_pool = globals::immutable_singleton_pool();
    let first = first_pool.nil();
    let first_pointer = first.projection();
    let first_cookie = first.identity_cookie();
    heap.set_reference_elt(&token, 0, ReferenceChild::Singleton(&first))
        .unwrap();
    globals::close_immutable_singletons_for_test();
    let second_pool = globals::immutable_singleton_pool();
    let second = second_pool.nil();
    let second_pointer = second.projection();
    let second_cookie = second.identity_cookie();
    heap.set_reference_elt(&token, 1, ReferenceChild::Singleton(&second))
        .unwrap();
    let links = heap.reference_links(&token).unwrap();
    assert_ne!(links[0], links[1]);
    assert_ne!(first_cookie, second_cookie);
    globals::close_immutable_singletons_for_test();
    drop(first);
    drop(second);
    drop(first_pool);
    drop(second_pool);
    drop(arena);
    for (link, cookie, pointer) in [
        (links[0], first_cookie, first_pointer),
        (links[1], second_cookie, second_pointer),
    ] {
        let ResolvedLink::Singleton(lease) = heap.resolve_link(link).unwrap() else {
            panic!("original singleton lease expected");
        };
        assert_eq!(lease.identity_cookie(), cookie);
        assert_eq!(lease.projection(), pointer);
        assert_eq!(lease.kind(), globals::SingletonKind::Nil);
    }
    assert_eq!(
        heap.reference_elements(&token),
        Some(vec![first_pointer, second_pointer])
    );
    drop(root);
    assert!(heap.resolve_link(links[0]).is_none());
    assert!(heap.resolve_link(links[1]).is_none());
}

#[test]
pub(super) fn nonowning_cyclic_links_release_physical_storage_with_the_last_root() {
    use super::super::{ffi::EdgeField, heap::ReferenceChild};
    let mut arena = super::RArena::new();
    let heap = arena.heap_identity();
    let first = arena.alloc_node(super::SEXPTYPE::LISTSXP);
    let first_token = arena.node_token(first).unwrap();
    let second = arena.alloc_node(super::SEXPTYPE::LISTSXP);
    let second_token = arena.node_token(second).unwrap();
    heap.set_edge(
        &first_token,
        EdgeField::ListCdr,
        ReferenceChild::Node(&second_token),
    )
    .unwrap();
    heap.set_edge(
        &second_token,
        EdgeField::ListCdr,
        ReferenceChild::Node(&first_token),
    )
    .unwrap();
    let root = first_token.root_lease().unwrap();
    let storage = std::rc::Rc::downgrade(&arena.backing);
    let first_link = first_token.link().unwrap();
    drop(arena);
    assert!(storage.upgrade().is_some());
    assert_eq!(heap.projection_of_link(first_link), Some(first));
    drop(root);
    assert!(storage.upgrade().is_none());
    assert!(!first_token.is_live());
    assert!(!second_token.is_live());
    assert!(heap.resolve_link(first_link).is_none());
}

#[test]
pub(super) fn reference_vector_budget_charges_actual_link_cells_and_releases_them() {
    let bytes = 3 * std::mem::size_of::<super::NodeLink>();
    let mut arena =
        super::RArena::with_budget(super::ArenaBudget::new(super::NODE_BYTES + bytes, 1));
    let vector = arena.alloc_vector(super::SEXPTYPE::VECSXP, 3);
    assert!(!vector.is_null());
    assert_eq!(arena.allocated_bytes.get(), super::NODE_BYTES + bytes);
    unsafe {
        arena.free_node(vector);
    }
    assert_eq!(arena.allocated_bytes.get(), super::NODE_BYTES);
    let numeric = arena.alloc_vector(super::SEXPTYPE::INTSXP, 3);
    assert_eq!(numeric, vector);
    assert_eq!(arena.allocated_bytes.get(), super::NODE_BYTES + 12);
}

#[test]
pub(super) fn checked_reference_writes_reject_foreign_stale_children_and_parent_reuse() {
    use super::super::heap::ReferenceChild;
    let mut arena = super::RArena::new();
    let heap = arena.heap_identity();
    let parent = arena.alloc_vector(super::SEXPTYPE::VECSXP, 3);
    let parent_token = arena.node_token(parent).unwrap();
    let child = arena.alloc_vector(super::SEXPTYPE::INTSXP, 1);
    let child_token = arena.node_token(child).unwrap();
    heap.set_reference_elt(&parent_token, 1, ReferenceChild::Node(&child_token))
        .unwrap();
    let expected = vec![std::ptr::null_mut(), child, std::ptr::null_mut()];
    assert_eq!(
        heap.reference_elements(&parent_token),
        Some(expected.clone())
    );
    let mut foreign = super::RArena::new();
    let foreign_child = foreign.alloc_vector(super::SEXPTYPE::INTSXP, 1);
    let foreign_token = foreign.node_token(foreign_child).unwrap();
    assert!(
        heap.replace_reference_elements(
            &parent_token,
            0,
            &[
                ReferenceChild::Node(&child_token),
                ReferenceChild::Node(&foreign_token)
            ]
        )
        .is_none()
    );
    assert!(
        heap.set_reference_elt(&parent_token, 3, ReferenceChild::Null)
            .is_none()
    );
    assert!(
        heap.replace_reference_elements(&parent_token, usize::MAX, &[ReferenceChild::Null])
            .is_none()
    );
    assert_eq!(
        heap.reference_elements(&parent_token),
        Some(expected.clone())
    );
    unsafe {
        arena.free_node(child);
    }
    let replacement = arena.alloc_vector(super::SEXPTYPE::INTSXP, 1);
    assert_eq!(replacement, child);
    assert!(
        heap.set_reference_elt(&parent_token, 0, ReferenceChild::Node(&child_token))
            .is_none()
    );
    assert!(heap.reference_elements(&parent_token).is_none());
    assert_eq!(
        heap.reference_link_elt(&parent_token, 1),
        Some(child_token.id().link())
    );
    assert!(heap.resolve_link(child_token.id().link()).is_none());
    unsafe {
        arena.free_node(parent);
    }
    let replacement = arena.alloc_vector(super::SEXPTYPE::VECSXP, 3);
    assert_eq!(replacement, parent);
    let replacement_token = arena.node_token(replacement).unwrap();
    assert!(heap.reference_elements(&parent_token).is_none());
    assert!(
        heap.set_reference_elt(&parent_token, 0, ReferenceChild::Null)
            .is_none()
    );
    assert_eq!(
        heap.reference_elements(&replacement_token),
        Some(vec![std::ptr::null_mut(); 3])
    );
    assert!(arena.backing.node_pages.try_borrow_mut().is_ok());
    assert!(
        arena
            .backing
            .node_pages
            .borrow()
            .iter()
            .all(|page| page.payloads.try_borrow_mut().is_ok())
    );
}

#[test]
pub(super) fn checked_reference_headers_validate_body_kind_length_and_actual_payload() {
    let mut arena = super::RArena::new();
    let heap = arena.heap_identity();
    let parent = arena.alloc_vector(super::SEXPTYPE::VECSXP, 3);
    let token = arena.node_token(parent).unwrap();
    let original = arena.backing.node_snapshot(token.id()).unwrap();
    let replace = |header| {
        arena.backing.node_pages.borrow()[token.id().page()]
            .storage
            .replace_live(token.id(), header)
            .unwrap()
    };
    let mut header = original;
    header.data.vector_mut().length = 4;
    replace(header);
    assert!(heap.reference_elements(&token).is_none());
    header.data.vector_mut().length = -1;
    replace(header);
    assert!(heap.reference_elt(&token, 0).is_none());
    header = original;
    header.data = super::NodeBody::Other;
    replace(header);
    assert!(heap.reference_elements(&token).is_none());
    header = original;
    header.sxpinfo.set_alt(true);
    replace(header);
    assert_eq!(
        heap.reference_elements(&token),
        Some(vec![std::ptr::null_mut(); 3])
    );
    assert_eq!(
        heap.set_reference_elt(&token, 0, super::super::heap::ReferenceChild::Null),
        Some(())
    );
    header.payload = super::PayloadLink::EMPTY;
    replace(header);
    assert!(heap.reference_elements(&token).is_none());
    assert!(
        heap.set_reference_elt(&token, 0, super::super::heap::ReferenceChild::Null)
            .is_none()
    );
    header.data.vector_mut().length = 0;
    replace(header);
    assert_eq!(heap.reference_elements(&token), Some(Vec::new()));
    header.data.vector_mut().length = 3;
    replace(original);
    let numbers = arena.alloc_vector(super::SEXPTYPE::INTSXP, 3);
    let numbers_token = arena.node_token(numbers).unwrap();
    let mut header = original;
    header.payload = arena
        .backing
        .node_snapshot(numbers_token.id())
        .unwrap()
        .payload;
    arena.backing.node_pages.borrow()[token.id().page()]
        .storage
        .replace_live(token.id(), header)
        .unwrap();
    assert!(heap.reference_elements(&token).is_none());
    assert!(heap.reference_elements(&numbers_token).is_none());
    let empty = arena.alloc_vector(super::SEXPTYPE::VECSXP, 0);
    let empty_token = arena.node_token(empty).unwrap();
    assert_eq!(heap.reference_elements(&empty_token), Some(Vec::new()));
    assert!(heap.reference_elt(&empty_token, 0).is_none());
    assert_eq!(
        heap.replace_reference_elements(&empty_token, 0, &[]),
        Some(())
    );
}

#[test]
pub(super) fn checked_reference_cells_keep_the_actual_singleton_after_pool_and_facade_drop() {
    use super::super::{globals, heap::ReferenceChild};
    let mut arena = super::RArena::new();
    let heap = arena.heap_identity();
    let parent = arena.alloc_vector(super::SEXPTYPE::VECSXP, 1);
    let token = arena.node_token(parent).unwrap();
    let root = token.root_lease().unwrap();
    let pool = globals::immutable_singleton_pool();
    let child = pool.nil();
    let projection = child.projection();
    heap.set_reference_elt(&token, 0, ReferenceChild::Singleton(&child))
        .unwrap();
    globals::close_immutable_singletons_for_test();
    drop(child);
    drop(pool);
    drop(arena);
    assert_eq!(heap.reference_elt(&token, 0), Some(projection));
    let retained = heap.retained_singleton(projection).unwrap();
    assert_eq!(
        retained.snapshot().sxpinfo.type_of(),
        super::SEXPTYPE::NILSXP
    );
    let new_pool = globals::immutable_singleton_pool();
    assert_ne!(new_pool.nil().projection(), projection);
    drop(retained);
    drop(root);
    assert!(heap.retained_singleton(projection).is_none());
    assert!(heap.reference_elements(&token).is_none());
}

#[test]
pub(super) fn checked_reference_cells_resolve_multiple_same_heap_stores_without_page_aliasing() {
    use super::super::{heap::ReferenceChild, instance::persistent::PersistentHeap};
    let mut arena = super::RArena::new();
    let heap = arena.heap_identity();
    let parent = arena.alloc_vector(super::SEXPTYPE::VECSXP, 2);
    let parent_token = arena.node_token(parent).unwrap();
    let root = parent_token.root_lease().unwrap();
    let mut later = super::RArena::fresh_with_identity(heap.clone());
    let child = later.alloc_vector(super::SEXPTYPE::INTSXP, 1);
    let child_token = later.node_token(child).unwrap();
    let mut permanent = PersistentHeap::new(heap.clone());
    let scalar = permanent.allocate_integer(9, false).unwrap();
    let scalar_token = permanent.token(scalar).unwrap();
    let string = permanent.allocate_string(std::ptr::null_mut()).unwrap();
    let string_token = permanent.token(string).unwrap();
    heap.replace_reference_elements(
        &parent_token,
        0,
        &[
            ReferenceChild::Node(&child_token),
            ReferenceChild::Node(&scalar_token),
        ],
    )
    .unwrap();
    heap.set_reference_elt(&string_token, 0, ReferenceChild::Node(&parent_token))
        .unwrap();
    assert_eq!(
        heap.reference_elements(&parent_token),
        Some(vec![child, scalar])
    );
    assert_eq!(heap.reference_elt(&string_token, 0), Some(parent));
    permanent.remove(scalar);
    assert!(
        heap.set_reference_elt(&string_token, 0, ReferenceChild::Node(&scalar_token))
            .is_none()
    );
    assert_eq!(heap.reference_elt(&string_token, 0), Some(parent));
    drop(arena);
    drop(later);
    drop(permanent);
    assert_eq!(heap.reference_elt(&string_token, 0), Some(parent));
    drop(root);
    assert!(heap.reference_elt(&string_token, 0).is_none());
}

#[test]
pub(super) fn heap_reference_snapshots_use_only_typed_owners_in_the_same_domain() {
    let mut arena = super::RArena::new();
    let identity = arena.heap_identity();
    let vector = arena.alloc_vector(super::SEXPTYPE::VECSXP, 3);
    let vector_token = arena.node_token(vector).unwrap();
    let value = vector_token.root_lease().unwrap();
    assert_eq!(
        identity.reference_elements(&vector_token),
        Some(vec![std::ptr::null_mut(); 3])
    );
    let snapshot = identity.payload_lease(&vector_token).unwrap();
    assert!(snapshot.copy_references(4).is_none());
    assert!(
        arena
            .backing
            .node_pages
            .borrow()
            .iter()
            .all(|page| page.payloads.try_borrow_mut().is_ok())
    );
    let integers = arena.alloc_vector(super::SEXPTYPE::INTSXP, 3);
    let integer_token = arena.node_token(integers).unwrap();
    assert!(identity.reference_elements(&integer_token).is_none());
    let mut foreign = super::RArena::new();
    let foreign_vector = foreign.alloc_vector(super::SEXPTYPE::VECSXP, 3);
    let foreign_token = foreign.node_token(foreign_vector).unwrap();
    assert!(identity.reference_elements(&foreign_token).is_none());
    let mut permanent = super::super::instance::persistent::PersistentHeap::new(identity.clone());
    let string = permanent.allocate_string(vector).unwrap();
    let string_token = permanent.token(string).unwrap();
    assert_eq!(
        identity.reference_elements(&string_token),
        Some(vec![vector])
    );
    drop(arena);
    drop(permanent);
    assert_eq!(
        identity.reference_elements(&vector_token),
        Some(vec![std::ptr::null_mut(); 3])
    );
    assert_eq!(
        identity.reference_elements(&string_token),
        Some(vec![vector])
    );
    drop(value);
    assert!(identity.reference_elements(&vector_token).is_none());
    assert!(identity.reference_elements(&string_token).is_none());
    // A detached payload snapshot pins the original actual cells alone,
    // without reviving a dead parent or manufacturing graph authority.
    assert_eq!(
        snapshot.copy_references(3),
        Some(vec![super::NodeLink::NULL; 3])
    );
}

#[test]
pub(super) fn automatic_roots_span_registered_arena_and_permanent_pages_in_one_heap() {
    let mut arena = super::RArena::new();
    let identity = arena.heap_identity();
    let mut permanent = super::super::instance::persistent::PersistentHeap::new(identity.clone());
    let arena_node = arena.alloc_node(super::SEXPTYPE::LISTSXP);
    let arena_token = arena.node_token(arena_node).unwrap();
    let arena_lease = arena_token.root_lease().unwrap();
    let permanent_node = permanent
        .allocate_header(super::SexprecCore::new(super::SEXPTYPE::ENVSXP))
        .unwrap();
    let permanent_token = permanent.token(permanent_node).unwrap();
    let permanent_lease = permanent_token.root_lease().unwrap();
    let mut foreign = super::RArena::new();
    let foreign_node = foreign.alloc_node(super::SEXPTYPE::INTSXP);
    let foreign_token = foreign.node_token(foreign_node).unwrap();
    let foreign_lease = foreign_token.root_lease().unwrap();
    let roots = super::automatic_roots(&identity);
    assert_eq!(roots.len(), 2);
    assert!(roots.contains(&(arena_node, arena_token.clone())));
    assert!(roots.contains(&(permanent_node, permanent_token.clone())));
    assert!(!roots.contains(&(foreign_node, foreign_token.clone())));
    drop(permanent_lease);
    assert_eq!(
        super::automatic_roots(&identity),
        [(arena_node, arena_token.clone())]
    );
    super::slab::SLAB_META.with(|pages| {
        let _directory_borrow = pages.borrow();
        drop(arena_lease);
    });
    assert!(super::automatic_roots(&identity).is_empty());
    let retained_metadata = arena_token.root_lease().unwrap();
    drop(arena);
    drop(permanent);
    assert!(arena_token.is_live());
    assert_eq!(
        super::automatic_roots(&identity),
        [(arena_node, arena_token.clone())]
    );
    drop(retained_metadata);
    assert!(!arena_token.is_live());
    assert!(!permanent_token.is_live());
    assert!(super::automatic_roots(&identity).is_empty());
    drop(foreign_lease);
}

#[test]
pub(super) fn automatic_value_retains_canonical_vector_and_later_character_payloads() {
    let mut arena = super::RArena::new();
    let vector = arena.alloc_vector(super::SEXPTYPE::REALSXP, 2);
    let vector_token = arena.node_token(vector).unwrap();
    let value = vector_token.root_lease().unwrap();
    // SAFETY: this fixture owns the live initialized real payload and no
    // payload reference or callback overlaps either raw write.
    unsafe {
        let elements = native_payload(vector).cast::<f64>();
        elements.write(12.5);
        elements.add(1).write(-7.0);
    }
    let chars = arena.alloc_charsxp(b"allocated after the value");
    let chars_token = arena.node_token(chars).unwrap();
    let physical_owner = std::rc::Rc::downgrade(&arena.backing);
    drop(arena);
    assert!(physical_owner.upgrade().is_some());
    assert!(vector_token.is_live());
    assert!(chars_token.is_live());
    let vector_header = super::checked_snapshot(vector, &vector_token).unwrap();
    let chars_header = super::checked_snapshot(chars, &chars_token).unwrap();
    // SAFETY: the automatic value retains the original canonical backing
    // and both typed payloads; reads copy values without returning loans.
    unsafe {
        let vector_lease = vector_token
            .heap_identity()
            .payload_lease(&vector_token)
            .unwrap();
        assert_eq!(vector_header.payload, vector_lease.link());
        let elements = vector_lease.native_projection().cast::<f64>();
        assert_eq!(elements.read(), 12.5);
        assert_eq!(elements.add(1).read(), -7.0);
        assert_eq!(
            std::slice::from_raw_parts(
                chars_token
                    .heap_identity()
                    .payload_lease(&chars_token)
                    .unwrap()
                    .native_projection()
                    .cast::<u8>(),
                25
            ),
            b"allocated after the value"
        );
    }
    drop(value);
    assert!(physical_owner.upgrade().is_none());
    assert!(!vector_token.is_live());
    assert!(!chars_token.is_live());
}

#[test]
pub(super) fn existing_value_retains_later_backing_and_its_graph_children_without_cycles() {
    let mut original = super::RArena::new();
    let identity = original.heap_identity();
    let parent = original.alloc_node(super::SEXPTYPE::LISTSXP);
    let parent_token = original.node_token(parent).unwrap();
    let value = parent_token.root_lease().unwrap();
    let mut later = super::RArena::fresh_with_identity(identity.clone());
    let child = later.alloc_charsxp(b"future store child");
    let child_token = later.node_token(child).unwrap();
    // SAFETY: both initialized nodes belong to this same heap domain;
    // this fixture installs the nonowning edge before facade teardown.
    identity
        .set_edge(
            &parent_token,
            super::super::ffi::EdgeField::ListCar,
            super::super::heap::ReferenceChild::Node(&child_token),
        )
        .unwrap();
    let original_owner = std::rc::Rc::downgrade(&original.backing);
    let later_owner = std::rc::Rc::downgrade(&later.backing);
    drop(original);
    drop(later);
    assert!(parent_token.is_live());
    assert!(child_token.is_live());
    assert!(original_owner.upgrade().is_some());
    assert!(later_owner.upgrade().is_some());
    assert_eq!(
        super::automatic_roots(&identity),
        [(parent, parent_token.clone())]
    );
    let header = super::checked_snapshot(parent, &parent_token).unwrap();
    // SAFETY: the parent header type is LISTSXP and the retained child
    // projection is precisely the one installed above.
    assert_eq!(
        identity.projection_of_link(header.data.list().carval),
        Some(child)
    );
    drop(value);
    assert!(original_owner.upgrade().is_none());
    assert!(later_owner.upgrade().is_none());
    assert!(!parent_token.is_live());
    assert!(!child_token.is_live());
    assert!(super::automatic_roots(&identity).is_empty());
}

#[test]
pub(super) fn arena_iterator_rejects_reused_generations_without_holding_storage_loans() {
    let mut arena = super::RArena::new();
    let first = arena.alloc_node(super::SEXPTYPE::LISTSXP);
    let discarded = arena.alloc_node(super::SEXPTYPE::LISTSXP);
    let first_token = arena.node_token(first).unwrap();
    let discarded_token = arena.node_token(discarded).unwrap();
    let mut nodes = arena.active_nodes();
    assert!(arena.backing.node_pages.try_borrow_mut().is_ok());
    // SAFETY: discarded is unreachable and no payload/header loan lives.
    unsafe {
        arena.free_node(discarded);
    }
    let replacement = arena.alloc_node(super::SEXPTYPE::REALSXP);
    assert_eq!(replacement, discarded);
    assert!(!discarded_token.is_live());
    let replacement_token = arena.node_token(replacement).unwrap();
    let fresh = arena.alloc_node(super::SEXPTYPE::ENVSXP);
    let fresh_token = arena.node_token(fresh).unwrap();
    let physical_owner = std::rc::Rc::downgrade(&arena.backing);
    drop(arena);
    assert!(first_token.is_live());
    assert_eq!(nodes.next(), Some(first));
    assert_eq!(nodes.next(), None);
    drop(nodes);
    assert!(physical_owner.upgrade().is_none());
    assert!(!first_token.is_live());
    assert!(!replacement_token.is_live());
    assert!(!fresh_token.is_live());
}

#[test]
pub(super) fn fresh_lend_allocations_survive_nested_collection_notifications() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let session = super::super::session::RSession::new_for_gc_tests();
    let notifications = Arc::new(AtomicUsize::new(0));
    let calls = notifications.clone();
    session.with_active(|| unsafe {
        super::super::instance::with_required_current_instance(|owner| {
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
        });
        super::super::gengc::register_gc_callback(Box::new(move |_| {
            if calls.fetch_add(1, Ordering::SeqCst) != 0 {
                return;
            }
            let identity = super::super::instance::with_required_current_instance(|owner| {
                (*owner).arena.heap_identity()
            });
            let original = super::fresh_allocation_roots(&identity);
            assert_eq!(original.len(), 4);
            super::super::gengc::full_gc();
            assert!(original.iter().all(|(_, token)| token.is_live()));
            let nested = super::with_arena(|arena| {
                let node = arena.alloc_vector(super::SEXPTYPE::REALSXP, 3);
                arena.node_token(node).unwrap()
            });
            assert!(nested.is_live());
            // The nested lend has returned, so its scope is gone while
            // the outer allocation-return scope remains rooted.
            super::super::gengc::full_gc();
            assert!(!nested.is_live());
            assert!(original.iter().all(|(_, token)| token.is_live()));
        }));
        let original = super::with_arena(|arena| {
            let nodes = [
                arena.alloc_node(super::SEXPTYPE::INTSXP),
                arena.alloc_vector(super::SEXPTYPE::REALSXP, 3),
                arena.alloc_charsxp(b"fresh allocation"),
                arena.alloc_node(super::SEXPTYPE::LISTSXP),
            ];
            nodes.map(|node| arena.node_token(node).unwrap())
        });
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        assert!(original.iter().all(super::CheckedNode::is_live));
        super::super::gengc::full_gc();
        assert!(original.iter().all(|token| !token.is_live()));
    });
}

#[test]
pub(super) fn fresh_lend_scope_is_removed_after_notification_panic() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let session = super::super::session::RSession::new_for_gc_tests();
    let panic_once = Arc::new(AtomicBool::new(true));
    session.with_active(|| unsafe {
        let identity = super::super::instance::with_required_current_instance(|owner| {
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
            (*owner).arena.heap_identity()
        });
        super::super::gengc::register_gc_callback(Box::new(move |_| {
            if panic_once.swap(false, Ordering::SeqCst) {
                panic!("injected fresh allocation notification panic");
            }
        }));
        let mut abandoned = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::with_arena(|arena| {
                let node = arena.alloc_vector(super::SEXPTYPE::INTSXP, 3);
                abandoned = arena.node_token(node);
                node
            })
        }));
        assert!(result.is_err());
        assert!(super::fresh_allocation_roots(&identity).is_empty());
        let abandoned = abandoned.unwrap();
        assert!(abandoned.is_live());
        super::super::gengc::full_gc();
        assert!(!abandoned.is_live());
    });
}

#[test]
pub(super) fn fresh_lend_roots_preserve_exact_generations_and_heap_domains() {
    let mut left = super::super::instance::RInstance::new_for_gc_tests();
    let mut right = super::super::instance::RInstance::new_for_gc_tests();
    let left_identity = left.arena.heap_identity();
    let right_identity = right.arena.heap_identity();
    unsafe {
        super::with_arena_in(&mut left, |arena| {
            let first = arena.alloc_node(super::SEXPTYPE::INTSXP);
            let old = arena.node_token(first).unwrap();
            arena.free_node(first);
            let replacement = arena.alloc_node(super::SEXPTYPE::REALSXP);
            assert_eq!(replacement, first);
            let current = arena.node_token(replacement).unwrap();
            let roots = super::fresh_allocation_roots(&left_identity);
            assert_eq!(roots.len(), 1);
            assert_eq!(roots[0].1, current);
            assert!(!old.is_live());
            assert!(super::fresh_allocation_roots(&right_identity).is_empty());
            super::with_arena_in(&mut right, |other| {
                let foreign = other.alloc_node(super::SEXPTYPE::SYMSXP);
                let foreign_token = other.node_token(foreign).unwrap();
                assert_eq!(
                    super::fresh_allocation_roots(&right_identity)[0].1,
                    foreign_token
                );
                assert_eq!(super::fresh_allocation_roots(&left_identity)[0].1, current);
            });
            assert!(super::fresh_allocation_roots(&right_identity).is_empty());
            assert_eq!(super::fresh_allocation_roots(&left_identity)[0].1, current);
        });
    }
    assert!(super::fresh_allocation_roots(&left_identity).is_empty());
    assert!(super::fresh_allocation_roots(&right_identity).is_empty());
}

#[test]
pub(super) fn fresh_lend_scope_is_removed_after_constructor_unwind() {
    let mut owner = super::super::instance::RInstance::new_for_gc_tests();
    let identity = owner.arena.heap_identity();
    let mut abandoned = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::with_arena_in(&mut owner, |arena| {
            let node = arena.alloc_charsxp(b"abandoned constructor");
            abandoned = arena.node_token(node);
            assert_eq!(super::fresh_allocation_roots(&identity).len(), 1);
            panic!("injected constructor unwind");
        });
    }));
    assert!(result.is_err());
    assert!(super::fresh_allocation_roots(&identity).is_empty());
    unsafe {
        super::super::gengc::full_gc_in(&mut owner);
    }
    assert!(!abandoned.unwrap().is_live());
}

#[test]
pub(super) fn standalone_arena_allocations_do_not_accumulate_temporary_roots() {
    let mut arena = super::RArena::new();
    let identity = arena.heap_identity();
    for _ in 0..3 {
        arena.alloc_node(super::SEXPTYPE::INTSXP);
    }
    assert!(super::fresh_allocation_roots(&identity).is_empty());
}

#[test]
pub(super) fn typed_scratch_storage_preserves_requested_layout_and_views_after_owner_moves() {
    for alignment in [1, 2, 4, 8] {
        let layout = std::alloc::Layout::from_size_align(17 * 8, alignment).unwrap();
        let buffer = super::OwnedBuffer::zeroed(layout).unwrap();
        let pointer = buffer.as_ptr();
        assert_eq!(pointer as usize % 8, 0);
        assert_eq!(buffer.layout(), layout);
        let mut owners = vec![buffer];
        owners.reserve(64);
        // SAFETY: scratch storage is initialized, at least eight-byte
        // aligned and owns the complete seventeen-u64 region. No byte or
        // typed view overlaps this temporary mutable view.
        unsafe {
            let values = std::slice::from_raw_parts_mut(pointer.cast::<u64>(), 17);
            assert!(values.iter().all(|value| *value == 0));
            for (index, value) in values.iter_mut().enumerate() {
                *value = index as u64 + 1;
            }
            assert_eq!(values[16], 17);
        }
        assert_eq!(owners[0].layout().size(), 136);
    }
    let before = super::buffer_allocation_attempts();
    assert!(
        super::OwnedBuffer::zeroed(std::alloc::Layout::from_size_align(0, 8).unwrap()).is_none()
    );
    assert!(
        super::OwnedBuffer::zeroed(std::alloc::Layout::from_size_align(16, 16).unwrap()).is_none()
    );
    assert_eq!(super::buffer_allocation_attempts(), before);
}

#[test]
pub(super) fn user_tls_owned_arena_survives_directory_shutdown_order() {
    struct LateArena {
        arena: super::RArena,
        pointer: super::SEXP,
        token: super::CheckedNode,
    }
    impl Drop for LateArena {
        fn drop(&mut self) {
            assert!(self.token.is_live());
            assert_eq!(self.arena.node_count(), 1);
            // The owning page is still live, but the already-destroyed
            // directory must return no projection instead of panicking.
            assert!(super::checked_projection(self.pointer).is_none());
            assert!(super::checked_snapshot(self.pointer, &self.token).is_none());
        }
    }
    thread_local! {
        static USER_ARENA: std::cell::RefCell<Option<LateArena>> = const { std::cell::RefCell::new(None) };
    }
    std::thread::spawn(|| {
        // Initialize the user's TLS destructor before the runtime's
        // directory TLS. The directory then shuts down first.
        USER_ARENA.with(|slot| {
            let mut arena = super::RArena::new();
            let pointer = arena.alloc_node(super::SEXPTYPE::INTSXP);
            assert!(super::checked_node(pointer).is_some());
            let token = arena.node_token(pointer).unwrap();
            *slot.borrow_mut() = Some(LateArena {
                arena,
                pointer,
                token,
            });
        });
    })
    .join()
    .expect("owned arena cleanup must tolerate destroyed directory TLS");
}

#[test]
pub(super) fn owned_header_snapshots_reject_stale_foreign_and_wrong_slot_tokens() {
    let mut arena = super::RArena::new();
    let first = arena.alloc_node(super::SEXPTYPE::INTSXP);
    let second = arena.alloc_node(super::SEXPTYPE::REALSXP);
    let first_id = arena.node_token(first).unwrap();
    let second_id = arena.node_token(second).unwrap();
    let input = std::ptr::without_provenance_mut::<super::SexprecCore>(first.addr());
    assert_eq!(
        super::checked_snapshot(input, &first_id)
            .unwrap()
            .sxpinfo
            .type_of(),
        super::SEXPTYPE::INTSXP
    );
    assert!(super::checked_snapshot(input, &second_id).is_none());
    let mut foreign = super::RArena::new();
    let other = foreign.alloc_node(super::SEXPTYPE::INTSXP);
    assert!(super::checked_snapshot(input, &foreign.node_token(other).unwrap()).is_none());
    // SAFETY: first has no reachable edge or payload loan. The snapshot
    // and expected metadata lease do not borrow its header storage.
    unsafe {
        arena.free_node(first);
    }
    let replacement = arena.alloc_node(super::SEXPTYPE::SYMSXP);
    assert_eq!(replacement, first);
    assert!(super::checked_snapshot(input, &first_id).is_none());
    let current = arena.node_token(replacement).unwrap();
    assert_eq!(
        super::checked_snapshot(input, &current)
            .unwrap()
            .sxpinfo
            .type_of(),
        super::SEXPTYPE::SYMSXP
    );
    drop(arena);
    assert!(super::checked_snapshot(input, &current).is_none());
}

#[test]
pub(super) fn prepared_payload_unwind_restores_ownership_and_budget_before_publication() {
    let session = super::super::session::RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let node = super::with_arena(|arena| arena.alloc_vector(super::SEXPTYPE::INTSXP, 0));
        let _root = super::super::protect::protect(node);
        (*node).sxpinfo.set_alt(true);
        (*node).data = super::NodeBody::Vector(super::super::ffi::Vecsxp {
            length: 3,
            truelength: 3,
            metadata: crate::sexp::ffi::VectorMetadata::None,
        });
        for lent in [false, true] {
            let initialize = || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    super::attach_initialized_data_buffer(node, 12, |_| {
                        panic!("injected initialization unwind")
                    });
                }));
                assert!(result.is_err());
                assert!((*node).payload.is_empty());
            };
            if lent {
                super::with_arena(|arena| {
                    let before = arena.allocated_bytes.get();
                    initialize();
                    assert_eq!(arena.allocated_bytes.get(), before);
                    assert_eq!(arena.transient_bytes.get(), 0);
                    assert!(
                        arena
                            .backing
                            .node_pages
                            .borrow()
                            .iter()
                            .all(|page| page.payloads.borrow().is_empty())
                    );
                });
            } else {
                initialize();
            }
        }
        let data = super::attach_initialized_data_buffer(node, 12, |data| {
            let values = std::slice::from_raw_parts_mut(data.cast::<i32>(), 3);
            values.copy_from_slice(&[4, 5, 6]);
        });
        assert_eq!(
            std::slice::from_raw_parts(data.cast::<i32>(), 3),
            &[4, 5, 6]
        );
        assert!(super::vector_payload_is_tracked(node));
    });
}

#[test]
pub(super) fn initialized_payload_cannot_attach_to_a_reused_header_generation() {
    let session = super::super::session::RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        super::with_arena(|arena| {
            let baseline = arena.total_bytes_allocated();
            let node = arena.alloc_vector(super::SEXPTYPE::INTSXP, 0);
            let allocated = arena.total_bytes_allocated();
            let old = arena.node_token(node).unwrap();
            let heap = old.heap_identity();
            let mut header = heap.node_snapshot(&old).unwrap();
            header.sxpinfo.set_alt(true);
            heap.replace_node(&old, header).unwrap();
            heap.publish_lazy_length(&old, 3).unwrap();
            let mut replacement = std::ptr::null_mut();
            let result = super::attach_initialized_payload(&old, |lease| {
                lease.set_integer_elt(0, 71)?;
                arena.free_node(node);
                replacement = arena.alloc_vector(super::SEXPTYPE::REALSXP, 0);
                Some(())
            });
            assert!(result.is_none());
            assert_eq!(replacement, node);
            assert!(!old.is_live());
            let replacement_node = arena.node_token(replacement).unwrap();
            assert!(
                heap.node_snapshot(&replacement_node)
                    .unwrap()
                    .payload
                    .is_empty()
            );
            assert!(heap.payload_lease(&replacement_node).is_none());
            assert_eq!(arena.transient_bytes.get(), 0);
            assert!(allocated >= baseline);
            assert_eq!(arena.allocated_bytes.get(), allocated);
        });
    });
}

#[test]
pub(super) fn typed_payload_projections_cover_chunk_boundaries_and_owner_moves() {
    for kind in [
        super::SEXPTYPE::RAWSXP,
        super::SEXPTYPE::INTSXP,
        super::SEXPTYPE::REALSXP,
        super::SEXPTYPE::CPLXSXP,
        super::SEXPTYPE::VECSXP,
    ] {
        let mut arena = super::RArena::new();
        let node = arena.alloc_vector(kind, 17);
        let data = unsafe { native_payload(node) as *mut u8 };
        assert_eq!(data as usize % 8, 0);
        let mut owners = vec![arena];
        owners.reserve(64);
        // SAFETY: moved Rc owners retain the entire initialized typed
        // allocation. Every access is within the seventeen logical
        // elements, and no competing payload borrow is held.
        unsafe {
            match kind {
                super::SEXPTYPE::RAWSXP => {
                    let values = std::slice::from_raw_parts_mut(data, 17);
                    for (index, value) in values.iter_mut().enumerate() {
                        *value = index as u8;
                    }
                    assert_eq!(values[16], 16);
                }
                super::SEXPTYPE::INTSXP => {
                    let values = std::slice::from_raw_parts_mut(data.cast::<i32>(), 17);
                    for (index, value) in values.iter_mut().enumerate() {
                        *value = index as i32;
                    }
                    assert_eq!(values[16], 16);
                }
                super::SEXPTYPE::REALSXP => {
                    let values = std::slice::from_raw_parts_mut(data.cast::<f64>(), 17);
                    for (index, value) in values.iter_mut().enumerate() {
                        *value = index as f64;
                    }
                    assert_eq!(values[16], 16.0);
                }
                super::SEXPTYPE::CPLXSXP => {
                    let values = std::slice::from_raw_parts_mut(data.cast::<super::Rcomplex>(), 17);
                    for (index, value) in values.iter_mut().enumerate() {
                        value.r = index as f64;
                        value.i = -(index as f64);
                    }
                    assert_eq!(values[16], super::Rcomplex { r: 16.0, i: -16.0 });
                }
                super::SEXPTYPE::VECSXP => {
                    let arena = &owners[0];
                    let token = arena.node_token(node).unwrap();
                    let heap = arena.heap_identity();
                    assert!(
                        heap.reference_links(&token)
                            .unwrap()
                            .iter()
                            .all(|link| link.is_null())
                    );
                    heap.set_reference_elt(
                        &token,
                        16,
                        super::super::heap::ReferenceChild::Node(&token),
                    )
                    .unwrap();
                    assert_eq!(heap.reference_elt(&token, 16), Some(node));
                }
                _ => unreachable!(),
            }
        }
        assert!(owners[0].tracks_altrep_test_buffer(data));
    }
}

#[test]
pub(super) fn all_header_allocations_reuse_slots_and_charge_only_payload_growth() {
    let mut arena = super::RArena::with_budget(super::ArenaBudget::new(super::NODE_BYTES + 16, 1));
    let first = arena.alloc_node(super::SEXPTYPE::LISTSXP);
    let old = arena.node_token(first).unwrap();
    // SAFETY: these unrooted fixture allocations have no external graph
    // edge or payload borrow; checked metadata leases can become stale.
    unsafe {
        arena.free_node(first);
    }
    for checked in [false, true] {
        let vector = if checked {
            arena
                .alloc_vector_checked(super::SEXPTYPE::INTSXP, 3)
                .unwrap()
        } else {
            arena.alloc_vector(super::SEXPTYPE::INTSXP, 3)
        };
        assert_eq!(vector, first);
        assert_eq!(arena.allocated_bytes.get(), super::NODE_BYTES + 12);
        assert!(!old.is_live());
        unsafe {
            arena.free_node(vector);
        }
        assert_eq!(arena.allocated_bytes.get(), super::NODE_BYTES);
    }
    let chars = arena.alloc_charsxp(b"abcdefghijklmno");
    assert_eq!(chars, first);
    assert_eq!(arena.allocated_bytes.get(), super::NODE_BYTES + 16);
    unsafe {
        arena.free_node(chars);
    }
    let zero = arena.alloc_vector(super::SEXPTYPE::REALSXP, 0);
    assert_eq!(zero, first);
    assert_eq!(arena.allocated_bytes.get(), super::NODE_BYTES);
    unsafe {
        arena.free_node(zero);
    }
    let boxed = Box::new(super::SexprecCore::new(super::SEXPTYPE::SYMSXP));
    assert_eq!(unsafe { arena.add_node(boxed) }, first);
    assert_eq!(arena.node_count(), 1);
    assert_eq!(arena.backing.node_pages.borrow().len(), 1);
    assert_eq!(arena.slab_offset, 1);
}
