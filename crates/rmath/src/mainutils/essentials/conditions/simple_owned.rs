#![forbid(unsafe_code)]
//! A simple condition retains each selected value through every allocation.
use crate::sexp::{
    ffi::SEXPTYPE,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};

pub(super) fn build(
    access: &RuntimeAccess,
    message: &Sexp<'_>,
    call: &Sexp<'_>,
    names_symbol: &Sexp<'_>,
    class_symbol: &Sexp<'_>,
    classes: &[&str],
) -> SexpResult<Sexp<'static>> {
    access.require_active()?;
    let domain = access.domain();
    for value in [message, call, names_symbol, class_symbol] {
        domain.link(value)?;
    }
    let allocator = access.allocator(&domain)?;
    let mut result = SexpMut::try_from_checked(
        allocator.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 2)))?,
    )?;
    result.try_set_vector_elt(0, message.clone())?;
    result.try_set_vector_elt(1, call.clone())?;
    let names = allocator.strings(&["message", "call"])?;
    let classes = allocator.strings(classes)?;
    let attributes = allocator.pairlist_cell(&classes, &domain.nil(), class_symbol)?;
    let attributes = allocator.pairlist_cell(&names, &attributes, names_symbol)?;
    result.try_set_attribute(&attributes)?;
    let result = result.freeze();
    let node = result.allocation()?;
    let heap = node.heap_identity();
    let mut header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
    header.sxpinfo.set_obj(true);
    heap.replace_node(node, header)
        .ok_or(SexpError::StaleAllocation)?;
    access.require_active()?;
    result.into_owned()
}
