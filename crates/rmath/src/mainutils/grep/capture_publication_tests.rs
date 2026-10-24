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
