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
    pub(crate) buffer: super::owned_line_buffer::OwnedLineBuffer,
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
            strvec: R_NilValue(),
            left: 0,
            buffer: super::owned_line_buffer::OwnedLineBuffer::default(),
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
pub fn isNull(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue()
}

#[inline]
pub fn isSymbol(x: SEXP) -> bool {
    !x.is_null() && TYPEOF(x) == SEXPTYPE::SYMSXP
}

#[inline]
pub fn isLanguage(x: SEXP) -> bool {
    !x.is_null() && TYPEOF(x) == SEXPTYPE::LANGSXP
}

#[inline]
pub fn isList(x: SEXP) -> bool {
    !x.is_null() && TYPEOF(x) == SEXPTYPE::LISTSXP
}

#[inline]
pub fn isString(x: SEXP) -> bool {
    !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP
}

#[inline]
pub fn isVectorAtomic(x: SEXP) -> bool {
    crate::sexp::object::raw_is_atomic_vector(x)
}

/// GNU `gram.y` `isValidName`: identifier characters, then reserved keywords.
/// `...` is a valid name; `.` + digit (`.1`, `.12`) is not.
pub unsafe fn isValidName(s: *const c_char) -> bool {
    unsafe {
        if s.is_null() {
            return false;
        }
        is_valid_r_name_bytes(std::ffi::CStr::from_ptr(s).to_bytes())
    }
}

pub(crate) fn is_valid_r_name_bytes(bytes: &[u8]) -> bool {
    if bytes == b"..." {
        return true;
    }
    let Ok(name) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(crate::mainutils::rlocale::identifier_alpha(first) || first == '.') {
        return false;
    }
    if first == '.' && chars.clone().next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    if !chars.all(|c| crate::mainutils::rlocale::identifier_alnum(c) || c == '.' || c == '_') {
        return false;
    }
    !is_gnu_reserved_name(bytes)
}

fn is_gnu_reserved_name(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        b"NULL"
            | b"NA"
            | b"TRUE"
            | b"FALSE"
            | b"Inf"
            | b"NaN"
            | b"NA_integer_"
            | b"NA_real_"
            | b"NA_character_"
            | b"NA_complex_"
            | b"function"
            | b"while"
            | b"repeat"
            | b"for"
            | b"if"
            | b"in"
            | b"else"
            | b"next"
            | b"break"
    )
}

/// Check if a symbol is a user-defined binary operator (%...%).
pub fn isUserBinop(sym: SEXP) -> bool {
    if !isSymbol(sym) {
        return false;
    }
    let name = char_cstring(PRINTNAME(sym));
    if name.is_none() {
        return false;
    }
    let bytes = name.as_deref().unwrap_or_default().to_bytes();
    if bytes.len() < 3 {
        return false;
    }
    bytes[0] == b'%' && bytes[bytes.len() - 1] == b'%'
}

/// Check if a string is a valid string (non-null, non-NA).
pub fn isValidString(x: SEXP) -> bool {
    isString(x) && LENGTH(x) >= 1 && !STRING_ELT(x, 0).is_null()
}

/// Get PPinfo for a builtin/special function.
/// Returns PPinfo for PP_FUNCALL if the symbol doesn't have a known entry.
pub fn getPPinfo(symval: SEXP) -> PPinfo {
    {
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

pub fn getPPinfo_for_symbol(sym: SEXP) -> Option<PPinfo> {
    fun_tab_entry_for_symbol(sym).map(|entry| entry.pp)
}

/// Resolve the `[` vs `[[` discriminator for a PP_SUBSET call.
///
/// Upstream deparse.c renders the brackets from `PRIMVAL(SYMVALUE(op))` — the
/// `R_FunTab` code of the primitive installed in the call-head symbol's value
/// slot (`installFunTab` binds every funtab name that way). Resolve the same
/// code value-first, then fall back to the funtab entry found by the
/// call-head name, mirroring the name-based PPinfo fallback in deparse2buff
/// (the port's symbol value slots stay unbound unless InitNames ran).
pub fn subset_primval(op: SEXP, symval: SEXP) -> c_int {
    let t = TYPEOF(symval);
    if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
        if let Some(entry) = crate::eval::primitive::fun_tab_descriptor(PRIMOFFSET(symval)) {
            return entry.offset;
        }
    }
    fun_tab_entry_for_symbol(op).map_or(0, |entry| entry.offset)
}

/// Get PPinfo for an argument to needsparens (takes kind/prec/rightassoc directly).
pub fn get_arg_ppinfo(arg: SEXP) -> Option<PPinfo> {
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

/// Check if an SEXP has the S4 object bit set.
pub fn IS_S4_OBJECT(x: SEXP) -> c_int {
    crate::sexp::accessors::checked_s4_object_flag(x)
}

/// Borrow the primitive name without its trailing NUL.
pub(super) fn primitive_name_bytes(op: SEXP) -> &'static [u8] {
    crate::eval::primitive::raw_primitive_entry(op).map_or(b"", |entry| {
        entry.name.strip_suffix(&[0]).unwrap_or(entry.name)
    })
}

fn fun_tab_entry_for_symbol(sym: SEXP) -> Option<&'static crate::mainutils::names::FunTabEntry> {
    if !isSymbol(sym) {
        return None;
    }
    let name = char_cstr_bytes_opt(PRINTNAME(sym))?;
    crate::mainutils::names::R_FunTab
        .iter()
        .find(|entry| entry.name.strip_suffix(&[0]) == Some(name.as_slice()))
}

/// Extract integer from SEXP — delegates to canonical `coerce::asInteger`.
pub fn Rf_asInteger(x: SEXP) -> c_int {
    crate::mainutils::coerce::asInteger(x)
}

/// Extract logical from SEXP — delegates to canonical `coerce::asLogical`.
pub fn Rf_asLogical(x: SEXP) -> c_int {
    crate::mainutils::coerce::asLogical(x)
}

// ---------------------------------------------------------------------------
