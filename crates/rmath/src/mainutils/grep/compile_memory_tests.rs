//! Actual TRE compiler admission, independently of input/capture buffers.
#![forbid(unsafe_code)]

use super::*;
use crate::sexp::{memory::ArenaBudget, session::RSession};

#[test]
fn retained_tre_tables_stay_charged_until_compiled_regex_drop() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let before = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
    let compiled = TreRegex::compile("abcdefghijklmnopqrstuvwxyz", false).unwrap();
    assert!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap() > before,
        "actual retained TNFA tables escaped the original ledger"
    );
    drop(compiled);
    assert_eq!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
        before
    );
}

#[test]
fn retained_tre_tables_cannot_accumulate_beyond_fixed_session_cap() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    // Native retains the original26-character/256KiB bypass reproduction.
    // Miri uses the same cumulative-admission invariant with a smaller literal.
    let (extra, pattern, attempts) = if cfg!(miri) {
        (16 * 1024, "a", 512)
    } else {
        (256 * 1024, "abcdefghijklmnopqrstuvwxyz", 256)
    };
    owner
        .with_arena(|arena| {
            let used = arena.total_bytes_allocated();
            arena.set_budget(ArenaBudget::new(used + extra, 0));
        })
        .unwrap();
    // A literal needs at least one transition record per character; this
    // lower bound alone exceeds the cap if every compiled object survives.
    assert!(
        attempts
            * pattern.chars().count()
            * std::mem::size_of::<crate::tre::ast::tre_tnfa_transition_t>()
            > extra
    );
    let mut retained = Vec::new();
    let mut rejected = false;
    for _ in 0..attempts {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            TreRegex::compile(pattern, false)
        })) {
            Ok(Ok(regex)) => retained.push(regex),
            Err(error) => {
                assert!(error.is::<RError>());
                rejected = true;
                break;
            }
            Ok(Err(error)) => panic!("valid literal rejected as syntax: {error}"),
        }
    }
    assert!(
        !retained.is_empty(),
        "control must admit an individual compile"
    );
    assert!(
        rejected,
        "retained TNFA tables accumulated beyond the fixed byte cap"
    );
    let limit = owner.with_arena(|arena| arena.budget().max_bytes).unwrap();
    drop(retained.pop());
    let recovered = TreRegex::compile(pattern, false).unwrap();
    assert_eq!(
        owner.with_arena(|arena| arena.budget().max_bytes).unwrap(),
        limit
    );
    drop(recovered);
    drop(retained);
    assert_eq!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
        0
    );
}

#[test]
fn tre_compile_rejects_when_only_wide_pattern_fits() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    owner
        .with_arena(|arena| {
            let used = arena.total_bytes_allocated();
            arena.set_budget(ArenaBudget::new(used + std::mem::size_of::<u32>(), 0));
        })
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        TreRegex::compile("a", false)
    }));
    assert!(
        result.err().unwrap().is::<RError>(),
        "TRE compiler allocated engine storage beyond the byte cap"
    );
    assert_eq!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
        0
    );
}

#[test]
fn tre_compile_arena_failure_releases_partial_storage_and_recovers() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let pattern = "((ab|[c-e]){1,3})+";
    let mut rejected = 0;
    for extra in [128, 1024, 2048, 4096, 8192, 16384, 32768] {
        owner
            .with_arena(|arena| {
                let used = arena.total_bytes_allocated();
                arena.set_budget(ArenaBudget::new(used + extra, 0));
            })
            .unwrap();
        let compiled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            TreRegex::compile(pattern, false)
        }));
        if let Err(error) = &compiled {
            assert!(error.is::<RError>());
            rejected += 1;
        }
        drop(compiled);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }
    assert!(
        rejected > 1,
        "exercise admission denial at multiple partial-storage stages"
    );
    owner
        .with_arena(|arena| arena.set_budget(ArenaBudget::new(0, 0)))
        .unwrap();
    let regex = TreRegex::compile(pattern, false).unwrap();
    assert!(regex.captures("ababecab").is_some());
    drop(regex);
    assert_eq!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
        0
    );
}

#[test]
fn retained_tre_tables_preserve_actual_captures_across_gc_and_owner_moves() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let ledger = owner
        .with_arena(|arena| arena.heap_identity().byte_ledger())
        .unwrap();
    let held = ledger.reserve(128).unwrap();
    let mut moved = Vec::new();
    for _ in 0..8 {
        moved.push(TreRegex::compile("((ab|[c-e]){1,3})+", false).unwrap());
    }
    owner.full_gc().unwrap();
    for regex in &moved {
        let captures = regex.captures("ababecab").unwrap();
        assert_eq!(captures[0], Some(RegexMatch { start: 0, end: 8 }));
        assert_eq!(captures[1], Some(RegexMatch { start: 4, end: 8 }));
    }
    drop(moved);
    assert_eq!(ledger.workspace_bytes_for_test(), 128);
    drop(held);
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
}

#[test]
fn retained_tre_tables_revoked_owner_drop_preserves_replacement_tables() {
    let mut left = RSession::new_for_gc_tests();
    let left_owner = left.owner_token().unwrap();
    let left_ledger = left_owner
        .with_arena(|arena| arena.heap_identity().byte_ledger())
        .unwrap();
    let left_held = left_ledger.reserve(128).unwrap();
    let regex = TreRegex::compile("((ab|[c-e]){1,3})+", false).unwrap();
    let right = RSession::new_for_gc_tests();
    let right_owner = right.owner_token().unwrap();
    let right_ledger = right_owner
        .with_arena(|arena| arena.heap_identity().byte_ledger())
        .unwrap();
    let right_held = right_ledger.reserve(256).unwrap();
    let survivor = TreRegex::compile("(é+)", false).unwrap();
    let right_charge = right_ledger.workspace_bytes_for_test();
    assert!(right_charge > 256);
    left.close();
    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| regex.captures("ab")));
    assert!(rejected.unwrap_err().is::<RError>());
    drop(regex);
    assert_eq!(left_ledger.workspace_bytes_for_test(), 128);
    assert_eq!(right_ledger.workspace_bytes_for_test(), right_charge);
    assert_eq!(
        survivor.captures("aéé").unwrap()[0],
        Some(RegexMatch { start: 1, end: 5 })
    );
    drop(survivor);
    assert_eq!(right_ledger.workspace_bytes_for_test(), 256);
    drop(left_held);
    drop(right_held);
    assert_eq!(left_ledger.workspace_bytes_for_test(), 0);
    assert_eq!(right_ledger.workspace_bytes_for_test(), 0);
}

#[test]
fn tre_option_adapters_propagate_memory_denial_and_recover_at_original_cap() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let ledger = owner
        .with_arena(|arena| arena.heap_identity().byte_ledger())
        .unwrap();
    let headroom = 64 * 1024;
    owner
        .with_arena(|arena| {
            let used = arena.total_bytes_allocated();
            arena.set_budget(ArenaBudget::new(used + headroom, 0));
        })
        .unwrap();
    let cap = owner.with_arena(|arena| arena.budget().max_bytes).unwrap();
    let held = ledger.reserve(headroom - 4).unwrap();
    for adapter in 0..6 {
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match adapter {
            0 => {
                ere_find_at("a", "a", false, 0, false);
            }
            1 => {
                mode_captures("a", "a", false, false, false);
            }
            2 => {
                ere_find("a", "a", false);
            }
            3 => {
                ere_captures("a", "a", false);
            }
            4 => {
                ere_find_at("a", "a", false, 0, true);
            }
            _ => {
                validate_pattern("a", false, false, false).unwrap();
            }
        }))
        .expect_err("memory denial became syntax error or successful no-match");
        let error = error.downcast_ref::<RError>().expect("typed R error");
        assert_eq!(
            error.message,
            "regex workspace exceeds session memory limit"
        );
        assert_eq!(ledger.workspace_bytes_for_test(), headroom - 4);
        assert_eq!(
            owner.with_arena(|arena| arena.budget().max_bytes).unwrap(),
            cap
        );
    }
    drop(held);
    assert_eq!(
        ere_find_at("a", "a", false, 0, false),
        Some(RegexMatch { start: 0, end: 1 })
    );
    assert_eq!(
        owner.with_arena(|arena| arena.budget().max_bytes).unwrap(),
        cap
    );
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
    assert!(
        validate_pattern("[", false, false, false)
            .unwrap_err()
            .contains("invalid regular expression")
    );
    assert_eq!(ere_find_at("[", "a", false, 0, false), None);
}
