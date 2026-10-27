//! Actual TRE compiler admission, independently of input/capture buffers.
#![forbid(unsafe_code)]

use super::*;
use crate::sexp::{memory::ArenaBudget, session::RSession};

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
    let result = TreRegex::compile("a", false);
    assert!(
        result.is_err(),
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
        let compiled = TreRegex::compile(pattern, false);
        if compiled.is_err() {
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
