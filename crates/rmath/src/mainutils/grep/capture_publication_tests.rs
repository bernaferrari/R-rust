use super::RegexMatch;
use crate::sexp::{ffi::SEXPTYPE, session::RSession};
use std::{cell::Cell, rc::Rc};

#[test]
fn owned_gregexpr_capture_rows_stay_charged_through_real_gc_publication() {
    const NAME: &str = "original_capture_observer_7e1d";
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let pattern = factory
        .strings(&["(?<original_capture_observer_7e1d>a)"])
        .unwrap();
    let text = factory.strings(&["a-aaa"]).unwrap();
    let nil = factory.nil();
    let perl = factory
        .wrap(unsafe { crate::mainutils::relop::R_TrueValue() })
        .unwrap();
    let ignore = factory
        .wrap(unsafe { crate::mainutils::relop::R_FalseValue() })
        .unwrap();
    let tail = factory.pairlist_cell(&perl, &nil, &nil).unwrap();
    let tail = factory.pairlist_cell(&ignore, &tail, &nil).unwrap();
    let tail = factory.pairlist_cell(&text, &tail, &nil).unwrap();
    let args = factory
        .pairlist_cell(&pattern, &tail, &nil)
        .unwrap()
        .into_owned()
        .unwrap();
    let heap = args.allocation().unwrap().heap_identity().clone();
    let ledger = heap.byte_ledger();
    let callback_heap = heap.clone();
    let callback_ledger = ledger.clone();
    let seen = Rc::new(Cell::new(0));
    let called = seen.clone();
    let observed = Rc::new(Cell::new(0));
    let recorded = observed.clone();
    let pin = owner.weak_owner().unwrap().pin().unwrap();
    let instance = pin.as_ptr();
    session.with_active(|| unsafe {
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if called.get() != 0 {
                return;
            }
            for (_, node) in crate::sexp::memory::fresh_allocation_roots_for_marking(&callback_heap)
            {
                if callback_heap
                    .node_snapshot(&node)
                    .unwrap()
                    .sxpinfo
                    .type_of()
                    != SEXPTYPE::CHARSXP
                {
                    continue;
                }
                let matches = {
                    let payload = callback_heap.payload_lease(&node).unwrap();
                    NAME.bytes()
                        .enumerate()
                        .all(|(i, byte)| payload.byte_elt(i) == Some(byte))
                };
                if matches {
                    called.set(1);
                    recorded.set(callback_ledger.workspace_bytes_for_test());
                    (*instance).memory_state.gc_force_gap = 0;
                    crate::sexp::gengc::full_gc();
                    break;
                }
            }
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        let raw = crate::mainutils::essentials::do_gregexpr(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        );
        (*instance).memory_state.gc_force_gap = 0;
        let result = owner.sexp(raw).unwrap().into_owned().unwrap();
        assert_eq!(seen.get(), 1, "real capture-name publication reached");
        assert!(
            observed.get() >= 8 * std::mem::size_of::<Option<RegexMatch>>(),
            "kernel capture rows remain admitted through R allocation: {}",
            observed.get()
        );
        owner.full_gc().unwrap();
        let element = result.try_vector_elt(0).unwrap();
        let starts = owner
            .sexp(crate::sexp::attrib_core::getAttrib(
                element.as_raw(),
                crate::sexp::symbol::Rf_install(c"capture.start".as_ptr()),
            ))
            .unwrap()
            .into_owned()
            .unwrap();
        let lengths = owner
            .sexp(crate::sexp::attrib_core::getAttrib(
                element.as_raw(),
                crate::sexp::symbol::Rf_install(c"capture.length".as_ptr()),
            ))
            .unwrap()
            .into_owned()
            .unwrap();
        for (index, expected) in [1, 3, 4, 5].into_iter().enumerate() {
            assert_eq!(starts.try_integer_elt(index as _).unwrap(), expected);
            assert_eq!(lengths.try_integer_elt(index as _).unwrap(), 1);
        }
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    });
}

#[test]
fn owned_regexec_named_captures_survive_every_collecting_constructor() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let nil = factory.nil();
    let pattern = factory.strings(&["(?<one>a)(b)?"]).unwrap();
    let text = factory.strings(&["ab", "a", "x"]).unwrap();
    let yes = factory.domain().logical(true);
    let no = factory.domain().logical(false);
    let mut args = nil.clone();
    for value in [pattern, text, no.clone(), yes, no.clone(), no]
        .into_iter()
        .rev()
    {
        args = factory.pairlist_cell(&value, &args, &nil).unwrap();
    }
    let pin = owner.weak_owner().unwrap().pin().unwrap();
    let instance = pin.as_ptr();
    let called = Rc::new(Cell::new(0));
    let observed = called.clone();
    session.with_active(|| unsafe {
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1)
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        let raw = crate::mainutils::essentials::do_regexec(
            nil.as_raw(),
            nil.as_raw(),
            args.as_raw(),
            nil.as_raw(),
        );
        let result = owner.sexp(raw).unwrap().into_owned().unwrap();
        (*instance).memory_state.gc_force_gap = 0;
        assert!(called.get() > 0);
        owner.full_gc().unwrap();
        for (i, expected) in [&[1, 1, 2][..], &[1, 1, 0][..], &[-1][..]]
            .into_iter()
            .enumerate()
        {
            let element = result.try_vector_elt(i as _).unwrap();
            for (j, value) in expected.iter().enumerate() {
                assert_eq!(element.try_integer_elt(j as _).unwrap(), *value);
            }
            let names = owner
                .sexp(crate::sexp::attrib_core::getAttrib(
                    element.as_raw(),
                    crate::sexp::attrib_core::R_NamesSymbol(),
                ))
                .unwrap();
            if i < 2 {
                assert_eq!(names.len(), 3);
                for (j, name) in ["", "one", ""].into_iter().enumerate() {
                    assert!(
                        names
                            .try_string_elt(j as _)
                            .unwrap()
                            .try_char_eq(name.as_bytes())
                            .unwrap()
                    );
                }
            } else {
                assert!(names.is_nil());
            }
        }
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    });
}
