use super::*;

#[test]
fn character_cache_reuses_live_ascii_without_a_second_header_or_payload() {
    let mut arena = RArena::with_budget(ArenaBudget::new(NODE_BYTES + 7, 1));
    let original = arena.alloc_charsxp(b"repeat");
    assert!(!original.is_null());
    let bytes = arena.total_bytes_allocated();
    let repeated = arena.alloc_charsxp(b"repeat");
    assert_eq!(repeated, original);
    assert_eq!(arena.node_count(), 1);
    assert_eq!(arena.total_bytes_allocated(), bytes);
}

#[test]
fn character_cache_rejects_retired_generations_and_changed_encoding() {
    let mut arena = RArena::new();
    let original = arena.alloc_charsxp(b"repeat");
    let token = arena.node_token(original).unwrap();
    let heap = arena.heap_identity();
    let mut changed = heap.node_snapshot(&token).unwrap();
    changed.sxpinfo.set_gp(1 << 3);
    heap.replace_node(&token, changed).unwrap();
    let fresh = arena.alloc_charsxp(b"repeat");
    assert_ne!(fresh, original);
    assert_eq!(
        heap.node_snapshot(&arena.node_token(fresh).unwrap())
            .unwrap()
            .sxpinfo
            .gp(),
        1 << 6
    );
    unsafe {
        arena.free_node(fresh);
    }
    let replacement = arena.alloc_node(SEXPTYPE::REALSXP);
    assert_eq!(replacement, fresh);
    let current = arena.alloc_charsxp(b"repeat");
    assert_ne!(current, replacement);
    assert_eq!(
        arena
            .node_token(replacement)
            .unwrap()
            .heap_identity()
            .node_snapshot(&arena.node_token(replacement).unwrap())
            .unwrap()
            .sxpinfo
            .type_of(),
        SEXPTYPE::REALSXP
    );
}

#[test]
fn character_cache_does_not_alias_domains_or_non_ascii_encoding_operands() {
    let mut left = RArena::new();
    let mut right = RArena::new();
    let a = left.alloc_charsxp(b"repeat");
    let b = right.alloc_charsxp(b"repeat");
    assert!(
        !left
            .node_token(a)
            .unwrap()
            .same_heap(&right.node_token(b).unwrap())
    );
    let a = left.alloc_charsxp("é".as_bytes());
    let b = left.alloc_charsxp("é".as_bytes());
    assert_ne!(a, b);
}

#[test]
fn character_cache_does_not_root_discarded_values_through_collection() {
    let mut session = crate::sexp::session::RSession::new_for_gc_tests();
    let original = session
        .with_arena(|arena| {
            let value = arena.alloc_charsxp(b"discarded cached character");
            arena.node_token(value).unwrap()
        })
        .unwrap();
    session.with_active(|| {
        crate::sexp::gengc::full_gc();
    });
    assert!(!original.is_live());
    session
        .with_arena(|arena| {
            let fresh = arena.alloc_charsxp(b"discarded cached character");
            assert!(!fresh.is_null());
            let fresh = arena.node_token(fresh).unwrap();
            assert!(fresh.is_live());
            assert!(!original.is_live());
        })
        .unwrap();
}
