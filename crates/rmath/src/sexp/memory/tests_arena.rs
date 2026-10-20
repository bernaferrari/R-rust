//! Arena allocation/GC tests extracted from the former inline `mod tests`.

use super::tests_payload::native_payload;
use super::*;

#[test]
pub(super) fn checked_projection_recovers_canonical_provenance_from_address_only_inputs() {
    let mut arena = super::RArena::new();
    let first = arena.alloc_node(super::SEXPTYPE::INTSXP);
    let second = arena.alloc_node(super::SEXPTYPE::REALSXP);
    for expected in [first, second] {
        let input = std::ptr::without_provenance_mut::<super::SexprecCore>(expected.addr());
        let (canonical, token) = super::checked_projection(input).unwrap();
        assert_eq!(canonical, expected);
        assert!(token.is_live());
        assert_eq!(arena.node_projection(input).unwrap().0, expected);
        // SAFETY: checked_projection re-derives the original live Cell
        // pointer; the address-only input itself is never dereferenced.
        unsafe {
            assert!(!(*canonical).sxpinfo.mark());
            (*canonical).sxpinfo.set_mark(true);
            assert!((*canonical).sxpinfo.mark());
            // A genuine previously projected alias remains valid when
            // the directory derives another pointer to the same Cell.
            assert!((*expected).sxpinfo.mark());
        }
    }
    let input = std::ptr::without_provenance_mut::<super::SexprecCore>(first.addr());
    // SAFETY: the fixture owns first, and no header/payload borrow remains.
    unsafe {
        arena.free_node(first);
    }
    assert!(super::checked_projection(input).is_none());
    drop(arena);
    assert!(super::checked_projection(input).is_none());
}

#[test]
pub(super) fn legacy_node_projection_survives_owned_page_moves() {
    let page = super::NodePage::try_new(super::HeapIdentity::new(), 0, 2, || {
        super::SexprecCore::new(super::SEXPTYPE::NILSXP)
    })
    .unwrap();
    let pointer = page
        .replace_inactive(1, super::SexprecCore::new(super::SEXPTYPE::INTSXP))
        .unwrap();
    page.metadata().activate(1, false).unwrap();
    let token = page.token(1).unwrap();
    let mut pages = Vec::new();
    pages.push(page);
    // SAFETY: the live Cell header belongs to pages; moving its owned
    // Rc handle must preserve the original projection's access rights.
    unsafe {
        assert_eq!((*pointer).sxpinfo.type_of(), super::SEXPTYPE::INTSXP);
    }
    pages.reserve(64);
    assert!(token.is_live());
    // SAFETY: reserve moved the page handles, and no header borrow is held.
    unsafe {
        (*pointer).sxpinfo.set_mark(true);
        assert!((*pointer).sxpinfo.mark());
    }
}

#[test]
pub(super) fn node_directory_accepts_only_exact_slots_and_retires_page_tokens() {
    let page = super::NodePage::try_new(super::HeapIdentity::new(), 0, 3, || {
        super::SexprecCore::new(super::SEXPTYPE::NILSXP)
    })
    .unwrap();
    let registration = super::register_node_page(&page);
    let metadata = page.metadata();
    for slot in 0..3 {
        metadata.activate(slot, false).unwrap();
    }
    let base = page.raw_slot(0).unwrap();
    let last = page.raw_slot(2).unwrap();
    assert!(super::checked_node(base).is_some());
    assert!(super::checked_node(last).is_some());
    let interior = std::ptr::without_provenance_mut(base.addr() + 1);
    let past_end = std::ptr::without_provenance_mut(base.addr() + 3 * super::NODE_BYTES);
    assert!(super::checked_node(interior).is_none());
    assert!(super::checked_node(past_end).is_none());
    // Alternate between actual registered pages, exceeding the cache's
    // working set. Every projection must still resolve the original Cell.
    let mut neighbors = Vec::new();
    for index in 1..12 {
        let neighbor = super::NodePage::try_new(super::HeapIdentity::new(), index, 2, || {
            super::SexprecCore::new(super::SEXPTYPE::NILSXP)
        })
        .unwrap();
        let registration = super::register_node_page(&neighbor);
        neighbor.metadata().activate(1, false).unwrap();
        neighbors.push((neighbor, registration));
    }
    for _ in 0..2 {
        for (neighbor, _) in &neighbors {
            let pointer = neighbor.raw_slot(1).unwrap();
            assert_eq!(
                super::checked_projection(pointer).map(|(raw, _)| raw),
                Some(pointer)
            );
            assert_eq!(
                super::checked_projection(last).map(|(raw, _)| raw),
                Some(last)
            );
        }
    }
    let token = super::checked_node(last).unwrap();
    drop(page);
    assert!(!token.is_live());
    assert!(super::checked_node(last).is_none());
    drop(registration);
    assert!(super::find_slab_slot(base).is_none());
    for (neighbor, _) in &neighbors {
        let pointer = neighbor.raw_slot(1).unwrap();
        assert_eq!(
            super::checked_projection(pointer).map(|(raw, _)| raw),
            Some(pointer)
        );
    }
    // A registration can retire while the physical page is still alive.
    // It must disappear from every cache position, including ranges other
    // than the most recently touched one.
    let (retired_page, retired_registration) = neighbors.pop().unwrap();
    let retired_pointer = retired_page.raw_slot(1).unwrap();
    assert!(super::checked_node(retired_pointer).is_some());
    for (neighbor, _) in neighbors.iter().rev().take(3) {
        assert!(super::checked_node(neighbor.raw_slot(1).unwrap()).is_some());
    }
    drop(retired_registration);
    assert!(retired_page.token(1).unwrap().is_live());
    assert!(super::checked_node(retired_pointer).is_none());
    assert!(super::checked_projection(retired_pointer).is_none());
}

#[test]
pub(super) fn production_arena_identities_reject_reuse_foreign_arenas_and_drop() {
    let mut arena = super::RArena::new();
    let pointer = arena.alloc_node(super::SEXPTYPE::INTSXP);
    let old = arena.node_token(pointer).unwrap();
    let old_id = old.id().clone();
    assert_eq!(arena.resolve_node(&old_id), Some(pointer));
    let foreign = super::RArena::new();
    assert_eq!(foreign.resolve_node(&old_id), None);
    assert!(foreign.node_token(pointer).is_none());
    // SAFETY: this fixture owns the live node and holds no header/payload
    // reference. Checked metadata leases do not borrow its storage.
    unsafe {
        arena.free_node(pointer);
    }
    assert!(!old.is_live());
    assert_eq!(arena.resolve_node(&old_id), None);
    let replacement = arena.alloc_node(super::SEXPTYPE::REALSXP);
    assert_eq!(replacement, pointer);
    let new = arena.node_token(replacement).unwrap();
    assert_ne!(old, new);
    assert!(new.is_live());
    assert_eq!(arena.resolve_node(&old_id), None);
    drop(arena);
    assert!(!new.is_live());
    assert!(super::checked_node(pointer).is_none());
}

#[test]
pub(super) fn vector_allocation_validates_platform_length_and_layout_without_consuming_budget() {
    let mut arena = super::RArena::new();
    let before = (arena.node_count(), arena.allocated_bytes.get());
    for kind in [super::SEXPTYPE::RAWSXP, super::SEXPTYPE::REALSXP] {
        for length in [-1, i64::MAX] {
            assert!(arena.alloc_vector_sexp(kind, length).is_none());
            assert!(matches!(
                arena.alloc_vector_checked_sexp(kind, length),
                Err(super::ArenaError::InvalidLength)
            ));
            assert_eq!((arena.node_count(), arena.allocated_bytes.get()), before);
        }
    }
    #[cfg(target_pointer_width = "32")]
    for length in [1_i64 << 32, (1_i64 << 32) + 1] {
        assert!(
            arena
                .alloc_vector_sexp(super::SEXPTYPE::RAWSXP, length)
                .is_none()
        );
        assert!(matches!(
            arena.alloc_vector_checked_sexp(super::SEXPTYPE::RAWSXP, length),
            Err(super::ArenaError::InvalidLength)
        ));
        assert_eq!((arena.node_count(), arena.allocated_bytes.get()), before);
    }
}

#[test]
pub(super) fn vector_allocation_preserves_all_supported_types_and_zero_lengths() {
    let mut arena = super::RArena::new();
    for tag in 0..=31 {
        let kind = super::SEXPTYPE::from(tag);
        if super::sexp_elem_size(kind) == 0 {
            continue;
        }
        for length in [0, 1, 8] {
            for checked in [false, true] {
                let value = if checked {
                    arena.alloc_vector_checked_sexp(kind, length).unwrap()
                } else {
                    arena.alloc_vector_sexp(kind, length).unwrap()
                };
                assert_eq!(value.typeof_(), kind);
                assert_eq!(value.len(), length);
            }
        }
    }
}

#[test]
pub(super) fn vector_allocation_rejects_incompatible_headers_without_consuming_budget() {
    let mut arena = super::RArena::new();
    let before = (arena.node_count(), arena.allocated_bytes.get());
    for tag in -1..=100 {
        let kind = super::SEXPTYPE::from(tag);
        if super::sexp_elem_size(kind) != 0 {
            continue;
        }
        for length in [0, 1, 8] {
            assert!(arena.alloc_vector_sexp(kind, length).is_none(), "tag {tag}");
            assert!(
                arena.alloc_vector_checked_sexp(kind, length).is_err(),
                "tag {tag}"
            );
            assert_eq!((arena.node_count(), arena.allocated_bytes.get()), before);
        }
    }
}

#[test]
pub(super) fn transient_reservations_share_limits_and_release_without_arena_borrows() {
    let mut arena = super::RArena::with_budget(super::ArenaBudget::new(1024, 0));
    let reservation = arena.try_reserve_transient(1024).unwrap();
    assert!(arena.try_reserve_transient(1).is_none());
    assert!(!arena.can_grow_bytes_by(1));
    assert!(
        arena
            .alloc_vector_checked(super::SEXPTYPE::REALSXP, 1)
            .is_err()
    );
    drop(reservation);
    assert!(arena.can_grow_bytes_by(1024));
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _reservation = arena.try_reserve_transient(1024).unwrap();
        panic!("workspace failure");
    }));
    assert!(unwound.is_err());
    assert!(arena.try_reserve_transient(1024).is_some());
    // Safe guards must remain safe if their arena moves or is destroyed.
    let reservation = arena.try_reserve_transient(128).unwrap();
    let moved = Box::new(arena);
    drop(moved);
    drop(reservation);
}
use crate::sexp::ffi::*;

#[test]
pub(super) fn nested_arena_lend_is_rejected_and_outer_borrow_survives() {
    let _session = crate::sexp::session::RSession::new_for_gc_tests();
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        with_arena(|outer| {
            let before = outer.node_count();
            let failure = std::panic::catch_unwind(|| with_arena(|_| ()));
            assert!(failure.is_err());
            outer.alloc_node(SEXPTYPE::LISTSXP);
            assert_eq!(outer.node_count(), before + 1);
        })
    });
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        with_arena(|arena| {
            arena.alloc_node(SEXPTYPE::LISTSXP);
        })
    });
}

#[test]
pub(super) fn test_arena_alloc_node() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
    assert!(!ptr.is_null());
    unsafe {
        assert_eq!((*ptr).sxpinfo.type_of(), SEXPTYPE::INTSXP);
    }
    assert_eq!(arena.node_count(), 1);
}

#[test]
pub(super) fn growth_warrants_gc_uses_delta_since_last_collection() {
    let mut arena = RArena::new();
    for _ in 0..=GC_TRIGGER_THRESHOLD {
        assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
    }
    assert!(arena.growth_warrants_gc());
    arena.note_gc_completed();
    assert!(!arena.growth_warrants_gc());
    for _ in 0..=GC_TRIGGER_THRESHOLD {
        assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
    }
    assert!(arena.growth_warrants_gc());
}

#[test]
pub(super) fn large_retained_graph_amortizes_node_collection_without_weakening_byte_trigger() {
    let mut arena = RArena::new();
    for _ in 0..80_000 {
        assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
    }
    arena.note_gc_completed();
    for _ in 0..15_000 {
        assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
    }
    assert!(!arena.growth_warrants_gc());
    for _ in 0..10_000 {
        assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
    }
    assert!(arena.growth_warrants_gc());
    arena.note_gc_completed();
    assert!(!arena.growth_warrants_gc());
    assert!(
        !arena
            .alloc_vector(SEXPTYPE::RAWSXP, (GC_BYTE_THRESHOLD + 1) as i64)
            .is_null()
    );
    assert!(arena.growth_warrants_gc());
}

#[test]
pub(super) fn near_node_budget_collects_before_the_amortized_growth_trigger() {
    let mut arena = RArena::with_budget(ArenaBudget::new(0, 100_000));
    for _ in 0..85_000 {
        assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
    }
    arena.note_gc_completed();
    assert!(!arena.growth_warrants_gc());
    for _ in 0..8_000 {
        assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
    }
    assert!(arena.growth_warrants_gc());
    assert_eq!(arena.budget.max_nodes, 100_000);
}

#[test]
pub(super) fn test_arena_alloc_vector_real() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 5);
    assert!(!ptr.is_null());
    unsafe {
        assert_eq!((*ptr).sxpinfo.type_of(), SEXPTYPE::REALSXP);
        assert_eq!((*ptr).vecsxp_length(), 5);
    }
}

#[test]
pub(super) fn charsxp_header_initializes_length_and_truelength() {
    let mut arena = RArena::new();
    let value = arena.alloc_charsxp(b"header");
    unsafe {
        assert_eq!((*value).vecsxp_length(), 6);
        assert_eq!((*value).vecsxp_truelength(), 0);
    }
}

#[test]
pub(super) fn test_arena_alloc_vector_int() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
    assert!(!ptr.is_null());
    unsafe {
        assert_eq!((*ptr).vecsxp_length(), 3);
        let data = native_payload(ptr) as *mut i32;
        assert_eq!(*data.add(0), 0);
        assert_eq!(*data.add(1), 0);
        assert_eq!(*data.add(2), 0);
    }
}

#[test]
pub(super) fn test_arena_sexp_wraps_only_owned_active_nodes() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
    assert!(arena.sexp(ptr).is_some());

    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(ptr)
    });
    assert!(arena.sexp(ptr).is_none());
}

#[test]
pub(super) fn test_arena_sexp_rejects_foreign_nodes() {
    let mut owner = RArena::new();
    let foreign = RArena::new();
    let ptr = owner.alloc_node(SEXPTYPE::INTSXP);

    assert!(owner.sexp(ptr).is_some());
    assert!(foreign.sexp(ptr).is_none());
}

#[test]
pub(super) fn test_arena_alloc_vector_empty() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 0);
    assert!(!ptr.is_null());
    unsafe {
        assert_eq!((*ptr).vecsxp_length(), 0);
    }
}

#[test]
pub(super) fn test_arena_alloc_vector_negative_length() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, -1);
    assert!(ptr.is_null());
}

#[test]
pub(super) fn test_arena_alloc_charsxp() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_charsxp(b"hello");
    assert!(!ptr.is_null());
    unsafe {
        assert_eq!((*ptr).sxpinfo.type_of(), SEXPTYPE::CHARSXP);
        let data = native_payload(ptr) as *const u8;
        let s = std::ffi::CStr::from_ptr(data as *const core::ffi::c_char);
        assert_eq!(s.to_str().unwrap_or(""), "hello");
    }
}

#[test]
pub(super) fn test_arena_alloc_charsxp_empty() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_charsxp(b"");
    assert!(!ptr.is_null());
    unsafe {
        let data = native_payload(ptr) as *const u8;
        let s = std::ffi::CStr::from_ptr(data as *const core::ffi::c_char);
        assert_eq!(s.to_str().unwrap_or(""), "");
    }
}

#[test]
pub(super) fn test_arena_cons() {
    let mut arena = RArena::new();
    let car = arena.alloc_node(SEXPTYPE::INTSXP);
    let cdr = arena.alloc_node(SEXPTYPE::REALSXP);
    let cell = unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.cons(car, cdr, ptr::null_mut())
    };
    assert!(!cell.is_null());
    unsafe {
        assert_eq!((*cell).sxpinfo.type_of(), SEXPTYPE::LISTSXP);
        assert_eq!(
            arena
                .heap_identity()
                .projection_of_link((*cell).data.list().carval),
            Some(car)
        );
        assert_eq!(
            arena
                .heap_identity()
                .projection_of_link((*cell).data.list().cdrval),
            Some(cdr)
        );
        assert!((*cell).data.list().tagval.is_null());
    }
}

#[test]
pub(super) fn test_arena_alloc_list_chain() {
    let mut arena = RArena::new();
    let list = arena.alloc_list_chain(3);
    assert!(!list.is_null());
    unsafe {
        assert_eq!((*list).sxpinfo.type_of(), SEXPTYPE::LISTSXP);
        assert!((*list).data.list().carval.is_null());
        let cdr1 = arena
            .heap_identity()
            .projection_of_link((*list).data.list().cdrval)
            .unwrap();
        assert!(!cdr1.is_null());
        let cdr2 = arena
            .heap_identity()
            .projection_of_link((*cdr1).data.list().cdrval)
            .unwrap();
        assert!(!cdr2.is_null());
        assert_eq!(
            arena
                .heap_identity()
                .projection_of_link((*cdr2).data.list().cdrval)
                .unwrap(),
            crate::sexp::globals::R_NilValue()
        );
    }
}

#[test]
pub(super) fn test_arena_alloc_list_chain_zero() {
    let mut arena = RArena::new();
    let list = arena.alloc_list_chain(0);
    assert_eq!(list, unsafe { crate::sexp::globals::R_NilValue() });
}

#[test]
pub(super) fn test_arena_alloc_list_chain_negative() {
    let mut arena = RArena::new();
    let list = arena.alloc_list_chain(-1);
    assert_eq!(list, unsafe { crate::sexp::globals::R_NilValue() });
}

#[test]
pub(super) fn test_arena_drop() {
    let mut arena = RArena::new();
    arena.alloc_vector(SEXPTYPE::REALSXP, 100);
    arena.alloc_charsxp(b"test string");
    arena.alloc_node(SEXPTYPE::INTSXP);
    drop(arena);
}

#[test]
pub(super) fn test_sexp_elem_size() {
    assert_eq!(sexp_elem_size(SEXPTYPE::LGLSXP), 4);
    assert_eq!(sexp_elem_size(SEXPTYPE::INTSXP), 4);
    assert_eq!(sexp_elem_size(SEXPTYPE::REALSXP), 8);
    assert_eq!(sexp_elem_size(SEXPTYPE::CPLXSXP), 16);
    assert_eq!(sexp_elem_size(SEXPTYPE::RAWSXP), 1);
    assert_eq!(
        sexp_elem_size(SEXPTYPE::STRSXP),
        std::mem::size_of::<super::NodeLink>()
    );
    assert_eq!(
        sexp_elem_size(SEXPTYPE::VECSXP),
        std::mem::size_of::<super::NodeLink>()
    );
    assert_eq!(sexp_elem_size(SEXPTYPE::NILSXP), 0);
    assert_eq!(sexp_elem_size(SEXPTYPE::SYMSXP), 0);
}

#[test]
pub(super) fn test_arena_write_read_real() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
    unsafe {
        let data = native_payload(ptr) as *mut f64;
        *data.add(0) = 1.5;
        *data.add(1) = 2.5;
        *data.add(2) = 3.5;
        assert_eq!(*data.add(0), 1.5);
        assert_eq!(*data.add(1), 2.5);
        assert_eq!(*data.add(2), 3.5);
    }
}

#[test]
pub(super) fn test_arena_write_read_int() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
    unsafe {
        let data = native_payload(ptr) as *mut i32;
        *data.add(0) = 42;
        *data.add(1) = -1;
        *data.add(2) = NA_INTEGER;
        assert_eq!(*data.add(0), 42);
        assert_eq!(*data.add(1), -1);
        assert_eq!(*data.add(2), NA_INTEGER);
    }
}

#[test]
pub(super) fn test_arena_free_node() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
    assert_eq!(arena.node_count(), 1);
    assert_eq!(arena.free_count(), 0);
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(ptr)
    });
    assert_eq!(arena.node_count(), 0);
    assert_eq!(arena.free_count(), 1);
}

#[test]
pub(super) fn test_arena_free_node_idempotent() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(ptr)
    });
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(ptr)
    });
    assert_eq!(arena.free_count(), 1);
}

#[test]
pub(super) fn test_arena_normalize_free_list_preserves_canonical_membership() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(ptr)
    });
    // Duplicate retirement cannot add another logical worklist entry.
    unsafe {
        arena.free_node(ptr);
    }

    arena.normalize_free_list();

    assert_eq!(arena.free_count(), 1);
    assert_eq!(arena.reclaimed_snapshot(), vec![ptr]);
    assert!(!arena.contains(ptr));
}

#[test]
pub(super) fn test_arena_free_node_null() {
    let mut arena = RArena::new();
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(ptr::null_mut())
    });
    assert_eq!(arena.free_count(), 0);
}

#[test]
pub(super) fn test_arena_free_node_reuse() {
    let mut arena = RArena::new();
    let ptr1 = arena.alloc_node(SEXPTYPE::INTSXP);
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(ptr1)
    });
    let ptr2 = arena.alloc_node(SEXPTYPE::REALSXP);
    assert_eq!(ptr1, ptr2);
    assert_eq!(arena.node_count(), 1);
    assert_eq!(arena.free_count(), 0);
}

#[test]
pub(super) fn test_arena_active_nodes_excludes_free_list() {
    let mut arena = RArena::new();
    let live = arena.alloc_node(SEXPTYPE::INTSXP);
    let freed = arena.alloc_node(SEXPTYPE::REALSXP);
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(freed)
    });

    let active: Vec<SEXP> = arena.active_nodes().collect();
    assert_eq!(active, vec![live]);
}

pub(super) fn membership_scan(arena: &super::RArena, expected: &[SEXP]) -> Vec<SEXP> {
    let membership: std::collections::HashSet<usize> =
        expected.iter().map(|ptr| ptr.addr()).collect();
    arena
        .nodes()
        .filter(|ptr| membership.contains(&ptr.addr()))
        .collect()
}

#[test]
pub(super) fn active_nodes_bitmap_matches_membership_and_old_sweep_skips_young() {
    let mut arena = super::RArena::new();
    let a = arena.alloc_node(super::SEXPTYPE::INTSXP);
    let b = arena.alloc_node(super::SEXPTYPE::REALSXP);
    let c = arena.alloc_node(super::SEXPTYPE::LGLSXP);
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(b)
    });
    let reused = arena.alloc_node(super::SEXPTYPE::REALSXP);
    assert_eq!(reused, b);
    assert_eq!(arena.active_nodes().collect::<Vec<_>>(), vec![a, b, c]);
    assert_eq!(
        arena.active_nodes().collect::<Vec<_>>(),
        membership_scan(&arena, &[a, b, c])
    );

    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(a)
    });
    assert_eq!(arena.active_nodes().collect::<Vec<_>>(), vec![b, c]);
    assert!(arena.old_nodes().next().is_none());

    unsafe {
        (*c).sxpinfo.set_gcgen(1);
    }
    assert_eq!(arena.old_nodes().collect::<Vec<_>>(), vec![c]);
    unsafe {
        (*b).sxpinfo.set_gcgen(1);
        (*c).sxpinfo.set_gcgen(0);
    }
    // `b` was allocated before `c`, so the old set stays in slab order.
    assert_eq!(arena.old_nodes().collect::<Vec<_>>(), vec![b]);

    let mut arena = super::RArena::new();
    let span = if cfg!(miri) {
        8
    } else {
        super::NODE_PAGE_SIZE + 3
    };
    let mut ptrs = Vec::with_capacity(span);
    for _ in 0..span {
        ptrs.push(arena.alloc_node(super::SEXPTYPE::INTSXP));
    }
    let mut freed = vec![0usize, 1];
    if span > super::NODE_PAGE_SIZE {
        freed.push(super::NODE_PAGE_SIZE);
    }
    freed.push(span - 1);
    for index in &freed {
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(ptrs[*index])
        });
    }
    let expect: Vec<SEXP> = ptrs
        .iter()
        .enumerate()
        .filter(|(index, _)| !freed.contains(&index))
        .map(|(_, ptr)| *ptr)
        .collect();
    let active = arena.active_nodes().collect::<Vec<_>>();
    assert_eq!(active, expect);
    assert_eq!(active, membership_scan(&arena, &expect));
    assert_eq!(active.len(), arena.node_count());

    unsafe {
        (*ptrs[2]).sxpinfo.set_gcgen(1);
        (*ptrs[3]).sxpinfo.set_gcgen(1);
    }
    super::begin_gc_epoch();
    assert_eq!(
        super::gc_touch(std::ptr::null_mut()),
        super::GcTouch::OutsideArena
    );
    let stray = super::SexprecCore::new(super::SEXPTYPE::INTSXP);
    assert_eq!(
        super::gc_touch(std::ptr::from_ref(&stray).cast_mut()),
        super::GcTouch::OutsideArena
    );
    assert_eq!(super::gc_touch(ptrs[2]), super::GcTouch::NewlyMarked);
    assert_eq!(super::gc_touch(ptrs[2]), super::GcTouch::AlreadyMarked);
    assert!(super::arena_node_marked(ptrs[2]));
    assert!(!super::arena_node_marked(ptrs[3]));
    let dead_old: Vec<SEXP> = arena
        .old_nodes()
        .filter(|ptr| !super::arena_node_marked(*ptr))
        .collect();
    assert_eq!(dead_old, vec![ptrs[3]]);
    assert!(arena.old_nodes().count() < arena.active_nodes().count());

    if cfg!(miri) {
        return;
    }

    let mut arena = super::RArena::new();
    let total = super::NODE_PAGE_SIZE * 4;
    let mut live = Vec::with_capacity(total);
    for index in 0..total {
        let ptr = arena.alloc_node(super::SEXPTYPE::INTSXP);
        if index % 64 == 0 {
            unsafe { (*ptr).sxpinfo.set_gcgen(1) };
        }
        live.push(ptr);
    }
    // Stand-in for the reachable graph: much smaller than the slab once
    // young garbage has piled up, which is the long torture run.
    let reachable: Vec<SEXP> = live.iter().copied().step_by(8).collect();
    let membership: std::collections::HashSet<usize> = live.iter().map(|ptr| ptr.addr()).collect();
    let iters = 12u32;
    let mut hash_ns = 0u128;
    let mut bitmap_ns = 0u128;
    let mut old_ns = 0u128;
    for _ in 0..iters {
        let started = std::time::Instant::now();
        let mut seen = 0usize;
        for ptr in arena.nodes() {
            if membership.contains(&(ptr as usize)) {
                seen += 1;
                std::hint::black_box(ptr);
            }
        }
        hash_ns += started.elapsed().as_nanos();
        std::hint::black_box(seen);

        let started = std::time::Instant::now();
        let mut seen = 0usize;
        for ptr in arena.active_nodes() {
            seen += 1;
            std::hint::black_box(ptr);
        }
        bitmap_ns += started.elapsed().as_nanos();
        std::hint::black_box(seen);

        let started = std::time::Instant::now();
        super::begin_gc_epoch();
        for ptr in &reachable {
            std::hint::black_box(super::gc_touch(*ptr));
        }
        let mut dead_old = 0usize;
        for ptr in arena.old_nodes() {
            if !super::arena_node_marked(ptr) {
                dead_old += 1;
            }
            std::hint::black_box(ptr);
        }
        old_ns += started.elapsed().as_nanos();
        std::hint::black_box(dead_old);
    }
    let hash_per = hash_ns / u128::from(iters);
    let bitmap_per = bitmap_ns / u128::from(iters);
    let old_per = old_ns / u128::from(iters);
    eprintln!(
        "active_nodes scan: membership {hash_per}ns, bitmap {bitmap_per}ns ({:.1}x), old-generation sweep {old_per}ns ({:.1}x vs membership)",
        hash_per as f64 / bitmap_per.max(1) as f64,
        hash_per as f64 / old_per.max(1) as f64
    );
}

#[test]
pub(super) fn test_arena_free_vector_clears_payload_pointer() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 2);
    assert!(!ptr.is_null());
    unsafe {
        assert!(!(*ptr).payload.is_empty());
    }
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(ptr)
    });
    unsafe {
        assert!((*ptr).payload.is_empty());
    }
}

#[test]
pub(super) fn test_arena_free_expression_vector_uses_tracked_layout() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_vector(SEXPTYPE::EXPRSXP, 2);
    assert!(!ptr.is_null());
    unsafe {
        assert!(!(*ptr).payload.is_empty());
    }
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(ptr)
    });
    assert_eq!(arena.free_count(), 1);
    unsafe {
        assert!((*ptr).payload.is_empty());
    }
}

#[test]
pub(super) fn test_arena_fragmentation_ratio() {
    let mut arena = RArena::new();
    assert_eq!(arena.fragmentation_ratio(), 0.0);
    let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
    assert_eq!(arena.fragmentation_ratio(), 0.0);
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(ptr)
    });
    assert_eq!(arena.fragmentation_ratio(), 1.0);
}

#[test]
pub(super) fn test_arena_total_bytes() {
    let mut arena = RArena::new();
    assert_eq!(arena.total_bytes_allocated(), 0);
    arena.alloc_node(SEXPTYPE::INTSXP);
    assert!(arena.total_bytes_allocated() > 0);
}

#[test]
pub(super) fn test_vector_and_charsxp_account_node_once() {
    let mut arena = RArena::new();
    assert!(!arena.alloc_vector(SEXPTYPE::REALSXP, 1).is_null());
    assert_eq!(
        arena.total_bytes_allocated(),
        super::tests_physical_headers::retained_page_bytes(&arena) + std::mem::size_of::<f64>()
    );

    let mut arena = RArena::new();
    assert!(!arena.alloc_charsxp(b"abc").is_null());
    assert_eq!(
        arena.total_bytes_allocated(),
        super::tests_physical_headers::retained_page_bytes(&arena) + 8
    );
}

#[test]
pub(super) fn test_arena_node_budget_applies_to_raw_alloc_node() {
    let mut arena = RArena::with_budget(ArenaBudget::new(0, 1));
    let first = arena.alloc_node(SEXPTYPE::INTSXP);
    assert!(!first.is_null());
    let second = arena.alloc_node(SEXPTYPE::REALSXP);
    assert!(second.is_null());

    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(first)
    });
    let reused = arena.alloc_node(SEXPTYPE::REALSXP);
    assert_eq!(reused, first);
}

#[test]
pub(super) fn test_arena_byte_budget_applies_to_raw_allocations() {
    let page_bytes = super::tests_physical_headers::minimum_page_bytes();

    let mut arena = RArena::with_budget(ArenaBudget::new(page_bytes, 0));
    let node = arena.alloc_node(SEXPTYPE::INTSXP);
    assert!(!node.is_null());
    assert!(arena.alloc_charsxp(b"x").is_null());
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.free_node(node)
    });
    assert_eq!(arena.alloc_node(SEXPTYPE::REALSXP), node);

    let mut arena = RArena::with_budget(ArenaBudget::new(page_bytes + 8, 0));
    assert!(!arena.alloc_vector(SEXPTYPE::REALSXP, 1).is_null());
    assert!(arena.alloc_vector(SEXPTYPE::REALSXP, 1).is_null());
}

#[test]
pub(super) fn test_arena_default() {
    let arena = RArena::default();
    assert_eq!(arena.node_count(), 0);
    assert_eq!(arena.free_count(), 0);
}

#[test]
pub(super) fn test_arena_add_node() {
    let mut arena = RArena::new();
    let boxed = Box::new(SexprecCore::new(SEXPTYPE::INTSXP));
    let ptr = unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        arena.add_node(boxed)
    };
    assert!(!ptr.is_null());
    assert_eq!(arena.node_count(), 1);
}

#[test]
pub(super) fn test_arena_add_node_obeys_budget() {
    let page_bytes = super::tests_physical_headers::minimum_page_bytes();
    let mut arena = RArena::with_budget(ArenaBudget::new(page_bytes, 1));
    assert!(
        !(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ arena
            .add_node(Box::new(SexprecCore::new(SEXPTYPE::INTSXP))) })
            .is_null()
    );
    assert!(
        (unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ arena
            .add_node(Box::new(SexprecCore::new(SEXPTYPE::REALSXP))) })
            .is_null()
    );
}

#[test]
pub(super) fn test_arena_can_target_instance_explicitly() {
    let mut left = super::super::instance::RInstance::new();
    let mut right = super::super::instance::RInstance::new();
    let left_before = unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        with_arena_in(&mut left, |arena| arena.node_count())
    };
    let right_before = unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        with_arena_in(&mut right, |arena| arena.node_count())
    };

    let left_node = unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        with_arena_in(&mut left, |arena| arena.alloc_node(SEXPTYPE::INTSXP))
    };
    assert!(!left_node.is_null());
    assert_eq!(
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena_in(&mut left, |arena| arena.node_count())
        }),
        left_before + 1
    );
    assert_eq!(
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena_in(&mut right, |arena| arena.node_count())
        }),
        right_before
    );

    let right_node = unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        with_arena_in(&mut right, |arena| arena.alloc_node(SEXPTYPE::REALSXP))
    };
    assert!(!right_node.is_null());
    assert_eq!(
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena_in(&mut left, |arena| arena.node_count())
        }),
        left_before + 1
    );
    assert_eq!(
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena_in(&mut right, |arena| arena.node_count())
        }),
        right_before + 1
    );

    // No checked handles or payload borrows remain in this raw fixture.
    left.arena = RArena::new();
    assert_eq!(
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena_in(&mut left, |arena| arena.node_count())
        }),
        0
    );
    assert_eq!(
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena_in(&mut right, |arena| arena.node_count())
        }),
        right_before + 1
    );
}

#[test]
pub(super) fn test_ambient_arena_borrow_depth_resets_after_panic() {
    let _session = crate::sexp::session::RSession::new_for_gc_tests();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|_| panic!("intentional arena borrow panic"))
        });
    }));

    assert!(result.is_err());
    assert_eq!(super::super::instance::instance_borrow_depth(), 0);
}
