//! Constructor callbacks must retain the selected original raw input.
use crate::sexp::{RSession, ffi::SEXPTYPE, object::SexpMut};
type Expected<'a> = Result<&'a [&'a [u8]], &'a str>;
type Case<'a> = (&'a str, &'a [u8], Option<i32>, Expected<'a>);

#[test]
fn raw_to_char_native_semantic_matrix() {
    use crate::sexp::{encoding::TextEncoding, ffi::NA_LOGICAL};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let mut failures = Vec::new();
        let cases: &[Case<'_>] = &[
            (
                "multiple bytes",
                &[65, 0, 233],
                Some(1),
                Ok(&[b"A", b"", &[233]]),
            ),
            ("multiple empty", &[], Some(1), Ok(&[])),
            ("native UTF8 tag", &[195, 169], Some(0), Ok(&[&[195, 169]])),
            (
                "interior NUL",
                &[65, 0, 66],
                Some(0),
                Err("embedded nul in string"),
            ),
            (
                "missing flag",
                &[65],
                Some(NA_LOGICAL),
                Err("argument 'multiple' must be TRUE or FALSE"),
            ),
            (
                "empty flag",
                &[65],
                None,
                Err("argument 'multiple' must be TRUE or FALSE"),
            ),
            ("trailing NUL control", &[65, 0, 0], Some(0), Ok(&[b"A"])),
            ("opaque native control", &[255], Some(0), Ok(&[&[255]])),
        ];
        for &(name, bytes, multiple, expected) in cases {
            let input = f
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::RAWSXP, bytes.len() as i64)))
                .unwrap();
            let mut input = SexpMut::try_from_checked(input).unwrap();
            for (i, &byte) in bytes.iter().enumerate() {
                input.try_set_raw_elt(i as i64, byte).unwrap();
            }
            let input = input.freeze();
            let flag = f
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::LGLSXP, i64::from(multiple.is_some()))))
                .unwrap();
            let mut flag = SexpMut::try_from_checked(flag).unwrap();
            if let Some(value) = multiple {
                flag.try_set_logical_elt(0, value).unwrap();
            }
            let rest = f.pairlist_cell(&flag.freeze(), &f.nil(), &f.nil()).unwrap();
            let args = f.pairlist_cell(&input, &rest, &f.nil()).unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let raw = unsafe {
                    super::do_rawToChar(
                        f.nil().as_raw(),
                        f.nil().as_raw(),
                        args.as_raw(),
                        f.nil().as_raw(),
                    )
                };
                f.wrap(raw).unwrap().into_owned().unwrap()
            }));
            let matches = match (result, expected) {
                (Ok(value), Ok(expected)) => {
                    value.len() == expected.len() as i64
                        && expected.iter().enumerate().all(|(i, expected)| {
                            let character = value.try_string_elt(i as i64).unwrap();
                            character.try_char_eq(expected).unwrap()
                                && TextEncoding::from_gp(character.header().sxpinfo.gp())
                                    == TextEncoding::Native
                        })
                }
                (Err(payload), Err(message)) => payload
                    .downcast_ref::<crate::sexp::context::RError>()
                    .is_some_and(|e| e.message.starts_with(message)),
                _ => false,
            };
            if !matches {
                failures.push(name);
            }
        }
        assert!(failures.is_empty(), "rawToChar cases failed: {failures:?}");
    });
}

#[test]
fn raw_to_char_constructor_keeps_selected_input_after_argument_detachment() {
    use std::{cell::Cell, rc::Rc};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let pin = owner.pin().unwrap().unwrap();
        let instance = pin.as_ptr();
        let factory = owner.node_factory();
        let input = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, 5)))
            .unwrap();
        let mut input = SexpMut::try_from_checked(input).unwrap();
        for (i, byte) in [b'A', b'B', b'C', 0, 0].into_iter().enumerate() {
            input.try_set_raw_elt(i as i64, byte).unwrap();
        }
        let input = input.freeze().into_owned().unwrap();
        let source = input.allocation().unwrap().clone();
        let flag = factory.domain().logical(false);
        let tail = factory
            .pairlist_cell(&flag, &factory.nil(), &factory.nil())
            .unwrap();
        let arguments = factory
            .pairlist_cell(&input, &tail, &factory.nil())
            .unwrap()
            .into_owned()
            .unwrap();
        drop(input);
        let seen = Rc::new(Cell::new(0));
        let callback_seen = seen.clone();
        let saved = arguments.clone();
        let observed = source.clone();
        let ledger = source.heap_identity().byte_ledger();
        // Retain the exact instance, ending every arena loan before notifications.
        unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if callback_seen.replace(1) != 0 {
                    return;
                }
                (*instance).memory_state.gc_force_gap = 0;
                assert!(ledger.workspace_bytes_for_test() >= 3);
                crate::sexp::accessors::SETCAR(saved.as_raw(), crate::sexp::globals::R_NilValue());
                crate::sexp::gengc::full_gc();
                assert!(
                    observed.is_live(),
                    "rawToChar must own its selected input across constructor notification"
                );
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let raw = super::do_rawToChar(
                factory.nil().as_raw(),
                factory.nil().as_raw(),
                arguments.as_raw(),
                factory.nil().as_raw(),
            );
            let output = factory.wrap(raw).unwrap().into_owned().unwrap();
            assert_eq!(seen.get(), 1);
            assert!(
                output
                    .try_string_elt(0)
                    .unwrap()
                    .try_char_eq(b"ABC")
                    .unwrap()
            );
            crate::sexp::gengc::full_gc();
            assert!(
                !source.is_live(),
                "detached raw input releases after publication"
            );
        }
    });
}

fn callback_case(action: u8, multiple: bool, publication: bool) {
    use crate::sexp::encoding::TextEncoding;
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
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
    let f = weak.node_factory().unwrap();
    // The exact pin retains physical cleanup storage after facade revocation.
    unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let data: &[u8] = if publication {
                &[195, 169]
            } else if multiple {
                &[65, 0, 233]
            } else {
                b"ABC\0\0"
            };
            let input = f
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::RAWSXP, data.len() as i64)))
                .unwrap();
            let mut input = SexpMut::try_from_checked(input).unwrap();
            for (i, &byte) in data.iter().enumerate() {
                input.try_set_raw_elt(i as i64, byte).unwrap();
            }
            let input = input.freeze();
            let source = input.allocation().unwrap().clone();
            let flag = f.domain().logical(multiple);
            let rest = f.pairlist_cell(&flag, &f.nil(), &f.nil()).unwrap();
            let args = f
                .pairlist_cell(&input, &rest, &f.nil())
                .unwrap()
                .into_owned()
                .unwrap();
            drop(input);
            let count = Rc::new(Cell::new(0));
            let seen = count.clone();
            let saved = args.clone();
            let observed = source.clone();
            let heap = source.heap_identity();
            let ledger = heap.byte_ledger();
            let callback_ledger = ledger.clone();
            let revoker = Rc::downgrade(&facade);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if seen.get() != 0 {
                    return;
                }
                if publication {
                    let mut character_seen = false;
                    for (_, node) in crate::sexp::memory::fresh_allocation_roots_for_marking(&heap)
                    {
                        let header = heap.node_snapshot(&node).unwrap();
                        if header.sxpinfo.type_of() == SEXPTYPE::CHARSXP {
                            character_seen = true;
                            assert_eq!(
                                TextEncoding::from_gp(header.sxpinfo.gp()),
                                TextEncoding::Native,
                                "Native tag must publish before constructor notification"
                            );
                        }
                    }
                    if !character_seen {
                        return;
                    }
                }
                seen.set(1);
                (*instance).memory_state.gc_force_gap = 0;
                if multiple {
                    assert_eq!(callback_ledger.workspace_bytes_for_test(), 0);
                } else {
                    assert!(
                        callback_ledger.workspace_bytes_for_test()
                            >= if publication { 2 } else { 3 }
                    );
                }
                crate::sexp::accessors::SETCAR(saved.as_raw(), crate::sexp::globals::R_NilValue());
                crate::sexp::gengc::full_gc();
                assert!(
                    observed.is_live(),
                    "selected original source survives detachment and collection"
                );
                match action {
                    1 => std::panic::panic_any(991_u32),
                    2 => drop(revoker.upgrade().unwrap().borrow_mut().take()),
                    _ => {}
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let raw = super::do_rawToChar(
                    f.nil().as_raw(),
                    f.nil().as_raw(),
                    args.as_raw(),
                    f.nil().as_raw(),
                );
                f.wrap(raw).unwrap().into_owned().unwrap()
            }));
            assert_eq!(
                count.get(),
                1,
                "the requested real constructor callback must run"
            );
            assert_eq!((*instance).memory_state.in_gc, 0);
            assert_eq!(ledger.workspace_bytes_for_test(), 0);
            match action {
                0 => {
                    let result = outcome.unwrap();
                    if multiple {
                        assert_eq!(result.len(), 3);
                        for (i, expected) in
                            [&b"A"[..], &b""[..], &[233][..]].into_iter().enumerate()
                        {
                            assert!(
                                result
                                    .try_string_elt(i as i64)
                                    .unwrap()
                                    .try_char_eq(expected)
                                    .unwrap()
                            );
                        }
                    } else {
                        assert_eq!(result.len(), 1);
                        let expected = if publication {
                            &[195, 169][..]
                        } else {
                            &b"ABC"[..]
                        };
                        assert!(
                            result
                                .try_string_elt(0)
                                .unwrap()
                                .try_char_eq(expected)
                                .unwrap()
                        );
                    }
                    crate::sexp::gengc::full_gc();
                    assert!(!source.is_live());
                }
                1 => assert_eq!(*outcome.unwrap_err().downcast::<u32>().unwrap(), 991),
                2 => {
                    assert!(!weak.is_live());
                    assert!(
                        outcome
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
fn raw_to_char_multiple_keeps_input_without_a_bulk_workspace() {
    callback_case(0, true, false);
}
#[test]
fn raw_to_char_character_publication_keeps_ticket_and_native_tag() {
    callback_case(0, false, true);
}
#[test]
fn raw_to_char_live_callback_panic_releases_original_workspace() {
    callback_case(1, false, false);
}
#[test]
fn raw_to_char_revoked_original_cannot_publish() {
    callback_case(2, false, false);
}
