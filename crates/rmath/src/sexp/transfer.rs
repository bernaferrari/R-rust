//! Thread-confined owning nonlocal transfers. Panic payloads carry identity only.
#![forbid(unsafe_code)]

use super::{
    context::RCNTXT,
    object::{Sexp, SexpError, SexpResult},
    owner::{OwnerPin, WeakOwner},
};
use std::{
    cell::{Cell, RefCell, UnsafeCell},
    collections::BTreeMap,
    rc::Rc,
    sync::Arc,
    thread::{self, ThreadId},
};

pub(crate) enum OwnedTransfer {
    Return {
        target: Option<Rc<UnsafeCell<RCNTXT>>>,
        value: Sexp<'static>,
    },
    Jump {
        target: Option<Rc<UnsafeCell<RCNTXT>>>,
        mask: i32,
        value: Sexp<'static>,
    },
    ExitingHandler {
        target_env: Sexp<'static>,
        result: Sexp<'static>,
    },
    Restart {
        target: Sexp<'static>,
        args: Sexp<'static>,
    },
}

/// Safe to transport or drop on any thread: contains no interpreter objects.
#[derive(Debug)]
pub struct TransferTicket {
    thread: ThreadId,
    scope: Arc<()>,
    entry: u64,
}

struct TransferScope {
    identity: Arc<()>,
    owner: WeakOwner,
    next: Cell<u64>,
    entries: RefCell<BTreeMap<u64, Rc<OwnedTransfer>>>,
}

thread_local! {
    static SCOPES: RefCell<Vec<(Arc<()>, Rc<TransferScope>)>> = const { RefCell::new(Vec::new()) };
}

pub(crate) struct TransferScopeGuard {
    activation: Arc<()>,
}
impl TransferScopeGuard {
    pub(crate) fn enter(owner: WeakOwner) -> SexpResult<Self> {
        owner.pin()?.require_live()?;
        let activation = Arc::new(());
        SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let scope = scopes
                .iter()
                .rev()
                .find(|(_, scope)| scope.owner.same_owner(&owner))
                .map(|(_, scope)| scope.clone())
                .unwrap_or_else(|| {
                    Rc::new(TransferScope {
                        identity: Arc::new(()),
                        owner,
                        next: Cell::new(0),
                        entries: RefCell::new(BTreeMap::new()),
                    })
                });
            scopes.push((activation.clone(), scope));
        });
        Ok(Self { activation })
    }
}
impl Drop for TransferScopeGuard {
    fn drop(&mut self) {
        // Drop the canonical values after releasing the TLS loan. Nested
        // activations retain the same scope until its last activation ends.
        let removed = SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let index = scopes
                .iter()
                .rposition(|(id, _)| Arc::ptr_eq(id, &self.activation));
            index.map(|index| scopes.remove(index))
        });
        drop(removed);
    }
}

fn active_scope() -> SexpResult<Rc<TransferScope>> {
    let owner = super::instance::current_instance_ptr().ok_or(SexpError::OwnerNotActive)?;
    SCOPES
        .with(|scopes| {
            scopes
                .borrow()
                .iter()
                .rev()
                .find(|(_, scope)| scope.owner.identity_ptr() == owner)
                .map(|(_, scope)| scope.clone())
        })
        .ok_or(SexpError::RootUnavailable)
}

pub(crate) fn active_owner_pin() -> SexpResult<OwnerPin> {
    active_scope()?.owner.pin()
}

pub(crate) fn publish(data: OwnedTransfer) -> SexpResult<TransferTicket> {
    let scope = active_scope()?;
    let pin = scope.owner.pin()?;
    let factory = scope.owner.node_factory()?;
    match &data {
        OwnedTransfer::Return { value, .. } | OwnedTransfer::Jump { value, .. } => {
            factory.link(value)?;
        }
        OwnedTransfer::ExitingHandler { target_env, result } => {
            factory.link(target_env)?;
            factory.link(result)?;
        }
        OwnedTransfer::Restart { target, args } => {
            factory.link(target)?;
            factory.link(args)?;
        }
    }
    pin.require_live()?;
    let entry = scope.next.get();
    scope
        .next
        .set(entry.checked_add(1).ok_or(SexpError::RootUnavailable)?);
    scope.entries.borrow_mut().insert(entry, Rc::new(data));
    Ok(TransferTicket {
        thread: thread::current().id(),
        scope: scope.identity.clone(),
        entry,
    })
}

pub(crate) struct TransferLease {
    data: Rc<OwnedTransfer>,
    pin: OwnerPin,
}
impl TransferLease {
    pub(crate) fn data(&self) -> &OwnedTransfer {
        &self.data
    }
    pub(crate) fn require_live(&self) -> SexpResult<()> {
        self.pin.require_live()?;
        if super::instance::current_instance_ptr() != Some(self.pin.as_ptr()) {
            return Err(SexpError::OwnerNotActive);
        }
        Ok(())
    }
}
impl TransferTicket {
    fn scope(&self) -> SexpResult<Rc<TransferScope>> {
        if thread::current().id() != self.thread {
            return Err(SexpError::RootUnavailable);
        }
        let scope = active_scope()?;
        if !Arc::ptr_eq(&scope.identity, &self.scope) {
            return Err(SexpError::OwnerNotActive);
        }
        Ok(scope)
    }
    pub(crate) fn resolve(&self) -> SexpResult<TransferLease> {
        let scope = self.scope()?;
        let pin = scope.owner.pin()?;
        let data = scope
            .entries
            .borrow()
            .get(&self.entry)
            .cloned()
            .ok_or(SexpError::RootUnavailable)?;
        Ok(TransferLease { data, pin })
    }
    pub(crate) fn take(self) -> SexpResult<TransferLease> {
        let scope = self.scope()?;
        let pin = scope.owner.pin()?;
        let data = scope
            .entries
            .borrow_mut()
            .remove(&self.entry)
            .ok_or(SexpError::RootUnavailable)?;
        Ok(TransferLease { data, pin })
    }
    pub(crate) fn discard(self) {
        let _ = self.take();
    }
}

impl Drop for TransferTicket {
    fn drop(&mut self) {
        if thread::current().id() != self.thread {
            return;
        }
        let scope = SCOPES
            .try_with(|scopes| {
                scopes
                    .borrow()
                    .iter()
                    .find(|(_, scope)| Arc::ptr_eq(&scope.identity, &self.scope))
                    .map(|(_, scope)| scope.clone())
            })
            .ok()
            .flatten();
        if let Some(scope) = scope {
            let removed = scope.entries.borrow_mut().remove(&self.entry);
            drop(removed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{
        context::{RSignal, handle_closure_signal, handle_loop_signal},
        ffi::SEXPTYPE,
        object::SexpMut,
        session::RSession,
    };

    fn value(session: &RSession, number: i32) -> Sexp<'static> {
        let value = session
            .owner_token()
            .unwrap()
            .node_factory()
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
            .unwrap();
        let mut value = SexpMut::try_from_checked(value).unwrap();
        value.try_set_integer_elt(0, number).unwrap();
        value.freeze().into_owned().unwrap()
    }
    fn returned(ticket: &TransferTicket) -> i32 {
        let lease = ticket.resolve().unwrap();
        let OwnedTransfer::Return { value, .. } = lease.data() else {
            panic!("incorrect kind")
        };
        value.try_integer_elt(0).unwrap()
    }
    #[test]
    fn tickets_are_send_without_transporting_interpreter_objects() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<TransferTicket>();
        send_sync::<RSignal>();
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let ticket = publish(OwnedTransfer::Return {
                target: None,
                value: value(&session, 37),
            })
            .unwrap();
            let ticket = std::thread::spawn(move || {
                assert!(matches!(ticket.resolve(), Err(SexpError::RootUnavailable)));
                ticket
            })
            .join()
            .unwrap();
            session.owner_token().unwrap().full_gc().unwrap();
            assert_eq!(returned(&ticket), 37);
            ticket.take().unwrap().require_live().unwrap();
            let dropped = publish(OwnedTransfer::Return {
                target: None,
                value: value(&session, 38),
            })
            .unwrap();
            std::thread::spawn(move || drop(dropped)).join().unwrap();
            assert_eq!(active_scope().unwrap().entries.borrow().len(), 1);
        });
        SCOPES.with(|scopes| assert!(scopes.borrow().is_empty()));
    }
    #[test]
    fn nested_activation_preserves_original_ticket_and_consumes_once() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session.with_active(|| {
                    std::panic::panic_any(RSignal::Return(
                        publish(OwnedTransfer::Return {
                            target: None,
                            value: value(&session, 59),
                        })
                        .unwrap(),
                    ));
                })
            }));
            let signal = unwind.unwrap_err().downcast::<RSignal>().unwrap();
            let RSignal::Return(ticket) = *signal else {
                panic!("incorrect signal")
            };
            session.owner_token().unwrap().full_gc().unwrap();
            assert_eq!(returned(&ticket), 59);
            let duplicate = TransferTicket {
                thread: ticket.thread,
                scope: ticket.scope.clone(),
                entry: ticket.entry,
            };
            let lease = ticket.take().unwrap();
            assert!(matches!(
                duplicate.resolve(),
                Err(SexpError::RootUnavailable)
            ));
            let OwnedTransfer::Return { value, .. } = lease.data() else {
                panic!("incorrect kind")
            };
            session.owner_token().unwrap().full_gc().unwrap();
            assert_eq!(value.try_integer_elt(0).unwrap(), 59);
        });
    }
    #[test]
    fn expired_scope_and_other_runtime_cannot_resolve_original_transfer() {
        let session = RSession::new_for_gc_tests();
        let other = RSession::new_for_gc_tests();
        let ticket = session.with_active(|| {
            let ticket = publish(OwnedTransfer::Return {
                target: None,
                value: value(&session, 61),
            })
            .unwrap();
            other.with_active(|| {
                assert!(matches!(ticket.resolve(), Err(SexpError::OwnerNotActive)))
            });
            assert_eq!(returned(&ticket), 61);
            ticket
        });
        session.with_active(|| {
            assert!(matches!(ticket.resolve(), Err(SexpError::OwnerNotActive)));
            assert!(active_scope().unwrap().entries.borrow().is_empty());
        });
    }
    #[test]
    fn foreign_heap_publication_rejected_and_revoked_lease_cannot_succeed() {
        let mut session = RSession::new_for_gc_tests();
        let other = RSession::new_for_gc_tests();
        let foreign = other.with_active(|| value(&other, 64));
        let ticket = session.with_active(|| {
            assert!(
                publish(OwnedTransfer::Return {
                    target: None,
                    value: foreign
                })
                .is_err()
            );
            publish(OwnedTransfer::Return {
                target: None,
                value: value(&session, 65),
            })
            .unwrap()
        });
        assert!(ticket.resolve().is_err());
        let weak = session.owner_token().unwrap().weak_owner().unwrap();
        let scope = TransferScopeGuard::enter(weak).unwrap();
        let lease = session.with_active(|| {
            publish(OwnedTransfer::Return {
                target: None,
                value: value(&session, 66),
            })
            .unwrap()
            .take()
            .unwrap()
        });
        session.close();
        assert!(matches!(
            lease.require_live(),
            Err(SexpError::RootUnavailable)
        ));
        drop(scope);
    }
    #[test]
    fn abandoned_owner_thread_tickets_release_roots_immediately() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let value = value(&session, 83);
            let allocation = value.allocation().unwrap().clone();
            let ticket = publish(OwnedTransfer::Return {
                target: None,
                value,
            })
            .unwrap();
            assert!(allocation.root_count() > 0);
            drop(ticket);
            assert_eq!(allocation.root_count(), 0);
            assert!(active_scope().unwrap().entries.borrow().is_empty());
            session.owner_token().unwrap().full_gc().unwrap();
            assert!(!allocation.is_live());
        });
    }

    #[test]
    fn generic_jump_is_preserved_through_closure_and_loop_catchers() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            for closure in [false, true] {
                let ticket = publish(OwnedTransfer::Jump {
                    target: None,
                    mask: 0x4000,
                    value: value(&session, 71),
                })
                .unwrap();
                let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let payload = Box::new(RSignal::Jump(ticket));
                    if closure {
                        handle_closure_signal(payload);
                    } else {
                        handle_loop_signal(payload);
                    }
                }));
                let signal = unwind.unwrap_err().downcast::<RSignal>().unwrap();
                let RSignal::Jump(ticket) = *signal else {
                    panic!("incorrect signal")
                };
                let lease = ticket.take().unwrap();
                let OwnedTransfer::Jump { mask, value, .. } = lease.data() else {
                    panic!("incorrect kind")
                };
                assert_eq!(*mask, 0x4000);
                assert_eq!(value.try_integer_elt(0).unwrap(), 71);
            }
        });
    }
}
