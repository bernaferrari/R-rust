#![forbid(unsafe_code)]
//! Typed object payloads and heap slot layout.

use super::id::ValueId;
use super::intern::{InternedStr, SymId};
use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

/// Complex number payload matching R's `Rcomplex`.
/// The integer NA sentinel, matching GNU's `NA_INTEGER` exactly so both
/// representations agree bit-for-bit during the migration.
pub const NA_INTEGER: i32 = i32::MIN;

/// The real NA bit pattern, matching GNU's `NA_real` exactly. NA is a
/// specific quiet NaN payload, distinct from both NaN and Inf.
pub const NA_REAL_BITS: u64 = 0x7FF00000000007A2;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RComplex {
    pub r: f64,
    pub i: f64,
}

/// Primitive/builtin identifier — an index into the builtin dispatch table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BuiltinId(pub u32);

/// Compiled bytecode chunk placeholder; the real chunk type arrives with the
/// bytecode VM migration. Reference-counted so closures share one copy.
#[derive(Debug)]
pub struct BytecodeChunk {
    pub code: Vec<u8>,
    pub constants: Vec<ValueId>,
}

/// One environment binding: the bound value plus the GNU binding metadata.
///
/// Binding flags live here — not on the value object — because locking and
/// active-binding status are properties of the binding cell, exactly as in
/// GNU R where they are header bits of the frame's pairlist cell.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub value: ValueId,
    pub locked: bool,
    pub active: bool,
}

impl Binding {
    pub fn plain(value: ValueId) -> Binding {
        Binding {
            value,
            locked: false,
            active: false,
        }
    }
}

/// Environment frame storage: small frames stay in association-list order,
/// large frames hash. Both are safe container types.
#[derive(Debug, Default)]
pub enum EnvFrame {
    #[default]
    Empty,
    List(Vec<(SymId, Binding)>),
    Hash(HashMap<SymId, Binding>),
}

impl EnvFrame {
    pub fn get(&self, sym: SymId) -> Option<ValueId> {
        self.get_binding(sym).map(|binding| binding.value)
    }

    /// Full binding lookup including lock/active metadata — what GNU
    /// environment semantics actually dispatch on.
    pub fn get_binding(&self, sym: SymId) -> Option<Binding> {
        match self {
            EnvFrame::Empty => None,
            EnvFrame::List(entries) => entries
                .iter()
                .find(|(k, _)| *k == sym)
                .map(|(_, binding)| *binding),
            EnvFrame::Hash(map) => map.get(&sym).copied(),
        }
    }

    pub fn insert(&mut self, sym: SymId, value: ValueId) {
        self.insert_binding(sym, Binding::plain(value));
    }

    pub fn insert_binding(&mut self, sym: SymId, binding: Binding) {
        match self {
            EnvFrame::Empty => {
                *self = EnvFrame::List(vec![(sym, binding)]);
            }
            EnvFrame::List(entries) => {
                if let Some(entry) = entries.iter_mut().find(|(k, _)| *k == sym) {
                    entry.1 = binding;
                } else {
                    entries.push((sym, binding));
                }
            }
            EnvFrame::Hash(map) => {
                map.insert(sym, binding);
            }
        }
    }

    pub fn remove(&mut self, sym: SymId) -> Option<ValueId> {
        self.remove_binding(sym).map(|binding| binding.value)
    }

    pub fn remove_binding(&mut self, sym: SymId) -> Option<Binding> {
        match self {
            EnvFrame::Empty => None,
            EnvFrame::List(entries) => entries
                .iter()
                .position(|(k, _)| *k == sym)
                .map(|pos| entries.remove(pos).1),
            EnvFrame::Hash(map) => map.remove(&sym),
        }
    }

    /// Collect every binding edge for GC tracing.
    pub(crate) fn trace_edges(&self, out: &mut Vec<ValueId>) {
        match self {
            EnvFrame::Empty => {}
            EnvFrame::List(entries) => {
                out.extend(entries.iter().map(|(_, binding)| binding.value))
            }
            EnvFrame::Hash(map) => out.extend(map.values().map(|binding| binding.value)),
        }
    }
}

/// Provider trait for ALTREP-style lazy/alternative vector representations.
///
/// Implementations supply materialized element access without the vector ever
/// necessarily existing as contiguous plain data.
pub trait AltrepProvider: std::fmt::Debug {
    /// Vector length in elements.
    fn len(&self) -> usize;
    /// Materialize element `index` as a plain value. The exact typed accessors
    /// grow with the migration; this probe is enough for heap/GC correctness.
    fn elt_real(&self, index: usize) -> Option<f64> {
        let _ = index;
        None
    }
    /// Providers may hold edges to other heap objects (e.g. closure-backed
    /// lazy sequences); the GC traces them through this hook.
    fn trace_edges(&self, _out: &mut Vec<ValueId>) {}
}

/// Typed object payload — the pure-Rust replacement for the union inside
/// `SEXPREC`. Every variant is plain safe Rust data.
#[derive(Debug)]
pub enum RObject {
    Nil,
    Symbol {
        name: SymId,
        value: ValueId,
        internal: ValueId,
    },
    Pairlist {
        car: ValueId,
        cdr: ValueId,
        tag: ValueId,
    },
    Language {
        car: ValueId,
        cdr: ValueId,
        tag: ValueId,
    },
    Closure {
        formals: ValueId,
        body: ValueId,
        env: ValueId,
    },
    Environment {
        frame: EnvFrame,
        enclos: ValueId,
    },
    Promise {
        expr: ValueId,
        env: ValueId,
        value: ValueId,
    },
    Primitive(BuiltinId),
    Logical(Vec<i32>),
    Integer(Vec<i32>),
    Real(Vec<f64>),
    Complex(Vec<RComplex>),
    String(Vec<InternedStr>),
    Raw(Vec<u8>),
    List(Vec<ValueId>),
    Expression(Vec<ValueId>),
    Bytecode(Rc<BytecodeChunk>),
    Altrep(Box<dyn AltrepProvider>),
    External(Rc<dyn Any>),
    WeakRef {
        key: ValueId,
        value: ValueId,
        finalizer: ValueId,
        on_exit: bool,
    },
    S4Object,
}

impl RObject {
    /// Append every direct heap edge of this object to `out` — the single
    /// exhaustive traversal the garbage collector relies on.
    pub(crate) fn trace_edges(&self, out: &mut Vec<ValueId>) {
        match self {
            RObject::Nil | RObject::Primitive(_) | RObject::S4Object => {}
            RObject::Symbol { value, internal, .. } => {
                out.push(*value);
                out.push(*internal);
            }
            RObject::Pairlist { car, cdr, tag } | RObject::Language { car, cdr, tag } => {
                out.push(*car);
                out.push(*cdr);
                out.push(*tag);
            }
            RObject::Closure { formals, body, env } => {
                out.push(*formals);
                out.push(*body);
                out.push(*env);
            }
            RObject::Environment { frame, enclos } => {
                frame.trace_edges(out);
                out.push(*enclos);
            }
            RObject::Promise { expr, env, value } => {
                out.push(*expr);
                out.push(*env);
                out.push(*value);
            }
            RObject::Logical(_)
            | RObject::Integer(_)
            | RObject::Real(_)
            | RObject::Complex(_)
            | RObject::String(_)
            | RObject::Raw(_) => {}
            RObject::List(items) | RObject::Expression(items) => {
                out.extend(items.iter().copied());
            }
            RObject::Bytecode(chunk) => {
                out.extend(chunk.constants.iter().copied());
            }
            RObject::Altrep(provider) => provider.trace_edges(out),
            RObject::External(_) => {}
            RObject::WeakRef {
                key,
                value,
                finalizer,
                ..
            } => {
                // Weak references trace their finalizer strongly but key/value
                // weakly: the collector handles them in a dedicated pass.
                out.push(*finalizer);
                let _ = key;
                let _ = value;
            }
        }
    }
}

/// Per-object flag bits (named/copy-on-modify and type bits grow with the
/// migration; the layout is stable now).
#[derive(Clone, Copy, Debug, Default)]
pub struct ObjectFlags {
    pub named: u8,
    pub s4: bool,
    pub active_binding: bool,
    pub locked_binding: bool,
    /// Environment lock — GNU's `LOCKED_ENVIRONMENT` header bit. Meaningful
    /// only on `RObject::Environment`; namespace integrity depends on it.
    pub env_locked: bool,
}

/// One heap slot: generation counter for stale-handle rejection, mark bit
/// for collection, attribute edge, and the typed payload.
#[derive(Debug)]
pub struct Slot {
    pub(crate) generation: u32,
    pub(crate) gc_mark: bool,
    pub flags: ObjectFlags,
    pub attrib: ValueId,
    pub body: RObject,
}

impl Slot {
    pub(crate) fn new(generation: u32, body: RObject) -> Slot {
        Slot {
            generation,
            gc_mark: false,
            flags: ObjectFlags::default(),
            attrib: ValueId::NIL,
            body,
        }
    }
}
