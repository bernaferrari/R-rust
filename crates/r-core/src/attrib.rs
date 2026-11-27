//! Attribute-chain access exactly as GNU's attrib.c semantics — the
//! pairlist-walking lookup and the class helpers every downstream
//! consumer (printing, dispatch, coercion) shares.

use crate::heap::Heap;
use crate::id::ValueId;
use crate::intern::SymbolInterner;
use crate::object::RObject;

pub fn get_attrib(
    heap: &Heap,
    interner: &SymbolInterner,
    obj: ValueId,
    attr_name: &str,
) -> Option<ValueId> {
    let mut cursor = heap.get(obj)?.attrib;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            return None;
        }
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => {
                if let Some(RObject::Symbol { name, .. }) = heap.object(*tag) {
                    if interner.symbol_name(*name) == attr_name {
                        return Some(*car);
                    }
                }
                cursor = *cdr;
            }
            _ => return None,
        }
    }
    None
}

/// The names attribute as plain strings — slot ordering for the S4
/// deparser exactly as the attribute carries it.
pub fn attrib_names(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<String> {
    get_attrib(heap, interner, value, "names")
        .map(|id| string_vec(heap, interner, id))
        .unwrap_or_default()
}

/// Read one named attribute off an object's slot attribute chain — the
/// pairlist walk every attribute access shares. Attribute chains are
/// pairlist nodes whose tags are the attribute name symbols.
/// Cross-module attribute access for the deparser — same reading as the
/// internal getter.
pub fn get_attrib_public(heap: &Heap, interner: &SymbolInterner, value: ValueId, name: &str) -> Option<ValueId> {
    get_attrib(heap, interner, value, name)
}

/// print.rs reaches the same extraction through this crate-visible alias.
pub fn string_vec_pub(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<String> {
    string_vec(heap, interner, value)
}

pub fn string_vec(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<String> {
    match heap.object(value) {
        Some(RObject::String(values)) => values
            .iter()
            .map(|v| interner.string_text(*v).to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether the object carries the Date class.
pub fn is_date_classed(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    get_attrib(heap, interner, value, "class")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "Date")),
            _ => None,
        })
        .unwrap_or(false)
}

/// Whether the object carries the POSIXct class (first of the two-element
/// POSIXct/POSIXt class vector exactly as GNU).
pub fn is_posixct_classed(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    get_attrib(heap, interner, value, "class")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "POSIXct")),
            _ => None,
        })
        .unwrap_or(false)
}
