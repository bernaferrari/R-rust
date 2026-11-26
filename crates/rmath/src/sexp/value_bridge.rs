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
            SEXPTYPE::PROMSXP => {
                // Promises carry three slots — expression, environment, and
                // the forced value (unbound until evaluation). All three
                // cross; forcing then happens in the new evaluator over the
                // imported expression graph, exactly the migration semantics.
                let id = heap.alloc(RObject::Promise {
                    expr: ValueId::NIL,
                    env: ValueId::NIL,
                    value: ValueId::UNBOUND,
                });
                self.forward.insert(key, id);
                let expr = match sexp.try_prcode() {
                    Ok(e) if e.typeof_() != SEXPTYPE::NILSXP => {
                        self.import(heap, interner, &e)?
                    }
                    _ => ValueId::NIL,
                };
                let env = match sexp.try_prenv() {
                    Ok(e) if e.typeof_() != SEXPTYPE::NILSXP => {
                        self.import(heap, interner, &e)?
                    }
                    _ => ValueId::NIL,
                };
                let value = match sexp.try_prvalue() {
                    Ok(v) => {
                        if v.as_raw() == crate::sexp::globals::R_UnboundValue() {
                            ValueId::UNBOUND
                        } else {
                            self.import(heap, interner, &v)?
                        }
                    }
                    _ => ValueId::UNBOUND,
                };
                match heap.object_mut(id) {
                    Some(slot @ RObject::Promise { .. }) => {
                        *slot = RObject::Promise { expr, env, value };
                    }
                    _ => return Err(ImportError::TruncatedPayload),
                }
                self.attach_attributes(heap, interner, sexp, id)?;
                Ok(id)
            }
            SEXPTYPE::BUILTINSXP | SEXPTYPE::SPECIALSXP => {
                // Legacy primitives map onto the new primitive registry by
                // their print name. The scaffolding table covers the
                // operator set the migration has reached; everything else is
                // genuinely unsupported until its primitive lands.
                let name = primitive_name_of(sexp).ok_or(ImportError::TruncatedPayload)?;
                let id = crate::value::primitives::primitive_id_for_name(&name)
                    .ok_or(ImportError::UnsupportedType(sexp.typeof_()))?;
                let body = RObject::Primitive(crate::value::object::BuiltinId(id));
                let id = heap.alloc(body);
                self.forward.insert(key, id);
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

/// Read a legacy primitive's print name through the same `PRIMNAME`
/// accessor the legacy dispatch uses.
fn primitive_name_of(sexp: &Sexp<'_>) -> Option<String> {
    let name = crate::mainutils::relop::PRIMNAME(sexp.as_raw());
    if name.is_null() {
        return None;
    }
    // SAFETY: PRIMNAME of a live primitive yields a NUL-terminated static
    // name valid for the borrow.
    let text = unsafe { std::ffi::CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

use crate::sexp::ffi::SEXP;

/// What the exporter found in the heap, for callers that need to distinguish
/// an unmigratable object from a structural gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExportError {
    /// The object variant has no export conversion yet.
    UnsupportedObject(&'static str),
    /// A reference edge points at a missing heap slot.
    DanglingEdge,
}

/// Stateful exporter from the `ValueId` heap back into the legacy SEXP world.
///
/// The reversed companion of [`ValueBridge`]: object identity survives
/// through a `ValueId`-keyed memo, and reference types pre-register their
/// skeleton before recursing so cyclic graphs terminate with correct
/// back-edges — the mirror image of the import discipline.
///
/// # Session and protection contract
///
/// Every export allocates in the legacy instance heap, so calls must happen
/// inside an active session. Objects under construction stay protected from
/// allocation until they are patched into an already-protected parent; the
/// returned root object is the caller's responsibility, exactly the contract
/// every legacy constructor documents.
pub(crate) struct ExportBridge {
    reverse: HashMap<ValueId, SEXP>,
}

impl ExportBridge {
    pub(crate) fn new() -> ExportBridge {
        ExportBridge {
            reverse: HashMap::new(),
        }
    }

    /// Already-exported identity for a heap object, if any.
    #[allow(dead_code)]
    pub(crate) fn lookup(&self, value: ValueId) -> Option<SEXP> {
        self.reverse.get(&value).copied()
    }

    /// Export any supported object into the legacy world. The result is a
    /// raw SEXP owned by the legacy instance heap. The slot's attribute edge
    /// crosses with the object: attributes carry names, dimensions, and
    /// classes, so dropping them would silently corrupt round-trips.
    pub(crate) fn export(
        &mut self,
        heap: &Heap,
        interner: &SymbolInterner,
        value: ValueId,
    ) -> Result<SEXP, ExportError> {
        // Memoized re-export: attributes were attached on the first crossing.
        if self.reverse.contains_key(&value) {
            return self.export_inner(heap, interner, value);
        }
        let sexp = self.export_inner(heap, interner, value)?;
        let attrib = heap
            .get(value)
            .map(|slot| slot.attrib)
            .unwrap_or(ValueId::NIL);
        if attrib != ValueId::NIL {
            let exported_attrib = self.export(heap, interner, attrib)?;
            crate::sexp::accessors::SET_ATTRIB(sexp, exported_attrib);
        }
        Ok(sexp)
    }

    /// The type-dispatching export body — object identity and structure,
    /// without the attribute edge the public entry wraps around it.
    fn export_inner(
        &mut self,
        heap: &Heap,
        interner: &SymbolInterner,
        value: ValueId,
    ) -> Result<SEXP, ExportError> {
        use crate::sexp::globals::{
            R_BaseEnv, R_EmptyEnv, R_GlobalEnv, R_MissingArg, R_NilValue, R_UnboundValue,
        };
        // Legacy singletons round-trip exactly: the sentinel design reserves
        // these identities in both worlds.
        if value == ValueId::NIL {
            return Ok(R_NilValue());
        }
        if value == ValueId::MISSING {
            return Ok(R_MissingArg());
        }
        if value == ValueId::UNBOUND {
            return Ok(R_UnboundValue());
        }
        if value == ValueId::EMPTY_ENV {
            return Ok(R_EmptyEnv());
        }
        if value == ValueId::BASE_ENV {
            return Ok(R_BaseEnv());
        }
        if value == ValueId::GLOBAL_ENV {
            return Ok(R_GlobalEnv());
        }
        if let Some(existing) = self.reverse.get(&value) {
            return Ok(*existing);
        }
        let object = heap.object(value).ok_or(ExportError::DanglingEdge)?;
        match object {
            RObject::Nil => Ok(R_NilValue()),
            RObject::Logical(values) => {
                let values = values.clone();
                self.export_atomic(heap, value, SEXPTYPE::LGLSXP, values.len(), |sexp, i| {
                    crate::sexp::accessors::SET_LOGICAL_ELT(sexp, i as i32, values[i]);
                })
            }
            RObject::Integer(values) => {
                let values = values.clone();
                self.export_atomic(heap, value, SEXPTYPE::INTSXP, values.len(), |sexp, i| {
                    crate::sexp::accessors::SET_INTEGER_ELT(sexp, i as i32, values[i]);
                })
            }
            RObject::Real(values) => {
                let values = values.clone();
                self.export_atomic(heap, value, SEXPTYPE::REALSXP, values.len(), |sexp, i| {
                    crate::sexp::accessors::SET_REAL_ELT(sexp, i as i32, values[i]);
                })
            }
            RObject::Raw(values) => {
                let values = values.clone();
                self.export_atomic(heap, value, SEXPTYPE::RAWSXP, values.len(), |sexp, i| {
                    crate::sexp::accessors::SET_RAW_ELT(sexp, i as i32, values[i]);
                })
            }
            RObject::Complex(values) => {
                let values = values.clone();
                self.export_atomic(heap, value, SEXPTYPE::CPLXSXP, values.len(), |sexp, i| {
                    crate::sexp::accessors::SET_COMPLEX_ELT(
                        sexp,
                        i as i32,
                        crate::sexp::ffi::Rcomplex {
                            r: values[i].r,
                            i: values[i].i,
                        },
                    );
                })
            }
            RObject::String(values) => {
                let values = values.clone();
                let vector = crate::sexp::constructors::Rf_allocVector(
                    SEXPTYPE::STRSXP,
                    values.len() as i32,
                );
                let guard = crate::sexp::protect::protect(vector);
                self.reverse.insert(value, vector);
                for (i, element) in values.iter().enumerate() {
                    let text = interner.string_text(*element);
                    // The NUL-prefixed sentinel reverses to the NA string —
                    // exactly the collision-free encoding the importer chose.
                    let charsxp = if let Some(rest) = text.strip_prefix('\0') {
                        debug_assert_eq!(rest, "NA", "only the NA sentinel carries NUL");
                        crate::sexp::globals::R_NaString()
                    } else {
                        crate::sexp::constructors::mk_char_str(text)
                    };
                    crate::sexp::accessors::SET_STRING_ELT(vector, i as i64, charsxp);
                }
                drop(guard);
                Ok(vector)
            }
            RObject::List(items) => {
                let items = items.clone();
                let vector = crate::sexp::constructors::Rf_allocVector(
                    SEXPTYPE::VECSXP,
                    items.len() as i32,
                );
                let guard = crate::sexp::protect::protect(vector);
                self.reverse.insert(value, vector);
                for (i, item) in items.iter().enumerate() {
                    let exported = self.export(heap, interner, *item)?;
                    crate::sexp::accessors::SET_VECTOR_ELT(vector, i as i64, exported);
                }
                drop(guard);
                Ok(vector)
            }
            RObject::Pairlist { .. } | RObject::Language { .. } => {
                self.export_cons_chain(heap, interner, value)
            }
            RObject::Symbol { name, .. } => {
                let symbol = crate::sexp::symbol::install_str(interner.symbol_name(*name));
                self.reverse.insert(value, symbol);
                Ok(symbol)
            }
            RObject::Environment { .. } => self.export_environment(heap, interner, value),
            RObject::Closure { .. } => self.export_closure(heap, interner, value),
            RObject::Promise { .. } => self.export_promise(heap, interner, value),
            RObject::Primitive(_) => Err(ExportError::UnsupportedObject("primitive")),
            _ => Err(ExportError::UnsupportedObject("unhandled variant")),
        }
    }

    /// Allocate an atomic vector, memoize it, and populate elements through
    /// the element setter — the shared spine of the atomic exports.
    fn export_atomic(
        &mut self,
        heap: &Heap,
        value: ValueId,
        sexptype: SEXPTYPE,
        len: usize,
        fill: impl Fn(SEXP, usize),
    ) -> Result<SEXP, ExportError> {
        let _ = heap;
        let vector = crate::sexp::constructors::Rf_allocVector(sexptype, len as i32);
        // Population writes no references, so protection is unnecessary here —
        // but the memo must be in place before any caller recurses back.
        self.reverse.insert(value, vector);
        for i in 0..len {
            fill(vector, i);
        }
        Ok(vector)
    }

    /// Export a pairlist or language chain cell-by-cell. Each cons allocates
    /// with NIL edges, enters the memo immediately, then patches — cyclic
    /// chains terminate through the memo exactly as in the import direction.
    fn export_cons_chain(
        &mut self,
        heap: &Heap,
        interner: &SymbolInterner,
        value: ValueId,
    ) -> Result<SEXP, ExportError> {
        use crate::sexp::accessors::{SETCAR, SETCDR, SETTAG, SET_TYPEOF};
        use crate::sexp::globals::R_NilValue;
        let is_language = matches!(heap.object(value), Some(RObject::Language { .. }));
        let skeleton = crate::sexp::constructors::Rf_cons(R_NilValue(), R_NilValue());
        let guard = crate::sexp::protect::protect(skeleton);
        self.reverse.insert(value, skeleton);
        if is_language {
            SET_TYPEOF(skeleton, SEXPTYPE::LANGSXP.as_c_int());
        }
        let (car, cdr, tag) = match heap.object(value) {
            Some(RObject::Pairlist { car, cdr, tag }) | Some(RObject::Language { car, cdr, tag }) => {
                (*car, *cdr, *tag)
            }
            _ => return Err(ExportError::DanglingEdge),
        };
        if car != ValueId::NIL {
            let exported = self.export(heap, interner, car)?;
            SETCAR(skeleton, exported);
        }
        if tag != ValueId::NIL {
            let exported = self.export(heap, interner, tag)?;
            SETTAG(skeleton, exported);
        }
        if cdr != ValueId::NIL {
            let exported = self.export(heap, interner, cdr)?;
            SETCDR(skeleton, exported);
        }
        drop(guard);
        Ok(skeleton)
    }

    /// Export an environment: enclosure first (GNU's acyclic enclosure
    /// invariant makes plain recursion safe), then bindings via defineVar.
    /// The environment enters the memo before its bindings export, so a
    /// binding that references its own environment resolves correctly.
    fn export_environment(
        &mut self,
        heap: &Heap,
        interner: &SymbolInterner,
        value: ValueId,
    ) -> Result<SEXP, ExportError> {
        use crate::sexp::globals::{R_EmptyEnv, R_NilValue};
        let (frame, enclos) = match heap.object(value) {
            Some(RObject::Environment { frame, enclos }) => (frame.clone(), *enclos),
            _ => return Err(ExportError::DanglingEdge),
        };
        let enclosure = if enclos == ValueId::EMPTY_ENV || enclos == ValueId::NIL {
            R_EmptyEnv()
        } else {
            self.export(heap, interner, enclos)?
        };
        let environment = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            enclosure,
            std::ptr::null_mut(),
        );
        let guard = crate::sexp::protect::protect(environment);
        self.reverse.insert(value, environment);
        if let crate::value::object::EnvFrame::List(bindings) = frame {
            for (name, binding) in bindings {
                let symbol = crate::sexp::symbol::install_str(interner.symbol_name(*name));
                let exported = self.export(heap, interner, binding.value)?;
                crate::sexp::envir::defineVar(symbol, exported, environment);
            }
        }
        drop(guard);
        Ok(environment)
    }

    /// Export a promise with the same skeleton-and-patch discipline as
    /// closures: the expression edge can reference the environment the
    /// promise belongs to, cycling back through bindings.
    fn export_promise(
        &mut self,
        heap: &Heap,
        interner: &SymbolInterner,
        value: ValueId,
    ) -> Result<SEXP, ExportError> {
        use crate::sexp::accessors::{SET_PRCODE, SET_PRENV, SET_PRVALUE};
        use crate::sexp::globals::{R_NilValue, R_UnboundValue};
        let (expr, env, forced) = match heap.object(value) {
            Some(RObject::Promise { expr, env, value }) => (*expr, *env, *value),
            _ => return Err(ExportError::DanglingEdge),
        };
        let skeleton = crate::sexp::memory_ext::allocSExp(SEXPTYPE::PROMSXP);
        let guard = crate::sexp::protect::protect(skeleton);
        self.reverse.insert(value, skeleton);
        SET_PRCODE(skeleton, R_NilValue());
        SET_PRENV(skeleton, R_NilValue());
        SET_PRVALUE(skeleton, R_UnboundValue());
        let expr_sexp = self.export(heap, interner, expr)?;
        SET_PRCODE(skeleton, expr_sexp);
        let env_sexp = self.export(heap, interner, env)?;
        SET_PRENV(skeleton, env_sexp);
        if forced != ValueId::UNBOUND {
            let forced_sexp = self.export(heap, interner, forced)?;
            SET_PRVALUE(skeleton, forced_sexp);
        }
        drop(guard);
        Ok(skeleton)
    }

    /// Export a closure through skeleton-and-patch pre-registration. The
    /// memo insertion happens before any edge exports because the canonical
    /// recursion shape — a closure inside its own captured environment —
    /// cycles back through the environment's bindings; without the early
    /// memo the export infinite-loops on exactly the functions import
    /// handles correctly.
    fn export_closure(
        &mut self,
        heap: &Heap,
        interner: &SymbolInterner,
        value: ValueId,
    ) -> Result<SEXP, ExportError> {
        use crate::sexp::accessors::{SET_BODY, SET_CLOENV, SET_FORMALS};
        use crate::sexp::globals::R_NilValue;
        let (formals, body, env) = match heap.object(value) {
            Some(RObject::Closure { formals, body, env }) => (*formals, *body, *env),
            _ => return Err(ExportError::DanglingEdge),
        };
        let skeleton = crate::sexp::memory_ext::allocSExp(SEXPTYPE::CLOSXP);
        let guard = crate::sexp::protect::protect(skeleton);
        self.reverse.insert(value, skeleton);
        SET_FORMALS(skeleton, R_NilValue());
        SET_BODY(skeleton, R_NilValue());
        SET_CLOENV(skeleton, R_NilValue());
        let formals_sexp = self.export(heap, interner, formals)?;
        SET_FORMALS(skeleton, formals_sexp);
        let body_sexp = self.export(heap, interner, body)?;
        SET_BODY(skeleton, body_sexp);
        let env_sexp = self.export(heap, interner, env)?;
        SET_CLOENV(skeleton, env_sexp);
        drop(guard);
        Ok(skeleton)
    }
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

            let raw = arena.alloc_node(SEXPTYPE::WEAKREFSXP);
            let sexp = Sexp::from_arena_raw(raw, &arena).expect("live weakref");
            match bridge.import_atomic(&mut heap, &mut interner, &sexp) {
                Err(ImportError::UnsupportedType(SEXPTYPE::WEAKREFSXP)) => {}
                other => panic!("weak references must be rejected for now, got {other:?}"),
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
    /// Bootstrap a new-world environment with the migrated primitives bound
    /// to their symbol names — what the future base-environment migration
    /// produces wholesale.
    fn bootstrap_env(
        heap: &mut crate::value::heap::Heap,
        interner: &mut crate::value::intern::SymbolInterner,
    ) -> crate::value::id::ValueId {
        use crate::value::env;
        use crate::value::object::BuiltinId;
        use crate::value::primitives::primitive_id_for_name;
        let env_id = env::new_environment(heap, crate::value::id::ValueId::EMPTY_ENV);
        for &name in crate::value::primitives::primitive_names() {
            let sym = interner.intern_symbol(name);
            let prim = primitive_id_for_name(name).expect("mapped primitive");
            let object = heap.alloc(RObject::Primitive(BuiltinId(prim)));
            env::define(heap, env_id, sym, object).expect("define primitive");
        }
        env_id
    }

    #[test]
    fn convergence_parsed_program_evaluates_in_both_worlds() {
        let mut session = crate::sexp::session::RSession::new();
        let mut heap = Heap::new();
        let mut interner = SymbolInterner::new();
        let mut bridge = ValueBridge::new();

        let expressions = session.with_active(|| {
            crate::eval::parser::parse_active_expressions("1 + 2")
                .expect("source parses")
        });
        assert_eq!(expressions.len(), 1);

        // import the parsed call tree into the new heap
        let imported = bridge
            .import(&mut heap, &mut interner, &expressions[0])
            .expect("call tree imports");

        // evaluate in the new world against the bootstrap environment
        let env_id = bootstrap_env(&mut heap, &mut interner);
        let new_world = crate::value::eval::eval(&mut heap, &mut interner, imported, env_id)
            .expect("new-world evaluation succeeds");
        match heap.object(new_world) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[3.0]),
            other => panic!("expected 3.0 in the new world, got {other:?}"),
        }

        // the legacy world agrees
        let (legacy, _output, _visible) = session.eval_code_with_output_capture("1 + 2");
        let legacy = legacy.expect("legacy evaluation succeeds");
        assert_eq!(legacy.try_real_elt(0).expect("legacy value"), 3.0);
    }

    #[test]
    fn convergence_compound_program_with_assignment() {
        let mut session = crate::sexp::session::RSession::new();
        let mut heap = Heap::new();
        let mut interner = SymbolInterner::new();
        let mut bridge = ValueBridge::new();

        // `{ x <- 10; x + 5 }` — assignment through the environment, then
        // symbol resolution feeding arithmetic, all in the new world.
        let expressions = session.with_active(|| {
            crate::eval::parser::parse_active_expressions("{ x <- 10; x + 5 }")
                .expect("source parses")
        });
        let imported = bridge
            .import(&mut heap, &mut interner, &expressions[0])
            .expect("program imports");
        let env_id = bootstrap_env(&mut heap, &mut interner);
        let result = crate::value::eval::eval(&mut heap, &mut interner, imported, env_id)
            .expect("new-world program runs");
        match heap.object(result) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[15.0]),
            other => panic!("expected 15.0, got {other:?}"),
        }
    }
    #[test]
    fn convergence_user_closure_definition_and_call() {
        let mut session = crate::sexp::session::RSession::new();
        let mut heap = Heap::new();
        let mut interner = SymbolInterner::new();
        let mut bridge = ValueBridge::new();

        // { f <- function(x) x * 2; f(21) } — defines and applies a closure
        // entirely in the new world: closure construction, lazy argument
        // promises, body evaluation in the application frame.
        let expressions = session.with_active(|| {
            crate::eval::parser::parse_active_expressions("{ f <- function(x) x * 2; f(21) }")
                .expect("source parses")
        });
        let imported = bridge
            .import(&mut heap, &mut interner, &expressions[0])
            .expect("program imports");
        let env_id = bootstrap_env(&mut heap, &mut interner);
        let result = crate::value::eval::eval(&mut heap, &mut interner, imported, env_id);
        match result {
            Ok(value) => match heap.object(value) {
                Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[42.0]),
                other => panic!("expected 42.0, got {other:?}"),
            },
            Err(error) => panic!("closure program should run in the new world: {error:?}"),
        }
    }

    #[test]
    fn convergence_recursive_closure() {
        let mut session = crate::sexp::session::RSession::new();
        let mut heap = Heap::new();
        let mut interner = SymbolInterner::new();
        let mut bridge = ValueBridge::new();

        // fib <- function(n) if (n < 2) n else fib(n-1) + fib(n-2); fib(10)
        // — recursion through the application frame's own environment: the
        // closure looks itself up by name in its definition environment.
        let expressions = session.with_active(|| {
            crate::eval::parser::parse_active_expressions(
                "{ fib <- function(n) if (n < 2) n else fib(n-1) + fib(n-2); fib(10) }",
            )
            .expect("source parses")
        });
        let imported = bridge
            .import(&mut heap, &mut interner, &expressions[0])
            .expect("program imports");
        let env_id = bootstrap_env(&mut heap, &mut interner);
        let result = crate::value::eval::eval(&mut heap, &mut interner, imported, env_id);
        match result {
            Ok(value) => match heap.object(value) {
                Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[55.0]),
                other => panic!("expected 55.0, got {other:?}"),
            },
            Err(error) => panic!("recursive program should run in the new world: {error:?}"),
        }
    }

    /// Shared harness: parse, import, evaluate in the new world.
    fn eval_in_new_world(source: &str) -> (Heap, SymbolInterner, crate::value::eval::EvalResult) {
        let mut session = crate::sexp::session::RSession::new();
        let mut heap = Heap::new();
        let mut interner = SymbolInterner::new();
        let mut bridge = ValueBridge::new();
        let expressions = session.with_active(|| {
            crate::eval::parser::parse_active_expressions(source).expect("source parses")
        });
        let imported = bridge
            .import(&mut heap, &mut interner, &expressions[0])
            .expect("program imports");
        let env_id = bootstrap_env(&mut heap, &mut interner);
        let result = crate::value::eval::eval(&mut heap, &mut interner, imported, env_id);
        (heap, interner, result)
    }

    fn expect_reals(heap: &Heap, result: crate::value::eval::EvalResult, expected: &[f64]) {
        match result {
            Ok(value) => match heap.object(value) {
                Some(RObject::Real(values)) => assert_eq!(values.as_slice(), expected),
                Some(RObject::Integer(values)) => assert_eq!(
                    values.iter().map(|v| *v as f64).collect::<Vec<_>>().as_slice(),
                    expected
                ),
                other => panic!("expected {expected:?}, got {other:?}"),
            },
            Err(error) => panic!("program should run in the new world: {error:?}"),
        }
    }

    fn expect_integers(heap: &Heap, result: crate::value::eval::EvalResult, expected: &[i32]) {
        match result {
            Ok(value) => match heap.object(value) {
                Some(RObject::Integer(values)) => assert_eq!(values.as_slice(), expected),
                other => panic!("expected {expected:?}, got {other:?}"),
            },
            Err(error) => panic!("program should run in the new world: {error:?}"),
        }
    }

    #[test]
    fn convergence_concat_and_subset() {
        // c() concatenation, real-literal indexing with truncation.
        let (heap, _interner, result) = eval_in_new_world("{ x <- c(1, 2, 3); x[2] }");
        expect_reals(&heap, result, &[2.0]);
    }

    #[test]
    fn convergence_colon_sequence_and_length() {
        // 1:10 yields an integer sequence; length reports element count.
        let (heap, _interner, result) = eval_in_new_world("{ x <- 1:10; length(x) }");
        expect_integers(&heap, result, &[10]);
    }

    #[test]
    fn convergence_colon_elements_indexed() {
        // Integer sequence indexed by integer-valued real: GNU truncation.
        let (heap, _interner, result) = eval_in_new_world("{ x <- 1:10; x[3] + x[7] }");
        expect_integers(&heap, result, &[10]);
    }

    #[test]
    fn convergence_logical_recycling_subset() {
        // Logical indices recycle across the vector: positions 1, 3, 5.
        let (heap, _interner, result) = eval_in_new_world("{ x <- 1:5; x[c(TRUE, FALSE)] }");
        expect_integers(&heap, result, &[1, 3, 5]);
    }

    #[test]
    fn convergence_negative_exclusion_subset() {
        // Negative indices exclude: position 2 drops out.
        let (heap, _interner, result) = eval_in_new_world("{ x <- 1:5; x[-2] }");
        expect_integers(&heap, result, &[1, 3, 4, 5]);
    }

    #[test]
    fn convergence_out_of_range_yields_na() {
        // Positive out-of-range indexing yields the NA element, GNU exactly.
        let (heap, _interner, result) = eval_in_new_world("{ x <- 1:3; x[5] }");
        expect_integers(&heap, result, &[crate::value::object::NA_INTEGER]);
    }

    #[test]
    fn convergence_concat_coercion_to_real() {
        // Mixed integer/logical operands coerce upward through c().
        let (heap, _interner, result) = eval_in_new_world("{ x <- c(1:2, 3.5); x[3] }");
        expect_reals(&heap, result, &[3.5]);
    }

    #[test]
    fn import_promise_slots_cross() {
        // A promise built in the legacy world imports with all three slots
        // intact: expression, environment, and the unbound forced-value slot.
        let mut session = crate::sexp::session::RSession::new();
        session.with_active(|| {
            use crate::sexp::accessors::{SET_PRCODE, SET_PRENV, SET_PRVALUE};
            use crate::sexp::globals::{R_NilValue, R_UnboundValue};
            let promise = crate::sexp::memory_ext::allocSExp(SEXPTYPE::PROMSXP);
            let guard = crate::sexp::protect::protect(promise);
            let expr = crate::sexp::constructors::Rf_ScalarReal(99.0);
            SET_PRCODE(promise, expr);
            SET_PRENV(promise, crate::sexp::globals::R_EmptyEnv());
            SET_PRVALUE(promise, R_UnboundValue());
            let _ = R_NilValue();
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            Sexp::with_registered_raw(promise, |sexp| {
                let id = bridge
                    .import(&mut heap, &mut interner, sexp)
                    .expect("promises import");
                match heap.object(id) {
                    Some(RObject::Promise { expr, env, value }) => {
                        match heap.object(*expr) {
                            Some(RObject::Real(values)) => {
                                assert_eq!(values.as_slice(), &[99.0])
                            }
                            other => panic!("expected 99.0 expr, got {other:?}"),
                        }
                        assert_eq!(*value, ValueId::UNBOUND, "unforced promises stay unbound");
                        let _ = env;
                    }
                    other => panic!("expected promise, got {other:?}"),
                }
            });
            drop(guard);
        });
    }

    #[test]
    fn export_promise_round_trips_slots() {
        // The new-world promise crosses back: expression and environment
        // slots land in the right places, the unbound value sentinel too.
        let mut session = crate::sexp::session::RSession::new();
        session.with_active(|| {
            let mut heap = Heap::new();
            let interner = SymbolInterner::new();
            let mut exporter = ExportBridge::new();
            let expr = heap.alloc(RObject::Real(vec![7.0]));
            let env_id = crate::value::env::new_environment(&mut heap, ValueId::EMPTY_ENV);
            let promise_id = heap.alloc(RObject::Promise {
                expr,
                env: env_id,
                value: ValueId::UNBOUND,
            });
            let raw = exporter
                .export(&heap, &interner, promise_id)
                .expect("promise exports");
            use crate::sexp::accessors::{PRCODE, PRENV, PRVALUE};
            let prvalue = PRVALUE(raw);
            assert_eq!(prvalue, crate::sexp::globals::R_UnboundValue(), "unforced stays unbound");
            let code = PRCODE(raw);
            assert!(!code.is_null(), "expression slot populated");
            Sexp::with_registered_raw(code, |sexp| {
                assert_eq!(sexp.try_real_elt(0).expect("expr value"), 7.0);
            });
            let env = PRENV(raw);
            assert!(!env.is_null(), "environment slot populated");
        });
    }

    /// Extract a numeric vector's elements from the legacy world as doubles,
    /// NA-pattern-preserving so cross-world comparison is bit-exact.
    fn legacy_as_doubles(sexp: &Sexp<'_>) -> Vec<f64> {
        let n = sexp.len();
        let mut out = Vec::new();
        for i in 0..n {
            let v = if let Ok(x) = sexp.try_real_elt(i) {
                x
            } else if let Ok(x) = sexp.try_integer_elt(i) {
                if x == crate::value::object::NA_INTEGER {
                    f64::from_bits(crate::value::object::NA_REAL_BITS)
                } else {
                    x as f64
                }
            } else if let Ok(x) = sexp.try_logical_elt(i) {
                x as f64
            } else {
                panic!("legacy element {i} is not numeric");
            };
            out.push(v);
        }
        out
    }

    /// Extract a string vector's elements from the legacy world.
    fn legacy_as_strings(sexp: &Sexp<'_>) -> Option<Vec<String>> {
        if sexp.typeof_() != SEXPTYPE::STRSXP {
            return None;
        }
        let n = sexp.len();
        let mut out = Vec::new();
        for i in 0..n {
            out.push(
                sexp.try_string_value_elt(i)
                    .expect("string element")
                    .unwrap_or_else(|| "<NA>".to_string()),
            );
        }
        Some(out)
    }

    /// Extract a numeric vector's elements from the new world as doubles.
    fn new_world_as_doubles(heap: &Heap, result: crate::value::eval::EvalResult) -> Vec<f64> {
        match result {
            Ok(value) => match heap.object(value) {
                Some(RObject::Real(values)) => values.clone(),
                Some(RObject::Integer(values)) => values
                    .iter()
                    .map(|v| {
                        if *v == crate::value::object::NA_INTEGER {
                            f64::from_bits(crate::value::object::NA_REAL_BITS)
                        } else {
                            *v as f64
                        }
                    })
                    .collect(),
                Some(RObject::Logical(values)) => values.iter().map(|v| *v as f64).collect(),
                other => panic!("new-world result is not numeric: {other:?}"),
            },
            Err(error) => panic!("new-world evaluation failed: {error:?}"),
        }
    }

    #[test]
    fn oracle_cross_world_numeric_programs() {
        let mut session = crate::sexp::session::RSession::new();
        let programs = [
            "{ x <- c(1, 2, 3); x[2] }",
            "{ x <- 1:5; x[c(TRUE, FALSE)] }",
            "{ x <- 1:5; x[-2] }",
            "{ x <- 1:10; x[3] + x[7] }",
            "c(1:3, 4.5)",
            "{ total <- 0; x <- 1:100; for (i in 1:100) total <- total + x[i]; total }",
            "{ fib <- function(n) if (n < 2) n else fib(n-1) + fib(n-2); fib(10) }",
            "{ x <- 1:3; length(x) }",
            "{ x <- c(2, 4, 8); x * 2 + 1 }",
            "-(1:5)",
            "c(TRUE, FALSE) & c(TRUE, TRUE)",
            "c(TRUE, FALSE) | c(FALSE, FALSE)",
            "!c(TRUE, FALSE, TRUE)",
            "{ x <- 5; x > 3 && x < 10 }",
            "{ x <- 1; x > 3 || x < 10 }",
            "FALSE && stop(\"boom\")",
            "TRUE || stop(\"boom\")",
            "sum(1:10)",
            "sum(c(1.5, 2.5, 3.0))",
            "prod(1:5)",
            "min(c(3, 1, 4, 1, 5))",
            "max(c(3, 1, 4, 1, 5))",
            "sqrt(c(4, 9, 16))",
            "abs(-(1:3))",
            "exp(0)",
            "log(exp(2))",
            "{ x <- 1:100; sum(x) / length(x) }",
            "{ x <- c(2, 4, 8); sqrt(sum(x^2)) }",
            "{ x <- c(10, 20, 30); x[[2]] }",
            "{ x <- 1:5; x[[3]] + x[[4]] }",
            "{ x <- list(10, 20); x[[1]] + x[[2]] }",
            "{ x <- list(1:3, c(4, 5)); sum(x[[1]]) + sum(x[[2]]) }",
            "paste(\"a\", \"b\")",
            "paste(\"x\", 1:3)",
            "paste0(\"x\", 1:3)",
            "paste(c(\"a\", \"b\"), collapse=\"-\")",
            "paste(\"a\", \"b\", sep=\"/\")",
            "nchar(c(\"ab\", \"c\"))",
            "{ x <- paste(\"v\", 1:5); nchar(x) }",
            "{ x <- 1:5; x[2] <- 99; x }",
            "{ x <- 1:3; x[5] <- 9; x }",
            "{ x <- c(1, 2, 3); x[c(1, 3)] <- c(10, 30); x }",
            "{ x <- 1:5; x[c(TRUE, FALSE)] <- 0; x }",
            "{ x <- 1:4; x[-2] <- c(7, 8, 9); x }",
            "{ x <- 1:3; x[2] <- 1.5; x }",
            "{ total <- numeric(0); total[1] <- 5; total }",
        ];
        // One session, one parse pass: expression objects stay valid for the
        // session's lifetime, so batching avoids per-program bootstrap cost.
        let parsed: Vec<_> = programs
            .iter()
            .map(|source| {
                session
                    .with_active(|| {
                        crate::eval::parser::parse_active_expressions(source)
                            .expect("source parses")
                    })
                    .into_iter()
                    .next()
                    .expect("one expression")
            })
            .collect();
        for (source, expression) in programs.iter().zip(parsed.iter()) {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut bridge = ValueBridge::new();
            let imported = bridge
                .import(&mut heap, &mut interner, expression)
                .expect("program imports");
            let env_id = bootstrap_env(&mut heap, &mut interner);
            let result = crate::value::eval::eval(&mut heap, &mut interner, imported, env_id);
            let (legacy, _output, _visible) = session.eval_code_with_output_capture(source);
            let legacy = legacy.expect("legacy evaluation succeeds");
            // String results compare as text; everything else as doubles.
            if let Some(legacy_strings) = legacy_as_strings(&legacy) {
                let new_strings = match result {
                    Ok(value) => match heap.object(value) {
                        Some(RObject::String(values)) => values
                            .iter()
                            .map(|s| interner.string_text(*s).to_string())
                            .collect::<Vec<_>>(),
                        other => panic!("`{source}`: legacy is character but new world is {other:?}"),
                    },
                    Err(error) => panic!("`{source}` new-world evaluation failed: {error:?}"),
                };
                assert_eq!(new_strings, legacy_strings, "string mismatch for `{source}`");
                continue;
            }
            let new_world = new_world_as_doubles(&heap, result);
            let legacy_world = legacy_as_doubles(&legacy);
            assert_eq!(
                new_world.len(),
                legacy_world.len(),
                "length mismatch for `{source}`: new {new_world:?} vs legacy {legacy_world:?}"
            );
            for (i, (a, b)) in new_world.iter().zip(legacy_world.iter()).enumerate() {
                let both_na = a.is_nan() && b.is_nan();
                assert!(
                    both_na || a == b,
                    "element {i} mismatch for `{source}`: new {a} vs legacy {b}"
                );
            }
        }
    }

    #[test]
    fn logic_na_semantics_new_world() {
        // GNU's three-valued logic, verified against the documented truth
        // table since the oracle extractor conflates NA representation.
        let (heap, _interner, result) = eval_in_new_world("FALSE & NA");
        match result {
            Ok(value) => match heap.object(value) {
                Some(RObject::Logical(values)) => assert_eq!(values.as_slice(), &[0]),
                other => panic!("FALSE & NA should be FALSE, got {other:?}"),
            },
            Err(error) => panic!("eval failed: {error:?}"),
        }
        let (heap, _interner, result) = eval_in_new_world("TRUE | NA");
        match result {
            Ok(value) => match heap.object(value) {
                Some(RObject::Logical(values)) => assert_eq!(values.as_slice(), &[1]),
                other => panic!("TRUE | NA should be TRUE, got {other:?}"),
            },
            Err(error) => panic!("eval failed: {error:?}"),
        }
        let (heap, _interner, result) = eval_in_new_world("TRUE & NA");
        match result {
            Ok(value) => match heap.object(value) {
                Some(RObject::Logical(values)) => {
                    assert_eq!(values.as_slice(), &[crate::value::object::NA_INTEGER])
                }
                other => panic!("TRUE & NA should be NA, got {other:?}"),
            },
            Err(error) => panic!("eval failed: {error:?}"),
        }
    }

    #[test]
    fn export_atomic_vectors_round_trip() {
        let mut session = crate::sexp::session::RSession::new();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut exporter = ExportBridge::new();
            let interner = SymbolInterner::new();
            let id = heap.alloc(RObject::Real(vec![1.5, 2.5, f64::from_bits(crate::value::object::NA_REAL_BITS)]));
            let raw = exporter.export(&heap, &interner, id).expect("reals export");
            Sexp::with_registered_raw(raw, |sexp| {
                assert_eq!(sexp.typeof_(), SEXPTYPE::REALSXP);
                assert_eq!(sexp.len(), 3);
                assert_eq!(sexp.try_real_elt(0).expect("elt"), 1.5);
                assert_eq!(sexp.try_real_elt(1).expect("elt"), 2.5);
                assert!(sexp.try_real_elt(2).expect("elt").is_nan());
            });
            let int_id = heap.alloc(RObject::Integer(vec![7, crate::value::object::NA_INTEGER]));
            let raw = exporter.export(&heap, &interner, int_id).expect("integers export");
            Sexp::with_registered_raw(raw, |sexp| {
                assert_eq!(sexp.typeof_(), SEXPTYPE::INTSXP);
                assert_eq!(sexp.try_integer_elt(0).expect("elt"), 7);
                assert_eq!(sexp.try_integer_elt(1).expect("elt"), crate::value::object::NA_INTEGER);
            });
        });
    }

    #[test]
    fn export_identity_is_stable() {
        let mut session = crate::sexp::session::RSession::new();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut exporter = ExportBridge::new();
            let interner = SymbolInterner::new();
            let id = heap.alloc(RObject::Integer(vec![42]));
            let first = exporter.export(&heap, &interner, id).expect("export");
            let second = exporter.export(&heap, &interner, id).expect("re-export");
            assert_eq!(first, second, "the same ValueId exports to the same SEXP");
        });
    }

    #[test]
    fn export_pairlist_chain_structure() {
        let mut session = crate::sexp::session::RSession::new();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut exporter = ExportBridge::new();
            let interner = SymbolInterner::new();
            // (1 . (2 . nil)) — a two-cell chain with distinct tail.
            let one = heap.alloc(RObject::Integer(vec![1]));
            let two = heap.alloc(RObject::Integer(vec![2]));
            let tail = heap.alloc(RObject::Pairlist { car: two, cdr: ValueId::NIL, tag: ValueId::NIL });
            let head = heap.alloc(RObject::Pairlist { car: one, cdr: tail, tag: ValueId::NIL });
            let raw = exporter.export(&heap, &interner, head).expect("pairlist exports");
            Sexp::with_registered_raw(raw, |sexp| {
                assert_eq!(sexp.typeof_(), SEXPTYPE::LISTSXP);
                let car = sexp.try_car().expect("car");
                assert_eq!(car.try_integer_elt(0).expect("car value"), 1);
                let cdr = sexp.try_cdr().expect("cdr");
                assert_eq!(cdr.typeof_(), SEXPTYPE::LISTSXP);
                let cadr = cdr.try_car().expect("cadr");
                assert_eq!(cadr.try_integer_elt(0).expect("cadr value"), 2);
                let cddr = cdr.try_cdr().expect("cddr");
                assert_eq!(cddr.typeof_(), SEXPTYPE::NILSXP);
            });
        });
    }

    #[test]
    fn export_recursive_closure_in_own_environment_terminates() {
        let mut session = crate::sexp::session::RSession::new();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut exporter = ExportBridge::new();
            // The canonical recursion shape: env binds f to a closure whose
            // captured environment is that same env. Without pre-registration
            // this export infinite-loops.
            let env_id = crate::value::env::new_environment(&mut heap, ValueId::EMPTY_ENV);
            let formals = heap.alloc(RObject::Nil);
            let body = heap.alloc(RObject::Integer(vec![1]));
            let closure = heap.alloc(RObject::Closure { formals, body, env: env_id });
            let f_sym = interner.intern_symbol("f");
            crate::value::env::define(&mut heap, env_id, f_sym, closure).expect("define");
            let raw = exporter.export(&heap, &interner, closure).expect("recursive closure exports");
            Sexp::with_registered_raw(raw, |sexp| {
                assert_eq!(sexp.typeof_(), SEXPTYPE::CLOSXP);
            });
        });
    }

    #[test]
    fn export_environment_bindings_visible() {
        let mut session = crate::sexp::session::RSession::new();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut exporter = ExportBridge::new();
            let env_id = crate::value::env::new_environment(&mut heap, ValueId::EMPTY_ENV);
            let answer = heap.alloc(RObject::Real(vec![42.0]));
            let sym = interner.intern_symbol("answer");
            crate::value::env::define(&mut heap, env_id, sym, answer).expect("define");
            let raw = exporter.export(&heap, &interner, env_id).expect("environment exports");
            Sexp::with_registered_raw(raw, |env_sexp| {
                assert_eq!(env_sexp.typeof_(), SEXPTYPE::ENVSXP);
                // Full round-trip: re-import the exported environment and
                // verify the binding survives both crossings.
                let mut heap2 = Heap::new();
                let mut interner2 = SymbolInterner::new();
                let mut bridge = ValueBridge::new();
                let reimported = bridge
                    .import(&mut heap2, &mut interner2, env_sexp)
                    .expect("re-import succeeds");
                let sym2 = interner2.intern_symbol("answer");
                let (_scope, binding) = crate::value::env::lookup(&heap2, reimported, sym2)
                    .expect("binding visible after round-trip");
                match heap2.object(binding.value) {
                    Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[42.0]),
                    other => panic!("expected 42.0 after round-trip, got {other:?}"),
                }
            });
        });
    }

    #[test]
    fn export_string_na_round_trips() {
        let mut session = crate::sexp::session::RSession::new();
        session.with_active(|| {
            let mut heap = Heap::new();
            let mut interner = SymbolInterner::new();
            let mut exporter = ExportBridge::new();
            let hello = interner.intern_string("hello");
            let na = interner.intern_string("\0NA");
            let id = heap.alloc(RObject::String(vec![hello, na]));
            let raw = exporter.export(&heap, &interner, id).expect("strings export");
            Sexp::with_registered_raw(raw, |sexp| {
                assert_eq!(sexp.typeof_(), SEXPTYPE::STRSXP);
                assert_eq!(sexp.len(), 2);
                // Element 1 must round-trip to the NA string sentinel, not text.
                let elt = crate::sexp::accessors::STRING_ELT(raw, 1);
                assert_eq!(elt, crate::sexp::globals::R_NaString(), "NA string round-trips");
            });
        });
    }

    #[test]
    fn interop_capstone_compute_new_world_consume_legacy_world() {
        // The migration use case end-to-end: a program evaluates in the
        // ValueId world, its result crosses back through the export bridge,
        // and the legacy world reads the exact value — the interop pattern
        // every consumer migration slice will follow.
        let mut session = crate::sexp::session::RSession::new();
        let mut heap = Heap::new();
        let mut interner = SymbolInterner::new();
        let mut bridge = ValueBridge::new();
        let expressions = session.with_active(|| {
            crate::eval::parser::parse_active_expressions(
                "{ x <- 1:10; c(x[3], x[7], x[3] * x[7]) }",
            )
            .expect("source parses")
        });
        let imported = bridge
            .import(&mut heap, &mut interner, &expressions[0])
            .expect("program imports");
        let env_id = bootstrap_env(&mut heap, &mut interner);
        let result = crate::value::eval::eval(&mut heap, &mut interner, imported, env_id)
            .expect("new-world evaluation succeeds");

        session.with_active(|| {
            let mut exporter = ExportBridge::new();
            let raw = exporter
                .export(&heap, &interner, result)
                .expect("result exports to the legacy world");
            Sexp::with_registered_raw(raw, |sexp| {
                // The legacy world sees exactly c(3, 7, 21) — integer-typed
                // through the colon sequence semantics.
                assert_eq!(sexp.len(), 3);
                assert_eq!(sexp.try_integer_elt(0).expect("first"), 3);
                assert_eq!(sexp.try_integer_elt(1).expect("second"), 7);
                assert_eq!(sexp.try_integer_elt(2).expect("third"), 21);
            });
        });
    }

    #[test]
    fn export_attributes_round_trip_names() {
        // Attributes carry the object's metadata — names here. The crossing
        // must preserve them in both directions or data silently corrupts.
        let mut session = crate::sexp::session::RSession::new();
        let mut heap = Heap::new();
        let mut interner = SymbolInterner::new();
        let mut bridge = ValueBridge::new();
        let expressions = session.with_active(|| {
            crate::eval::parser::parse_active_expressions("c(a=1, b=2)").expect("source parses")
        });
        // The call itself isn't what we import — evaluate it in the legacy
        // world so a real attributed vector exists, then import that.
        let (legacy, _output, _visible) = session.eval_code_with_output_capture("c(a=1, b=2)");
        let legacy = legacy.expect("legacy evaluation succeeds");
        let _ = expressions;
        let imported = bridge
            .import(&mut heap, &mut interner, &legacy)
            .expect("attributed vector imports");
        assert_ne!(
            heap.get(imported).map(|slot| slot.attrib).unwrap_or(ValueId::NIL),
            ValueId::NIL,
            "import must attach the names attribute edge"
        );
        session.with_active(|| {
            let mut exporter = ExportBridge::new();
            let raw = exporter
                .export(&heap, &interner, imported)
                .expect("export succeeds");
            Sexp::with_registered_raw(raw, |sexp| {
                let attrib = sexp.try_attrib().expect("attributes survive the crossing");
                assert_ne!(
                    attrib.typeof_(),
                    SEXPTYPE::NILSXP,
                    "names attribute must survive the round-trip"
                );
            });
        });
    }

    #[test]
    fn short_circuit_skips_rhs_evaluation() {
        // The right operand never evaluates when the left decides: a side
        // effect that would error is silently skipped, GNU exactly.
        let (heap, _interner, result) =
            eval_in_new_world("{ x <- 0; f <- function() { x <<- 99; TRUE }; FALSE && f(); x }");
        expect_reals(&heap, result, &[0.0]);
        let (heap, _interner, result) =
            eval_in_new_world("{ x <- 0; f <- function() { x <<- 99; TRUE }; TRUE || f(); x }");
        expect_reals(&heap, result, &[0.0]);
        // And when the left does not decide, the right does run.
        let (heap, _interner, result) =
            eval_in_new_world("{ x <- 0; f <- function() { x <<- 99; TRUE }; TRUE && f(); x }");
        expect_reals(&heap, result, &[99.0]);
    }

    #[test]
    fn convergence_loop_with_indexing() {
        // The data primitives compose with control flow: sum 1:100 by loop.
        let (heap, _interner, result) = eval_in_new_world(
            "{ total <- 0; x <- 1:100; for (i in 1:100) total <- total + x[i]; total }",
        );
        // `total <- 0` binds a real literal, so GNU accumulates in real.
        expect_reals(&heap, result, &[5050.0]);
    }
}
