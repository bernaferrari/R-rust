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

use std::alloc::{Layout, alloc, dealloc};
use std::os::raw::{c_int, c_void};
use std::ptr;

use super::ffi::{SEXP, SEXPTYPE, SexprecCore};
use super::globals::R_NilValue;
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
                (*env).data.envsxp.frame = frame;
                (*env).data.envsxp.enclos = enclos;
                (*env).data.envsxp.hashtab = hashtab;
            }
            env
        })
    }
}

pub unsafe fn NewPersistentEnvironment(frame: SEXP, enclos: SEXP, hashtab: SEXP) -> SEXP {
    let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::ENVSXP));
    let env: SEXP = &mut *boxed as *mut _;
    (*env).data.envsxp.frame = frame;
    (*env).data.envsxp.enclos = enclos;
    (*env).data.envsxp.hashtab = hashtab;
    Box::leak(boxed)
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
                (*prom).data.promsxp.value = R_NilValue();
                (*prom).data.promsxp.expr = expr;
                (*prom).data.promsxp.env = env;
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
                (*prom).data.promsxp.value = value;
                (*prom).data.promsxp.expr = expr;
                (*prom).data.promsxp.env = R_NilValue();
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
    memory::with_arena(|arena| arena.alloc_node(sexptype))
}

/// Create a PROMSXP binding an expression to an environment.
pub unsafe fn mkPROMSXP(expr: SEXP, env: SEXP) -> SEXP {
    let p = allocSExp(SEXPTYPE::PROMSXP);
    if !p.is_null() {
        (*p).data.promsxp.value = R_NilValue();
        (*p).data.promsxp.expr = expr;
        (*p).data.promsxp.env = env;
    }
    p
}

// ---------------------------------------------------------------------------
// Raw cons cell (not arena-tracked)
// ---------------------------------------------------------------------------

// Registry of raw cons cells allocated outside the arena.
thread_local! {
    static RAW_CONS: std::cell::RefCell<Vec<*mut SexprecCore>> = std::cell::RefCell::new(Vec::new());
}

/// Create a cons cell tracked for cleanup.
pub unsafe fn cons_raw(car: SEXP, cdr: SEXP) -> SEXP {
    let boxed = Box::new(SexprecCore::new(SEXPTYPE::LISTSXP));
    let ptr: SEXP = Box::into_raw(boxed);
    unsafe {
        (*ptr).data.listsxp.carval = car;
        (*ptr).data.listsxp.cdrval = cdr;
        (*ptr).data.listsxp.tagval = ptr::null_mut();
    }
    RAW_CONS.with(|rc| rc.borrow_mut().push(ptr));
    ptr
}

/// Free a raw cons cell allocated by cons_raw.
pub unsafe fn free_raw_cons(ptr: SEXP) {
    if ptr.is_null() {
        return;
    }
    RAW_CONS.with(|rc| {
        let mut cells = rc.borrow_mut();
        if let Some(pos) = cells.iter().position(|&p| p == ptr) {
            cells.remove(pos);
            unsafe {
                let _ = Box::from_raw(ptr);
            }
        }
    });
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

/// Create a formals list from 3 symbols.
pub unsafe fn allocFormalsList3(sym1: SEXP, sym2: SEXP, sym3: SEXP) -> SEXP {
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

// ---------------------------------------------------------------------------
// allocList / allocLang — allocate list/lang vectors
// ---------------------------------------------------------------------------

/// Allocate a pairlist (LISTSXP chain) of n elements.
///
/// This is the equivalent of R's `allocList()`.
pub unsafe fn allocList(n: c_int) -> SEXP {
    memory::with_arena(|arena| arena.alloc_list_chain(n))
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
                current = (*current).data.listsxp.cdrval;
            }
        }
        list
    }
}

// ---------------------------------------------------------------------------
// R_alloc / vmaxget / vmaxset — transient memory (C stack-like)
// ---------------------------------------------------------------------------

thread_local! {
    /// Transient allocation buffer (like C stack allocation).
    static VMAX: std::cell::RefCell<Vec<(*mut u8, Layout)>> =
        std::cell::RefCell::new(Vec::new());
}

/// Allocate transient memory (freed by vmaxset).
///
/// This is the equivalent of R's `R_alloc()` which allocates on the C stack.
/// In Rust, we use a thread-local buffer that's freed on vmaxset().
#[unsafe(no_mangle)]
pub unsafe fn R_alloc(_size: usize, nelem: usize) -> *mut c_void {
    unsafe {
        let total = _size.checked_mul(nelem).unwrap_or(0);
        if total == 0 {
            return ptr::null_mut();
        }
        let layout =
            Layout::from_size_align(total, std::mem::align_of::<u64>()).unwrap_or_else(|_| {
                Layout::from_size_align(total, 1)
                    .expect("Layout::from_size_align with align=1 must succeed")
            });
        let ptr = alloc(layout);
        if ptr.is_null() {
            return ptr::null_mut();
        }
        // Zero-initialize
        std::ptr::write_bytes(ptr, 0, total);
        VMAX.with(|vmax| {
            vmax.borrow_mut().push((ptr, layout));
        });
        ptr as *mut c_void
    }
}

/// Get the current transient allocation watermark.
///
/// Returns an opaque value to pass to vmaxset().
pub unsafe fn vmaxget() -> *mut c_void {
    VMAX.with(|vmax| {
        let len = vmax.borrow().len();
        len as *mut c_void
    })
}

/// Reset transient allocations to the given watermark.
///
/// Frees all transient allocations made since the corresponding vmaxget().
pub unsafe fn vmaxset(value: *mut c_void) {
    let mark = value as usize;
    VMAX.with(|vmax| {
        let mut vmax = vmax.borrow_mut();
        let drain_start = vmax
            .iter()
            .position(|(ptr, _)| *ptr as usize >= mark)
            .unwrap_or(vmax.len());
        for (ptr, layout) in vmax.drain(drain_start..) {
            if !ptr.is_null() && layout.size() > 0 {
                unsafe {
                    dealloc(ptr, layout);
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::constructors::*;
    use super::super::ffi::*;
    use super::*;

    #[test]
    fn test_new_environment() {
        unsafe {
            let env = NewEnvironment(ptr::null_mut(), R_NilValue(), ptr::null_mut());
            assert!(!env.is_null());
            assert_eq!((*env).sxpinfo.type_of(), SEXPTYPE::ENVSXP);
        }
    }

    #[test]
    fn test_mk_promise() {
        unsafe {
            let expr = Rf_ScalarInteger(42);
            let prom = mkPROMISE(expr, R_NilValue());
            assert!(!prom.is_null());
            assert_eq!((*prom).sxpinfo.type_of(), SEXPTYPE::PROMSXP);
            assert_eq!((*prom).data.promsxp.expr, expr);
        }
    }

    #[test]
    fn test_alloc_s_exp() {
        unsafe {
            let s = allocSExp(SEXPTYPE::SYMSXP);
            assert!(!s.is_null());
            assert_eq!((*s).sxpinfo.type_of(), SEXPTYPE::SYMSXP);
        }
    }

    #[test]
    fn test_cons_nr() {
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
        unsafe {
            let lang = allocLang(3);
            assert!(!lang.is_null());
            assert_eq!((*lang).sxpinfo.type_of(), SEXPTYPE::LANGSXP);
        }
    }

    #[test]
    fn test_r_alloc_and_vmaxset() {
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
}
