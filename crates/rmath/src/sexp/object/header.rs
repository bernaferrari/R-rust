#![forbid(unsafe_code)]
//! Copied headers retain their actual typed payload allocation.

use super::Sexp;
pub(crate) use crate::sexp::ffi::NodeBody;
use crate::sexp::ffi::{SEXPTYPE, SexprecCore, SxpInfo};
use crate::sexp::heap::{NodeLink, ResolvedLink};
use crate::sexp::payload::{PayloadLease, PayloadLink};

/// A header snapshot owns the original payload; a copied link alone is not
/// authority to read cells, and no native payload pointer is stored here.
#[derive(Clone)]
pub(crate) struct HeaderSnap {
    pub sxpinfo: SxpInfo,
    pub attrib: NodeLink,
    pub payload: PayloadLink,
    pub body: NodeBody,
    payload_lease: Option<PayloadLease>,
}

impl HeaderSnap {
    pub(crate) fn type_of(&self) -> SEXPTYPE {
        self.sxpinfo.type_of()
    }

    fn new(core: SexprecCore, lease: Option<PayloadLease>) -> Option<Self> {
        if !core.has_valid_shape() {
            return None;
        }
        match core.data {
            NodeBody::Vector(vector) => {
                let length = usize::try_from(vector.length).ok()?;
                match &lease {
                    Some(lease) if lease.matches_header(&core) => {}
                    None if core.payload.is_empty() && (length == 0 || core.sxpinfo.alt()) => {}
                    _ => return None,
                }
            }
            _ if core.payload.is_empty() && lease.is_none() => {}
            _ => return None,
        }
        Some(Self {
            sxpinfo: core.sxpinfo,
            attrib: core.attrib,
            payload: core.payload,
            body: core.data,
            payload_lease: lease,
        })
    }

    pub(crate) fn payload_lease(&self) -> Option<&PayloadLease> {
        self.payload_lease.as_ref()
    }

    pub(crate) fn char_eq(&self, expected: &[u8]) -> bool {
        if self.type_of() != SEXPTYPE::CHARSXP {
            return false;
        }
        let NodeBody::Vector(vector) = self.body else {
            return false;
        };
        if usize::try_from(vector.length).ok() != Some(expected.len()) {
            return false;
        }
        if expected.is_empty() {
            return true;
        }
        let Some(lease) = &self.payload_lease else {
            return false;
        };
        expected
            .iter()
            .enumerate()
            .all(|(index, value)| lease.byte_elt(index) == Some(*value))
    }

    pub(crate) fn char_bytes(&self) -> Option<Vec<u8>> {
        if self.type_of() != SEXPTYPE::CHARSXP {
            return None;
        }
        let NodeBody::Vector(vector) = self.body else {
            return None;
        };
        let length = usize::try_from(vector.length).ok()?;
        if length == 0 {
            return Some(Vec::new());
        }
        let lease = self.payload_lease.as_ref()?;
        (0..length).map(|index| lease.byte_elt(index)).collect()
    }
}

impl<'a> Sexp<'a> {
    pub(crate) fn header(&self) -> HeaderSnap {
        self.ensure_live()
            .expect("SEXP allocation has been reclaimed");
        if let Some(node) = &self.node {
            let heap = node.heap_identity();
            return HeaderSnap::new(
                heap.node_snapshot(node).expect("live header"),
                heap.payload_lease(node),
            )
            .expect("canonical header and payload must agree");
        }
        if let Some(singleton) = self
            .singleton
            .clone()
            .or_else(|| {
                self.singletons
                    .as_ref()
                    .and_then(|pool| pool.lease(self.ptr))
            })
            .or_else(|| crate::sexp::globals::immutable_singleton_lease(self.ptr))
        {
            return HeaderSnap::new(singleton.snapshot(), singleton.payload_lease())
                .expect("immutable header and payload must agree");
        }
        // Unsafe raw factories may select an owned projection, but cannot
        // turn foreign bytes into safe header or payload authority.
        let (_, node) = crate::sexp::memory::checked_projection(self.ptr).expect("unowned header");
        let heap = node.heap_identity();
        HeaderSnap::new(
            heap.node_snapshot(&node).expect("live header"),
            heap.payload_lease(&node),
        )
        .expect("canonical header and payload must agree")
    }

    pub(crate) fn copied_header_link(&self, link: NodeLink) -> Option<HeaderSnap> {
        self.ensure_live().ok()?;
        let parent = self.reference_node().ok()?;
        let heap = parent.heap_identity();
        match heap.resolve_link(link)? {
            ResolvedLink::Null => None,
            ResolvedLink::Singleton(lease) => {
                HeaderSnap::new(lease.snapshot(), lease.payload_lease())
            }
            ResolvedLink::Node { allocation, .. } => HeaderSnap::new(
                heap.node_snapshot(&allocation)?,
                heap.payload_lease(&allocation),
            ),
        }
    }
}
