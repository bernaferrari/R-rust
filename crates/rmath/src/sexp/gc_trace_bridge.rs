#![forbid(unsafe_code)]
//! Narrow projection reader for the translated physical header representation.
//! Tracing owns copied edges; it never lends mutable node or vector fields.

use super::gc_trace::{TraceContext, TraceError, TraceNode};
use crate::sexp::ffi::SEXP;

pub(super) struct ChildSnapshot {
    fixed: [SEXP; 4],
    vector: Vec<SEXP>,
}
impl ChildSnapshot {
    pub(super) fn into_edges(self) -> impl Iterator<Item = SEXP> {
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
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
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
            fixed[1] = body[1].cast();
            fixed[2] = body[2].cast();
        }
        _ => {}
    }
    let vector = if super::vector_payload_has_sexp_refs(header.sxpinfo.type_of())
        && !header.gengc_next_node.is_null()
    {
        let NodeBody::Vector(body) = header.data else {
            return Err(TraceError::InvalidPayload(node.address()));
        };
        let length =
            usize::try_from(body.length).map_err(|_| TraceError::InvalidPayload(node.address()))?;
        context
            .copy_reference_payload(header.gengc_next_node.cast(), length)
            .ok_or(TraceError::InvalidPayload(node.address()))?
    } else {
        Vec::new()
    };
    Ok(ChildSnapshot { fixed, vector })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{
        ffi::{SEXPTYPE, SexprecCore},
        gengc::gc_trace::TraceWorklist,
        heap::{HeapIdentity, NodePage},
        memory::register_node_page,
    };
    use std::rc::Rc;

    #[test]
    fn vector_tracing_rejects_unowned_payload_addresses_without_reading_them() {
        let mut arena = crate::sexp::memory::RArena::new();
        let heap = arena.heap_identity();
        let numeric = arena.alloc_vector(SEXPTYPE::INTSXP, 2);
        let numeric_token = crate::sexp::memory::checked_projection(numeric).unwrap().1;
        let numeric_payload = crate::sexp::memory::checked_snapshot(numeric, &numeric_token)
            .unwrap()
            .gengc_next_node;
        let references = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
        let references_token = crate::sexp::memory::checked_projection(references)
            .unwrap()
            .1;
        let short_payload = crate::sexp::memory::checked_snapshot(references, &references_token)
            .unwrap()
            .gengc_next_node;
        for payload in [std::ptr::dangling_mut(), numeric_payload, short_payload] {
            let page = NodePage::try_new(heap.clone(), 17, 1, || {
                let mut header = SexprecCore::new_vector(SEXPTYPE::VECSXP, 2);
                header.gengc_next_node = payload;
                header
            })
            .unwrap();
            page.metadata().activate(0, false).unwrap();
            let pointer = page.raw_slot(0).unwrap();
            let _registration = register_node_page(&page);
            let mut worklist = TraceWorklist::new(Rc::new(TraceContext::new(heap.clone(), 41)));
            worklist.enqueue(pointer).unwrap();
            let node = worklist.next_marked().unwrap().unwrap();
            assert!(matches!(
                snapshot_children(&node, worklist.context()),
                Err(TraceError::InvalidPayload(_))
            ));
        }
    }

    #[test]
    fn provenance_free_inputs_recover_owned_header_projection_for_both_enqueue_paths() {
        let heap = HeapIdentity::new();
        let page =
            NodePage::try_new(heap.clone(), 0, 1, || SexprecCore::new(SEXPTYPE::LISTSXP)).unwrap();
        page.metadata().activate(0, false).unwrap();
        let pointer = page.raw_slot(0).unwrap();
        let _registration = register_node_page(&page);
        let forged = std::ptr::without_provenance_mut(pointer.addr());
        let token = page.token(0).unwrap();
        let mut header = page.projection().copy_live(token.id()).unwrap();
        header.data.list_mut().cdrval = pointer;
        page.replace_live(token.id(), header).unwrap();
        for (epoch, checked_root) in [(31, false), (32, true)] {
            let mut worklist = TraceWorklist::new(Rc::new(TraceContext::new(heap.clone(), epoch)));
            if checked_root {
                worklist
                    .enqueue_checked(forged, page.token(0).unwrap())
                    .unwrap();
            } else {
                worklist.enqueue(forged).unwrap();
            }
            let node = worklist.next_marked().unwrap().unwrap();
            let children = snapshot_children(&node, worklist.context()).unwrap();
            assert!(children.into_edges().any(|child| child == pointer));
            let mut header = page.projection().copy_live(token.id()).unwrap();
            assert_eq!(header.sxpinfo.type_of(), SEXPTYPE::LISTSXP);
            header.data.list_mut().carval = pointer;
            page.replace_live(token.id(), header).unwrap();
        }
    }
}
