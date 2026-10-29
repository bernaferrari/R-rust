//! Registered signed-rank counts: one latest-n table in charged R storage.
use super::*;
use crate::sexp::{Sexp, SexpMut, owner::OwnerToken};

pub(crate) struct Cache {
    n: i32,
    values: Sexp<'static>,
}
pub(super) enum Kind {
    Density,
    Cdf,
    Quantile,
}
fn active(owner: OwnerToken<'_>) {
    owner
        .require_active()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
}
fn dimensions(n: i32) -> Result<(usize, usize), String> {
    if n < 1 {
        return Err("invalid signed-rank sample size".into());
    }
    let total = i64::from(n)
        .checked_mul(i64::from(n) + 1)
        .and_then(|v| v.checked_div(2))
        .filter(|&v| v <= i64::from(i32::MAX))
        .ok_or_else(|| "signed-rank sample size is too large".to_string())?;
    let half =
        usize::try_from(total / 2).map_err(|_| "signed-rank table is too large".to_string())?;
    let length = half
        .checked_add(1)
        .filter(|&n| {
            n.checked_mul(size_of::<f64>())
                .is_some_and(|b| b <= isize::MAX as usize)
        })
        .ok_or_else(|| "signed-rank table is too large".to_string())?;
    Ok((total as usize, length))
}
fn cached(owner: OwnerToken<'_>, n: i32) -> Option<Sexp<'static>> {
    active(owner);
    let _pin = owner
        .pin()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    // Short original-owner field access ends before any allocation or callback.
    unsafe {
        (*owner.as_ptr())
            .signed_rank_cache
            .as_ref()
            .filter(|entry| entry.n == n)
            .map(|entry| entry.values.clone())
    }
}
fn table(owner: OwnerToken<'_>, n: i32) -> Sexp<'static> {
    let (_, length) = dimensions(n).unwrap_or_else(|e| crate::mainutils::essentials::base_error(e));
    if let Some(table) = cached(owner, n) {
        return table;
    }
    let value = owner
        .node_factory()
        .allocate(|a| {
            a.alloc_vector_sexp(SEXPTYPE::REALSXP, length as i64)
                .map(|v| v.as_raw())
        })
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()))
        .into_owned()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    active(owner);
    {
        // Fresh private dense storage; no R calls, arena lends or callbacks occur
        // during this bounded arithmetic loan. Publish only the completed table.
        let values = unsafe { std::slice::from_raw_parts_mut(REAL(value.as_raw()), length) };
        values.fill(0.0);
        values[0] = 1.0;
        if length > 1 {
            values[1] = 1.0;
        }
        let half = length - 1;
        for j in 2..=n as usize {
            let end = (j * (j + 1) / 2).min(half);
            for i in (j..=end).rev() {
                values[i] += values[i - j];
            }
        }
    }
    active(owner);
    let _pin = owner
        .pin()
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let replacement = Cache {
        n,
        values: value.clone(),
    };
    // The replaced root drops after this field borrow ends; its physical storage
    // remains charged until the arena reclaims it. Old/new allocation overlaps.
    let old =
        unsafe { std::mem::replace(&mut (*owner.as_ptr()).signed_rank_cache, Some(replacement)) };
    drop(old);
    active(owner);
    value
}
struct Counts {
    n: Option<i32>,
    total: usize,
    pointer: *const f64,
    // Keeps the immutable published dense buffer alive during pure arithmetic.
    values: Option<Sexp<'static>>,
    bounded_search: bool,
}
impl Counts {
    fn new(bounded_search: bool) -> Self {
        Self {
            n: None,
            total: 0,
            pointer: std::ptr::null(),
            values: None,
            bounded_search,
        }
    }
    fn get(&mut self, owner: OwnerToken<'_>, k: i32, n: i32) -> f64 {
        if self.n != Some(n) {
            let (total, _) =
                dimensions(n).unwrap_or_else(|e| crate::mainutils::essentials::base_error(e));
            let values = table(owner, n);
            active(owner);
            self.pointer = unsafe { REAL(values.as_raw()) };
            self.total = total;
            self.values = Some(values);
            self.n = Some(n);
        }
        if k < 0 || k as usize > self.total {
            if self.bounded_search {
                crate::mainutils::essentials::base_error("signed-rank quantile did not converge");
            }
            return 0.0;
        }
        let k = (k as usize).min(self.total - k as usize);
        // Once the table is acquired, the pure nmath scalar algorithm calls
        // only this copied read: no R callback or allocation crosses the loan.
        unsafe { *self.pointer.add(k) }
    }
}
pub(super) unsafe fn evaluate(sa: SEXP, sb: SEXP, sI: SEXP, sJ: SEXP, kind: Kind) -> SEXP {
    let owner = unsafe { OwnerToken::current() }
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    active(owner);
    if !unsafe { isNumeric(owner, sa) } || !unsafe { isNumeric(owner, sb) } {
        crate::mainutils::essentials::base_error("Non-numeric argument to mathematical function");
    }
    let na = unsafe { XLENGTH(sa) };
    let nb = unsafe { XLENGTH(sb) };
    let n = if na == 0 || nb == 0 { 0 } else { na.max(nb) };
    let result = unsafe { Rf_allocVector3(SEXPTYPE::REALSXP, n) };
    active(owner);
    let _result = unsafe { protect(result) };
    active(owner);
    if n == 0 {
        if na == 0 {
            unsafe { copy_math_attributes(owner, result, sa) }
        }
        return result;
    }
    let sa = unsafe { coerceVector(sa, SEXPTYPE::REALSXP.as_c_int()) };
    active(owner);
    let _sa = unsafe { protect(sa) };
    active(owner);
    let sb = unsafe { coerceVector(sb, SEXPTYPE::REALSXP.as_c_int()) };
    active(owner);
    let _sb = unsafe { protect(sb) };
    active(owner);
    let inputs = [(sa, na), (sb, nb)];
    prepare_inputs(owner, &inputs);
    let (lower, log) = match kind {
        Kind::Density => (true, unsafe { as_integer(owner, sI) } != 0),
        _ => (
            unsafe { as_integer(owner, sI) } != 0,
            unsafe { as_integer(owner, sJ) } != 0,
        ),
    };
    check_inputs(owner, &inputs);
    let a = owner
        .sexp(sa)
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let b = owner
        .sexp(sb)
        .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let mut output = SexpMut::try_from_checked(
        owner
            .sexp(result)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string())),
    )
    .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    let mut warn = false;
    for i in 0..n {
        let x = a
            .try_real_elt(i % na)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
        let size = b
            .try_real_elt(i % nb)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
        active(owner);
        let value = if R_IsNA(x) || R_IsNA(size) {
            NA_REAL
        } else if x.is_nan() || size.is_nan() {
            f64::NAN
        } else {
            let mut counts = Counts::new(matches!(kind, Kind::Quantile));
            let value = match kind {
                Kind::Density => {
                    crate::nmath::dist::signrank::dsignrank_with_count(x, size, log, |k, n| {
                        counts.get(owner, k, n)
                    })
                }
                Kind::Cdf => crate::nmath::dist::signrank::psignrank_with_count(
                    x,
                    size,
                    lower,
                    log,
                    |k, n| counts.get(owner, k, n),
                ),
                Kind::Quantile => crate::nmath::dist::signrank::qsignrank_with_count(
                    x,
                    size,
                    lower,
                    log,
                    |k, n| counts.get(owner, k, n),
                ),
            };
            if value.is_nan() {
                warn = true;
            }
            value
        };
        active(owner);
        output
            .try_set_real_elt(i, value)
            .unwrap_or_else(|e| crate::mainutils::essentials::base_error(e.to_string()));
    }
    if warn {
        unsafe { nans_produced() };
    }
    active(owner);
    unsafe { finish_math_attributes(owner, result, &inputs, n) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::RSession;
    #[test]
    fn checked_dimensions_cache_replacement_and_owner_locality() {
        assert!(dimensions(0).is_err());
        assert!(dimensions(-1).is_err());
        assert!(dimensions(65536).is_err());
        assert_eq!(dimensions(65535).is_ok(), usize::BITS == 64);
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let old = session.with_active(|| table(owner, 10));
        let old_node = old.allocation().unwrap().clone();
        assert_eq!(old.len(), 28);
        let expected = session.with_active(|| table(owner, 10));
        assert_eq!(old, expected);
        drop(expected);
        let next = session.with_active(|| table(owner, 40));
        assert_eq!(next.len(), 411);
        assert_ne!(old, next);
        assert!(old_node.is_live());
        drop(old);
        session.with_active(|| unsafe { crate::sexp::gengc::full_gc() });
        assert!(!old_node.is_live());
        assert_eq!(next.try_real_elt(0), Ok(1.0));
        unsafe {
            assert!((*owner.as_ptr()).math_state.signrank_cache.is_empty());
        }
        let other = RSession::new_for_gc_tests();
        let other_owner = other.owner_token().unwrap();
        let second = other.with_active(|| table(other_owner, 40));
        assert_ne!(next, second);
        drop(session);
        assert_eq!(next.try_real_elt(0), Ok(1.0));
        let before = other_owner
            .with_arena(|a| a.total_bytes_allocated())
            .unwrap();
        drop(next);
        assert_eq!(
            other_owner
                .with_arena(|a| a.total_bytes_allocated())
                .unwrap(),
            before
        );
    }
    #[test]
    fn allocation_gc_reentry_publishes_only_complete_tables() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let called = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = called.clone();
        session.with_active(|| {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed.replace(true) {
                    return;
                }
                let owner = unsafe { OwnerToken::current().unwrap() };
                unsafe {
                    (*owner.as_ptr()).memory_state.gc_force_gap = 0;
                }
                assert!(cached(owner, 10).is_none());
                let nested = table(owner, 5);
                assert_eq!(nested.try_real_elt(0), Ok(1.0));
                crate::sexp::gengc::full_gc();
            }))
        });
        session.with_active_in(|owner| unsafe {
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
        });
        let result = session.with_active(|| table(owner, 10));
        assert!(called.get());
        assert_eq!(result.try_real_elt(5), Ok(3.0));
        assert_eq!(cached(owner, 10).unwrap(), result);
    }
    #[test]
    fn allocation_revocation_prevents_cache_publication() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        let facade = std::rc::Rc::new(std::cell::RefCell::new(None));
        let closer = facade.clone();
        let called = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = called.clone();
        session.with_active(|| {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed.replace(true) {
                    return;
                }
                unsafe {
                    (*OwnerToken::current().unwrap().as_ptr())
                        .memory_state
                        .gc_force_gap = 0;
                }
                closer
                    .borrow_mut()
                    .as_mut()
                    .map(|session: &mut RSession| session.close());
            }))
        });
        session.with_active_in(|owner| unsafe {
            (*owner).memory_state.gc_force_gap = 1;
            (*owner).memory_state.gc_force_wait = 1;
        });
        *facade.borrow_mut() = Some(session);
        let published = std::cell::Cell::new(false);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::sexp::owner::with_runtime(&weak, |access| {
                access.with_native(|owner| {
                    let table = table(owner, 10);
                    published.set(true);
                    Ok(table)
                })
            })
        }));
        assert!(called.get());
        assert!(!published.get());
        match result {
            Err(_) => {}
            Ok(value) => assert!(value.is_err()),
        };
        drop(facade.borrow_mut().take());
    }
    #[test]
    fn denied_replacement_keeps_the_live_table_and_retry_recovers() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let old = session.with_active(|| table(owner, 10));
        let old_node = old.allocation().unwrap().clone();
        let allocated = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(crate::sexp::memory::ArenaBudget::new(allocated + 512, 0)))
            .unwrap();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.with_active(|| table(owner, 40))
        }));
        assert!(failure.is_err());
        assert_eq!(cached(owner, 10).unwrap(), old);
        assert!(old_node.is_live());
        owner
            .with_arena(|a| a.set_budget(crate::sexp::memory::ArenaBudget::unlimited()))
            .unwrap();
        let replacement = session.with_active(|| table(owner, 40));
        assert_eq!(replacement.len(), 411);
        assert_ne!(replacement, old);
    }
}
