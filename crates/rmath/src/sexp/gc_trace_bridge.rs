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

/// Copy children only after exact allocation and heap validation.
///
/// # Safety
/// The live node's selected union arm and pointer-vector payload must retain
/// the translated runtime's representation invariants. A vector payload is
/// published with storage covering its header length, and no payload writer
/// or owner teardown may overlap collection. Checked metadata proves header
/// liveness; the pending typed-payload migration will encode buffer bounds.
pub(super) unsafe fn snapshot_children(
    node: &TraceNode,
    context: &TraceContext,
) -> Result<ChildSnapshot, TraceError> {
    let header = node.snapshot(context)?;
    let (info, attrib, data, payload) = (
        header.sxpinfo,
        header.attrib,
        header.data,
        header.gengc_next_node,
    );
    let mut fixed = [
        attrib,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    ];
    // SAFETY: read only the union arm selected by the copied type tag. Weak
    // references deliberately omit CAR, their weak key, from marking.
    unsafe {
        match info.type_of().0 {
            1 => {
                let body = data.symsxp;
                fixed[1..].copy_from_slice(&[body.pname, body.value, body.internal]);
            }
            2 | 6 | 17 => {
                let body = data.listsxp;
                fixed[1..].copy_from_slice(&[body.carval, body.cdrval, body.tagval]);
            }
            3 => {
                let body = data.closxp;
                fixed[1..].copy_from_slice(&[body.formals, body.body, body.env]);
            }
            4 => {
                let body = data.envsxp;
                fixed[1..].copy_from_slice(&[body.frame, body.enclos, body.hashtab]);
            }
            5 => {
                let body = data.promsxp;
                fixed[1..].copy_from_slice(&[body.value, body.expr, body.env]);
            }
            22 => {
                let body = data.extptr;
                fixed[1] = body[1].cast();
                fixed[2] = body[2].cast();
            }
            23 => {
                let body = data.listsxp;
                fixed[1] = body.cdrval;
                fixed[2] = body.tagval;
            }
            _ => {}
        }
    }
    let mut vector = Vec::new();
    if super::vector_payload_has_sexp_refs(info.type_of()) {
        // SAFETY: these types select the vector arm. A lazy vector may have
        // no projected payload; its owned metadata remains an attribute edge.
        let length = unsafe { data.vecsxp.length };
        let length = usize::try_from(length)
            .ok()
            .filter(|length| *length <= isize::MAX as usize / std::mem::size_of::<SEXP>())
            .ok_or(TraceError::InvalidPayload(node.address()))?;
        if !payload.is_null() {
            vector.reserve(length);
            let pointers = payload.cast::<SEXP>();
            for index in 0..length {
                // SAFETY: the caller guarantees the published payload span.
                // Every copied child is independently checked on enqueue.
                vector.push(unsafe { pointers.add(index).read() });
            }
        }
    }
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
    fn provenance_free_inputs_recover_owned_header_projection_for_both_enqueue_paths() {
        let heap = HeapIdentity::new();
        let page =
            NodePage::try_new(heap.clone(), 0, 1, || SexprecCore::new(SEXPTYPE::LISTSXP)).unwrap();
        page.metadata().activate(0, false).unwrap();
        let pointer = page.raw_slot(0).unwrap();
        let _registration = register_node_page(&page);
        let forged = std::ptr::without_provenance_mut(pointer.addr());
        // SAFETY: this genuine projection belongs to the live owned Cell.
        unsafe { (*pointer).data.listsxp.cdrval = pointer };
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
            // SAFETY: the page and genuine cyclic edge remain live; the
            // worklist must recover provenance before this header read.
            let children = unsafe { snapshot_children(&node, worklist.context()) }.unwrap();
            assert!(children.into_edges().any(|child| child == pointer));
            // SAFETY: canonical projection recovery must preserve this
            // original live alias, rather than invalidate its pointer tag.
            unsafe {
                assert_eq!((*pointer).sxpinfo.type_of(), SEXPTYPE::LISTSXP);
                (*pointer).data.listsxp.carval = pointer;
            }
        }
    }
}
