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

fn actual_message_publication(action: u8) {
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
            let mut input = SexpMut::try_from_checked(
                factory
                    .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 1)))
                    .unwrap(),
            )
            .unwrap();
            let character = factory
                .allocate(|a| encoding::allocate_character(a, b"a\xff", TextEncoding::Utf8))
                .unwrap();
            input.try_set_string_elt(0, character).unwrap();
            let input = input.freeze();
            let source = input.allocation().unwrap().clone();
            let args = factory
                .pairlist_cell(&input, &factory.nil(), &factory.nil())
                .unwrap()
                .into_owned()
                .unwrap();
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
                        if payload.byte_elt(0) == Some(b'i')
                            && payload.byte_elt(1) == Some(b'n')
                            && payload.byte_elt(2) == Some(b'v')
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
                assert!(callback_ledger.workspace_bytes_for_test() >= EXPECTED.len());
                super::accessors::SETCAR(saved.as_raw(), super::globals::R_NilValue());
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
            let result = catch_unwind(AssertUnwindSafe(|| {
                crate::mainutils::character::case_entry(args.as_raw(), true)
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
                    assert!(text.try_char_eq(EXPECTED).unwrap());
                    assert_eq!(
                        TextEncoding::from_gp(text.header().sxpinfo.gp()),
                        TextEncoding::Native
                    );
                    super::gengc::full_gc();
                    assert!(
                        !source.is_live(),
                        "detached input releases after diagnostic publication"
                    );
                    assert!(text.try_char_eq(EXPECTED).unwrap());
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
    actual_message_publication(0);
}
#[test]
fn owned_diagnostic_actual_message_callback_panic_releases_ticket() {
    actual_message_publication(1);
}
#[test]
fn owned_diagnostic_actual_message_callback_revocation_cannot_publish() {
    actual_message_publication(2);
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
