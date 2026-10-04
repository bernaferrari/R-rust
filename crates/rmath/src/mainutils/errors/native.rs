//! Owning Rust condition scopes and the narrow native callback adapters.
//! Native function pointers explicitly permit R's unwind transport.

use crate::sexp::{
    context::{self, ContextGuard, ContextValue, RError, RSignal},
    ffi::{SEXP, SEXPTYPE},
    object::{SessionNodeFactory, Sexp, SexpMut, SexpResult},
    owner::{OwnerPin, OwnerToken, WeakOwner},
    transfer::OwnedTransfer,
};
use std::{
    cell::Cell,
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
};

pub type NativeBody = unsafe extern "C-unwind" fn(*mut c_void) -> SEXP;
pub type NativeHandler = unsafe extern "C-unwind" fn(SEXP, *mut c_void) -> SEXP;
pub type NativeFinally = unsafe extern "C-unwind" fn(*mut c_void);

fn checked<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|e| context::r_error(e.to_string()))
}
fn owned(value: Sexp<'_>) -> Sexp<'static> {
    checked(value.into_owned())
}
fn factory_and_pin() -> (SessionNodeFactory<'static>, OwnerPin) {
    let token = checked(unsafe { OwnerToken::current() });
    let pin = checked(token.pin())
        .unwrap_or_else(|| context::r_error("condition scopes require a managed runtime"));
    let owner = token.weak_owner().expect("pinned managed runtime");
    (checked(owner.node_factory()), pin)
}

struct HandlerStackScope {
    pin: OwnerPin,
    previous: Option<ContextValue>,
}
impl HandlerStackScope {
    fn capture(pin: OwnerPin) -> Self {
        let previous = unsafe { (*pin.as_ptr()).error_state.handler_stack.clone() };
        Self {
            pin,
            previous: Some(previous),
        }
    }
    fn restore(&mut self) {
        if let Some(previous) = self.previous.take() {
            // The original allocation stays physically pinned through cleanup,
            // including revocation and replacement of ambient dispatch.
            unsafe {
                (*self.pin.as_ptr()).error_state.handler_stack = previous;
            }
        }
    }
}
impl Drop for HandlerStackScope {
    fn drop(&mut self) {
        self.restore();
    }
}

fn handler_entry(
    factory: &SessionNodeFactory<'_>,
    class: &Sexp<'_>,
    handler: &Sexp<'_>,
    target: &Sexp<'_>,
    result: &Sexp<'_>,
    calling: bool,
) -> Sexp<'static> {
    let entry = checked(factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 5))));
    let mut entry = checked(SexpMut::try_from_checked(entry));
    let parent = checked(factory.wrap(unsafe { crate::sexp::globals::R_GlobalEnv() }));
    for (index, value) in [class, &parent, handler, target, result]
        .into_iter()
        .enumerate()
    {
        checked(entry.try_set_vector_elt(index as i64, value.clone()));
    }
    let entry = owned(entry.freeze());
    unsafe {
        crate::sexp::accessors::SETLEVELS(entry.as_raw(), i32::from(calling));
    }
    entry
}

struct ExitingScope {
    stack: HandlerStackScope,
    _context: ContextGuard,
    target: Sexp<'static>,
}
impl ExitingScope {
    fn install(factory: &SessionNodeFactory<'_>, pin: OwnerPin, classes: &[Sexp<'static>]) -> Self {
        let stack = HandlerStackScope::capture(pin);
        let target = owned(checked(
            factory.allocate(|arena| Some(arena.alloc_node(SEXPTYPE::ENVSXP))),
        ));
        let nil = factory.nil();
        let context = unsafe {
            context::begin_context_guard(
                context::ctxt_flags::CTXT_CCODE,
                nil.as_raw(),
                target.as_raw(),
                nil.as_raw(),
                None,
                nil.as_raw(),
                nil.as_raw(),
            )
        };
        let result =
            owned(checked(factory.allocate(|arena| {
                Some(arena.alloc_vector(SEXPTYPE::VECSXP, 3))
            })));
        let previous = super::handler_stack();
        let mut top = if previous.is_null() {
            owned(factory.nil())
        } else {
            owned(checked(factory.wrap(previous)))
        };
        for class in classes.iter().rev() {
            let entry = handler_entry(factory, class, &nil, &target, &result, false);
            top = owned(checked(factory.pairlist_cell(&entry, &top, &nil)));
        }
        super::set_handler_stack(top.as_raw());
        Self {
            stack,
            _context: context,
            target,
        }
    }
}

/// Evaluate with owning Rust callbacks. The class list is snapshotted before
/// evaluation; only this scope's exact exiting transfer can select its handler.
/// Finalization runs after handlers are removed on every normal or unwind path.
pub fn try_catch_owned(
    body: impl FnOnce() -> Sexp<'static>,
    classes: &Sexp<'_>,
    handler: impl FnOnce(Sexp<'static>) -> Sexp<'static>,
    finally: impl FnOnce(),
) -> Sexp<'static> {
    let (factory, pin) = factory_and_pin();
    checked(factory.link(classes));
    if classes.typeof_() != SEXPTYPE::STRSXP {
        context::r_error("condition classes must be a character vector");
    }
    let names: Vec<_> = (0..classes.len())
        .map(|i| owned(checked(classes.try_string_elt(i))))
        .collect();
    let texts: Vec<_> = names
        .iter()
        .map(|name| checked(name.try_as_string()))
        .collect();
    let catches_error = texts
        .iter()
        .any(|s| matches!(s.as_str(), "error" | "simpleError" | "condition"));
    checked(pin.require_live());
    let scope_pin = checked(unsafe { OwnerToken::from_raw(pin.as_ptr()).pin() })
        .expect("original managed condition owner");
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut scope = ExitingScope::install(&factory, scope_pin, &names);
        let outcome = catch_unwind(AssertUnwindSafe(body));
        scope.stack.restore();
        checked(scope.stack.pin.require_live());
        let condition = match outcome {
            Ok(value) => {
                checked(factory.link(&value));
                return value;
            }
            Err(payload) => {
                if let Some(RSignal::ExitingHandler(ticket)) = payload.downcast_ref::<RSignal>() {
                    let lease = checked(ticket.resolve());
                    let OwnedTransfer::ExitingHandler { target_env, .. } = lease.data() else {
                        context::r_error("incorrect exiting transfer kind")
                    };
                    if target_env == &scope.target {
                        let signal = payload
                            .downcast::<RSignal>()
                            .expect("matched exiting handler");
                        let RSignal::ExitingHandler(ticket) = *signal else {
                            unreachable!()
                        };
                        let lease = checked(ticket.take());
                        checked(lease.require_live());
                        let OwnedTransfer::ExitingHandler { result, .. } = lease.data() else {
                            unreachable!()
                        };
                        owned(checked(result.try_vector_elt(0)))
                    } else {
                        resume_unwind(payload)
                    }
                } else {
                    let message = payload
                        .downcast_ref::<RError>()
                        .map(|e| e.message.as_str())
                        .or_else(|| match payload.downcast_ref::<RSignal>() {
                            Some(RSignal::Error { message }) => Some(message.as_str()),
                            _ => None,
                        });
                    if !catches_error || message.is_none() {
                        resume_unwind(payload);
                    }
                    unsafe {
                        context::own_control_value(super::conditions::make_condition(
                            factory.nil().as_raw(),
                            "simpleError",
                            "",
                            0,
                            message.unwrap(),
                            "error",
                        ))
                    }
                }
            }
        };
        // Remove the catcher before invoking its handler, so a handler error
        // belongs to an outer scope and cannot recursively catch itself.
        drop(scope);
        let result = handler(condition);
        checked(factory.require_active());
        checked(factory.link(&result));
        result
    }));
    let finalized = catch_unwind(AssertUnwindSafe(finally));
    if let Err(payload) = finalized {
        resume_unwind(payload);
    }
    checked(pin.require_live());
    checked(factory.require_active());
    match outcome {
        Ok(value) => value,
        Err(payload) => resume_unwind(payload),
    }
}

pub(crate) unsafe fn catch_native(
    body: Option<NativeBody>,
    bdata: *mut c_void,
    classes: SEXP,
    handler: Option<NativeHandler>,
    hdata: *mut c_void,
    finally: Option<NativeFinally>,
    fdata: *mut c_void,
) -> SEXP {
    let body = body.unwrap_or_else(|| context::r_error("must supply a body function"));
    let (factory, _pin) = factory_and_pin();
    let classes = if classes.is_null() {
        owned(checked(factory.strings(&[])))
    } else {
        unsafe { context::own_control_value(classes) }
    };
    try_catch_owned(
        || unsafe { context::own_control_value(body(bdata)) },
        &classes,
        |condition| {
            if let Some(handler) = handler {
                unsafe { context::own_control_value(handler(condition.as_raw(), hdata)) }
            } else {
                owned(factory.nil())
            }
        },
        || {
            if let Some(finally) = finally {
                unsafe { finally(fdata) }
            }
        },
    )
    .as_raw()
}

struct NativeCallingHandler {
    function: NativeHandler,
    data: *mut c_void,
    owner: WeakOwner,
    active: Rc<Cell<bool>>,
}
struct CallingScope {
    stack: HandlerStackScope,
    active: Rc<Cell<bool>>,
}
impl Drop for CallingScope {
    fn drop(&mut self) {
        self.active.set(false);
        self.stack.restore();
    }
}

/// Dispatch only an authenticated typed callback at its original live scope.
/// Returns false for ordinary R functions, which use the evaluator's dispatch.
pub(super) fn dispatch_calling_handler(handler: SEXP, condition: SEXP) -> bool {
    let (factory, _pin) = factory_and_pin();
    let handler = checked(factory.wrap(handler));
    let Ok(node) = handler.allocation() else {
        return false;
    };
    let Some(callback) = node.heap_identity().resource::<NativeCallingHandler>(node) else {
        return false;
    };
    let pin = checked(callback.owner.pin());
    if !callback.active.get() || crate::sexp::instance::current_instance_ptr() != Some(pin.as_ptr())
    {
        context::r_error("native condition handler is no longer active");
    }
    let condition = unsafe { context::own_control_value(condition) };
    // Native adapters require the data to outlive this dynamic frame. The
    // authenticated resource is inert once its owning scope is dropped.
    unsafe {
        (callback.function)(condition.as_raw(), callback.data);
    }
    checked(pin.require_live());
    true
}

pub(crate) unsafe fn calling_native(
    body: Option<NativeBody>,
    bdata: *mut c_void,
    handler: Option<NativeHandler>,
    hdata: *mut c_void,
) -> SEXP {
    let body = body.unwrap_or_else(|| context::r_error("must supply a body function"));
    let (factory, pin) = factory_and_pin();
    let active = Rc::new(Cell::new(true));
    let mut scope = CallingScope {
        stack: HandlerStackScope::capture(pin),
        active: active.clone(),
    };
    let nil = factory.nil();
    if let Some(handler) = handler {
        let owner = checked(unsafe { OwnerToken::current() })
            .weak_owner()
            .expect("managed native scope");
        let callback = Rc::new(NativeCallingHandler {
            function: handler,
            data: hdata,
            owner,
            active,
        });
        let pointer = owned(checked(factory.allocate(|arena| {
            let pointer = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
            let node = arena.node_token(pointer)?;
            arena
                .heap_identity()
                .attach_resource(&node, callback.clone())?;
            Some(pointer)
        })));
        let class = checked(factory.character("error"));
        let entry = handler_entry(&factory, &class, &pointer, &nil, &nil, true);
        let previous = super::handler_stack();
        let old = if previous.is_null() {
            factory.nil()
        } else {
            checked(factory.wrap(previous))
        };
        let top = checked(factory.pairlist_cell(&entry, &old, &nil));
        super::set_handler_stack(top.as_raw());
    }
    let result = unsafe { context::own_control_value(body(bdata)) };
    scope.stack.restore();
    checked(factory.require_active());
    result.as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{owner::WeakOwner, session::RSession};
    use std::cell::RefCell;

    struct Trace {
        events: RefCell<Vec<&'static str>>,
        owner: WeakOwner,
    }
    impl Trace {
        fn collect(&self, event: &'static str) {
            self.events.borrow_mut().push(event);
            let pin = self.owner.pin().unwrap();
            unsafe { OwnerToken::from_raw(pin.as_ptr()) }
                .full_gc()
                .unwrap();
        }
    }
    unsafe extern "C-unwind" fn body_error(_: *mut c_void) -> SEXP {
        context::r_error("native body error")
    }
    unsafe extern "C-unwind" fn body_rendered_error(_: *mut c_void) -> SEXP {
        unsafe {
            super::super::errorcall(
                crate::sexp::globals::R_NilValue(),
                c"rendered native error".as_ptr(),
            );
        }
        unreachable!()
    }
    unsafe extern "C-unwind" fn handler_collects(condition: SEXP, data: *mut c_void) -> SEXP {
        let condition = unsafe { context::own_control_value(condition) };
        let message = condition
            .try_vector_elt(0)
            .unwrap()
            .try_string_value_elt(0)
            .unwrap()
            .unwrap();
        assert!(matches!(
            message.as_str(),
            "native body error" | "rendered native error"
        ));
        unsafe { &*data.cast::<Trace>() }.collect("handler");
        assert_eq!(
            condition
                .try_vector_elt(0)
                .unwrap()
                .try_string_value_elt(0)
                .unwrap()
                .unwrap(),
            message
        );
        condition.as_raw()
    }
    unsafe extern "C-unwind" fn finally_collects(data: *mut c_void) {
        unsafe { &*data.cast::<Trace>() }.collect("finally");
    }
    fn trace(session: &RSession) -> Trace {
        Trace {
            events: RefCell::new(Vec::new()),
            owner: session.owner_token().unwrap().weak_owner().unwrap(),
        }
    }
    #[test]
    fn owned_native_trycatch_callbacks_unwind_and_keep_conditions_through_final_gc() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let trace = trace(&session);
            let data = (&trace as *const Trace).cast_mut().cast();
            let factory = session.owner_token().unwrap().node_factory();
            let classes = factory.strings(&["error"]).unwrap();
            for body in [body_error as NativeBody, body_rendered_error as NativeBody] {
                trace.events.borrow_mut().clear();
                let raw = unsafe {
                    super::super::R_tryCatch(
                        Some(body),
                        std::ptr::null_mut(),
                        classes.as_raw(),
                        Some(handler_collects),
                        data,
                        Some(finally_collects),
                        data,
                    )
                };
                let result = factory.wrap(raw).unwrap();
                assert_eq!(*trace.events.borrow(), ["handler", "finally"]);
                assert!(
                    result
                        .try_vector_elt(0)
                        .unwrap()
                        .try_string_value_elt(0)
                        .unwrap()
                        .is_some()
                );
            }
            trace.events.borrow_mut().clear();
            let raw = unsafe {
                super::super::R_tryCatchError(
                    Some(body_error),
                    std::ptr::null_mut(),
                    Some(handler_collects),
                    data,
                )
            };
            assert_eq!(*trace.events.borrow(), ["handler"]);
            assert_eq!(
                factory
                    .wrap(raw)
                    .unwrap()
                    .try_vector_elt(0)
                    .unwrap()
                    .try_string_value_elt(0)
                    .unwrap()
                    .as_deref(),
                Some("native body error")
            );
        });
    }
    #[test]
    fn owned_native_trycatch_filters_classes_and_finalizes_unmatched_or_host_panics() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let trace = trace(&session);
            let data = (&trace as *const Trace).cast_mut().cast();
            let factory = session.owner_token().unwrap().node_factory();
            let classes = factory.strings(&["warning"]).unwrap();
            let payload = catch_unwind(AssertUnwindSafe(|| unsafe {
                super::super::R_tryCatch(
                    Some(body_error),
                    std::ptr::null_mut(),
                    classes.as_raw(),
                    Some(handler_collects),
                    data,
                    Some(finally_collects),
                    data,
                )
            }))
            .unwrap_err();
            assert_eq!(
                payload.downcast::<RError>().unwrap().message,
                "native body error"
            );
            assert_eq!(*trace.events.borrow(), ["finally"]);
            assert!(
                super::super::handler_stack().is_null()
                    || super::super::handler_stack() == factory.nil().as_raw()
            );
            let error_classes = factory.strings(&["error"]).unwrap();
            let payload = catch_unwind(AssertUnwindSafe(|| {
                try_catch_owned(
                    || std::panic::panic_any(73usize),
                    &error_classes,
                    |_| panic!("host panic must not select R handler"),
                    || trace.collect("host finally"),
                )
            }))
            .unwrap_err();
            assert_eq!(*payload.downcast::<usize>().unwrap(), 73);
            assert_eq!(*trace.events.borrow(), ["finally", "host finally"]);
        });
    }
    #[test]
    fn owned_native_calling_handler_invokes_before_error_and_restores_stack_on_unwind() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let trace = trace(&session);
            let data = (&trace as *const Trace).cast_mut().cast();
            let previous = super::super::handler_stack();
            let payload = catch_unwind(AssertUnwindSafe(|| unsafe {
                super::super::R_withCallingErrorHandler(
                    Some(body_rendered_error),
                    std::ptr::null_mut(),
                    Some(handler_collects),
                    data,
                )
            }))
            .unwrap_err();
            assert!(
                payload.downcast_ref::<RError>().is_some()
                    || matches!(
                        payload.downcast_ref::<RSignal>(),
                        Some(RSignal::Error { .. })
                    )
            );
            assert_eq!(*trace.events.borrow(), ["handler"]);
            assert_eq!(super::super::handler_stack(), previous);
            session.owner_token().unwrap().full_gc().unwrap();
        });
    }
    #[test]
    fn owned_rust_trycatch_retains_normal_value_through_collecting_finalizer() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let classes = factory.strings(&["error"]).unwrap();
            let value = owned(factory.strings(&["original result"]).unwrap());
            let result = try_catch_owned(
                || value,
                &classes,
                |_| panic!("unexpected handler"),
                || {
                    session
                        .owner_token()
                        .unwrap()
                        .full_gc()
                        .map(|_| ())
                        .unwrap()
                },
            );
            assert_eq!(
                result.try_string_value_elt(0).unwrap().as_deref(),
                Some("original result")
            );
        });
    }
    #[test]
    fn owned_rust_trycatch_finalizes_revoked_runtime_without_publishing_success() {
        let mut session = Some(RSession::new_for_gc_tests());
        let owner = session
            .as_ref()
            .unwrap()
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap();
        let pin = owner.pin().unwrap();
        let finalized = Cell::new(false);
        let payload = catch_unwind(AssertUnwindSafe(|| unsafe {
            crate::sexp::session::with_instance_active(pin.as_ptr(), || {
                let factory = owner.node_factory().unwrap();
                let classes = factory.strings(&["error"]).unwrap();
                let value = owned(factory.strings(&["retained after close"]).unwrap());
                try_catch_owned(
                    || {
                        let mut runtime = session.take().unwrap();
                        runtime.close();
                        drop(runtime);
                        value
                    },
                    &classes,
                    |_| panic!("revoked callback must not select a handler"),
                    || finalized.set(true),
                )
            })
        }))
        .unwrap_err();
        assert!(finalized.get());
        assert!(session.is_none());
        assert!(payload.downcast_ref::<RError>().is_some());
        assert!(owner.pin().is_err());
    }

    unsafe extern "C-unwind" fn capture_calling_handler(data: *mut c_void) -> SEXP {
        let handler = unsafe {
            super::super::conditions::ENTRY_HANDLER(crate::sexp::accessors::CAR(
                super::super::handler_stack(),
            ))
        };
        let handler = unsafe { context::own_control_value(handler) };
        unsafe { &*data.cast::<RefCell<Option<Sexp<'static>>>>() }.replace(Some(handler));
        unsafe { crate::sexp::globals::R_NilValue() }
    }
    #[test]
    fn owned_native_calling_handler_cannot_call_expired_userdata() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let saved: RefCell<Option<Sexp<'static>>> = RefCell::new(None);
            let trace = Box::new(trace(&session));
            unsafe {
                super::super::R_withCallingErrorHandler(
                    Some(capture_calling_handler),
                    (&saved as *const RefCell<Option<Sexp<'static>>>)
                        .cast_mut()
                        .cast(),
                    Some(handler_collects),
                    (&*trace as *const Trace).cast_mut().cast(),
                );
            }
            drop(trace);
            session.owner_token().unwrap().full_gc().unwrap();
            let handler = saved.take().unwrap();
            let payload = catch_unwind(AssertUnwindSafe(|| {
                dispatch_calling_handler(handler.as_raw(), unsafe {
                    crate::sexp::globals::R_NilValue()
                })
            }))
            .unwrap_err();
            assert_eq!(
                payload.downcast::<RError>().unwrap().message,
                "native condition handler is no longer active"
            );
        });
    }
    unsafe extern "C-unwind" fn body_return(data: *mut c_void) -> SEXP {
        let value = unsafe { *data.cast::<SEXP>() };
        let ticket = unsafe { context::return_transfer(std::ptr::null_mut(), value) };
        std::panic::panic_any(RSignal::Return(ticket));
    }
    #[test]
    fn owned_native_trycatch_rethrows_return_ticket_through_collecting_finalizer() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let trace = trace(&session);
            let data = (&trace as *const Trace).cast_mut().cast();
            let factory = session.owner_token().unwrap().node_factory();
            let classes = factory.strings(&["error"]).unwrap();
            let value = factory
                .strings(&["return through native callback"])
                .unwrap();
            let raw = value.as_raw();
            let payload = catch_unwind(AssertUnwindSafe(|| unsafe {
                super::super::R_tryCatch(
                    Some(body_return),
                    (&raw as *const SEXP).cast_mut().cast(),
                    classes.as_raw(),
                    Some(handler_collects),
                    data,
                    Some(finally_collects),
                    data,
                )
            }))
            .unwrap_err();
            assert_eq!(*trace.events.borrow(), ["finally"]);
            let signal = payload.downcast::<RSignal>().unwrap();
            let RSignal::Return(ticket) = *signal else {
                panic!("return must propagate unchanged")
            };
            let lease = ticket.take().unwrap();
            let OwnedTransfer::Return { value, .. } = lease.data() else {
                panic!("return payload kind")
            };
            assert_eq!(
                value.try_string_value_elt(0).unwrap().as_deref(),
                Some("return through native callback")
            );
        });
    }
}
