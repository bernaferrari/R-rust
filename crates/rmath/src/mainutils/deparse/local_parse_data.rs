#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;

// ---------------------------------------------------------------------------
// LocalParseData — carries all state across recursive deparse calls
// ---------------------------------------------------------------------------

/// Local parse data struct for deparsing (equivalent to C's LocalParseData).
///
/// This holds all the state needed during recursive deparsing: line tracking,
/// buffer management, indentation, and option flags.
pub struct LocalParseData {
    /// Current line number being written.
    pub linenumber: c_int,
    /// Length of the current line buffer content.
    pub len: c_int,
    /// Whether we are inside curly braces.
    pub incurly: c_int,
    /// Whether we are inside a list.
    pub inlist: c_int,
    /// Whether we are at the start of a new line.
    pub startline: bool,
    /// Current indentation level (number of tabs).
    pub indent: c_int,
    /// String vector being built (R_NilValue when just counting).
    pub strvec: SEXP,
    /// Left-side precedence tracking for parenthesization.
    pub left: c_int,
    /// The string buffer for building the current line.
    pub buffer: R_StringBuffer,
    /// Line width cutoff.
    pub cutoff: c_int,
    /// Whether to use backticks for non-standard names.
    pub backtick: c_int,
    /// Deparse option flags.
    pub opts: c_int,
    /// Whether the result is source()-able.
    pub sourceable: c_int,
    /// Maximum number of lines to deparse.
    pub maxlines: c_int,
    /// Whether deparsing is still active (false after maxlines reached).
    pub active: bool,
    /// Whether an S4 object was encountered.
    pub isS4: c_int,
    /// Whether we are in a function argument context.
    pub fnarg: bool,
}

impl Default for LocalParseData {
    fn default() -> Self {
        LocalParseData {
            linenumber: 0,
            len: 0,
            incurly: 0,
            inlist: 0,
            startline: true,
            indent: 0,
            strvec: unsafe { R_NilValue() },
            left: 0,
            buffer: R_StringBuffer {
                data: ptr::null_mut(),
                bufsize: 0,
                defaultSize: BUFSIZE as usize,
            },
            cutoff: DEFAULT_CUTOFF,
            backtick: 0,
            opts: 0,
            sourceable: 1,
            maxlines: c_int::MAX,
            active: true,
            isS4: 0,
            fnarg: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: isNull, isSymbol, isLanguage, isList, isString, isVectorAtomic
// ---------------------------------------------------------------------------

#[inline]
pub unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

#[inline]
pub unsafe fn isSymbol(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::SYMSXP }
}

#[inline]
pub unsafe fn isLanguage(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::LANGSXP }
}

#[inline]
pub unsafe fn isList(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::LISTSXP }
}

#[inline]
pub unsafe fn isString(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP }
}

#[inline]
pub fn isVectorAtomic(x: SEXP) -> bool {
    crate::sexp::object::raw_is_atomic_vector(x)
}

/// Check if a name is a valid R identifier.
/// R identifiers must start with [a-zA-Z.] and contain only [a-zA-Z0-9._].
pub unsafe fn isValidName(s: *const c_char) -> bool {
    unsafe {
        if s.is_null() {
            return false;
        }
        let bytes = std::ffi::CStr::from_ptr(s).to_bytes();
        if bytes.is_empty() {
            return false;
        }
        let mut i = 0;
        // First char: letter, dot, or underscore-like patterns
        let first = bytes[0];
        if !((first >= b'a' && first <= b'z') || (first >= b'A' && first <= b'Z') || first == b'.')
        {
            return false;
        }
        i = 1;
        while i < bytes.len() {
            let c = bytes[i];
            if !((c >= b'a' && c <= b'z')
                || (c >= b'A' && c <= b'Z')
                || (c >= b'0' && c <= b'9')
                || c == b'.'
                || c == b'_')
            {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// Check if a symbol is a user-defined binary operator (%...%).
pub unsafe fn isUserBinop(sym: SEXP) -> bool {
    unsafe {
        if !isSymbol(sym) {
            return false;
        }
        let name = CHAR(PRINTNAME(sym));
        if name.is_null() {
            return false;
        }
        let bytes = std::ffi::CStr::from_ptr(name).to_bytes();
        if bytes.len() < 3 {
            return false;
        }
        bytes[0] == b'%' && bytes[bytes.len() - 1] == b'%'
    }
}

/// Check if a string is a valid string (non-null, non-NA).
pub unsafe fn isValidString(x: SEXP) -> bool {
    unsafe { isString(x) && LENGTH(x) >= 1 && !STRING_ELT(x, 0).is_null() }
}

/// Get PPinfo for a builtin/special function.
/// Returns PPinfo for PP_FUNCALL if the symbol doesn't have a known entry.
pub unsafe fn getPPinfo(symval: SEXP) -> PPinfo {
    unsafe {
        let t = TYPEOF(symval);
        if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            // Try to find the primitive in R_FunTab by PRIMOFFSET
            let offset = PRIMOFFSET(symval);
            if offset >= 0 {
                // Look up in the function table
                let funtab = crate::mainutils::names::R_FunTab;
                if (offset as usize) < funtab.len() {
                    return funtab[offset as usize].pp;
                }
            }
        }
        PPinfo::new(PP_FUNCALL, 0, 0)
    }
}

pub unsafe fn getPPinfo_for_symbol(sym: SEXP) -> Option<PPinfo> {
    unsafe {
        if !isSymbol(sym) {
            return None;
        }
        let name = CHAR(PRINTNAME(sym));
        if name.is_null() {
            return None;
        }
        let name_bytes = std::ffi::CStr::from_ptr(name).to_bytes_with_nul();
        crate::mainutils::names::R_FunTab
            .iter()
            .find(|entry| entry.name == name_bytes)
            .map(|entry| entry.pp)
    }
}

/// Resolve the `[` vs `[[` discriminator for a PP_SUBSET call.
///
/// Upstream deparse.c renders the brackets from `PRIMVAL(SYMVALUE(op))` — the
/// `R_FunTab` code of the primitive installed in the call-head symbol's value
/// slot (`installFunTab` binds every funtab name that way). Resolve the same
/// code value-first, then fall back to the funtab entry found by the
/// call-head name, mirroring the name-based PPinfo fallback in deparse2buff
/// (the port's symbol value slots stay unbound unless InitNames ran).
pub unsafe fn subset_primval(op: SEXP, symval: SEXP) -> c_int {
    unsafe {
        let t = TYPEOF(symval);
        if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            if let Some(entry) = crate::eval::primitive::fun_tab_descriptor(PRIMOFFSET(symval)) {
                return entry.offset;
            }
        }
        if isSymbol(op) {
            let pn = PRINTNAME(op);
            if !pn.is_null() {
                let name = CHAR(pn);
                if !name.is_null() {
                    let name_bytes = std::ffi::CStr::from_ptr(name).to_bytes_with_nul();
                    if let Some(entry) = crate::mainutils::names::R_FunTab
                        .iter()
                        .find(|entry| entry.name == name_bytes)
                    {
                        return entry.offset;
                    }
                }
            }
        }
        0
    }
}

/// Get PPinfo for an argument to needsparens (takes kind/prec/rightassoc directly).
pub unsafe fn get_arg_ppinfo(arg: SEXP) -> Option<PPinfo> {
    unsafe {
        if TYPEOF(arg) != SEXPTYPE::LANGSXP {
            return None;
        }
        let op = CAR(arg);
        if !isSymbol(op) {
            return None;
        }
        let symval = SYMVALUE(op);
        let t = TYPEOF(symval);
        if t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
            return getPPinfo_for_symbol(op);
        }
        Some(getPPinfo(symval))
    }
}

pub const S4_OBJECT_MASK: u16 = 1 << 4;

/// Check if an SEXP has the S4 object bit set.
pub unsafe fn IS_S4_OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.gp() & S4_OBJECT_MASK) != 0) as c_int
    }
}

/// streql: compare two C strings for equality.
pub unsafe fn streql(a: *const c_char, b: *const c_char) -> bool {
    unsafe {
        if a.is_null() || b.is_null() {
            return false;
        }
        std::ffi::CStr::from_ptr(a) == std::ffi::CStr::from_ptr(b)
    }
}

/// Get PRIMNAME as a C string (avoids conflict with eval/builtin.rs version).
pub unsafe fn primname_c(op: SEXP) -> *const c_char {
    unsafe {
        // Use the relop.rs version which returns *const c_char
        crate::mainutils::relop::PRIMNAME(op)
    }
}

/// Extract integer from SEXP — delegates to canonical `coerce::asInteger`.
pub unsafe fn Rf_asInteger(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asInteger(x) }
}

/// Extract logical from SEXP — delegates to canonical `coerce::asLogical`.
pub unsafe fn Rf_asLogical(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asLogical(x) }
}

// ---------------------------------------------------------------------------
