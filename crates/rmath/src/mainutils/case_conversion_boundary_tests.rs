use crate::sexp::RSession;
use crate::sexp::{
    encoding::{self, TextEncoding},
    ffi::SEXPTYPE,
    object::{SessionNodeFactory, Sexp, SexpMut},
};

fn utf8_strings(factory: &SessionNodeFactory<'_>, values: &[&str]) -> Sexp<'static> {
    let mut out = SexpMut::try_from_checked(
        factory
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, values.len() as i64)))
            .unwrap(),
    )
    .unwrap();
    for (i, value) in values.iter().enumerate() {
        let character = factory
            .allocate(|a| encoding::allocate_character(a, value.as_bytes(), TextEncoding::Utf8))
            .unwrap();
        out.try_set_string_elt(i as i64, character).unwrap();
    }
    out.freeze().into_owned().unwrap()
}

#[test]
fn shared_lower_level_entry_uses_simple_scalar_casing() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let upper = utf8_strings(&factory, &["éßᾀ𐐨"]);
        let lower = utf8_strings(&factory, &["İΣẞΟΣ"]);
        // Native adapter consumes owned roots; no borrowed payload spans it.
        let upper = unsafe { crate::mainutils::character::toupper_safe(upper) }.unwrap();
        let upper = factory.wrap(upper).unwrap();
        assert!(
            upper
                .try_string_elt(0)
                .unwrap()
                .try_char_eq("Éßᾈ𐐀".as_bytes())
                .unwrap()
        );
        let lower = unsafe { crate::mainutils::character::tolower_safe(lower) }.unwrap();
        let lower = factory.wrap(lower).unwrap();
        assert!(
            lower
                .try_string_elt(0)
                .unwrap()
                .try_char_eq("iσßοσ".as_bytes())
                .unwrap()
        );
    });
}

#[test]
fn native_adapter_preserves_formal_object_bits() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let input = utf8_strings(&factory, &["abc"]);
        let node = input.allocation().unwrap();
        let heap = node.heap_identity();
        let mut header = heap.node_snapshot(node).unwrap();
        header.sxpinfo.set_obj(true);
        header
            .sxpinfo
            .set_gp(header.sxpinfo.gp() | crate::sexp::accessors::S4_OBJECT_MASK);
        heap.replace_node(node, header).unwrap();
        let raw = unsafe { crate::mainutils::character::toupper_safe(input) }.unwrap();
        let output = factory.wrap(raw).unwrap();
        assert!(output.is_object());
        assert_ne!(
            output.header().sxpinfo.gp() & crate::sexp::accessors::S4_OBJECT_MASK,
            0
        );
        assert!(
            output
                .try_string_elt(0)
                .unwrap()
                .try_char_eq(b"ABC")
                .unwrap()
        );
    });
}

fn original_input_callback(action: u8, during_publication: bool) {
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
    let factory = weak.node_factory().unwrap();
    // Pin owns the original allocation through every callback and revocation.
    unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let input = utf8_strings(&factory, &["éßᾀ", "abc"]);
            let source = input.allocation().unwrap().clone();
            let args = factory
                .pairlist_cell(&input, &factory.nil(), &factory.nil())
                .unwrap()
                .into_owned()
                .unwrap();
            drop(input);
            let saved = args.clone();
            let observed = source.clone();
            let ledger = source.heap_identity().byte_ledger();
            let called = Rc::new(Cell::new(0));
            let count = called.clone();
            let revoker = Rc::downgrade(&facade);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if during_publication && ledger.workspace_bytes_for_test() == 0 {
                    return;
                }
                if count.replace(1) != 0 {
                    return;
                }
                (*instance).memory_state.gc_force_gap = 0;
                if during_publication {
                    assert!(ledger.workspace_bytes_for_test() >= "Éßᾈ".len());
                }
                crate::sexp::accessors::SETCAR(saved.as_raw(), crate::sexp::globals::R_NilValue());
                crate::sexp::gengc::full_gc();
                assert!(
                    observed.is_live(),
                    "selected original vector survives detachment and GC"
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
                let raw = crate::mainutils::character::case_entry(args.as_raw(), true);
                factory.wrap(raw).unwrap().into_owned().unwrap()
            }));
            assert_eq!(called.get(), 1);
            assert_eq!((*instance).memory_state.in_gc, 0);
            match action {
                0 => {
                    let result = outcome.unwrap();
                    assert!(
                        result
                            .try_string_elt(0)
                            .unwrap()
                            .try_char_eq("Éßᾈ".as_bytes())
                            .unwrap()
                    );
                    crate::sexp::gengc::full_gc();
                    assert!(
                        !source.is_live(),
                        "detached input releases after conversion"
                    );
                    assert!(
                        result
                            .try_string_elt(1)
                            .unwrap()
                            .try_char_eq(b"ABC")
                            .unwrap()
                    );
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
            assert_eq!(
                (*instance)
                    .heap_identity
                    .byte_ledger()
                    .workspace_bytes_for_test(),
                0
            );
        });
    }
}

#[test]
fn selected_input_survives_detach_gc_then_releases() {
    original_input_callback(0, false);
}
#[test]
fn original_callback_panic_propagates_without_leaking_output_admission() {
    original_input_callback(1, false);
}
#[test]
fn revoked_original_cannot_publish() {
    original_input_callback(2, false);
}

#[test]
fn output_admission_survives_character_publication_and_reentrant_gc() {
    original_input_callback(0, true);
}
