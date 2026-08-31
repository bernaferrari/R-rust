#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Symbol table for R symbol interning.
//!
//! Provides `Rf_install()` which interns symbol names so that each unique
//! name maps to exactly one SexprecCore node. Uses a hash table with
//! separate chaining for collision resolution.

use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;

use super::accessors::{CHAR, PRINTNAME, TYPEOF};
use super::ffi::{R_xlen_t, SEXP, SEXPTYPE, SexprecCore, SexprecData};
use super::instance::RInstance;

// ---------------------------------------------------------------------------
// Symbol table helpers
// ---------------------------------------------------------------------------

fn persistent_charsxp_from_bytes(bytes: &[u8]) -> SEXP {
    unsafe {
        use std::alloc::{Layout, alloc};

        let len = bytes.len() as R_xlen_t;
        let total = bytes.len() + 1;
        let Ok(layout) = Layout::from_size_align(total, 1) else {
            return ptr::null_mut();
        };
        let data_ptr = alloc(layout);
        if data_ptr.is_null() {
            return ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data_ptr, bytes.len());
        *data_ptr.add(bytes.len()) = 0;
        // Disown the box immediately: deriving a raw pointer first and then
        // leaking (moving) the box would retag the allocation and invalidate
        // the returned SEXP under Stacked Borrows.
        let charsxp: SEXP = Box::into_raw(Box::new(SexprecCore::new(SEXPTYPE::CHARSXP)));
        (*charsxp).data = SexprecData {
            charsxp_truelen: len,
        };
        (*charsxp).gengc_next_node = data_ptr as SEXP;
        charsxp
    }
}

fn intern_symbol_with_pname<F>(
    symbols: &mut HashMap<String, SEXP>,
    nodes: &mut Vec<*mut SexprecCore>,
    name_str: String,
    make_pname: F,
) -> SEXP
where
    F: FnOnce() -> SEXP,
{
    if let Some(&existing) = symbols.get(&name_str) {
        return existing;
    }

    let pname = make_pname();
    if pname.is_null() {
        return ptr::null_mut();
    }

    // Disown the box immediately so the later `nodes.push` move cannot
    // retag the allocation and invalidate the returned SEXP.
    let sexp: SEXP = Box::into_raw(Box::new(SexprecCore {
        sxpinfo: super::ffi::SxpInfo::new(SEXPTYPE::SYMSXP),
        attrib: ptr::null_mut(),
        gengc_next_node: ptr::null_mut(),
        gengc_prev_node: ptr::null_mut(),
        data: SexprecData {
            symsxp: super::ffi::Symsxp {
                pname,
                value: ptr::null_mut(),
                internal: ptr::null_mut(),
            },
        },
    }));
    symbols.insert(name_str, sexp);
    nodes.push(sexp);
    sexp
}

/// Best-effort reverse lookup: get the interned name for a symbol pointer.
///
/// This avoids dereferencing SYMSXP internals when callers only need the
/// textual name and already have an interned symbol handle.
pub(crate) fn symbol_name_from_ptr(sym: SEXP) -> Option<String> {
    if sym.is_null() {
        return None;
    }
    super::instance::with_current_instance(|inst| symbol_name_from_ptr_in(inst, sym)).flatten()
}

pub(crate) fn symbol_name_from_ptr_in(inst: &mut RInstance, sym: SEXP) -> Option<String> {
    inst.symbols
        .iter()
        .find_map(|(name, &ptr)| if ptr == sym { Some(name.clone()) } else { None })
}

/// Compare two symbols by interned identity first, then by printed name bytes.
///
/// The Rust runtime can still encounter distinct SYMSXP handles with the same
/// printed name while older translated paths are being consolidated. Environment
/// and argument matching must follow R's symbol-name semantics rather than raw
/// allocation identity in those cases.
pub(crate) fn symbol_name_bytes_equal(left: SEXP, right: SEXP) -> bool {
    unsafe {
        if left.is_null() || right.is_null() {
            return false;
        }
        if TYPEOF(left) != SEXPTYPE::SYMSXP || TYPEOF(right) != SEXPTYPE::SYMSXP {
            return false;
        }
        if left == right {
            return true;
        }

        let left_name = PRINTNAME(left);
        let right_name = PRINTNAME(right);
        if left_name.is_null() || right_name.is_null() {
            return false;
        }

        let left_chars = CHAR(left_name);
        let right_chars = CHAR(right_name);
        if left_chars.is_null() || right_chars.is_null() {
            return false;
        }

        CStr::from_ptr(left_chars).to_bytes() == CStr::from_ptr(right_chars).to_bytes()
    }
}

// ---------------------------------------------------------------------------
// Symbol interning
// ---------------------------------------------------------------------------

/// Install a symbol name into the symbol table.
///
/// If the name already exists, returns the existing symbol.
/// Otherwise creates a new SYMSXP node and adds it to the table.
/// This is the equivalent of R's `Rf_install()`.
pub(crate) unsafe fn Rf_install(name: *const c_char) -> SEXP {
    super::instance::with_required_current_instance(|inst| unsafe { Rf_install_in(inst, name) })
}

pub(crate) unsafe fn Rf_install_in(inst: &mut RInstance, name: *const c_char) -> SEXP {
    unsafe {
        if name.is_null() {
            return ptr::null_mut();
        }

        let cstr = std::ffi::CStr::from_ptr(name);
        let name_str = match cstr.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return ptr::null_mut(),
        };

        intern_symbol_with_pname(&mut inst.symbols, &mut inst.symbol_nodes, name_str, || {
            super::constructors::persistent_mkChar(name)
        })
    }
}

/// Install a symbol with a known length (not null-terminated).
pub unsafe fn Rf_installChar(name: *const c_char, len: R_xlen_t) -> SEXP {
    super::instance::with_required_current_instance(|inst| unsafe {
        Rf_installChar_in(inst, name, len)
    })
}

pub(crate) unsafe fn Rf_installChar_in(
    inst: &mut RInstance,
    name: *const c_char,
    len: R_xlen_t,
) -> SEXP {
    if name.is_null() || len < 0 {
        return ptr::null_mut();
    }
    let bytes = unsafe { std::slice::from_raw_parts(name as *const u8, len as usize) };
    let name_str = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => return ptr::null_mut(),
    };

    intern_symbol_with_pname(&mut inst.symbols, &mut inst.symbol_nodes, name_str, || {
        persistent_charsxp_from_bytes(bytes)
    })
}

// ---------------------------------------------------------------------------
// Pre-interned common symbols
// ---------------------------------------------------------------------------

/// Get or create the "base" symbol.
pub unsafe fn R_BaseSymbol() -> SEXP {
    unsafe { Rf_install(c"base".as_ptr()) }
}

/// Get or create the "{ brace" symbol.
pub unsafe fn R_BraceSymbol() -> SEXP {
    unsafe { Rf_install(c"{".as_ptr()) }
}

/// Get or create the "[[" symbol.
pub unsafe fn R_Bracket2Symbol() -> SEXP {
    unsafe { Rf_install(c"[[".as_ptr()) }
}

/// Get or create the "[" symbol.
pub unsafe fn R_BracketSymbol() -> SEXP {
    unsafe { Rf_install(c"[".as_ptr()) }
}

/// Get or create the "function" symbol.
pub unsafe fn R_FunctionSymbol() -> SEXP {
    unsafe { Rf_install(c"function".as_ptr()) }
}

/// Get or create the "while" symbol.
pub unsafe fn R_WhileSymbol() -> SEXP {
    unsafe { Rf_install(c"while".as_ptr()) }
}

/// Get or create the "for" symbol.
pub unsafe fn R_ForSymbol() -> SEXP {
    unsafe { Rf_install(c"for".as_ptr()) }
}

/// Get or create the "if" symbol.
pub unsafe fn R_IfSymbol() -> SEXP {
    unsafe { Rf_install(c"if".as_ptr()) }
}

/// Get or create the "repeat" symbol.
pub unsafe fn R_RepeatSymbol() -> SEXP {
    unsafe { Rf_install(c"repeat".as_ptr()) }
}

/// Get or create the "break" symbol.
pub unsafe fn R_BreakSymbol() -> SEXP {
    unsafe { Rf_install(c"break".as_ptr()) }
}

/// Get or create the "next" symbol.
pub unsafe fn R_NextSymbol() -> SEXP {
    unsafe { Rf_install(c"next".as_ptr()) }
}

/// Get or create the "..." symbol.
pub unsafe fn R_DotsSymbol() -> SEXP {
    unsafe { Rf_install(c"...".as_ptr()) }
}

/// Get or create the "double colon" symbol (::).
pub unsafe fn R_DoubleColonSymbol() -> SEXP {
    unsafe { Rf_install(c"::".as_ptr()) }
}

/// Get or create the "triple colon" symbol (:::).
pub unsafe fn R_TripleColonSymbol() -> SEXP {
    unsafe { Rf_install(c":::".as_ptr()) }
}

/// Get or create the "$" symbol.
pub unsafe fn R_DollarSymbol() -> SEXP {
    unsafe { Rf_install(c"$".as_ptr()) }
}

/// Get or create the "@" symbol.
pub unsafe fn R_AtSymbol() -> SEXP {
    unsafe { Rf_install(c"@".as_ptr()) }
}

/// Get or create the "=" symbol.
pub unsafe fn R_EqSymbol() -> SEXP {
    unsafe { Rf_install(c"=".as_ptr()) }
}

/// Get or create the "<-" symbol.
pub unsafe fn R_LeftAssignSymbol() -> SEXP {
    unsafe { Rf_install(c"<-".as_ptr()) }
}

/// Get or create the "<<" symbol.
pub unsafe fn R_DoubleLeftAssignSymbol() -> SEXP {
    unsafe { Rf_install(c"<<-".as_ptr()) }
}

/// Get or create the "as" symbol.
pub unsafe fn R_AsSymbol() -> SEXP {
    unsafe { Rf_install(c"as".as_ptr()) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::instance::RInstance;
    use crate::sexp::session::RSession;

    fn with_session<F, T>(f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let session = RSession::new();
        session.with_protected(f)
    }

    #[test]
    fn test_install_basic() {
        with_session(|| unsafe {
            let s1 = Rf_install(c"hello".as_ptr());
            assert!(!s1.is_null());
            assert_eq!((*s1).sxpinfo.type_of(), SEXPTYPE::SYMSXP);
        });
    }

    #[test]
    fn test_install_interning() {
        with_session(|| unsafe {
            let s1 = Rf_install(c"myvar".as_ptr());
            let s2 = Rf_install(c"myvar".as_ptr());
            assert_eq!(s1, s2); // Same pointer
        });
    }

    #[test]
    fn test_install_different() {
        with_session(|| unsafe {
            let s1 = Rf_install(c"x".as_ptr());
            let s2 = Rf_install(c"y".as_ptr());
            assert_ne!(s1, s2);
        });
    }

    #[test]
    fn test_session_local_symbol_tables() {
        let left = RSession::new();
        let right = RSession::new();

        let left_a = left.with_active(|| unsafe { Rf_install(c"session_local_symbol".as_ptr()) });
        let left_b = left.with_active(|| unsafe { Rf_install(c"session_local_symbol".as_ptr()) });
        let right_a = right.with_active(|| unsafe { Rf_install(c"session_local_symbol".as_ptr()) });

        assert_eq!(left_a, left_b);
        assert_ne!(left_a, right_a);
    }

    #[test]
    fn test_symbol_table_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        unsafe {
            let left_a = Rf_install_in(&mut left, c"runtime_bound_symbol".as_ptr());
            let left_b = Rf_install_in(&mut left, c"runtime_bound_symbol".as_ptr());
            let right_a = Rf_install_in(&mut right, c"runtime_bound_symbol".as_ptr());

            assert_eq!(left_a, left_b);
            assert_ne!(left_a, right_a);
            assert_eq!(
                symbol_name_from_ptr_in(&mut left, left_a).as_deref(),
                Some("runtime_bound_symbol")
            );
            assert_eq!(symbol_name_from_ptr_in(&mut right, left_a), None);

            let bytes = b"runtime_bound_char_extra";
            let left_char = Rf_installChar_in(&mut left, bytes.as_ptr() as *const c_char, 18);
            let right_char = Rf_installChar_in(&mut right, bytes.as_ptr() as *const c_char, 18);
            assert_ne!(left_char, right_char);
            assert_eq!(
                symbol_name_from_ptr_in(&mut left, left_char).as_deref(),
                Some("runtime_bound_char")
            );
        }
    }

    #[test]
    fn test_install_null() {
        with_session(|| unsafe {
            assert!(Rf_install(ptr::null()).is_null());
        });
    }

    #[test]
    fn test_pre_interned_symbols() {
        with_session(|| unsafe {
            let base = R_BaseSymbol();
            assert!(!base.is_null());
            assert_eq!((*base).sxpinfo.type_of(), SEXPTYPE::SYMSXP);

            let brace = R_BraceSymbol();
            assert!(!brace.is_null());

            // Same symbol should always return same pointer
            let base2 = R_BaseSymbol();
            assert_eq!(base, base2);
        });
    }

    #[test]
    fn test_special_char_symbols() {
        with_session(|| unsafe {
            assert!(!R_BracketSymbol().is_null());
            assert!(!R_Bracket2Symbol().is_null());
            assert!(!R_DollarSymbol().is_null());
            assert!(!R_DotsSymbol().is_null());
            assert!(!R_LeftAssignSymbol().is_null());
        });
    }
}
