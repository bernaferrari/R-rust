//! Interop bridge from the legacy SEXP object world into the `ValueId` heap.
//!
//! The bridge is the first piece of migration infrastructure for the
//! SEXP-to-ValueId transition: it imports legacy objects into the new heap
//! while preserving object identity through a memoized bijection keyed on
//! the legacy pointer identity of each source object.
//!
//! # Liveness contract
//!
//! Imported objects are deep copies: mutating the legacy source after import
//! does not affect the imported object, and vice versa. The memo table is
//! keyed by pointer identity, so callers must keep every imported source
//! object reachable for the lifetime of the bridge; a freed-and-reused
//! address would otherwise alias unrelated objects. Migration slices hold
//! their source graphs alive while the bridge exists, so this contract is
//! discharged structurally rather than by runtime bookkeeping.
//!
//! # NA representation
//!
//! Atomic NA values use the GNU R raw representations already shared by both
//! heaps: `NA_INTEGER` is `i32::MIN`, NA_real is the reserved bit pattern,
//! and both pass through the copy unchanged. NA string elements have no
//! representation in the `ValueId` object model yet; they import as a
//! NUL-prefixed sentinel that cannot collide with valid UTF-8 user data.

use std::collections::HashMap;

use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::object::Sexp;
use crate::value::heap::Heap;
use crate::value::id::ValueId;
use crate::value::intern::SymbolInterner;
use crate::value::object::{RComplex, RObject};

/// Stateful importer from the legacy world into a `ValueId` heap.
///
/// One bridge owns one memo table; importing the same source object twice
/// yields the same `ValueId`, which is what preserves sharing and cyclic
/// structure across a migrated object graph.
pub(crate) struct ValueBridge {
    forward: HashMap<usize, ValueId>,
}

/// What the bridge found at the source, for callers that need to
/// distinguish an unsupported type from a dangling pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportError {
    /// The source object type has no bridge conversion yet.
    UnsupportedType(SEXPTYPE),
    /// The source payload could not be read in full.
    TruncatedPayload,
}

impl ValueBridge {
    pub(crate) fn new() -> ValueBridge {
        ValueBridge {
            forward: HashMap::new(),
        }
    }

    /// Number of distinct legacy objects imported so far.
    #[allow(dead_code)]
    pub(crate) fn imported_count(&self) -> usize {
        self.forward.len()
    }

    /// Already-imported identity for a source object, if any.
    #[allow(dead_code)]
    pub(crate) fn lookup(&self, sexp: &Sexp<'_>) -> Option<ValueId> {
        self.forward.get(&sexp_key(sexp)).copied()
    }

    /// Import an atomic vector or scalar object into the heap.
    ///
    /// Handles NILSXP, LGLSXP, INTSXP, REALSXP, CPLXSXP, STRSXP, RAWSXP,
    /// LISTSXP, and LANGSXP. Closures, environments, and other reference
    /// types are rejected until each migration slice extends the conversion.
    #[allow(dead_code)]
    pub(crate) fn import_atomic(
        &mut self,
        heap: &mut Heap,
        interner: &mut SymbolInterner,
        sexp: &Sexp<'_>,
    ) -> Result<ValueId, ImportError> {
        self.import(heap, interner, sexp)
    }

    /// Import any supported object, recursing through pairlist and language
    /// node edges. Cyclic structures import correctly: the node's `ValueId`
    /// is registered before its edges are converted, so a back-edge resolves
    /// to the already-registered identity.
    #[allow(dead_code)]
    pub(crate) fn import(
        &mut self,
        heap: &mut Heap,
        interner: &mut SymbolInterner,
        sexp: &Sexp<'_>,
    ) -> Result<ValueId, ImportError> {
        let key = sexp_key(sexp);
        if let Some(existing) = self.forward.get(&key) {
            return Ok(*existing);
        }
        // Well-known singletons map to their reserved sentinels rather than
        // heap objects. Besides matching the new object model, this is what
        // keeps closure import tractable: a closure's environment edge ends
        // at the global-environment sentinel instead of dragging the entire
        // session object graph into the heap.
        if let Some(sentinel) = singleton_sentinel(sexp) {
            return Ok(sentinel);
        }
        // NIL is a singleton sentinel in both worlds: it maps to the
        // reserved `ValueId::NIL` instead of allocating a heap object.
        if sexp.typeof_() == SEXPTYPE::NILSXP {
            return Ok(ValueId::NIL);
        }
        match sexp.typeof_() {
            SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => {
                // List and expression vectors pre-register with empty element
                // storage, then fill in element identities recursively —
                // elements can reference the list itself through nesting.
                let len = sexp.len();
                let is_expression = sexp.typeof_() == SEXPTYPE::EXPRSXP;
                let placeholder = if is_expression {
                    RObject::Expression(Vec::new())
                } else {
                    RObject::List(Vec::new())
                };
                let id = heap.alloc(placeholder);
                self.forward.insert(key, id);

                let mut elements = Vec::with_capacity(len as usize);
                for i in 0..len {
                    let element = sexp
                        .try_vector_elt(i)
                        .map_err(|_| ImportError::TruncatedPayload)?;
                    elements.push(self.import(heap, interner, &element)?);
                }

                let patched = if is_expression {
                    RObject::Expression(elements)
                } else {
                    RObject::List(elements)
                };
                match heap.object_mut(id) {
                    Some(slot) => *slot = patched,
                    None => return Err(ImportError::TruncatedPayload),
                }
                self.attach_attributes(heap, interner, sexp, id)?;
                Ok(id)
            }
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => {
                // Pre-register with empty edges so cycles terminate, then
                // convert the actual edges and patch the slot in place.
                let is_language = sexp.typeof_() == SEXPTYPE::LANGSXP;
                let placeholder = if is_language {
                    RObject::Language {
                        car: ValueId::NIL,
                        cdr: ValueId::NIL,
                        tag: ValueId::NIL,
                    }
                } else {
                    RObject::Pairlist {
                        car: ValueId::NIL,
                        cdr: ValueId::NIL,
                        tag: ValueId::NIL,
                    }
                };
                let id = heap.alloc(placeholder);
                self.forward.insert(key, id);

                let car = self.import_edge(heap, interner, sexp.try_car().ok())?;
                let cdr = self.import_edge(heap, interner, sexp.try_cdr().ok())?;
                let tag = self.import_edge(heap, interner, sexp.try_tag().ok())?;

                let patched = if is_language {
                    RObject::Language { car, cdr, tag }
                } else {
                    RObject::Pairlist { car, cdr, tag }
                };
                match heap.object_mut(id) {
                    Some(slot) => *slot = patched,
                    None => return Err(ImportError::TruncatedPayload),
                }
                self.attach_attributes(heap, interner, sexp, id)?;
                Ok(id)
            }
            SEXPTYPE::ENVSXP => {
                // Environments pre-register empty: enclosure chains and
                // binding values reference their own environment constantly
                // in real object graphs, and the memoized identity is what
                // makes those cycles terminate.
                let id = heap.alloc(RObject::Environment {
                    frame: crate::value::object::EnvFrame::Empty,
                    enclos: ValueId::NIL,
                });
                self.forward.insert(key, id);

                // The legacy frame is a pairlist of binding cells: each
                // cell's tag is the binding symbol, its car the bound value.
                let mut entries = Vec::new();
                if let Ok(frame) = sexp.try_frame() {
                    let mut cursor = Some(frame);
                    while let Some(cell) = cursor {
                        if cell.typeof_() != SEXPTYPE::LISTSXP {
                            break;
                        }
                        let sym_id = cell
                            .try_tag()
                            .ok()
                            .filter(|tag| tag.typeof_() == SEXPTYPE::SYMSXP)
                            .and_then(|tag| tag.try_printname().ok())
                            .and_then(|pname| pname.as_string())
                            .map(|text| interner.intern_symbol(&text));
                        let value = cell
                            .try_car()
                            .ok()
                            .map(|car| self.import(heap, interner, &car))
                            .transpose()?
                            .unwrap_or(ValueId::NIL);
                        if let Some(sym_id) = sym_id {
                            entries.push((sym_id, crate::value::object::Binding::plain(value)));
                        }
                        cursor = cell.try_cdr().ok().filter(|rest| {
                            rest.typeof_() == SEXPTYPE::LISTSXP
                        });
                    }
                }

                let enclos = match sexp.enclos() {
                    Some(parent) if parent.typeof_() != SEXPTYPE::NILSXP => {
                        self.import(heap, interner, &parent)?
                    }
                    _ => ValueId::NIL,
                };

                let frame = if entries.is_empty() {
                    crate::value::object::EnvFrame::Empty
                } else {
                    crate::value::object::EnvFrame::List(entries)
                };
                match heap.object_mut(id) {
                    Some(slot @ RObject::Environment { .. }) => {
                        *slot = RObject::Environment { frame, enclos };
                    }
                    _ => return Err(ImportError::TruncatedPayload),
                }
                self.attach_attributes(heap, interner, sexp, id)?;
                Ok(id)
            }
            SEXPTYPE::CLOSXP => {
                // Closures pre-register: a closure's environment edge nearly
                // always references the environment the closure was defined
                // in, which commonly contains the closure itself.
                let id = heap.alloc(RObject::Closure {
                    formals: ValueId::NIL,
                    body: ValueId::NIL,
                    env: ValueId::NIL,
                });
                self.forward.insert(key, id);

                let formals = match sexp.formals() {
                    Some(f) if f.typeof_() != SEXPTYPE::NILSXP => {
                        self.import(heap, interner, &f)?
                    }
                    _ => ValueId::NIL,
                };
                let body = match sexp.body() {
                    Some(b) if b.typeof_() != SEXPTYPE::NILSXP => {
                        self.import(heap, interner, &b)?
                    }
                    _ => ValueId::NIL,
                };
                let env = match sexp.cloenv() {
                    Some(e) if e.typeof_() != SEXPTYPE::NILSXP => {
                        self.import(heap, interner, &e)?
                    }
                    _ => ValueId::NIL,
                };

                match heap.object_mut(id) {
                    Some(slot @ RObject::Closure { .. }) => {
                        *slot = RObject::Closure { formals, body, env };
                    }
                    _ => return Err(ImportError::TruncatedPayload),
                }
                self.attach_attributes(heap, interner, sexp, id)?;
                Ok(id)
            }
            SEXPTYPE::SYMSXP => {
                // Symbols pre-register like pairlists: an unbound symbol's
                // value edge can be the symbol itself, and the memoized
                // identity is what makes that cycle terminate.
                let id = heap.alloc(RObject::Symbol {
                    name: crate::value::intern::SymId(0),
                    value: ValueId::NIL,
                    internal: ValueId::NIL,
                });
                self.forward.insert(key, id);

                let name_text = sexp
                    .try_printname()
                    .ok()
                    .and_then(|pname| pname.as_string())
                    .ok_or(ImportError::TruncatedPayload)?;
                let name = interner.intern_symbol(&name_text);

                // Sentinel edges (unbound, missing) resolve through the
                // singleton mapping inside the recursive import.
                let value = match sexp.symvalue() {
                    Some(target) if target.typeof_() != SEXPTYPE::NILSXP => {
                        self.import(heap, interner, &target)?
                    }
                    _ => ValueId::NIL,
                };

                match heap.object_mut(id) {
                    Some(slot @ RObject::Symbol { .. }) => {
                        *slot = RObject::Symbol {
                            name,
                            value,
                            internal: ValueId::NIL,
                        };
                    }
                    _ => return Err(ImportError::TruncatedPayload),
                }
                self.attach_attributes(heap, interner, sexp, id)?;
                Ok(id)
            }
            _ => {
                let body = convert_atomic(interner, sexp)?;
                let id = heap.alloc(body);
                self.forward.insert(key, id);
                self.attach_attributes(heap, interner, sexp, id)?;
                Ok(id)
            }
        }
    }

    /// Import the attribute chain of a source object and attach it to the
    /// already-imported heap object. Attribute-less objects keep
    /// `ValueId::NIL` in the slot's attribute edge.
    fn attach_attributes(
        &mut self,
        heap: &mut Heap,
        interner: &mut SymbolInterner,
        sexp: &Sexp<'_>,
        id: ValueId,
    ) -> Result<(), ImportError> {
        let attrib = match sexp.try_attrib() {
            Ok(attrib) if attrib.typeof_() != SEXPTYPE::NILSXP => {
                self.import(heap, interner, &attrib)?
            }
            _ => ValueId::NIL,
        };
        match heap.get_mut(id) {
            Some(slot) => {
                slot.attrib = attrib;
                Ok(())
            }
            None => Err(ImportError::TruncatedPayload),
        }
    }

    /// Import one edge of a reference type. Missing edges (the end of a
    /// pairlist chain, an untagged cell) map to `ValueId::NIL`.
    fn import_edge(
        &mut self,
        heap: &mut Heap,
        interner: &mut SymbolInterner,
        edge: Option<Sexp<'_>>,
    ) -> Result<ValueId, ImportError> {
        match edge {
            Some(target) if target.typeof_() != SEXPTYPE::NILSXP => {
                self.import(heap, interner, &target)
            }
            _ => Ok(ValueId::NIL),
        }
    }
}

fn convert_atomic(
    interner: &mut SymbolInterner,
    sexp: &Sexp<'_>,
) -> Result<RObject, ImportError> {
    let kind = sexp.typeof_();
    let len = sexp.len();
    match kind {
        SEXPTYPE::NILSXP => Ok(RObject::Nil),
        SEXPTYPE::LGLSXP => {
            let mut values = Vec::with_capacity(len as usize);
            for i in 0..len {
                values.push(
                    sexp.try_logical_elt(i)
                        .map_err(|_| ImportError::TruncatedPayload)?,
                );
            }
            Ok(RObject::Logical(values))
        }
        SEXPTYPE::INTSXP => {
            let mut values = Vec::with_capacity(len as usize);
            for i in 0..len {
                values.push(
                    sexp.try_integer_elt(i)
                        .map_err(|_| ImportError::TruncatedPayload)?,
                );
            }
            Ok(RObject::Integer(values))
        }
        SEXPTYPE::REALSXP => {
            let mut values = Vec::with_capacity(len as usize);
            for i in 0..len {
                values.push(
                    sexp.try_real_elt(i)
                        .map_err(|_| ImportError::TruncatedPayload)?,
                );
            }
            Ok(RObject::Real(values))
        }
        SEXPTYPE::CPLXSXP => {
            let mut values = Vec::with_capacity(len as usize);
            for i in 0..len {
                let value = sexp
                    .try_complex_elt(i)
                    .map_err(|_| ImportError::TruncatedPayload)?;
                values.push(RComplex {
                    r: value.r,
                    i: value.i,
                });
            }
            Ok(RObject::Complex(values))
        }
        SEXPTYPE::RAWSXP => {
            let mut values = Vec::with_capacity(len as usize);
            for i in 0..len {
                values.push(
                    sexp.try_raw_elt(i)
                        .map_err(|_| ImportError::TruncatedPayload)?,
                );
            }
            Ok(RObject::Raw(values))
        }
        SEXPTYPE::STRSXP => {
            let mut values = Vec::with_capacity(len as usize);
            for i in 0..len {
                let element = sexp
                    .try_string_value_elt(i)
                    .map_err(|_| ImportError::TruncatedPayload)?;
                // NA string elements import as a collision-free sentinel:
                // NUL bytes are not valid inside R character data.
                let text = element.unwrap_or_else(|| "\0NA".to_string());
                values.push(interner.intern_string(&text));
            }
            Ok(RObject::String(values))
        }
        other => Err(ImportError::UnsupportedType(other)),
    }
}

/// Map the legacy singleton objects onto their reserved `ValueId`
/// sentinels. Pointer identity is exact: these singletons are unique
/// objects in the legacy world.
fn singleton_sentinel(sexp: &Sexp<'_>) -> Option<ValueId> {
    use crate::sexp::globals;
    let raw = sexp.as_raw();
    // Ordered by frequency in typical object graphs: nil first.
    let pairs: [(crate::sexp::ffi::SEXP, ValueId); 5] = [
        (globals::R_NilValue(), ValueId::NIL),
        (globals::R_MissingArg(), ValueId::MISSING),
        (globals::R_UnboundValue(), ValueId::UNBOUND),
        (globals::R_GlobalEnv(), ValueId::GLOBAL_ENV),
        (globals::R_BaseEnv(), ValueId::BASE_ENV),
    ];
    // R_EmptyEnv is not universally present in every harness; resolve it
    // separately so a missing singleton never masks the others.
    if raw == globals::R_EmptyEnv() {
        return Some(ValueId::EMPTY_ENV);
    }
    pairs
        .iter()
        .find(|(singleton, _)| *singleton == raw)
        .map(|(_, sentinel)| *sentinel)
}

/// Stable identity key for a legacy object: its pointer identity, which is
/// what object identity means everywhere in the legacy world.
fn sexp_key(sexp: &Sexp<'_>) -> usize {
    sexp.as_raw() as usize
}

#[cfg(test)]
#[allow(deprecated)] // test scaffolding fills vectors through the translation shims
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    /// Allocate a real vector and fill it through the translation shims.
    /// Raw allocation happens before wrapping so the arena borrow ends before
    /// the lifetime-bound `Sexp` is constructed — callers can then allocate
    /// further vectors while earlier handles stay live.
    fn real_vector<'a>(arena: &'a mut crate::sexp::memory::RArena, data: &[f64]) -> Sexp<'a> {
        let raw = arena.alloc_vector(SEXPTYPE::REALSXP, data.len() as crate::sexp::ffi::R_xlen_t);
        assert!(!raw.is_null());
        fill_real_vector(arena, raw, data)
    }

    fn fill_real_vector<'a>(
        arena: &'a crate::sexp::memory::RArena,
        raw: crate::sexp::ffi::SEXP,
        data: &[f64],
    ) -> Sexp<'a> {
        let sexp = Sexp::from_arena_raw(raw, arena).expect("live vector");
        for (i, value) in data.iter().enumerate() {
            assert!(sexp
                .clone()
                .try_set_real_elt(i as crate::sexp::ffi::R_xlen_t, *value)
                .is_ok());
        }
        sexp
    }

    #[test]
    fn import_real_vector_copies_payload() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            let mut arena = crate::sexp::memory::RArena::new();

            let sexp = real_vector(&mut arena, &[10.0, 11.0, 12.0]);
            let id = bridge
                .import_atomic(&mut heap, &mut interner, &sexp)
                .expect("real vectors import");
            match heap.object(id) {
                Some(RObject::Real(values)) => {
                    assert_eq!(values.as_slice(), &[10.0, 11.0, 12.0]);
                }
                other => panic!("expected real vector, got {other:?}"),
            }
        });
    }

    #[test]
    fn import_is_memoized_by_identity() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            let mut arena = crate::sexp::memory::RArena::new();

            let sexp = real_vector(&mut arena, &[7.0]);
            let first = bridge
                .import_atomic(&mut heap, &mut interner, &sexp)
                .expect("first import");
            let second = bridge
                .import_atomic(&mut heap, &mut interner, &sexp)
                .expect("second import");
            assert_eq!(first, second, "same source must yield same ValueId");
            assert_eq!(bridge.imported_count(), 1);
        });
    }

    #[test]
    fn distinct_sources_get_distinct_ids() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            let mut arena = crate::sexp::memory::RArena::new();

            let left_raw = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            let right_raw = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            let left = fill_real_vector(&arena, left_raw, &[1.0]);
            let right = fill_real_vector(&arena, right_raw, &[2.0]);
            let left_id = bridge
                .import_atomic(&mut heap, &mut interner, &left)
                .expect("left imports");
            let right_id = bridge
                .import_atomic(&mut heap, &mut interner, &right)
                .expect("right imports");
            assert_ne!(left_id, right_id);
            assert_eq!(bridge.imported_count(), 2);
        });
    }

    #[test]
    fn unsupported_reference_types_are_rejected() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            let mut arena = crate::sexp::memory::RArena::new();

            let raw = arena.alloc_node(SEXPTYPE::PROMSXP);
            let sexp = Sexp::from_arena_raw(raw, &arena).expect("live promise");
            match bridge.import_atomic(&mut heap, &mut interner, &sexp) {
                Err(ImportError::UnsupportedType(SEXPTYPE::PROMSXP)) => {}
                other => panic!("promises must be rejected for now, got {other:?}"),
            }
        });
    }

    #[test]
    fn import_preserves_na_bit_patterns() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            let mut arena = crate::sexp::memory::RArena::new();

            let raw = arena.alloc_vector(SEXPTYPE::INTSXP, 2);
            let sexp = Sexp::from_arena_raw(raw, &arena).expect("live vector");
            assert!(sexp.clone().try_set_integer_elt(0, 42).is_ok());
            assert!(sexp
                .clone()
                .try_set_integer_elt(1, crate::sexp::ffi::NA_INTEGER)
                .is_ok());

            let id = bridge
                .import_atomic(&mut heap, &mut interner, &sexp)
                .expect("integer vectors import");
            match heap.object(id) {
                Some(RObject::Integer(values)) => {
                    assert_eq!(values[0], 42);
                    assert_eq!(values[1], crate::sexp::ffi::NA_INTEGER);
                }
                other => panic!("expected integer vector, got {other:?}"),
            }
        });
    }
    #[test]
    fn import_pairlist_chain_preserves_shape() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            let mut arena = crate::sexp::memory::RArena::new();

            let chain = arena.alloc_list_chain(3);
            let sexp = Sexp::from_arena_raw(chain, &arena).expect("live chain");
            let id = bridge
                .import(&mut heap, &mut interner, &sexp)
                .expect("pairlist chains import");

            // Walk the imported chain: three pairlist cells, then NIL.
            let mut depth = 0;
            let mut cursor = id;
            loop {
                match heap.object(cursor) {
                    Some(RObject::Pairlist { cdr, .. }) if *cdr != ValueId::NIL => {
                        depth += 1;
                        cursor = *cdr;
                    }
                    Some(RObject::Pairlist { cdr, .. }) => {
                        assert_eq!(*cdr, ValueId::NIL);
                        depth += 1;
                        break;
                    }
                    other => panic!("expected pairlist cell, got {other:?}"),
                }
            }
            assert_eq!(depth, 3, "chain depth must survive the import");
        });
    }

    #[test]
    fn import_pairlist_cycle_terminates_and_preserves_backedge() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            let mut arena = crate::sexp::memory::RArena::new();

            let chain = arena.alloc_list_chain(2);
            // Close the cycle: second cell's cdr points back at the first.
            let second = unsafe { crate::sexp::accessors::CDR(chain) };
            assert!(!second.is_null());
            unsafe { crate::sexp::accessors::SETCDR(second, chain) };

            let sexp = Sexp::from_arena_raw(chain, &arena).expect("live cycle");
            let id = bridge
                .import(&mut heap, &mut interner, &sexp)
                .expect("cyclic chains import without diverging");

            let second_id = match heap.object(id) {
                Some(RObject::Pairlist { cdr, .. }) => *cdr,
                other => panic!("expected pairlist cell, got {other:?}"),
            };
            match heap.object(second_id) {
                Some(RObject::Pairlist { cdr, .. }) => {
                    assert_eq!(
                        *cdr, id,
                        "the back-edge must resolve to the first cell's identity"
                    );
                }
                other => panic!("expected second pairlist cell, got {other:?}"),
            }
        });
    }

    #[test]
    fn import_pairlist_with_vector_car_preserves_payload() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            let mut arena = crate::sexp::memory::RArena::new();

            let vector_raw = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            let chain = arena.alloc_list_chain(1);
            unsafe { crate::sexp::accessors::SETCAR(chain, vector_raw) };
            let vector = fill_real_vector(&arena, vector_raw, &[99.0]);
            let sexp = Sexp::from_arena_raw(chain, &arena).expect("live chain");

            let id = bridge
                .import(&mut heap, &mut interner, &sexp)
                .expect("chain imports");
            let car_id = match heap.object(id) {
                Some(RObject::Pairlist { car, .. }) => *car,
                other => panic!("expected pairlist cell, got {other:?}"),
            };
            match heap.object(car_id) {
                Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[99.0]),
                other => panic!("expected real vector car, got {other:?}"),
            }
            drop(vector);
        });
    }
    #[test]
    fn import_symbol_captures_printname() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();

            let raw = crate::sexp::symbol::install_str("bridgeprobe");
            assert!(!raw.is_null());
            Sexp::with_registered_raw(raw, |sexp| {
                assert_eq!(sexp.typeof_(), SEXPTYPE::SYMSXP);
                let id = bridge
                    .import(&mut heap, &mut interner, sexp)
                    .expect("symbols import");
                match heap.object(id) {
                    Some(RObject::Symbol { name, .. }) => {
                        assert_eq!(interner.symbol_name(*name), "bridgeprobe");
                    }
                    other => panic!("expected symbol, got {other:?}"),
                }
            })
            .expect("registered symbol");
        });
    }

    #[test]
    fn import_attaches_attribute_chain() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            let mut arena = crate::sexp::memory::RArena::new();

            let vector_raw = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            let attrib_chain = arena.alloc_list_chain(1);
            unsafe { crate::sexp::accessors::SET_ATTRIB(vector_raw, attrib_chain) };
            let vector = fill_real_vector(&arena, vector_raw, &[5.0]);

            let id = bridge
                .import(&mut heap, &mut interner, &vector)
                .expect("vector imports");
            let attrib_id = heap.get(id).map(|slot| slot.attrib);
            assert!(
                matches!(attrib_id, Some(a) if a != ValueId::NIL),
                "attribute chain must be attached, got {attrib_id:?}"
            );
            assert!(
                matches!(heap.object(attrib_id.unwrap()), Some(RObject::Pairlist { .. })),
                "attributes import as their pairlist representation"
            );
        });
    }
    #[test]
    fn import_list_vector_preserves_elements() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            let mut arena = crate::sexp::memory::RArena::new();

            let list_raw = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
            let element_raw = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            unsafe {
                crate::sexp::accessors::SET_VECTOR_ELT(list_raw, 0, element_raw);
                crate::sexp::accessors::SET_VECTOR_ELT(
                    list_raw,
                    1,
                    crate::sexp::globals::R_NilValue(),
                );
            }
            let element = fill_real_vector(&arena, element_raw, &[42.0]);
            let list = Sexp::from_arena_raw(list_raw, &arena).expect("live list");

            let id = bridge
                .import(&mut heap, &mut interner, &list)
                .expect("list vectors import");
            match heap.object(id) {
                Some(RObject::List(elements)) => {
                    assert_eq!(elements.len(), 2);
                    match heap.object(elements[0]) {
                        Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[42.0]),
                        other => panic!("expected real element, got {other:?}"),
                    }
                    assert_eq!(elements[1], ValueId::NIL);
                }
                other => panic!("expected list vector, got {other:?}"),
            }
            drop(element);
        });
    }
    #[test]
    fn import_environment_preserves_bindings_and_enclosure() {
        let session = crate::sexp::session::RSession::new();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();

            // parent env (empty) <- child env with binding x = 3.5, all in
            // the instance heap where cross-references are natural.
            let parent = unsafe {
                crate::sexp::memory_ext::NewEnvironment(
                    std::ptr::null_mut(),
                    crate::sexp::globals::R_NilValue(),
                    std::ptr::null_mut(),
                )
            };
            let child = unsafe {
                crate::sexp::memory_ext::NewEnvironment(
                    std::ptr::null_mut(),
                    parent,
                    std::ptr::null_mut(),
                )
            };
            assert!(!parent.is_null() && !child.is_null());

            let sym = crate::sexp::symbol::install_str("x");
            let value = crate::sexp::constructors::Rf_ScalarReal(3.5);
            unsafe { crate::sexp::envir::defineVar(sym, value, child) };

            crate::sexp::object::Sexp::with_registered_raw(child, |child| {
                let id = bridge
                    .import(&mut heap, &mut interner, child)
                    .expect("environments import");
                match heap.object(id) {
                    Some(RObject::Environment { frame, enclos }) => {
                        let sym_id = interner.intern_symbol("x");
                        let bound = frame.get(sym_id).expect("binding x survives");
                        match heap.object(bound) {
                            Some(RObject::Real(values)) => {
                                assert_eq!(values.as_slice(), &[3.5])
                            }
                            other => panic!("expected real binding, got {other:?}"),
                        }
                        assert!(
                            matches!(
                                heap.object(*enclos),
                                Some(RObject::Environment { .. })
                            ),
                            "enclosure imports as an environment"
                        );
                    }
                    other => panic!("expected environment, got {other:?}"),
                }
            })
            .expect("registered env");
        });
    }

    #[test]
    fn import_environment_cycle_through_enclosure_terminates() {
        let session = crate::sexp::session::RSession::new();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();

            // env that is its own enclosure — degenerate but legal, and the
            // memoized pre-registration is the only thing that terminates it.
            let env = unsafe {
                crate::sexp::memory_ext::NewEnvironment(
                    std::ptr::null_mut(),
                    crate::sexp::globals::R_NilValue(),
                    std::ptr::null_mut(),
                )
            };
            assert!(!env.is_null());
            unsafe { crate::sexp::accessors::SET_ENCLOS(env, env) };

            crate::sexp::object::Sexp::with_registered_raw(env, |env_sexp| {
                let id = bridge
                    .import(&mut heap, &mut interner, env_sexp)
                    .expect("self-enclosing environments import");
                match heap.object(id) {
                    Some(RObject::Environment { enclos, .. }) => {
                        assert_eq!(
                            *enclos, id,
                            "the enclosure cycle must resolve to the env's own identity"
                        );
                    }
                    other => panic!("expected environment, got {other:?}"),
                }
            })
            .expect("registered env");
        });
    }
    #[test]
    fn import_closure_preserves_formals_body_and_env() {
        let mut session = crate::sexp::session::RSession::new();
        let mut heap = Heap::new();
        let mut interner = SymbolInterner::new();
        let mut bridge = ValueBridge::new();

        let (result, _output, _visible) =
            session.eval_code_with_output_capture("function(x) x + 1");
        let closure = result.expect("closure expression evaluates");
        assert_eq!(closure.typeof_(), SEXPTYPE::CLOSXP);

        let id = bridge
            .import(&mut heap, &mut interner, &closure)
            .expect("closures import");
        match heap.object(id) {
            Some(RObject::Closure { formals, body, env }) => {
                assert_ne!(*formals, ValueId::NIL, "formals survive");
                assert_ne!(*body, ValueId::NIL, "body survives");
                // Top-level eval creates closures in the global environment,
                // which imports as the reserved sentinel rather than dragging
                // the whole session graph into the heap.
                assert_eq!(
                    *env,
                    ValueId::GLOBAL_ENV,
                    "the closure environment maps to the global-env sentinel"
                );
                // the formals pairlist should tag its single cell with x
                match heap.object(*formals) {
                    Some(RObject::Pairlist { tag, .. }) => {
                        assert_ne!(*tag, ValueId::NIL, "formal parameters carry names");
                    }
                    other => panic!("expected formals pairlist, got {other:?}"),
                }
            }
            other => panic!("expected closure, got {other:?}"),
        }
    }
}
