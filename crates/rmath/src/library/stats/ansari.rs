/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1999-2024  The R Core Team
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *  Ported from r-source/src/library/stats/src/ansari.c
 *
 *  ansari.c
 *  Compute the exact distribution of the Ansari-Bradley test statistic.
 */

use crate::sexp::{
    ffi::{SEXP, SEXPTYPE},
    owner::OwnerToken,
};
mod memo;
use memo::MemoTable;
fn active(owner: OwnerToken<'_>) {
    owner
        .require_active()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
}
unsafe fn distribution(input: SEXP, sm: SEXP, sn: SEXP, quantile: bool) -> SEXP {
    let owner = unsafe { OwnerToken::current() }
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let m = unsafe { crate::main::coerce::asInteger(sm) };
    active(owner);
    let n = unsafe { crate::main::coerce::asInteger(sn) };
    active(owner);
    memo::Dimensions::new(m, n).unwrap_or_else(|e| crate::mainutils::essentials::base_error(e));
    let input = unsafe { crate::main::coerce::coerceVector(input, SEXPTYPE::REALSXP.as_c_int()) };
    active(owner);
    let _input = unsafe { crate::sexp::protect::protect(input) };
    let input = owner
        .sexp(input)
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    crate::sexp::altrep::force_materialization(&input)
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    active(owner);
    let len = input.len();
    let output = unsafe { crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::REALSXP, len) };
    let _output = unsafe { crate::sexp::protect::protect(output) };
    let mut result = crate::sexp::SexpMut::try_from_checked(
        owner
            .sexp(output)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string())),
    )
    .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let mut memo =
        MemoTable::new(owner, m, n).unwrap_or_else(|e| crate::mainutils::essentials::base_error(e));
    for i in 0..len {
        let value = input
            .try_real_elt(i)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
        active(owner);
        let value = if quantile {
            memo.quantile(owner, value)
        } else {
            memo.cdf(owner, value)
        }
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e));
        result
            .try_set_real_elt(i, value)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    }
    active(owner);
    output
}
pub unsafe fn pAnsari(q: SEXP, sm: SEXP, sn: SEXP) -> SEXP {
    unsafe { distribution(q, sm, sn, false) }
}
pub unsafe fn qAnsari(p: SEXP, sm: SEXP, sn: SEXP) -> SEXP {
    unsafe { distribution(p, sm, sn, true) }
}

#[cfg(test)]
mod lower_tail_tests {
    use super::*;

    #[test]
    fn exact_cdf_includes_the_requested_mass_once() {
        // Complete support and rounding neighbors from GNU R r90451.
        let q = [
            5.0,
            6.0,
            7.0,
            8.0,
            9.0,
            10.0,
            11.0,
            12.0,
            13.0,
            14.0,
            15.0,
            16.0,
            17.0,
            18.0,
            19.0,
            6.0 - 5e-8,
            6.0 - 2e-7,
        ];
        let counts = [
            0, 1, 5, 14, 30, 56, 88, 122, 154, 180, 196, 205, 209, 210, 210, 1, 0,
        ];
        let session = crate::sexp::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut memo = MemoTable::new(owner, 4, 6).unwrap();
        let p = q.map(|q| memo.cdf(owner, q).unwrap());
        for (actual, count) in p.into_iter().zip(counts) {
            assert!((actual - f64::from(count) / 210.0).abs() < 1e-14);
        }
    }
}

#[cfg(test)]
mod native_contract_tests {
    use super::*;
    use crate::sexp::{RSession, SexpMut, memory::ArenaBudget};
    fn real(session: &RSession, x: f64) -> crate::sexp::Sexp<'static> {
        let owner = session.owner_token().unwrap();
        let value = owner
            .node_factory()
            .allocate(|a| {
                a.alloc_vector_sexp(SEXPTYPE::REALSXP, 1)
                    .map(|s| s.as_raw())
            })
            .unwrap()
            .into_owned()
            .unwrap();
        let mut value = SexpMut::try_from_checked(value).unwrap();
        value.try_set_real_elt(0, x).unwrap();
        value.freeze()
    }
    #[test]
    fn registered_cdf_and_quantile_denial_recover_and_survive_gc() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let m = real(&session, 4.0);
        let n = real(&session, 6.0);
        for (name, input, expected) in [("pAnsari", 6.0, 1.0 / 210.0), ("qAnsari", 0.5, 12.0)] {
            let input = real(&session, input);
            let invoke = || {
                session.with_active(|| {
                    let routine = crate::library::stats::random::lookup_call(name).unwrap();
                    let raw = unsafe {
                        routine
                            .invoke_call(&[input.as_raw(), m.as_raw(), n.as_raw()])
                            .unwrap()
                    };
                    owner.sexp(raw).unwrap().into_owned().unwrap()
                })
            };
            drop(invoke());
            let allocated = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
            owner
                .with_arena(|a| a.set_budget(ArenaBudget::new(allocated + 512, 0)))
                .unwrap();
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(invoke))
                .expect_err("memo must be admitted");
            assert!(
                failure
                    .downcast_ref::<crate::sexp::context::RError>()
                    .is_some_and(|e| e.message.contains("Ansari workspace"))
            );
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            owner
                .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
                .unwrap();
            let result = invoke();
            assert!((result.try_real_elt(0).unwrap() - expected).abs() < 1e-14);
            drop(input);
            session.with_active(|| unsafe { crate::sexp::gengc::full_gc() });
            assert!((result.try_real_elt(0).unwrap() - expected).abs() < 1e-14);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        }
    }
    struct Provider {
        facade: std::rc::Rc<std::cell::RefCell<Option<RSession>>>,
        calls: std::rc::Rc<std::cell::Cell<usize>>,
    }
    impl crate::sexp::altrep::AltrepClass for Provider {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::REALSXP
        }
        fn length(
            &self,
            _: &crate::sexp::altrep::AltrepContext<'_>,
        ) -> crate::sexp::SexpResult<i64> {
            Ok(2)
        }
        fn element<'s>(
            &self,
            context: &crate::sexp::altrep::AltrepContext<'s>,
            i: i64,
        ) -> crate::sexp::SexpResult<crate::sexp::altrep::AltrepElement<'s>> {
            self.calls.set(self.calls.get() + 1);
            context.gc()?;
            if let Some(session) = self.facade.borrow_mut().as_mut() {
                session.close();
            }
            Ok(crate::sexp::altrep::AltrepElement::Real(
                [6.0, 7.0][i as usize],
            ))
        }
    }
    #[test]
    fn registered_altrep_gc_and_owner_revocation_are_checked() {
        for revoke in [false, true] {
            let session = RSession::new_for_gc_tests();
            let owner = session.owner_token().unwrap();
            let weak = owner.weak_owner().unwrap();
            let facade = std::rc::Rc::new(std::cell::RefCell::new(None));
            let calls = std::rc::Rc::new(std::cell::Cell::new(0));
            let class = session
                .register_altrep_class(
                    "ansari.checked",
                    Provider {
                        facade: facade.clone(),
                        calls: calls.clone(),
                    },
                )
                .unwrap();
            let input = crate::sexp::altrep::AltrepBuilder::new(class)
                .build()
                .unwrap()
                .into_owned()
                .unwrap();
            let m = real(&session, 4.0);
            let n = real(&session, 6.0);
            if revoke {
                *facade.borrow_mut() = Some(session);
            }
            let published = std::cell::Cell::new(false);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::sexp::owner::with_runtime(&weak, |access| {
                    access.with_native(|owner| {
                        let routine =
                            crate::library::stats::random::lookup_call("pAnsari").unwrap();
                        let raw = unsafe {
                            routine
                                .invoke_call(&[input.as_raw(), m.as_raw(), n.as_raw()])
                                .unwrap()
                        };
                        published.set(true);
                        owner.sexp(raw)?.into_owned()
                    })
                })
            }));
            assert!(calls.get() > 0);
            if revoke {
                assert!(!published.get());
                match result {
                    Err(_) => {}
                    Ok(value) => assert!(value.is_err()),
                }
                assert!(input.try_real_elt(0).is_err());
                drop(facade.borrow_mut().take());
            } else {
                let result = result.unwrap().unwrap().unwrap();
                assert!((result.try_real_elt(0).unwrap() - 1.0 / 210.0).abs() < 1e-14);
                assert!((result.try_real_elt(1).unwrap() - 5.0 / 210.0).abs() < 1e-14);
            }
        }
    }
}
