//! Canonical promise admission and cleanup, independent of ambient execution.
#![forbid(unsafe_code)]

use crate::sexp::{
    ffi::{NodeBody, SEXPTYPE, SexprecCore},
    object::{NodeDomain, Sexp, SexpError, SexpOwner, SexpResult},
};

pub(super) const RECURSIVE: &str =
    "promise already under evaluation: recursive default argument reference or earlier problems?";

fn snapshot(promise: &Sexp<'_>) -> SexpResult<SexprecCore> {
    let node = promise.allocation()?;
    let header = node
        .heap_identity()
        .node_snapshot(node)
        .ok_or(SexpError::StaleAllocation)?;
    if header.sxpinfo.type_of() != SEXPTYPE::PROMSXP || !matches!(header.data, NodeBody::Promise(_))
    {
        return Err(SexpError::TypeMismatch {
            expected: "promise",
            actual: header.sxpinfo.type_of(),
        });
    }
    Ok(header)
}

fn replace(promise: &Sexp<'_>, header: SexprecCore) -> SexpResult<()> {
    let node = promise.allocation()?;
    node.heap_identity()
        .replace_node(node, header)
        .ok_or(SexpError::StaleAllocation)
}

fn set_state(promise: &Sexp<'_>, state: u16) -> SexpResult<()> {
    let mut header = snapshot(promise)?;
    // GNU PRSEEN occupies the entire promise gp field.
    header.sxpinfo.set_gp(state);
    replace(promise, header)
}

pub(super) enum Admission<'s> {
    Evaluating,
    Ready(Evaluation<'s>),
    Restart(Restart<'s>),
}

impl<'s> Admission<'s> {
    pub(super) fn begin(promise: Sexp<'s>) -> SexpResult<Self> {
        let previous = snapshot(&promise)?.sxpinfo.gp();
        if previous == 1 {
            return Ok(Self::Evaluating);
        }
        set_state(&promise, 1)?;
        if previous == 0 {
            Ok(Self::Ready(Evaluation::new(promise)))
        } else {
            Ok(Self::Restart(Restart { promise }))
        }
    }
}

/// A restart warning precedes GNU pending-promise admission. If the warning
/// aborts (including warn=2), state remains evaluating; handler reentry must
/// already see that state. Only a returning warning admits ordinary cleanup.
pub(super) struct Restart<'s> {
    promise: Sexp<'s>,
}

impl<'s> Restart<'s> {
    pub(super) fn enter(self) -> Evaluation<'s> {
        Evaluation::new(self.promise)
    }
}

pub(super) struct Evaluation<'s> {
    promise: Sexp<'s>,
    armed: bool,
}

impl<'s> Evaluation<'s> {
    fn new(promise: Sexp<'s>) -> Self {
        Self {
            promise,
            armed: true,
        }
    }

    pub(super) fn publish(
        mut self,
        domain: &NodeDomain<'_>,
        value: &Sexp<'_>,
        nil: &Sexp<'_>,
    ) -> SexpResult<()> {
        // Authenticate every edge before changing either canonical header.
        let _live_promise = domain.wrap(self.promise.as_raw())?;
        domain.link(&self.promise)?;
        let value_link = domain.link(value)?;
        let nil_link = domain.link(nil)?;
        if value.owner() != SexpOwner::Static {
            let node = value.allocation()?;
            let heap = node.heap_identity();
            let mut value_header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
            value_header.sxpinfo.set_named(2);
            heap.replace_node(node, value_header)
                .ok_or(SexpError::StaleAllocation)?;
        }
        let mut header = snapshot(&self.promise)?;
        let NodeBody::Promise(ref mut body) = header.data else {
            unreachable!("snapshot authenticates the promise body");
        };
        body.value = value_link;
        body.env = nil_link;
        header.sxpinfo.set_gp(0);
        replace(&self.promise, header)?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for Evaluation<'_> {
    fn drop(&mut self) {
        if self.armed {
            // Passive, callback-free cleanup uses this retained allocation's
            // domain only. It never activates or dereferences a runtime; a
            // retired identity cannot select a replacement allocation.
            let _ = set_state(&self.promise, 2);
        }
    }
}
