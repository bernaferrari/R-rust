//! Owned copy of one node header.
//!
//! Checked handles copy their owning Cell after allocation validation. Only
//! foreign native views retain an unsafe physical-header read. No header loan
//! escapes either operation.

use std::os::raw::{c_double, c_int};

use super::Sexp;
pub(crate) use crate::sexp::ffi::NodeBody;
use crate::sexp::ffi::{SEXP, SEXPTYPE, SexprecCore, SxpInfo};
use crate::sexp::heap::{NodeLink, ResolvedLink};

/// Header fields copied out of a node. Graph links preserve exact identities; payload projections are copied values.
#[derive(Clone, Copy)]
pub(crate) struct HeaderSnap {
    pub sxpinfo: SxpInfo,
    pub attrib: NodeLink,
    /// `gengc_next_node`. For vectors this is the element buffer, not a SEXP.
    pub payload: SEXP,
    pub body: NodeBody,
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
        } else if let Some(core) = self.singleton_snapshot(self.ptr) {
            core
        } else if let Some(core) = crate::sexp::globals::immutable_singleton_snapshot(self.ptr) {
            core
        } else {
            // Only unsafe legacy factories can produce an unregistered view.
            // Their caller retains the original liveness/rooting contract.
            return read_legacy_header(self.ptr);
        };
        snapshot_header(core)
    }

    /// Copy a graph child's header using its saved allocation identity.
    /// No child pointer is reclassified or promoted to a new generation.
    #[inline]
    pub(crate) fn copied_header_link(&self, link: NodeLink) -> Option<HeaderSnap> {
        self.ensure_live().ok()?;
        let parent = self.reference_node().ok()?;
        match parent.heap_identity().resolve_link(link)? {
            ResolvedLink::Null => None,
            ResolvedLink::Singleton(owner) => Some(snapshot_header(owner.snapshot())),
            ResolvedLink::Node {
                projection,
                allocation,
            } => {
                crate::sexp::memory::checked_snapshot(projection, &allocation).map(snapshot_header)
            }
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
    HeaderSnap {
        sxpinfo: core.sxpinfo,
        attrib: core.attrib,
        payload: core.gengc_next_node,
        body: core.data,
    }
}
