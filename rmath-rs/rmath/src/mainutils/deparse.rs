#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/deparse.c (1,962 lines) — deparse, dput, dump.
//!
//! Deparsing has 3 layers:
//! - User interfaces: do_deparse(), do_dput(), do_dump() (should not be called
//!   from internal functions).
//! - The actual deparsing via deparse2() needs to be done twice (once to count
//!   lines, once to fill the string vector), unless nlines > 0.
//! - Printing to a file is handled by the calling routine.
//!
//! Current call paths:
//!   do_deparse() ------------> deparse1WithCutoff()
//!   do_dput() -> deparse1() -> deparse1WithCutoff()
//!   do_dump() -> deparse1() -> deparse1WithCutoff()
//!
//! Workhorse: deparse1WithCutoff() -> deparse2() -> deparse2buff()
//!
//! Porting status:
//! - Full implementation of deparse2buff, deparse2, print2buff, writeline,
//!   linebreak, printtab2buff, args2buff, vector2buff, vec2buff.
//! - deparse1WithCutoff, deparse1, deparse1w, deparse1line, deparse1s, deparse1m.
//! - do_deparse with argument extraction.
//! - Helper functions: curlyahead, needsparens, quotify, etc.
//! - S4 deparsing, source reference deparsing kept as stubs (need eval/methods).
//! - do_dput, do_dump kept as stubs (need connections infrastructure).

use std::cell::Cell;
use std::os::raw::{c_char, c_int, c_uint};
use std::ptr;

use crate::eval::attrib_core::getAttrib;
use crate::mainutils::memory_main::{R_AllocStringBuffer, R_FreeStringBuffer, R_StringBuffer};
use crate::mainutils::names::{
    PP_ASSIGN as N_PP_ASSIGN, PP_ASSIGN2 as N_PP_ASSIGN2, PP_BINARY as N_PP_BINARY,
    PP_BINARY2 as N_PP_BINARY2, PP_BREAK as N_PP_BREAK, PP_CURLY as N_PP_CURLY,
    PP_DOLLAR as N_PP_DOLLAR, PP_FOR as N_PP_FOR, PP_FOREIGN as N_PP_FOREIGN,
    PP_FUNCALL as N_PP_FUNCALL, PP_FUNCTION as N_PP_FUNCTION, PP_IF as N_PP_IF,
    PP_NEXT as N_PP_NEXT, PP_PAREN as N_PP_PAREN, PP_REPEAT as N_PP_REPEAT,
    PP_RETURN as N_PP_RETURN, PP_SUBASS as N_PP_SUBASS, PP_SUBSET as N_PP_SUBSET,
    PP_UNARY as N_PP_UNARY, PP_WHILE as N_PP_WHILE, PPinfo, PREC_COMPARE as N_PREC_COMPARE,
    PREC_PERCENT as N_PREC_PERCENT, PREC_SIGN as N_PREC_SIGN, PREC_SUBSET as N_PREC_SUBSET,
    PREC_SUM as N_PREC_SUM,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{ISNAN, NA_INTEGER, R_FINITE, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::*;
use crate::sexp::protect::*;
use crate::sexp::symbol::R_BraceSymbol;
use crate::sexp::symbol::R_DotsSymbol;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Constants from deparse.c
// ---------------------------------------------------------------------------

/// Buffer size for deparsing strings.
const BUFSIZE: c_int = 512;

/// Minimum allowed cutoff value.
const MIN_CUTOFF: c_int = 20;

/// Default cutoff value for line width.
const DEFAULT_CUTOFF: c_int = 60;

/// Maximum allowed cutoff value (must be < BUFSIZE).
const MAX_CUTOFF: c_int = BUFSIZE - 12;

// ---------------------------------------------------------------------------
// Deparse option flags
// ---------------------------------------------------------------------------

/// Keep NAs as NA_real_, NA_integer_, NA_character_, NA_complex_ (not just NA).
const KEEPNA: c_int = 1;

/// Keep integer constants with trailing L (e.g. 1L).
const KEEPINTEGER: c_int = 2;

/// Show attributes in deparse output.
const SHOWATTRIBUTES: c_int = 4;

/// Use source references if available.
const USESOURCE: c_int = 8;

/// Delay promises (show as <promise: ...>).
const DELAYPROMISES: c_int = 16;

/// S-compatible deparse (use old-style quoting, etc.).
const S_COMPAT: c_int = 32;

/// Quote expressions.
const QUOTEEXPRESSIONS: c_int = 64;

/// Use hex notation for floating-point numbers.
const HEXNUMERIC: c_int = 128;

/// Use 17 significant digits for real numbers.
const DIGITS17: c_int = 256;

/// Show names nicely (as tag = value).
const NICE_NAMES: c_int = 512;

/// Warn if deparsed result may not be source()-able.
const WARNINCOMPLETE: c_int = 1024;

/// Simple deparse options (no quoting, no attributes, no delay).
const SIMPLEDEPARSE: c_int = 0;

/// Default deparse options (show attributes).
const DEFAULTDEPARSE: c_int = SHOWATTRIBUTES;

/// Simple opts mask: keep KEEPINTEGER | USESOURCE | KEEPNA | S_COMPAT | WARNINCOMPLETE.
const SIMPLE_OPTS: c_int = !(QUOTEEXPRESSIONS | SHOWATTRIBUTES | DELAYPROMISES);

/// Show attributes or nice names.
const SHOW_ATTR_OR_NMS: c_int = SHOWATTRIBUTES | NICE_NAMES;

// ---------------------------------------------------------------------------
// Precedence constants (local aliases for names.rs values)
// ---------------------------------------------------------------------------

/// Precedence level for comparison operators (<, >, ==, etc.).
const PREC_COMPARE: c_int = N_PREC_COMPARE;
/// Precedence level for sum operators (+, -).
const PREC_SUM: c_int = N_PREC_SUM;
/// Precedence level for sign operators (unary +, -).
const PREC_SIGN: c_int = N_PREC_SIGN;
/// Precedence level for %op% operators.
const PREC_PERCENT: c_int = N_PREC_PERCENT;
/// Precedence level for subset operators ([, [[).
const PREC_SUBSET: c_int = N_PREC_SUBSET;

// ---------------------------------------------------------------------------
// PPinfo kinds (local aliases for names.rs values)
// ---------------------------------------------------------------------------

const PP_BINARY: c_int = N_PP_BINARY;
const PP_BINARY2: c_int = N_PP_BINARY2;
const PP_UNARY: c_int = N_PP_UNARY;
const PP_SUBSET: c_int = N_PP_SUBSET;
const PP_SUBASS: c_int = N_PP_SUBASS;
const PP_DOLLAR: c_int = N_PP_DOLLAR;
const PP_ASSIGN: c_int = N_PP_ASSIGN;
const PP_ASSIGN2: c_int = N_PP_ASSIGN2;
const PP_IF: c_int = N_PP_IF;
const PP_WHILE: c_int = N_PP_WHILE;
const PP_FOR: c_int = N_PP_FOR;
const PP_REPEAT: c_int = N_PP_REPEAT;
const PP_FUNCALL: c_int = N_PP_FUNCALL;
const PP_RETURN: c_int = N_PP_RETURN;
const PP_PAREN: c_int = N_PP_PAREN;
const PP_CURLY: c_int = N_PP_CURLY;
const PP_FOREIGN: c_int = N_PP_FOREIGN;
const PP_FUNCTION: c_int = N_PP_FUNCTION;
const PP_BREAK: c_int = N_PP_BREAK;
const PP_NEXT: c_int = N_PP_NEXT;

// ---------------------------------------------------------------------------
// Attribute type enum for deparsing
// ---------------------------------------------------------------------------

/// Unknown attribute state.
const ATTR_UNKNOWN: c_int = -1;
/// Simple object (no attributes shown).
const ATTR_SIMPLE: c_int = 0;
/// Object with OK names (names written as n1 = v1).
const ATTR_OK_NAMES: c_int = 1;
/// Object with structure attributes (non-names only).
const ATTR_STRUC_ATTR: c_int = 2;
/// Object with structure attributes including names.
const ATTR_STRUC_NMS_A: c_int = 3;

// ---------------------------------------------------------------------------
// NB / NB2 constants for complex encoding
// ---------------------------------------------------------------------------
const NB: usize = 1000;
const NB2: usize = 2 * NB + 25;

// ---------------------------------------------------------------------------
// LocalParseData — carries all state across recursive deparse calls
// ---------------------------------------------------------------------------

/// Local parse data struct for deparsing (equivalent to C's LocalParseData).
///
/// This holds all the state needed during recursive deparsing: line tracking,
/// buffer management, indentation, and option flags.
struct LocalParseData {
    /// Current line number being written.
    linenumber: c_int,
    /// Length of the current line buffer content.
    len: c_int,
    /// Whether we are inside curly braces.
    incurly: c_int,
    /// Whether we are inside a list.
    inlist: c_int,
    /// Whether we are at the start of a new line.
    startline: bool,
    /// Current indentation level (number of tabs).
    indent: c_int,
    /// String vector being built (R_NilValue when just counting).
    strvec: SEXP,
    /// Left-side precedence tracking for parenthesization.
    left: c_int,
    /// The string buffer for building the current line.
    buffer: R_StringBuffer,
    /// Line width cutoff.
    cutoff: c_int,
    /// Whether to use backticks for non-standard names.
    backtick: c_int,
    /// Deparse option flags.
    opts: c_int,
    /// Whether the result is source()-able.
    sourceable: c_int,
    /// Maximum number of lines to deparse.
    maxlines: c_int,
    /// Whether deparsing is still active (false after maxlines reached).
    active: bool,
    /// Whether an S4 object was encountered.
    isS4: c_int,
    /// Whether we are in a function argument context.
    fnarg: bool,
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
unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

#[inline]
unsafe fn isSymbol(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::SYMSXP }
}

#[inline]
unsafe fn isLanguage(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::LANGSXP }
}

#[inline]
unsafe fn isList(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::LISTSXP }
}

#[inline]
unsafe fn isString(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP }
}

#[inline]
unsafe fn isVectorAtomic(x: SEXP) -> bool {
    unsafe {
        if x.is_null() {
            return false;
        }
        let t = TYPEOF(x);
        t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
    }
}

/// Check if a name is a valid R identifier.
/// R identifiers must start with [a-zA-Z.] and contain only [a-zA-Z0-9._].
unsafe fn isValidName(s: *const c_char) -> bool {
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
unsafe fn isUserBinop(sym: SEXP) -> bool {
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
unsafe fn isValidString(x: SEXP) -> bool {
    unsafe { isString(x) && LENGTH(x) >= 1 && !STRING_ELT(x, 0).is_null() }
}

/// Get PPinfo for a builtin/special function.
/// Returns PPinfo for PP_FUNCALL if the symbol doesn't have a known entry.
unsafe fn getPPinfo(symval: SEXP) -> PPinfo {
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

/// Get PPinfo for an argument to needsparens (takes kind/prec/rightassoc directly).
unsafe fn get_arg_ppinfo(arg: SEXP) -> Option<PPinfo> {
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
            return None;
        }
        Some(getPPinfo(symval))
    }
}

const S4_OBJECT_MASK: u16 = 1 << 11;

/// Check if an SEXP has the S4 object bit set.
unsafe fn IS_S4_OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (((*x).sxpinfo.gp() & S4_OBJECT_MASK) != 0) as c_int
    }
}

/// streql: compare two C strings for equality.
unsafe fn streql(a: *const c_char, b: *const c_char) -> bool {
    unsafe {
        if a.is_null() || b.is_null() {
            return false;
        }
        std::ffi::CStr::from_ptr(a) == std::ffi::CStr::from_ptr(b)
    }
}

/// Get PRIMNAME as a C string (avoids conflict with eval/builtin.rs version).
unsafe fn primname_c(op: SEXP) -> *const c_char {
    unsafe {
        // Use the relop.rs version which returns *const c_char
        crate::mainutils::relop::PRIMNAME(op)
    }
}

/// Extract integer from SEXP — delegates to canonical `coerce::asInteger`.
unsafe fn Rf_asInteger(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asInteger(x) }
}

/// Extract logical from SEXP — delegates to canonical `coerce::asLogical`.
unsafe fn Rf_asLogical(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asLogical(x) }
}

// ---------------------------------------------------------------------------
// print2buff — append a string to the deparse buffer
// ---------------------------------------------------------------------------

/// Append a string to the deparse buffer, handling indentation at line start.
unsafe fn print2buff(strng: *const c_char, d: *mut LocalParseData) {
    unsafe {
        if strng.is_null() {
            return;
        }
        let d = &mut *d;

        if d.startline {
            d.startline = false;
            printtab2buff(d.indent, d);
        }
        let tlen = libc::strlen(strng);
        // Allocate buffer
        R_AllocStringBuffer(0, &mut d.buffer);
        let bufflen = libc::strlen(d.buffer.data);
        R_AllocStringBuffer(bufflen + tlen, &mut d.buffer);
        // Append string
        libc::strcat(d.buffer.data, strng);
        d.len += tlen as c_int;
    }
}

// ---------------------------------------------------------------------------
// printtab2buff — write indentation tabs to the buffer
// ---------------------------------------------------------------------------

/// Write indentation to the buffer. First 4 levels use 4 spaces each,
/// subsequent levels use 2 spaces each (emacs-style).
unsafe fn printtab2buff(ntab: c_int, d: *mut LocalParseData) {
    unsafe {
        for i in 1..=ntab {
            if i <= 4 {
                print2buff(b"    \0".as_ptr() as *const c_char, d);
            } else {
                print2buff(b"  \0".as_ptr() as *const c_char, d);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// writeline — flush current buffer line to the string vector
// ---------------------------------------------------------------------------

/// Flush the current buffer line to the output string vector.
///
/// If strvec is R_NilValue (counting pass), just increments linenumber.
/// Otherwise, stores the buffer content in strvec[linenumber].
unsafe fn writeline(d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        if !isNull(d.strvec) && d.linenumber < d.maxlines {
            let chars = Rf_mkChar(d.buffer.data);
            SET_STRING_ELT(d.strvec, d.linenumber as R_xlen_t, chars);
        }
        d.linenumber += 1;
        if d.linenumber >= d.maxlines {
            d.active = false;
        }
        // Reset
        d.len = 0;
        if !d.buffer.data.is_null() {
            *d.buffer.data = 0;
        }
        d.startline = true;
    }
}

// ---------------------------------------------------------------------------
// linebreak — break line if current line exceeds cutoff
// ---------------------------------------------------------------------------

/// Break the current line if it exceeds the cutoff width.
unsafe fn linebreak(lbreak: *mut bool, d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        if d.len > d.cutoff {
            if !*lbreak {
                *lbreak = true;
                d.indent += 1;
            }
            writeline(d);
        }
    }
}

// ---------------------------------------------------------------------------
// curlyahead — check if expression is a curly-brace block
// ---------------------------------------------------------------------------

/// Check if s is a list whose first element is a curly brace ({).
/// Used for correct if-then-else formatting.
unsafe fn curlyahead(s: SEXP) -> bool {
    unsafe {
        if (isList(s) || isLanguage(s)) && isSymbol(CAR(s)) {
            CAR(s) == R_BraceSymbol()
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// needsparens — determine if an argument needs parenthesization
// ---------------------------------------------------------------------------

/// Check if an argument to a unary or binary operator needs parentheses.
///
/// mainop_kind, mainop_prec, mainop_rightassoc describe the outer operator.
/// arg is an argument to it, on the left if left == 1.
/// deepLeft is the precedence from further up the left side.
unsafe fn needsparens(
    mainop_kind: c_int,
    mainop_prec: c_int,
    mainop_rightassoc: c_int,
    arg: SEXP,
    left: c_int,
    deepLeft: c_int,
) -> bool {
    unsafe {
        if let Some(mut arginfo) = get_arg_ppinfo(arg) {
            // Not all binary ops are binary!
            match arginfo.kind {
                PP_BINARY | PP_BINARY2 => {
                    let nargs = Rf_length(CDR(arg));
                    match nargs {
                        1 => {
                            // binary +/- precedence upgraded as unary
                            if arginfo.prec == PREC_SUM {
                                arginfo.prec = PREC_SIGN;
                            }
                            arginfo.kind = PP_UNARY;
                        }
                        2 => {}
                        _ => return false,
                    }
                }
                _ => {} // intentionally unhandled: SEXPTYPE not relevant for deparse precedence
            }

            match arginfo.kind {
                PP_SUBSET => {
                    match mainop_kind {
                        PP_DOLLAR | PP_SUBSET => {
                            if mainop_prec > arginfo.prec {
                                return false;
                            }
                            // else fall through
                        }
                        _ => {} // intentionally unhandled: unknown precedence level for deparse
                    }
                    // fall through
                }
                PP_BINARY | PP_BINARY2 => {
                    if mainop_prec == PREC_COMPARE && arginfo.prec == PREC_COMPARE {
                        return true; // a < b < c is not legal syntax
                    }
                    // fall through
                }
                PP_ASSIGN | PP_ASSIGN2 | PP_DOLLAR => {}
                _ => {} // intentionally unhandled: unknown PP pattern for deparse
            }

            match arginfo.kind {
                PP_BINARY | PP_BINARY2 | PP_ASSIGN | PP_ASSIGN2 | PP_DOLLAR => {
                    if mainop_prec > arginfo.prec
                        || (mainop_prec == arginfo.prec && left == mainop_rightassoc)
                    {
                        return true;
                    }
                }
                PP_UNARY => {
                    return (left != 0 && mainop_prec > arginfo.prec)
                        || (deepLeft != 0 && deepLeft > arginfo.prec);
                }
                PP_FOR | PP_IF | PP_WHILE | PP_REPEAT => {
                    return left != 0 || deepLeft != 0;
                }
                PP_SUBSET => {
                    if mainop_kind != PP_DOLLAR
                        && mainop_kind != PP_SUBSET
                        && (mainop_prec > arginfo.prec
                            || (mainop_prec == arginfo.prec && left == mainop_rightassoc))
                    {
                        return true;
                    }
                }
                _ => return false,
            }
        } else if isUserBinop(CAR(arg))
            && isLanguage(arg)
            && (mainop_prec > PREC_PERCENT
                || (mainop_prec == PREC_PERCENT && left == mainop_rightassoc))
        {
            return true;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// usable_nice_names — check if names can be used in nice form
// ---------------------------------------------------------------------------

/// Check if the character vector x contains no NA_character_ or all "",
/// or if isAtomic, does not contain "recursive" or "use.names".
unsafe fn usable_nice_names(x: SEXP, isAtomic: bool) -> bool {
    unsafe {
        if !isString(x) {
            return true;
        }
        let n = XLENGTH(x) as usize;
        let mut all_0 = true;
        for i in 0..n {
            let elt = STRING_ELT(x, i as R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                return false;
            }
            if isAtomic {
                let name = CHAR(elt);
                if !name.is_null() {
                    let bytes = std::ffi::CStr::from_ptr(name).to_bytes();
                    if bytes == b"recursive" || bytes == b"use.names" {
                        return false;
                    }
                }
            }
            if all_0 {
                let name = CHAR(elt);
                if !name.is_null() && *name != 0 {
                    all_0 = false;
                }
            }
        }
        !all_0
    }
}

// ---------------------------------------------------------------------------
// attr1 — determine attribute display type
// ---------------------------------------------------------------------------

/// Determine how to display attributes during deparsing.
///
/// Returns one of ATTR_SIMPLE, ATTR_OK_NAMES, ATTR_STRUC_ATTR, ATTR_STRUC_NMS_A.
unsafe fn attr1(s: SEXP, d: *mut LocalParseData) -> c_int {
    unsafe {
        let d = &mut *d;
        let a = ATTRIB(s);
        // For attr1 we need R_NamesSymbol and R_SrcrefSymbol - install them
        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
        let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);
        let nm = getAttrib(s, names_sym);

        let mut attr = ATTR_UNKNOWN;
        let nice_names = (d.opts & NICE_NAMES) != 0;
        let show_attr = (d.opts & SHOWATTRIBUTES) != 0;
        let has_names = !isNull(nm);

        if has_names {
            let ok_names = nice_names && usable_nice_names(nm, isVectorAtomic(s));
            if !ok_names {
                attr = if show_attr {
                    ATTR_STRUC_NMS_A
                } else {
                    ATTR_OK_NAMES
                };
            }
        }

        let mut cur = a;
        while attr == ATTR_UNKNOWN && !isNull(cur) {
            if has_names && TAG(cur) == names_sym {
                // skip names
            } else if show_attr && TAG(cur) != srcref_sym {
                attr = ATTR_STRUC_ATTR;
                break;
            }
            cur = CDR(cur);
        }
        if attr == ATTR_UNKNOWN {
            attr = if has_names {
                ATTR_OK_NAMES
            } else {
                ATTR_SIMPLE
            };
        }

        if attr >= ATTR_STRUC_ATTR {
            print2buff(b"structure(\0".as_ptr() as *const c_char, d);
        }
        attr
    }
}

// ---------------------------------------------------------------------------
// attr2 — write attribute suffix to buffer
// ---------------------------------------------------------------------------

/// Write the attribute suffix (e.g., ", names = ..., dim = ...") to buffer.
unsafe fn attr2(s: SEXP, d: *mut LocalParseData, not_names: bool) {
    unsafe {
        let d = &mut *d;
        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
        let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);
        let dim_sym = Rf_install(b"dim\0".as_ptr() as *const c_char);
        let dimnames_sym = Rf_install(b"dimnames\0".as_ptr() as *const c_char);
        let tsp_sym = Rf_install(b"tsp\0".as_ptr() as *const c_char);
        let levels_sym = Rf_install(b"levels\0".as_ptr() as *const c_char);

        let mut a = ATTRIB(s);
        while !isNull(a) {
            if TAG(a) != srcref_sym && !(TAG(a) == names_sym && not_names) {
                print2buff(b", \0".as_ptr() as *const c_char, d);
                if TAG(a) == dim_sym {
                    print2buff(b"dim\0".as_ptr() as *const c_char, d);
                } else if TAG(a) == dimnames_sym {
                    print2buff(b"dimnames\0".as_ptr() as *const c_char, d);
                } else if TAG(a) == names_sym {
                    print2buff(b"names\0".as_ptr() as *const c_char, d);
                } else if TAG(a) == tsp_sym {
                    print2buff(b"tsp\0".as_ptr() as *const c_char, d);
                } else if TAG(a) == levels_sym {
                    print2buff(b"levels\0".as_ptr() as *const c_char, d);
                } else {
                    // TAG(a) might contain spaces etc
                    let tag_name = CHAR(PRINTNAME(TAG(a)));
                    let d_opts_in = d.opts;
                    d.opts = SIMPLEDEPARSE;
                    if !tag_name.is_null() && isValidName(tag_name) {
                        deparse2buff(TAG(a), d);
                    } else {
                        print2buff(b"\"\0".as_ptr() as *const c_char, d);
                        deparse2buff(TAG(a), d);
                        print2buff(b"\"\0".as_ptr() as *const c_char, d);
                    }
                    d.opts = d_opts_in;
                }
                print2buff(b" = \0".as_ptr() as *const c_char, d);
                let fnarg = d.fnarg;
                d.fnarg = true;
                deparse2buff(CAR(a), d);
                d.fnarg = fnarg;
            }
            a = CDR(a);
        }
        print2buff(b")\0".as_ptr() as *const c_char, d);
    }
}

// ---------------------------------------------------------------------------
// quotify — quote a symbol name if needed
// ---------------------------------------------------------------------------

/// If a symbol is not a valid R name, return a quoted/escaped version.
/// Otherwise return the name as-is.
unsafe fn quotify(name: SEXP, quote: c_int) -> *const c_char {
    unsafe {
        if name.is_null() {
            return ptr::null();
        }
        let s = CHAR(name);
        if s.is_null() {
            return ptr::null();
        }
        if isValidName(s) || *s == 0 {
            return s;
        }
        // For backtick or double-quote quoting, just return the name with quotes
        // Full EncodeString is in printutils but currently stubbed.
        // We implement basic quoting here.
        thread_local! { static QUOTE_BUF: Cell<[u8; 1024]> = Cell::new([0; 1024]); }
        QUOTE_BUF.with(|cell| {
            let buf: &mut [u8; 1024] = &mut *cell.as_ptr();
            let bytes = std::ffi::CStr::from_ptr(s).to_bytes();
            let quote_char = if quote == b'`' as c_int { b'`' } else { b'"' };
            let mut pos = 0;
            buf[pos] = quote_char;
            pos += 1;
            for &b in bytes.iter() {
                if b == quote_char || b == b'\\' {
                    buf[pos] = b'\\';
                    pos += 1;
                }
                if pos + 1 >= 1022 {
                    break;
                }
                buf[pos] = b;
                pos += 1;
            }
            buf[pos] = quote_char;
            pos += 1;
            buf[pos] = 0;
            buf.as_ptr() as *const c_char
        })
    }
}

// ---------------------------------------------------------------------------
// parenthesizeCaller — check if a function caller needs parentheses
// ---------------------------------------------------------------------------

/// Check if a function caller needs to be parenthesized.
/// For example: `(f+g)(z)` needs parens, but `x$f(z)` does not.
unsafe fn parenthesizeCaller(s: SEXP) -> bool {
    unsafe {
        if TYPEOF(s) != SEXPTYPE::LANGSXP {
            return false;
        }
        let op = CAR(s);
        if isSymbol(op) {
            if isUserBinop(op) {
                return true;
            } // %foo%
            let sym = SYMVALUE(op);
            let t = TYPEOF(sym);
            if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
                let pp = getPPinfo(sym);
                return !(pp.prec >= PREC_SUBSET
                    || pp.kind == PP_FUNCALL
                    || pp.kind == PP_PAREN
                    || pp.kind == PP_CURLY);
            }
            return false; // regular function call
        } else if TYPEOF(op) == SEXPTYPE::CLOSXP {
            return true;
        } else {
            return true; // something strange, like (1)(x)
        }
    }
}

// ---------------------------------------------------------------------------
// src2buff1 — deparse one source reference to buffer
// ---------------------------------------------------------------------------

/// Deparse one source reference to the buffer.
///
/// Unimplemented: requires R_AsCharacterSymbol and eval infrastructure.
unsafe fn src2buff1(_srcref: SEXP, _d: *mut LocalParseData) {
    // requires eval/R_AsCharacterSymbol
}

// ---------------------------------------------------------------------------
// src2buff — deparse source element k to buffer
// ---------------------------------------------------------------------------

/// Deparse source element k to buffer if possible. Returns false on failure.
unsafe fn src2buff(sv: SEXP, k: c_int, d: *mut LocalParseData) -> bool {
    unsafe {
        if !sv.is_null() && TYPEOF(sv) == SEXPTYPE::VECSXP && LENGTH(sv) > k {
            let t = VECTOR_ELT(sv, k as R_xlen_t);
            if !isNull(t) {
                src2buff1(t, d);
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// deparse2buf_name — deparse a vector element name to buffer
// ---------------------------------------------------------------------------

/// Deparse a name from a names vector to the buffer, with quoting as needed.
unsafe fn deparse2buf_name(nv: SEXP, i: c_int, d: *mut LocalParseData) {
    unsafe {
        if isNull(nv) {
            return;
        }
        let d = &mut *d;
        let elt = STRING_ELT(nv, i as R_xlen_t);
        if isNull(elt) {
            return;
        }
        let name = CHAR(elt);
        if name.is_null() || *name == 0 {
            return;
        } // length test

        if !name.is_null() && isValidName(name) {
            deparse2buff(elt, d);
        } else if d.backtick != 0 {
            print2buff(b"`\0".as_ptr() as *const c_char, d);
            deparse2buff(elt, d);
            print2buff(b"`\0".as_ptr() as *const c_char, d);
        } else {
            print2buff(b"\"\0".as_ptr() as *const c_char, d);
            deparse2buff(elt, d);
            print2buff(b"\"\0".as_ptr() as *const c_char, d);
        }
        print2buff(b" = \0".as_ptr() as *const c_char, d);
    }
}

// ---------------------------------------------------------------------------
// EncodeNonFiniteComplexElement — encode non-finite complex number
// ---------------------------------------------------------------------------

/// Encode a complex value with non-finite components as a syntactically
/// correct string (using complex(real=..., imaginary=...) form).
unsafe fn EncodeNonFiniteComplexElement(x: Rcomplex, buff: *mut c_char) -> *const c_char {
    unsafe {
        // Simplified implementation: format real and imaginary parts
        let mut re_buf = [0i8; 64];
        let mut im_buf = [0i8; 64];
        if R_FINITE(x.r) {
            libc::snprintf(
                re_buf.as_mut_ptr(),
                64,
                b"%.17g\0".as_ptr() as *const c_char,
                x.r,
            );
        } else if ISNAN(x.r) {
            libc::snprintf(re_buf.as_mut_ptr(), 64, b"NaN\0".as_ptr() as *const c_char);
        } else {
            libc::snprintf(re_buf.as_mut_ptr(), 64, b"Inf\0".as_ptr() as *const c_char);
        }
        if R_FINITE(x.i) {
            libc::snprintf(
                im_buf.as_mut_ptr(),
                64,
                b"%.17g\0".as_ptr() as *const c_char,
                x.i,
            );
        } else if ISNAN(x.i) {
            libc::snprintf(im_buf.as_mut_ptr(), 64, b"NaN\0".as_ptr() as *const c_char);
        } else {
            libc::snprintf(im_buf.as_mut_ptr(), 64, b"Inf\0".as_ptr() as *const c_char);
        }
        libc::snprintf(
            buff,
            NB2 as usize,
            b"complex(real=%s, imaginary=%s)\0".as_ptr() as *const c_char,
            re_buf.as_ptr(),
            im_buf.as_ptr(),
        );
        buff
    }
}

// ---------------------------------------------------------------------------
// Format helpers for vectors
// ---------------------------------------------------------------------------

/// Format an integer element as a string.
unsafe fn format_int_element(val: c_int) -> *const c_char {
    unsafe {
        thread_local! { static BUF: Cell<[c_char; 32]> = Cell::new([0; 32]); }
        BUF.with(|cell| {
            let buf: &mut [c_char; 32] = &mut *cell.as_ptr();
            if val == NA_INTEGER {
                libc::snprintf(buf.as_mut_ptr(), 32, b"NA\0".as_ptr() as *const c_char);
            } else {
                libc::snprintf(buf.as_mut_ptr(), 32, b"%d\0".as_ptr() as *const c_char, val);
            }
            buf.as_ptr() as *const c_char
        })
    }
}

/// Format a logical element as a string.
unsafe fn format_logical_element(val: c_int) -> *const c_char {
    unsafe {
        thread_local! { static BUF: Cell<[c_char; 8]> = Cell::new([0; 8]); }
        BUF.with(|cell| {
            let buf: &mut [c_char; 8] = &mut *cell.as_ptr();
            if val == NA_INTEGER {
                libc::snprintf(buf.as_mut_ptr(), 8, b"NA\0".as_ptr() as *const c_char);
            } else if val != 0 {
                buf[0] = b'T' as c_char;
                buf[1] = b'R' as c_char;
                buf[2] = b'U' as c_char;
                buf[3] = b'E' as c_char;
                buf[4] = 0;
            } else {
                buf[0] = b'F' as c_char;
                buf[1] = b'A' as c_char;
                buf[2] = b'L' as c_char;
                buf[3] = b'S' as c_char;
                buf[4] = b'E' as c_char;
                buf[5] = 0;
            }
            buf.as_ptr() as *const c_char
        })
    }
}

/// Format a real element as a string with maximal precision.
unsafe fn format_real_element(val: f64) -> *const c_char {
    unsafe {
        thread_local! { static BUF: Cell<[c_char; 64]> = Cell::new([0; 64]); }
        BUF.with(|cell| {
            let buf: &mut [c_char; 64] = &mut *cell.as_ptr();
            if ISNAN(val) && (val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN) {
                libc::snprintf(buf.as_mut_ptr(), 64, b"NA\0".as_ptr() as *const c_char);
            } else if ISNAN(val) {
                libc::snprintf(buf.as_mut_ptr(), 64, b"NaN\0".as_ptr() as *const c_char);
            } else if !R_FINITE(val) {
                if val > 0.0 {
                    libc::snprintf(buf.as_mut_ptr(), 64, b"Inf\0".as_ptr() as *const c_char);
                } else {
                    libc::snprintf(buf.as_mut_ptr(), 64, b"-Inf\0".as_ptr() as *const c_char);
                }
            } else {
                libc::snprintf(
                    buf.as_mut_ptr(),
                    64,
                    b"%.17g\0".as_ptr() as *const c_char,
                    val,
                );
            }
            buf.as_ptr() as *const c_char
        })
    }
}

/// Format a string element with quoting.
unsafe fn format_string_element(s: SEXP) -> *const c_char {
    unsafe {
        thread_local! { static BUF: Cell<[u8; 2048]> = Cell::new([0; 2048]); }
        BUF.with(|cell| {
            let buf: &mut [u8; 2048] = &mut *cell.as_ptr();
            if s.is_null() || s == R_NilValue() {
                buf[0] = b'N';
                buf[1] = b'A';
                buf[2] = 0;
                return buf.as_ptr() as *const c_char;
            }
            let name = CHAR(s);
            if name.is_null() {
                buf[0] = b'N';
                buf[1] = b'A';
                buf[2] = 0;
                return buf.as_ptr() as *const c_char;
            }
            let bytes = std::ffi::CStr::from_ptr(name).to_bytes();
            let mut pos = 0;
            buf[pos] = b'"';
            pos += 1;
            for &b in bytes.iter() {
                if pos + 2 >= 2046 {
                    break;
                }
                match b {
                    b'"' | b'\\' => {
                        buf[pos] = b'\\';
                        pos += 1;
                        buf[pos] = b;
                        pos += 1;
                    }
                    b'\n' => {
                        buf[pos] = b'\\';
                        pos += 1;
                        buf[pos] = b'n';
                        pos += 1;
                    }
                    b'\r' => {
                        buf[pos] = b'\\';
                        pos += 1;
                        buf[pos] = b'r';
                        pos += 1;
                    }
                    b'\t' => {
                        buf[pos] = b'\\';
                        pos += 1;
                        buf[pos] = b't';
                        pos += 1;
                    }
                    _ => {
                        buf[pos] = b;
                        pos += 1;
                    }
                }
            }
            buf[pos] = b'"';
            pos += 1;
            buf[pos] = 0;
            buf.as_ptr() as *const c_char
        })
    }
}

/// Format a raw element as hex.
unsafe fn format_raw_element(val: Rbyte) -> *const c_char {
    unsafe {
        thread_local! { static BUF: Cell<[c_char; 8]> = Cell::new([0; 8]); }
        BUF.with(|cell| {
            let buf: &mut [c_char; 8] = &mut *cell.as_ptr();
            libc::snprintf(
                buf.as_mut_ptr(),
                8,
                b"0x%02x\0".as_ptr() as *const c_char,
                val as c_uint,
            );
            buf.as_ptr() as *const c_char
        })
    }
}

// ---------------------------------------------------------------------------
// vector2buff — deparse atomic vectors to buffer
// ---------------------------------------------------------------------------

/// Deparse atomic vectors (LGLSXP, INTSXP, REALSXP, CPLXSXP, STRSXP, RAWSXP).
unsafe fn vector2buff(vector: SEXP, d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        let d_opts_in = d.opts;
        let tlen = LENGTH(vector);
        let quote = if TYPEOF(vector) == SEXPTYPE::STRSXP {
            b'"' as c_int
        } else {
            0
        };
        let mut surround = false;

        // Check for integer sequences (m:n)
        let mut int_seq = false;
        if TYPEOF(vector) == SEXPTYPE::INTSXP && tlen > 1 {
            let vec = INTEGER(vector);
            if !vec.is_null() {
                let v0 = *vec;
                let v1 = *vec.add(1);
                if v0 != NA_INTEGER && v1 != NA_INTEGER {
                    let d_i = (v1 as f64) - (v0 as f64);
                    if d_i.abs() == 1.0 {
                        int_seq = true;
                        for i in 2..tlen as usize {
                            let vi = *vec.add(i);
                            if vi == NA_INTEGER {
                                int_seq = false;
                                break;
                            }
                            let diff = (vi as f64) - (*vec.add(i - 1) as f64);
                            if (diff - d_i).abs() > 1e-10 {
                                int_seq = false;
                                break;
                            }
                        }
                    }
                }
            }
        }

        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
        let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);
        let mut nv = R_NilValue();
        let mut do_names = (d_opts_in & SHOW_ATTR_OR_NMS) != 0;
        if do_names {
            nv = getAttrib(vector, names_sym);
            if isNull(nv) {
                do_names = false;
            }
        }
        Rf_protect(nv);

        let mut str_names = false;
        let need_c = tlen > 1;
        str_names = do_names && (int_seq || tlen == 0);
        if str_names {
            d.opts &= !NICE_NAMES;
        }
        let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
            attr1(vector, d)
        } else {
            ATTR_SIMPLE
        };
        if do_names {
            do_names = attr == ATTR_OK_NAMES || attr == ATTR_STRUC_ATTR;
        }

        if tlen == 0 {
            match TYPEOF(vector) {
                10 => print2buff(b"logical(0)\0".as_ptr() as *const c_char, d), // LGLSXP
                13 => print2buff(b"integer(0)\0".as_ptr() as *const c_char, d), // INTSXP
                14 => print2buff(b"numeric(0)\0".as_ptr() as *const c_char, d), // REALSXP
                15 => print2buff(b"complex(0)\0".as_ptr() as *const c_char, d), // CPLXSXP
                16 => print2buff(b"character(0)\0".as_ptr() as *const c_char, d), // STRSXP
                24 => print2buff(b"raw(0)\0".as_ptr() as *const c_char, d),     // RAWSXP
                _ => {} // intentionally unhandled: unsupported type for empty vector display
            }
        } else if TYPEOF(vector) == SEXPTYPE::INTSXP {
            if int_seq {
                let vec = INTEGER(vector);
                if !vec.is_null() {
                    let strp = format_int_element(*vec);
                    print2buff(strp, d);
                    print2buff(b":\0".as_ptr() as *const c_char, d);
                    let strp = format_int_element(*vec.add((tlen - 1) as usize));
                    print2buff(strp, d);
                }
            } else {
                let vec = INTEGER(vector);
                let add_l = (d.opts & KEEPINTEGER != 0) && (d.opts & S_COMPAT == 0);
                let mut all_na = (d.opts & KEEPNA != 0) || add_l;
                if !vec.is_null() {
                    for i in 0..tlen as usize {
                        if *vec.add(i) != NA_INTEGER {
                            all_na = false;
                            break;
                        }
                    }
                }
                if (d.opts & KEEPINTEGER != 0) && (d.opts & S_COMPAT != 0) {
                    print2buff(b"as.integer(\0".as_ptr() as *const c_char, d);
                    surround = true;
                }
                all_na = all_na && (d.opts & S_COMPAT == 0);
                if need_c {
                    print2buff(b"c(\0".as_ptr() as *const c_char, d);
                }
                if !vec.is_null() {
                    for i in 0..tlen as usize {
                        if do_names {
                            deparse2buf_name(nv, i as c_int, d);
                        }
                        if all_na && *vec.add(i) == NA_INTEGER {
                            print2buff(b"NA_integer_\0".as_ptr() as *const c_char, d);
                        } else {
                            let strp = format_int_element(*vec.add(i));
                            print2buff(strp, d);
                            if add_l && *vec.add(i) != NA_INTEGER {
                                print2buff(b"L\0".as_ptr() as *const c_char, d);
                            }
                        }
                        if i < (tlen as usize) - 1 {
                            print2buff(b", \0".as_ptr() as *const c_char, d);
                        }
                        if tlen > 1 && d.len > d.cutoff {
                            writeline(d);
                        }
                        if !d.active {
                            break;
                        }
                    }
                }
                if need_c {
                    print2buff(b")\0".as_ptr() as *const c_char, d);
                }
                if surround {
                    print2buff(b")\0".as_ptr() as *const c_char, d);
                }
            }
        } else {
            // tlen > 0; not INTSXP
            let mut all_na = d.opts & KEEPNA != 0;

            // Handle NA-heavy types
            if (d.opts & KEEPNA != 0) && TYPEOF(vector) == SEXPTYPE::REALSXP {
                let vec = REAL(vector);
                if !vec.is_null() {
                    for i in 0..tlen as usize {
                        if !ISNAN(*vec.add(i)) {
                            all_na = false;
                            break;
                        }
                    }
                }
                if all_na && (d.opts & S_COMPAT != 0) {
                    print2buff(b"as.double(\0".as_ptr() as *const c_char, d);
                    surround = true;
                }
            } else if (d.opts & KEEPNA != 0) && TYPEOF(vector) == SEXPTYPE::CPLXSXP {
                let vec = COMPLEX(vector);
                if !vec.is_null() {
                    for i in 0..tlen as usize {
                        let c = *vec.add(i);
                        if !ISNAN(c.r) && !ISNAN(c.i) {
                            all_na = false;
                            break;
                        }
                    }
                }
                if all_na && (d.opts & S_COMPAT != 0) {
                    print2buff(b"as.complex(\0".as_ptr() as *const c_char, d);
                    surround = true;
                }
            } else if TYPEOF(vector) == SEXPTYPE::RAWSXP {
                print2buff(b"as.raw(\0".as_ptr() as *const c_char, d);
                surround = true;
            }

            if need_c {
                print2buff(b"c(\0".as_ptr() as *const c_char, d);
            }
            all_na = all_na && (d.opts & S_COMPAT == 0);

            for i in 0..tlen as usize {
                if do_names {
                    deparse2buf_name(nv, i as c_int, d);
                }

                let mut strp: *const c_char = ptr::null();

                match TYPEOF(vector) {
                    10 => {
                        // LGLSXP
                        let vec = LOGICAL(vector);
                        if !vec.is_null() {
                            if all_na && *vec.add(i) == NA_INTEGER {
                                strp = b"NA\0".as_ptr() as *const c_char;
                            } else {
                                strp = format_logical_element(*vec.add(i));
                            }
                        }
                    }
                    14 => {
                        // REALSXP
                        let vec = REAL(vector);
                        if !vec.is_null() {
                            let v = *vec.add(i);
                            if all_na
                                && ISNAN(v)
                                && (v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN)
                            {
                                strp = b"NA_real_\0".as_ptr() as *const c_char;
                            } else if (d.opts & HEXNUMERIC != 0) && R_FINITE(v) {
                                thread_local! { static HEX_BUF: Cell<[c_char; 64]> = Cell::new([0; 64]); }
                                HEX_BUF.with(|cell| {
                                    let hex_buf: &mut [c_char; 64] = &mut *cell.as_ptr();
                                    libc::snprintf(
                                        hex_buf.as_mut_ptr(),
                                        64,
                                        b"%a\0".as_ptr() as *const c_char,
                                        v,
                                    );
                                    strp = hex_buf.as_ptr() as *const c_char;
                                });
                            } else if (d.opts & DIGITS17 != 0) && R_FINITE(v) {
                                thread_local! { static DIG_BUF: Cell<[c_char; 64]> = Cell::new([0; 64]); }
                                DIG_BUF.with(|cell| {
                                    let dig_buf: &mut [c_char; 64] = &mut *cell.as_ptr();
                                    libc::snprintf(
                                        dig_buf.as_mut_ptr(),
                                        64,
                                        b"%.17g\0".as_ptr() as *const c_char,
                                        v,
                                    );
                                    strp = dig_buf.as_ptr() as *const c_char;
                                });
                            } else {
                                strp = format_real_element(v);
                            }
                        }
                    }
                    15 => {
                        // CPLXSXP
                        let vec = COMPLEX(vector);
                        if !vec.is_null() {
                            let c = *vec.add(i);
                            if all_na && ISNAN(c.r) && ISNAN(c.i) {
                                strp = b"NA_complex_\0".as_ptr() as *const c_char;
                            } else if ISNAN(c.r) || !R_FINITE(c.i) {
                                thread_local! { static CPLX_BUF: Cell<[c_char; NB2]> = Cell::new([0; NB2]); }
                                CPLX_BUF.with(|cell| {
                                    let cplx_buf: &mut [c_char; NB2] = &mut *cell.as_ptr();
                                    strp = EncodeNonFiniteComplexElement(c, cplx_buf.as_mut_ptr());
                                });
                            } else if (d.opts & HEXNUMERIC != 0) && R_FINITE(c.r) && R_FINITE(c.i) {
                                thread_local! { static HEX_CPLX: Cell<[c_char; 128]> = Cell::new([0; 128]); }
                                HEX_CPLX.with(|cell| {
                                    let hex_cplx: &mut [c_char; 128] = &mut *cell.as_ptr();
                                    libc::snprintf(
                                        hex_cplx.as_mut_ptr(),
                                        128,
                                        b"%a + %ai\0".as_ptr() as *const c_char,
                                        c.r,
                                        c.i,
                                    );
                                    strp = hex_cplx.as_ptr() as *const c_char;
                                });
                            } else if (d.opts & DIGITS17 != 0) && R_FINITE(c.r) && R_FINITE(c.i) {
                                thread_local! { static DIG_CPLX: Cell<[c_char; 128]> = Cell::new([0; 128]); }
                                DIG_CPLX.with(|cell| {
                                    let dig_cplx: &mut [c_char; 128] = &mut *cell.as_ptr();
                                    libc::snprintf(
                                        dig_cplx.as_mut_ptr(),
                                        128,
                                        b"%.17g%+.17gi\0".as_ptr() as *const c_char,
                                        c.r,
                                        c.i,
                                    );
                                    strp = dig_cplx.as_ptr() as *const c_char;
                                });
                            } else {
                                thread_local! { static CPLX_BUF2: Cell<[c_char; 256]> = Cell::new([0; 256]); }
                                CPLX_BUF2.with(|cell| {
                                    let cplx_buf2: &mut [c_char; 256] = &mut *cell.as_ptr();
                                    let re = format_real_element(c.r);
                                    let im = format_real_element(c.i);
                                    libc::snprintf(
                                        cplx_buf2.as_mut_ptr(),
                                        256,
                                        b"%s%s%si\0".as_ptr() as *const c_char,
                                        re,
                                        if c.i >= 0.0 {
                                            b"+\0".as_ptr() as *const c_char
                                        } else {
                                            b"\0".as_ptr() as *const c_char
                                        },
                                        im,
                                    );
                                    strp = cplx_buf2.as_ptr() as *const c_char;
                                });
                            }
                        }
                    }
                    16 => {
                        // STRSXP
                        let elt = STRING_ELT(vector, i as R_xlen_t);
                        if all_na && (elt.is_null() || elt == R_NilValue()) {
                            strp = b"NA_character_\0".as_ptr() as *const c_char;
                        } else {
                            strp = format_string_element(elt);
                        }
                    }
                    24 => {
                        // RAWSXP
                        let vec = RAW(vector);
                        if !vec.is_null() {
                            strp = format_raw_element(*vec.add(i));
                        }
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for element formatting
                }

                if !strp.is_null() {
                    print2buff(strp, d);
                }
                if i < (tlen as usize) - 1 {
                    print2buff(b", \0".as_ptr() as *const c_char, d);
                }
                if tlen > 1 && d.len > d.cutoff {
                    writeline(d);
                }
                if !d.active {
                    break;
                }
            }

            if need_c {
                print2buff(b")\0".as_ptr() as *const c_char, d);
            }
            if surround {
                print2buff(b")\0".as_ptr() as *const c_char, d);
            }
        }
        if attr >= ATTR_STRUC_ATTR {
            attr2(vector, d, attr == ATTR_STRUC_ATTR);
        }
        if str_names {
            d.opts = d_opts_in;
        }
        Rf_unprotect(1); // nv
    }
}

// ---------------------------------------------------------------------------
// vec2buff — deparse list/expression vectors to buffer
// ---------------------------------------------------------------------------

/// Deparse vectors of S-expressions (list() and expression() objects).
unsafe fn vec2buff(v: SEXP, d: *mut LocalParseData, do_names: bool) {
    unsafe {
        let d = &mut *d;
        let mut lbreak = false;
        let n = LENGTH(v);
        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
        let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);
        let mut nv = R_NilValue();
        let mut do_names = do_names;
        if do_names {
            nv = getAttrib(v, names_sym);
            if isNull(nv) {
                do_names = false;
            }
        }
        Rf_protect(nv);

        let mut sv = R_NilValue();
        if (d.opts & USESOURCE) != 0 {
            sv = getAttrib(v, srcref_sym);
            if TYPEOF(sv) != SEXPTYPE::VECSXP {
                sv = R_NilValue();
            }
        }

        for i in 0..n as usize {
            if i > 0 {
                print2buff(b", \0".as_ptr() as *const c_char, d);
            }
            linebreak(&mut lbreak, d);
            if do_names {
                deparse2buf_name(nv, i as c_int, d);
            }
            if !src2buff(sv, i as c_int, d) {
                deparse2buff(VECTOR_ELT(v, i as R_xlen_t), d);
            }
        }
        if lbreak {
            d.indent -= 1;
        }
        Rf_unprotect(1); // nv
    }
}

// ---------------------------------------------------------------------------
// args2buff — deparse argument list to buffer
// ---------------------------------------------------------------------------

/// Deparse an argument list (pairlist) to the buffer.
///
/// Handles named and unnamed arguments, default values for formals, and
/// line breaking for long argument lists.
unsafe fn args2buff(arglist: SEXP, _lineb: c_int, formals: c_int, d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        let mut lbreak = false;
        let mut cur = arglist;

        while !isNull(cur) {
            if TYPEOF(cur) != SEXPTYPE::LISTSXP && TYPEOF(cur) != SEXPTYPE::LANGSXP {
                break;
            }
            if !isNull(TAG(cur)) {
                let s = TAG(cur);
                if s == R_DotsSymbol() {
                    let pn = CHAR(PRINTNAME(s));
                    if !pn.is_null() {
                        print2buff(pn, d);
                    }
                } else if d.backtick != 0 {
                    let q = quotify(PRINTNAME(s), b'`' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                } else {
                    let q = quotify(PRINTNAME(s), b'"' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                }
                if formals != 0 {
                    if !isNull(CAR(cur)) && CAR(cur) != R_MissingArg() {
                        print2buff(b" = \0".as_ptr() as *const c_char, d);
                        d.fnarg = true;
                        deparse2buff(CAR(cur), d);
                    }
                } else {
                    print2buff(b" = \0".as_ptr() as *const c_char, d);
                    if !isNull(CAR(cur)) && CAR(cur) != R_MissingArg() {
                        d.fnarg = true;
                        deparse2buff(CAR(cur), d);
                    }
                }
            } else {
                d.fnarg = true;
                deparse2buff(CAR(cur), d);
            }
            cur = CDR(cur);
            if !isNull(cur) {
                print2buff(b", \0".as_ptr() as *const c_char, d);
                linebreak(&mut lbreak, d);
            }
        }
        if lbreak {
            d.indent -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// deparse2buff — recursive deparsing workhorse
// ---------------------------------------------------------------------------

/// The recursive part of deparsing. Handles all SEXP types.
///
/// This is the main recursive function that dispatches based on the SEXPTYPE
/// of the input and builds the deparsed string representation.
unsafe fn deparse2buff(s: SEXP, d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        let d_opts_in = d.opts;
        let fnarg = d.fnarg;
        d.fnarg = false;

        // This flag should only be set when recursing through the LHS
        // of binary ops, so by default we reset to zero
        let prev_left = d.left;
        d.left = 0;

        if !d.active {
            d.left = prev_left;
            return;
        }

        // S4 object handling — stubbed (needs methods infrastructure)
        // Skipping S4 handling and fall through to type-based dispatch
        let s4_check = IS_S4_OBJECT(s);
        if s4_check != 0 {
            d.isS4 = 1;
            d.sourceable = 0;
            print2buff(b"<S4 object>\0".as_ptr() as *const c_char, d);
            d.left = prev_left;
            return;
        }

        // non-S4 cases:
        let sexp_type = TYPEOF(s);

        if sexp_type == SEXPTYPE::NILSXP {
            print2buff(b"NULL\0".as_ptr() as *const c_char, d);
        } else if sexp_type == SEXPTYPE::SYMSXP {
            let doquote = (d_opts_in & QUOTEEXPRESSIONS != 0) && {
                let pn = CHAR(PRINTNAME(s));
                !pn.is_null() && *pn != 0
            };
            if doquote {
                let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
                    attr1(s, d)
                } else {
                    ATTR_SIMPLE
                };
                print2buff(b"quote(\0".as_ptr() as *const c_char, d);
                // Now print the name
                if (d_opts_in & S_COMPAT) != 0 {
                    let q = quotify(PRINTNAME(s), b'"' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                } else if d.backtick != 0 {
                    let q = quotify(PRINTNAME(s), b'`' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                } else {
                    let pn = CHAR(PRINTNAME(s));
                    if !pn.is_null() {
                        print2buff(pn, d);
                    }
                }
                print2buff(b")\0".as_ptr() as *const c_char, d);
                if attr >= ATTR_STRUC_ATTR {
                    attr2(s, d, attr == ATTR_STRUC_ATTR);
                }
            } else {
                if (d_opts_in & S_COMPAT) != 0 {
                    let q = quotify(PRINTNAME(s), b'"' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                } else if d.backtick != 0 {
                    let q = quotify(PRINTNAME(s), b'`' as c_int);
                    if !q.is_null() {
                        print2buff(q, d);
                    }
                } else {
                    let pn = CHAR(PRINTNAME(s));
                    if !pn.is_null() {
                        print2buff(pn, d);
                    }
                }
            }
        } else if sexp_type == SEXPTYPE::CHARSXP {
            let name = CHAR(s);
            if !name.is_null() {
                print2buff(name, d);
            }
        } else if sexp_type == SEXPTYPE::SPECIALSXP || sexp_type == SEXPTYPE::BUILTINSXP {
            print2buff(b".Primitive(\"\0".as_ptr() as *const c_char, d);
            let pname = primname_c(s);
            print2buff(pname, d);
            print2buff(b"\")\0".as_ptr() as *const c_char, d);
        } else if sexp_type == SEXPTYPE::PROMSXP {
            if (d.opts & DELAYPROMISES) != 0 {
                d.sourceable = 0;
                print2buff(b"<promise: \0".as_ptr() as *const c_char, d);
                d.opts &= !QUOTEEXPRESSIONS;
                // PREXPR is not available, just print <promise>
                print2buff(b">\0".as_ptr() as *const c_char, d);
            } else {
                print2buff(b"<promise>\0".as_ptr() as *const c_char, d);
            }
        } else if sexp_type == SEXPTYPE::CLOSXP {
            let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
                attr1(s, d)
            } else {
                ATTR_SIMPLE
            };
            let srcref_sym = Rf_install(b"srcref\0".as_ptr() as *const c_char);
            let t = getAttrib(s, srcref_sym);
            if (d.opts & USESOURCE != 0) && !isNull(t) {
                src2buff1(t, d);
            } else {
                d.opts &= SIMPLE_OPTS & !USESOURCE;
                print2buff(b"function (\0".as_ptr() as *const c_char, d);
                args2buff(FORMALS(s), 0, 1, d);
                print2buff(b") \0".as_ptr() as *const c_char, d);
                writeline(d);
                deparse2buff(BODY(s), d);
                d.opts = d_opts_in;
            }
            if attr >= ATTR_STRUC_ATTR {
                attr2(s, d, attr == ATTR_STRUC_ATTR);
            }
        } else if sexp_type == SEXPTYPE::ENVSXP {
            d.sourceable = 0;
            print2buff(b"<environment>\0".as_ptr() as *const c_char, d);
        } else if sexp_type == SEXPTYPE::VECSXP {
            let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
                attr1(s, d)
            } else {
                ATTR_SIMPLE
            };
            print2buff(b"list(\0".as_ptr() as *const c_char, d);
            d.opts = d_opts_in;
            vec2buff(s, d, attr == ATTR_OK_NAMES || attr == ATTR_STRUC_ATTR);
            d.opts |= NICE_NAMES;
            print2buff(b")\0".as_ptr() as *const c_char, d);
            if attr >= ATTR_STRUC_ATTR {
                attr2(s, d, attr == ATTR_STRUC_ATTR);
            }
            d.opts = d_opts_in;
        } else if sexp_type == SEXPTYPE::EXPRSXP {
            let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
                attr1(s, d)
            } else {
                ATTR_SIMPLE
            };
            if LENGTH(s) <= 0 {
                print2buff(b"expression()\0".as_ptr() as *const c_char, d);
            } else {
                let loc_opts = d.opts;
                print2buff(b"expression(\0".as_ptr() as *const c_char, d);
                d.opts &= SIMPLE_OPTS;
                vec2buff(s, d, attr == ATTR_OK_NAMES || attr == ATTR_STRUC_ATTR);
                d.opts = loc_opts;
                print2buff(b")\0".as_ptr() as *const c_char, d);
            }
            if attr >= ATTR_STRUC_ATTR {
                attr2(s, d, attr == ATTR_STRUC_ATTR);
            }
            d.opts = d_opts_in;
        } else if sexp_type == SEXPTYPE::LISTSXP {
            let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
                attr1(s, d)
            } else {
                ATTR_SIMPLE
            };
            // Check for missing args
            let mut missing = false;
            let mut t = s;
            while !isNull(t) {
                if CAR(t) == R_MissingArg() {
                    missing = true;
                    break;
                }
                t = CDR(t);
            }
            if missing {
                print2buff(b"as.pairlist(alist(\0".as_ptr() as *const c_char, d);
            } else {
                print2buff(b"pairlist(\0".as_ptr() as *const c_char, d);
            }
            d.inlist += 1;
            let mut t = s;
            while !isNull(CDR(t)) {
                if !isNull(TAG(t)) {
                    d.opts = SIMPLEDEPARSE;
                    deparse2buff(TAG(t), d);
                    d.opts = d_opts_in;
                    print2buff(b" = \0".as_ptr() as *const c_char, d);
                }
                deparse2buff(CAR(t), d);
                print2buff(b", \0".as_ptr() as *const c_char, d);
                t = CDR(t);
            }
            if !isNull(TAG(t)) {
                d.opts = SIMPLEDEPARSE;
                deparse2buff(TAG(t), d);
                d.opts = d_opts_in;
                print2buff(b" = \0".as_ptr() as *const c_char, d);
            }
            deparse2buff(CAR(t), d);
            if missing {
                print2buff(b"))\0".as_ptr() as *const c_char, d);
            } else {
                print2buff(b")\0".as_ptr() as *const c_char, d);
            }
            d.inlist -= 1;
            if attr >= ATTR_STRUC_ATTR {
                attr2(s, d, attr == ATTR_STRUC_ATTR);
            }
        } else if sexp_type == SEXPTYPE::LANGSXP {
            if !isNull(ATTRIB(s)) {
                d.sourceable = 0;
            }
            let op = CAR(s);
            let mut doquote = false;
            let maybe_quote = (d_opts_in & QUOTEEXPRESSIONS) != 0;
            if maybe_quote {
                // do *not* quote() formulas (tilde):
                let is_tilde = isSymbol(op) && {
                    let pn = CHAR(PRINTNAME(op));
                    !pn.is_null() && streql(pn, b"~\0".as_ptr() as *const c_char)
                };
                doquote = !is_tilde;
                if doquote {
                    print2buff(b"quote(\0".as_ptr() as *const c_char, d);
                    d.opts &= SIMPLE_OPTS;
                } else {
                    d.opts &= !QUOTEEXPRESSIONS;
                }
            }

            if isSymbol(op) {
                let mut userbinop = 0;
                let symval = SYMVALUE(op);
                let symval_type = TYPEOF(symval);
                let is_builtin =
                    symval_type == SEXPTYPE::BUILTINSXP || symval_type == SEXPTYPE::SPECIALSXP;
                if is_builtin {
                    userbinop = 0;
                } else if isUserBinop(op) {
                    userbinop = 1;
                } else {
                    userbinop = 0;
                }

                if is_builtin || userbinop != 0 {
                    let mut fop: PPinfo;
                    let s = CDR(s);
                    if userbinop != 0 {
                        let names_sym = Rf_install(b"names\0".as_ptr() as *const c_char);
                        if isNull(getAttrib(s, names_sym)) {
                            fop = PPinfo::new(PP_BINARY2, PREC_PERCENT, 0);
                        } else {
                            fop = PPinfo::new(PP_FUNCALL, 0, 0);
                        }
                    } else {
                        fop = getPPinfo(symval);
                    }

                    // Adjust kind based on argument count
                    match fop.kind {
                        PP_BINARY => {
                            let nargs = Rf_length(s);
                            match nargs {
                                1 => {
                                    fop.kind = PP_UNARY;
                                    if fop.prec == PREC_SUM {
                                        fop.prec = PREC_SIGN;
                                    }
                                }
                                2 => {}
                                _ => {
                                    fop.kind = PP_FUNCALL;
                                }
                            }
                        }
                        PP_BINARY2 => {
                            if Rf_length(s) != 2 {
                                fop.kind = PP_FUNCALL;
                            } else if userbinop != 0 {
                                fop.kind = PP_BINARY;
                            }
                        }
                        PP_DOLLAR => {
                            if Rf_length(s) != 2 {
                                fop.kind = PP_FUNCALL;
                            } else {
                                let rhs = CADR(s);
                                if !(isSymbol(rhs)
                                    || (isValidString(rhs) && !isNull(STRING_ELT(rhs, 0))))
                                {
                                    fop.kind = PP_FUNCALL;
                                }
                            }
                        }
                        _ => {} // intentionally unhandled: SEXPTYPE not relevant for function op detection
                    }

                    // Dispatch on operator kind
                    match fop.kind {
                        PP_IF => {
                            print2buff(b"if (\0".as_ptr() as *const c_char, d);
                            deparse2buff(CAR(s), d);
                            print2buff(b") \0".as_ptr() as *const c_char, d);
                            if d.incurly != 0 && d.inlist == 0 {
                                let lookahead = curlyahead(CADR(s));
                                if !lookahead {
                                    writeline(d);
                                    d.indent += 1;
                                }
                            }
                            if Rf_length(s) > 2 {
                                deparse2buff(CADR(s), d);
                                if d.incurly != 0 && d.inlist == 0 {
                                    writeline(d);
                                    if !curlyahead(CADR(s)) {
                                        d.indent -= 1;
                                    }
                                } else {
                                    print2buff(b" \0".as_ptr() as *const c_char, d);
                                }
                                print2buff(b"else \0".as_ptr() as *const c_char, d);
                                deparse2buff(CADDR(s), d);
                            } else {
                                deparse2buff(CADR(s), d);
                                if d.incurly != 0 && !curlyahead(CADR(s)) && d.inlist == 0 {
                                    d.indent -= 1;
                                }
                            }
                        }
                        PP_WHILE => {
                            print2buff(b"while (\0".as_ptr() as *const c_char, d);
                            deparse2buff(CAR(s), d);
                            print2buff(b") \0".as_ptr() as *const c_char, d);
                            deparse2buff(CADR(s), d);
                        }
                        PP_FOR => {
                            print2buff(b"for (\0".as_ptr() as *const c_char, d);
                            deparse2buff(CAR(s), d);
                            print2buff(b" in \0".as_ptr() as *const c_char, d);
                            deparse2buff(CADR(s), d);
                            print2buff(b") \0".as_ptr() as *const c_char, d);
                            deparse2buff(CADDR(s), d);
                        }
                        PP_REPEAT => {
                            print2buff(b"repeat \0".as_ptr() as *const c_char, d);
                            deparse2buff(CAR(s), d);
                        }
                        PP_CURLY => {
                            print2buff(b"{\0".as_ptr() as *const c_char, d);
                            d.incurly += 1;
                            d.indent += 1;
                            writeline(d);
                            let mut cur = s;
                            while !isNull(cur) {
                                deparse2buff(CAR(cur), d);
                                writeline(d);
                                cur = CDR(cur);
                            }
                            d.indent -= 1;
                            print2buff(b"}\0".as_ptr() as *const c_char, d);
                            d.incurly -= 1;
                        }
                        PP_PAREN => {
                            print2buff(b"(\0".as_ptr() as *const c_char, d);
                            deparse2buff(CAR(s), d);
                            print2buff(b")\0".as_ptr() as *const c_char, d);
                        }
                        PP_SUBSET => {
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                1,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            // Determine [ or [[
                            let primval = crate::mainutils::relop::PRIMVAL(symval);
                            if primval == 1 {
                                print2buff(b"[\0".as_ptr() as *const c_char, d);
                            } else {
                                print2buff(b"[[\0".as_ptr() as *const c_char, d);
                            }
                            args2buff(CDR(s), 0, 0, d);
                            if primval == 1 {
                                print2buff(b"]\0".as_ptr() as *const c_char, d);
                            } else {
                                print2buff(b"]]\0".as_ptr() as *const c_char, d);
                            }
                        }
                        PP_FUNCALL | PP_RETURN => {
                            if d.backtick != 0 {
                                let q = quotify(PRINTNAME(op), b'`' as c_int);
                                if !q.is_null() {
                                    print2buff(q, d);
                                }
                            } else {
                                let q = quotify(PRINTNAME(op), b'"' as c_int);
                                if !q.is_null() {
                                    print2buff(q, d);
                                }
                            }
                            print2buff(b"(\0".as_ptr() as *const c_char, d);
                            d.inlist += 1;
                            args2buff(s, 0, 0, d);
                            d.inlist -= 1;
                            print2buff(b")\0".as_ptr() as *const c_char, d);
                        }
                        PP_FOREIGN => {
                            let pn = CHAR(PRINTNAME(op));
                            if !pn.is_null() {
                                print2buff(pn, d);
                            } // ASCII
                            print2buff(b"(\0".as_ptr() as *const c_char, d);
                            d.inlist += 1;
                            args2buff(s, 1, 0, d);
                            d.inlist -= 1;
                            print2buff(b")\0".as_ptr() as *const c_char, d);
                        }
                        PP_FUNCTION => {
                            if (d.opts & USESOURCE == 0) || !isString(CADDR(s)) {
                                let pn = CHAR(PRINTNAME(op));
                                if !pn.is_null() {
                                    print2buff(pn, d);
                                } // ASCII
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                                args2buff(FORMALS(s), 0, 1, d);
                                print2buff(b") \0".as_ptr() as *const c_char, d);
                                deparse2buff(CADR(s), d);
                            } else {
                                // Use source reference
                                let src = CADDR(s);
                                let n = LENGTH(src);
                                for i in 0..n as usize {
                                    let elt = STRING_ELT(src, i as R_xlen_t);
                                    let name = CHAR(elt);
                                    if !name.is_null() {
                                        print2buff(name, d);
                                    }
                                    writeline(d);
                                }
                            }
                        }
                        PP_ASSIGN | PP_ASSIGN2 => {
                            let op_name = CHAR(PRINTNAME(op));
                            let is_eq = !op_name.is_null()
                                && streql(op_name, b"=\0".as_ptr() as *const c_char);
                            let outerparens = fnarg && is_eq;
                            if outerparens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                1,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { fop.prec };
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            print2buff(b" \0".as_ptr() as *const c_char, d);
                            if !op_name.is_null() {
                                print2buff(op_name, d);
                            } // ASCII
                            print2buff(b" \0".as_ptr() as *const c_char, d);
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CADR(s),
                                0,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { prev_left };
                            deparse2buff(CADR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            if outerparens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            d.left = 0;
                        }
                        PP_DOLLAR => {
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                1,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { fop.prec };
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            let op_name = CHAR(PRINTNAME(op));
                            if !op_name.is_null() {
                                print2buff(op_name, d);
                            } // ASCII ($)
                            // Handle x$a's
                            let rhs = CADR(s);
                            if isString(rhs) {
                                let elt = STRING_ELT(rhs, 0);
                                if !elt.is_null() {
                                    let name = CHAR(elt);
                                    if !name.is_null() && isValidName(name) {
                                        deparse2buff(elt, d);
                                    } else {
                                        let parens = needsparens(
                                            fop.kind,
                                            fop.prec,
                                            fop.rightassoc,
                                            rhs,
                                            0,
                                            prev_left,
                                        );
                                        if parens {
                                            print2buff(b"(\0".as_ptr() as *const c_char, d);
                                        }
                                        d.left = if parens { 0 } else { prev_left };
                                        deparse2buff(rhs, d);
                                        if parens {
                                            print2buff(b")\0".as_ptr() as *const c_char, d);
                                        }
                                    }
                                }
                            } else {
                                let parens = needsparens(
                                    fop.kind,
                                    fop.prec,
                                    fop.rightassoc,
                                    rhs,
                                    0,
                                    prev_left,
                                );
                                if parens {
                                    print2buff(b"(\0".as_ptr() as *const c_char, d);
                                }
                                d.left = if parens { 0 } else { prev_left };
                                deparse2buff(rhs, d);
                                if parens {
                                    print2buff(b")\0".as_ptr() as *const c_char, d);
                                }
                            }
                            d.left = 0;
                        }
                        PP_BINARY => {
                            let mut lbreak = false;
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                1,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { fop.prec };
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            print2buff(b" \0".as_ptr() as *const c_char, d);
                            let op_name = CHAR(PRINTNAME(op));
                            if !op_name.is_null() {
                                print2buff(op_name, d);
                            } // ASCII
                            print2buff(b" \0".as_ptr() as *const c_char, d);
                            linebreak(&mut lbreak, d);
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CADR(s),
                                0,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { prev_left };
                            deparse2buff(CADR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            if lbreak {
                                d.indent -= 1;
                            }
                            d.left = 0;
                        }
                        PP_BINARY2 => {
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                1,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { fop.prec };
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            let op_name = CHAR(PRINTNAME(op));
                            if !op_name.is_null() {
                                print2buff(op_name, d);
                            } // ASCII
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CADR(s),
                                0,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { prev_left };
                            deparse2buff(CADR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            d.left = 0;
                        }
                        PP_UNARY => {
                            let op_name = CHAR(PRINTNAME(op));
                            if !op_name.is_null() {
                                print2buff(op_name, d);
                            } // ASCII
                            let parens = needsparens(
                                fop.kind,
                                fop.prec,
                                fop.rightassoc,
                                CAR(s),
                                0,
                                prev_left,
                            );
                            if parens {
                                print2buff(b"(\0".as_ptr() as *const c_char, d);
                            }
                            d.left = if parens { 0 } else { prev_left };
                            deparse2buff(CAR(s), d);
                            if parens {
                                print2buff(b")\0".as_ptr() as *const c_char, d);
                            }
                            d.left = 0;
                        }
                        PP_BREAK => {
                            print2buff(b"break\0".as_ptr() as *const c_char, d);
                        }
                        PP_NEXT => {
                            print2buff(b"next\0".as_ptr() as *const c_char, d);
                        }
                        PP_SUBASS => {
                            if (d.opts & S_COMPAT) != 0 {
                                print2buff(b"\"\0".as_ptr() as *const c_char, d);
                                let op_name = CHAR(PRINTNAME(op));
                                if !op_name.is_null() {
                                    print2buff(op_name, d);
                                } // ASCII
                                print2buff(b"\'(\0".as_ptr() as *const c_char, d);
                            } else {
                                print2buff(b"`\0".as_ptr() as *const c_char, d);
                                let op_name = CHAR(PRINTNAME(op));
                                if !op_name.is_null() {
                                    print2buff(op_name, d);
                                } // ASCII
                                print2buff(b"`(\0".as_ptr() as *const c_char, d);
                            }
                            args2buff(s, 0, 0, d);
                            print2buff(b")\0".as_ptr() as *const c_char, d);
                        }
                        _ => {
                            d.sourceable = 0;
                        }
                    }
                } else {
                    // op is a symbol but not builtin/special/userbinop
                    let op_name = CHAR(PRINTNAME(op));
                    let val = if isSymbol(op) {
                        SYMVALUE(op)
                    } else {
                        R_NilValue()
                    };

                    if isSymbol(op)
                        && TYPEOF(val) == SEXPTYPE::CLOSXP
                        && !op_name.is_null()
                        && streql(op_name, b"::\0".as_ptr() as *const c_char)
                    {
                        deparse2buff(CADR(s), d);
                        print2buff(b"::\0".as_ptr() as *const c_char, d);
                        deparse2buff(CADDR(s), d);
                    } else if isSymbol(op)
                        && TYPEOF(val) == SEXPTYPE::CLOSXP
                        && !op_name.is_null()
                        && streql(op_name, b":::\0".as_ptr() as *const c_char)
                    {
                        deparse2buff(CADR(s), d);
                        print2buff(b":::\0".as_ptr() as *const c_char, d);
                        deparse2buff(CADDR(s), d);
                    } else {
                        if isSymbol(op) {
                            if (d.opts & S_COMPAT) != 0 {
                                let q = quotify(PRINTNAME(op), b'\'' as c_int);
                                if !q.is_null() {
                                    print2buff(q, d);
                                }
                            } else {
                                let q = quotify(PRINTNAME(op), b'`' as c_int);
                                if !q.is_null() {
                                    print2buff(q, d);
                                }
                            }
                        } else {
                            deparse2buff(CAR(s), d);
                        }
                        print2buff(b"(\0".as_ptr() as *const c_char, d);
                        args2buff(CDR(s), 0, 0, d);
                        print2buff(b")\0".as_ptr() as *const c_char, d);
                    }
                }
            } else if TYPEOF(op) == SEXPTYPE::CLOSXP
                || TYPEOF(op) == SEXPTYPE::SPECIALSXP
                || TYPEOF(op) == SEXPTYPE::BUILTINSXP
            {
                if parenthesizeCaller(op) {
                    print2buff(b"(\0".as_ptr() as *const c_char, d);
                    deparse2buff(op, d);
                    print2buff(b")\0".as_ptr() as *const c_char, d);
                } else {
                    deparse2buff(op, d);
                }
                print2buff(b"(\0".as_ptr() as *const c_char, d);
                args2buff(CDR(s), 0, 0, d);
                print2buff(b")\0".as_ptr() as *const c_char, d);
            } else {
                // lambda expression or other
                if parenthesizeCaller(op) {
                    print2buff(b"(\0".as_ptr() as *const c_char, d);
                    deparse2buff(op, d);
                    print2buff(b")\0".as_ptr() as *const c_char, d);
                } else {
                    deparse2buff(op, d);
                }
                print2buff(b"(\0".as_ptr() as *const c_char, d);
                args2buff(CDR(s), 0, 0, d);
                print2buff(b")\0".as_ptr() as *const c_char, d);
            }
            if maybe_quote {
                d.opts = d_opts_in;
                if doquote {
                    print2buff(b")\0".as_ptr() as *const c_char, d);
                }
            }
        } else if sexp_type == SEXPTYPE::LGLSXP
            || sexp_type == SEXPTYPE::INTSXP
            || sexp_type == SEXPTYPE::REALSXP
            || sexp_type == SEXPTYPE::CPLXSXP
            || sexp_type == SEXPTYPE::STRSXP
            || sexp_type == SEXPTYPE::RAWSXP
        {
            vector2buff(s, d);
        } else if sexp_type == SEXPTYPE::EXTPTRSXP {
            print2buff(b"<pointer: 0x0>\0".as_ptr() as *const c_char, d);
        } else if sexp_type == SEXPTYPE::BCODESXP {
            d.sourceable = 0;
            print2buff(b"<bytecode>\0".as_ptr() as *const c_char, d);
        } else if sexp_type == SEXPTYPE::WEAKREFSXP {
            d.sourceable = 0;
            print2buff(b"<weak reference>\0".as_ptr() as *const c_char, d);
        } else if sexp_type == SEXPTYPE::OBJSXP {
            d.sourceable = 0;
            print2buff(b"<object>\0".as_ptr() as *const c_char, d);
        } else {
            d.sourceable = 0;
        }

        d.left = prev_left;
    }
}

// ---------------------------------------------------------------------------
// deparse2 — setup and call deparse2buff
// ---------------------------------------------------------------------------

/// Setup deparsing state and call the recursive deparse2buff.
unsafe fn deparse2(what: SEXP, svec: SEXP, d: *mut LocalParseData) {
    unsafe {
        let d = &mut *d;
        d.strvec = svec;
        d.linenumber = 0;
        d.indent = 0;
        deparse2buff(what, d);
        writeline(d);
    }
}

// ---------------------------------------------------------------------------
// deparse1WithCutoff — core deparse engine with configurable cutoff
// ---------------------------------------------------------------------------

/// Core deparsing routine with configurable line width cutoff.
///
/// Equivalent to C's `deparse1WithCutoff()`. If abbrev is true, returns a
/// single string with at most 13 characters (for plot labelling).
#[allow(clippy::field_reassign_with_default)]
unsafe fn deparse1WithCutoff(
    call: SEXP,
    abbrev: bool,
    cutoff: c_int,
    backtick: bool,
    opts: c_int,
    nlines: c_int,
) -> SEXP {
    unsafe {
        let mut local_data = LocalParseData::default();
        local_data.cutoff = cutoff;
        local_data.backtick = if backtick { 1 } else { 0 };
        local_data.opts = opts;
        local_data.strvec = R_NilValue();

        // Ensure buffer allocation
        print2buff(b"\0".as_ptr() as *const c_char, &mut local_data);

        let mut svec = R_NilValue();
        let mut need_ellipses = false;

        if nlines > 0 {
            local_data.linenumber = nlines;
            local_data.maxlines = nlines;
        } else {
            if R_BrowseLines.with(|v| v.get()) > 0 {
                local_data.maxlines = R_BrowseLines.with(|v| v.get()) + 1;
            }
            deparse2(call, svec, &mut local_data);
            local_data.active = true;
            if R_BrowseLines.with(|v| v.get()) > 0
                && local_data.linenumber > R_BrowseLines.with(|v| v.get())
            {
                local_data.linenumber = R_BrowseLines.with(|v| v.get()) + 1;
                need_ellipses = true;
            }
        }

        svec = Rf_allocVector(SEXPTYPE::STRSXP, local_data.linenumber);
        Rf_protect(svec);

        deparse2(call, svec, &mut local_data);

        if abbrev {
            let mut data = [0u8; 14];
            let first = STRING_ELT(svec, 0);
            if !first.is_null() {
                let name = CHAR(first);
                if !name.is_null() {
                    let bytes = std::ffi::CStr::from_ptr(name).to_bytes();
                    let copy_len = std::cmp::min(bytes.len(), 10);
                    data[..copy_len].copy_from_slice(&bytes[..copy_len]);
                    data[copy_len] = 0;
                    if bytes.len() > 10 {
                        data[10] = b'.';
                        data[11] = b'.';
                        data[12] = b'.';
                        data[13] = 0;
                    } else {
                        data[copy_len] = 0;
                    }
                }
            }
            let result = Rf_mkString(data.as_ptr() as *const c_char);
            Rf_unprotect(1);
            R_FreeStringBuffer(&mut local_data.buffer);
            return result;
        } else if need_ellipses {
            let ellipsis = Rf_mkChar(b"  ...\0".as_ptr() as *const c_char);
            SET_STRING_ELT(svec, R_BrowseLines.with(|v| v.get()) as R_xlen_t, ellipsis);
        }

        Rf_unprotect(1);
        R_FreeStringBuffer(&mut local_data.buffer);
        svec
    }
}

// ---------------------------------------------------------------------------
// do_deparse — .Internal(deparse(expr, width.cutoff, backtick, .deparseOpts(control), nlines))
// ---------------------------------------------------------------------------

/// Implementation of R's `deparse()` function.
///
/// This is the equivalent of R's `do_deparse()` from deparse.c.
/// It converts an R expression to a character vector representation.
pub unsafe fn do_deparse(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, rho);
        let mut args = args;

        let expr = CAR(args);
        args = CDR(args);

        let mut cut0 = DEFAULT_CUTOFF;
        if !isNull(CAR(args)) {
            let v = Rf_asInteger(CAR(args));
            if v == NA_INTEGER || v < MIN_CUTOFF || v > MAX_CUTOFF {
                cut0 = DEFAULT_CUTOFF;
            } else {
                cut0 = v;
            }
        }
        args = CDR(args);

        let backtick = !isNull(CAR(args)) && Rf_asLogical(CAR(args)) != 0;
        args = CDR(args);

        let opts = if isNull(CAR(args)) {
            SHOWATTRIBUTES
        } else {
            Rf_asInteger(CAR(args))
        };
        args = CDR(args);

        let mut nlines = Rf_asInteger(CAR(args));
        if nlines == NA_INTEGER {
            nlines = -1;
        }

        deparse1WithCutoff(expr, false, cut0, backtick, opts, nlines)
    }
}

// ---------------------------------------------------------------------------
// do_dput — .Internal(dput(x, file, .deparseOpts(control)))
// ---------------------------------------------------------------------------

/// Implementation of R's `dput()` function.
///
/// Writes a deparsed representation of an R object to a file or connection.
/// Port of `do_dput` in deparse.c.
pub unsafe fn do_dput(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::relop::checkArity(_op, args);
        let tval_raw = CAR(args);
        let sfile = CADR(args);
        let opts_arg = CADDR(args);
        let opts = if isNull(opts_arg) {
            SHOWATTRIBUTES
        } else {
            Rf_asInteger(opts_arg)
        };

        let tval = deparse1(tval_raw, false, opts);
        Rf_protect(tval);

        // Write to stdout (connection index 1) or a connection
        let ifile = crate::mainutils::coerce::asInteger(sfile);
        if ifile == 1 {
            for i in 0..LENGTH(tval) {
                let s = CHAR(STRING_ELT(tval, i as R_xlen_t));
                if !s.is_null() {
                    let bytes = std::ffi::CStr::from_ptr(s).to_bytes();
                    let line = String::from_utf8_lossy(bytes);
                    println!("{}", line);
                }
            }
        } else if ifile >= 3 {
            // Write to a connection
            let con_sexp = sfile;
            let lines_sexp = tval;
            // Build a STRSXP with newlines appended for writeLines
            let n = LENGTH(lines_sexp);
            let text = Rf_allocVector(SEXPTYPE::STRSXP, n);
            Rf_protect(text);
            for i in 0..n as R_xlen_t {
                SET_STRING_ELT(text, i, STRING_ELT(lines_sexp, i));
            }
            crate::mainutils::connections::do_writeLines(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Rf_cons(
                    text,
                    Rf_cons(
                        con_sexp,
                        Rf_cons(Rf_mkString(b"\n\0".as_ptr() as *const c_char), R_NilValue()),
                    ),
                ),
                R_NilValue(),
            );
            Rf_unprotect(1);
        }

        Rf_unprotect(1);
        CAR(args)
    }
}

/// Implementation of R's `dump()` function.
///
/// Writes deparsed representations of named R objects to a file or connection.
/// Port of `do_dump` in deparse.c.
pub unsafe fn do_dump(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::relop::checkArity(_op, args);
        let names = CAR(args);
        let sfile = CADR(args);
        let _source = CADDR(args);
        let opts = Rf_asInteger(CADDDR(args));
        let _evaluate = CAR(CDR(CDR(CDR(CDR(args)))));

        if !isString(names) {
            return R_NilValue();
        }
        let nobjs = LENGTH(names);
        if nobjs < 1 {
            return R_NilValue();
        }

        let ifile = crate::mainutils::coerce::asInteger(sfile);

        for i in 0..nobjs as R_xlen_t {
            let name_charsxp = STRING_ELT(names, i);
            if name_charsxp.is_null() {
                continue;
            }
            let obj_name = CHAR(name_charsxp);
            if obj_name.is_null() {
                continue;
            }
            let name_str = std::ffi::CStr::from_ptr(obj_name)
                .to_string_lossy()
                .into_owned();

            // Deparse the object — in this port we deparse the name itself as a symbol
            let sym = Rf_install(obj_name);
            let tval = deparse1(
                sym,
                false,
                if opts == NA_INTEGER {
                    DEFAULTDEPARSE
                } else {
                    opts
                },
            );
            Rf_protect(tval);

            if ifile == 1 {
                if isValidName(obj_name) {
                    println!("{} <-", name_str);
                } else {
                    println!("`{}` <-", name_str);
                }
                for j in 0..LENGTH(tval) {
                    let s = CHAR(STRING_ELT(tval, j as R_xlen_t));
                    if !s.is_null() {
                        let bytes = std::ffi::CStr::from_ptr(s).to_bytes();
                        let line = String::from_utf8_lossy(bytes);
                        println!("{}", line);
                    }
                }
            }

            Rf_unprotect(1);
        }

        let outnames = Rf_allocVector(SEXPTYPE::STRSXP, nobjs);
        for i in 0..nobjs as R_xlen_t {
            SET_STRING_ELT(outnames, i, STRING_ELT(names, i));
        }
        outnames
    }
}

// ---------------------------------------------------------------------------
// deparse1 — deparse with R_BrowseLines := 0
// ---------------------------------------------------------------------------

/// Deparse an expression with default cutoff (60), no line limit.
///
/// Used in bind.c, builtin.c, coerce.c, match.c, relop.c, and do_dput/do_dump.
pub unsafe fn deparse1(call: SEXP, abbrev: bool, opts: c_int) -> SEXP {
    unsafe {
        let old_bl = R_BrowseLines.with(|v| v.get());
        R_BrowseLines.with(|v| v.set(0));
        let result = deparse1WithCutoff(call, abbrev, DEFAULT_CUTOFF, true, opts, 0);
        R_BrowseLines.with(|v| v.set(old_bl));
        result
    }
}

// ---------------------------------------------------------------------------
// deparse1m — deparse looking at getOption("deparse.max.lines")
// ---------------------------------------------------------------------------

/// Deparse with default cutoff, respecting getOption("deparse.max.lines").
///
/// Unimplemented: requires getOption infrastructure.
pub unsafe fn deparse1m(call: SEXP, abbrev: bool, opts: c_int) -> SEXP {
    unsafe {
        let old_bl = R_BrowseLines.with(|v| v.get());
        let max_lines = {
            let val = crate::mainutils::options::GetOption(
                b"deparse.max.lines\0".as_ptr() as *const c_char
            );
            let n = crate::mainutils::coerce::asInteger(val);
            if n == NA_INTEGER { 100 } else { n }
        };
        R_BrowseLines.with(|v| v.set(max_lines));
        let result = deparse1WithCutoff(call, abbrev, DEFAULT_CUTOFF, true, opts, 0);
        R_BrowseLines.with(|v| v.set(old_bl));
        result
    }
}

// ---------------------------------------------------------------------------
// deparse1w — deparse for print() (uses R_print.cutoff)
// ---------------------------------------------------------------------------

/// Deparse for printing language objects (uses R_print.cutoff, nlines = -1).
///
/// Used in print.c for PrintLanguage, PrintClosure, PrintExpression.
pub unsafe fn deparse1w(call: SEXP, abbrev: bool, opts: c_int) -> SEXP {
    unsafe {
        // Use DEFAULT_CUTOFF since R_print.cutoff is not yet available as a global
        deparse1WithCutoff(call, abbrev, DEFAULT_CUTOFF, true, opts, -1)
    }
}

// ---------------------------------------------------------------------------
// deparse1line — concatenate all deparse lines into one
// ---------------------------------------------------------------------------

/// Deparse and concatenate all lines into a single string.
///
/// Used for non-trivial list entries in as.character(<list>) and in
/// terms.formula where a term label must be a single line.
pub unsafe fn deparse1line(call: SEXP, abbrev: bool) -> SEXP {
    unsafe {
        let temp = deparse1WithCutoff(call, abbrev, MAX_CUTOFF, true, SIMPLEDEPARSE, -1);
        Rf_protect(temp);
        let lines = LENGTH(temp);
        if lines > 1 {
            // Calculate total length
            let mut total_len: usize = 0;
            for i in 0..lines as usize {
                let s = STRING_ELT(temp, i as R_xlen_t);
                if !s.is_null() {
                    let name = CHAR(s);
                    if !name.is_null() {
                        total_len += libc::strlen(name);
                    }
                }
                total_len += 1; // newline
            }
            // Allocate buffer and concatenate
            let mut buf = vec![0u8; total_len + 1];
            let mut pos = 0;
            for i in 0..lines as usize {
                let s = STRING_ELT(temp, i as R_xlen_t);
                if !s.is_null() {
                    let name = CHAR(s);
                    if !name.is_null() {
                        let bytes = std::ffi::CStr::from_ptr(name).to_bytes();
                        for &b in bytes.iter() {
                            if pos < buf.len() {
                                buf[pos] = b;
                            }
                            pos += 1;
                        }
                    }
                }
                if i < (lines as usize) - 1 && pos < buf.len() {
                    buf[pos] = b'\n';
                    pos += 1;
                }
            }
            if pos < buf.len() {
                buf[pos] = 0;
            }
            let result = Rf_mkString(buf.as_ptr() as *const c_char);
            Rf_unprotect(1);
            result
        } else {
            Rf_unprotect(1);
            temp
        }
    }
}

// ---------------------------------------------------------------------------
// deparse1s — deparse for error/warning messages (single line)
// ---------------------------------------------------------------------------

/// Deparse for error/warning messages (single line, default deparse options).
///
/// Used in errors.c for warningcall_dflt() and PrintWarnings().
pub unsafe fn deparse1s(call: SEXP) -> SEXP {
    unsafe { deparse1WithCutoff(call, false, DEFAULT_CUTOFF, true, DEFAULTDEPARSE, 1) }
}

// ---------------------------------------------------------------------------
// R_inspect — inspect an R object (from inspect.c)
// ---------------------------------------------------------------------------

/// Inspect an R object, returning a string representation.
///
/// Unimplemented: requires full inspect infrastructure.
pub unsafe fn R_inspect(s: SEXP, deep: c_int, pvec: SEXP) -> c_int {
    let _ = (s, deep, pvec);
    0
}

/// R_inspect3 — inspect with additional options.
///
/// Unimplemented: requires full inspect infrastructure.
pub unsafe fn R_inspect3(
    s: SEXP,
    deep: c_int,
    pvec: SEXP,
    writefun: SEXP,
    callfun: SEXP,
    env: SEXP,
) -> c_int {
    let _ = (s, deep, pvec, writefun, callfun, env);
    0
}

// ---------------------------------------------------------------------------
// con_cleanup — connection cleanup handler (for do_dput/do_dump)
// ---------------------------------------------------------------------------

/// Connection cleanup handler used in do_dput and do_dump.
/// Closes the connection identified by the data pointer (an INTSXP containing
/// the connection index) if it was opened by the deparse routine.
///
/// Port of `con_cleanup` in deparse.c:378.
pub unsafe fn con_cleanup(data: *mut std::ffi::c_void) {
    unsafe {
        if data.is_null() {
            return;
        }
        let scon = data as SEXP;
        if scon.is_null() {
            return;
        }
        crate::mainutils::connections::do_close(
            scon,
            std::ptr::null_mut(),
            scon,
            std::ptr::null_mut(),
        );
    }
}

// ---------------------------------------------------------------------------
// R_BrowseLines — global for max lines in deparsing
// ---------------------------------------------------------------------------

thread_local! { pub static R_BrowseLines: Cell<c_int> = Cell::new(0); }

// ---------------------------------------------------------------------------
// Additional helper stubs needed by other modules
// ---------------------------------------------------------------------------

/// Rf_isValidName — check if a string is a valid R name.
///
/// Exported for use by other modules.
pub unsafe fn Rf_isValidName(s: *const c_char) -> c_int {
    unsafe { if isValidName(s) { 1 } else { 0 } }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(BUFSIZE, 512);
        assert_eq!(MIN_CUTOFF, 20);
        assert_eq!(DEFAULT_CUTOFF, 60);
        assert_eq!(MAX_CUTOFF, BUFSIZE - 12);
    }

    #[test]
    fn test_deparse_option_flags() {
        assert!(KEEPNA == 1);
        assert!(KEEPINTEGER == 2);
        assert!(SHOWATTRIBUTES == 4);
        assert!(USESOURCE == 8);
        assert!(DELAYPROMISES == 16);
        assert!(S_COMPAT == 32);
        assert!(QUOTEEXPRESSIONS == 64);
        assert!(HEXNUMERIC == 128);
        assert!(DIGITS17 == 256);
        assert!(NICE_NAMES == 512);
        assert!(WARNINCOMPLETE == 1024);
    }

    #[test]
    fn test_precedence_constants() {
        assert!(PREC_COMPARE < PREC_SUM);
        assert!(PREC_SUM < PREC_SIGN);
        assert!(PREC_SUBSET > PREC_SIGN);
    }

    #[test]
    fn test_ppinfo_kinds_are_distinct() {
        let kinds = [
            PP_BINARY,
            PP_BINARY2,
            PP_UNARY,
            PP_SUBSET,
            PP_SUBASS,
            PP_DOLLAR,
            PP_ASSIGN,
            PP_ASSIGN2,
            PP_IF,
            PP_WHILE,
            PP_FOR,
            PP_REPEAT,
            PP_FUNCALL,
            PP_RETURN,
            PP_PAREN,
            PP_CURLY,
            PP_FOREIGN,
            PP_FUNCTION,
            PP_BREAK,
            PP_NEXT,
        ];
        // Verify all kinds are distinct
        for i in 0..kinds.len() {
            for j in (i + 1)..kinds.len() {
                assert_ne!(kinds[i], kinds[j], "PPinfo kinds must be distinct");
            }
        }
    }

    #[test]
    fn test_attr_type_constants() {
        assert_eq!(ATTR_UNKNOWN, -1);
        assert_eq!(ATTR_SIMPLE, 0);
        assert_eq!(ATTR_OK_NAMES, 1);
        assert_eq!(ATTR_STRUC_ATTR, 2);
        assert_eq!(ATTR_STRUC_NMS_A, 3);
    }

    #[test]
    fn test_local_parse_data_default() {
        let d = LocalParseData::default();
        assert_eq!(d.linenumber, 0);
        assert_eq!(d.len, 0);
        assert_eq!(d.incurly, 0);
        assert_eq!(d.inlist, 0);
        assert!(d.startline);
        assert_eq!(d.indent, 0);
        assert_eq!(d.cutoff, DEFAULT_CUTOFF);
        assert_eq!(d.backtick, 0);
        assert_eq!(d.opts, 0);
        assert_eq!(d.sourceable, 1);
        assert_eq!(d.maxlines, c_int::MAX);
        assert!(d.active);
        assert_eq!(d.isS4, 0);
        assert!(!d.fnarg);
    }

    #[test]
    fn test_do_deparse_simple_expr() {
        unsafe {
            // Create a simple expression: 1L
            let expr = Rf_ScalarInteger(1);
            Rf_protect(expr);
            let args = Rf_cons(
                expr,
                Rf_cons(
                    Rf_ScalarInteger(DEFAULT_CUTOFF),
                    Rf_cons(
                        Rf_ScalarLogical(0),
                        Rf_cons(
                            Rf_ScalarInteger(SHOWATTRIBUTES),
                            Rf_cons(Rf_ScalarInteger(-1), R_NilValue()),
                        ),
                    ),
                ),
            );
            Rf_protect(args);
            let result = do_deparse(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            // Should return a character vector (STRSXP)
            assert!(!result.is_null());
            Rf_unprotect(2);
        }
    }

    #[test]
    fn test_do_dput_returns_nil() {
        unsafe {
            let result = do_dput(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn test_do_dump_returns_nil() {
        unsafe {
            let result = do_dump(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn test_simple_opts_mask() {
        let opts = KEEPNA | KEEPINTEGER | USESOURCE | S_COMPAT | WARNINCOMPLETE | NICE_NAMES;
        assert_eq!(opts & SIMPLE_OPTS, opts);
    }

    #[test]
    fn test_show_attr_or_nms() {
        assert_eq!(SHOW_ATTR_OR_NMS, SHOWATTRIBUTES | NICE_NAMES);
    }

    #[test]
    fn test_browse_lines_initial() {
        assert_eq!(R_BrowseLines.with(|v| v.get()), 0);
    }

    #[test]
    fn test_is_valid_name() {
        unsafe {
            assert!(isValidName(b"foo\0".as_ptr() as *const c_char));
            assert!(isValidName(b".foo\0".as_ptr() as *const c_char));
            assert!(isValidName(b"foo_bar\0".as_ptr() as *const c_char));
            assert!(isValidName(b"foo.bar\0".as_ptr() as *const c_char));
            assert!(isValidName(b"foo1\0".as_ptr() as *const c_char));
            assert!(!isValidName(b"1foo\0".as_ptr() as *const c_char));
            assert!(!isValidName(b"foo bar\0".as_ptr() as *const c_char));
            assert!(!isValidName(b"\0".as_ptr() as *const c_char));
            assert!(!isValidName(ptr::null()));
        }
    }

    #[test]
    fn test_streql() {
        unsafe {
            assert!(streql(
                b"foo\0".as_ptr() as *const c_char,
                b"foo\0".as_ptr() as *const c_char
            ));
            assert!(!streql(
                b"foo\0".as_ptr() as *const c_char,
                b"bar\0".as_ptr() as *const c_char
            ));
            assert!(!streql(ptr::null(), b"foo\0".as_ptr() as *const c_char));
            assert!(!streql(b"foo\0".as_ptr() as *const c_char, ptr::null()));
        }
    }

    #[test]
    fn test_ppinfo_values_from_names_rs() {
        // Verify our PPkind constants match names.rs
        assert_eq!(PP_BINARY, N_PP_BINARY);
        assert_eq!(PP_UNARY, N_PP_UNARY);
        assert_eq!(PP_SUBSET, N_PP_SUBSET);
        assert_eq!(PP_DOLLAR, N_PP_DOLLAR);
        assert_eq!(PP_IF, N_PP_IF);
        assert_eq!(PP_WHILE, N_PP_WHILE);
        assert_eq!(PP_FOR, N_PP_FOR);
        assert_eq!(PP_REPEAT, N_PP_REPEAT);
        assert_eq!(PP_FUNCALL, N_PP_FUNCALL);
        assert_eq!(PP_RETURN, N_PP_RETURN);
        assert_eq!(PP_PAREN, N_PP_PAREN);
        assert_eq!(PP_CURLY, N_PP_CURLY);
        assert_eq!(PP_FUNCTION, N_PP_FUNCTION);
        assert_eq!(PP_BREAK, N_PP_BREAK);
        assert_eq!(PP_NEXT, N_PP_NEXT);
        // Verify precedence values match
        assert_eq!(PREC_COMPARE, N_PREC_COMPARE);
        assert_eq!(PREC_SUM, N_PREC_SUM);
        assert_eq!(PREC_SIGN, N_PREC_SIGN);
        assert_eq!(PREC_PERCENT, N_PREC_PERCENT);
        assert_eq!(PREC_SUBSET, N_PREC_SUBSET);
    }
}
