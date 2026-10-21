use super::{
    RSession,
    diagnostic::Diagnostic,
    encoding::{self, TextEncoding},
    ffi::SEXPTYPE,
    object::SexpMut,
    owner::with_runtime,
};
use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};
const EXPECTED: &[u8] = b"invalid input 'a\xff' in 'utf8towcs'";

#[derive(Clone, Copy)]
enum MessageOperation {
    Case,
    Raw,
    Iconv,
    Path,
}

fn actual_message_publication(action: u8, operation: MessageOperation) {
    let expected: &'static [u8] = match operation {
        MessageOperation::Case => EXPECTED,
        MessageOperation::Path => b"unable to translate '<81>' to UTF-8",
        _ => b"embedded nul in string: 'A\\0B'",
    };
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
    let factory = weak.node_factory().unwrap();
    unsafe {
        super::session::with_instance_active(instance, || {
            let input = match operation {
                MessageOperation::Case | MessageOperation::Path => {
                    let mut input = SexpMut::try_from_checked(
                        factory
                            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 1)))
                            .unwrap(),
                    )
                    .unwrap();
                    let (bytes, encoding) = if matches!(operation, MessageOperation::Path) {
                        (&b"\x81"[..], TextEncoding::Latin1)
                    } else {
                        (&b"a\xff"[..], TextEncoding::Utf8)
                    };
                    let character = factory
                        .allocate(|a| encoding::allocate_character(a, bytes, encoding))
                        .unwrap();
                    input.try_set_string_elt(0, character).unwrap();
                    input.freeze()
                }
                MessageOperation::Raw | MessageOperation::Iconv => {
                    let mut raw = SexpMut::try_from_checked(
                        factory
                            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::RAWSXP, 3)))
                            .unwrap(),
                    )
                    .unwrap();
                    for (i, byte) in b"A\0B".iter().copied().enumerate() {
                        raw.try_set_raw_elt(i as i64, byte).unwrap();
                    }
                    let raw = raw.freeze();
                    if matches!(operation, MessageOperation::Iconv) {
                        let mut list = SexpMut::try_from_checked(
                            factory
                                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, 1)))
                                .unwrap(),
                        )
                        .unwrap();
                        list.try_set_vector_elt(0, raw).unwrap();
                        list.freeze()
                    } else {
                        raw
                    }
                }
            };
            let source = input.allocation().unwrap().clone();
            let args = if matches!(operation, MessageOperation::Iconv) {
                let values = [
                    input.clone(),
                    factory.strings(&["UTF-8"]).unwrap(),
                    factory.strings(&["UTF-8"]).unwrap(),
                    factory.strings(&["byte"]).unwrap(),
                    factory.domain().logical(true),
                    factory.domain().logical(false),
                ];
                let mut args = factory.nil();
                for value in values.into_iter().rev() {
                    args = factory
                        .pairlist_cell(&value, &args, &factory.nil())
                        .unwrap();
                }
                args.into_owned().unwrap()
            } else if matches!(operation, MessageOperation::Path) {
                let mut components = SexpMut::try_from_checked(
                    factory
                        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, 1)))
                        .unwrap(),
                )
                .unwrap();
                components.try_set_vector_elt(0, input.clone()).unwrap();
                let tail = factory
                    .pairlist_cell(
                        &factory.strings(&["/"]).unwrap(),
                        &factory.nil(),
                        &factory.nil(),
                    )
                    .unwrap()
                    .into_owned()
                    .unwrap();
                factory
                    .pairlist_cell(&components.freeze(), &tail, &factory.nil())
                    .unwrap()
                    .into_owned()
                    .unwrap()
            } else {
                factory
                    .pairlist_cell(&input, &factory.nil(), &factory.nil())
                    .unwrap()
                    .into_owned()
                    .unwrap()
            };
            drop(input);
            let saved = args.clone();
            let observed = source.clone();
            let heap = source.heap_identity();
            let ledger = heap.byte_ledger();
            let callback_ledger = ledger.clone();
            let seen = Rc::new(Cell::new(0));
            let called = seen.clone();
            let revoker = Rc::downgrade(&facade);
            super::gengc::register_gc_callback(Box::new(move |_| {
                if called.get() != 0 {
                    return;
                }
                let mut published = false;
                for (_, node) in super::memory::fresh_allocation_roots_for_marking(&heap) {
                    let header = heap.node_snapshot(&node).unwrap();
                    if header.sxpinfo.type_of() == SEXPTYPE::CHARSXP {
                        let payload = heap.payload_lease(&node).unwrap();
                        if payload.byte_elt(0) == Some(expected[0])
                            && payload.byte_elt(1) == Some(expected[1])
                            && payload.byte_elt(2) == Some(expected[2])
                        {
                            assert_eq!(
                                TextEncoding::from_gp(header.sxpinfo.gp()),
                                TextEncoding::Native
                            );
                            published = true;
                        }
                    }
                }
                if !published {
                    return;
                }
                called.set(1);
                (*instance).memory_state.gc_force_gap = 0;
                assert!(callback_ledger.workspace_bytes_for_test() >= expected.len());
                super::accessors::SETCAR(saved.as_raw(), super::globals::R_NilValue());
                super::accessors::SETCDR(saved.as_raw(), super::globals::R_NilValue());
                super::gengc::full_gc();
                assert!(
                    observed.is_live(),
                    "selected original input survives message constructor GC"
                );
                match action {
                    1 => std::panic::panic_any(991u32),
                    2 => drop(revoker.upgrade().unwrap().borrow_mut().take()),
                    _ => {}
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = catch_unwind(AssertUnwindSafe(|| match operation {
                MessageOperation::Case => {
                    crate::mainutils::character::case_entry(args.as_raw(), true)
                }
                MessageOperation::Raw => crate::mainutils::essentials::do_rawToChar(
                    factory.nil().as_raw(),
                    factory.nil().as_raw(),
                    args.as_raw(),
                    factory.nil().as_raw(),
                ),
                MessageOperation::Path => crate::mainutils::paste::do_filepath(
                    factory.nil().as_raw(),
                    factory.nil().as_raw(),
                    args.as_raw(),
                    factory.nil().as_raw(),
                ),
                MessageOperation::Iconv => crate::mainutils::essentials::do_iconv(
                    factory.nil().as_raw(),
                    factory.nil().as_raw(),
                    args.as_raw(),
                    factory.nil().as_raw(),
                ),
            }));
            assert_eq!(seen.get(), 1);
            assert_eq!((*instance).memory_state.in_gc, 0);
            assert_eq!(ledger.workspace_bytes_for_test(), 0);
            match action {
                0 => {
                    assert!(
                        result
                            .unwrap_err()
                            .downcast_ref::<super::context::RError>()
                            .is_some()
                    );
                    let condition = (*instance)
                        .error_state
                        .signalled_condition
                        .owned()
                        .expect("original condition survives default rendering");
                    let text = condition
                        .try_vector_elt(0)
                        .unwrap()
                        .try_string_elt(0)
                        .unwrap()
                        .into_owned()
                        .unwrap();
                    assert!(text.try_char_eq(expected).unwrap());
                    assert_eq!(
                        TextEncoding::from_gp(text.header().sxpinfo.gp()),
                        TextEncoding::Native
                    );
                    super::gengc::full_gc();
                    assert!(
                        !source.is_live(),
                        "detached input releases after diagnostic publication"
                    );
                    assert!(text.try_char_eq(expected).unwrap());
                }
                1 => assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 991),
                2 => {
                    assert!(!weak.is_live());
                    assert!(
                        result
                            .unwrap_err()
                            .downcast_ref::<super::context::RError>()
                            .is_some()
                    );
                }
                _ => unreachable!(),
            }
        });
    }
}
#[test]
fn owned_diagnostic_actual_message_constructor_keeps_source_tag_and_ticket() {
    actual_message_publication(0, MessageOperation::Case);
}
#[test]
fn owned_diagnostic_actual_message_callback_panic_releases_ticket() {
    actual_message_publication(1, MessageOperation::Case);
}
#[test]
fn owned_diagnostic_actual_message_callback_revocation_cannot_publish() {
    actual_message_publication(2, MessageOperation::Case);
}
#[test]
fn owned_diagnostic_foreign_raise_cannot_signal_or_allocate_in_current_runtime() {
    let original = RSession::new_for_gc_tests();
    let diagnostic = original.with_active(|| {
        let owner = original.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        with_runtime(&owner.weak_owner().unwrap(), |access| {
            Diagnostic::new(access, &parent, EXPECTED.len(), 999, true, |i| EXPECTED[i])
        })
        .unwrap()
        .unwrap()
    });
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        let error = catch_unwind(AssertUnwindSafe(|| {
            crate::mainutils::errors::raise_owned_diagnostic(&owner, diagnostic)
        }))
        .unwrap_err();
        let error = error.downcast_ref::<super::context::RError>().unwrap();
        assert!(error.message.contains("another heap domain"));
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}

#[test]
fn owned_nul_diagnostic_raw_constructor_keeps_source_tag_and_ticket() {
    actual_message_publication(0, MessageOperation::Raw);
}

#[test]
fn owned_nul_diagnostic_raw_callback_panic_releases_ticket() {
    actual_message_publication(1, MessageOperation::Raw);
}

#[test]
fn owned_nul_diagnostic_raw_callback_revocation_cannot_publish() {
    actual_message_publication(2, MessageOperation::Raw);
}

#[test]
fn owned_nul_diagnostic_iconv_constructor_keeps_source_tag_and_ticket() {
    actual_message_publication(0, MessageOperation::Iconv);
}

#[test]
fn owned_nul_diagnostic_iconv_callback_panic_releases_ticket() {
    actual_message_publication(1, MessageOperation::Iconv);
}

#[test]
fn owned_nul_diagnostic_iconv_callback_revocation_cannot_publish() {
    actual_message_publication(2, MessageOperation::Iconv);
}

#[test]
fn owned_path_diagnostic_actual_constructor_keeps_source_tag_and_ticket() {
    actual_message_publication(0, MessageOperation::Path);
}
#[test]
fn owned_path_diagnostic_actual_callback_panic_releases_ticket() {
    actual_message_publication(1, MessageOperation::Path);
}
#[test]
fn owned_path_diagnostic_actual_callback_revocation_cannot_publish() {
    actual_message_publication(2, MessageOperation::Path);
}
