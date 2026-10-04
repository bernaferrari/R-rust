#![forbid(unsafe_code)]
//! Narrow projection reader for the translated physical header representation.
//! Tracing owns copied edges; it never lends mutable node or vector fields.

use super::gc_trace::{TraceContext, TraceError, TraceNode};
use super::{
    EDGE_ATTRIB, EDGE_BODY, EDGE_CAR, EDGE_CDR, EDGE_CLOENV, EDGE_ENCLOS, EDGE_EXT_PROT,
    EDGE_EXT_TAG, EDGE_FORMALS, EDGE_FRAME, EDGE_HASHTAB, EDGE_INTERNAL, EDGE_PNAME, EDGE_PROM_ENV,
    EDGE_PROM_EXPR, EDGE_PROM_VALUE, EDGE_SYM_VALUE, EDGE_TAG, EDGE_VECTOR, child_mask,
};
use crate::sexp::heap::NodeLink;
use crate::sexp::{ffi::EdgeField, heap::CheckedNode};

pub(super) struct ChildSnapshot {
    fixed: [NodeLink; 4],
    vector: Vec<NodeLink>,
}
impl ChildSnapshot {
    pub(super) fn into_edges(self) -> impl Iterator<Item = NodeLink> {
        self.fixed.into_iter().chain(self.vector)
    }
}

/// Copy children from checked headers and owned typed payload cells.
/// No caller-supplied address is dereferenced and no storage loan escapes.
pub(super) fn snapshot_children(
    node: &TraceNode,
    context: &TraceContext,
) -> Result<ChildSnapshot, TraceError> {
    use crate::sexp::ffi::{NodeBody, SEXPTYPE};
    let header = node.snapshot(context)?;
    let mut fixed = [
        header.attrib,
        NodeLink::null(),
        NodeLink::null(),
        NodeLink::null(),
    ];
    match header.data {
        NodeBody::Symbol(body) => {
            fixed[1..].copy_from_slice(&[body.pname, body.value, body.internal])
        }
        NodeBody::List(body) => {
            // A weak key does not retain itself; value and finalizer do.
            if header.sxpinfo.type_of() != SEXPTYPE::WEAKREFSXP {
                fixed[1] = body.carval;
            }
            fixed[2] = body.cdrval;
            fixed[3] = body.tagval;
        }
        NodeBody::Closure(body) => fixed[1..].copy_from_slice(&[body.formals, body.body, body.env]),
        NodeBody::Environment(body) => {
            fixed[1..].copy_from_slice(&[body.frame, body.enclos, body.hashtab])
        }
        NodeBody::Promise(body) => fixed[1..].copy_from_slice(&[body.value, body.expr, body.env]),
        NodeBody::ExtPtr(body) => {
            fixed[1] = body.protected;
            fixed[2] = body.tag;
        }
        _ => {}
    }
    let vector = if super::vector_payload_has_sexp_refs(header.sxpinfo.type_of())
        && !header.payload.is_empty()
    {
        context
            .reference_links(node)
            .ok_or(TraceError::InvalidPayload(node.address()))?
    } else {
        Vec::new()
    };
    Ok(ChildSnapshot { fixed, vector })
}

/// Rewrite detached exact links. No header or payload loan spans the visitor;
/// unchanged capabilities retain their original generation and lease identity.
pub(super) fn rewrite_children(
    parent: &CheckedNode,
    follow_weak_key: bool,
    mut visit: impl FnMut(&mut NodeLink),
) {
    let heap = parent.heap_identity();
    let header = heap.node_snapshot(parent).expect("GC parent remains live");
    let mask = child_mask(header.sxpinfo.type_of().0, follow_weak_key);
    // Snapshot before callbacks. Neither the header Cell nor reference cells
    // remain borrowed while the visitor runs, including materialized ALTREP.
    let vector_payload = if mask & EDGE_VECTOR != 0 && !header.payload.is_empty() {
        Some(
            heap.reference_payload_lease(parent)
                .expect("GC vector has an owned reference payload"),
        )
    } else {
        None
    };
    let vector_links = match &vector_payload {
        Some(payload) => payload
            .snapshot(
                usize::try_from(header.vecsxp_length()).expect("GC vector length is representable"),
            )
            .expect("GC vector length fits its reference allocation"),
        None => Vec::new(),
    };
    let fields = [
        (EDGE_PNAME, EdgeField::SymbolName),
        (EDGE_SYM_VALUE, EdgeField::SymbolValue),
        (EDGE_INTERNAL, EdgeField::SymbolInternal),
        (EDGE_CAR, EdgeField::ListCar),
        (EDGE_CDR, EdgeField::ListCdr),
        (EDGE_TAG, EdgeField::ListTag),
        (EDGE_FORMALS, EdgeField::ClosureFormals),
        (EDGE_BODY, EdgeField::ClosureBody),
        (EDGE_CLOENV, EdgeField::ClosureEnvironment),
        (EDGE_FRAME, EdgeField::EnvironmentFrame),
        (EDGE_ENCLOS, EdgeField::EnvironmentEnclosure),
        (EDGE_HASHTAB, EdgeField::EnvironmentHashTable),
        (EDGE_PROM_VALUE, EdgeField::PromiseValue),
        (EDGE_PROM_EXPR, EdgeField::PromiseExpression),
        (EDGE_PROM_ENV, EdgeField::PromiseEnvironment),
        (EDGE_EXT_TAG, EdgeField::ExternalTag),
        (EDGE_EXT_PROT, EdgeField::ExternalProtected),
        (EDGE_ATTRIB, EdgeField::Attribute),
    ];
    let mut header_changes = Vec::new();
    for (bit, field) in fields {
        if mask & bit == 0 {
            continue;
        }
        let original = header.edge(field).expect("GC field matches node body");
        let mut child = original;
        visit(&mut child);
        if child != original {
            header_changes.push((field, child));
        }
    }
    let mut vector_changes = Vec::new();
    for (index, original) in vector_links.iter().copied().enumerate() {
        let mut child = original;
        visit(&mut child);
        if child != original {
            vector_changes.push((index, child));
        }
    }
    // Validate the complete transaction before publishing any rewritten edge.
    // A visitor may update unrelated flags/edges, but cannot silently retarget
    // this snapshot to a different body, vector shape or allocation.
    let mut current = heap
        .node_snapshot(parent)
        .expect("rewritten GC parent remains live");
    assert_eq!(
        current.sxpinfo.type_of(),
        header.sxpinfo.type_of(),
        "GC parent type changed during visitation"
    );
    assert_eq!(
        std::mem::discriminant(&current.data),
        std::mem::discriminant(&header.data),
        "GC parent body family changed during visitation"
    );
    if mask & EDGE_VECTOR != 0 {
        assert_eq!(
            current.vecsxp_length(),
            header.vecsxp_length(),
            "GC vector length changed during visitation"
        );
        let current_payload = if !current.payload.is_empty() {
            Some(
                heap.reference_payload_lease(parent)
                    .expect("rewritten GC vector has an owned reference payload"),
            )
        } else {
            None
        };
        assert!(
            match (&vector_payload, &current_payload) {
                (None, None) => true,
                (Some(original), Some(current)) => original.same_allocation(current),
                _ => false,
            },
            "GC reference allocation changed during visitation"
        );
    }
    for (field, _) in &header_changes {
        assert_eq!(
            current.edge(*field),
            header.edge(*field),
            "GC scalar edge changed during visitation"
        );
    }
    for (index, _) in &vector_changes {
        assert_eq!(
            vector_payload
                .as_ref()
                .and_then(|payload| payload.element(*index)),
            vector_links.get(*index).copied(),
            "GC reference cell changed during visitation"
        );
    }
    for child in header_changes
        .iter()
        .map(|(_, child)| child)
        .chain(vector_changes.iter().map(|(_, child)| child))
    {
        heap.resolve_link(*child)
            .expect("rewritten GC edge names a live allocation in its heap");
    }
    for (field, child) in &header_changes {
        current
            .set_edge(*field, *child)
            .expect("GC field matches node body");
    }
    // No visitor, semantic callback or storage loan can run between this
    // validation and the two commits. Sparse cells preserve unrelated writes.
    if !header_changes.is_empty() {
        heap.replace_node(parent, current)
            .expect("rewritten GC parent remains live");
    }
    if !vector_changes.is_empty() {
        vector_payload
            .as_ref()
            .expect("GC reference changes have an allocation")
            .replace_sparse(&vector_changes)
            .expect("validated reference positions remain bounded");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{
        ffi::SEXPTYPE,
        gengc::gc_trace::TraceWorklist,
        heap::ReferenceChild,
        memory::{RArena, checked_projection, checked_snapshot},
    };
    use std::rc::Rc;

    #[test]
    fn vector_tracing_rejects_incompatible_payload_publication() {
        let mut arena = RArena::new();
        let heap = arena.heap_identity();
        let numeric = arena.alloc_vector(SEXPTYPE::INTSXP, 2);
        let numeric_token = checked_projection(numeric).unwrap().1;
        let numeric_payload = heap.payload_lease(&numeric_token).unwrap();
        let short = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
        let short_token = checked_projection(short).unwrap().1;
        let short_payload = heap.payload_lease(&short_token).unwrap();
        let pointer = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
        let token = checked_projection(pointer).unwrap().1;
        let original = heap.node_snapshot(&token).unwrap();
        for payload in [&numeric_payload, &short_payload] {
            assert!(
                heap.publish_payload(&token, original.payload, payload)
                    .is_none()
            );
            let unchanged = heap.node_snapshot(&token).unwrap();
            assert_eq!(unchanged.payload, original.payload);
            assert_eq!(unchanged.data, original.data);
            let mut worklist = TraceWorklist::new(Rc::new(TraceContext::new(heap.clone(), 41)));
            worklist.enqueue(pointer).unwrap();
            let node = worklist.next_marked().unwrap().unwrap();
            assert_eq!(
                snapshot_children(&node, worklist.context()).unwrap().vector,
                vec![NodeLink::NULL; 2]
            );
        }
    }

    #[test]
    fn copied_canonical_edges_keep_the_original_generation_after_slot_reuse() {
        let mut session = crate::sexp::session::RSession::new_for_gc_tests();
        let parent = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::LISTSXP))
            .unwrap();
        let parent_token = checked_projection(parent).unwrap().1;
        let heap = parent_token.heap_identity();
        let child = session
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::LISTSXP))
            .unwrap();
        let parent_value = session.sexp(parent).unwrap();
        let original = checked_projection(child).unwrap().1;
        let saved = original.link().unwrap();
        super::super::full_gc();
        assert!(!original.is_live());
        assert!(parent_value.is_live());
        let factory = crate::sexp::object::SessionNodeFactory::new(session.owner_token().unwrap());
        let replacement_value = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::LISTSXP)))
            .unwrap();
        let replacement = replacement_value.as_raw();
        assert_eq!(child.addr(), replacement.addr());
        // Exercise a copied stale capability; no pointer reconstruction can
        // turn it into the allocation now occupying the same physical slot.
        let mut header = checked_snapshot(parent, &parent_token).unwrap();
        header.data.list_mut().carval = saved;
        heap.replace_node(&parent_token, header).unwrap();
        let mut worklist = TraceWorklist::new(Rc::new(TraceContext::new(heap, 47)));
        worklist.enqueue(parent).unwrap();
        let node = worklist.next_marked().unwrap().unwrap();
        let copied = snapshot_children(&node, worklist.context()).unwrap();
        let edge = copied.into_edges().find(|link| !link.is_null()).unwrap();
        assert_eq!(edge, saved);
        assert_eq!(
            worklist.enqueue_link(edge),
            Err(TraceError::InvalidLink(saved))
        );
        assert!(worklist.next_marked().unwrap().is_none());
    }

    #[test]
    fn copied_child_rewrites_reject_foreign_links_before_any_field_is_published() {
        let mut arena = RArena::new();
        let heap = arena.heap_identity();
        let first = arena.alloc_node(SEXPTYPE::LISTSXP);
        let second = arena.alloc_node(SEXPTYPE::LISTSXP);
        let replacement = arena.alloc_node(SEXPTYPE::LISTSXP);
        let parent = arena.alloc_node(SEXPTYPE::LISTSXP);
        let token = checked_projection(parent).unwrap().1;
        let first_token = checked_projection(first).unwrap().1;
        let second_token = checked_projection(second).unwrap().1;
        heap.set_edge(
            &token,
            EdgeField::ListCar,
            ReferenceChild::Node(&first_token),
        )
        .unwrap();
        heap.set_edge(
            &token,
            EdgeField::ListCdr,
            ReferenceChild::Node(&second_token),
        )
        .unwrap();
        let original = heap.node_snapshot(&token).unwrap();
        let first_link = checked_projection(first).unwrap().1.link().unwrap();
        let second_link = checked_projection(second).unwrap().1.link().unwrap();
        let replacement_link = checked_projection(replacement).unwrap().1.link().unwrap();
        let mut foreign = RArena::new();
        let foreign_child = foreign.alloc_node(SEXPTYPE::LISTSXP);
        let foreign_link = checked_projection(foreign_child).unwrap().1.link().unwrap();
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rewrite_children(&token, true, |child| {
                if *child == first_link {
                    *child = replacement_link;
                } else if *child == second_link {
                    *child = foreign_link;
                }
            });
        }));
        assert!(rejected.is_err());
        let current = heap.node_snapshot(&token).unwrap();
        assert_eq!(current.data, original.data);
        assert_eq!(current.attrib, original.attrib);
        assert_eq!(heap.projection_of_link(first_link), Some(first));
        assert_eq!(heap.projection_of_link(second_link), Some(second));
    }

    #[test]
    fn parent_shape_changes_reject_all_rewrites_and_preserve_callback_state() {
        #[derive(Clone, Copy, Debug)]
        enum Change {
            Shorter,
            DifferentPayload,
            DifferentBody,
        }
        for change in [
            Change::Shorter,
            Change::DifferentPayload,
            Change::DifferentBody,
        ] {
            let mut arena = RArena::new();
            let heap = arena.heap_identity();
            let first = arena.alloc_node(SEXPTYPE::LISTSXP);
            let replacement = arena.alloc_node(SEXPTYPE::LISTSXP);
            let parent = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
            let other = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
            let token = checked_projection(parent).unwrap().1;
            let first = checked_projection(first).unwrap().1;
            let replacement = checked_projection(replacement).unwrap().1;
            let other = checked_projection(other).unwrap().1;
            let original = first.link().unwrap();
            let rewritten = replacement.link().unwrap();
            heap.set_edge(&token, EdgeField::Attribute, ReferenceChild::Node(&first))
                .unwrap();
            for index in 0..2 {
                heap.set_reference_elt(&token, index, ReferenceChild::Node(&first))
                    .unwrap();
            }
            let original_payload = heap.reference_payload_lease(&token).unwrap();
            let different_payload = heap.reference_payload_lease(&other).unwrap();
            assert!(!original_payload.same_allocation(&different_payload));
            let other_payload = heap.payload_lease(&other).unwrap();
            let mut changed = false;
            let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rewrite_children(&token, true, |link| {
                    if *link != original {
                        return;
                    }
                    *link = rewritten;
                    if !changed {
                        changed = true;
                        let mut header = heap.node_snapshot(&token).unwrap();
                        header.sxpinfo.set_named(2);
                        match change {
                            Change::Shorter => header.set_vecsxp_length(1),
                            Change::DifferentPayload => {
                                heap.publish_payload(&token, header.payload, &other_payload)
                                    .unwrap();
                                header = heap.node_snapshot(&token).unwrap();
                                header.sxpinfo.set_named(2);
                            }
                            Change::DifferentBody => {
                                let original_header = heap.node_snapshot(&token).unwrap();
                                let mut incompatible = header;
                                incompatible.data = crate::sexp::ffi::NodeBody::Other;
                                assert!(heap.replace_node(&token, incompatible).is_none());
                                let unchanged = heap.node_snapshot(&token).unwrap();
                                assert_eq!(unchanged.data, original_header.data);
                                assert_eq!(
                                    unchanged.sxpinfo.type_and_flags,
                                    original_header.sxpinfo.type_and_flags
                                );
                                assert_eq!(unchanged.attrib, original_header.attrib);
                                assert_eq!(unchanged.payload, original_header.payload);
                                assert_eq!(
                                    heap.reference_links(&token),
                                    Some(vec![original, original])
                                );
                                // A callback can publish a complete valid transition;
                                // the pending GC transaction must still reject it.
                                header.sxpinfo.set_type(SEXPTYPE::S4SXP);
                                header.data = crate::sexp::ffi::NodeBody::Other;
                                header.payload = crate::sexp::payload::PayloadLink::EMPTY;
                            }
                        }
                        heap.replace_node(&token, header).unwrap();
                    }
                });
            }));
            assert!(
                rejected.is_err(),
                "{change:?} must reject before publication"
            );
            let current = heap.node_snapshot(&token).unwrap();
            assert_eq!(
                current.attrib, original,
                "pending scalar rewrite must not publish"
            );
            assert_eq!(current.sxpinfo.named(), 2);
            assert_eq!(original_payload.snapshot(2), Some(vec![original, original]));
            assert_eq!(different_payload.snapshot(2), Some(vec![NodeLink::NULL; 2]));
            match change {
                Change::Shorter => assert_eq!(current.vecsxp_length(), 1),
                Change::DifferentPayload => assert_eq!(current.payload, other_payload.link()),
                Change::DifferentBody => {
                    assert_eq!(current.sxpinfo.type_of(), SEXPTYPE::S4SXP);
                    assert!(matches!(current.data, crate::sexp::ffi::NodeBody::Other));
                }
            }
        }
    }

    #[test]
    fn selected_scalar_conflict_rejects_earlier_pending_rewrite() {
        let mut arena = RArena::new();
        let heap = arena.heap_identity();
        let parent = arena.alloc_node(SEXPTYPE::LISTSXP);
        let children: Vec<_> = (0..4)
            .map(|_| {
                let child = arena.alloc_node(SEXPTYPE::LISTSXP);
                checked_projection(child).unwrap().1
            })
            .collect();
        let parent = checked_projection(parent).unwrap().1;
        heap.set_edge(
            &parent,
            EdgeField::ListCar,
            ReferenceChild::Node(&children[0]),
        )
        .unwrap();
        heap.set_edge(
            &parent,
            EdgeField::ListCdr,
            ReferenceChild::Node(&children[1]),
        )
        .unwrap();
        let first = children[0].link().unwrap();
        let second = children[1].link().unwrap();
        let replacement = children[2].link().unwrap();
        let callback = children[3].link().unwrap();
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rewrite_children(&parent, true, |link| {
                if *link == first {
                    *link = replacement;
                    heap.set_edge(
                        &parent,
                        EdgeField::ListCdr,
                        ReferenceChild::Node(&children[3]),
                    )
                    .unwrap();
                } else if *link == second {
                    *link = replacement;
                }
            });
        }));
        assert!(rejected.is_err());
        assert_eq!(heap.edge(&parent, EdgeField::ListCar), Some(first));
        assert_eq!(heap.edge(&parent, EdgeField::ListCdr), Some(callback));
    }

    #[test]
    fn selected_reference_conflict_rejects_pending_attribute_rewrite() {
        let mut arena = RArena::new();
        let heap = arena.heap_identity();
        let parent = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
        let children: Vec<_> = (0..4)
            .map(|_| {
                let child = arena.alloc_node(SEXPTYPE::LISTSXP);
                checked_projection(child).unwrap().1
            })
            .collect();
        let parent = checked_projection(parent).unwrap().1;
        heap.set_edge(
            &parent,
            EdgeField::Attribute,
            ReferenceChild::Node(&children[0]),
        )
        .unwrap();
        heap.set_reference_elt(&parent, 0, ReferenceChild::Node(&children[1]))
            .unwrap();
        let attribute = children[0].link().unwrap();
        let original = children[1].link().unwrap();
        let replacement = children[2].link().unwrap();
        let callback = children[3].link().unwrap();
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rewrite_children(&parent, true, |link| {
                if *link == attribute {
                    *link = replacement;
                } else if *link == original {
                    heap.set_reference_elt(&parent, 0, ReferenceChild::Node(&children[3]))
                        .unwrap();
                    *link = replacement;
                }
            });
        }));
        assert!(rejected.is_err());
        assert_eq!(heap.edge(&parent, EdgeField::Attribute), Some(attribute));
        assert_eq!(heap.reference_link_elt(&parent, 0), Some(callback));
    }

    #[test]
    fn sparse_reference_rewrites_preserve_unselected_cells_flags_and_scalar_edges() {
        let mut arena = RArena::new();
        let heap = arena.heap_identity();
        let parent = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
        let children: Vec<_> = (0..3)
            .map(|_| {
                let child = arena.alloc_node(SEXPTYPE::LISTSXP);
                checked_projection(child).unwrap().1
            })
            .collect();
        let parent = checked_projection(parent).unwrap().1;
        heap.set_reference_elt(&parent, 0, ReferenceChild::Node(&children[0]))
            .unwrap();
        let original = children[0].link().unwrap();
        let replacement = children[1].link().unwrap();
        let callback = children[2].link().unwrap();
        rewrite_children(&parent, true, |link| {
            if *link == original {
                heap.set_reference_elt(&parent, 1, ReferenceChild::Node(&children[2]))
                    .unwrap();
                heap.set_edge(
                    &parent,
                    EdgeField::Attribute,
                    ReferenceChild::Node(&children[2]),
                )
                .unwrap();
                let mut header = heap.node_snapshot(&parent).unwrap();
                header.sxpinfo.set_named(2);
                heap.replace_node(&parent, header).unwrap();
                *link = replacement;
            }
        });
        let current = heap.node_snapshot(&parent).unwrap();
        assert_eq!(current.sxpinfo.named(), 2);
        assert_eq!(current.attrib, callback);
        assert_eq!(
            heap.reference_links(&parent),
            Some(vec![replacement, callback])
        );
    }

    #[test]
    fn provenance_free_inputs_recover_owned_header_projection_for_both_enqueue_paths() {
        let mut arena = RArena::new();
        let heap = arena.heap_identity();
        let pointer = arena.alloc_node(SEXPTYPE::LISTSXP);
        let token = checked_projection(pointer).unwrap().1;
        let forged = std::ptr::without_provenance_mut(pointer.addr());
        let mut header = checked_snapshot(pointer, &token).unwrap();
        header.data.list_mut().cdrval = token.link().unwrap();
        heap.replace_node(&token, header).unwrap();
        for (epoch, checked_root) in [(31, false), (32, true)] {
            let mut worklist = TraceWorklist::new(Rc::new(TraceContext::new(heap.clone(), epoch)));
            if checked_root {
                worklist.enqueue_checked(forged, token.clone()).unwrap();
            } else {
                worklist.enqueue(forged).unwrap();
            }
            let node = worklist.next_marked().unwrap().unwrap();
            let children = snapshot_children(&node, worklist.context()).unwrap();
            assert!(
                children
                    .into_edges()
                    .any(|child| child == token.link().unwrap())
            );
            assert_eq!(
                heap.projection_of_link(token.link().unwrap()),
                Some(pointer)
            );
            let mut header = checked_snapshot(pointer, &token).unwrap();
            assert_eq!(header.sxpinfo.type_of(), SEXPTYPE::LISTSXP);
            header.data.list_mut().carval = token.link().unwrap();
            heap.replace_node(&token, header).unwrap();
        }
    }
}
