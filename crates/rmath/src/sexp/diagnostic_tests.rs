use super::*;
use crate::sexp::{RSession, memory::ArenaBudget, owner::with_runtime};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn bytes(diagnostic: &Diagnostic) -> Vec<u8> {
    (0..diagnostic.len())
        .map(|i| diagnostic.byte(i).unwrap())
        .collect()
}

fn prepared_text(parent: &Sexp<'_>, text: &str) -> PreparedDiagnostic {
    let ledger = parent.allocation().unwrap().heap_identity().byte_ledger();
    let mut ticket = ledger.reserve(text.len()).unwrap();
    let mut message = String::new();
    message.try_reserve_exact(text.len()).unwrap();
    ticket.try_resize(message.capacity()).unwrap();
    message.push_str(text);
    PreparedDiagnostic::from_admitted_string(message, ticket).unwrap()
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn send_diagnostic_keeps_original_charge_through_unwind_and_session_close() {
    let mut session = RSession::new_for_gc_tests();
    let (diagnostic, ledger, bytes) = session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let parent = owner.node_factory().character("anchor").unwrap();
        let ledger = parent.allocation().unwrap().heap_identity().byte_ledger();
        let prepared = prepared_text(&parent, "non-ASCII: á中😀");
        let bytes = prepared.capacity();
        assert_eq!(ledger.workspace_bytes_for_test(), bytes);
        let resident = owner
            .with_arena(|arena| arena.total_bytes_allocated())
            .unwrap();
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::new(resident + bytes, 0)))
            .unwrap();
        assert!(ledger.reserve(1).is_none());
        let weak = owner.weak_owner().unwrap();
        let diagnostic = with_runtime(&weak, |access| prepared.into_send(access, &parent))
            .unwrap()
            .unwrap();
        assert_eq!(ledger.workspace_bytes_for_test(), bytes);
        (diagnostic, ledger, bytes)
    });
    session.close();
    assert_eq!(ledger.workspace_bytes_for_test(), bytes);
    let payload = std::thread::spawn(move || {
        catch_unwind(AssertUnwindSafe(|| std::panic::panic_any(diagnostic))).unwrap_err()
    })
    .join()
    .unwrap();
    let diagnostic = payload.downcast::<SendDiagnostic>().unwrap();
    assert_eq!(diagnostic.as_str(), "non-ASCII: á中😀");
    assert_eq!(ledger.workspace_bytes_for_test(), bytes);
    std::thread::spawn(move || drop(diagnostic)).join().unwrap();
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
}

#[test]
fn send_diagnostic_rejects_foreign_admission_and_releases_it() {
    let original = RSession::new_for_gc_tests();
    let (prepared, ledger) = original.with_active(|| {
        let parent = original
            .owner_token()
            .unwrap()
            .node_factory()
            .character("original")
            .unwrap();
        let ledger = parent.allocation().unwrap().heap_identity().byte_ledger();
        (prepared_text(&parent, "owned by the original"), ledger)
    });
    assert!(ledger.workspace_bytes_for_test() > 0);
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let parent = owner.node_factory().character("foreign").unwrap();
        let weak = owner.weak_owner().unwrap();
        let result = with_runtime(&weak, |access| prepared.into_send(access, &parent)).unwrap();
        assert!(matches!(result, Err(SexpError::HeapDomainMismatch)));
    });
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
}

#[test]
fn send_diagnostic_budget_denial_recovers_at_the_same_limit() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let parent = owner.node_factory().character("anchor").unwrap();
        let ledger = parent.allocation().unwrap().heap_identity().byte_ledger();
        let prepared = prepared_text(&parent, "funded message");
        let capacity = prepared.capacity();
        let resident = owner
            .with_arena(|arena| arena.total_bytes_allocated())
            .unwrap();
        let limit = resident + capacity;
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::new(limit, 0)))
            .unwrap();
        let weak = owner.weak_owner().unwrap();
        let message = with_runtime(&weak, |access| prepared.into_send(access, &parent))
            .unwrap()
            .unwrap();
        assert!(ledger.reserve(1).is_none());
        drop(message);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert!(ledger.reserve(1).is_some());
        assert_eq!(
            owner.with_arena(|arena| arena.budget().max_bytes).unwrap(),
            limit
        );
    });
}

#[test]
fn send_diagnostic_cannot_publish_after_owner_revocation() {
    let mut session = RSession::new_for_gc_tests();
    let (prepared, parent, ledger, weak) = session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let ledger = parent.allocation().unwrap().heap_identity().byte_ledger();
        let prepared = prepared_text(&parent, "not published after close");
        (prepared, parent, ledger, owner.weak_owner().unwrap())
    });
    assert!(ledger.workspace_bytes_for_test() > 0);
    session.close();
    let result = with_runtime(&weak, |access| prepared.into_send(access, &parent));
    assert!(matches!(result, Err(SexpError::RootUnavailable)));
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
}

#[test]
fn send_diagnostic_rejects_foreign_parent_before_publication() {
    let original = RSession::new_for_gc_tests();
    let parent = original.with_active(|| {
        original
            .owner_token()
            .unwrap()
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let original_ledger = parent.allocation().unwrap().heap_identity().byte_ledger();
    let prepared = original.with_active(|| prepared_text(&parent, "must not be published"));
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        let before = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
        let result = with_runtime(&weak, |access| prepared.into_send(access, &parent)).unwrap();
        assert!(matches!(result, Err(SexpError::UnownedPointer { .. })));
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            before
        );
    });
    assert_eq!(original_ledger.workspace_bytes_for_test(), 0);
}

#[test]
fn owned_diagnostic_copy_denial_recovers_with_smaller_message_at_unchanged_limit() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(resident + 6, 0)))
            .unwrap();
        let weak = owner.weak_owner().unwrap();
        with_runtime(&weak, |access| {
            let mut read = false;
            assert!(matches!(
                MessageCopy::new(access, &parent, 7, 999, true, |_| {
                    read = true;
                    b'a'
                }),
                Err(SexpError::AllocationFailed { .. })
            ));
            assert!(!read, "deny the full copy before reading any source byte");
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            let small = MessageCopy::new(access, &parent, 1, 999, true, |_| b'x').unwrap();
            assert_eq!(small.bytes, b"x");
            assert_eq!(
                owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                small.bytes.capacity()
            );
            drop(small);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            assert!(owner.with_arena(|a| a.total_bytes_allocated()).unwrap() <= resident + 6);
            assert_eq!(
                owner.with_arena(|a| a.budget().max_bytes).unwrap(),
                resident + 6
            );
        })
        .unwrap();
    });
}

#[test]
fn owned_diagnostic_bytes_and_charge_survive_facade_teardown_and_clone() {
    let original = RSession::new_for_gc_tests();
    let (diagnostic, ledger) = original.with_active(|| {
        let owner = original.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let ledger = parent.allocation().unwrap().heap_identity().byte_ledger();
        let weak = owner.weak_owner().unwrap();
        let diagnostic = with_runtime(&weak, |access| {
            Diagnostic::new(access, &parent, 3, 999, true, |i| b"a\xffz"[i])
        })
        .unwrap()
        .unwrap();
        (diagnostic, ledger)
    });
    let alias = diagnostic.clone();
    drop(original);
    let other = RSession::new_for_gc_tests();
    let before = other.with_active(|| {
        other
            .owner_token()
            .unwrap()
            .with_arena(|a| a.total_bytes_allocated())
            .unwrap()
    });
    assert!(ledger.allocated_bytes() >= 4);
    assert_eq!(bytes(&diagnostic), b"a\xffz");
    assert_eq!(diagnostic.to_string(), "a\\xffz");
    drop(diagnostic);
    assert_eq!(bytes(&alias), b"a\xffz");
    assert!(ledger.allocated_bytes() >= 4);
    drop(alias);
    assert_eq!(ledger.allocated_bytes(), 0);
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
    other.with_active(|| {
        assert_eq!(
            other
                .owner_token()
                .unwrap()
                .with_arena(|a| a.total_bytes_allocated())
                .unwrap(),
            before
        )
    });
}

#[test]
fn owned_diagnostic_foreign_parent_rejects_before_copy_or_current_allocation() {
    let original = RSession::new_for_gc_tests();
    let parent = original.with_active(|| {
        original
            .owner_token()
            .unwrap()
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let before = owner.with_arena(|a| (a.total_bytes_allocated(),a.workspace_bytes())).unwrap();
        let weak = owner.weak_owner().unwrap();
        let mut read = false;
        let result = with_runtime(&weak, |access| Diagnostic::new(access, &parent, 3, 999, true, |_| {read=true;b'a'})).unwrap();
        assert!(matches!(result, Err(SexpError::UnownedPointer { address }) if address == parent.as_raw().addr()));
        assert!(!read);
        assert_eq!(owner.with_arena(|a| (a.total_bytes_allocated(),a.workspace_bytes())).unwrap(), before);
    });
}

#[test]
fn owned_diagnostic_truncation_repairs_only_final_sequence_in_utf8_profile() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let weak = owner.weak_owner().unwrap();
        with_runtime(&weak, |access| {
            let source = b"a\xff\xc3\xa9Z";
            for (utf8, limit, expected) in [
                (true, 3, &b"a\xff"[..]),
                (true, 2, &b"a"[..]),
                (false, 3, &b"a\xff\xc3"[..]),
                (true, 5, &source[..]),
            ] {
                let diagnostic =
                    Diagnostic::new(access, &parent, source.len(), limit, utf8, |i| source[i])
                        .unwrap();
                assert_eq!(bytes(&diagnostic), expected);
                assert_eq!(
                    encoding::TextEncoding::from_gp(
                        diagnostic.message(access).unwrap().header().sxpinfo.gp()
                    ),
                    TextEncoding::Native
                );
            }
            let source = b"a\x80\x80z";
            let diagnostic =
                Diagnostic::new(access, &parent, source.len(), 3, true, |i| source[i]).unwrap();
            assert_eq!(bytes(&diagnostic), b"a");
        })
        .unwrap();
    });
}

#[test]
fn owned_diagnostic_source_panic_releases_original_copy_ticket() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let weak = owner.weak_owner().unwrap();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        let result = catch_unwind(AssertUnwindSafe(|| {
            with_runtime(&weak, |access| {
                Diagnostic::new(access, &parent, 3, 999, true, |_| {
                    std::panic::panic_any(991u32)
                })
            })
        }));
        assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 991);
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}

fn prepared(parent: &Sexp<'_>, source: &[u8]) -> (Vec<u8>, WorkspaceReservation) {
    let mut ticket = parent
        .allocation()
        .unwrap()
        .heap_identity()
        .byte_ledger()
        .reserve(source.len())
        .unwrap();
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(source.len()).unwrap();
    ticket.try_resize(bytes.capacity()).unwrap();
    bytes.extend_from_slice(source);
    (bytes, ticket)
}

#[test]
fn nul_diagnostic_prepared_buffer_promotes_into_canonical_original_charge() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let ledger = parent.allocation().unwrap().heap_identity().byte_ledger();
        let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        let (data, ticket) = prepared(&parent, b"prepared\xff");
        let capacity = data.capacity();
        assert_eq!(ledger.workspace_bytes_for_test(), capacity);
        let diagnostic = with_runtime(&owner.weak_owner().unwrap(), |access| {
            Diagnostic::from_workspace(access, &parent, data, ticket)
        })
        .unwrap()
        .unwrap();
        assert_eq!(bytes(&diagnostic), b"prepared\xff");
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert!(owner.with_arena(|a| a.total_bytes_allocated()).unwrap() >= resident + capacity);
    });
}

#[test]
fn nul_diagnostic_prepared_foreign_ticket_cannot_fund_current_heap() {
    let original = RSession::new_for_gc_tests();
    let (data, ticket, ledger) = original.with_active(|| {
        let parent = original
            .owner_token()
            .unwrap()
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let ledger = parent.allocation().unwrap().heap_identity().byte_ledger();
        let (data, ticket) = prepared(&parent, b"foreign");
        (data, ticket, ledger)
    });
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        let result = with_runtime(&owner.weak_owner().unwrap(), |access| {
            Diagnostic::from_workspace(access, &parent, data, ticket)
        })
        .unwrap();
        assert!(matches!(result, Err(SexpError::HeapDomainMismatch)));
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}

#[test]
fn nul_diagnostic_prepared_underfunded_capacity_rejects_before_publication() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let ledger = parent.allocation().unwrap().heap_identity().byte_ledger();
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        // A malformed internal packet must not authenticate a larger buffer.
        let ticket = ledger.reserve(1).unwrap();
        let data = b"larger".to_vec();
        assert!(data.capacity() > ticket.bytes());
        let result = with_runtime(&owner.weak_owner().unwrap(), |access| {
            Diagnostic::from_workspace(access, &parent, data, ticket)
        })
        .unwrap();
        assert!(matches!(
            result,
            Err(SexpError::AllocationFailed {
                object: "evaluation diagnostic"
            })
        ));
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
    });
}

#[test]
fn prepared_diagnostic_rejects_underfunded_capacity_before_publication() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let ledger = parent.allocation().unwrap().heap_identity().byte_ledger();
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        let mut message = String::with_capacity(8);
        message.push_str("message");
        let ticket = ledger.reserve(message.capacity() - 1).unwrap();
        assert!(matches!(
            PreparedDiagnostic::from_admitted_string(message, ticket),
            Err(SexpError::AllocationFailed { .. })
        ));
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
    });
}
