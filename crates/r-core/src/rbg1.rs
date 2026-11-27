//! RBG1 — the flat index-addressed binary graph format, wire-compatible
//! with the canonical encoding the conversion pipeline writes: every object
//! is a node in an index-addressed table, references between objects are
//! u32 node indices, and two-pass materialization handles arbitrary sharing
//! and true cycles by construction. GNU wire compat is intentionally
//! dropped; compat with the baked package assets is the whole point.
//!
//! Layout (all integers little-endian):
//!   magic "RBG1", u32 format_version, u8 compression, u64 uncompressed_len,
//!   u64 compressed_len
//!   payload (compressed when flag set):
//!     u32 node_count   — including the implicit nil node at index 0
//!     node records in index order, starting at index 1
//!
//! Node record: u8 tag, u32 meta, payload. References are u32 node indices;
//! index 0 is the canonical nil object. The root object is index 1 — the
//! writer interns it first.

use crate::heap::Heap;
use crate::id::ValueId;
use crate::intern::{InternedStr, SymbolInterner};
use crate::object::{EnvFrame, RComplex, RObject};

const MAGIC: &[u8; 4] = b"RBG1";
const FORMAT_VERSION: u32 = 1;
const COMPRESS_NONE: u8 = 0;
const COMPRESS_GZIP: u8 = 1;
const HEADER_LEN: usize = 25;

// Node type tags — identical numbering to the canonical writer.
const T_SYM: u8 = 1;
const T_CHAR: u8 = 2;
const T_LGL: u8 = 3;
const T_INT: u8 = 4;
const T_REAL: u8 = 5;
const T_CPLX: u8 = 6;
const T_STR: u8 = 7;
const T_RAW: u8 = 8;
const T_VEC: u8 = 9;
const T_EXPR: u8 = 10;
const T_LIST: u8 = 11;
const T_LANG: u8 = 12;
const T_CLOS: u8 = 13;
const T_ENV: u8 = 14;
const T_PROM: u8 = 15;
const T_S4: u8 = 18;
const T_BCODE: u8 = 19;
const T_GLOBALENV: u8 = 40;
const T_BASEENV: u8 = 41;
const T_EMPTYENV: u8 = 42;
const T_UNBOUND: u8 = 43;
const T_MISSING: u8 = 44;
const T_NAMESPACE: u8 = 46;
const T_DOTS: u8 = 48;

// ---------------------------------------------------------------------------
// Decode
// ---------------------------------------------------------------------------

struct Parsed {
    tag: u8,
    refs: Vec<u32>,
    data: Vec<u8>,
    len: u32,
}

struct Cursor<'a> {
    payload: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.payload.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn u32(&mut self) -> Option<u32> {
        let end = self.pos + 4;
        let slice = self.payload.get(self.pos..end)?;
        self.pos = end;
        Some(u32::from_le_bytes(slice.try_into().ok()?))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(self.u32()? as i32)
    }

    fn bytes(&mut self, n: usize) -> Option<Vec<u8>> {
        let end = self.pos.checked_add(n)?;
        let slice = self.payload.get(self.pos..end)?;
        self.pos = end;
        Some(slice.to_vec())
    }
}

fn parse_node(cursor: &mut Cursor<'_>) -> Option<Parsed> {
    let tag = cursor.u8()?;
    let _meta = cursor.u32()?;
    let mut refs = Vec::new();
    let mut data = Vec::new();
    let mut len = 0u32;
    let mut extra = 0u8;
    match tag {
        T_SYM | T_NAMESPACE => {
            let n = cursor.u32()? as usize;
            if n > (1 << 24) {
                return None;
            }
            data = cursor.bytes(n)?;
        }
        T_CHAR => {
            let n = cursor.i32()?;
            if n >= 0 {
                len = n as u32;
                data = cursor.bytes(n as usize)?;
            } else {
                len = u32::MAX; // NA marker
            }
        }
        T_LGL | T_INT => {
            len = cursor.u32()?;
            if len as usize > (1 << 28) {
                return None;
            }
            refs.push(cursor.u32()?);
            data = cursor.bytes(len as usize * 4)?;
        }
        T_REAL => {
            len = cursor.u32()?;
            if len as usize > (1 << 27) {
                return None;
            }
            refs.push(cursor.u32()?);
            data = cursor.bytes(len as usize * 8)?;
        }
        T_CPLX => {
            len = cursor.u32()?;
            if len as usize > (1 << 26) {
                return None;
            }
            refs.push(cursor.u32()?);
            data = cursor.bytes(len as usize * 16)?;
        }
        T_STR | T_VEC | T_EXPR | T_S4 | T_BCODE => {
            len = cursor.u32()?;
            if len as usize > (1 << 28) {
                return None;
            }
            refs.push(cursor.u32()?); // attrib
            for _ in 0..len {
                refs.push(cursor.u32()?);
            }
        }
        T_RAW => {
            len = cursor.u32()?;
            if len as usize > (1 << 30) {
                return None;
            }
            refs.push(cursor.u32()?);
            data = cursor.bytes(len as usize)?;
        }
        T_LIST | T_LANG | T_CLOS | T_PROM | T_DOTS => {
            for _ in 0..4 {
                refs.push(cursor.u32()?);
            }
        }
        T_ENV => {
            extra = cursor.u8()?;
            for _ in 0..3 {
                refs.push(cursor.u32()?);
            }
        }
        T_GLOBALENV | T_BASEENV | T_EMPTYENV | T_UNBOUND | T_MISSING => {}
        _ => return None,
    }
    Some(Parsed {
        tag,
        refs,
        data,
        len,
    })
}

/// Decode the framing header, answering the inflated payload. Compression
/// is the container layer's job exactly as the canonical format arranges;
/// gzip frames inflate here since baked containers store frames whole.
fn decode_frame_payload(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < HEADER_LEN || bytes[..4] != *MAGIC {
        return None;
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    if version != FORMAT_VERSION {
        return None;
    }
    let method = bytes[8];
    let uncompressed_len = u64::from_le_bytes(bytes[9..17].try_into().ok()?) as usize;
    let compressed_len = u64::from_le_bytes(bytes[17..25].try_into().ok()?) as usize;
    if uncompressed_len > (1 << 31) {
        return None;
    }
    let frame_end = HEADER_LEN.checked_add(compressed_len)?;
    if bytes.len() < frame_end {
        return None;
    }
    match method {
        COMPRESS_NONE => Some(bytes[HEADER_LEN..frame_end].to_vec()),
        COMPRESS_GZIP => {
            use std::io::Read;
            let mut decoder = flate2::read::GzDecoder::new(&bytes[HEADER_LEN..frame_end]);
            let mut out = Vec::with_capacity(uncompressed_len);
            decoder.read_to_end(&mut out).ok()?;
            if out.len() != uncompressed_len {
                return None;
            }
            Some(out)
        }
        _ => None,
    }
}

/// Materialize an object graph from RBG1 frame bytes — the single-object
/// RDS shape: root at node index 1, two-pass linking so cyclic structure
/// materializes correctly. `default_env` wires references to the global
/// environment sentinel — exactly the session the decode runs inside.
pub fn decode_rds(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    bytes: &[u8],
    default_env: ValueId,
) -> Option<ValueId> {
    let payload = decode_frame_payload(bytes)?;
    decode_payload(heap, interner, &payload, default_env)
}

/// Materialize a payload graph — the framing-stripped form the lazy-load
/// database reader shares.
pub fn decode_payload(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    payload: &[u8],
    default_env: ValueId,
) -> Option<ValueId> {
    let mut cursor = Cursor { payload, pos: 0 };
    let node_count = cursor.u32()? as usize;
    if node_count == 0 || node_count > (1 << 28) {
        return None;
    }
    // Pass 1: parse every record.
    let mut nodes: Vec<Parsed> = Vec::with_capacity(node_count);
    nodes.push(Parsed {
        tag: 0,
        refs: vec![],
        data: vec![],
        len: 0,
    });
    for _ in 1..node_count {
        nodes.push(parse_node(&mut cursor)?);
    }
    if cursor.pos != payload.len() {
        return None;
    }

    // Pass 2a: allocate every object with its final type. CHAR nodes never
    // become heap objects — they are string content, materializing into the
    // side table the string-vector fill consults. Singletons resolve to
    // their sentinels without allocation.
    let mut ids: Vec<ValueId> = vec![ValueId::NIL; node_count];
    let mut chars: Vec<Option<InternedStr>> = vec![None; node_count];
    for (idx, node) in nodes.iter().enumerate().skip(1) {
        ids[idx] = match node.tag {
            T_GLOBALENV => heap.global_env_id.unwrap_or(default_env),
            T_BASEENV => heap.base_env_id.unwrap_or(ValueId::BASE_ENV),
            T_EMPTYENV => ValueId::EMPTY_ENV,
            T_UNBOUND => ValueId::UNBOUND,
            T_MISSING => ValueId::MISSING,
            T_CHAR => {
                let text = if node.len == u32::MAX {
                    // The NA string sentinel — exactly the interned NUL-
                    // prefixed form the rest of the world recognizes.
                    "\0NA".to_string()
                } else {
                    String::from_utf8(node.data.clone()).unwrap_or_default()
                };
                chars[idx] = Some(interner.intern_string(&text));
                ValueId::NIL
            }
            T_SYM => {
                let name = String::from_utf8(node.data.clone()).unwrap_or_default();
                let sym = interner.intern_symbol(&name);
                heap.alloc(RObject::Symbol {
                    name: sym,
                    value: ValueId::UNBOUND,
                    internal: ValueId::NIL,
                })
            }
            T_LGL => {
                let mut items = Vec::with_capacity(node.len as usize);
                for chunk in node.data.chunks_exact(4) {
                    items.push(i32::from_le_bytes(chunk.try_into().ok()?));
                }
                heap.alloc(RObject::Logical(items))
            }
            T_INT => {
                let mut items = Vec::with_capacity(node.len as usize);
                for chunk in node.data.chunks_exact(4) {
                    items.push(i32::from_le_bytes(chunk.try_into().ok()?));
                }
                heap.alloc(RObject::Integer(items))
            }
            T_REAL => {
                let mut items = Vec::with_capacity(node.len as usize);
                for chunk in node.data.chunks_exact(8) {
                    items.push(f64::from_bits(u64::from_le_bytes(chunk.try_into().ok()?)));
                }
                heap.alloc(RObject::Real(items))
            }
            T_CPLX => {
                let mut items = Vec::with_capacity(node.len as usize);
                for chunk in node.data.chunks_exact(16) {
                    let r = f64::from_bits(u64::from_le_bytes(chunk[..8].try_into().ok()?));
                    let i = f64::from_bits(u64::from_le_bytes(chunk[8..].try_into().ok()?));
                    items.push(RComplex { r, i });
                }
                heap.alloc(RObject::Complex(items))
            }
            T_STR => {
                let empty = interner.intern_string("");
                heap.alloc(RObject::String(vec![empty; node.len as usize]))
            }
            T_RAW => heap.alloc(RObject::Raw(node.data.clone())),
            T_VEC => heap.alloc(RObject::List(vec![ValueId::NIL; node.len as usize])),
            T_EXPR => heap.alloc(RObject::Expression(vec![ValueId::NIL; node.len as usize])),
            T_LIST | T_DOTS => heap.alloc(RObject::Pairlist {
                car: ValueId::NIL,
                cdr: ValueId::NIL,
                tag: ValueId::NIL,
            }),
            T_LANG => heap.alloc(RObject::Language {
                car: ValueId::NIL,
                cdr: ValueId::NIL,
                tag: ValueId::NIL,
            }),
            T_CLOS => heap.alloc(RObject::Closure {
                formals: ValueId::NIL,
                body: ValueId::NIL,
                env: default_env,
            }),
            T_ENV => heap.alloc(RObject::Environment {
                frame: EnvFrame::default(),
                enclos: ValueId::EMPTY_ENV,
            }),
            T_S4 => heap.alloc(RObject::S4Object),
            T_PROM => heap.alloc(RObject::Promise {
                expr: ValueId::NIL,
                env: default_env,
                value: ValueId::NIL,
            }),
            T_NAMESPACE => {
                // Named namespace references resolve through the loaded
                // package registry exactly as the canonical reader resolves
                // them through the namespace loader.
                let name = String::from_utf8(node.data.clone()).unwrap_or_default();
                match heap.package_registry.get(&name).copied() {
                    Some(env) => env,
                    None => return None,
                }
            }
            _ => return None,
        };
    }

    // Pass 2b: link references and fill vector element slots. Attributes
    // attach to the slot edge exactly as the object model arranges — the
    // wire's attrib reference is the head of the attribute pairlist chain.
    for (idx, node) in nodes.iter().enumerate().skip(1) {
        let id = ids[idx];
        match node.tag {
            T_LGL | T_INT | T_REAL | T_CPLX | T_RAW => {
                let attrib = ids.get(node.refs[0] as usize).copied().unwrap_or(ValueId::NIL);
                if let Some(slot) = heap.get_mut(id) {
                    slot.attrib = attrib;
                }
            }
            T_STR => {
                let attrib = ids.get(node.refs[0] as usize).copied().unwrap_or(ValueId::NIL);
                let elements: Vec<InternedStr> = node.refs[1..]
                    .iter()
                    .filter_map(|r| chars.get(*r as usize).copied().flatten())
                    .collect();
                if let Some(slot) = heap.get_mut(id) {
                    slot.attrib = attrib;
                    if let RObject::String(items) = &mut slot.body {
                        *items = elements;
                    }
                }
            }
            T_VEC | T_EXPR | T_BCODE => {
                let attrib = ids.get(node.refs[0] as usize).copied().unwrap_or(ValueId::NIL);
                let elements: Vec<ValueId> = node.refs[1..]
                    .iter()
                    .map(|r| ids.get(*r as usize).copied().unwrap_or(ValueId::NIL))
                    .collect();
                if let Some(slot) = heap.get_mut(id) {
                    slot.attrib = attrib;
                    match &mut slot.body {
                        RObject::List(items) | RObject::Expression(items) => {
                            *items = elements;
                        }
                        _ => {}
                    }
                }
            }
            T_S4 => {
                let attrib = ids.get(node.refs[0] as usize).copied().unwrap_or(ValueId::NIL);
                if let Some(slot) = heap.get_mut(id) {
                    slot.attrib = attrib;
                }
            }
            T_LIST | T_DOTS | T_LANG => {
                let car = ids.get(node.refs[0] as usize).copied().unwrap_or(ValueId::NIL);
                let cdr = ids.get(node.refs[1] as usize).copied().unwrap_or(ValueId::NIL);
                let tagv = ids.get(node.refs[2] as usize).copied().unwrap_or(ValueId::NIL);
                let attrib = ids.get(node.refs[3] as usize).copied().unwrap_or(ValueId::NIL);
                if let Some(slot) = heap.get_mut(id) {
                    slot.attrib = attrib;
                    match &mut slot.body {
                        RObject::Pairlist { car: c, cdr: d, tag: t }
                        | RObject::Language { car: c, cdr: d, tag: t } => {
                            *c = car;
                            *d = cdr;
                            *t = tagv;
                        }
                        _ => {}
                    }
                }
            }
            T_CLOS => {
                let env = ids.get(node.refs[0] as usize).copied().unwrap_or(default_env);
                let formals = ids.get(node.refs[1] as usize).copied().unwrap_or(ValueId::NIL);
                let body = ids.get(node.refs[2] as usize).copied().unwrap_or(ValueId::NIL);
                let attrib = ids.get(node.refs[3] as usize).copied().unwrap_or(ValueId::NIL);
                if let Some(slot) = heap.get_mut(id) {
                    slot.attrib = attrib;
                    if let RObject::Closure {
                        formals: f,
                        body: b,
                        env: e,
                    } = &mut slot.body
                    {
                        *f = formals;
                        *b = body;
                        *e = env;
                    }
                }
            }
            T_ENV => {
                // The enclosure links now; the frame reconstructs in pass 2c
                // once every pairlist cell is fully linked, exactly as the
                // canonical reader arranges. The wire order is enclos,
                // frame, attrib.
                let enclos = ids.get(node.refs[0] as usize).copied().unwrap_or(ValueId::EMPTY_ENV);
                let attrib = ids.get(node.refs[2] as usize).copied().unwrap_or(ValueId::NIL);
                if let Some(slot) = heap.get_mut(id) {
                    slot.attrib = attrib;
                    if let RObject::Environment { enclos: e, .. } = &mut slot.body {
                        *e = enclos;
                    }
                }
            }
            T_PROM => {
                // The wire order mirrors the closure layout: environment,
                // expression, value, attrib. Baked lazy databases force
                // before encoding, so materialized promises carry their
                // already-forced value exactly as the canonical reader's.
                let env = ids.get(node.refs[0] as usize).copied().unwrap_or(default_env);
                let expr = ids.get(node.refs[1] as usize).copied().unwrap_or(ValueId::NIL);
                let value = ids.get(node.refs[2] as usize).copied().unwrap_or(ValueId::NIL);
                let attrib = ids.get(node.refs[3] as usize).copied().unwrap_or(ValueId::NIL);
                if let Some(slot) = heap.get_mut(id) {
                    slot.attrib = attrib;
                    if let RObject::Promise {
                        expr: x,
                        env: e,
                        value: v,
                    } = &mut slot.body
                    {
                        *x = expr;
                        *e = env;
                        *v = value;
                    }
                }
            }
            _ => {}
        }
    }

    // Pass 2c: environment frames — walk the wire pairlist chain, inserting
    // each binding into the frame exactly as the canonical reader's
    // defineVar loop reconstructs them.
    for (idx, node) in nodes.iter().enumerate().skip(1) {
        if node.tag != T_ENV {
            continue;
        }
        let id = ids[idx];
        let mut cursor = ids.get(node.refs[1] as usize).copied().unwrap_or(ValueId::NIL);
        for _ in 0..(1 << 20) {
            if cursor == ValueId::NIL || cursor.is_sentinel() {
                break;
            }
            let (car, cdr, tag) = match heap.object(cursor) {
                Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                _ => break,
            };
            let sym = match heap.object(tag) {
                Some(RObject::Symbol { name, .. }) => *name,
                _ => break,
            };
            if let Some(slot) = heap.get_mut(id) {
                if let RObject::Environment { frame, .. } = &mut slot.body {
                    frame.insert(sym, car);
                }
            }
            cursor = cdr;
        }
    }

    // A nil root interned as index 0 produces no records: the object is nil.
    Some(ids.get(1).copied().unwrap_or(ValueId::NIL))
}

/// Decode the save-frame shape — bindings written as a tagged pairlist
/// chain exactly as the writer frames them — answering name+object pairs
/// in frame order.
pub fn decode_save(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    bytes: &[u8],
    default_env: ValueId,
) -> Option<Vec<(String, ValueId)>> {
    let root = decode_rds(heap, interner, bytes, default_env)?;
    let mut out = Vec::new();
    let mut cursor = root;
    for _ in 0..(1 << 20) {
        if cursor == ValueId::NIL || cursor.is_sentinel() {
            break;
        }
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
            out.push((interner.symbol_name(*name).to_string(), car));
        }
        cursor = cdr;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Encode
// ---------------------------------------------------------------------------

/// Writer index keys — objects, character content, and sentinels share the
/// one index space exactly as the wire arranges. Character nodes key by
/// their interned identity so shared strings dedupe exactly as the
/// canonical format's CHAR sharing does.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Object(ValueId),
    Char(InternedStr),
    Sentinel(u8),
}

struct GraphWriter<'h, 'i> {
    heap: &'h Heap,
    interner: &'i SymbolInterner,
    indices: std::collections::HashMap<Key, u32>,
    order: Vec<Key>,
}

impl<'h, 'i> GraphWriter<'h, 'i> {
    fn new(heap: &'h Heap, interner: &'i SymbolInterner) -> Self {
        GraphWriter {
            heap,
            interner,
            indices: std::collections::HashMap::new(),
            order: Vec::new(),
        }
    }

    fn intern_key(&mut self, key: Key) -> u32 {
        if let Some(idx) = self.indices.get(&key) {
            return *idx;
        }
        let idx = (self.order.len() + 1) as u32;
        self.indices.insert(key, idx);
        self.order.push(key);
        idx
    }

    fn sentinel_tag(value: ValueId) -> Option<u8> {
        if value == ValueId::GLOBAL_ENV {
            Some(T_GLOBALENV)
        } else if value == ValueId::BASE_ENV {
            Some(T_BASEENV)
        } else if value == ValueId::EMPTY_ENV {
            Some(T_EMPTYENV)
        } else if value == ValueId::UNBOUND {
            Some(T_UNBOUND)
        } else if value == ValueId::MISSING {
            Some(T_MISSING)
        } else {
            None
        }
    }

    /// The index key a value maps to — the single mapping intern() and
    /// reference emission share exactly, since a value interned as a
    /// sentinel pseudo-reference must resolve as that same sentinel at
    /// every referencing node. The session's global and base environments
    /// cross as pseudo-references exactly as GNU's globalenv/baseenv
    /// items: the environment graph is session state, never document
    /// content.
    fn key_for(&self, value: ValueId) -> Option<Key> {
        if value == ValueId::NIL {
            return None;
        }
        if let Some(tag) = Self::sentinel_tag(value) {
            return Some(Key::Sentinel(tag));
        }
        if value.is_sentinel() {
            return None;
        }
        if matches!(
            self.heap.object(value),
            Some(RObject::Environment { .. })
        ) {
            if self.heap.global_env_id == Some(value) {
                return Some(Key::Sentinel(T_GLOBALENV));
            }
            if self.heap.base_env_id == Some(value) {
                return Some(Key::Sentinel(T_BASEENV));
            }
        }
        Some(Key::Object(value))
    }

    /// Intern an object and every edge it references — breadth-first in
    /// emission order exactly as the canonical writer's cursor walk. The
    /// returned index is stable across the whole document.
    fn intern(&mut self, value: ValueId) -> u32 {
        let Some(key) = self.key_for(value) else {
            return 0;
        };
        if matches!(key, Key::Sentinel(_)) {
            return self.intern_key(key);
        }
        if self.indices.contains_key(&key) {
            return self.intern_key(key);
        }
        let idx = self.intern_key(key);
        // Edges intern eagerly — children must hold indices before the
        // emission pass writes this node's reference slots.
        if let Some(slot) = self.heap.get(value) {
            if slot.attrib != ValueId::NIL {
                self.intern(slot.attrib);
            }
            match &slot.body {
                RObject::String(items) => {
                    for item in items.iter().copied() {
                        self.intern_key(Key::Char(item));
                    }
                }
                RObject::List(items) | RObject::Expression(items) => {
                    for item in items.iter().copied() {
                        self.intern(item);
                    }
                }
                RObject::Pairlist { car, cdr, tag } | RObject::Language { car, cdr, tag } => {
                    self.intern(*car);
                    self.intern(*cdr);
                    self.intern(*tag);
                }
                RObject::Closure { formals, body, env } => {
                    self.intern(*env);
                    self.intern(*formals);
                    self.intern(*body);
                }
                RObject::Promise { expr, env, value: v } => {
                    self.intern(*env);
                    self.intern(*expr);
                    self.intern(*v);
                }
                RObject::Symbol { .. } => {}
                RObject::Environment { .. } => {
                    // Environment encoding is deferred: the new world's
                    // frame is not a heap pairlist, and synthesizing one
                    // mid-encode would mutate the very heap being read.
                    // Package namespaces cross documents by name reference
                    // instead — the T_NAMESPACE path the loader consumes.
                }
                _ => {}
            }
        }
        idx
    }

    fn emit(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&((self.order.len() + 1) as u32).to_le_bytes());
        for key in &self.order {
            self.emit_node(*key, &mut out);
        }
        out
    }

    fn push_meta(&self, out: &mut Vec<u8>) {
        // gp bits and the obj bit are presentation metadata the new world
        // does not maintain — zero exactly as a fresh object's bits read.
        out.extend_from_slice(&0u32.to_le_bytes());
    }

    fn push_ref(&self, out: &mut Vec<u8>, idx: u32) {
        out.extend_from_slice(&idx.to_le_bytes());
    }

    fn emit_node(&self, key: Key, out: &mut Vec<u8>) {
        match key {
            Key::Sentinel(tag) => {
                out.push(tag);
                self.push_meta(out);
            }
            Key::Char(text) => {
                out.push(T_CHAR);
                self.push_meta(out);
                let text = self.interner.string_text(text);
                if text == "\0NA" {
                    // The NA string marker — negative length exactly as the
                    // canonical writer emits.
                    out.extend_from_slice(&(-1i32).to_le_bytes());
                } else {
                    out.extend_from_slice(&(text.len() as i32).to_le_bytes());
                    out.extend_from_slice(text.as_bytes());
                }
            }
            Key::Object(value) => {
                let Some(slot) = self.heap.get(value) else {
                    return;
                };
                let attrib_idx = self.object_index(slot.attrib);
                match &slot.body {
                    RObject::Nil => {
                        // Nil beyond the canonical index-0 — unreachable in
                        // practice; encode as the nil reference directly.
                        out.push(T_LIST);
                        self.push_meta(out);
                        for _ in 0..4 {
                            self.push_ref(out, 0);
                        }
                    }
                    RObject::Symbol { name, .. } => {
                        out.push(T_SYM);
                        self.push_meta(out);
                        let text = self.interner.symbol_name(*name);
                        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
                        out.extend_from_slice(text.as_bytes());
                    }
                    RObject::Logical(items) => {
                        out.push(T_LGL);
                        self.push_meta(out);
                        out.extend_from_slice(&(items.len() as u32).to_le_bytes());
                        self.push_ref(out, attrib_idx);
                        for item in items {
                            out.extend_from_slice(&item.to_le_bytes());
                        }
                    }
                    RObject::Integer(items) => {
                        out.push(T_INT);
                        self.push_meta(out);
                        out.extend_from_slice(&(items.len() as u32).to_le_bytes());
                        self.push_ref(out, attrib_idx);
                        for item in items {
                            out.extend_from_slice(&item.to_le_bytes());
                        }
                    }
                    RObject::Real(items) => {
                        out.push(T_REAL);
                        self.push_meta(out);
                        out.extend_from_slice(&(items.len() as u32).to_le_bytes());
                        self.push_ref(out, attrib_idx);
                        for item in items {
                            out.extend_from_slice(&item.to_bits().to_le_bytes());
                        }
                    }
                    RObject::Complex(items) => {
                        out.push(T_CPLX);
                        self.push_meta(out);
                        out.extend_from_slice(&(items.len() as u32).to_le_bytes());
                        self.push_ref(out, attrib_idx);
                        for item in items {
                            out.extend_from_slice(&item.r.to_bits().to_le_bytes());
                            out.extend_from_slice(&item.i.to_bits().to_le_bytes());
                        }
                    }
                    RObject::String(items) => {
                        out.push(T_STR);
                        self.push_meta(out);
                        out.extend_from_slice(&(items.len() as u32).to_le_bytes());
                        self.push_ref(out, attrib_idx);
                        for item in items {
                            let idx = self.indices.get(&Key::Char(*item)).copied().unwrap_or(0);
                            self.push_ref(out, idx);
                        }
                    }
                    RObject::Raw(items) => {
                        out.push(T_RAW);
                        self.push_meta(out);
                        out.extend_from_slice(&(items.len() as u32).to_le_bytes());
                        self.push_ref(out, attrib_idx);
                        out.extend_from_slice(items);
                    }
                    RObject::List(items) | RObject::Expression(items) => {
                        let is_expr = matches!(slot.body, RObject::Expression(_));
                        out.push(if is_expr { T_EXPR } else { T_VEC });
                        self.push_meta(out);
                        out.extend_from_slice(&(items.len() as u32).to_le_bytes());
                        self.push_ref(out, attrib_idx);
                        for item in items {
                            let idx = self.object_index(*item);
                            self.push_ref(out, idx);
                        }
                    }
                    RObject::Pairlist { car, cdr, tag } => {
                        out.push(T_LIST);
                        self.push_meta(out);
                        self.push_ref(out, self.object_index(*car));
                        self.push_ref(out, self.object_index(*cdr));
                        self.push_ref(out, self.object_index(*tag));
                        self.push_ref(out, attrib_idx);
                    }
                    RObject::Language { car, cdr, tag } => {
                        out.push(T_LANG);
                        self.push_meta(out);
                        self.push_ref(out, self.object_index(*car));
                        self.push_ref(out, self.object_index(*cdr));
                        self.push_ref(out, self.object_index(*tag));
                        self.push_ref(out, attrib_idx);
                    }
                    RObject::Closure { formals, body, env } => {
                        out.push(T_CLOS);
                        self.push_meta(out);
                        self.push_ref(out, self.object_index(*env));
                        self.push_ref(out, self.object_index(*formals));
                        self.push_ref(out, self.object_index(*body));
                        self.push_ref(out, attrib_idx);
                    }
                    RObject::Promise { expr, env, value: v } => {
                        out.push(T_PROM);
                        self.push_meta(out);
                        self.push_ref(out, self.object_index(*env));
                        self.push_ref(out, self.object_index(*expr));
                        self.push_ref(out, self.object_index(*v));
                        self.push_ref(out, attrib_idx);
                    }
                    RObject::S4Object => {
                        out.push(T_S4);
                        self.push_meta(out);
                        out.extend_from_slice(&0u32.to_le_bytes());
                        self.push_ref(out, attrib_idx);
                    }
                    _ => {
                        // Environments, bytecode, altrep, external pointers
                        // and weak references have no new-world encoding yet
                        // — the document degrades to a nil reference rather
                        // than corrupting the graph, exactly as the canonical
                        // writer treats unrepresentable objects.
                        out.push(T_LIST);
                        self.push_meta(out);
                        for _ in 0..4 {
                            self.push_ref(out, 0);
                        }
                    }
                }
            }
        }
    }

    fn object_index(&self, value: ValueId) -> u32 {
        let Some(key) = self.key_for(value) else {
            return 0;
        };
        self.indices.get(&key).copied().unwrap_or(0)
    }
}

fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.push(COMPRESS_NONE);
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Encode one object graph — the single-object RDS shape exactly as the
/// canonical writer frames it: root interned first, flat node table,
/// uncompressed container since compression is the container layer's job.
pub fn encode_rds(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<u8> {
    let mut writer = GraphWriter::new(heap, interner);
    writer.intern(value);
    let payload = writer.emit();
    encode_frame(&payload)
}

/// Encode a save frame — the bindings assembled as a real tagged pairlist
/// chain in the heap, then encoded as the document root exactly as
/// decode_save's walk expects. The chain nodes are ordinary heap objects:
/// no synthetic records, no parallel index spaces — the graph the writer
/// encodes is the graph the heap contains.
pub fn encode_save(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    bindings: &[(String, ValueId)],
) -> Vec<u8> {
    // Build the chain in reverse so the document reads in binding order.
    let mut chain = ValueId::NIL;
    for (name, value) in bindings.iter().rev() {
        let sym = interner.intern_symbol(name);
        let tag = heap.alloc(RObject::Symbol {
            name: sym,
            value: ValueId::UNBOUND,
            internal: ValueId::NIL,
        });
        chain = heap.alloc(RObject::Pairlist {
            car: *value,
            cdr: chain,
            tag,
        });
    }
    encode_rds(heap, interner, chain)
}
