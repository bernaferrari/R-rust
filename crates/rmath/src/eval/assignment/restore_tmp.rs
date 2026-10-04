//! Temporary binding cleanup uses physical ownership, never ambient execution.
#![forbid(unsafe_code)]

use crate::sexp::{
    ffi::{EdgeField, NodeBody, SEXPTYPE},
    heap::{HeapIdentity, NodeLink, ResolvedLink},
    object::{Sexp, SexpError, SexpResult},
    owner::{OwnerPin, StoredOwner},
};

pub(super) struct RestoreTmp {
    authority: StoredOwner<'static>,
    pin: OwnerPin,
    environment: Sexp<'static>,
    symbol: Sexp<'static>,
    saved: Option<(Sexp<'static>, Sexp<'static>)>,
    armed: bool,
}

fn shape_error() -> SexpError {
    SexpError::EvaluationFailed {
        message: "invalid temporary binding frame".to_owned(),
    }
}

fn next(heap: &HeapIdentity, cell: NodeLink) -> SexpResult<Option<NodeLink>> {
    match heap.resolve_link(cell).ok_or(SexpError::StaleAllocation)? {
        ResolvedLink::Null => Ok(None),
        ResolvedLink::Singleton(value)
            if value.snapshot().sxpinfo.type_of() == SEXPTYPE::NILSXP =>
        {
            Ok(None)
        }
        ResolvedLink::Node { allocation, .. } => {
            let header = heap
                .node_snapshot(&allocation)
                .ok_or(SexpError::StaleAllocation)?;
            match header.data {
                NodeBody::List(body) if header.sxpinfo.type_of() == SEXPTYPE::LISTSXP => {
                    Ok(Some(body.cdrval))
                }
                _ => Err(shape_error()),
            }
        }
        _ => Err(shape_error()),
    }
}

/// Prove termination without allocating a visited set during unwinding.
fn acyclic(heap: &HeapIdentity, head: NodeLink) -> SexpResult<()> {
    let mut slow = head;
    let mut fast = head;
    loop {
        let Some(step) = next(heap, fast)? else {
            return Ok(());
        };
        let Some(step) = next(heap, step)? else {
            return Ok(());
        };
        fast = step;
        let Some(step) = next(heap, slow)? else {
            return Ok(());
        };
        slow = step;
        if slow == fast {
            return Err(shape_error());
        }
    }
}

impl RestoreTmp {
    pub(super) fn capture(environment: Sexp<'static>, symbol: Sexp<'static>) -> SexpResult<Self> {
        let authority = StoredOwner::from_value(&environment)?.into_owned()?;
        authority.require_active()?;
        let pin = authority
            .managed()
            .ok_or(SexpError::RootUnavailable)?
            .pin()?;
        let mut result = Self {
            authority,
            pin,
            environment,
            symbol,
            saved: None,
            armed: false,
        };
        let (_, cell) = result.find()?;
        if let Some(cell) = cell {
            // GNU reads R_GetVarLocValue before checking the recorded cell's
            // lock/active flags. Its getter can collect or unwind; admission
            // is deliberately unarmed until those checks have all succeeded.
            let _old_child = cell.try_car()?.into_owned()?;
            let value = super::restore_tmp_read(
                &result.authority,
                &result.pin,
                &result.environment,
                &result.symbol,
            )?;
            let (_, current) = result.find()?;
            if current.as_ref() != Some(&cell)
                || super::restore_tmp_locked(&result.pin, &result.environment, &result.symbol)
            {
                return Err(SexpError::EvaluationFailed {
                    message: "existing `*tmp*` binding is locked".to_owned(),
                });
            }
            if super::restore_tmp_active(&result.pin, &result.environment, &result.symbol) {
                return Err(SexpError::EvaluationFailed {
                    message: "existing `*tmp*` binding is an active binding".to_owned(),
                });
            }
            if value != result.environment.node_factory()?.unbound() {
                result.saved = Some((cell, value));
            }
        }
        result.armed = true;
        Ok(result)
    }

    fn find(&self) -> SexpResult<(Option<Sexp<'static>>, Option<Sexp<'static>>)> {
        let env = self.environment.allocation()?;
        let heap = env.heap_identity();
        let header = heap.node_snapshot(env).ok_or(SexpError::StaleAllocation)?;
        let NodeBody::Environment(body) = header.data else {
            return Err(shape_error());
        };
        acyclic(&heap, body.frame)?;
        let symbol = self.symbol.link_in(&heap)?;
        let mut current = body.frame;
        let mut previous = None;
        while let Some(tail) = next(&heap, current)? {
            let cell = self.environment.checked_child(current)?.into_owned()?;
            let node = cell.allocation()?;
            if heap.edge(node, EdgeField::ListTag) == Some(symbol) {
                return Ok((previous, Some(cell)));
            }
            previous = Some(cell);
            current = tail;
        }
        Ok((None, None))
    }

    fn edge(&self, parent: &Sexp<'_>, field: EdgeField, child: &Sexp<'_>) -> SexpResult<()> {
        let node = parent.allocation()?;
        let heap = node.heap_identity();
        let link = child.link_in(&heap)?;
        super::restore_tmp_barrier(&self.pin, parent, child)?;
        let mut header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
        header.set_edge(field, link).ok_or_else(shape_error)?;
        heap.replace_node(node, header)
            .ok_or(SexpError::StaleAllocation)
    }

    fn remove(&self) -> SexpResult<()> {
        let (previous, cell) = self.find()?;
        if let Some(cell) = cell {
            let tail = cell.try_cdr()?.into_owned()?;
            if let Some(previous) = previous {
                self.edge(&previous, EdgeField::ListCdr, &tail)?;
            } else {
                self.edge(&self.environment, EdgeField::EnvironmentFrame, &tail)?;
            }
            super::restore_tmp_remove_metadata(&self.pin, &self.environment, &self.symbol);
        }
        Ok(())
    }

    pub(super) fn finish(mut self) -> SexpResult<()> {
        self.authority.require_active()?;
        // GNU ends its C cleanup context before restoring a previous binding.
        // The recorded cell, rather than a fresh symbol lookup, is restored.
        // A removed/replaced or late-locked cell errors without a second remove.
        self.armed = false;
        if let Some((saved_cell, saved_value)) = &self.saved {
            let (_, current) = self.find()?;
            if current.as_ref() != Some(saved_cell)
                || super::restore_tmp_locked(&self.pin, &self.environment, &self.symbol)
            {
                return Err(SexpError::EvaluationFailed {
                    message: "cannot change value of locked binding for '*tmp*'".to_owned(),
                });
            }
            self.edge(saved_cell, EdgeField::ListCar, saved_value)?;
        } else {
            self.remove()?;
        }
        self.authority.require_active()?;
        Ok(())
    }

    pub(super) fn run<T>(self, operation: impl FnOnce() -> T) -> SexpResult<T> {
        self.authority.require_active()?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
        // Keep both the original allocation and its cleanup guard through the
        // complete unwind. A closed runtime cannot publish a value or borrow
        // another runtime's error state. Live signals keep their exact payload.
        self.pin.require_live()?;
        match result {
            Ok(value) => {
                self.finish()?;
                Ok(value)
            }
            Err(payload) => {
                drop(self);
                std::panic::resume_unwind(payload)
            }
        }
    }
}

impl Drop for RestoreTmp {
    fn drop(&mut self) {
        if self.armed {
            // GNU's error cleanup removes *tmp* rather than restoring its old
            // binding. Physical leases permit this even after revocation. An
            // invalid user-mutated frame is left unchanged; never panic twice.
            let _ = self.remove();
        }
    }
}
