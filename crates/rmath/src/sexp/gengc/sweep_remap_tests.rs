use super::*;
use crate::sexp::{
    ffi::EdgeField,
    heap::{CheckedNode, ReferenceChild},
    memory::checked_projection,
    session::RSession,
};

fn token(projection: SEXP) -> CheckedNode {
    checked_projection(projection).unwrap().1
}

fn make_old(projection: SEXP, node: &CheckedNode) {
    let heap = node.heap_identity();
    let mut header = heap.node_snapshot(node).unwrap();
    header.sxpinfo.set_gcgen(Generation::Old as u8);
    heap.replace_node(node, header).unwrap();
    crate::sexp::memory::note_slab_generation(projection, Generation::Old as u8);
}

#[test]
fn sweep_remap_minor_clears_unmarked_old_vector_and_attribute_edges() {
    let mut session = RSession::new_for_gc_tests();
    let (parent, child, attribute) = session
        .with_arena(|arena| {
            let parent = arena.alloc_vector(SEXPTYPE::VECSXP, 3);
            let child = arena.alloc_node(SEXPTYPE::LISTSXP);
            let attribute = arena.alloc_node(SEXPTYPE::LISTSXP);
            let node = token(parent);
            let heap = node.heap_identity();
            heap.set_reference_elt(&node, 0, ReferenceChild::Node(&token(child)))
                .unwrap();
            heap.set_reference_elt(&node, 2, ReferenceChild::Node(&token(child)))
                .unwrap();
            heap.set_edge(
                &node,
                EdgeField::Attribute,
                ReferenceChild::Node(&token(attribute)),
            )
            .unwrap();
            make_old(parent, &node);
            (node, token(child), token(attribute))
        })
        .unwrap();
    // Tokens retain metadata only. No owning value or remembered-set barrier
    // accidentally roots this old, unreachable parent or its young children.
    session.with_active(|| {
        assert_eq!(minor_gc(), (0, 2));
        assert!(parent.is_live());
        assert!(!child.is_live());
        assert!(!attribute.is_live());
        let heap = parent.heap_identity();
        let nil = heap
            .link_from_projection(crate::sexp::object::Sexp::nil().as_raw())
            .unwrap();
        assert_eq!(
            heap.reference_links(&parent),
            Some(vec![nil, NodeLink::NULL, nil])
        );
        assert_eq!(heap.edge(&parent, EdgeField::Attribute), Some(nil));
        assert_eq!(full_gc(), (0, 1));
        assert!(!parent.is_live());
    });
}

#[test]
fn sweep_remap_torture_clears_old_children_of_unmarked_young_survivor() {
    let mut session = RSession::new_for_gc_tests();
    let (parent, child) = session
        .with_arena(|arena| {
            let parent = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
            let child = arena.alloc_node(SEXPTYPE::LISTSXP);
            let node = token(parent);
            let child_node = token(child);
            node.heap_identity()
                .set_reference_elt(&node, 1, ReferenceChild::Node(&child_node))
                .unwrap();
            make_old(child, &child_node);
            (node, child_node)
        })
        .unwrap();
    session.with_active(|| {
        let collected = instance::with_required_current_instance(|owner| {
            run_gc_cycle_in(owner, do_torture_mark_sweep_in)
        });
        assert_eq!(collected, (0, 1));
        assert!(parent.is_live());
        assert!(!child.is_live());
        let heap = parent.heap_identity();
        let nil = heap
            .link_from_projection(crate::sexp::object::Sexp::nil().as_raw())
            .unwrap();
        assert_eq!(
            heap.reference_links(&parent),
            Some(vec![NodeLink::NULL, nil])
        );
        assert_eq!(full_gc(), (0, 1));
        assert!(!parent.is_live());
    });
}

#[test]
fn sweep_remap_retains_newly_ready_finalizer_vector_graph_before_retirement() {
    unsafe extern "C" fn finalizer(_: *mut std::ffi::c_void) {}
    for collect in [minor_gc as fn() -> (usize, usize), full_gc] {
        let mut session = RSession::new_for_gc_tests();
        let (key, vector, child, garbage) = session
            .with_arena(|arena| {
                let key = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
                let vector = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
                let child = arena.alloc_node(SEXPTYPE::LISTSXP);
                let garbage = arena.alloc_node(SEXPTYPE::LISTSXP);
                let key_node = token(key);
                let vector_node = token(vector);
                let child_node = token(child);
                let heap = key_node.heap_identity();
                heap.set_edge(
                    &key_node,
                    EdgeField::ExternalProtected,
                    ReferenceChild::Node(&vector_node),
                )
                .unwrap();
                heap.set_reference_elt(&vector_node, 1, ReferenceChild::Node(&child_node))
                    .unwrap();
                (key, vector_node, child_node, token(garbage))
            })
            .unwrap();
        let key_node = token(key);
        session.with_active(|| {
            // SAFETY: the exact active key is live, and registration borrows no
            // header or payload across finalizer execution or collection.
            unsafe { crate::mainutils::memory_main::R_RegisterCFinalizer(key, finalizer) };
            assert_eq!(collect(), (3, 1));
            assert!(key_node.is_live());
            assert!(vector.is_live());
            assert!(child.is_live());
            assert!(!garbage.is_live());
            let heap = vector.heap_identity();
            assert_eq!(heap.reference_link_elt(&vector, 1), child.link());
            assert_eq!(
                heap.edge(&key_node, EdgeField::ExternalProtected),
                vector.link()
            );
            // Full GC itself doesn't invoke ready callbacks. The ready graph
            // remains rooted through another epoch until the real dispatcher.
            assert_eq!(full_gc(), (0, 0));
            assert!(child.is_live());
        });
    }
}

#[test]
fn sweep_remap_generic_mapping_does_not_skip_marked_parents() {
    let mut arena = RArena::new();
    let parent = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
    let child = arena.alloc_node(SEXPTYPE::LISTSXP);
    let replacement = arena.alloc_node(SEXPTYPE::LISTSXP);
    let parent_node = token(parent);
    let heap = parent_node.heap_identity();
    heap.set_reference_elt(&parent_node, 0, ReferenceChild::Node(&token(child)))
        .unwrap();
    crate::sexp::memory::begin_gc_epoch();
    assert!(matches!(
        crate::sexp::memory::gc_touch(parent),
        crate::sexp::memory::GcTouch::NewlyMarked
    ));
    update_references_in_object(
        parent,
        &HashMap::from([(
            token(child).link().unwrap(),
            token(replacement).link().unwrap(),
        )]),
    );
    assert_eq!(
        heap.reference_link_elt(&parent_node, 0),
        token(replacement).link()
    );
    assert_eq!(
        heap.reference_link_elt(&parent_node, 1),
        Some(NodeLink::NULL)
    );
}
