#![allow(unsafe_code)]
//! Audited adapter for raw translated R operations. Safe callers pass rooted
//! handles; raw callers must uphold the documented owner and liveness contract.
use super::*;

/// Clone an owned Rust lease, rather than returning an interpreter reference.
/// All subsequent registry reads, writes and guard cleanup use safe Rust.
pub(super) fn runtime(owner: OwnerToken<'_>) -> AltrepRuntimeState {
    // SAFETY: the retained owner is live. Borrow only this field long enough to
    // clone its Rc; no callback, GC or mutable interpreter borrow overlaps.
    unsafe { (*owner.as_ptr()).altrep_state.clone() }
}

pub(super) fn eval<'s>(
    owner: OwnerToken<'s>,
    expression: Sexp<'s>,
    environment: Sexp<'s>,
) -> SexpResult<Sexp<'s>> {
    let expression = owner.sexp(expression.as_raw())?;
    let environment = owner.sexp(environment.as_raw())?;
    if environment.typeof_() != SEXPTYPE::ENVSXP {
        return Err(SexpError::TypeMismatch {
            expected: "environment",
            actual: environment.typeof_(),
        });
    }
    // SAFETY: both inputs stay rooted; no interpreter or payload reference
    // survives R execution. Restore the original owner after all callbacks.
    let result = storage::activate(owner, || unsafe {
        crate::eval::eval::eval(expression, environment)
    });
    owner.sexp(
        result
            .map_err(|message| SexpError::EvaluationFailed { message })?
            .as_raw(),
    )
}

pub(super) fn compact_integer_sequence<'s>(
    owner: OwnerToken<'s>,
    origin: i32,
    step: i32,
    length: usize,
) -> SexpResult<Sexp<'s>> {
    builtins::new_sequence(
        owner,
        SEXPTYPE::INTSXP,
        origin as f64,
        step as f64,
        i64::try_from(length).map_err(|_| failure("sequence length"))?,
    )
}
pub(super) fn compact_real_sequence<'s>(
    owner: OwnerToken<'s>,
    origin: f64,
    step: f64,
    length: usize,
) -> SexpResult<Sexp<'s>> {
    builtins::new_sequence(
        owner,
        SEXPTYPE::REALSXP,
        origin,
        step,
        i64::try_from(length).map_err(|_| failure("sequence length"))?,
    )
}

/// Probe copied headers without installing roots or borrowing an owner. Raw
/// callers already retain this graph, and the probe cannot allocate or collect.
/// # Safety
/// `raw` and its reachable metadata must be live throughout this nonallocating read.
pub(crate) unsafe fn has_extension_raw(raw: SEXP) -> bool {
    let Some(view) = (unsafe { Sexp::from_raw(raw) }) else {
        return false;
    };
    Metadata::load(&view).is_some()
}

/// # Safety
/// Raw translated callers retain the active owner and exclude payload loans.
pub(crate) unsafe fn lazy_raw<'s>(
    raw: SEXP,
    index: R_xlen_t,
) -> Option<SexpResult<AltrepElement<'s>>> {
    let view = unsafe { Sexp::from_raw(raw) }?;
    if !view.header().payload.is_empty() || Metadata::load(&view).is_none() {
        return None;
    }
    let object = match unsafe { rooted_raw(raw) } {
        Ok(object) => object,
        Err(error) => return Some(Err(error)),
    };
    let value = lazy_element(&object, index)?;
    Some(value.and_then(|value| {
        if let AltrepElement::String(child) | AltrepElement::List(child) = &value {
            retain_native_child(&object, index, child)?;
        }
        Ok(value)
    }))
}

/// Raw runtime adapter: validate ownership and retain a lease before callbacks.
/// # Safety
/// `raw` must name a live node in the active session; no payload loan may overlap.
pub(crate) unsafe fn rooted_raw<'a>(raw: SEXP) -> SexpResult<Sexp<'a>> {
    let current =
        super::super::instance::current_instance_ptr().ok_or(SexpError::OwnerNotActive)?;
    if super::super::memory::is_arena_lent(current) {
        return Err(failure("release the arena lend before class dispatch"));
    }
    // This unsafe boundary relies on the caller retaining the owner lifetime.
    unsafe { OwnerToken::from_raw(current) }.sexp(raw)
}

/// Called by raw and checked bulk accessors. Returns false for compact sequences.
/// # Safety
/// Same owner and loan requirements as `rooted_raw`.
pub(crate) unsafe fn materialize_raw(raw: SEXP) -> SexpResult<bool> {
    if !unsafe { has_extension_raw(raw) } {
        return Ok(false);
    }
    let current =
        super::super::instance::current_instance_ptr().ok_or(SexpError::OwnerNotActive)?;
    let object = unsafe { OwnerToken::from_raw(current) }.sexp(raw)?;
    if builtins::materialize_sequence(&object)? {
        return Ok(true);
    }
    let object = unsafe { rooted_raw(raw) }?;
    force_materialization(&object)?;
    Ok(true)
}
