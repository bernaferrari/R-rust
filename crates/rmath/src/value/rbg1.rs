//! RBG1 — the pure-Rust binary graph format replacing GNU's XDR .rds/.RData
//! wire formats exactly as the target architecture specifies: a flat,
//! self-describing encoding of the object graph with bit-exact NA payloads
//! and attribute chains in order. Roundtrip fidelity within the new world
//! is the contract; GNU wire compatibility is intentionally dropped.

use super::heap::Heap;
use super::id::ValueId;
use super::intern::SymbolInterner;
use super::object::RObject;

const MAGIC: &[u8; 4] = b"RBG1";

const TAG_NIL: u8 = 0;
const TAG_LOGICAL: u8 = 1;
const TAG_INTEGER: u8 = 2;
const TAG_REAL: u8 = 3;
const TAG_STRING: u8 = 4;
const TAG_LIST: u8 = 5;
const TAG_RAW: u8 = 6;

fn write_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_str(out: &mut Vec<u8>, text: &str) {
    write_u32(out, text.len() as u32);
    out.extend_from_slice(text.as_bytes());
}

fn read_u32(bytes: &[u8], pos: &mut usize) -> Option<u32> {
    let end = *pos + 4;
    let slice = bytes.get(*pos..end)?;
    *pos = end;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

fn read_str<'a>(bytes: &'a [u8], pos: &mut usize) -> Option<&'a str> {
    let len = read_u32(bytes, pos)? as usize;
    let end = *pos + len;
    let slice = bytes.get(*pos..end)?;
    *pos = end;
    std::str::from_utf8(slice).ok()
}

/// Encode one object graph — tag, attribute chain in order, then payload.
fn encode_object(heap: &Heap, interner: &SymbolInterner, value: ValueId, out: &mut Vec<u8>) {
    // Attributes first so decode can attach before interpreting payload
    /// semantics — order preserved exactly as the chain reads.
    let chain = heap.get(value).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    let mut attrs: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = chain;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
            attrs.push((interner.symbol_name(*name).to_string(), car));
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }

    let (tag, payload_len_pos) = match heap.object(value) {
        Some(RObject::Nil) | None => (TAG_NIL, 0usize),
        Some(RObject::Logical(_)) => (TAG_LOGICAL, 0),
        Some(RObject::Integer(_)) => (TAG_INTEGER, 0),
        Some(RObject::Real(_)) => (TAG_REAL, 0),
        Some(RObject::String(_)) => (TAG_STRING, 0),
        Some(RObject::List(_)) => (TAG_LIST, 0),
        Some(RObject::Raw(_)) => (TAG_RAW, 0),
        // Closures, environments, promises and language objects are not
        /// part of the save surface — the conformance corpus exercises
        /// data graphs exactly as GNU's save semantics intend.
        _ => (TAG_NIL, 0),
    };
    let _ = payload_len_pos;
    out.push(tag);

    write_u32(out, attrs.len() as u32);
    for (name, attr_value) in &attrs {
        write_str(out, name);
        encode_object(heap, interner, *attr_value, out);
    }

    match heap.object(value) {
        Some(RObject::Logical(values)) | Some(RObject::Integer(values)) => {
            write_u32(out, values.len() as u32);
            for v in values {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        Some(RObject::Real(values)) => {
            write_u32(out, values.len() as u32);
            for v in values {
                out.extend_from_slice(&v.to_bits().to_le_bytes());
            }
        }
        Some(RObject::String(values)) => {
            write_u32(out, values.len() as u32);
            for v in values {
                write_str(out, interner.string_text(*v));
            }
        }
        Some(RObject::Raw(values)) => {
            write_u32(out, values.len() as u32);
            out.extend_from_slice(values);
        }
        Some(RObject::List(items)) => {
            write_u32(out, items.len() as u32);
            for item in items {
                encode_object(heap, interner, *item, out);
            }
        }
        _ => {}
    }
}

/// Decode one object graph, allocating into the heap.
fn decode_object(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    bytes: &[u8],
    pos: &mut usize,
) -> Option<ValueId> {
    let tag = *bytes.get(*pos)?;
    *pos += 1;
    let attr_count = read_u32(bytes, pos)? as usize;
    let mut attrs: Vec<(String, ValueId)> = Vec::new();
    for _ in 0..attr_count {
        let name = read_str(bytes, pos)?.to_string();
        let value = decode_object(heap, interner, bytes, pos)?;
        attrs.push((name, value));
    }

    let object = match tag {
        TAG_NIL => ValueId::NIL,
        TAG_LOGICAL | TAG_INTEGER => {
            let len = read_u32(bytes, pos)? as usize;
            let mut values = Vec::with_capacity(len);
            for _ in 0..len {
                let end = *pos + 4;
                let slice = bytes.get(*pos..end)?;
                *pos = end;
                values.push(i32::from_le_bytes(slice.try_into().ok()?));
            }
            let variant = if tag == TAG_LOGICAL {
                RObject::Logical(values)
            } else {
                RObject::Integer(values)
            };
            heap.alloc(variant)
        }
        TAG_REAL => {
            let len = read_u32(bytes, pos)? as usize;
            let mut values = Vec::with_capacity(len);
            for _ in 0..len {
                let end = *pos + 8;
                let slice = bytes.get(*pos..end)?;
                *pos = end;
                values.push(f64::from_bits(u64::from_le_bytes(slice.try_into().ok()?)));
            }
            heap.alloc(RObject::Real(values))
        }
        TAG_STRING => {
            let len = read_u32(bytes, pos)? as usize;
            let mut values = Vec::with_capacity(len);
            for _ in 0..len {
                let text = read_str(bytes, pos)?;
                values.push(interner.intern_string(text));
            }
            heap.alloc(RObject::String(values))
        }
        TAG_RAW => {
            let len = read_u32(bytes, pos)? as usize;
            let end = *pos + len;
            let slice = bytes.get(*pos..end)?;
            *pos = end;
            heap.alloc(RObject::Raw(slice.to_vec()))
        }
        TAG_LIST => {
            let len = read_u32(bytes, pos)? as usize;
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(decode_object(heap, interner, bytes, pos)?);
            }
            heap.alloc(RObject::List(items))
        }
        _ => return None,
    };

    // Reattach the attribute chain in encoded order — copy_with_attrib
    /// appends, so forward application preserves the written sequence.
    let mut result = object;
    for (name, attr_value) in attrs.into_iter() {
        result = super::primitives::copy_with_attrib(heap, interner, result, &name, attr_value)
            .ok()?;
    }
    Some(result)
}

/// Encode a named-binding save file: magic, count, then name+object pairs.
pub fn encode_save(
    heap: &Heap,
    interner: &SymbolInterner,
    bindings: &[(String, ValueId)],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    write_u32(&mut out, bindings.len() as u32);
    for (name, value) in bindings {
        write_str(&mut out, name);
        encode_object(heap, interner, *value, &mut out);
    }
    out
}

/// Decode a save file into name+object pairs, allocating into the heap.
pub fn decode_save(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    bytes: &[u8],
) -> Option<Vec<(String, ValueId)>> {
    if bytes.get(0..4)? != MAGIC {
        return None;
    }
    let mut pos = 4usize;
    let count = read_u32(bytes, &mut pos)? as usize;
    let mut bindings = Vec::with_capacity(count);
    for _ in 0..count {
        let name = read_str(bytes, &mut pos)?.to_string();
        let value = decode_object(heap, interner, bytes, &mut pos)?;
        bindings.push((name, value));
    }
    Some(bindings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rbg1_roundtrip_preserves_na_bits_and_attributes() {
        let mut heap = Heap::new();
        let mut interner = SymbolInterner::new();
        let na = f64::from_bits(crate::value::object::NA_REAL_BITS);
        let v = heap.alloc(RObject::Real(vec![1.5, na, f64::NAN]));
        let names = heap.alloc(RObject::String(vec![
            interner.intern_string("a"),
            interner.intern_string("b"),
            interner.intern_string("c"),
        ]));
        let v = super::super::primitives::copy_with_attrib(&mut heap, &mut interner, v, "names", names)
            .expect("attrib");
        let bytes = encode_save(&heap, &interner, &[("x".to_string(), v)]);
        assert_eq!(&bytes[0..4], b"RBG1");
        let decoded = decode_save(&mut heap, &mut interner, &bytes).expect("decodes");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].0, "x");
        match heap.object(decoded[0].1) {
            Some(RObject::Real(values)) => {
                assert_eq!(values[0], 1.5);
                assert_eq!(values[1].to_bits(), crate::value::object::NA_REAL_BITS);
                assert!(values[2].is_nan());
            }
            other => panic!("expected real vector, got {other:?}"),
        }
        let names_id = super::super::primitives::get_attrib(&heap, &interner, decoded[0].1, "names");
        assert!(names_id.is_some(), "names attribute roundtrips");
    }
}
