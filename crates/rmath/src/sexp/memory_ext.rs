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

use std::alloc::Layout;
use std::os::raw::{c_int, c_void};
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
pub unsafe fn NewEnvironment(frame: SEXP, enclos: SEXP, hashtab: SEXP) -> SEXP {
    unsafe {
        memory::with_arena(|arena| {
            let env = arena.alloc_node(SEXPTYPE::ENVSXP);
            if !env.is_null() {
                crate::sexp::accessors::SET_FRAME(env, frame);
                crate::sexp::accessors::SET_ENCLOS(env, enclos);
                crate::sexp::accessors::SET_HASHTAB(env, hashtab);
            }
            env
        })
    }
}

pub unsafe fn NewPersistentEnvironment(frame: SEXP, enclos: SEXP, hashtab: SEXP) -> SEXP {
    super::instance::with_required_current_instance(|owner| unsafe {
        let mut header = SexprecCore::new(SEXPTYPE::ENVSXP);
        header.data = super::ffi::NodeBody::Environment(super::ffi::Envsxp {
            frame: (*owner).persistent_nodes.link_from_projection(frame).expect("environment frame belongs to its heap"),
            enclos: (*owner).persistent_nodes.link_from_projection(enclos).expect("environment enclosure belongs to its heap"),
            hashtab: (*owner).persistent_nodes.link_from_projection(hashtab).expect("environment table belongs to its heap"),
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
pub unsafe fn mkPROMISE(expr: SEXP, env: SEXP) -> SEXP {
    unsafe {
        memory::with_arena(|arena| {
            let prom = arena.alloc_node(SEXPTYPE::PROMSXP);
            if !prom.is_null() {
                crate::sexp::accessors::SET_PRVALUE(prom, R_UnboundValue());
                crate::sexp::accessors::SET_PRCODE(prom, expr);
                crate::sexp::accessors::SET_PRENV(prom, env);
            }
            prom
        })
    }
}

/// Create an already-evaluated promise (EVPROMISE).
///
/// This is the equivalent of R's `R_mkEVPROMISE()`.
pub unsafe fn R_mkEVPROMISE(expr: SEXP, value: SEXP) -> SEXP {
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
pub unsafe fn allocSExp(sexptype: SEXPTYPE) -> SEXP {
    unsafe { memory::with_arena(|arena| arena.alloc_node(sexptype)) }
}

/// Create a PROMSXP binding an expression to an environment.
pub unsafe fn mkPROMSXP(expr: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let p = allocSExp(SEXPTYPE::PROMSXP);
        if !p.is_null() {
            crate::sexp::accessors::SET_PRVALUE(p, R_UnboundValue());
            crate::sexp::accessors::SET_PRCODE(p, expr);
            crate::sexp::accessors::SET_PRENV(p, env);
        }
        p
    }
}

// ---------------------------------------------------------------------------
// Raw cons cell (not arena-tracked)
// ---------------------------------------------------------------------------

fn with_raw_cons<F, R>(f: F) -> R
where
    F: FnOnce(&mut Vec<*mut SexprecCore>) -> R,
{
    super::instance::with_required_current_instance(|instance| with_raw_cons_in(instance, f))
}

fn with_raw_cons_in<F, R>(instance: *mut RInstance, f: F) -> R
where
    F: FnOnce(&mut Vec<*mut SexprecCore>) -> R,
{
    // P1: the `&mut` field lend is held only across strictly-local Vec
    // operations that never reenter the interpreter.
    unsafe { f(&mut (*instance).raw_cons) }
}

/// Create a cons cell tracked for cleanup.
pub unsafe fn cons_raw(car: SEXP, cdr: SEXP) -> SEXP {
    super::instance::with_required_current_instance(|instance| unsafe {
        cons_raw_in(instance, car, cdr)
    })
}

pub(crate) unsafe fn cons_raw_in(instance: *mut RInstance, car: SEXP, cdr: SEXP) -> SEXP {
    let mut header = SexprecCore::new(SEXPTYPE::LISTSXP);
    header.data = super::ffi::NodeBody::List(super::ffi::Listsxp {
        carval: unsafe { (*instance).persistent_nodes.link_from_projection(car) }.expect("cons child belongs to its heap"),
        cdrval: unsafe { (*instance).persistent_nodes.link_from_projection(cdr) }.expect("cons tail belongs to its heap"),
        tagval: super::heap::NodeLink::null(),
    });
    let ptr = unsafe { (*instance).persistent_nodes.allocate_header(header) }
        .unwrap_or_else(|_| crate::sexp::context::r_error("persistent cons allocation"));
    with_raw_cons_in(instance, |rc| rc.push(ptr));
    ptr
}

/// Free a raw cons cell allocated by cons_raw.
pub unsafe fn free_raw_cons(ptr: SEXP) {
    super::instance::with_required_current_instance(|instance| unsafe {
        free_raw_cons_in(instance, ptr);
    });
}

pub(crate) unsafe fn free_raw_cons_in(instance: *mut RInstance, ptr: SEXP) {
    if ptr.is_null() {
        return;
    }
    let removed = with_raw_cons_in(instance, |cells| {
        cells
            .iter()
            .position(|&p| p == ptr)
            .map(|pos| cells.remove(pos))
            .is_some()
    });
    if removed {
        unsafe { (*instance).persistent_nodes.remove(ptr) };
    }
}

/// Create a cons cell that is not reference counted (CONS_NR).
///
/// This is the equivalent of R's `CONS_NR()` macro.
pub unsafe fn CONS_NR(car: SEXP, cdr: SEXP) -> SEXP {
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

/// Create a formals list from 2 symbols.
pub unsafe fn allocFormalsList2(sym1: SEXP, sym2: SEXP) -> SEXP {
    unsafe {
        memory::with_arena(|arena| {
            let cdr = if sym2.is_null() {
                unsafe { R_NilValue() }
            } else {
                let cell = arena.cons(sym2, unsafe { R_NilValue() }, ptr::null_mut());
                if !cell.is_null() {
                    unsafe {
                        (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                    }
                }
                cell
            };
            let car = arena.cons(sym1, cdr, ptr::null_mut());
            if !car.is_null() {
                unsafe {
                    (*car).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
            }
            car
        })
    }
}

/// Create a formals list from 3 symbols.
pub unsafe fn allocFormalsList3(sym1: SEXP, sym2: SEXP, sym3: SEXP) -> SEXP {
    unsafe {
        memory::with_arena(|arena| {
            let c3 = if sym3.is_null() {
                unsafe { R_NilValue() }
            } else {
                let cell = arena.cons(sym3, unsafe { R_NilValue() }, ptr::null_mut());
                if !cell.is_null() {
                    unsafe {
                        (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                    }
                }
                cell
            };
            let c2 = if sym2.is_null() {
                c3
            } else {
                let cell = arena.cons(sym2, c3, ptr::null_mut());
                if !cell.is_null() {
                    unsafe {
                        (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                    }
                }
                cell
            };
            let c1 = if sym1.is_null() {
                c2
            } else {
                let cell = arena.cons(sym1, c2, ptr::null_mut());
                if !cell.is_null() {
                    unsafe {
                        (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                    }
                }
                cell
            };
            c1
        })
    }
}

// ---------------------------------------------------------------------------
// allocList / allocLang — allocate list/lang vectors
// ---------------------------------------------------------------------------

/// Allocate a pairlist (LISTSXP chain) of n elements.
///
/// This is the equivalent of R's `allocList()`.
pub unsafe fn allocList(n: c_int) -> SEXP {
    unsafe { memory::with_arena(|arena| arena.alloc_list_chain(n)) }
}

/// Allocate a lang (LANGSXP) pairlist of n elements.
///
/// This is the equivalent of R's `allocLang()` in memory.c.
pub unsafe fn allocLang(n: c_int) -> SEXP {
    unsafe {
        let list = allocList(n);
        if !list.is_null() {
            // Walk the list and set each element to LANGSXP type
            let mut current = list;
            while !current.is_null() && current != R_NilValue() {
                (*current).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                current = crate::sexp::accessors::CDR(current);
            }
        }
        list
    }
}

// ---------------------------------------------------------------------------
// R_alloc / vmaxget / vmaxset — transient memory (C stack-like)
// ---------------------------------------------------------------------------

/// A transient buffer and its original owner's byte reservation are released
/// together, including when an instance is destroyed during an unwind.
pub(crate) struct TransientAllocation {
    buffer: memory::OwnedBuffer,
    _reservation: memory::TransientReservation,
}

fn with_vmax<F, R>(f: F) -> R
where
    F: FnOnce(&mut Vec<TransientAllocation>) -> R,
{
    super::instance::with_required_current_instance(|instance| with_vmax_in(instance, f))
}

fn with_vmax_in<F, R>(instance: *mut RInstance, f: F) -> R
where
    F: FnOnce(&mut Vec<TransientAllocation>) -> R,
{
    // P1: the `&mut` field lend is held only across strictly-local Vec
    // operations (push/len/drain + dealloc) that never reenter the
    // interpreter.
    unsafe { f(&mut (*instance).vmax) }
}

/// Allocate transient memory (freed by vmaxset).
///
/// This is the equivalent of R's `R_alloc()` which allocates on the C stack.
/// In Rust, the active session owns a transient allocation buffer that's freed
/// on vmaxset().
pub(crate) unsafe fn R_alloc(size: usize, nelem: usize) -> *mut c_void {
    let ptr = super::instance::with_required_current_instance(|instance| unsafe {
        R_alloc_in(instance, size, nelem)
    });
    // Ported callers rely on R_alloc returning usable storage or raising an
    // R error. Returning null on a nonempty request invites unchecked writes.
    if ptr.is_null() && size != 0 && nelem != 0 {
        super::context::r_error(
            "cannot allocate transient memory: size, memory budget or allocation failure",
        );
    }
    ptr
}

pub(crate) unsafe fn R_alloc_in(
    instance: *mut RInstance,
    size: usize,
    nelem: usize,
) -> *mut c_void {
    unsafe {
        let Some(total) = size.checked_mul(nelem) else {
            return ptr::null_mut();
        };
        if total == 0 {
            return ptr::null_mut();
        }
        let Ok(layout) = Layout::from_size_align(total, std::mem::align_of::<u64>()) else {
            return ptr::null_mut();
        };
        // Reserve the bookkeeping slot before allocating any raw storage.
        if !with_vmax_in(instance, |vmax| vmax.try_reserve(1).is_ok()) {
            return ptr::null_mut();
        }
        let Some(reservation) = memory::reserve_transient_in(instance, total) else {
            return ptr::null_mut();
        };
        let Some(buffer) = memory::OwnedBuffer::zeroed(layout) else {
            return ptr::null_mut();
        };
        let ptr = buffer.as_ptr();
        with_vmax_in(instance, |vmax| {
            vmax.push(TransientAllocation {
                buffer,
                _reservation: reservation,
            });
        });
        ptr.cast()
    }
}

/// Get the current transient allocation watermark.
///
/// Returns an opaque value to pass to vmaxset().
pub unsafe fn vmaxget() -> *mut c_void {
    super::instance::with_required_current_instance(vmaxget_in)
}

pub(crate) fn vmaxget_in(instance: *mut RInstance) -> *mut c_void {
    // This is an opaque index, never an address to dereference.
    with_vmax_in(instance, |vmax| ptr::without_provenance_mut(vmax.len()))
}

/// Reset transient allocations to the given watermark.
///
/// Frees all transient allocations made since the corresponding vmaxget().
pub unsafe fn vmaxset(value: *mut c_void) {
    super::instance::with_required_current_instance(|instance| unsafe {
        vmaxset_in(instance, value);
    });
}

pub(crate) unsafe fn vmaxset_in(instance: *mut RInstance, value: *mut c_void) {
    let mark = value.addr();
    with_vmax_in(instance, |vmax| {
        let drain_start = mark.min(vmax.len());
        vmax.truncate(drain_start);
    });
}

#[cfg(test)]
fn raw_cons_len() -> usize {
    with_raw_cons(|rc| rc.len())
}

#[cfg(test)]
fn raw_cons_len_in(instance: *mut RInstance) -> usize {
    with_raw_cons_in(instance, |rc| rc.len())
}

#[cfg(test)]
fn vmax_len() -> usize {
    with_vmax(|vmax| vmax.len())
}

#[cfg(test)]
fn vmax_len_in(instance: *mut RInstance) -> usize {
    with_vmax_in(instance, |vmax| vmax.len())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ptr::addr_of_mut;

    use super::super::constructors::*;
    use super::super::ffi::*;
    use crate::sexp::instance::RInstance;
    use crate::sexp::session::RSession;

    use super::*;

    #[test]
    fn transient_allocation_rejects_unrepresentable_layout() {
        let mut owner = RInstance::new_for_gc_tests();
        unsafe {
            assert!(R_alloc_in(addr_of_mut!(owner), 1, isize::MAX as usize + 1).is_null());
            assert!(R_alloc_in(addr_of_mut!(owner), usize::MAX, 2).is_null());
            assert!(R_alloc_in(addr_of_mut!(owner), 0, usize::MAX).is_null());
            assert_eq!(vmax_len_in(addr_of_mut!(owner)), 0);
        }
    }

    #[test]
    fn transient_allocations_are_zeroed_aligned_and_isolated_during_nested_lends() {
        let mut left = RInstance::new_for_gc_tests();
        let mut right = RInstance::new_for_gc_tests();
        let node_bytes = std::mem::size_of::<SexprecCore>();
        left.arena
            .set_budget(memory::ArenaBudget::new(node_bytes + 32, 0));
        right.arena.set_budget(memory::ArenaBudget::new(4, 0));
        unsafe {
            let left_ptr = addr_of_mut!(left);
            let right_ptr = addr_of_mut!(right);
            memory::with_arena_in(left_ptr, |left_arena| {
                memory::with_arena_in(right_ptr, |_right_arena| {
                    assert!(!left_arena.alloc_node(SEXPTYPE::LISTSXP).is_null());
                    left_arena.set_budget(memory::ArenaBudget::new(node_bytes + 16, 0));
                    // This must find the outer owner's ledger, not the top one.
                    let buffer = R_alloc_in(left_ptr, 8, 2).cast::<u64>();
                    assert!(!buffer.is_null());
                    assert_eq!(*buffer, 0);
                    assert_eq!(*buffer.add(1), 0);
                    *buffer.add(1) = 42;
                    assert_eq!(*buffer.add(1), 42);
                    assert!(R_alloc_in(left_ptr, 1, 1).is_null());
                    assert!(R_alloc_in(right_ptr, 1, 5).is_null());
                    assert!(!R_alloc_in(right_ptr, 1, 4).is_null());
                });
                assert!(left_arena.try_reserve_transient(1).is_none());
                vmaxset_in(left_ptr, ptr::null_mut());
                assert!(left_arena.try_reserve_transient(16).is_some());
            });
            assert_eq!(vmax_len_in(left_ptr), 0);
            assert_eq!(vmax_len_in(right_ptr), 1);
            vmaxset_in(right_ptr, ptr::null_mut());
            assert!(right.arena.try_reserve_transient(4).is_some());
        }
    }

    #[test]
    fn transient_allocation_failure_raises_r_error_and_session_recovers() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            (*instance).arena.set_budget(memory::ArenaBudget::new(8, 0));
            for (size, count) in [(1, 9), (usize::MAX, 2), (1, isize::MAX as usize + 1)] {
                let error = std::panic::catch_unwind(|| R_alloc(size, count)).unwrap_err();
                assert!(
                    error
                        .downcast_ref::<crate::sexp::context::RError>()
                        .is_some()
                );
                assert_eq!(vmax_len_in(instance), 0);
                assert!((*instance).arena.try_reserve_transient(8).is_some());
            }
            assert!(!R_alloc(1, 8).is_null());
            vmaxset_in(instance, ptr::null_mut());
            assert!((*instance).arena.try_reserve_transient(8).is_some());
        });
    }

    #[test]
    fn transient_allocation_teardown_releases_original_reservation_on_unwind() {
        let result = std::panic::catch_unwind(|| {
            let mut owner = RInstance::new_for_gc_tests();
            owner.arena.set_budget(memory::ArenaBudget::new(8, 0));
            unsafe {
                assert!(!R_alloc_in(addr_of_mut!(owner), 1, 8).is_null());
                assert!(owner.arena.try_reserve_transient(1).is_none());
            }
            // Drop the owner after moving it while its transient buffers live.
            let _moved = Box::new(owner);
            panic!("fixture unwind");
        });
        assert!(result.is_err());
    }

    #[test]
    fn transient_allocations_share_arena_budget_and_release_at_watermark() {
        let mut owner = RInstance::new_for_gc_tests();
        owner.arena.set_budget(memory::ArenaBudget::new(16, 0));
        unsafe {
            let instance = addr_of_mut!(owner);
            let first = R_alloc_in(instance, 1, 8);
            assert!(!first.is_null());
            let mark = vmaxget_in(instance);
            assert!(!R_alloc_in(instance, 2, 4).is_null());
            assert!(R_alloc_in(instance, 1, 1).is_null());
            assert!(!owner.arena.try_reserve_transient(1).is_some());
            vmaxset_in(instance, mark);
            assert!(owner.arena.try_reserve_transient(8).is_some());
            assert!(!owner.arena.try_reserve_transient(9).is_some());
            vmaxset_in(instance, ptr::null_mut());
            assert!(owner.arena.try_reserve_transient(16).is_some());
        }
    }

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
    fn test_r_alloc_and_vmaxset() {
        let _session = crate::sexp::session::RSession::new();
        let _session = RSession::new();
        unsafe {
            let mark = vmaxget();
            let ptr = R_alloc(std::mem::size_of::<i32>(), 10);
            assert!(!ptr.is_null());
            // Write to it
            let ints = ptr as *mut i32;
            *ints.add(0) = 42;
            assert_eq!(*ints.add(0), 42);
            vmaxset(mark);
        }
    }

    #[test]
    fn test_session_raw_cons_and_vmax_are_local_on_same_thread() {
        let _session = crate::sexp::session::RSession::new();
        let left = RSession::new();
        let right = RSession::new();

        let mut left_cons = ptr::null_mut();

        left.with_active(|| unsafe {
            left_cons = cons_raw(ptr::null_mut(), ptr::null_mut());
            assert_eq!(raw_cons_len(), 1);
            let mark = vmaxget();
            let ptr = R_alloc(1, 8);
            assert!(!ptr.is_null());
            assert_eq!(vmax_len(), 1);
            vmaxset(mark);
            assert_eq!(vmax_len(), 0);
        });

        right.with_active(|| unsafe {
            assert_eq!(raw_cons_len(), 0);
            let right_cons = cons_raw(ptr::null_mut(), ptr::null_mut());
            assert_eq!(raw_cons_len(), 1);
            assert_ne!(right_cons, left_cons);
            free_raw_cons(right_cons);
            assert_eq!(raw_cons_len(), 0);
        });

        left.with_active(|| unsafe {
            assert_eq!(raw_cons_len(), 1);
            free_raw_cons(left_cons);
            assert_eq!(raw_cons_len(), 0);
        });
    }

    #[test]
    fn test_raw_cons_and_vmax_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        unsafe {
            let left_cons = cons_raw_in(addr_of_mut!(left), ptr::null_mut(), ptr::null_mut());
            assert_eq!(raw_cons_len_in(addr_of_mut!(left)), 1);
            assert_eq!(raw_cons_len_in(addr_of_mut!(right)), 0);

            let right_cons = cons_raw_in(addr_of_mut!(right), ptr::null_mut(), ptr::null_mut());
            assert_eq!(raw_cons_len_in(addr_of_mut!(left)), 1);
            assert_eq!(raw_cons_len_in(addr_of_mut!(right)), 1);

            let left_mark = vmaxget_in(addr_of_mut!(left));
            let right_mark = vmaxget_in(addr_of_mut!(right));
            let left_ptr = R_alloc_in(addr_of_mut!(left), 1, 8);
            assert!(!left_ptr.is_null());
            assert_eq!(vmax_len_in(addr_of_mut!(left)), 1);
            assert_eq!(vmax_len_in(addr_of_mut!(right)), 0);

            let right_ptr = R_alloc_in(addr_of_mut!(right), 1, 4);
            assert!(!right_ptr.is_null());
            assert_eq!(vmax_len_in(addr_of_mut!(left)), 1);
            assert_eq!(vmax_len_in(addr_of_mut!(right)), 1);

            vmaxset_in(addr_of_mut!(left), left_mark);
            assert_eq!(vmax_len_in(addr_of_mut!(left)), 0);
            assert_eq!(vmax_len_in(addr_of_mut!(right)), 1);

            vmaxset_in(addr_of_mut!(right), right_mark);
            free_raw_cons_in(addr_of_mut!(left), left_cons);
            free_raw_cons_in(addr_of_mut!(right), right_cons);
            assert_eq!(raw_cons_len_in(addr_of_mut!(left)), 0);
            assert_eq!(raw_cons_len_in(addr_of_mut!(right)), 0);
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
                std::ffi::CStr::from_ptr(CHAR(character)).to_bytes(),
                b"multi-byte-persistent"
            );
            assert_eq!(
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(name))).to_bytes(),
                b"multi-byte-symbol"
            );
            assert_eq!(INTEGER_ELT(integer, 0), 41);
            assert_eq!(LOGICAL_ELT(logical, 0), 1);
            assert_eq!(REAL_ELT(real, 0), 2.5);
            assert_eq!(
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(string, 0))).to_bytes(),
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
