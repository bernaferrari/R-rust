#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Extended memory allocation functions for the R interpreter.
//!
//! These functions are used by the evaluator and other main/ modules.
//! They complement the basic arena allocator in memory.rs with:
//! - Environment creation (NewEnvironment)
//! - Promise creation (mkPROMISE)
//! - Raw cons cell allocation (not arena-tracked)
//! - allocSExp, allocFormalsList, etc.
//! - R_alloc/vmaxget/vmaxset (transient memory from C stack)

use std::os::raw::c_int;
use std::ptr;

use super::ffi::{SEXP, SEXPTYPE, SexprecCore};
use super::globals::{R_NilValue, R_UnboundValue};
use super::instance::RInstance;
use super::memory;

// ---------------------------------------------------------------------------
// NewEnvironment — create a new environment
// ---------------------------------------------------------------------------

/// Create a new environment with the given frame, enclosing env, and size.
///
/// This is the equivalent of R's `NewEnvironment()` in memory.c.
pub fn NewEnvironment(frame: SEXP, enclos: SEXP, hashtab: SEXP) -> SEXP {
    unsafe {
        let env = memory::with_arena(|arena| {
            let env = arena.alloc_node(SEXPTYPE::ENVSXP);
            if !env.is_null() {
                crate::sexp::accessors::SET_FRAME(env, frame);
                crate::sexp::accessors::SET_ENCLOS(env, enclos);
                crate::sexp::accessors::SET_HASHTAB(env, hashtab);
            }
            env
        });
        // Under a bounded arena, a failed node allocation is a budget
        // exhaustion, which GNU R reports as a catchable allocation error
        // rather than a silent NULL that evaluates to R_NilValue.
        super::constructors::require_allocation(env)
    }
}

pub fn NewPersistentEnvironment(frame: SEXP, enclos: SEXP, hashtab: SEXP) -> SEXP {
    super::instance::with_required_current_instance(|owner| unsafe {
        let mut header = SexprecCore::new(SEXPTYPE::ENVSXP);
        header.data = super::ffi::NodeBody::Environment(super::ffi::Envsxp {
            frame: (*owner)
                .persistent_nodes
                .link_from_projection(frame)
                .expect("environment frame belongs to its heap"),
            enclos: (*owner)
                .persistent_nodes
                .link_from_projection(enclos)
                .expect("environment enclosure belongs to its heap"),
            hashtab: (*owner)
                .persistent_nodes
                .link_from_projection(hashtab)
                .expect("environment table belongs to its heap"),
        });
        let value = (*owner)
            .persistent_nodes
            .allocate_header(header)
            .unwrap_or_else(|_| crate::sexp::context::r_error("persistent environment allocation"));
        (*owner).env_nodes.push(value);
        value
    })
}

// ---------------------------------------------------------------------------
// mkPROMISE — create a promise
// ---------------------------------------------------------------------------

/// Create a promise (PROMSXP) from an expression and environment.
///
/// This is the equivalent of R's `mkPROMISE()` in memory.c.
pub fn mkPROMISE(expr: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let prom = memory::with_arena(|arena| {
            let prom = arena.alloc_node(SEXPTYPE::PROMSXP);
            if !prom.is_null() {
                crate::sexp::accessors::SET_PRVALUE(prom, R_UnboundValue());
                crate::sexp::accessors::SET_PRCODE(prom, expr);
                crate::sexp::accessors::SET_PRENV(prom, env);
            }
            prom
        });
        super::constructors::require_allocation(prom)
    }
}

/// Create an already-evaluated promise (EVPROMISE).
///
/// This is the equivalent of R's `R_mkEVPROMISE()`.
pub fn R_mkEVPROMISE(expr: SEXP, value: SEXP) -> SEXP {
    unsafe {
        memory::with_arena(|arena| {
            let prom = arena.alloc_node(SEXPTYPE::PROMSXP);
            if !prom.is_null() {
                crate::sexp::accessors::SET_PRVALUE(prom, value);
                crate::sexp::accessors::SET_PRCODE(prom, expr);
                crate::sexp::accessors::SET_PRENV(prom, R_NilValue());
                // Set gp bits for EVPROMISE
                (*prom).sxpinfo.set_gp(1); // PRSEEN flag
            }
            prom
        })
    }
}

// ---------------------------------------------------------------------------
// allocSExp — allocate a scalar SEXP of any type
// ---------------------------------------------------------------------------

/// Allocate a scalar (non-vector) SEXP of the given type.
///
/// This is the equivalent of R's `allocSExp()`.
pub fn allocSExp(sexptype: SEXPTYPE) -> SEXP {
    unsafe { super::constructors::allocate_with_gc_retry([], |arena| arena.alloc_node(sexptype)) }
}

/// Create a PROMSXP binding an expression to an environment.
pub fn mkPROMSXP(expr: SEXP, env: SEXP) -> SEXP {
    unsafe {
        super::constructors::allocate_with_gc_retry([expr, env], |arena| {
            let p = arena.alloc_node(SEXPTYPE::PROMSXP);
            if !p.is_null() {
                crate::sexp::accessors::SET_PRVALUE(p, R_UnboundValue());
                crate::sexp::accessors::SET_PRCODE(p, expr);
                crate::sexp::accessors::SET_PRENV(p, env);
            }
            p
        })
    }
}

// ---------------------------------------------------------------------------
// Raw cons cell (not arena-tracked)
// ---------------------------------------------------------------------------

fn with_raw_cons<F, R>(f: F) -> R
where
    F: FnOnce(&mut Vec<*mut SexprecCore>) -> R,
{
    // SAFETY: `inst` is the live current-instance pointer handed to this accessor closure (or owned by this caller) for the duration of the call.
    super::instance::with_required_current_instance(|instance| unsafe {
        with_raw_cons_in(instance, f)
    })
}

/// # Safety
///
/// `inst`/`instance` must point to a live `RInstance` (as supplied by the instance accessors) with
/// no conflicting outstanding borrow of the fields touched here.
unsafe fn with_raw_cons_in<F, R>(instance: *mut RInstance, f: F) -> R
where
    F: FnOnce(&mut Vec<*mut SexprecCore>) -> R,
{
    // P1: the `&mut` field lend is held only across strictly-local Vec
    // operations that never reenter the interpreter.
    unsafe { f(&mut (*instance).raw_cons) }
}

/// Create a cons cell tracked for cleanup.
pub fn cons_raw(car: SEXP, cdr: SEXP) -> SEXP {
    super::instance::with_required_current_instance(|instance| unsafe {
        cons_raw_in(instance, car, cdr)
    })
}

pub(crate) unsafe fn cons_raw_in(instance: *mut RInstance, car: SEXP, cdr: SEXP) -> SEXP {
    let mut header = SexprecCore::new(SEXPTYPE::LISTSXP);
    header.data = super::ffi::NodeBody::List(super::ffi::Listsxp {
        carval: unsafe { (*instance).persistent_nodes.link_from_projection(car) }
            .expect("cons child belongs to its heap"),
        cdrval: unsafe { (*instance).persistent_nodes.link_from_projection(cdr) }
            .expect("cons tail belongs to its heap"),
        tagval: super::heap::NodeLink::null(),
    });
    let ptr = unsafe { (*instance).persistent_nodes.allocate_header(header) }
        .unwrap_or_else(|_| crate::sexp::context::r_error("persistent cons allocation"));
    // SAFETY: `inst` is the live current-instance pointer handed to this accessor closure (or owned by this caller) for the duration of the call.
    unsafe { with_raw_cons_in(instance, |rc| rc.push(ptr)) };
    ptr
}

/// Free a raw cons cell allocated by cons_raw.
pub fn free_raw_cons(ptr: SEXP) {
    super::instance::with_required_current_instance(|instance| unsafe {
        free_raw_cons_in(instance, ptr);
    });
}

pub(crate) unsafe fn free_raw_cons_in(instance: *mut RInstance, ptr: SEXP) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: `inst` is the live current-instance pointer handed to this accessor closure (or owned by this caller) for the duration of the call.
    let removed = unsafe {
        with_raw_cons_in(instance, |cells| {
            cells
                .iter()
                .position(|&p| p == ptr)
                .map(|pos| cells.remove(pos))
                .is_some()
        })
    };
    if removed {
        unsafe { (*instance).persistent_nodes.remove(ptr) };
    }
}

/// Create a cons cell that is not reference counted (CONS_NR).
///
/// This is the equivalent of R's `CONS_NR()` macro.
pub fn CONS_NR(car: SEXP, cdr: SEXP) -> SEXP {
    unsafe {
        memory::with_arena(|arena| {
            let cell = arena.cons(car, cdr, ptr::null_mut());
            if !cell.is_null() {
                // Set NAMED to 0 (not reference counted)
                (*cell).sxpinfo.set_named(0);
            }
            cell
        })
    }
}

// ---------------------------------------------------------------------------
// allocFormalsList — create formals list for closures
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// allocList / allocLang — allocate list/lang vectors
// ---------------------------------------------------------------------------

/// Allocate a pairlist (LISTSXP chain) of n elements.
///
/// This is the equivalent of R's `allocList()`.
pub fn allocList(n: c_int) -> SEXP {
    unsafe { memory::with_arena(|arena| arena.alloc_list_chain(n)) }
}

/// Allocate a lang (LANGSXP) pairlist of n elements.
///
/// This is the equivalent of R's `allocLang()` in memory.c.
pub fn allocLang(n: c_int) -> SEXP {
    {
        let list = allocList(n);
        if !list.is_null() {
            // Walk the list and set each element to LANGSXP type
            let mut current = list;
            while !current.is_null() && current != R_NilValue() {
                crate::sexp::accessors::SET_TYPEOF(current, SEXPTYPE::LANGSXP.as_c_int());
                current = crate::sexp::accessors::CDR(current);
            }
        }
        list
    }
}

// ---------------------------------------------------------------------------
// R_alloc / vmaxget / vmaxset — transient memory (C stack-like)
// ---------------------------------------------------------------------------

/// Mark-relative C-stack storage for one primitive application. Managed
/// runtimes retain the original physical owner through cleanup, including
/// revocation; explicitly borrowed fixtures retain their caller's lifetime.
pub(crate) struct TransientScope<'session> {
    owner: super::owner::StoredOwner<'session>,
    // Drop's body releases buffers before this operation pin is released.
    _pin: Option<super::owner::OwnerPin>,
}

impl<'session> TransientScope<'session> {
    pub(crate) fn enter(
        owner: super::owner::StoredOwner<'session>,
    ) -> super::object::SexpResult<Self> {
        let pin = owner.managed().map(|owner| owner.pin()).transpose()?;
        owner.require_active()?;
        Ok(Self {
            owner,
            _pin: pin,
        })
    }
}

impl Drop for TransientScope<'_> {
    fn drop(&mut self) {
        // The owner pin is released after this scope's work completes; field
        // ordering performs the release. No transient-arena teardown remains:
        // the watermark arena was retired with R_alloc.
    }
}

fn raw_cons_len() -> usize {
    with_raw_cons(|rc| rc.len())
}

#[cfg(test)]
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn raw_cons_len_in(instance: *mut RInstance) -> usize {
    // SAFETY: `inst` is the live current-instance pointer handed to this accessor closure (or owned by this caller) for the duration of the call.
    unsafe { with_raw_cons_in(instance, |rc| rc.len()) }
}


#[cfg(test)]
mod tests {
    use std::ptr::addr_of_mut;

    use super::super::constructors::*;
    use super::super::ffi::*;
    use crate::sexp::instance::RInstance;
    use crate::sexp::session::RSession;

    use super::*;






    #[test]
    fn test_new_environment() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let env = NewEnvironment(ptr::null_mut(), R_NilValue(), ptr::null_mut());
            assert!(!env.is_null());
            assert_eq!((*env).sxpinfo.type_of(), SEXPTYPE::ENVSXP);
        }
    }

    #[test]
    fn test_mk_promise() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let expr = Rf_ScalarInteger(42);
            let prom = mkPROMISE(expr, R_NilValue());
            assert!(!prom.is_null());
            assert_eq!((*prom).sxpinfo.type_of(), SEXPTYPE::PROMSXP);
            assert_eq!(crate::sexp::accessors::PRCODE(prom), expr);
        }
    }

    #[test]
    fn test_alloc_s_exp() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = allocSExp(SEXPTYPE::SYMSXP);
            assert!(!s.is_null());
            assert_eq!((*s).sxpinfo.type_of(), SEXPTYPE::SYMSXP);
        }
    }

    #[test]
    fn test_cons_nr() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let car = Rf_ScalarInteger(1);
            let cdr = Rf_ScalarInteger(2);
            let cell = CONS_NR(car, cdr);
            assert!(!cell.is_null());
            assert_eq!((*cell).sxpinfo.type_of(), SEXPTYPE::LISTSXP);
        }
    }

    #[test]
    fn test_alloc_lang() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let lang = allocLang(3);
            assert!(!lang.is_null());
            assert_eq!((*lang).sxpinfo.type_of(), SEXPTYPE::LANGSXP);
        }
    }



    #[test]
    fn persistent_owned_headers_and_typed_payloads_survive_collection_and_owner_move() {
        use crate::sexp::accessors::*;
        let session = RSession::new_for_gc_tests();
        let token = session.with_active(|| unsafe {
            let value = Rf_ScalarInteger(77);
            let binding = cons_raw(value, R_NilValue());
            let name = super::super::symbol::Rf_installChar(c"multi-byte-symbol".as_ptr(), 17);
            SETTAG(binding, name);
            let environment =
                NewPersistentEnvironment(binding, crate::sexp::globals::R_BaseEnv(), R_NilValue());
            let character = persistent_mkChar(c"multi-byte-persistent".as_ptr());
            let integer = persistent_scalar_integer(41);
            let logical = persistent_scalar_logical(1);
            let real = persistent_scalar_real(2.5);
            let string = persistent_mkstring(c"owned-string-bytes".as_ptr());
            let instance = super::super::instance::current_instance_ptr().unwrap();
            let token = (*instance).node_token(environment).unwrap();
            for raw in [
                binding, name, character, integer, logical, real, string, value,
            ] {
                assert!(token.same_heap(&(*instance).node_token(raw).unwrap()));
            }
            super::super::gengc::full_gc();
            assert_eq!(INTEGER_ELT(CAR(binding), 0), 77);
            assert_eq!(
                &crate::sexp::accessors::char_cstr_bytes(character)[..],
                b"multi-byte-persistent"
            );
            assert_eq!(
                &crate::sexp::accessors::char_cstr_bytes(PRINTNAME(name))[..],
                b"multi-byte-symbol"
            );
            assert_eq!(INTEGER_ELT(integer, 0), 41);
            assert_eq!(LOGICAL_ELT(logical, 0), 1);
            assert_eq!(REAL_ELT(real, 0), 2.5);
            assert_eq!(
                &crate::sexp::accessors::char_cstr_bytes(STRING_ELT(string, 0))[..],
                b"owned-string-bytes"
            );
            token
        });
        assert!(token.is_live());
        let moved = Box::new(session);
        moved.gc();
        assert!(token.is_live());
        drop(moved);
        assert!(!token.is_live());
    }

    #[test]
    fn persistent_raw_cons_release_invalidates_the_exact_allocation() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let value = cons_raw(R_NilValue(), R_NilValue());
            let instance = super::super::instance::current_instance_ptr().unwrap();
            let token = (*instance).node_token(value).unwrap();
            free_raw_cons(value);
            assert!(!token.is_live());
            assert!((*instance).node_token(value).is_none());
            assert!(super::super::memory::checked_node(value).is_none());
            let next = cons_raw(R_NilValue(), R_NilValue());
            assert_ne!(token, (*instance).node_token(next).unwrap());
        });
    }
}
