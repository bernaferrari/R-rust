//! RBG1 — rport's native binary graph serialization format.
//!
//! Unlike GNU's XDR/ASCII formats, RBG1 is a *flat* graph encoding: every
//! object is a node in an index-addressed table, and references between
//! objects are u32 node indices. This handles arbitrary sharing AND true
//! cycles by construction (the reader materializes all nodes before linking
//! them), where GNU's pre-order traversal can only reference backwards.
//!
//! Layout (all integers little-endian):
//!   magic "RBG1", u32 format_version, u8 compression, u64 uncompressed_len
//!   payload (compressed when flag set):
//!     u32 node_count   — including the implicit nil node at index 0
//!     node records in index order, starting at index 1
//!
//! Node record: u8 tag, u32 meta (gp bits low 16 | obj bit << 16), payload.
//! References are u32 node indices; index 0 is the canonical nil object.

use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::*;

pub mod read;
pub mod write;

pub(crate) const FORMAT_VERSION: u32 = 1;
pub(crate) const MAGIC: &[u8; 4] = b"RBG1";

// Node type tags. Index 0 is always the canonical nil object and never has
// a record; singleton pseudo-tags carry no payload.
pub(crate) const T_SYM: u8 = 1;
pub(crate) const T_CHAR: u8 = 2;
pub(crate) const T_LGL: u8 = 3;
pub(crate) const T_INT: u8 = 4;
pub(crate) const T_REAL: u8 = 5;
pub(crate) const T_CPLX: u8 = 6;
pub(crate) const T_STR: u8 = 7;
pub(crate) const T_RAW: u8 = 8;
pub(crate) const T_VEC: u8 = 9;
pub(crate) const T_EXPR: u8 = 10;
pub(crate) const T_LIST: u8 = 11;
pub(crate) const T_LANG: u8 = 12;
pub(crate) const T_CLOS: u8 = 13;
pub(crate) const T_ENV: u8 = 14;
pub(crate) const T_PROM: u8 = 15;
pub(crate) const T_BUILTIN: u8 = 16;
pub(crate) const T_SPECIAL: u8 = 17;
pub(crate) const T_S4: u8 = 18;
// Singleton pseudo-nodes: no payload, resolved at materialization.
pub(crate) const T_GLOBALENV: u8 = 40;
pub(crate) const T_BASEENV: u8 = 41;
pub(crate) const T_EMPTYENV: u8 = 42;
pub(crate) const T_UNBOUND: u8 = 43;
pub(crate) const T_MISSING: u8 = 44;
pub(crate) const T_BASENAMESPACE: u8 = 45;
// Namespace/package environments serialize by name (persistent reference).
pub(crate) const T_PERSIST_NS: u8 = 50;

/// Compression method flags in the header.
pub(crate) const COMPRESS_NONE: u8 = 0;
pub(crate) const COMPRESS_GZIP: u8 = 1;

/// Serialize one object graph to RBG1 bytes (gzip-compressed).
pub fn write_object(s: SEXP) -> Result<Vec<u8>, String> {
    write_object_hooked(s, R_NilValue())
}

/// Serialize with a GNU-style refhook consulted for persistent references.
pub fn write_object_hooked(s: SEXP, hook: SEXP) -> Result<Vec<u8>, String> {
    let mut writer = write::GraphWriter::new();
    writer.set_persist_hook(hook);
    writer.intern_root(s)?;
    let payload = writer.finish_payload()?;
    Ok(encode_frame(&payload))
}

/// Materialize an object graph from RBG1 bytes.
pub fn read_object(bytes: &[u8]) -> Result<SEXP, String> {
    read_object_hooked(bytes, R_NilValue())
}

/// Total byte length of the frame that starts at `bytes` — header plus the
/// compressed payload — or `None` when the header is truncated or foreign.
pub fn frame_total_len(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < HEADER_LEN || bytes[..4] != *MAGIC {
        return None;
    }
    let compressed_len = u64::from_le_bytes(bytes[17..25].try_into().ok()?) as usize;
    Some(HEADER_LEN + compressed_len)
}

/// Materialize with a GNU-style restore hook for persistent references.
pub fn read_object_hooked(bytes: &[u8], hook: SEXP) -> Result<SEXP, String> {
    let payload = decode_frame(bytes)?;
    read::GraphReader::new(&payload).read_hooked(hook)
}

/// Encode the payload with the framing header.
fn encode_frame(payload: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write as _;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    if encoder.write_all(payload).is_err() {
        return encode_frame_raw(payload, COMPRESS_NONE);
    }
    match encoder.finish() {
        Ok(compressed) => {
            let mut out = Vec::with_capacity(HEADER_LEN + compressed.len());
            out.extend_from_slice(MAGIC);
            out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
            out.push(COMPRESS_GZIP);
            out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
            out.extend_from_slice(&(compressed.len() as u64).to_le_bytes());
            out.extend_from_slice(&compressed);
            out
        }
        Err(_) => encode_frame_raw(payload, COMPRESS_NONE),
    }
}

fn encode_frame_raw(payload: &[u8], method: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.push(method);
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Decode a frame, enforcing an upfront size budget from the header.
/// Frame header length: magic(4) + version(4) + compression(1)
/// + uncompressed_len(8) + compressed_len(8).
pub(crate) const HEADER_LEN: usize = 25;

fn decode_frame(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < HEADER_LEN || &bytes[..4] != MAGIC {
        return Err("not an RBG1 stream".into());
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != FORMAT_VERSION {
        return Err(format!("unsupported RBG1 version {version}"));
    }
    let method = bytes[8];
    let uncompressed_len = u64::from_le_bytes(bytes[9..17].try_into().unwrap()) as usize;
    let compressed_len = u64::from_le_bytes(bytes[17..25].try_into().unwrap()) as usize;
    // Upfront budget validation: refuse expansions before allocating.
    if uncompressed_len as u64 > (1 << 31) {
        return Err("RBG1 payload exceeds size budget".into());
    }
    let frame_end = HEADER_LEN
        .checked_add(compressed_len)
        .ok_or("RBG1 frame length overflow")?;
    if bytes.len() < frame_end {
        return Err("RBG1: truncated frame".into());
    }
    match method {
        COMPRESS_NONE => Ok(bytes[HEADER_LEN..frame_end].to_vec()),
        COMPRESS_GZIP => {
            use flate2::read::GzDecoder;
            use std::io::Read as _;
            let mut decoder = GzDecoder::new(&bytes[HEADER_LEN..frame_end]);
            let mut out = Vec::with_capacity(uncompressed_len);
            decoder
                .take(uncompressed_len as u64 + 1)
                .read_to_end(&mut out)
                .map_err(|e| format!("RBG1 decompression failed: {e}"))?;
            if out.len() != uncompressed_len {
                return Err("RBG1 payload length mismatch".into());
            }
            Ok(out)
        }
        other => Err(format!("unknown RBG1 compression {other}")),
    }
}

/// Snapshot the gp-bits + object flag metadata word for one node.
pub(crate) fn meta_of(s: SEXP) -> u32 {
    let object = unsafe { crate::sexp::object::Sexp::try_from_raw(s) };
    match object {
        Ok(obj) => {
            let header = obj.header();
            let gp = u32::from(header.sxpinfo.gp());
            let obj = u32::from(header.sxpinfo.obj());
            gp | (obj << 16)
        }
        Err(_) => 0,
    }
}

/// Restore gp-bits + object flag on a materialized node.
pub(crate) fn restore_meta(s: SEXP, meta: u32) {
    use crate::sexp::accessors::{SETLEVELS, SET_OBJECT, SET_S4_OBJECT, S4_OBJECT_MASK};
    let levs = (meta & 0xFFFF) as c_int;
    SETLEVELS(s, levs);
    if (meta >> 16) != 0 {
        SET_OBJECT(s, 1);
    }
    if (levs & S4_OBJECT_MASK as c_int) != 0 {
        SET_S4_OBJECT(s);
    }
}
