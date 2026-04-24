#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Symbol table for R symbol interning.
//!
//! Provides `Rf_install()` which interns symbol names so that each unique
//! name maps to exactly one SexprecCore node. Uses a hash table with
//! separate chaining for collision resolution.

use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr;

use super::ffi::{R_xlen_t, SEXP, SEXPTYPE, SexprecCore, SexprecData};

// ---------------------------------------------------------------------------
// Symbol table helpers
// ---------------------------------------------------------------------------

fn persistent_charsxp_from_bytes(bytes: &[u8]) -> SEXP {
    unsafe {
        use std::alloc::{Layout, alloc};

        let len = bytes.len() as R_xlen_t;
        let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::CHARSXP));
        boxed.data = SexprecData {
            charsxp_truelen: len,
        };
        let charsxp: SEXP = &mut *boxed as *mut _;
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
        (*charsxp).gengc_next_node = data_ptr as SEXP;
        Box::leak(boxed)
    }
}

fn intern_symbol_with_pname<F>(
    symbols: &mut HashMap<String, SEXP>,
    nodes: &mut Vec<Box<SexprecCore>>,
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

    let mut boxed = Box::new(SexprecCore {
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
    });

    let sexp: SEXP = &mut *boxed as *mut _;
    symbols.insert(name_str, sexp);
    nodes.push(boxed);
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
    super::instance::with_current_instance(|inst| {
        inst.symbols
            .iter()
            .find_map(|(name, &ptr)| if ptr == sym { Some(name.clone()) } else { None })
    })
    .flatten()
}

// ---------------------------------------------------------------------------
// Symbol interning
// ---------------------------------------------------------------------------

/// Install a symbol name into the symbol table.
///
/// If the name already exists, returns the existing symbol.
/// Otherwise creates a new SYMSXP node and adds it to the table.
/// This is the equivalent of R's `Rf_install()`.
#[unsafe(no_mangle)]
pub unsafe fn Rf_install(name: *const c_char) -> SEXP {
    unsafe {
        if name.is_null() {
            return ptr::null_mut();
        }

        let cstr = std::ffi::CStr::from_ptr(name);
        let name_str = match cstr.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return ptr::null_mut(),
        };

        super::instance::with_required_current_instance(|inst| {
            intern_symbol_with_pname(
                &mut inst.symbols,
                &mut inst.symbol_nodes,
                name_str.clone(),
                || super::constructors::persistent_mkChar(name),
            )
        })
    }
}

/// Install a symbol with a known length (not null-terminated).
pub unsafe fn Rf_installChar(name: *const c_char, len: R_xlen_t) -> SEXP {
    unsafe {
        if name.is_null() || len < 0 {
            return ptr::null_mut();
        }
        let bytes = std::slice::from_raw_parts(name as *const u8, len as usize);
        let name_str = match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => return ptr::null_mut(),
        };

        super::instance::with_required_current_instance(|inst| {
            intern_symbol_with_pname(
                &mut inst.symbols,
                &mut inst.symbol_nodes,
                name_str.clone(),
                || persistent_charsxp_from_bytes(bytes),
            )
        })
    }
}

// ---------------------------------------------------------------------------
// Pre-interned common symbols
// ---------------------------------------------------------------------------

/// Get or create the "base" symbol.
pub unsafe fn R_BaseSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("base").unwrap_or_default().as_ptr()) }
}

/// Get or create the "{ brace" symbol.
pub unsafe fn R_BraceSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("{").unwrap_or_default().as_ptr()) }
}

/// Get or create the "[[" symbol.
pub unsafe fn R_Bracket2Symbol() -> SEXP {
    unsafe { Rf_install(CString::new("[[").unwrap_or_default().as_ptr()) }
}

/// Get or create the "[" symbol.
pub unsafe fn R_BracketSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("[").unwrap_or_default().as_ptr()) }
}

/// Get or create the "function" symbol.
pub unsafe fn R_FunctionSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("function").unwrap_or_default().as_ptr()) }
}

/// Get or create the "while" symbol.
pub unsafe fn R_WhileSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("while").unwrap_or_default().as_ptr()) }
}

/// Get or create the "for" symbol.
pub unsafe fn R_ForSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("for").unwrap_or_default().as_ptr()) }
}

/// Get or create the "if" symbol.
pub unsafe fn R_IfSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("if").unwrap_or_default().as_ptr()) }
}

/// Get or create the "repeat" symbol.
pub unsafe fn R_RepeatSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("repeat").unwrap_or_default().as_ptr()) }
}

/// Get or create the "break" symbol.
pub unsafe fn R_BreakSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("break").unwrap_or_default().as_ptr()) }
}

/// Get or create the "next" symbol.
pub unsafe fn R_NextSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("next").unwrap_or_default().as_ptr()) }
}

/// Get or create the "..." symbol.
pub unsafe fn R_DotsSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("...").unwrap_or_default().as_ptr()) }
}

/// Get or create the "double colon" symbol (::).
pub unsafe fn R_DoubleColonSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("::").unwrap_or_default().as_ptr()) }
}

/// Get or create the "triple colon" symbol (:::).
pub unsafe fn R_TripleColonSymbol() -> SEXP {
    unsafe { Rf_install(CString::new(":::").unwrap_or_default().as_ptr()) }
}

/// Get or create the "$" symbol.
pub unsafe fn R_DollarSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("$").unwrap_or_default().as_ptr()) }
}

/// Get or create the "@" symbol.
pub unsafe fn R_AtSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("@").unwrap_or_default().as_ptr()) }
}

/// Get or create the "=" symbol.
pub unsafe fn R_EqSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("=").unwrap_or_default().as_ptr()) }
}

/// Get or create the "<-" symbol.
pub unsafe fn R_LeftAssignSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("<-").unwrap_or_default().as_ptr()) }
}

/// Get or create the "<<" symbol.
pub unsafe fn R_DoubleLeftAssignSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("<<-").unwrap_or_default().as_ptr()) }
}

/// Get or create the "as" symbol.
pub unsafe fn R_AsSymbol() -> SEXP {
    unsafe { Rf_install(CString::new("as").unwrap_or_default().as_ptr()) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
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
            let s1 = Rf_install(CString::new("hello").unwrap_or_default().as_ptr());
            assert!(!s1.is_null());
            assert_eq!((*s1).sxpinfo.type_of(), SEXPTYPE::SYMSXP);
        });
    }

    #[test]
    fn test_install_interning() {
        with_session(|| unsafe {
            let s1 = Rf_install(CString::new("myvar").unwrap_or_default().as_ptr());
            let s2 = Rf_install(CString::new("myvar").unwrap_or_default().as_ptr());
            assert_eq!(s1, s2); // Same pointer
        });
    }

    #[test]
    fn test_install_different() {
        with_session(|| unsafe {
            let s1 = Rf_install(CString::new("x").unwrap_or_default().as_ptr());
            let s2 = Rf_install(CString::new("y").unwrap_or_default().as_ptr());
            assert_ne!(s1, s2);
        });
    }

    #[test]
    fn test_session_local_symbol_tables() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        let left_a = left
            .with_arena(|_| unsafe {
                Rf_install(CString::new("session_local_symbol").unwrap().as_ptr())
            })
            .unwrap();
        let left_b = left
            .with_arena(|_| unsafe {
                Rf_install(CString::new("session_local_symbol").unwrap().as_ptr())
            })
            .unwrap();
        let right_a = right
            .with_arena(|_| unsafe {
                Rf_install(CString::new("session_local_symbol").unwrap().as_ptr())
            })
            .unwrap();

        assert_eq!(left_a, left_b);
        assert_ne!(left_a, right_a);
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
