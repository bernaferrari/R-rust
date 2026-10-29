use super::*;
use crate::sexp::{RSession, Sexp, SexpMut, memory::ArenaBudget};
fn reals(session: &RSession, value: f64) -> Sexp<'static> {
    let owner = session.owner_token().unwrap();
    let value_object = owner
        .node_factory()
        .allocate(|a| {
            a.alloc_vector_sexp(SEXPTYPE::REALSXP, 1)
                .map(|v| v.as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let mut object = SexpMut::try_from_checked(value_object).unwrap();
    object.try_set_real_elt(0, value).unwrap();
    object.freeze()
}
#[test]
fn registered_signed_rank_cache_admission_denial_recovers() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let n10 = reals(&session, 10.0);
    let n40 = reals(&session, 40.0);
    let zero = reals(&session, 0.0);
    let one = reals(&session, 1.0);
    let five = reals(&session, 5.0);
    let invoke = |name: &str, args: &[SEXP]| {
        session.with_active(|| {
            let routine = lookup_call(name).unwrap();
            let raw = unsafe { routine.invoke_call(args).unwrap() };
            owner.sexp(raw).unwrap().into_owned().unwrap()
        })
    };
    for (name, input, expected) in [
        ("dsignrank", 5.0, 3.0 / 2.0_f64.powi(40)),
        ("psignrank", 5.0, 10.0 / 2.0_f64.powi(40)),
        ("qsignrank", 0.5, 410.0),
    ] {
        drop(invoke(
            "dsignrank",
            &[five.as_raw(), n10.as_raw(), zero.as_raw()],
        ));
        session.with_active(|| unsafe { crate::sexp::gengc::full_gc() });
        let input = reals(&session, input);
        let args = if name == "dsignrank" {
            vec![input.as_raw(), n40.as_raw(), zero.as_raw()]
        } else {
            vec![input.as_raw(), n40.as_raw(), one.as_raw(), zero.as_raw()]
        };
        let allocated = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(allocated + 512, 0)))
            .unwrap();
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(name, &args)))
            .expect_err("count storage must be admitted");
        assert!(
            error
                .downcast_ref::<crate::sexp::context::RError>()
                .is_some()
        );
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        let old = invoke("dsignrank", &[five.as_raw(), n10.as_raw(), zero.as_raw()]);
        assert!((old.try_real_elt(0).unwrap() - 3.0 / 1024.0).abs() < 1e-14);
        drop(old);
        let result = invoke(name, &args);
        assert!((result.try_real_elt(0).unwrap() - expected).abs() < 1e-14);
        drop(input);
        session.with_active(|| unsafe { crate::sexp::gengc::full_gc() });
        assert!((result.try_real_elt(0).unwrap() - expected).abs() < 1e-14);
    }
}
