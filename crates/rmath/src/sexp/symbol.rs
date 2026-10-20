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
use super::ffi::{NodeBody, R_xlen_t, SEXP, SEXPTYPE, SexprecCore};
use super::instance::RInstance;

// ---------------------------------------------------------------------------
// Symbol table helpers
// ---------------------------------------------------------------------------

/// Own a terminated character header and its bytes in the original session.
/// # Safety
/// The owner must be live and no callback may overlap these short field lends.
pub(crate) unsafe fn persistent_charsxp_from_bytes_in(inst: *mut RInstance, bytes: &[u8]) -> SEXP {
    unsafe { (*inst).symbol_nodes.try_reserve(1) }.unwrap_or_else(|_| symbol_allocation_error());
    let value = unsafe { (*inst).persistent_nodes.allocate_chars(bytes) }
        .unwrap_or_else(|_| symbol_allocation_error());
    unsafe { (*inst).symbol_nodes.push(value) };
    value
}

fn symbol_allocation_error() -> ! {
    super::context::r_error("permanent symbol allocation exceeds session memory limit")
}

fn intern_symbol(
    symbols: &mut HashMap<String, SEXP>,
    nodes: &mut Vec<*mut SexprecCore>,
    persistent: &mut super::instance::persistent::PersistentHeap,
    name_str: String,
) -> SEXP {
    if let Some(&existing) = symbols.get(&name_str) {
        return existing;
    }
    symbols
        .try_reserve(1)
        .unwrap_or_else(|_| symbol_allocation_error());
    nodes
        .try_reserve(2)
        .unwrap_or_else(|_| symbol_allocation_error());
    let (pname, symbol) = persistent
        .allocate_symbol(name_str.as_bytes())
        .unwrap_or_else(|_| symbol_allocation_error());
    symbols.insert(name_str, symbol);
    nodes.extend([pname, symbol]);
    symbol
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

pub(crate) fn symbol_name_from_ptr_in(inst: *mut RInstance, sym: SEXP) -> Option<String> {
    // P2: read-only scan of one field; no ambient write intervenes.
    unsafe {
        (*inst)
            .symbols
            .iter()
            .find_map(|(name, &ptr)| if ptr == sym { Some(name.clone()) } else { None })
    }
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

/// Reuse a character already retained by this interpreter's symbol table.
/// This adds no permanent allocations and grants no authority through a byte
/// key: the original checked header and immutable payload must still agree.
pub(crate) fn interned_ascii_character(bytes: &[u8]) -> Option<SEXP> {
    if !bytes.is_ascii() {
        return None;
    }
    let name = std::str::from_utf8(bytes).ok()?;
    super::instance::with_current_instance(|instance| unsafe {
        // The ambient entry pins this original instance; only its symbol-map
        // field is borrowed, and no allocation or callback occurs here.
        let symbol = *(*instance).symbols.get(name)?;
        let pointer = PRINTNAME(symbol);
        let value = super::object::Sexp::try_from_raw(pointer).ok()?;
        let header = value.header();
        let NodeBody::Vector(vector) = header.body else {
            return None;
        };
        (header.sxpinfo.type_of() == SEXPTYPE::CHARSXP
            && header.sxpinfo.gp() == 1 << 6
            && !header.sxpinfo.alt()
            && !header.sxpinfo.obj()
            && header.attrib.is_null()
            && vector.truelength == 0
            && matches!(vector.metadata, super::ffi::VectorMetadata::None)
            && header
                .payload_lease()
                .is_some_and(|payload| payload.is_immutable())
            && header.char_eq(bytes))
        .then_some(pointer)
    })
    .flatten()
}

/// Install a symbol name into the symbol table.
///
/// If the name already exists, returns the existing symbol.
/// Otherwise creates a new SYMSXP node and adds it to the table.
/// This is the equivalent of R's `Rf_install()`.
pub(crate) unsafe fn Rf_install(name: *const c_char) -> SEXP {
    super::instance::with_required_current_instance(|inst| unsafe { Rf_install_in(inst, name) })
}

pub(crate) unsafe fn Rf_install_in(inst: *mut RInstance, name: *const c_char) -> SEXP {
    unsafe {
        if name.is_null() {
            return ptr::null_mut();
        }

        let cstr = std::ffi::CStr::from_ptr(name);
        let name_str = match cstr.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return ptr::null_mut(),
        };

        if let Some(&existing) = (*inst).symbols.get(&name_str) {
            return existing;
        }
        // The safe allocator publishes the name and symbol together.
        intern_symbol(
            &mut (*inst).symbols,
            &mut (*inst).symbol_nodes,
            &mut (*inst).persistent_nodes,
            name_str,
        )
    }
}

/// Install a symbol with a known length (not null-terminated).
pub unsafe fn Rf_installChar(name: *const c_char, len: R_xlen_t) -> SEXP {
    super::instance::with_required_current_instance(|inst| unsafe {
        Rf_installChar_in(inst, name, len)
    })
}

pub(crate) unsafe fn Rf_installChar_in(
    inst: *mut RInstance,
    name: *const c_char,
    len: R_xlen_t,
) -> SEXP {
    if name.is_null() || len < 0 {
        return ptr::null_mut();
    }
    let Ok(length) = usize::try_from(len) else {
        return ptr::null_mut();
    };
    let bytes = unsafe { std::slice::from_raw_parts(name as *const u8, length) };
    let name_str = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => return ptr::null_mut(),
    };

    // No interpreter callback runs during these owned allocations.
    unsafe {
        if let Some(&existing) = (*inst).symbols.get(&name_str) {
            return existing;
        }
        intern_symbol(
            &mut (*inst).symbols,
            &mut (*inst).symbol_nodes,
            &mut (*inst).persistent_nodes,
            name_str,
        )
    }
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
    use std::ptr::addr_of_mut;

    use super::*;
    use crate::sexp::instance::RInstance;
    use crate::sexp::session::RSession;

    #[test]
    fn permanent_budget_symbol_denial_is_atomic_and_recovers_under_same_cap() {
        let mut instance = RInstance::new_for_gc_tests();
        let before = instance.arena.total_bytes_allocated();
        let nodes = instance.persistent_nodes.len();
        let width = std::mem::size_of::<SexprecCore>();
        let name = std::ffi::CString::new(vec![b'x'; width + 31]).unwrap();
        let limit = before + 2 * width + name.as_bytes_with_nul().len();
        instance
            .arena
            .set_budget(crate::sexp::memory::ArenaBudget::new(limit, 0));
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            Rf_install_in(&mut instance, name.as_ptr())
        }));
        assert!(
            denied.is_err(),
            "permanent symbol admission must report a resource error, not return NULL"
        );
        assert_eq!(instance.persistent_nodes.len(), nodes);
        assert!(instance.symbol_nodes.is_empty());
        assert!(instance.symbols.is_empty());
        assert_eq!(instance.arena.total_bytes_allocated(), before);
        let short = unsafe { Rf_install_in(&mut instance, c"s".as_ptr()) };
        assert!(!short.is_null());
        assert_eq!(instance.arena.budget().max_bytes, limit);
        assert_eq!(instance.arena.workspace_bytes(), 0);
    }

    #[test]
    fn permanent_budget_existing_symbol_reuse_requires_no_headroom() {
        let mut instance = RInstance::new_for_gc_tests();
        let first = unsafe { Rf_install_in(&mut instance, c"retained_symbol".as_ptr()) };
        let before = instance.arena.total_bytes_allocated();
        instance
            .arena
            .set_budget(crate::sexp::memory::ArenaBudget::new(before - 1, 0));
        let second = unsafe { Rf_install_in(&mut instance, c"retained_symbol".as_ptr()) };
        assert_eq!(first, second);
        assert_eq!(instance.arena.total_bytes_allocated(), before);
    }

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
    fn canonical_character_decode_reuses_only_its_original_interpreters_symbol_name() {
        let left = RSession::new_for_gc_tests();
        let right = RSession::new_for_gc_tests();
        let decode = |session: &RSession| {
            session.with_active(|| unsafe {
                use crate::sexp::{
                    accessors::STRING_ELT, constructors::Rf_ScalarString, globals::R_NilValue,
                    protect::protect,
                };
                let name = PRINTNAME(Rf_install(c"shared_decoded_name".as_ptr()));
                let source = Rf_ScalarString(name);
                let _source = protect(source);
                let wire = crate::mainutils::serialize::R_serialize(
                    source,
                    R_NilValue(),
                    R_NilValue(),
                    R_NilValue(),
                    R_NilValue(),
                );
                let _wire = protect(wire);
                let decoded = crate::mainutils::serialize::R_unserialize(wire, R_NilValue());
                assert_eq!(STRING_ELT(decoded, 0), name);
                name
            })
        };
        let left_name = decode(&left);
        let right_name = decode(&right);
        assert_ne!(left_name, right_name);
    }

    #[test]
    fn canonical_character_lookup_rejects_modified_headers_and_non_ascii_names() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let name = PRINTNAME(Rf_install(c"changed_decoded_name".as_ptr()));
            assert_eq!(
                interned_ascii_character(b"changed_decoded_name"),
                Some(name)
            );
            crate::sexp::accessors::SETLEVELS(name, 1);
            assert_eq!(interned_ascii_character(b"changed_decoded_name"), None);
            Rf_install(c"café".as_ptr());
            assert_eq!(interned_ascii_character("café".as_bytes()), None);
            assert_eq!(interned_ascii_character(b"not_interned"), None);
        });
    }

    #[test]
    fn test_symbol_table_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        unsafe {
            let left_a = Rf_install_in(addr_of_mut!(left), c"runtime_bound_symbol".as_ptr());
            let left_b = Rf_install_in(addr_of_mut!(left), c"runtime_bound_symbol".as_ptr());
            let right_a = Rf_install_in(addr_of_mut!(right), c"runtime_bound_symbol".as_ptr());

            assert_eq!(left_a, left_b);
            assert_ne!(left_a, right_a);
            assert_eq!(
                symbol_name_from_ptr_in(addr_of_mut!(left), left_a).as_deref(),
                Some("runtime_bound_symbol")
            );
            assert_eq!(symbol_name_from_ptr_in(addr_of_mut!(right), left_a), None);

            let bytes = b"runtime_bound_char_extra";
            let left_char =
                Rf_installChar_in(addr_of_mut!(left), bytes.as_ptr() as *const c_char, 18);
            let right_char =
                Rf_installChar_in(addr_of_mut!(right), bytes.as_ptr() as *const c_char, 18);
            assert_ne!(left_char, right_char);
            assert_eq!(
                symbol_name_from_ptr_in(addr_of_mut!(left), left_char).as_deref(),
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
