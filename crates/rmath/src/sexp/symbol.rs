#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Symbol table for R symbol interning.
//!
//! Provides `Rf_install()` which interns symbol names so that each unique
//! name maps to exactly one SexprecCore node. Uses a hash table with
//! separate chaining for collision resolution.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::ptr;

use super::accessors::PRINTNAME;
use super::ffi::{NodeBody, R_xlen_t, SEXP, SEXPTYPE, SexprecCore};
use super::instance::RInstance;

// GNU R limits symbol names by bytes, independently of the session budget.
const MAX_SYMBOL_BYTES: usize = 10000;

fn check_symbol_length(bytes: usize) {
    if bytes > MAX_SYMBOL_BYTES {
        super::context::r_error("variable names are limited to 10000 bytes");
    }
}

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

/// Intern a Rust `&str` in the active session's symbol table.
///
/// Checked spelling of [`Rf_install`] for callers that already hold UTF-8:
/// the ambient entry pins the original instance, so no unsafe contract is
/// left for the caller to uphold.
pub(crate) fn install_str(name: &str) -> SEXP {
    super::instance::with_required_current_instance(|inst| unsafe { install_str_in(inst, name) })
}

/// Intern a C string; safe spelling of [`Rf_install`] for Rust-owned
/// `CStr`/`CString` values. Non-UTF-8 names return null, like `Rf_install`.
pub(crate) fn install_cstr(name: &std::ffi::CStr) -> SEXP {
    match name.to_str() {
        Ok(name) => install_str(name),
        Err(_) => ptr::null_mut(),
    }
}

/// Safe spelling of `Rf_install(CHAR(x))`: null where `CHAR` would return
/// null or the name is not UTF-8, like `Rf_install`.
pub(crate) fn install_charsxp(x: SEXP) -> SEXP {
    match super::accessors::char_cstr_bytes_opt(x).and_then(|bytes| String::from_utf8(bytes).ok()) {
        Some(name) => install_str(&name),
        None => ptr::null_mut(),
    }
}

/// Intern `name` in `inst`'s symbol table, returning the existing SYMSXP
/// when the name was already installed.
///
/// # Safety
/// `inst` must point at a live [`RInstance`] whose owner stays active across
/// this strictly-local table update; no callback or allocation reentry may
/// overlap the field lends.
pub(crate) unsafe fn install_str_in(inst: *mut RInstance, name: &str) -> SEXP {
    check_symbol_length(name.len());
    // P1/P2: strictly-local field lends; no interpreter callback runs during
    // these owned allocations.
    unsafe {
        if let Some(&existing) = (*inst).symbols.get(name) {
            return existing;
        }
        intern_symbol(
            &mut (*inst).symbols,
            &mut (*inst).symbol_nodes,
            &mut (*inst).persistent_nodes,
            name.to_string(),
        )
    }
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
    // SAFETY: `inst` is the live current-instance pointer handed to this accessor closure (or owned by this caller) for the duration of the call.
    super::instance::with_current_instance(|inst| unsafe { symbol_name_from_ptr_in(inst, sym) })
        .flatten()
}

/// # Safety
///
/// `inst`/`instance` must point to a live `RInstance` (as supplied by the instance accessors) with
/// no conflicting outstanding borrow of the fields touched here.
pub(crate) unsafe fn symbol_name_from_ptr_in(inst: *mut RInstance, sym: SEXP) -> Option<String> {
    // P2: read-only scan of one field; no ambient write intervenes.
    unsafe {
        (*inst)
            .symbols
            .iter()
            .find_map(|(name, &ptr)| if ptr == sym { Some(name.clone()) } else { None })
    }
}

pub(crate) use super::accessors::symbol_name_bytes_equal;

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
    if name.is_null() {
        return ptr::null_mut();
    }
    let name_str = match unsafe { std::ffi::CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    unsafe { install_str_in(inst, name_str) }
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
    check_symbol_length(length);
    let bytes = unsafe { std::slice::from_raw_parts(name as *const u8, length) };
    let Ok(name_str) = std::str::from_utf8(bytes) else {
        return ptr::null_mut();
    };
    unsafe { install_str_in(inst, name_str) }
}

// ---------------------------------------------------------------------------
// Pre-interned common symbols
// ---------------------------------------------------------------------------

/// Get or create the "base" symbol.
pub fn R_BaseSymbol() -> SEXP {
    install_str("base")
}

/// Get or create the "{ brace" symbol.
pub fn R_BraceSymbol() -> SEXP {
    install_str("{")
}

/// Get or create the "[[" symbol.
pub fn R_Bracket2Symbol() -> SEXP {
    install_str("[[")
}

/// Get or create the "[" symbol.
pub fn R_BracketSymbol() -> SEXP {
    install_str("[")
}


/// Get or create the "while" symbol.
pub fn R_WhileSymbol() -> SEXP {
    install_str("while")
}

/// Get or create the "for" symbol.
pub fn R_ForSymbol() -> SEXP {
    install_str("for")
}

/// Get or create the "if" symbol.
pub fn R_IfSymbol() -> SEXP {
    install_str("if")
}

/// Get or create the "repeat" symbol.
pub fn R_RepeatSymbol() -> SEXP {
    install_str("repeat")
}



/// Get or create the "..." symbol.
pub fn R_DotsSymbol() -> SEXP {
    install_str("...")
}

/// Get or create the "double colon" symbol (::).
pub fn R_DoubleColonSymbol() -> SEXP {
    install_str("::")
}

/// Get or create the "triple colon" symbol (:::).
pub fn R_TripleColonSymbol() -> SEXP {
    install_str(":::")
}

/// Get or create the "$" symbol.
pub fn R_DollarSymbol() -> SEXP {
    install_str("$")
}



/// Get or create the "<-" symbol.
pub fn R_LeftAssignSymbol() -> SEXP {
    install_str("<-")
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

    fn rejected_symbol_length(text: &str, known_length: bool, bounded: bool) {
        let mut instance = RInstance::new_for_gc_tests();
        let bytes = instance.arena.total_bytes_allocated();
        let nodes = instance.persistent_nodes.len();
        if bounded {
            instance
                .arena
                .set_budget(crate::sexp::memory::ArenaBudget::new(bytes + 1, 0));
        }
        let name = std::ffi::CString::new(text).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            if known_length {
                Rf_installChar_in(&mut instance, name.as_ptr(), text.len() as i64)
            } else {
                Rf_install_in(&mut instance, name.as_ptr())
            }
        }));
        let failure = result.expect_err("oversized symbol was installed");
        assert_eq!(
            failure
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            "variable names are limited to 10000 bytes"
        );
        assert_eq!(instance.persistent_nodes.len(), nodes);
        assert_eq!(instance.arena.total_bytes_allocated(), bytes);
        assert_eq!(instance.arena.workspace_bytes(), 0);
        assert!(instance.symbols.is_empty());
    }

    #[test]
    fn symbol_length_cstring_rejects_10001_bytes_before_publication() {
        rejected_symbol_length(&"x".repeat(10001), false, false);
    }
    #[test]
    fn symbol_length_explicit_rejects_10001_bytes_before_publication() {
        rejected_symbol_length(&"x".repeat(10001), true, false);
    }
    #[test]
    fn symbol_length_limit_counts_utf8_bytes_and_precedes_memory_admission() {
        for known_length in [false, true] {
            rejected_symbol_length(&"é".repeat(5001), known_length, true);
        }
    }
    #[test]
    fn symbol_length_10000_byte_boundary_is_valid_and_reuses_both_entries() {
        let mut instance = RInstance::new_for_gc_tests();
        for text in ["x".repeat(10000), "é".repeat(5000)] {
            let name = std::ffi::CString::new(text).unwrap();
            let first = unsafe { Rf_install_in(&mut instance, name.as_ptr()) };
            assert!(!first.is_null());
            let before = instance.arena.total_bytes_allocated();
            let again = unsafe { Rf_installChar_in(&mut instance, name.as_ptr(), 10000) };
            assert_eq!(again, first);
            assert_eq!(instance.arena.total_bytes_allocated(), before);
        }
    }

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
            let s1 = install_str("hello");
            assert!(!s1.is_null());
            assert_eq!((*s1).sxpinfo.type_of(), SEXPTYPE::SYMSXP);
        });
    }

    #[test]
    fn test_install_interning() {
        with_session(|| unsafe {
            let s1 = install_str("myvar");
            let s2 = install_str("myvar");
            assert_eq!(s1, s2); // Same pointer
        });
    }

    #[test]
    fn test_install_different() {
        with_session(|| unsafe {
            let s1 = install_str("x");
            let s2 = install_str("y");
            assert_ne!(s1, s2);
        });
    }

    #[test]
    fn test_session_local_symbol_tables() {
        let left = RSession::new();
        let right = RSession::new();

        let left_a = left.with_active(|| unsafe { install_str("session_local_symbol") });
        let left_b = left.with_active(|| unsafe { install_str("session_local_symbol") });
        let right_a = right.with_active(|| unsafe { install_str("session_local_symbol") });

        assert_eq!(left_a, left_b);
        assert_ne!(left_a, right_a);
    }

    #[test]
    fn symbol_comparison_checks_each_projection_once() {
        use crate::sexp::memory::projection_directory_probe_count;

        let _session = RSession::new_for_gc_tests();
        let left = unsafe { install_str("left_symbol_name") };
        let right = unsafe { install_str("right_symbol_name") };

        let before = projection_directory_probe_count();
        assert!(symbol_name_bytes_equal(left, left));
        assert_eq!(projection_directory_probe_count() - before, 1);

        let before = projection_directory_probe_count();
        assert!(!symbol_name_bytes_equal(left, right));
        // Two symbol headers and two sealed character projections.
        assert_eq!(projection_directory_probe_count() - before, 4);
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
                let name = PRINTNAME(install_str("shared_decoded_name"));
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
            let name = PRINTNAME(install_str("changed_decoded_name"));
            assert_eq!(
                interned_ascii_character(b"changed_decoded_name"),
                Some(name)
            );
            crate::sexp::accessors::SETLEVELS(name, 1);
            assert_eq!(interned_ascii_character(b"changed_decoded_name"), None);
            install_str("café");
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
