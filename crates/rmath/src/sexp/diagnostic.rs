#![forbid(unsafe_code)]
//! Evaluation diagnostics retain canonical, charged bytes through publication.
use super::{
    encoding::{self, TextEncoding},
    ffi::SEXPTYPE,
    memory::WorkspaceReservation,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
    payload::PayloadLease,
};

#[derive(Debug)]
pub(crate) enum EvaluationError {
    Access(SexpError),
    Diagnostic(Diagnostic),
}
impl From<SexpError> for EvaluationError {
    fn from(error: SexpError) -> Self {
        Self::Access(error)
    }
}
impl std::fmt::Display for EvaluationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Access(error) => error.fmt(f),
            Self::Diagnostic(error) => error.fmt(f),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Diagnostic {
    root: std::rc::Rc<super::heap::NodeRootLease>,
    payload: PayloadLease,
    len: usize,
}
fn allocation() -> SexpError {
    SexpError::AllocationFailed {
        object: "evaluation diagnostic",
    }
}
struct MessageCopy {
    bytes: Vec<u8>,
    // Physical data deallocates before its exact original-ledger ticket.
    _admission: Option<WorkspaceReservation>,
}
impl MessageCopy {
    fn new(
        access: &RuntimeAccess,
        parent: &Sexp<'_>,
        length: usize,
        limit: usize,
        utf8: bool,
        mut byte: impl FnMut(usize) -> u8,
    ) -> SexpResult<Self> {
        access.require_active()?;
        access.domain().link(parent)?;
        let copied = length.min(limit);
        let ledger = parent.allocation()?.heap_identity().byte_ledger();
        let mut admission = if copied == 0 {
            None
        } else {
            Some(ledger.reserve(copied).ok_or_else(allocation)?)
        };
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(copied).map_err(|_| allocation())?;
        if let Some(ticket) = &mut admission {
            ticket.try_resize(bytes.capacity()).ok_or_else(allocation)?;
        }
        for i in 0..copied {
            bytes.push(byte(i));
            access.require_active()?;
        }
        // GNU Rvsnprintf_mbcs only repairs the final sequence when truncated.
        // Invalid bytes earlier in an error message must remain intact.
        if length > limit && utf8 && !bytes.is_empty() {
            let mut last = bytes.len() - 1;
            while last > 0 && bytes[last] & 0xc0 == 0x80 {
                last -= 1;
            }
            if std::str::from_utf8(&bytes[last..]).is_err() {
                bytes.truncate(last);
            }
        }
        Ok(Self {
            bytes,
            _admission: admission,
        })
    }
}

impl Diagnostic {
    pub(crate) fn new(
        access: &RuntimeAccess,
        parent: &Sexp<'_>,
        length: usize,
        limit: usize,
        utf8: bool,
        byte: impl FnMut(usize) -> u8,
    ) -> SexpResult<Self> {
        let copy = MessageCopy::new(access, parent, length, limit, utf8, byte)?;
        Self::from_copy(access, parent, copy)
    }

    pub(crate) fn from_workspace(
        access: &RuntimeAccess,
        parent: &Sexp<'_>,
        bytes: Vec<u8>,
        ticket: WorkspaceReservation,
    ) -> SexpResult<Self> {
        let copy = MessageCopy {
            bytes,
            _admission: Some(ticket),
        };
        access.require_active()?;
        access.domain().link(parent)?;
        let ticket = copy._admission.as_ref().expect("prepared workspace ticket");
        if !ticket.belongs_to(&parent.allocation()?.heap_identity().byte_ledger()) {
            return Err(SexpError::HeapDomainMismatch);
        }
        if ticket.bytes() != copy.bytes.capacity() {
            return Err(allocation());
        }
        Self::from_copy(access, parent, copy)
    }

    fn from_copy(access: &RuntimeAccess, parent: &Sexp<'_>, copy: MessageCopy) -> SexpResult<Self> {
        access.require_active()?;
        access.domain().link(parent)?;
        let bytes = &copy.bytes;
        let domain = access.domain();
        let message = access
            .allocator(&domain)?
            .allocate(|arena| encoding::allocate_character(arena, bytes, TextEncoding::Native))?;
        let node = message.allocation()?;
        let payload = node
            .heap_identity()
            .payload_lease(node)
            .ok_or(SexpError::MissingData {
                sexptype: SEXPTYPE::CHARSXP,
            })?;
        if !payload.is_immutable() || payload.byte_elt(bytes.len()) != Some(0) {
            return Err(SexpError::MissingData {
                sexptype: SEXPTYPE::CHARSXP,
            });
        }
        let root = message.allocation_root()?;
        Ok(Self {
            len: bytes.len(),
            root,
            payload,
        })
    }

    pub(crate) fn message(&self, access: &RuntimeAccess) -> SexpResult<Sexp<'static>> {
        access.require_active()?;
        access.domain().rooted_value(&self.root)
    }
    pub(crate) fn len(&self) -> usize {
        self.len
    }
    pub(crate) fn byte(&self, index: usize) -> Option<u8> {
        (index < self.len).then(|| {
            self.payload
                .byte_elt(index)
                .expect("sealed diagnostic payload")
        })
    }

    pub(crate) fn condition(
        &self,
        access: &RuntimeAccess,
        call: &Sexp<'_>,
        names_symbol: &Sexp<'_>,
        class_symbol: &Sexp<'_>,
    ) -> SexpResult<Sexp<'static>> {
        access.require_active()?;
        let domain = access.domain();
        let text = self.message(access)?;
        for value in [&text, call, names_symbol, class_symbol] {
            domain.link(value)?;
        }
        let allocator = access.allocator(&domain)?;
        let mut message = SexpMut::try_from_checked(
            allocator.allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 1)))?,
        )?;
        message.try_set_string_elt(0, text)?;
        let mut condition = SexpMut::try_from_checked(
            allocator.allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, 2)))?,
        )?;
        condition.try_set_vector_elt(0, message.freeze())?;
        condition.try_set_vector_elt(1, call.clone())?;
        let names = allocator.strings(&["message", "call"])?;
        let classes = allocator.strings(&["simpleError", "error", "condition"])?;
        let attrs = allocator.pairlist_cell(&classes, &domain.nil(), class_symbol)?;
        let attrs = allocator.pairlist_cell(&names, &attrs, names_symbol)?;
        condition.try_set_attribute(&attrs)?;
        let condition = condition.freeze();
        let node = condition.allocation()?;
        let heap = node.heap_identity();
        let mut header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
        header.sxpinfo.set_obj(true);
        heap.replace_node(node, header)
            .ok_or(SexpError::StaleAllocation)?;
        condition.into_owned()
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut i = 0;
        while i < self.len {
            let byte = self.byte(i).expect("diagnostic bounds");
            let width = match byte {
                0..=127 => 1,
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => 0,
            };
            let mut raw = [0; 4];
            if width != 0 && i + width <= self.len {
                for (j, out) in raw[..width].iter_mut().enumerate() {
                    *out = self.byte(i + j).expect("diagnostic bounds");
                }
                if let Ok(text) = std::str::from_utf8(&raw[..width]) {
                    f.write_str(text)?;
                    i += width;
                    continue;
                }
            }
            write!(f, "\\x{byte:02x}")?;
            i += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "diagnostic_tests.rs"]
mod tests;
