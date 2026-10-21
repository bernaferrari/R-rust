//! Original-budget controls for the actual retained EncodeString storage.
use super::*;
use crate::sexp::{RSession, memory::ArenaBudget};

#[test]
fn printer_workspace_default_has_no_unfunded_native_buffer() {
    assert_eq!(
        PrintUtilsState::default().encode_string.capacity(),
        0,
        "formatter defaults must not allocate before their original ledger exists"
    );
}

#[test]
fn printer_workspace_retained_capacity_remains_in_the_original_ledger() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let value = factory.character(&"x".repeat(16384)).unwrap();
        let old = (*owner.as_ptr())
            .eval_state
            .printutils
            .encode_string
            .capacity();
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        let encoded = EncodeString(value.as_raw(), 0, 0, Rprt_adj::none);
        assert_eq!(CStr::from_ptr(encoded).to_bytes().len(), 16384);
        let capacity = (*owner.as_ptr())
            .eval_state
            .printutils
            .encode_string
            .capacity();
        let after = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        assert_eq!(
            after - before,
            capacity - old,
            "actual retained native capacity must remain charged"
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    });
}

fn denied_growth(delta_only: bool, unicode: bool) {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let short = factory.character("ok").unwrap();
        let previous = EncodeString(short.as_raw(), 0, 0, Rprt_adj::none);
        assert_eq!(CStr::from_ptr(previous).to_bytes(), b"ok");
        let capacity = (*owner.as_ptr())
            .eval_state
            .printutils
            .encode_string
            .capacity();
        let text = if unicode {
            "é".repeat(4096)
        } else {
            "x".repeat(if delta_only { capacity + 32 } else { 16384 })
        };
        let value = factory
            .allocate(|arena| {
                crate::sexp::encoding::allocate_character(
                    arena,
                    text.as_bytes(),
                    crate::sexp::encoding::TextEncoding::Utf8,
                )
            })
            .unwrap();
        let headroom = if delta_only {
            text.len() + 1 - capacity
        } else {
            128
        };
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before + headroom, 0)))
            .unwrap();
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            EncodeString(
                value.as_raw(),
                0,
                if unicode { b'"' as i32 } else { 0 },
                Rprt_adj::none,
            );
        }));
        assert!(
            denied.is_err(),
            "native formatter escaped original headroom={headroom}, UTF8-profile={}",
            crate::mainutils::platform::locale_is_utf8()
        );
        assert!(
            denied
                .unwrap_err()
                .downcast_ref::<crate::sexp::context::RError>()
                .is_some()
        );
        assert_eq!(
            (*owner.as_ptr())
                .eval_state
                .printutils
                .encode_string
                .capacity(),
            capacity
        );
        assert_eq!(
            CStr::from_ptr(previous).to_bytes(),
            b"ok",
            "denial must retain the old buffer and its contents"
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        let recovered = EncodeString(short.as_raw(), 0, 0, Rprt_adj::none);
        assert_eq!(CStr::from_ptr(recovered).to_bytes(), b"ok");
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
        assert_eq!(
            owner.with_arena(|a| a.budget().max_bytes).unwrap(),
            before + headroom
        );
    });
}
#[test]
fn printer_workspace_growth_denial_preserves_buffer_and_same_limit_recovery() {
    denied_growth(false, false);
}
#[test]
fn printer_workspace_growth_requires_the_complete_replacement_peak() {
    denied_growth(true, false);
}
#[test]
fn printer_workspace_unicode_escape_growth_obeys_original_limit() {
    denied_growth(false, true);
}

#[test]
fn printer_workspace_existing_capacity_control_needs_no_extra_headroom() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let value = owner.node_factory().character("ok").unwrap();
        assert_eq!(
            CStr::from_ptr(EncodeString(value.as_raw(), 0, 0, Rprt_adj::none)).to_bytes(),
            b"ok"
        );
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before, 0)))
            .unwrap();
        assert_eq!(
            CStr::from_ptr(EncodeString(value.as_raw(), 0, 0, Rprt_adj::none)).to_bytes(),
            b"ok"
        );
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), before);
    });
}

#[test]
fn printer_workspace_registered_arena_lend_preserves_the_original_limit() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let short = factory.character("ok").unwrap();
        let large = factory.character(&"x".repeat(16384)).unwrap();
        EncodeString(short.as_raw(), 0, 0, Rprt_adj::none);
        owner
            .with_arena(|arena| {
                let before = arena.total_bytes_allocated();
                arena.set_budget(ArenaBudget::new(before + 32, 0));
                let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    EncodeString(large.as_raw(), 0, 0, Rprt_adj::none);
                }));
                assert!(denied.is_err());
                assert_eq!(arena.workspace_bytes(), 0);
                assert_eq!(
                    CStr::from_ptr(EncodeString(short.as_raw(), 0, 0, Rprt_adj::none)).to_bytes(),
                    b"ok"
                );
                assert_eq!(arena.total_bytes_allocated(), before);
                assert_eq!(arena.budget().max_bytes, before + 32);
            })
            .unwrap();
    });
}

#[test]
fn printer_workspace_charge_releases_with_its_final_runtime_owner() {
    let session = RSession::new_for_gc_tests();
    let ledger = session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let value = owner.node_factory().character(&"x".repeat(16384)).unwrap();
        EncodeString(value.as_raw(), 0, 0, Rprt_adj::none);
        let ledger = (*owner.as_ptr()).heap_identity.byte_ledger();
        let retained = (*owner.as_ptr())
            .eval_state
            .printutils
            .encode_string
            .capacity();
        assert!(ledger.allocated_bytes() >= retained && retained >= 16385);
        ledger
    });
    drop(session);
    assert_eq!(
        ledger.allocated_bytes(),
        0,
        "no formatter storage or byte charge may outlive its final owner"
    );
}

#[test]
fn printer_workspace_native_quote_presence_is_distinct_from_its_byte() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let value = session
            .owner_token()
            .unwrap()
            .node_factory()
            .character("x")
            .unwrap();
        for (quote, expected) in [
            (0, b"x".as_slice()),
            (34, b"\"x\"".as_slice()),
            (256, b"".as_slice()),
            (-256, b"".as_slice()),
        ] {
            let encoded = EncodeString(value.as_raw(), 0, quote, Rprt_adj::none);
            assert_eq!(CStr::from_ptr(encoded).to_bytes(), expected);
        }
    });
}
