//! Exercise actual error-buffer entrypoints under constructor GC and revocation.
use super::*;
use crate::sexp::{encoding::TextEncoding, owner::with_runtime, session::RSession};
use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};
const ORIGINAL: &[u8] = b"original\xfftail";

#[test]
fn owned_error_buffer_utf8_tail_and_fixed_limit_match_rstrncpy() {
    for (source, expected) in [
        (b"a\xff".as_slice(), b"a".as_slice()),
        (b"a\xffb".as_slice(), b"a\xffb".as_slice()),
        (b"a\xc3".as_slice(), b"a".as_slice()),
        (b"a\xc3\xa9".as_slice(), b"a\xc3\xa9".as_slice()),
        (b"a\x80\x80".as_slice(), b"a".as_slice()),
    ] {
        let snapshot = byte_buffer::Snapshot::copy(|i| source.get(i).copied(), true);
        let array = snapshot.into_array();
        assert_eq!(&array[..expected.len()], expected);
        assert!(array[expected.len()..].iter().all(|&b| b == 0));
        let array = byte_buffer::Snapshot::copy(|i| source.get(i).copied(), false).into_array();
        assert_eq!(&array[..source.len()], source);
    }
    let source = [b'a'; BUFSIZE + 7];
    let array = byte_buffer::Snapshot::copy(|i| source.get(i).copied(), true).into_array();
    assert_eq!(&array[..BUFSIZE - 1], &source[..BUFSIZE - 1]);
    assert_eq!(&array[BUFSIZE - 1..], &[0, 0]);
}

#[test]
fn owned_error_buffer_c_setter_can_read_its_own_buffer_without_aliasing() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        R_SetErrmessage_c(c"original\xfftail".as_ptr());
        let original = R_curErrorBuf();
        R_SetErrmessage_c(original);
        assert_eq!(
            &state::error_buffer_snapshot().into_array()[..ORIGINAL.len()],
            ORIGINAL
        );
        R_SetErrmessage_c(R_curErrorBuf().add(9));
        assert_eq!(&state::error_buffer_snapshot().into_array()[..4], b"tail");
    });
}

fn getter_constructor(action: u8) {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let instance = pin.as_ptr();
    unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            R_SetErrmessage_c(c"original\xfftail".as_ptr());
            let offset = crate::mainutils::names::R_FunTab
                .iter()
                .position(|entry| entry.name == b"geterrmessage\0")
                .unwrap();
            let op = crate::mainutils::dstruct::mkPRIMSXP(offset as i32, 1);
            let factory = weak.node_factory().unwrap();
            let nil = factory.nil();
            let op = factory.wrap(op).unwrap().into_owned().unwrap();
            let heap = op.allocation().unwrap().heap_identity();
            let callback_heap = heap.clone();
            let seen = Rc::new(Cell::new(0));
            let called = seen.clone();
            let revoker = Rc::downgrade(&facade);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if called.get() != 0 {
                    return;
                }
                for (_, node) in
                    crate::sexp::memory::fresh_allocation_roots_for_marking(&callback_heap)
                {
                    let header = callback_heap.node_snapshot(&node).unwrap();
                    if header.sxpinfo.type_of() != SEXPTYPE::CHARSXP {
                        continue;
                    }
                    let payload = callback_heap.payload_lease(&node).unwrap();
                    if payload.byte_elt(0) != Some(b'o') || payload.byte_elt(1) != Some(b'r') {
                        continue;
                    }
                    assert_eq!(
                        TextEncoding::from_gp(header.sxpinfo.gp()),
                        TextEncoding::Native
                    );
                    called.set(1);
                    (*instance).memory_state.gc_force_gap = 0;
                    R_SetErrmessage("mutated during message constructor");
                    crate::sexp::gengc::full_gc();
                    match action {
                        1 => std::panic::panic_any(991u32),
                        2 => drop(revoker.upgrade().unwrap().borrow_mut().take()),
                        _ => {}
                    }
                    break;
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = catch_unwind(AssertUnwindSafe(|| {
                native::error_buffer_entry(op.as_raw(), nil.as_raw(), false)
            }));
            assert_eq!(seen.get(), 1);
            assert_eq!((*instance).memory_state.in_gc, 0);
            assert_eq!(heap.byte_ledger().workspace_bytes_for_test(), 0);
            match action {
                0 => {
                    let value = factory.wrap(result.unwrap()).unwrap().into_owned().unwrap();
                    assert!(
                        value
                            .try_string_elt(0)
                            .unwrap()
                            .try_char_eq(ORIGINAL)
                            .unwrap()
                    );
                    crate::sexp::gengc::full_gc();
                    assert!(
                        value
                            .try_string_elt(0)
                            .unwrap()
                            .try_char_eq(ORIGINAL)
                            .unwrap()
                    );
                    assert_eq!(R_GetErrorBuf(), "mutated during message constructor");
                }
                1 => assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 991),
                2 => {
                    assert!(!weak.is_live());
                    assert!(
                        result
                            .unwrap_err()
                            .downcast_ref::<crate::sexp::context::RError>()
                            .is_some()
                    );
                }
                _ => unreachable!(),
            }
        });
    }
}
#[test]
fn owned_error_buffer_getter_snapshot_precedes_constructor_gc() {
    getter_constructor(0);
}
#[test]
fn owned_error_buffer_getter_constructor_panic_unwinds() {
    getter_constructor(1);
}
#[test]
fn owned_error_buffer_getter_constructor_revocation_rejects_publication() {
    getter_constructor(2);
}

#[test]
fn owned_error_buffer_setter_rejects_foreign_arguments_before_allocation_or_mutation() {
    let original = RSession::new_for_gc_tests();
    let args = original.with_active(|| {
        let factory = original.owner_token().unwrap().node_factory();
        factory
            .pairlist_cell(
                &factory.strings(&["foreign"]).unwrap(),
                &factory.nil(),
                &factory.nil(),
            )
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        R_SetErrmessage("original state");
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        let error = with_runtime(&owner.weak_owner().unwrap(), |access| {
            byte_buffer::argument(access, &args)
        })
        .unwrap()
        .err()
        .unwrap();
        assert_eq!(
            error,
            crate::sexp::object::SexpError::UnownedPointer {
                address: args.as_raw().addr()
            }
        );
        assert_eq!(R_GetErrorBuf(), "original state");
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}

struct HandlerBytes {
    args: crate::sexp::object::Sexp<'static>,
    calls: Cell<usize>,
}
const BARE: &[u8] = b"invalid input 'a\xff' in 'utf8towcs'";
unsafe extern "C-unwind" fn raise_byte_diagnostic(data: *mut c_void) -> SEXP {
    let data = unsafe { &*data.cast::<HandlerBytes>() };
    unsafe { crate::mainutils::character::case_entry(data.args.as_raw(), true) }
}
unsafe extern "C-unwind" fn observe_calling_bytes(condition: SEXP, data: *mut c_void) -> SEXP {
    let data = unsafe { &*data.cast::<HandlerBytes>() };
    assert_eq!(
        &state::error_buffer_snapshot().into_array()[..BARE.len()],
        BARE
    );
    let condition = unsafe { crate::sexp::context::own_control_value(condition) };
    unsafe { crate::sexp::gengc::full_gc() };
    assert!(
        condition
            .try_vector_elt(0)
            .unwrap()
            .try_string_elt(0)
            .unwrap()
            .try_char_eq(BARE)
            .unwrap()
    );
    data.calls.set(data.calls.get() + 1);
    R_SetErrmessage("mutated by calling handler");
    condition.as_raw()
}
unsafe extern "C-unwind" fn inner_calling_bytes(data: *mut c_void) -> SEXP {
    unsafe {
        R_withCallingErrorHandler(
            Some(raise_byte_diagnostic),
            data,
            Some(observe_calling_bytes),
            data,
        )
    }
}
unsafe extern "C-unwind" fn outer_calling_bytes(data: *mut c_void) -> SEXP {
    unsafe {
        R_withCallingErrorHandler(
            Some(inner_calling_bytes),
            data,
            Some(observe_calling_bytes),
            data,
        )
    }
}
#[test]
fn owned_error_buffer_simple_error_resets_raw_bytes_before_each_collecting_handler() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let mut input = crate::sexp::object::SexpMut::try_from_checked(
            factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 1)))
                .unwrap(),
        )
        .unwrap();
        let character = factory
            .allocate(|a| {
                crate::sexp::encoding::allocate_character(a, b"a\xff", TextEncoding::Utf8)
            })
            .unwrap();
        input.try_set_string_elt(0, character).unwrap();
        let input = input.freeze();
        let mut data = HandlerBytes {
            args: factory
                .pairlist_cell(&input, &factory.nil(), &factory.nil())
                .unwrap()
                .into_owned()
                .unwrap(),
            calls: Cell::new(0),
        };
        let classes = factory.strings(&["error"]).unwrap();
        let data_ptr = (&mut data as *mut HandlerBytes).cast();
        let result = native::try_catch_owned(
            || crate::sexp::context::own_control_value(outer_calling_bytes(data_ptr)),
            &classes,
            |condition| {
                assert_eq!(
                    &state::error_buffer_snapshot().into_array()[..BARE.len()],
                    BARE
                );
                crate::sexp::gengc::full_gc();
                assert!(
                    condition
                        .try_vector_elt(0)
                        .unwrap()
                        .try_string_elt(0)
                        .unwrap()
                        .try_char_eq(BARE)
                        .unwrap()
                );
                condition
            },
            || {},
        );
        assert_eq!(data.calls.get(), 2);
        assert!(
            result
                .try_vector_elt(0)
                .unwrap()
                .try_string_elt(0)
                .unwrap()
                .try_char_eq(BARE)
                .unwrap()
        );
    });
}

#[test]
fn owned_error_buffer_stack_snapshot_needs_no_workspace_but_getter_obeys_original_cap() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let _anchor = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        R_SetErrmessage("original state");
        owner
            .with_arena(|a| a.set_budget(crate::sexp::memory::ArenaBudget::new(1, 0)))
            .unwrap();
        let snapshot = state::error_buffer_snapshot();
        let result = with_runtime(&owner.weak_owner().unwrap(), |access| {
            snapshot.value(access)
        })
        .unwrap();
        assert!(matches!(
            result,
            Err(crate::sexp::object::SexpError::AllocationFailed { .. })
        ));
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), 1);
        assert_eq!(R_GetErrorBuf(), "original state");
        R_SetErrmessage("replacement fits the fixed buffer");
        assert_eq!(R_GetErrorBuf(), "replacement fits the fixed buffer");
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), 1);
    });
}

#[test]
fn owned_error_buffer_matches_gnu_oracle_for_every_byte_after_valid_utf8_prefixes() {
    fn decode(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
    let rows = include_str!("byte_buffer_tail_utf8.out")
        .lines()
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 768);
    for row in rows {
        let (source, expected) = row.split_once('|').unwrap();
        let source = decode(source);
        let expected = decode(expected);
        let array = byte_buffer::Snapshot::copy(|i| source.get(i).copied(), true).into_array();
        let len = array.iter().position(|&b| b == 0).unwrap();
        assert_eq!(&array[..len], expected, "GNU corpus row {row}");
    }
}
