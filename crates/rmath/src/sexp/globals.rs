#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Global singleton R objects.
//!
//! These are immutable thread-confined sentinel values used throughout R. Mutable runtime environments are session-owned and reached
//! through the active `RInstance`, not through process-global fallback slots.

use super::ffi::SEXP;
#[path = "singletons.rs"]
mod singletons;
use super::instance::{RInstance, with_required_current_instance};
pub(crate) use singletons::{SingletonKind, SingletonLease, SingletonPoolLease};

// ---------------------------------------------------------------------------
// Immutable sentinel projections. Rc leases own their stable Rust cells.
// ---------------------------------------------------------------------------

pub(crate) fn immutable_singleton_projection(pointer: SEXP) -> Option<SEXP> {
    singletons::canonical_projection(pointer)
}

pub(crate) fn immutable_singleton_lease(pointer: SEXP) -> Option<SingletonLease> {
    singletons::lease(pointer)
}

pub(crate) fn immutable_singleton_snapshot(pointer: SEXP) -> Option<super::ffi::SexprecCore> {
    singletons::snapshot(pointer)
}

/// Inspect the existing bank without creating TLS state during a read.
pub(crate) fn immutable_na_string_projection() -> Option<SEXP> {
    singletons::current_na_string_projection()
}

pub(crate) fn immutable_singleton_pool() -> SingletonPoolLease {
    singletons::pool()
}

#[cfg(test)]
pub(crate) fn close_immutable_singletons_for_test() {
    singletons::close_pool_for_test();
}

/// Get a pointer to R_NilValue.
pub unsafe fn R_NilValue() -> SEXP {
    singletons::nil()
}
/// Get a pointer to R_UnboundValue.
pub unsafe fn R_UnboundValue() -> SEXP {
    singletons::unbound()
}
/// Get a pointer to R_MissingArg.
pub unsafe fn R_MissingArg() -> SEXP {
    singletons::missing()
}
/// Get a pointer to R_RestartToken.
pub unsafe fn R_RestartToken() -> SEXP {
    singletons::restart()
}

// ---------------------------------------------------------------------------
// Global environment accessor functions
// ---------------------------------------------------------------------------

pub unsafe fn R_GlobalEnv() -> SEXP {
    with_required_current_instance(R_GlobalEnv_in)
}

pub unsafe fn R_BaseEnv() -> SEXP {
    with_required_current_instance(R_BaseEnv_in)
}

pub unsafe fn R_EmptyEnv() -> SEXP {
    with_required_current_instance(R_EmptyEnv_in)
}

pub(crate) fn R_GlobalEnv_in(inst: *mut RInstance) -> SEXP {
    // P2: single-field read; no ambient write intervenes.
    unsafe { (*inst).global_env }
}

pub(crate) fn R_BaseEnv_in(inst: *mut RInstance) -> SEXP {
    // P2: single-field read; no ambient write intervenes.
    unsafe { (*inst).base_env }
}

pub(crate) fn R_EmptyEnv_in(inst: *mut RInstance) -> SEXP {
    // P2: single-field read; no ambient write intervenes.
    unsafe { (*inst).empty_env }
}

/// Set the global environment.
pub unsafe fn set_R_GlobalEnv(env: SEXP) {
    with_required_current_instance(|inst| set_R_GlobalEnv_in(inst, env));
}

/// Set the base environment.
pub unsafe fn set_R_BaseEnv(env: SEXP) {
    with_required_current_instance(|inst| set_R_BaseEnv_in(inst, env));
}

/// Set the empty environment.
pub unsafe fn set_R_EmptyEnv(env: SEXP) {
    with_required_current_instance(|inst| set_R_EmptyEnv_in(inst, env));
}

pub(crate) fn set_R_GlobalEnv_in(inst: *mut RInstance, env: SEXP) {
    // P2: single-field write; no other raw path touches the instance here.
    unsafe {
        (*inst).global_env = env;
    }
}

pub(crate) fn set_R_BaseEnv_in(inst: *mut RInstance, env: SEXP) {
    // P2: single-field write; no other raw path touches the instance here.
    unsafe {
        (*inst).base_env = env;
    }
}

pub(crate) fn set_R_EmptyEnv_in(inst: *mut RInstance, env: SEXP) {
    // P2: single-field write; no other raw path touches the instance here.
    unsafe {
        (*inst).empty_env = env;
    }
}

// ---------------------------------------------------------------------------
// NA helpers
// ---------------------------------------------------------------------------

/// Check if a logical value is NA.
#[inline]
pub fn LOGICAL_IS_NA(x: i32) -> bool {
    x == super::ffi::NA_INTEGER
}

/// Check if an integer value is NA.
#[inline]
pub fn INTEGER_IS_NA(x: i32) -> bool {
    x == super::ffi::NA_INTEGER
}

// ---------------------------------------------------------------------------
// Evaluator globals
// ---------------------------------------------------------------------------

/// Get the current R_Visible flag.
pub fn R_Visible() -> i32 {
    with_required_current_instance(R_Visible_in)
}

/// Set the R_Visible flag.
pub fn set_R_Visible(v: i32) {
    with_required_current_instance(|inst| set_R_Visible_in(inst, v));
}

/// Get the current evaluation depth.
pub fn R_EvalDepth() -> i32 {
    with_required_current_instance(R_EvalDepth_in)
}

/// Set the evaluation depth.
pub fn set_R_EvalDepth(d: i32) {
    with_required_current_instance(|inst| set_R_EvalDepth_in(inst, d));
}

/// Get the evaluation depth limit.
pub fn R_EvalDepthLimit() -> i32 {
    with_required_current_instance(R_EvalDepthLimit_in)
}

pub(crate) fn R_Visible_in(inst: *mut RInstance) -> i32 {
    // P2: single-field read; no ambient write intervenes.
    unsafe { (*inst).eval_state.visible }
}

pub(crate) fn set_R_Visible_in(inst: *mut RInstance, v: i32) {
    // P2: single-field write; no other raw path touches the instance here.
    unsafe {
        (*inst).eval_state.visible = v;
    }
}

pub(crate) fn R_EvalDepth_in(inst: *mut RInstance) -> i32 {
    // P2: single-field read; no ambient write intervenes.
    unsafe { (*inst).eval_state.eval_depth }
}

pub(crate) fn set_R_EvalDepth_in(inst: *mut RInstance, d: i32) {
    // P2: single-field write; no other raw path touches the instance here.
    unsafe {
        (*inst).eval_state.eval_depth = d;
    }
}

pub(crate) fn R_EvalDepthLimit_in(inst: *mut RInstance) -> i32 {
    // P2: single-field read; no ambient write intervenes.
    unsafe { (*inst).eval_state.eval_depth_limit }
}

// ---------------------------------------------------------------------------
// Common symbols (re-exports from symbol.rs for convenience)
// ---------------------------------------------------------------------------

/// Get R_DotsSymbol (the "..." symbol).
pub unsafe fn R_DotsSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_DotsSymbol() }
}

/// Get R_IfSymbol (the "if" symbol).
pub unsafe fn R_IfSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_IfSymbol() }
}

/// Get R_WhileSymbol (the "while" symbol).
pub unsafe fn R_WhileSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_WhileSymbol() }
}

/// Get R_ForSymbol (the "for" symbol).
pub unsafe fn R_ForSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_ForSymbol() }
}

/// Get R_RepeatSymbol (the "repeat" symbol).
pub unsafe fn R_RepeatSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_RepeatSymbol() }
}

/// Get R_BraceSymbol (the "{" symbol).
pub unsafe fn R_BraceSymbol_fn() -> SEXP {
    unsafe { super::symbol::R_BraceSymbol() }
}

// ---------------------------------------------------------------------------
// R_True and R_False logical singletons
// ---------------------------------------------------------------------------

/// Get a pointer to the immutable logical TRUE scalar.
pub unsafe fn R_True() -> SEXP {
    singletons::logical(true)
}
/// Get a pointer to the immutable logical FALSE scalar.
pub unsafe fn R_False() -> SEXP {
    singletons::logical(false)
}
/// Get a pointer to the immutable NA_STRING character sentinel.
pub unsafe fn R_NaString() -> SEXP {
    singletons::na_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::ffi::*;
    use super::super::instance::{RInstance, current_instance_ptr, replace_current_instance};
    use super::*;

    #[test]
    fn singleton_handles_retain_headers_and_payloads_after_pool_close() {
        let nil = crate::sexp::object::Sexp::nil();
        let logical = unsafe { crate::sexp::object::Sexp::from_static_raw_unchecked(R_True()) };
        let pointer = logical.clone().as_raw();
        let pool = immutable_singleton_pool();
        let address_only = std::ptr::without_provenance_mut(pointer.addr());
        assert_eq!(pool.canonical_projection(address_only), Some(pointer));
        singletons::close_pool_for_test();
        assert!(immutable_singleton_projection(pointer).is_none());
        assert!(nil.is_nil());
        assert_eq!(logical.logical_elt(0), Some(TRUE));
        assert_eq!(logical.clone().as_raw(), pointer);
    }

    #[test]
    fn saved_singleton_links_keep_each_original_bank_identity() {
        let old = unsafe { crate::sexp::object::Sexp::from_static_raw_unchecked(R_True()) };
        let old_lease = immutable_singleton_lease(old.as_raw()).unwrap();
        singletons::close_pool_for_test();
        let new = unsafe { crate::sexp::object::Sexp::from_static_raw_unchecked(R_True()) };
        let new_lease = immutable_singleton_lease(new.as_raw()).unwrap();
        assert_eq!(old_lease.kind(), new_lease.kind());
        assert_ne!(old_lease.identity_cookie(), new_lease.identity_cookie());
        let mut arena = crate::sexp::memory::RArena::new();
        let vector_ptr = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
        let vector = arena.sexp(vector_ptr).unwrap();
        let mut mutation = crate::sexp::object::SexpMut::try_from_checked(vector).unwrap();
        mutation.try_set_vector_elt(0, old.clone()).unwrap();
        mutation.try_set_vector_elt(1, new.clone()).unwrap();
        let vector = mutation.freeze();
        drop(old_lease);
        drop(new_lease);
        drop(old);
        drop(new);
        singletons::close_pool_for_test();
        let first = vector.vector_elt(0).unwrap();
        let second = vector.vector_elt(1).unwrap();
        assert_ne!(first.as_raw(), second.as_raw());
        assert_eq!(first.logical_elt(0), Some(TRUE));
        assert_eq!(second.logical_elt(0), Some(TRUE));
    }

    #[test]
    fn checked_graph_children_retain_the_original_singleton_bank() {
        let nil = unsafe { R_NilValue() };
        let logical = unsafe { R_True() };
        let mut arena = crate::sexp::memory::RArena::new();
        let node = arena.alloc_node(SEXPTYPE::LISTSXP);
        unsafe {
            crate::sexp::accessors::SETCAR(node, logical);
            crate::sexp::accessors::SETCDR(node, nil);
        }
        let parent = crate::sexp::object::Sexp::from_arena_raw(node, &arena).unwrap();
        singletons::close_pool_for_test();
        let child = parent.car().expect("retained static child");
        let tail = parent.cdr().expect("retained nil child");
        drop(parent);
        assert_eq!(child.logical_elt(0), Some(TRUE));
        assert!(tail.is_nil());
        assert_eq!(tail.as_raw(), nil);
    }

    #[test]
    fn string_values_keep_na_identity_after_pool_close() {
        let mut arena = crate::sexp::memory::RArena::new();
        let node = arena.alloc_vector(SEXPTYPE::STRSXP, 2);
        let present = arena.alloc_charsxp(b"NA");
        let na = unsafe { R_NaString() };
        unsafe {
            super::super::accessors::SET_STRING_ELT(node, 0, na);
            super::super::accessors::SET_STRING_ELT(node, 1, present);
        }
        let strings = crate::sexp::object::Sexp::from_arena_raw(node, &arena).unwrap();
        singletons::close_pool_for_test();
        assert!(strings.try_string_elt(0).unwrap().is_na_string());
        assert!(!strings.try_string_elt(1).unwrap().is_na_string());
        assert_eq!(strings.try_string_value_elt(0).unwrap(), None);
        assert_eq!(
            strings.try_string_value_elt(1).unwrap(),
            Some("NA".to_owned())
        );
        assert!(immutable_na_string_projection().is_none());
    }

    #[test]
    fn singleton_identity_is_stable_and_confined_to_its_thread() {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let value = crate::sexp::object::Sexp::nil();
                    let pointer = value.clone().as_raw();
                    assert_eq!(unsafe { R_NilValue() }, pointer);
                    assert!(value.is_nil());
                    barrier.wait();
                    pointer.addr()
                })
            })
            .collect();
        let pointers: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_ne!(pointers[0], pointers[1]);
    }

    #[test]
    fn static_handles_read_owned_snapshots_during_tls_teardown() {
        struct EarlierHolder {
            values: Option<(
                crate::sexp::object::Sexp<'static>,
                crate::sexp::object::Sexp<'static>,
                crate::sexp::object::Sexp<'static>,
            )>,
            report: Option<std::sync::Arc<std::sync::Mutex<bool>>>,
        }
        impl Drop for EarlierHolder {
            fn drop(&mut self) {
                if let Some((nil, logical, na)) = self.values.take() {
                    assert!(nil.is_nil());
                    assert_eq!(logical.logical_elt(0), Some(TRUE));
                    assert!(na.is_na_string());
                    *self.report.as_ref().unwrap().lock().unwrap() = true;
                }
            }
        }
        thread_local! {
            static HOLDER: std::cell::RefCell<EarlierHolder> = const {
                std::cell::RefCell::new(EarlierHolder { values: None, report: None })
            };
        }
        let report = std::sync::Arc::new(std::sync::Mutex::new(false));
        let observed = report.clone();
        std::thread::spawn(move || {
            // Initialized first, this holder drops after the singleton TLS pool.
            HOLDER.with(|_| ());
            let nil = crate::sexp::object::Sexp::nil();
            let logical = unsafe { crate::sexp::object::Sexp::from_static_raw_unchecked(R_True()) };
            let na = unsafe { crate::sexp::object::Sexp::from_static_raw_unchecked(R_NaString()) };
            HOLDER.with(|holder| {
                let mut holder = holder.borrow_mut();
                holder.values = Some((nil, logical, na));
                holder.report = Some(observed);
            });
        })
        .join()
        .unwrap();
        assert!(*report.lock().unwrap());
    }

    #[test]
    fn shared_singletons_remain_immutable() {
        use crate::sexp::accessors::*;
        unsafe {
            for node in [
                R_NilValue(),
                R_UnboundValue(),
                R_MissingArg(),
                R_RestartToken(),
                R_NaString(),
                R_True(),
                R_False(),
            ] {
                let flags = (*node).sxpinfo;
                let attributes = (*node).attrib;
                let data = (*node).gengc_next_node;
                let body = (*node).data;
                let snapshot = immutable_singleton_snapshot(node).unwrap();
                assert_eq!(snapshot.sxpinfo.type_and_flags, flags.type_and_flags);
                assert_eq!(snapshot.attrib, attributes);
                assert_eq!(snapshot.gengc_next_node, data);
                // Safe snapshots agree with the genuine owned header, including
                // the scalar logical's typed vector lengths.
                if flags.type_of() == SEXPTYPE::LGLSXP || flags.type_of() == SEXPTYPE::CHARSXP {
                    assert_eq!(snapshot.data.vector().length, (*node).data.vector().length);
                    assert_eq!(
                        snapshot.data.vector().truelength,
                        (*node).data.vector().truelength
                    );
                }
                SET_NAMED(node, 0);
                SET_OBJECT(node, 1);
                SET_S4_OBJECT(node);
                UNSET_S4_OBJECT(node);
                SETLEVELS(node, 3);
                SET_MISSING(node, 1);
                SET_SCALAR(node, 0);
                SET_ALTREP(node, 1);
                SET_MARK(node, 0);
                mark_charsxp_encoding(node, "UTF-8");
                SET_ATTRIB(node, R_True());
                SET_TRUELENGTH(node, 99);
                SETCAR(node, R_True());
                SETCDR(node, R_True());
                SETTAG(node, R_True());
                SET_PRINTNAME(node, R_True());
                SET_SYMVALUE(node, R_True());
                SET_INTERNAL(node, R_True());
                SET_FORMALS(node, R_True());
                SET_BODY(node, R_True());
                SET_CLOENV(node, R_True());
                SET_PRVALUE(node, R_True());
                SET_PRCODE(node, R_True());
                SET_PRENV(node, R_True());
                SET_PRIMOFFSET(node, 99);
                SET_DATAPTR(node, std::ptr::null_mut());
                SET_LOGICAL_ELT(node, 0, 99);
                SET_INTEGER_ELT(node, 0, 99);
                SET_REAL_ELT(node, 0, 99.0);
                SET_COMPLEX_ELT(node, 0, Rcomplex { r: 99.0, i: 99.0 });
                SET_RAW_ELT(node, 0, 99);
                assert!(
                    std::panic::catch_unwind(|| {
                        SET_STRING_ELT(node, 0, R_NaString());
                    })
                    .is_err()
                );
                assert!(
                    std::panic::catch_unwind(|| {
                        SET_VECTOR_ELT(node, 0, R_True());
                    })
                    .is_err()
                );
                assert_eq!((*node).sxpinfo.type_and_flags, flags.type_and_flags);
                assert_eq!((*node).sxpinfo.rcount, flags.rcount);
                assert_eq!((*node).attrib, attributes);
                assert_eq!((*node).gengc_next_node, data);
                assert_eq!((*node).data, body);
                assert_eq!(NAMED(node), 2);
            }
            assert_eq!(LOGICAL_ELT(R_True(), 0), 1);
            assert_eq!(LOGICAL_ELT(R_False(), 0), 0);
        }
    }

    #[test]
    fn singleton_storage_preserves_provenance() {
        // No session or arena is needed: exercise every thread-owned slot
        // directly, including its logical payload, under strict-provenance Miri.
        unsafe {
            for (value, kind) in [
                (R_NilValue(), SEXPTYPE::NILSXP),
                (R_UnboundValue(), SEXPTYPE::SYMSXP),
                (R_MissingArg(), SEXPTYPE::SYMSXP),
                (R_RestartToken(), SEXPTYPE::SPECIALSXP),
                (R_NaString(), SEXPTYPE::CHARSXP),
                (R_True(), SEXPTYPE::LGLSXP),
                (R_False(), SEXPTYPE::LGLSXP),
            ] {
                assert_eq!((*value).sxpinfo.type_of(), kind);
            }
            assert_eq!(*((*R_True()).gengc_next_node as *const i32), TRUE);
            assert_eq!(*((*R_False()).gengc_next_node as *const i32), FALSE);
            assert_eq!(R_True(), R_True());
            assert_eq!(R_NaString(), R_NaString());
        }
    }

    #[test]
    fn test_r_nilvalue_type() {
        unsafe {
            let nil = R_NilValue();
            assert!(!nil.is_null());
            assert_eq!((*nil).sxpinfo.type_of(), SEXPTYPE::NILSXP);
        }
    }

    #[test]
    fn test_r_nilvalue_stable() {
        unsafe {
            let nil1 = R_NilValue();
            let nil2 = R_NilValue();
            assert_eq!(nil1, nil2);
        }
    }

    #[test]
    fn test_r_unboundvalue_type() {
        unsafe {
            let ub = R_UnboundValue();
            assert!(!ub.is_null());
            assert_eq!((*ub).sxpinfo.type_of(), SEXPTYPE::SYMSXP);
            assert!((*ub).sxpinfo.mark());
        }
    }

    #[test]
    fn test_r_missingarg_type() {
        unsafe {
            let ma = R_MissingArg();
            assert!(!ma.is_null());
            assert_eq!((*ma).sxpinfo.type_of(), SEXPTYPE::SYMSXP);
        }
    }

    #[test]
    fn test_logical_is_na() {
        assert!(LOGICAL_IS_NA(NA_INTEGER));
        assert!(!LOGICAL_IS_NA(0));
        assert!(!LOGICAL_IS_NA(1));
    }

    #[test]
    fn test_integer_is_na() {
        assert!(INTEGER_IS_NA(NA_INTEGER));
        assert!(!INTEGER_IS_NA(0));
        assert!(!INTEGER_IS_NA(42));
    }

    #[test]
    fn test_set_global_env() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        unsafe {
            let saved = R_GlobalEnv();
            let fake = 0x1 as SEXP;
            set_R_GlobalEnv(fake);
            assert_eq!(R_GlobalEnv(), fake);
            set_R_GlobalEnv(saved);
        }
    }

    #[test]
    fn test_environment_accessors_can_target_instance_explicitly() {
        let mut left = RInstance::new_for_gc_tests();
        let mut right = RInstance::new_for_gc_tests();
        let left_saved = R_GlobalEnv_in(&mut left);
        let right_saved = R_GlobalEnv_in(&mut right);

        set_R_GlobalEnv_in(&mut left, 0x1 as SEXP);
        set_R_GlobalEnv_in(&mut right, 0x2 as SEXP);

        assert_eq!(R_GlobalEnv_in(&mut left), 0x1 as SEXP);
        assert_eq!(R_GlobalEnv_in(&mut right), 0x2 as SEXP);

        set_R_GlobalEnv_in(&mut left, left_saved);
        set_R_GlobalEnv_in(&mut right, right_saved);
    }

    #[test]
    fn test_ambient_environment_wrapper_uses_current_instance_only() {
        let mut left = RInstance::new_for_gc_tests();
        let mut right = RInstance::new_for_gc_tests();
        let left_saved = R_GlobalEnv_in(&mut left);
        let right_saved = R_GlobalEnv_in(&mut right);
        let previous = unsafe { replace_current_instance(Some(&mut left)) };

        unsafe {
            set_R_GlobalEnv(0x1 as SEXP);
        }
        assert_eq!(R_GlobalEnv_in(&mut left), 0x1 as SEXP);
        assert_eq!(R_GlobalEnv_in(&mut right), right_saved);

        unsafe {
            replace_current_instance(Some(&mut right));
            set_R_GlobalEnv(0x2 as SEXP);
        }
        assert_eq!(R_GlobalEnv_in(&mut left), 0x1 as SEXP);
        assert_eq!(R_GlobalEnv_in(&mut right), 0x2 as SEXP);

        set_R_GlobalEnv_in(&mut left, left_saved);
        set_R_GlobalEnv_in(&mut right, right_saved);
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_eval_flags_can_target_instance_explicitly() {
        let mut left = RInstance::new_for_gc_tests();
        let mut right = RInstance::new_for_gc_tests();
        let previous = unsafe { replace_current_instance(Some(&mut left)) };
        // Direct access to the installed instance goes through the pointer
        // recorded at install time: a fresh `&mut left` would retag the
        // allocation and pop the installed borrow tag out from under the
        // ambient re-acquisition the `set_R_Visible`/`set_R_EvalDepth`
        // calls below perform (Stacked Borrows).
        let left_ptr = current_instance_ptr().expect("left should be installed");
        set_R_Visible_in(unsafe { &mut *left_ptr }, 0);
        set_R_Visible_in(&mut right, 1);
        set_R_EvalDepth_in(unsafe { &mut *left_ptr }, 7);
        set_R_EvalDepth_in(&mut right, 13);

        assert_eq!(R_Visible_in(unsafe { &mut *left_ptr }), 0);
        assert_eq!(R_Visible_in(&mut right), 1);
        assert_eq!(R_EvalDepth_in(unsafe { &mut *left_ptr }), 7);
        assert_eq!(R_EvalDepth_in(&mut right), 13);

        set_R_Visible(1);
        set_R_EvalDepth(3);
        assert_eq!(R_Visible_in(&mut left), 1);
        assert_eq!(R_Visible_in(&mut right), 1);
        assert_eq!(R_EvalDepth_in(&mut left), 3);
        assert_eq!(R_EvalDepth_in(&mut right), 13);
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_r_true_false_readable_through_standard_accessors() {
        use crate::sexp::accessors::{DATAPTR, LENGTH, LOGICAL, LOGICAL_ELT, XLENGTH};
        unsafe {
            let t = R_True();
            let f = R_False();

            // Shape: scalar LGLSXP vectors of length 1.
            assert_eq!((*t).sxpinfo.type_of(), SEXPTYPE::LGLSXP);
            assert_eq!((*f).sxpinfo.type_of(), SEXPTYPE::LGLSXP);
            assert_eq!(LENGTH(t), 1);
            assert_eq!(XLENGTH(f), 1);
            assert!((*t).sxpinfo.scalar());
            assert!((*f).sxpinfo.scalar());

            // The payload must be a real data word, not a null pointer.
            assert!(!DATAPTR(t).is_null());
            assert!(!DATAPTR(f).is_null());
            assert!(!LOGICAL(t).is_null());
            assert!(!LOGICAL(f).is_null());
            assert_eq!(*LOGICAL(t), 1);
            assert_eq!(*LOGICAL(f), 0);

            // Element accessor used across the evaluator (LGLSXP storage is
            // INTEGER-compatible, so translated C code reads it via LOGICAL
            // and sometimes INTEGER pointers; INTEGER_ELT itself requires
            // exact INTSXP).
            assert_eq!(LOGICAL_ELT(t, 0), 1);
            assert_eq!(LOGICAL_ELT(f, 0), 0);
            assert!(!LOGICAL_IS_NA(LOGICAL_ELT(t, 0)));
            assert!(!LOGICAL_IS_NA(LOGICAL_ELT(f, 0)));

            // Stable, distinct singletons.
            assert_eq!(R_True(), t);
            assert_eq!(R_False(), f);
            assert_ne!(t, f);
        }
    }

    #[test]
    fn test_r_true_false_survive_gc_cycles() {
        use crate::sexp::accessors::LOGICAL_ELT;
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        unsafe {
            // Churn young garbage, then run both collection kinds: the
            // singletons are out-of-arena and must keep their payload word.
            for i in 0..2000 {
                let _ = crate::sexp::constructors::Rf_ScalarInteger(i);
            }
            crate::sexp::gengc::minor_gc();
            crate::sexp::gengc::full_gc();
            crate::sexp::gengc::minor_gc();
            assert_eq!(LOGICAL_ELT(R_True(), 0), 1);
            assert_eq!(LOGICAL_ELT(R_False(), 0), 0);
        }
    }
}
