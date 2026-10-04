//! Registered arity must be checked without invoking an incompatible ABI.
use super::*;
use crate::mainutils::rdynload::{R_ExternalMethodDef, R_getEmbeddingDllInfo, R_registerRoutines};
use crate::sexp::{RSession, object::Sexp};
use std::cell::Cell;

thread_local! {
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

// .External always invokes this one-list ABI, even when the user payload
// length disagrees with its declaration. The RED fixture never calls through
// a mismatched Rust function type.
unsafe extern "C-unwind" fn external_counter(_: SEXP) -> SEXP {
    CALLS.with(|calls| calls.set(calls.get() + 1));
    unsafe { R_NilValue() }
}

unsafe extern "C" fn buffer_counter(_: *mut c_void) {
    CALLS.with(|calls| calls.set(calls.get() + 1));
}

#[test]
fn registered_buffer_type_admission_precedes_foreign_callback() {
    use crate::mainutils::rdynload::R_CMethodDef;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        CALLS.with(|calls| calls.set(0));
        crate::mainutils::rdynload::set_native_extensions_enabled(true);
        let dll = R_getEmbeddingDllInfo();
        let factory = session.owner_token().unwrap().node_factory();
        let nil = factory.nil().into_owned().unwrap();
        let name = factory
            .strings(&["rport_buffer_counter"])
            .unwrap()
            .into_owned()
            .unwrap();
        let value = factory
            .strings(&["owned input"])
            .unwrap()
            .into_owned()
            .unwrap();
        let tail = factory
            .pairlist_cell(&value, &nil, &nil)
            .unwrap()
            .into_owned()
            .unwrap();
        let args = factory
            .pairlist_cell(&name, &tail, &nil)
            .unwrap()
            .into_owned()
            .unwrap();
        let op = factory
            .wrap(crate::eval::primitive::make_primitive_binding(
                ".C",
                SEXPTYPE::BUILTINSXP,
            ))
            .unwrap()
            .into_owned()
            .unwrap();
        for (kind, accepted) in [(SEXPTYPE::REALSXP.0, false), (SEXPTYPE::ANYSXP.0, true)] {
            let types = [kind];
            let table = [
                R_CMethodDef {
                    name: c"rport_buffer_counter".as_ptr(),
                    fun: std::mem::transmute::<unsafe extern "C" fn(*mut c_void), DL_FUNC>(
                        buffer_counter,
                    ),
                    num_args: 1,
                    types: types.as_ptr(),
                },
                R_CMethodDef {
                    name: ptr::null(),
                    fun: None,
                    num_args: 0,
                    types: ptr::null(),
                },
            ];
            R_registerRoutines(dll, table.as_ptr(), ptr::null(), ptr::null(), ptr::null());
            let result = catch_unwind(AssertUnwindSafe(|| {
                do_foreign_dotcode(nil.as_raw(), op.as_raw(), args.as_raw(), nil.as_raw())
            }));
            if accepted {
                let output = factory.wrap(result.unwrap()).unwrap().into_owned().unwrap();
                assert_eq!(
                    output
                        .try_vector_elt(0)
                        .unwrap()
                        .try_string_elt(0)
                        .unwrap()
                        .try_as_string()
                        .unwrap(),
                    "owned input"
                );
                CALLS.with(|calls| assert_eq!(calls.get(), 1));
            } else {
                let error = result
                    .unwrap_err()
                    .downcast::<crate::sexp::context::RError>()
                    .unwrap();
                assert!(error.message.contains("wrong type for argument 1"));
                CALLS.with(|calls| assert_eq!(calls.get(), 0));
            }
        }
    });
}

unsafe fn invoke_external(
    count: c_int,
    provided: usize,
) -> crate::sexp::object::SexpResult<Sexp<'static>> {
    let owner = unsafe { crate::sexp::owner::OwnerToken::current()? };
    let factory = owner.node_factory();
    let nil = factory.nil().into_owned()?;
    let name = factory.strings(&["rport_foreign_counter"])?.into_owned()?;
    let routines = [
        R_ExternalMethodDef {
            name: c"rport_foreign_counter".as_ptr(),
            fun: unsafe {
                std::mem::transmute::<unsafe extern "C-unwind" fn(SEXP) -> SEXP, DL_FUNC>(
                    external_counter,
                )
            },
            num_args: count,
        },
        R_ExternalMethodDef {
            name: ptr::null(),
            fun: None,
            num_args: 0,
        },
    ];
    unsafe {
        R_registerRoutines(
            R_getEmbeddingDllInfo(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            routines.as_ptr(),
        );
    }
    let mut tail = nil.clone();
    for _ in 0..provided {
        tail = factory.pairlist_cell(&nil, &tail, &nil)?.into_owned()?;
    }
    let args = factory.pairlist_cell(&name, &tail, &nil)?.into_owned()?;
    unsafe {
        invoke_native_handler(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
            crate::mainutils::native_routines::NativeInterface::External,
        )
    }
}

#[test]
fn registered_external_correct_and_variadic_requests_execute_once() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        CALLS.with(|calls| calls.set(0));
        crate::mainutils::rdynload::set_native_extensions_enabled(true);
        assert!(invoke_external(0, 0).unwrap().is_nil());
        CALLS.with(|calls| assert_eq!(calls.get(), 1));
        assert!(invoke_external(-1, 3).unwrap().is_nil());
        CALLS.with(|calls| assert_eq!(calls.get(), 2));
    });
}

#[test]
fn external_registration_cannot_resolve_as_call() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        CALLS.with(|calls| calls.set(0));
        crate::mainutils::rdynload::set_native_extensions_enabled(true);
        let routines = [
            R_ExternalMethodDef {
                name: c"rport_foreign_counter".as_ptr(),
                fun: std::mem::transmute::<unsafe extern "C-unwind" fn(SEXP) -> SEXP, DL_FUNC>(
                    external_counter,
                ),
                num_args: 0,
            },
            R_ExternalMethodDef {
                name: ptr::null(),
                fun: None,
                num_args: 0,
            },
        ];
        R_registerRoutines(
            R_getEmbeddingDllInfo(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            routines.as_ptr(),
        );
        let f = session.owner_token().unwrap().node_factory();
        let nil = f.nil().into_owned().unwrap();
        let name = f
            .strings(&["rport_foreign_counter"])
            .unwrap()
            .into_owned()
            .unwrap();
        let args = f
            .pairlist_cell(&name, &nil, &nil)
            .unwrap()
            .into_owned()
            .unwrap();
        let result = invoke_native_handler(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
            crate::mainutils::native_routines::NativeInterface::Call,
        );
        assert!(result.is_err());
        CALLS.with(|calls| assert_eq!(calls.get(), 0));
    });
}

#[test]
fn registered_external_wrong_arity_is_rejected_before_callback() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        CALLS.with(|calls| calls.set(0));
        crate::mainutils::rdynload::set_native_extensions_enabled(true);
        let dll = R_getEmbeddingDllInfo();
        let routines = [
            R_ExternalMethodDef {
                name: c"rport_foreign_counter".as_ptr(),
                fun: std::mem::transmute::<unsafe extern "C-unwind" fn(SEXP) -> SEXP, DL_FUNC>(
                    external_counter,
                ),
                num_args: 0,
            },
            R_ExternalMethodDef {
                name: ptr::null(),
                fun: None,
                num_args: 0,
            },
        ];
        R_registerRoutines(
            dll,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            routines.as_ptr(),
        );
        let factory = session.owner_token().unwrap().node_factory();
        let nil = factory.nil().into_owned().unwrap();
        let name = factory
            .strings(&["rport_foreign_counter"])
            .unwrap()
            .into_owned()
            .unwrap();
        let payload = nil.clone();
        let tail = factory
            .pairlist_cell(&payload, &nil, &nil)
            .unwrap()
            .into_owned()
            .unwrap();
        let args: Sexp<'static> = factory
            .pairlist_cell(&name, &tail, &nil)
            .unwrap()
            .into_owned()
            .unwrap();
        let result = invoke_native_handler(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
            crate::mainutils::native_routines::NativeInterface::External,
        );
        assert!(
            result.is_err(),
            "registered arity must reject a one-value request for a zero-value routine"
        );
        CALLS.with(|calls| assert_eq!(calls.get(), 0));
    });
}
