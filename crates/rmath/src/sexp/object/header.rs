//! Owned copy of one node header.
//!
//! A safe collector handle (`gc::Gc`, `gc_arena::Gc`) does not give callers
//! the allocator's raw pointer. This runtime does not move live nodes, but it
//! does mutate them through raw pointers. [`Sexp::header`] copies the `Copy`
//! fields out in one read so no `&SexprecCore` escapes that read.

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
        read_header(self.ptr)
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
                let canonical = if let Some(owner) = self.session_owner_ptr {
                    // This handle retains the session lifetime; derive the
                    // child projection from its owned cell before reading.
                    unsafe { (*owner.as_ptr()).canonical_projection(ptr) }?
                } else {
                    crate::sexp::memory::checked_projection(ptr)?.0
                };
                let node = crate::sexp::memory::checked_node(canonical)?;
                if !parent_node.same_heap(&node) {
                    return None;
                }
                canonical
            }
        } else {
            ptr
        };
        Some(read_header(ptr))
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

fn read_header(ptr: SEXP) -> HeaderSnap {
    // SAFETY: `ptr` addresses a live node. Each loaded field is `Copy`. The
    // union arm follows the type tag copied first. No reference is returned,
    // and this read does not call back into R.
    unsafe {
        let sxpinfo = std::ptr::addr_of!((*ptr).sxpinfo).read();
        let attrib = std::ptr::addr_of!((*ptr).attrib).read();
        let payload = std::ptr::addr_of!((*ptr).gengc_next_node).read();
        let ty = sxpinfo.type_of();
        let body = if stores_vecsxp(ty) {
            NodeBody::Vector(std::ptr::addr_of!((*ptr).data.vecsxp).read())
        } else if ty.is_list_type() {
            NodeBody::List(std::ptr::addr_of!((*ptr).data.listsxp).read())
        } else if ty == SEXPTYPE::SYMSXP {
            NodeBody::Symbol(std::ptr::addr_of!((*ptr).data.symsxp).read())
        } else if ty == SEXPTYPE::CLOSXP {
            NodeBody::Closure(std::ptr::addr_of!((*ptr).data.closxp).read())
        } else if ty == SEXPTYPE::ENVSXP {
            NodeBody::Environment(std::ptr::addr_of!((*ptr).data.envsxp).read())
        } else if ty == SEXPTYPE::PROMSXP {
            NodeBody::Promise(std::ptr::addr_of!((*ptr).data.promsxp).read())
        } else if ty == SEXPTYPE::SPECIALSXP || ty == SEXPTYPE::BUILTINSXP {
            NodeBody::Primitive(std::ptr::addr_of!((*ptr).data.primsxp).read())
        } else if ty == SEXPTYPE::EXTPTRSXP {
            NodeBody::ExtPtr(std::ptr::addr_of!((*ptr).data.extptr).read())
        } else {
            NodeBody::Other
        };
        HeaderSnap {
            sxpinfo,
            attrib,
            payload,
            body,
        }
    }
}
