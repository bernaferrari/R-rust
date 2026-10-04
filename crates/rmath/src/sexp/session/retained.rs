//! Private host roots. R bindings never contain or resolve these slot identities.

use super::{REvalError, RResult, RSession, catch_eval_result};
use crate::sexp::{
    object::{Sexp, SexpError, SexpValue},
    owner::{RuntimeAccess, with_runtime},
};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_NAMESPACE: AtomicU64 = AtomicU64::new(1);

/// Opaque, session-scoped identity for a privately retained value.
///
/// This copyable identity contains no heap reference. The owning session checks
/// its namespace, slot and generation before every read, write and removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RetainedValueId {
    namespace: u64,
    slot: u32,
    generation: u32,
}

struct Slot {
    // None permanently retires an exhausted generation; it can never wrap.
    generation: Option<u32>,
    value: Option<Sexp<'static>>,
}

#[derive(Default)]
pub(super) struct RetainedValues {
    namespace: Option<u64>,
    slots: Vec<Slot>,
}

fn error(message: impl Into<String>) -> REvalError {
    REvalError {
        message: message.into(),
    }
}

fn ownership_error(value: SexpError) -> REvalError {
    error(value.to_string())
}

fn retained_operation<T>(
    original: &crate::sexp::owner::WeakOwner,
    operation: impl for<'execution> FnOnce(&'execution RuntimeAccess) -> RResult<T>,
) -> RResult<T> {
    with_runtime(original, |access| {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(access))) {
            Ok(result) => result,
            Err(payload) => match access.require_live() {
                // Native continuations can unwind before their own availability
                // check. The original pin still owns cleanup storage here.
                Err(failure) => Err(ownership_error(failure)),
                // An ordinary programming panic in a live runtime remains the
                // original panic, never a misleading R evaluation error.
                Ok(()) => std::panic::resume_unwind(payload),
            },
        }
    })
    .map_err(ownership_error)?
}

impl RetainedValues {
    fn prepare(&mut self) -> RResult<()> {
        if self.namespace.is_none() {
            self.namespace = Some(
                NEXT_NAMESPACE
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                        current.checked_add(1)
                    })
                    .map_err(|_| error("retained value session identities exhausted"))?,
            );
        }
        if !self
            .slots
            .iter()
            .any(|slot| slot.value.is_none() && slot.generation.is_some())
        {
            u32::try_from(self.slots.len()).map_err(|_| error("retained value slots exhausted"))?;
            self.slots
                .try_reserve(1)
                .map_err(|_| error("retained value allocation failed"))?;
        }
        Ok(())
    }

    fn insert(&mut self, value: Sexp<'static>) -> RResult<RetainedValueId> {
        self.prepare()?;
        let index = match self
            .slots
            .iter()
            .position(|slot| slot.value.is_none() && slot.generation.is_some())
        {
            Some(index) => index,
            None => {
                self.slots.push(Slot {
                    generation: Some(0),
                    value: None,
                });
                self.slots.len() - 1
            }
        };
        let slot = &mut self.slots[index];
        let id = RetainedValueId {
            namespace: self
                .namespace
                .ok_or_else(|| error("retained value namespace unavailable"))?,
            slot: u32::try_from(index).map_err(|_| error("retained value slots exhausted"))?,
            generation: slot
                .generation
                .ok_or_else(|| error("stale value handle: retired slot"))?,
        };
        slot.value = Some(value);
        Ok(id)
    }

    fn index(&self, id: RetainedValueId) -> RResult<usize> {
        if self.namespace != Some(id.namespace) {
            return Err(error("value handle belongs to another session"));
        }
        let index = id.slot as usize;
        let slot = self
            .slots
            .get(index)
            .ok_or_else(|| error("stale value handle: slot never existed"))?;
        if slot.generation != Some(id.generation) || slot.value.is_none() {
            return Err(error("stale value handle: slot was removed"));
        }
        Ok(index)
    }

    fn value(&self, id: RetainedValueId) -> RResult<Sexp<'static>> {
        self.slots[self.index(id)?]
            .value
            .clone()
            .ok_or_else(|| error("stale value handle"))
    }

    fn replace(&mut self, id: RetainedValueId, value: Sexp<'static>) -> RResult<()> {
        let index = self.index(id)?;
        self.slots[index].value = Some(value);
        Ok(())
    }

    fn remove(&mut self, id: RetainedValueId) -> RResult<()> {
        let index = self.index(id)?;
        let slot = &mut self.slots[index];
        slot.value = None;
        slot.generation = id.generation.checked_add(1);
        Ok(())
    }

    pub(super) fn clear(&mut self) {
        self.slots.clear();
        self.namespace = None;
    }
}

impl RSession {
    fn retained_owner(&self) -> RResult<crate::sexp::owner::WeakOwner> {
        if !self.is_active() {
            return Err(error("Session closed"));
        }
        let original = self
            .owner_token()
            .and_then(|token| token.weak_owner())
            .ok_or_else(|| error("retained value runtime unavailable"))?;
        let _live = original.pin().map_err(ownership_error)?;
        Ok(original)
    }

    /// Evaluate source globally and retain the successful result privately.
    /// No identity is published on parse, evaluation or allocation failure.
    pub fn define_retained(&mut self, code: &str) -> RResult<RetainedValueId> {
        let original = self.retained_owner()?;
        self.retained_values.prepare()?;
        let _activation = self.activate();
        retained_operation(&original, |access| {
            let value = self.eval_retained_source(access, code, None)?;
            mark_shared(access, &value)?;
            self.retained_values.insert(value)
        })
    }

    /// Copy a privately retained value into an owned Rust snapshot.
    pub fn retained_snapshot(&self, id: RetainedValueId) -> RResult<SexpValue> {
        self.with_retained_value(id, |value| value.to_owned_value().map_err(ownership_error))
    }

    /// Validate an identity without evaluating R code or consulting bindings.
    pub fn validate_retained(&self, id: RetainedValueId) -> RResult<()> {
        self.with_retained_value(id, |_| Ok(()))
    }

    /// Replace a slot only after evaluation succeeds. Ordinary R side effects
    /// remain observable, including mutations to shared reference objects.
    pub fn set_retained(&mut self, id: RetainedValueId, code: &str) -> RResult<()> {
        self.write_retained(id, code, false)
    }

    /// Evaluate in a fresh private child frame with the previous value bound to
    /// `.`. Failed evaluation keeps the old slot; shared environments retain
    /// their ordinary R reference semantics.
    pub fn update_retained(&mut self, id: RetainedValueId, code: &str) -> RResult<()> {
        self.write_retained(id, code, true)
    }

    fn write_retained(&mut self, id: RetainedValueId, code: &str, update: bool) -> RResult<()> {
        let original = self.retained_owner()?;
        let previous = self.retained_values.value(id)?;
        let _activation = self.activate();
        retained_operation(&original, |access| {
            access.domain().link(&previous).map_err(ownership_error)?;
            let value = self.eval_retained_source(access, code, update.then_some(&previous))?;
            mark_shared(access, &value)?;
            self.retained_values.replace(id, value)
        })
    }

    /// Remove the owning root and invalidate every copy of this identity.
    pub fn remove_retained(&mut self, id: RetainedValueId) -> RResult<()> {
        let original = self.retained_owner()?;
        self.retained_values.index(id)?;
        let _activation = self.activate();
        retained_operation(&original, |_| self.retained_values.remove(id))
    }

    // Embedding adapters budget and convert while the original operation is
    // active. This owning handle is never exposed by the public host API.
    pub(crate) fn with_retained_value<T>(
        &self,
        id: RetainedValueId,
        operation: impl FnOnce(Sexp<'static>) -> RResult<T>,
    ) -> RResult<T> {
        let original = self.retained_owner()?;
        let value = self.retained_values.value(id)?;
        let _activation = self.activate();
        retained_operation(&original, |access| {
            access.domain().link(&value).map_err(ownership_error)?;
            let result = operation(value)?;
            self.retained_values.index(id)?;
            access.require_active().map_err(ownership_error)?;
            Ok(result)
        })
    }

    fn eval_retained_source(
        &self,
        access: &RuntimeAccess,
        code: &str,
        previous: Option<&Sexp<'static>>,
    ) -> RResult<Sexp<'static>> {
        let domain = access.domain();
        let expression = access
            .with_arena(|arena| crate::eval::parser::parse(code, arena, domain.clone()))
            .map_err(ownership_error)?
            .map_err(|failure| error(failure.to_string()))?;
        let global = self
            .global_env()
            .ok_or_else(|| error("session has no global environment"))?
            .into_owned()
            .map_err(ownership_error)?;
        let environment = if let Some(previous) = previous {
            mark_shared(access, previous)?;
            let dot = access
                .with_native(|owner| {
                    let raw = unsafe { crate::sexp::symbol::Rf_install(c".".as_ptr()) };
                    owner.sexp(raw)?.into_owned()
                })
                .map_err(ownership_error)?;
            let allocator = access.allocator(&domain).map_err(ownership_error)?;
            let frame = allocator
                .pairlist_cell(previous, &domain.nil(), &dot)
                .map_err(ownership_error)?;
            access
                .with_native(|owner| {
                    // All graph inputs own physical leases before allocation; the
                    // native allocator publishes this frame before callbacks run.
                    let raw = unsafe {
                        crate::sexp::memory_ext::NewEnvironment(
                            frame.as_raw(),
                            global.as_raw(),
                            domain.nil().as_raw(),
                        )
                    };
                    owner.sexp(raw)?.into_owned()
                })
                .map_err(ownership_error)?
        } else {
            global
        };
        crate::mainutils::errors::clear_last_rendered_message();
        // This original RuntimeAccess pins the capture's starting instance;
        // dropping the nested guard restores capture even on Rust unwind.
        let _capture = crate::sexp::output::OutputCaptureGuard::start();
        catch_eval_result(|| {
            access
                .with_native(|owner| {
                    let value = unsafe {
                        crate::eval::eval::EvalContext::new(environment).eval(expression)
                    }
                    .map_err(|message| SexpError::EvaluationFailed { message })?;
                    owner.sexp(value.as_raw())?.into_owned()
                })
                .map_err(|failure| failure.to_string())
        })
    }
}

fn mark_shared(access: &RuntimeAccess, value: &Sexp<'_>) -> RResult<()> {
    access.domain().link(value).map_err(ownership_error)?;
    access
        .with_native(|_| {
            // Native metadata projection only; no payload borrow or callback.
            unsafe {
                let named = crate::sexp::accessors::NAMED(value.as_raw());
                crate::sexp::accessors::SET_NAMED(value.as_raw(), named.max(2));
            }
            Ok(())
        })
        .map_err(ownership_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::object::SexpValue;

    fn eval(session: &mut RSession, source: &str) {
        session.eval_code_with_output_capture(source).0.unwrap();
    }

    fn integers(values: &[i32]) -> SexpValue {
        SexpValue::IntegerVector(values.iter().copied().map(Some).collect())
    }

    #[test]
    fn owned_retained_private_roots_ignore_r_binding_interference_and_collect() {
        let mut session = RSession::new_for_gc_tests();
        let id = session.define_retained("x <- c(1L,2L); x").unwrap();
        let original = session.retained_values.value(id).unwrap().as_raw().addr();
        eval(
            &mut session,
            "..rport_handles.. <- list(h0=99L); exists <- function(...) FALSE; x[1L] <- 8L; gc()",
        );
        assert_eq!(session.retained_snapshot(id).unwrap(), integers(&[1, 2]));
        assert_eq!(
            session.retained_values.value(id).unwrap().as_raw().addr(),
            original
        );
        eval(&mut session, "rm(..rport_handles..); gc()");
        assert_eq!(session.retained_snapshot(id).unwrap(), integers(&[1, 2]));
    }

    #[test]
    fn owned_retained_failed_writes_keep_old_value_and_private_update_frame() {
        let mut session = RSession::new_for_gc_tests();
        eval(&mut session, ". <- 90L; side <- 0L");
        let id = session.define_retained("c(1L,2L)").unwrap();
        assert!(
            session
                .set_retained(id, "side <- 1L; gc(); stop('fail')")
                .is_err()
        );
        assert!(
            session
                .update_retained(id, ".[1L] <- 7L; gc(); stop('fail')")
                .is_err()
        );
        assert_eq!(session.retained_snapshot(id).unwrap(), integers(&[1, 2]));
        assert_eq!(session.find_var("side").unwrap().integer_elt(0), Some(1));
        assert_eq!(session.find_var(".").unwrap().integer_elt(0), Some(90));
        session.update_retained(id, ".[1L] <- 3L; gc(); .").unwrap();
        assert_eq!(session.retained_snapshot(id).unwrap(), integers(&[3, 2]));
        assert_eq!(session.find_var(".").unwrap().integer_elt(0), Some(90));
    }

    #[test]
    fn owned_retained_failures_null_reuse_and_forged_ids_are_transactional() {
        let mut session = RSession::new_for_gc_tests();
        assert!(session.define_retained("stop('fail')").is_err());
        assert!(session.define_retained("(").is_err());
        assert!(session.retained_values.slots.is_empty());
        let id = session.define_retained("NULL").unwrap();
        assert_eq!(id.slot, 0);
        assert_eq!(session.retained_snapshot(id).unwrap(), SexpValue::Null);
        let forged = RetainedValueId {
            generation: id.generation + 1,
            ..id
        };
        assert!(session.set_retained(forged, "forged_side <- 1L").is_err());
        assert!(session.find_var("forged_side").is_none());
        session.remove_retained(id).unwrap();
        let next = session.define_retained("11L").unwrap();
        assert_eq!(id.slot, next.slot);
        assert_ne!(id.generation, next.generation);
        assert!(session.retained_snapshot(id).is_err());
        assert!(session.update_retained(id, "stale_side <- 1L").is_err());
        assert!(session.find_var("stale_side").is_none());
        assert_eq!(
            session.retained_snapshot(next).unwrap(),
            SexpValue::Integer(Some(11))
        );
    }

    #[test]
    fn owned_retained_foreign_and_closed_sessions_fail_before_evaluation() {
        let mut first = RSession::new_for_gc_tests();
        let id = first.define_retained("7L").unwrap();
        let mut second = RSession::new_for_gc_tests();
        assert!(
            second
                .set_retained(id, "foreign_side <- 1L")
                .unwrap_err()
                .message
                .contains("another session")
        );
        assert!(second.find_var("foreign_side").is_none());
        first.close();
        assert!(first.retained_values.slots.is_empty());
        assert!(first.define_retained("closed_side <- 1L").is_err());
        assert!(first.retained_snapshot(id).is_err());
        assert!(first.validate_retained(id).is_err());
        assert!(first.set_retained(id, "1L").is_err());
        assert!(first.update_retained(id, ".").is_err());
        assert!(first.remove_retained(id).is_err());
    }

    #[test]
    fn owned_retained_generation_exhaustion_retires_slot_without_aliasing() {
        let mut session = RSession::new_for_gc_tests();
        let id = session.define_retained("1L").unwrap();
        session.retained_values.slots[id.slot as usize].generation = Some(u32::MAX);
        let last = RetainedValueId {
            generation: u32::MAX,
            ..id
        };
        session.remove_retained(last).unwrap();
        assert!(session.retained_values.slots[0].generation.is_none());
        let next = session.define_retained("2L").unwrap();
        assert_ne!(next.slot, last.slot);
        assert!(session.validate_retained(last).is_err());
        assert!(session.validate_retained(id).is_err());
        assert_eq!(
            session.retained_snapshot(next).unwrap(),
            SexpValue::Integer(Some(2))
        );
    }

    #[test]
    fn owned_retained_update_keeps_actual_original_root_during_callback_and_gc() {
        let mut session = RSession::new_for_gc_tests();
        let id = session.define_retained("list(c(1L,2L))").unwrap();
        let old = session.retained_values.value(id).unwrap();
        let child = old.try_vector_elt(0).unwrap();
        let observed = std::rc::Rc::new(std::cell::Cell::new(false));
        let callback_observed = observed.clone();
        session.with_active(|| {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                callback_observed.set(true);
                assert_eq!(child.integer_elt(0), Some(1));
                assert_eq!(child.integer_elt(1), Some(2));
            }))
        });
        session
            .update_retained(id, "gc(); .[[1L]][1L] <- 4L; .")
            .unwrap();
        assert!(observed.get(), "the update must really collect");
        assert_eq!(old.try_vector_elt(0).unwrap().integer_elt(0), Some(1));
        assert_eq!(
            session.retained_snapshot(id).unwrap(),
            SexpValue::List(vec![integers(&[4, 2])])
        );
    }

    #[test]
    fn owned_retained_closure_has_sole_private_root_until_removal_and_close() {
        let mut session = RSession::new_for_gc_tests();
        let id = session.define_retained("function() c(1L,2L)").unwrap();
        let identity = {
            let value = session.retained_values.value(id).unwrap();
            assert_eq!(value.typeof_(), crate::sexp::ffi::SEXPTYPE::CLOSXP);
            value.allocation().unwrap().clone()
        };
        // CheckedNode retains metadata, not the object or a root lease.
        assert_eq!(identity.root_count(), 1);
        let observed = std::rc::Rc::new(std::cell::Cell::new(false));
        let callback_observed = observed.clone();
        let callback_identity = identity.clone();
        session.with_active(|| {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                callback_observed.set(true);
                assert!(callback_identity.is_live());
                assert_eq!(callback_identity.root_count(), 1);
            }))
        });
        session.with_active(crate::sexp::gengc::full_gc);
        assert!(observed.get(), "the sole-root case must really collect");
        assert!(identity.is_live());
        assert_eq!(identity.root_count(), 1);
        // Remove the observing callback before testing retirement.
        session.with_active_in(|instance| unsafe { (*instance).gc_state.callbacks.clear() });
        session.update_retained(id, ".()").unwrap();
        session.with_active(crate::sexp::gengc::full_gc);
        assert!(
            !identity.is_live(),
            "replacement releases the original closure root"
        );
        assert!(identity.heap_identity().node_snapshot(&identity).is_none());
        let replacement = {
            let value = session.retained_values.value(id).unwrap();
            value.allocation().unwrap().clone()
        };
        session.remove_retained(id).unwrap();
        assert_eq!(replacement.root_count(), 0);
        session.with_active(crate::sexp::gengc::full_gc);
        assert!(!replacement.is_live());
        assert!(
            replacement
                .heap_identity()
                .node_snapshot(&replacement)
                .is_none()
        );
        let closed = session.define_retained("function() 3L").unwrap();
        let closed_identity = session
            .retained_values
            .value(closed)
            .unwrap()
            .allocation()
            .unwrap()
            .clone();
        session.close();
        assert_eq!(closed_identity.root_count(), 0);
        drop(session);
        assert!(!closed_identity.is_live());
    }

    #[test]
    fn owned_retained_publication_preserves_higher_existing_namedness() {
        let mut session = RSession::new_for_gc_tests();
        eval(&mut session, "x <- c(1L,2L)");
        session.with_active(|| {
            let value = session.find_var("x").unwrap();
            unsafe { crate::sexp::accessors::SET_NAMED(value.as_raw(), 3) };
        });
        let id = session.define_retained("x").unwrap();
        session.with_active(|| {
            let value = session.retained_values.value(id).unwrap();
            assert_eq!(unsafe { crate::sexp::accessors::NAMED(value.as_raw()) }, 3);
        });
    }

    #[test]
    fn owned_retained_live_owner_callback_panic_keeps_exact_payload_and_slot() {
        let mut session = RSession::new_for_gc_tests();
        let id = session.define_retained("7L").unwrap();
        session.with_active(|| {
            crate::sexp::gengc::register_gc_callback(Box::new(|_| {
                std::panic::panic_any(701_u32);
            }))
        });
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.set_retained(id, "gc(); 9L")
        }));
        let payload =
            outcome.expect_err("a live-owner programming panic must not become an R error");
        assert_eq!(payload.downcast_ref::<u32>(), Some(&701));
        assert_eq!(
            session.retained_snapshot(id).unwrap(),
            SexpValue::Integer(Some(7))
        );
        assert!(!session.inst().output_capture.borrow().is_capturing());
    }

    #[test]
    fn owned_retained_runtime_revocation_cannot_publish_partial_write() {
        let mut session = RSession::new_for_gc_tests();
        let id = session.define_retained("7L").unwrap();
        let old = session.retained_values.value(id).unwrap();
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let pin = owner.pin().unwrap();
        let revoked = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = revoked.clone();
        session.with_active_in(|instance| {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !observed.replace(true) {
                    unsafe { crate::sexp::instance::revoke_instance_availability(instance) };
                }
            }))
        });
        assert!(session.set_retained(id, "gc(); 9L").is_err());
        assert!(revoked.get());
        assert_eq!(
            session.retained_values.value(id).unwrap().as_raw(),
            old.as_raw()
        );
        assert!(session.retained_snapshot(id).is_err());
        assert!(owner.pin().is_err());
        assert!(!unsafe { (*pin.as_ptr()).output_capture.borrow().is_capturing() });
        // The old operation pin only keeps bytes allocated; it grants no new
        // access to a revoked runtime or replacement session.
        drop(pin);
    }
}
