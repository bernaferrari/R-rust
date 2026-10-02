//! Owned copy of one node header.
//!
//! Checked handles copy their owning Cell after allocation validation. Only
//! foreign native views retain an unsafe physical-header read. No header loan
//! escapes either operation.

use std::os::raw::{c_double, c_int, c_void};

use super::Sexp;
use crate::sexp::ffi::{
    Closxp, Envsxp, Listsxp, Primsxp, Promsxp, SEXP, SEXPTYPE, SexprecCore, SxpInfo, Symsxp, Vecsxp,
};

/// Header fields copied out of a node. Every pointer here is a value, not a borrow.
#[derive(Clone, Copy)]
pub(crate) struct HeaderSnap {
    pub sxpinfo: SxpInfo,
    pub attrib: SEXP,
    /// `gengc_next_node`. For vectors this is the element buffer, not a SEXP.
    pub payload: SEXP,
    pub body: NodeBody,
}

/// The union arm selected by the type tag. Other arms are not read.
#[derive(Clone, Copy)]
pub(crate) enum NodeBody {
    Vector(Vecsxp),
    List(Listsxp),
    Symbol(Symsxp),
    Closure(Closxp),
    Environment(Envsxp),
    Promise(Promsxp),
    Primitive(Primsxp),
    ExtPtr([*mut c_void; 3]),
    Other,
}

/// First two scalars of a plain integer or real buffer.
#[derive(Clone, Copy)]
pub(crate) enum LeadingScalars {
    Int(c_int, c_int),
    Real(c_double, c_double),
}

impl HeaderSnap {
    #[inline]
    pub(crate) fn type_of(self) -> SEXPTYPE {
        self.sxpinfo.type_of()
    }

    /// Byte-compare a CHARSXP payload to `expected`.
    ///
    /// The slice lives only for the comparison. A length mismatch or a missing
    /// buffer is not a match.
    pub(crate) fn char_eq(self, expected: &[u8]) -> bool {
        if self.type_of() != SEXPTYPE::CHARSXP {
            return false;
        }
        let NodeBody::Vector(vec) = self.body else {
            return false;
        };
        if vec.length < 0 || vec.length as usize != expected.len() {
            return false;
        }
        if expected.is_empty() {
            return true;
        }
        let bytes = self.payload as *const u8;
        if bytes.is_null() {
            return false;
        }
        // SAFETY: the CHARSXP header length is the byte count, excluding the
        // trailing NUL. The slice is dropped before this function returns.
        unsafe { std::slice::from_raw_parts(bytes, expected.len()) == expected }
    }
}

impl<'a> Sexp<'a> {
    /// Copy this node's header. Does not allocate and does not borrow the node.
    #[inline]
    pub(crate) fn header(&self) -> HeaderSnap {
        self.ensure_live()
            .expect("SEXP allocation has been reclaimed");
        let core = if let Some(node) = &self.node {
            crate::sexp::memory::checked_snapshot(self.ptr, node)
                .expect("SEXP allocation has been reclaimed")
        } else if let Some(core) = crate::sexp::globals::immutable_singleton_snapshot(self.ptr) {
            core
        } else {
            // Only unsafe legacy factories can produce an unregistered view.
            // Their caller retains the original liveness/rooting contract.
            return read_legacy_header(self.ptr);
        };
        snapshot_header(core)
    }

    /// Copy the header of a pointer just loaded from a live node in this graph.
    ///
    /// Null, misaligned, and non-canonical addresses yield `None`. This does
    /// not mint a child handle and does not protect, so it does not allocate.
    #[inline]
    pub(crate) fn copied_header(&self, ptr: SEXP) -> Option<HeaderSnap> {
        if !self.is_live() || !canonical_node(ptr) {
            return None;
        }
        let ptr = if let Some(parent_node) = &self.node {
            if let Some(canonical) = crate::sexp::session::immutable_singleton_projection(ptr) {
                canonical
            } else {
                let (canonical, node) = crate::sexp::memory::checked_projection(ptr)?;
                if !parent_node.same_heap(&node) {
                    return None;
                }
                canonical
            }
        } else {
            ptr
        };
        if let Some(core) = crate::sexp::globals::immutable_singleton_snapshot(ptr) {
            Some(snapshot_header(core))
        } else if let Some((canonical, node)) = crate::sexp::memory::checked_projection(ptr) {
            crate::sexp::memory::checked_snapshot(canonical, &node).map(snapshot_header)
        } else if self.node.is_none() && self.owner == super::SexpOwner::Unknown {
            Some(read_legacy_header(ptr))
        } else {
            None
        }
    }
}

/// Copy the first two buffer elements when `header` is an integer or real
/// vector of length at least 2. Does not allocate or expand a sequence.
pub(crate) fn copy_leading_scalars(header: HeaderSnap) -> Option<LeadingScalars> {
    let NodeBody::Vector(vec) = header.body else {
        return None;
    };
    if vec.length < 2 || header.payload.is_null() {
        return None;
    }
    // SAFETY: the length and type were copied from the same header. The two
    // scalars are copied by value; no element reference is returned.
    unsafe {
        match header.type_of() {
            SEXPTYPE::INTSXP => {
                let ptr = header.payload.cast::<c_int>();
                if (ptr as usize) % std::mem::align_of::<c_int>() != 0 {
                    return None;
                }
                Some(LeadingScalars::Int(ptr.read(), ptr.add(1).read()))
            }
            SEXPTYPE::REALSXP => {
                let ptr = header.payload.cast::<c_double>();
                if (ptr as usize) % std::mem::align_of::<c_double>() != 0 {
                    return None;
                }
                Some(LeadingScalars::Real(ptr.read(), ptr.add(1).read()))
            }
            _ => None,
        }
    }
}

fn canonical_node(ptr: SEXP) -> bool {
    let addr = ptr as usize;
    addr >= 0x1000 && addr % std::mem::align_of::<SexprecCore>() == 0
}

fn stores_vecsxp(ty: SEXPTYPE) -> bool {
    ty == SEXPTYPE::CHARSXP || ty.is_vector_type()
}

fn read_legacy_header(ptr: SEXP) -> HeaderSnap {
    // Prefer owned snapshots even for a legacy wrapper. Only foreign native
    // memory uses the factory's explicit unsafe liveness contract.
    if let Some(core) = crate::sexp::globals::immutable_singleton_snapshot(ptr) {
        return snapshot_header(core);
    }
    if let Some((canonical, node)) = crate::sexp::memory::checked_projection(ptr) {
        if let Some(core) = crate::sexp::memory::checked_snapshot(canonical, &node) {
            return snapshot_header(core);
        }
    }
    // SAFETY: an unsafe legacy factory promised a live initialized header.
    snapshot_header(unsafe { ptr.read() })
}

fn snapshot_header(core: SexprecCore) -> HeaderSnap {
    let sxpinfo = core.sxpinfo;
    let ty = sxpinfo.type_of();
    // SAFETY: interpret only the arm selected by the owned header's type tag.
    // Physical copying and allocation validation happen in safe Rust first.
    let body = unsafe {
        if stores_vecsxp(ty) {
            NodeBody::Vector(core.data.vecsxp)
        } else if ty.is_list_type() {
            NodeBody::List(core.data.listsxp)
        } else if ty == SEXPTYPE::SYMSXP {
            NodeBody::Symbol(core.data.symsxp)
        } else if ty == SEXPTYPE::CLOSXP {
            NodeBody::Closure(core.data.closxp)
        } else if ty == SEXPTYPE::ENVSXP {
            NodeBody::Environment(core.data.envsxp)
        } else if ty == SEXPTYPE::PROMSXP {
            NodeBody::Promise(core.data.promsxp)
        } else if ty == SEXPTYPE::SPECIALSXP || ty == SEXPTYPE::BUILTINSXP {
            NodeBody::Primitive(core.data.primsxp)
        } else if ty == SEXPTYPE::EXTPTRSXP {
            NodeBody::ExtPtr(core.data.extptr)
        } else {
            NodeBody::Other
        }
    };
    HeaderSnap {
        sxpinfo,
        attrib: core.attrib,
        payload: core.gengc_next_node,
        body,
    }
}
