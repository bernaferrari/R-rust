use super::*;
use crate::sexp::{RSession, memory::ArenaBudget, owner::with_runtime};
#[test]
fn derivative_indices_admit_before_allocation_and_recover() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let held = owner
        .with_arena(|a| a.reserve_workspace(128).unwrap())
        .unwrap();
    let budget = owner
        .with_arena(|a| {
            let old = a.budget();
            a.set_budget(ArenaBudget::new(a.total_bytes_allocated() + 128 + 19, 0));
            old
        })
        .unwrap();
    let authority = owner.weak_owner().unwrap();
    assert!(
        with_runtime(&authority, |access| Indices::new(access, 2, true))
            .unwrap()
            .is_err()
    );
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 128);
    owner.with_arena(|a| a.set_budget(budget)).unwrap();
    let values = with_runtime(&authority, |access| Indices::new(access, 2, true))
        .unwrap()
        .unwrap();
    assert_eq!(values.first, &[0, 0]);
    assert_eq!(values.second, &[0, 0, 0]);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 148);
    drop(values);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 128);
    drop(held);
}
#[test]
fn derivative_indices_check_hessian_size_before_allocating() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let limit = owner
        .with_arena(|a| {
            let limit = a.total_bytes_allocated() + 1024;
            a.set_budget(ArenaBudget::new(limit, 0));
            limit
        })
        .unwrap();
    let authority = owner.weak_owner().unwrap();
    for count in [46341, usize::MAX] {
        assert!(
            with_runtime(&authority, |access| Indices::new(access, count, true))
                .unwrap()
                .is_err()
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
    }
}

#[test]
fn derivative_temporary_name_admits_both_owned_and_interner_scratch() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let authority = owner.weak_owner().unwrap();
    let held = owner
        .with_arena(|a| a.reserve_workspace(128).unwrap())
        .unwrap();
    with_runtime(&authority, |access| {
        let builder = Builder::new(access);
        let tag = access
            .allocator(&access.domain())
            .unwrap()
            .character(".tmp")
            .unwrap()
            .into_owned()
            .unwrap();
        let expressions = builder.call("{", &[]).unwrap();
        let program = Program {
            access,
            builder,
            expressions,
            tag,
            tag_length: 4,
        };
        let (budget, before) = access
            .with_arena(|a| {
                let before = a.total_bytes_allocated();
                let old = a.budget();
                // '.tmp7' needs six owned bytes plus five interner bytes.
                a.set_budget(ArenaBudget::new(before + 128 + 10, 0));
                (old, before)
            })
            .unwrap();
        assert!(matches!(
            program.variable(7),
            Err(SexpError::AllocationFailed {
                object: "derivative temporary name"
            })
        ));
        assert_eq!(
            access
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            (before, 128)
        );
        access.with_arena(|a| a.set_budget(budget)).unwrap();
        assert_eq!(program.variable(7).unwrap().typeof_(), SEXPTYPE::SYMSXP);
        assert_eq!(access.with_arena(|a| a.workspace_bytes()).unwrap(), 128);
    })
    .unwrap();
    drop(held);
}
