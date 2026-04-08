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
use std::sync::{Mutex, OnceLock};

use super::ffi::{R_xlen_t, SEXP, SEXPTYPE, SexprecCore, SexprecData};

// ---------------------------------------------------------------------------
// Global symbol table
// ---------------------------------------------------------------------------

struct SymbolTableInner {
    symbols: HashMap<String, SEXP>,
    #[allow(clippy::vec_box)]
    nodes: Vec<Box<SexprecCore>>,
}

// Safety: SymbolTableInner is protected by Mutex and only accessed
// from code that handles pointers safely.
unsafe impl Send for SymbolTableInner {}
unsafe impl Sync for SymbolTableInner {}

impl SymbolTableInner {
    fn new() -> Self {
        SymbolTableInner {
            symbols: HashMap::new(),
            nodes: Vec::new(),
        }
    }
}

/// Global symbol table, lazily initialized.
static SYMBOL_TABLE: OnceLock<Mutex<SymbolTableInner>> = OnceLock::new();

fn get_symbol_table() -> &'static Mutex<SymbolTableInner> {
    SYMBOL_TABLE.get_or_init(|| Mutex::new(SymbolTableInner::new()))
}

/// Best-effort reverse lookup: get the interned name for a symbol pointer.
///
/// This avoids dereferencing SYMSXP internals when callers only need the
/// textual name and already have an interned symbol handle.
pub(crate) fn symbol_name_from_ptr(sym: SEXP) -> Option<String> {
    if sym.is_null() {
        return None;
    }
    let table = get_symbol_table().lock().unwrap_or_else(|e| e.into_inner());
    table
        .symbols
        .iter()
        .find_map(|(name, &ptr)| if ptr == sym { Some(name.clone()) } else { None })
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

        let mut table = get_symbol_table().lock().expect("lock poisoned");

        if let Some(&existing) = table.symbols.get(&name_str) {
            return existing;
        }

        // Create a CHARSXP for the print name
        let pname = super::constructors::Rf_mkChar(name);
        if pname.is_null() {
            return ptr::null_mut();
        }

        // Create new SYMSXP node
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

        // Store in table
        table.symbols.insert(name_str, sexp);
        table.nodes.push(boxed);

        sexp
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

        let mut table = get_symbol_table().lock().expect("lock poisoned");

        if let Some(&existing) = table.symbols.get(&name_str) {
            return existing;
        }

        // Create a CHARSXP for the print name
        let pname = super::constructors::Rf_mkCharLen(name, len as std::os::raw::c_int);
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

        table.symbols.insert(name_str, sexp);
        table.nodes.push(boxed);

        sexp
    }
}

// ---------------------------------------------------------------------------
// Pre-interned common symbols
// ---------------------------------------------------------------------------

/// Get or create the "base" symbol.
pub unsafe fn R_BaseSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("base")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "{ brace" symbol.
pub unsafe fn R_BraceSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("{")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "[[" symbol.
pub unsafe fn R_Bracket2Symbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("[[")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "[" symbol.
pub unsafe fn R_BracketSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("[")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "function" symbol.
pub unsafe fn R_FunctionSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("function")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "while" symbol.
pub unsafe fn R_WhileSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("while")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "for" symbol.
pub unsafe fn R_ForSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("for")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "if" symbol.
pub unsafe fn R_IfSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("if")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "repeat" symbol.
pub unsafe fn R_RepeatSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("repeat")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "break" symbol.
pub unsafe fn R_BreakSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("break")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "next" symbol.
pub unsafe fn R_NextSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("next")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "..." symbol.
pub unsafe fn R_DotsSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("...")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "double colon" symbol (::).
pub unsafe fn R_DoubleColonSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("::")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "triple colon" symbol (:::).
pub unsafe fn R_TripleColonSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new(":::")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "$" symbol.
pub unsafe fn R_DollarSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("$")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "@" symbol.
pub unsafe fn R_AtSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("@")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "=" symbol.
pub unsafe fn R_EqSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("=")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "<-" symbol.
pub unsafe fn R_LeftAssignSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("<-")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "<<" symbol.
pub unsafe fn R_DoubleLeftAssignSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("<<-")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get or create the "as" symbol.
pub unsafe fn R_AsSymbol() -> SEXP {
    unsafe {
        Rf_install(
            CString::new("as")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_basic() {
        unsafe {
            let s1 = Rf_install(CString::new("hello").unwrap().as_ptr());
            assert!(!s1.is_null());
            assert_eq!((*s1).sxpinfo.type_of(), SEXPTYPE::SYMSXP);
        }
    }

    #[test]
    fn test_install_interning() {
        unsafe {
            let s1 = Rf_install(CString::new("myvar").unwrap().as_ptr());
            let s2 = Rf_install(CString::new("myvar").unwrap().as_ptr());
            assert_eq!(s1, s2); // Same pointer
        }
    }

    #[test]
    fn test_install_different() {
        unsafe {
            let s1 = Rf_install(CString::new("x").unwrap().as_ptr());
            let s2 = Rf_install(CString::new("y").unwrap().as_ptr());
            assert_ne!(s1, s2);
        }
    }

    #[test]
    fn test_install_null() {
        unsafe {
            assert!(Rf_install(ptr::null()).is_null());
        }
    }

    #[test]
    fn test_pre_interned_symbols() {
        unsafe {
            let base = R_BaseSymbol();
            assert!(!base.is_null());
            assert_eq!((*base).sxpinfo.type_of(), SEXPTYPE::SYMSXP);

            let brace = R_BraceSymbol();
            assert!(!brace.is_null());

            // Same symbol should always return same pointer
            let base2 = R_BaseSymbol();
            assert_eq!(base, base2);
        }
    }

    #[test]
    fn test_special_char_symbols() {
        unsafe {
            assert!(!R_BracketSymbol().is_null());
            assert!(!R_Bracket2Symbol().is_null());
            assert!(!R_DollarSymbol().is_null());
            assert!(!R_DotsSymbol().is_null());
            assert!(!R_LeftAssignSymbol().is_null());
        }
    }
}
