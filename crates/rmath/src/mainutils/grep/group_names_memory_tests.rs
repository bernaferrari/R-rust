use super::*;
use crate::sexp::session::RSession;

#[test]
fn owned_perl_group_names_retained_capacity_is_charged_to_original_ledger() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let (count, names) = perl_group_info("(?<first>a)(?<second>b)", false).unwrap();
    assert_eq!(count, 2);
    assert_eq!(names.iter().collect::<Vec<_>>(), ["first", "second"]);
    let physical = names.physical_capacity();
    assert!(physical > 0);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), physical);
    drop(names);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}

#[test]
fn owned_perl_group_names_partial_copy_denial_releases_and_recovers_at_same_cap() {
    use crate::sexp::memory::ArenaBudget;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let workspace = regex_workspace();
    let used = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    let cap = used + 2 * std::mem::size_of::<WorkspaceText>() + 5;
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(cap, 0)))
        .unwrap();
    assert!(
        catch_unwind(AssertUnwindSafe(|| GroupNames::copy(
            &workspace,
            2,
            ["first", "second"].into_iter()
        )))
        .is_err()
    );
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    let names = GroupNames::copy(&workspace, 1, ["é"].into_iter());
    assert_eq!(names.iter().next(), Some("é"));
    assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), cap);
    drop(names);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}

#[test]
fn owned_perl_group_names_original_owner_and_revoked_read_lifetime() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let first = RSession::new_for_gc_tests();
    let witness = first
        .owner_token()
        .unwrap()
        .node_factory()
        .strings(&["ledger"])
        .unwrap();
    let ledger = witness.allocation().unwrap().heap_identity().byte_ledger();
    let workspace = regex_workspace();
    let second = RSession::new_for_gc_tests();
    let other = second.owner_token().unwrap();
    let before = other.with_arena(|a| a.workspace_bytes()).unwrap();
    let names = GroupNames::copy(&workspace, 2, ["é", ""].into_iter());
    assert_eq!(ledger.workspace_bytes_for_test(), names.physical_capacity());
    assert_eq!(other.with_arena(|a| a.workspace_bytes()).unwrap(), before);
    drop(witness);
    drop(first);
    assert_eq!(names.iter().collect::<Vec<_>>(), ["é", ""]);
    assert!(
        catch_unwind(AssertUnwindSafe(|| GroupNames::copy(
            &workspace,
            1,
            ["x"].into_iter()
        )))
        .is_err()
    );
    drop(names);
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
    assert_eq!(other.with_arena(|a| a.workspace_bytes()).unwrap(), before);
}
