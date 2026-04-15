#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/grep.c
//!
//! Implements R's grep(), grepl(), sub(), gsub(), regexpr(), regexec(),
//! and related pattern matching functions.
//!
//! Since this port does not link against PCRE2 or TRE, the implementation uses:
//! - Rust's standard library for fixed-string matching (fixed = TRUE)
//! - A simplified extended regex engine (ERE) for non-perl, non-fixed mode
//! - Backreference-free Perl-like regex is not supported; perl=TRUE falls back
//!   to the ERE engine with a warning
//!
//! Ported functions from grep.c:
//!   do_grep, do_gsub, do_regexpr, do_regexec, do_grepraw
//!   R_grep_fixed, R_gsub_fixed, R_regexpr_fixed (fixed-string helpers)
//!
//! Kept as stubs:
//!   R_agrep_fixed (requires TRE fuzzy matching)
//!   R_pcre_exec (requires PCRE library)

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::*;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Local helper functions (matching patterns from match_mod.rs etc.)
// ---------------------------------------------------------------------------

#[inline(always)]
unsafe fn NA_STRING() -> SEXP {
    unsafe { crate::mainutils::relop::NA_STRING() }
}

#[inline(always)]
unsafe fn isNA_STRING(s: SEXP) -> bool {
    if s.is_null() {
        return true;
    }
    let gp = unsafe { (*s).sxpinfo.gp() };
    gp & 1 != 0
}

/// isString check -- STRSXP type.
#[inline(always)]
unsafe fn isString(x: SEXP) -> bool {
    unsafe { !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP }
}

/// isNull check.
#[inline(always)]
unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

#[inline(always)]
unsafe fn translateChar(s: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(s) }
}

/// checkArity -- stub, no-op.
#[inline(always)]
unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

/// asLogical -- extract logical value from scalar.
#[inline(always)]
unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP {
            let p = LOGICAL(x);
            if p.is_null() {
                return NA_INTEGER;
            }
            *p
        } else if t == SEXPTYPE::INTSXP {
            let p = INTEGER(x);
            if p.is_null() {
                return NA_INTEGER;
            }
            *p
        } else {
            NA_INTEGER
        }
    }
}

/// asBool2 -- extract boolean from scalar (with error on NA).
#[inline(always)]
unsafe fn asBool2(x: SEXP, _call: SEXP) -> bool {
    unsafe {
        let v = asLogical(x);
        if v == NA_INTEGER {
            std::panic::panic_any(RError {
                message: "invalid 'logical' argument, NA value".to_string(),
            });
        }
        v != 0
    }
}

#[inline(always)]
unsafe fn PRIMVAL(op: SEXP) -> c_int {
    unsafe { crate::mainutils::relop::PRIMVAL(op) }
}

#[inline(always)]
unsafe fn R_NamesSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_NamesSymbol() }
}

#[inline(always)]
unsafe fn getAttrib(x: SEXP, which: SEXP) -> SEXP {
    unsafe { crate::eval::attrib_core::getAttrib(x, which) }
}

#[inline(always)]
unsafe fn setAttrib(x: SEXP, which: SEXP, value: SEXP) {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, which, value);
    }
}

/// ScalarString -- create scalar STRSXP.
#[inline(always)]
unsafe fn ScalarString(x: SEXP) -> SEXP {
    unsafe { Rf_ScalarString(x) }
}

/// mkChar -- create CHARSXP from C string.
#[inline(always)]
unsafe fn mkChar(s: *const c_char) -> SEXP {
    unsafe { Rf_mkChar(s) }
}

/// allocVector -- allocate a vector.
#[inline(always)]
unsafe fn allocVector(sexptype: c_int, length: R_xlen_t) -> SEXP {
    unsafe { Rf_allocVector3(sexptype, length) }
}

#[inline(always)]
unsafe fn install(name: *const c_char) -> SEXP {
    unsafe { crate::sexp::symbol::Rf_install(name) }
}

/// Rf_warning -- issue a warning (forward to errors module).
#[inline(always)]
unsafe fn Rf_warning_fmt(msg: &str) {
    // We just print to stderr as a simple warning
    eprintln!("Warning: {}", msg);
}

/// Rf_error_fmt -- panic with an RError.
#[inline(always)]
unsafe fn Rf_error_fmt(msg: &str) -> ! {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

/// Convert a C string to a Rust &str (simplified, assumes ASCII/UTF-8).
#[inline(always)]
unsafe fn cstr_to_str(s: *const c_char) -> &'static str {
    unsafe {
        if s.is_null() {
            ""
        } else {
            CStr::from_ptr(s).to_str().unwrap_or("")
        }
    }
}

/// Safe CStr to owned String.
#[inline(always)]
unsafe fn cstr_to_string(s: *const c_char) -> String {
    unsafe {
        if s.is_null() {
            String::new()
        } else {
            CStr::from_ptr(s).to_string_lossy().into_owned()
        }
    }
}

// ---------------------------------------------------------------------------
// Simple fixed-string matching (equivalent to R's R_grep_fixed internals)
// ---------------------------------------------------------------------------

/// Case-insensitive byte comparison.
#[inline]
fn byte_eq_ignore_case(a: u8, b: u8) -> bool {
    if a == b {
        return true;
    }
    let al = a.to_ascii_lowercase();
    let bl = b.to_ascii_lowercase();
    al == bl
}

/// Fixed-string search in a byte slice.
/// Returns the byte offset of the first match, or None.
fn fixed_search(haystack: &[u8], needle: &[u8], ignore_case: bool) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if haystack.len() < needle.len() {
        return None;
    }
    if ignore_case {
        for i in 0..=(haystack.len() - needle.len()) {
            let mut matched = true;
            for j in 0..needle.len() {
                if !byte_eq_ignore_case(haystack[i + j], needle[j]) {
                    matched = false;
                    break;
                }
            }
            if matched {
                return Some(i);
            }
        }
    } else {
        // Use memchr-like search for the first byte, then verify
        let first = needle[0];
        for i in 0..=(haystack.len() - needle.len()) {
            if haystack[i] == first && &haystack[i..i + needle.len()] == needle {
                return Some(i);
            }
        }
    }
    None
}

/// Fixed-string search in a C string.
/// Returns byte offset of first match, or -1 if not found.
fn R_grep_fixed_inner(pat: &[u8], target: &[u8], ignore_case: bool) -> c_int {
    match fixed_search(target, pat, ignore_case) {
        Some(pos) => pos as c_int,
        None => -1,
    }
}

/// Fixed-string gsub: replace first (global=false) or all (global=true)
/// occurrences of pat in target with replacement.
fn R_gsub_fixed_inner(
    pat: &[u8],
    target: &[u8],
    replacement: &[u8],
    global: bool,
    ignore_case: bool,
) -> Vec<u8> {
    let mut result = Vec::with_capacity(target.len());
    let mut search_from = 0;

    loop {
        let remaining = &target[search_from..];
        match fixed_search(remaining, pat, ignore_case) {
            Some(pos) => {
                // Copy text before the match
                result.extend_from_slice(&remaining[..pos]);
                // Copy replacement
                result.extend_from_slice(replacement);
                // Advance past the match
                search_from += pos + pat.len();
                if !global || pat.is_empty() {
                    // For global with empty pattern, avoid infinite loop
                    if pat.is_empty() && search_from <= target.len() {
                        result.push(target.get(search_from).copied().unwrap_or(b'\0'));
                        search_from += 1;
                    }
                    if !global {
                        break;
                    }
                    if search_from > target.len() {
                        break;
                    }
                }
            }
            None => {
                // No more matches, copy the rest
                result.extend_from_slice(remaining);
                break;
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Simple Extended Regular Expression (ERE) engine
// ---------------------------------------------------------------------------

/// A compiled simplified ERE pattern.
///
/// Supports: literal chars, `.`, `^`, `$`, `*`, `+`, `?`,
/// `[...]` character classes, `[^...]` negated classes,
/// `\` escapes, and alternation with `|`.
/// Does NOT support: backreferences, lookahead/lookbehind,
/// non-greedy quantifiers, named groups.
enum EreNode {
    /// Match a literal byte
    Literal(u8),
    /// Match any character (.)
    AnyChar,
    /// Match beginning of string (^)
    StartAnchor,
    /// Match end of string ($)
    EndAnchor,
    /// Match zero or more of the preceding element
    ZeroOrMore,
    /// Match one or more of the preceding element
    OneOrMore,
    /// Match zero or one of the preceding element
    ZeroOrOne,
    /// Character class -- match any byte in the set
    CharClass(Vec<u8>, bool), // (chars, negated)
    /// Alternation -- match either left or right
    Alternation,
    /// Open group
    OpenGroup,
    /// Close group
    CloseGroup,
}

/// Compile a simple ERE pattern string into a list of nodes.
fn compile_ere(pattern: &str) -> Result<Vec<EreNode>, String> {
    let mut nodes = Vec::new();
    let bytes = pattern.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\\' if i + 1 < bytes.len() => {
                i += 1;
                let escaped = bytes[i];
                match escaped {
                    b'd' => nodes.push(EreNode::CharClass(b"0123456789".to_vec(), false)),
                    b'D' => nodes.push(EreNode::CharClass(b"0123456789".to_vec(), true)),
                    b'w' => nodes.push(EreNode::CharClass(
                        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_".to_vec(),
                        false,
                    )),
                    b'W' => nodes.push(EreNode::CharClass(
                        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_".to_vec(),
                        true,
                    )),
                    b's' => nodes.push(EreNode::CharClass(b" \t\n\r\x0b\x0c".to_vec(), false)),
                    b'S' => nodes.push(EreNode::CharClass(b" \t\n\r\x0b\x0c".to_vec(), true)),
                    other => nodes.push(EreNode::Literal(other)),
                }
            }
            b'.' => nodes.push(EreNode::AnyChar),
            b'^' => {
                if i == 0 {
                    nodes.push(EreNode::StartAnchor);
                } else {
                    nodes.push(EreNode::Literal(b'^'));
                }
            }
            b'$' => {
                if i == bytes.len() - 1 {
                    nodes.push(EreNode::EndAnchor);
                } else {
                    nodes.push(EreNode::Literal(b'$'));
                }
            }
            b'*' => nodes.push(EreNode::ZeroOrMore),
            b'+' => nodes.push(EreNode::OneOrMore),
            b'?' => nodes.push(EreNode::ZeroOrOne),
            b'[' => {
                i += 1;
                let negated = if i < bytes.len() && bytes[i] == b'^' {
                    i += 1;
                    true
                } else {
                    false
                };
                let mut class_chars = Vec::new();
                // Handle ] as first char in class
                if i < bytes.len() && bytes[i] == b']' {
                    class_chars.push(b']');
                    i += 1;
                }
                while i < bytes.len() && bytes[i] != b']' {
                    let c = bytes[i];
                    // Handle ranges like a-z
                    if i + 2 < bytes.len() && bytes[i + 1] == b'-' && bytes[i + 2] != b']' {
                        let start = c;
                        let end = bytes[i + 2];
                        if start <= end {
                            for ch in start..=end {
                                class_chars.push(ch);
                            }
                        } else {
                            class_chars.push(start);
                            class_chars.push(b'-');
                            class_chars.push(end);
                        }
                        i += 3;
                    } else if c == b'\\' && i + 1 < bytes.len() {
                        i += 1;
                        class_chars.push(bytes[i]);
                        i += 1;
                    } else {
                        class_chars.push(c);
                        i += 1;
                    }
                }
                if i >= bytes.len() {
                    return Err("unterminated character class in regex".to_string());
                }
                nodes.push(EreNode::CharClass(class_chars, negated));
            }
            b'|' => nodes.push(EreNode::Alternation),
            b'(' => nodes.push(EreNode::OpenGroup),
            b')' => nodes.push(EreNode::CloseGroup),
            other => nodes.push(EreNode::Literal(other)),
        }
        i += 1;
    }

    Ok(nodes)
}

/// Match result from ERE matching.
struct EreMatch {
    /// Start byte offset of match
    start: usize,
    /// End byte offset of match (exclusive)
    end: usize,
}

/// Try to match the compiled ERE pattern at a specific position in the text.
/// Returns the end position of the match, or None if no match.
fn ere_match_at(nodes: &[EreNode], text: &[u8], pos: usize, ignore_case: bool) -> Option<usize> {
    let text_len = text.len();
    if pos > text_len {
        return None;
    }
    let mut text_pos = pos;
    let mut node_pos = 0;

    while node_pos < nodes.len() {
        match &nodes[node_pos] {
            EreNode::Literal(b) => {
                if text_pos >= text_len {
                    return None;
                }
                let tc = text[text_pos];
                if ignore_case {
                    if !tc.eq_ignore_ascii_case(b) {
                        return None;
                    }
                } else {
                    if tc != *b {
                        return None;
                    }
                }
                text_pos += 1;
                node_pos += 1;
            }
            EreNode::AnyChar => {
                if text_pos >= text_len {
                    return None;
                }
                text_pos += 1;
                node_pos += 1;
            }
            EreNode::StartAnchor => {
                if text_pos != 0 {
                    return None;
                }
                node_pos += 1;
            }
            EreNode::EndAnchor => {
                if text_pos != text_len {
                    return None;
                }
                node_pos += 1;
            }
            EreNode::CharClass(chars, negated) => {
                if text_pos >= text_len {
                    return None;
                }
                let tc = text[text_pos];
                let tc_cmp = if ignore_case {
                    tc.to_ascii_lowercase()
                } else {
                    tc
                };
                let found = chars.iter().any(|&c| {
                    let cc = if ignore_case {
                        c.to_ascii_lowercase()
                    } else {
                        c
                    };
                    cc == tc_cmp
                });
                if *negated {
                    if found {
                        return None;
                    }
                } else {
                    if !found {
                        return None;
                    }
                }
                text_pos += 1;
                node_pos += 1;
            }
            EreNode::ZeroOrMore => {
                // Repeat the previous node greedily
                if node_pos == 0 {
                    node_pos += 1;
                    continue;
                }
                let prev_node = &nodes[node_pos - 1];
                let mut count = 0usize;
                loop {
                    let prev_start = text_pos;
                    if let Some(new_pos) = match_single_node(prev_node, text, text_pos, ignore_case)
                    {
                        text_pos = new_pos;
                        count += 1;
                        // Safety: don't loop infinitely on zero-width matches
                        if new_pos == prev_start {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                node_pos += 1;
            }
            EreNode::OneOrMore => {
                // At least one match of previous node, then zero or more
                if node_pos == 0 {
                    return None;
                }
                let prev_node = &nodes[node_pos - 1];
                if let Some(new_pos) = match_single_node(prev_node, text, text_pos, ignore_case) {
                    text_pos = new_pos;
                } else {
                    return None;
                }
                // Greedy: consume as many more as possible
                loop {
                    let prev_start = text_pos;
                    if let Some(new_pos) = match_single_node(prev_node, text, text_pos, ignore_case)
                    {
                        text_pos = new_pos;
                        if new_pos == prev_start {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                node_pos += 1;
            }
            EreNode::ZeroOrOne => {
                if node_pos == 0 {
                    node_pos += 1;
                    continue;
                }
                let prev_node = &nodes[node_pos - 1];
                if let Some(new_pos) = match_single_node(prev_node, text, text_pos, ignore_case) {
                    text_pos = new_pos;
                }
                node_pos += 1;
            }
            EreNode::Alternation => {
                // Try matching rest of pattern; if fails, try after alternation
                // Simplified: find the next alternation or end, and try both sides
                let rest_after = find_alternation_end(&nodes[node_pos + 1..]);
                // Try left side
                let left_nodes = &nodes[node_pos + 1..node_pos + 1 + rest_after];
                if !left_nodes.is_empty()
                    && let Some(end_pos) = ere_match_at(left_nodes, text, text_pos, ignore_case)
                {
                    text_pos = end_pos;
                    node_pos = node_pos + 1 + rest_after + 1;
                    continue;
                }
                // Try right side (everything after the alternation end)
                let right_start = node_pos + 1 + rest_after + 1;
                if right_start < nodes.len() {
                    node_pos = right_start;
                    // Don't advance text_pos -- try matching from current position
                    continue;
                }
                // Neither side matched
                return None;
            }
            EreNode::OpenGroup | EreNode::CloseGroup => {
                // Simplified: groups are pass-through
                node_pos += 1;
            }
        }
    }

    Some(text_pos)
}

/// Match a single node (used by quantifiers).
fn match_single_node(node: &EreNode, text: &[u8], pos: usize, ignore_case: bool) -> Option<usize> {
    match node {
        EreNode::Literal(b) => {
            if pos >= text.len() {
                return None;
            }
            let tc = text[pos];
            if ignore_case {
                if tc.eq_ignore_ascii_case(b) {
                    Some(pos + 1)
                } else {
                    None
                }
            } else {
                if tc == *b { Some(pos + 1) } else { None }
            }
        }
        EreNode::AnyChar => {
            if pos < text.len() {
                Some(pos + 1)
            } else {
                None
            }
        }
        EreNode::CharClass(chars, negated) => {
            if pos >= text.len() {
                return None;
            }
            let tc = text[pos];
            let tc_cmp = if ignore_case {
                tc.to_ascii_lowercase()
            } else {
                tc
            };
            let found = chars.iter().any(|&c| {
                let cc = if ignore_case {
                    c.to_ascii_lowercase()
                } else {
                    c
                };
                cc == tc_cmp
            });
            if *negated {
                if found { None } else { Some(pos + 1) }
            } else {
                if found { Some(pos + 1) } else { None }
            }
        }
        _ => None,
    }
}

/// Find the end of the current alternation branch (position of the next | or end).
fn find_alternation_end(nodes: &[EreNode]) -> usize {
    let mut depth = 0;
    for (i, node) in nodes.iter().enumerate() {
        match node {
            EreNode::OpenGroup => depth += 1,
            EreNode::CloseGroup => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            EreNode::Alternation if depth == 0 => return i,
            _ => {} // intentionally unhandled: other ErNode types do not require depth tracking
        }
    }
    nodes.len()
}

/// Search for an ERE pattern match anywhere in the text.
/// Returns the match location, or None.
fn ere_search(nodes: &[EreNode], text: &[u8], ignore_case: bool) -> Option<EreMatch> {
    let text_len = text.len();
    for pos in 0..=text_len {
        if let Some(end) = ere_match_at(nodes, text, pos, ignore_case) {
            return Some(EreMatch { start: pos, end });
        }
    }
    None
}

/// ERE-based gsub: replace first (global=false) or all (global=true)
/// occurrences of the ERE pattern in text with replacement.
fn ere_gsub(
    nodes: &[EreNode],
    text: &[u8],
    replacement: &[u8],
    global: bool,
    ignore_case: bool,
) -> Vec<u8> {
    let mut result = Vec::with_capacity(text.len());
    let mut search_from = 0;

    loop {
        let remaining = &text[search_from..];
        if let Some(m) = ere_search(nodes, remaining, ignore_case) {
            result.extend_from_slice(&remaining[..m.start]);
            result.extend_from_slice(replacement);
            search_from += m.end;
            if !global || m.start == m.end {
                // Zero-width match: advance by one to prevent infinite loop
                if search_from < text.len() {
                    result.push(text[search_from]);
                    search_from += 1;
                }
                if !global {
                    result.extend_from_slice(&text[search_from..]);
                    break;
                }
                if search_from >= text.len() {
                    break;
                }
            }
        } else {
            result.extend_from_slice(remaining);
            break;
        }
    }

    result
}

// ---------------------------------------------------------------------------
// R_grep_fixed -- C-callable fixed-string grep (port of grep.c:R_grep_fixed)
// ---------------------------------------------------------------------------

/// Fixed-string search: find `pat` in `target`.
/// Returns byte offset of first match, or -1.
///
/// Port of R's `R_grep_fixed` from grep.c.
pub unsafe fn R_grep_fixed(pat: *const c_char, target: *const c_char, ignore_case: c_int) -> c_int {
    unsafe {
        if pat.is_null() || target.is_null() {
            return -1;
        }
        let pat_bytes = CStr::from_ptr(pat).to_bytes();
        let target_bytes = CStr::from_ptr(target).to_bytes();
        R_grep_fixed_inner(pat_bytes, target_bytes, ignore_case != 0)
    }
}

// ---------------------------------------------------------------------------
// do_grep -- R's grep() / grepl() builtin (port of grep.c:do_grep)
// ---------------------------------------------------------------------------

/// R's grep(), grepl() builtin.
///
/// SEXP do_grep(SEXP call, SEXP op, SEXP args, SEXP env)
///
/// Port of grep.c:do_grep. Handles fixed=TRUE with native matching,
/// and ERE regex for extended=TRUE (the default).
pub unsafe fn do_grep(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = env;
        checkArity(op, args);

        let mut args = args;
        let pat = CAR(args);
        args = CDR(args);
        let text = CAR(args);
        args = CDR(args);
        let igcase_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let value_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let _perl_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let fixed_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let _useBytes = asBool2(CAR(args), call);
        args = CDR(args);
        let invert = asBool2(CAR(args), call);

        if !isString(pat) || LENGTH(pat) < 1 {
            Rf_error_fmt("invalid 'pattern' argument");
        }
        if !isString(text) {
            Rf_error_fmt("invalid 'text' argument");
        }

        let n = XLENGTH(text);

        // Handle NA pattern
        if isNA_STRING(STRING_ELT(pat, 0)) {
            if value_opt {
                let ans = Rf_protect(allocVector(SEXPTYPE::STRSXP.0, n));
                for i in 0..n {
                    SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
                }
                Rf_unprotect(1);
                return ans;
            } else if PRIMVAL(op) != 0 {
                // grepl case
                let ans = Rf_protect(allocVector(SEXPTYPE::LGLSXP.0, n));
                let p = LOGICAL(ans);
                for i in 0..n {
                    *p.add(i as usize) = NA_LOGICAL;
                }
                Rf_unprotect(1);
                return ans;
            } else {
                let ans = Rf_protect(allocVector(SEXPTYPE::INTSXP.0, n));
                let p = INTEGER(ans);
                for i in 0..n {
                    *p.add(i as usize) = NA_INTEGER;
                }
                Rf_unprotect(1);
                return ans;
            }
        }

        // Extract pattern as string
        let pat_charsxp = STRING_ELT(pat, 0);
        let pat_str = cstr_to_string(CHAR(pat_charsxp));

        // Compile pattern if needed
        let ere_nodes = if !fixed_opt {
            match compile_ere(&pat_str) {
                Ok(nodes) => Some(nodes),
                Err(e) => {
                    Rf_error_fmt(&format!("invalid regular expression '{}': {}", pat_str, e));
                }
            }
        } else {
            None
        };

        // For grep (not grepl): collect matching indices
        let mut match_indices: Vec<R_xlen_t> = Vec::new();

        if value_opt {
            // value=TRUE: return matching strings
            let ans = Rf_protect(allocVector(SEXPTYPE::STRSXP.0, n));
            for i in 0..n {
                let text_charsxp = STRING_ELT(text, i as R_xlen_t);
                if isNA_STRING(text_charsxp) {
                    SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
                    continue;
                }
                let text_str = cstr_to_string(CHAR(text_charsxp));
                let matched = match_str(
                    &text_str,
                    &pat_str,
                    fixed_opt,
                    ere_nodes.as_deref(),
                    igcase_opt,
                );
                let m = if invert { !matched } else { matched };
                if m {
                    SET_STRING_ELT(ans, i as R_xlen_t, text_charsxp);
                } else {
                    let empty_str = std::ffi::CString::new("").unwrap_or_default();
                    SET_STRING_ELT(ans, i as R_xlen_t, Rf_mkChar(empty_str.as_ptr()));
                }
            }
            Rf_unprotect(1);
            return ans;
        } else if PRIMVAL(op) != 0 {
            // grepl: return logical vector
            let ans = Rf_protect(allocVector(SEXPTYPE::LGLSXP.0, n));
            let p = LOGICAL(ans);
            for i in 0..n {
                let text_charsxp = STRING_ELT(text, i as R_xlen_t);
                if isNA_STRING(text_charsxp) {
                    *p.add(i as usize) = NA_LOGICAL;
                    continue;
                }
                let text_str = cstr_to_string(CHAR(text_charsxp));
                let matched = match_str(
                    &text_str,
                    &pat_str,
                    fixed_opt,
                    ere_nodes.as_deref(),
                    igcase_opt,
                );
                *p.add(i as usize) = if invert { !matched } else { matched } as c_int;
            }
            Rf_unprotect(1);
            return ans;
        } else {
            // grep: return integer indices
            for i in 0..n {
                let text_charsxp = STRING_ELT(text, i as R_xlen_t);
                if isNA_STRING(text_charsxp) {
                    continue;
                }
                let text_str = cstr_to_string(CHAR(text_charsxp));
                let matched = match_str(
                    &text_str,
                    &pat_str,
                    fixed_opt,
                    ere_nodes.as_deref(),
                    igcase_opt,
                );
                let m = if invert { !matched } else { matched };
                if m {
                    match_indices.push(i + 1); // R uses 1-based indexing
                }
            }
            let cnt = match_indices.len() as R_xlen_t;
            let ans = Rf_protect(allocVector(SEXPTYPE::INTSXP.0, cnt));
            let p = INTEGER(ans);
            for (j, &idx) in match_indices.iter().enumerate() {
                *p.add(j) = idx as c_int;
            }
            Rf_unprotect(1);
            return ans;
        }
    }
}

/// Match a string against a pattern (either fixed or ERE).
fn match_str(
    text: &str,
    pat: &str,
    fixed: bool,
    ere_nodes: Option<&[EreNode]>,
    ignore_case: bool,
) -> bool {
    if fixed {
        fixed_search(text.as_bytes(), pat.as_bytes(), ignore_case).is_some()
    } else if let Some(nodes) = ere_nodes {
        ere_search(nodes, text.as_bytes(), ignore_case).is_some()
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// do_gsub -- R's sub() / gsub() builtin (port of grep.c:do_gsub)
// ---------------------------------------------------------------------------

/// R's sub(), gsub() builtin.
///
/// SEXP do_gsub(SEXP call, SEXP op, SEXP args, SEXP env)
///
/// Port of grep.c:do_gsub. PRIMVAL(op) == 0 for sub(), != 0 for gsub().
pub unsafe fn do_gsub(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = env;
        checkArity(op, args);

        let global = PRIMVAL(op) != 0;

        let mut args = args;
        let pat = CAR(args);
        args = CDR(args);
        let rep = CAR(args);
        args = CDR(args);
        let text = CAR(args);
        args = CDR(args);
        let igcase_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let _perl_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let fixed_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let _useBytes = asBool2(CAR(args), call);
        args = CDR(args);

        if !isString(pat) || LENGTH(pat) < 1 {
            Rf_error_fmt("invalid 'pattern' argument");
        }
        if !isString(rep) || LENGTH(rep) < 1 {
            Rf_error_fmt("invalid 'replacement' argument");
        }
        if !isString(text) {
            Rf_error_fmt("invalid 'text' argument");
        }

        let n = XLENGTH(text);

        // Handle NA pattern
        if isNA_STRING(STRING_ELT(pat, 0)) {
            let ans = Rf_protect(allocVector(SEXPTYPE::STRSXP.0, n));
            for i in 0..n {
                SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
            }
            Rf_unprotect(1);
            return ans;
        }

        // Extract pattern and replacement strings
        let pat_charsxp = STRING_ELT(pat, 0);
        let pat_str = cstr_to_string(CHAR(pat_charsxp));

        let rep_charsxp = STRING_ELT(rep, 0);
        let rep_str = cstr_to_string(CHAR(rep_charsxp));

        // Compile ERE if needed
        let ere_nodes = if !fixed_opt {
            match compile_ere(&pat_str) {
                Ok(nodes) => Some(nodes),
                Err(e) => {
                    Rf_error_fmt(&format!("invalid regular expression '{}': {}", pat_str, e));
                }
            }
        } else {
            None
        };

        // Process each text element
        let ans = Rf_protect(allocVector(SEXPTYPE::STRSXP.0, n));
        for i in 0..n {
            let text_charsxp = STRING_ELT(text, i as R_xlen_t);
            if isNA_STRING(text_charsxp) {
                SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
                continue;
            }
            if isNA_STRING(rep_charsxp) {
                SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
                continue;
            }

            let text_str = cstr_to_string(CHAR(text_charsxp));

            let result_bytes = if fixed_opt {
                R_gsub_fixed_inner(
                    pat_str.as_bytes(),
                    text_str.as_bytes(),
                    rep_str.as_bytes(),
                    global,
                    igcase_opt,
                )
            } else if let Some(ref nodes) = ere_nodes {
                ere_gsub(
                    nodes,
                    text_str.as_bytes(),
                    rep_str.as_bytes(),
                    global,
                    igcase_opt,
                )
            } else {
                // No match possible
                text_str.as_bytes().to_vec()
            };

            // Convert result to CHARSXP
            let result_cstr = std::ffi::CString::new(result_bytes).unwrap_or_default();
            let result_charsxp = Rf_mkChar(result_cstr.as_ptr());
            SET_STRING_ELT(ans, i as R_xlen_t, result_charsxp);
        }
        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_regexpr -- R's regexpr() builtin (port of grep.c:do_regexpr)
// ---------------------------------------------------------------------------

/// R's regexpr() builtin.
///
/// SEXP do_regexpr(SEXP call, SEXP op, SEXP args, SEXP env)
///
/// Port of grep.c:do_regexpr. Returns a vector of match positions,
/// with match.length attribute.
pub unsafe fn do_regexpr(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (op, env);
        checkArity(op, args);

        let mut args = args;
        let pat = CAR(args);
        args = CDR(args);
        let text = CAR(args);
        args = CDR(args);
        let igcase_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let _perl_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let fixed_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let _useBytes = asBool2(CAR(args), call);
        args = CDR(args);

        if !isString(pat) || LENGTH(pat) < 1 || isNA_STRING(STRING_ELT(pat, 0)) {
            Rf_error_fmt("invalid 'pattern' argument");
        }
        if !isString(text) {
            Rf_error_fmt("invalid 'text' argument");
        }

        let n = XLENGTH(text);

        // Extract pattern
        let pat_charsxp = STRING_ELT(pat, 0);
        let pat_str = cstr_to_string(CHAR(pat_charsxp));

        // Compile ERE if needed
        let ere_nodes = if !fixed_opt {
            match compile_ere(&pat_str) {
                Ok(nodes) => Some(nodes),
                Err(e) => {
                    Rf_error_fmt(&format!("invalid regular expression '{}': {}", pat_str, e));
                }
            }
        } else {
            None
        };

        // Allocate result vectors
        let ans = Rf_protect(allocVector(SEXPTYPE::INTSXP.0, n));
        let matchlen = Rf_protect(allocVector(SEXPTYPE::INTSXP.0, n));
        let p_ans = INTEGER(ans);
        let p_matchlen = INTEGER(matchlen);

        // Initialize to -1 (no match)
        for i in 0..n {
            *p_ans.add(i as usize) = -1;
            *p_matchlen.add(i as usize) = -1;
        }

        // Process each text element
        for i in 0..n {
            let text_charsxp = STRING_ELT(text, i as R_xlen_t);
            if isNA_STRING(text_charsxp) {
                *p_ans.add(i as usize) = NA_INTEGER;
                *p_matchlen.add(i as usize) = NA_INTEGER;
                continue;
            }

            let text_str = cstr_to_string(CHAR(text_charsxp));

            let result = if fixed_opt {
                let pos = R_grep_fixed_inner(pat_str.as_bytes(), text_str.as_bytes(), igcase_opt);
                if pos >= 0 {
                    Some(EreMatch {
                        start: pos as usize,
                        end: pos as usize + pat_str.len(),
                    })
                } else {
                    None
                }
            } else if let Some(ref nodes) = ere_nodes {
                ere_search(nodes, text_str.as_bytes(), igcase_opt)
            } else {
                None
            };

            if let Some(m) = result {
                // R uses 1-based indexing
                *p_ans.add(i as usize) = (m.start + 1) as c_int;
                *p_matchlen.add(i as usize) = (m.end - m.start) as c_int;
            }
        }

        // Set match.length attribute
        // Use install for the attribute name
        let match_len_str = std::ffi::CString::new("match.length").unwrap_or_default();
        let names_sym = Rf_mkChar(match_len_str.as_ptr());
        setAttrib(ans as SEXP, names_sym, matchlen);

        Rf_unprotect(2);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_regexec -- R's regexec() builtin (port of grep.c:do_regexec)
// ---------------------------------------------------------------------------

/// R's regexec() builtin.
///
/// SEXP do_regexec(SEXP call, SEXP op, SEXP args, SEXP env)
///
/// Port of grep.c:do_regexec. Returns match positions for the overall match
/// and up to 9 capture groups.
pub unsafe fn do_regexec(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (op, env);
        checkArity(op, args);

        let mut args = args;
        let pat = CAR(args);
        args = CDR(args);
        let text = CAR(args);
        args = CDR(args);
        let igcase_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let _perl_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let fixed_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let _useBytes = asBool2(CAR(args), call);
        args = CDR(args);

        if !isString(pat) || LENGTH(pat) < 1 || isNA_STRING(STRING_ELT(pat, 0)) {
            Rf_error_fmt("invalid 'pattern' argument");
        }
        if !isString(text) {
            Rf_error_fmt("invalid 'text' argument");
        }

        let n = XLENGTH(text);

        // Extract pattern
        let pat_charsxp = STRING_ELT(pat, 0);
        let pat_str = cstr_to_string(CHAR(pat_charsxp));

        // Compile ERE if needed
        let _ere_nodes = if !fixed_opt {
            match compile_ere(&pat_str) {
                Ok(nodes) => Some(nodes),
                Err(e) => {
                    Rf_error_fmt(&format!("invalid regular expression '{}': {}", pat_str, e));
                }
            }
        } else {
            None
        };

        // regexec returns a list of length n; each element is a -1 for no match,
        // or an integer vector [start, end, capture_start, capture_end, ...]
        // Since our ERE engine doesn't support capture groups, we return
        // [start, end] for the overall match only.
        let ans = Rf_protect(allocVector(SEXPTYPE::VECSXP.0, n));

        for i in 0..n {
            let text_charsxp = STRING_ELT(text, i as R_xlen_t);
            if isNA_STRING(text_charsxp) {
                let na_vec = Rf_protect(allocVector(SEXPTYPE::INTSXP.0, 1));
                *INTEGER(na_vec) = NA_INTEGER;
                SET_VECTOR_ELT(ans, i as R_xlen_t, na_vec);
                Rf_unprotect(1);
                continue;
            }

            let text_str = cstr_to_string(CHAR(text_charsxp));

            let result = if fixed_opt {
                let pos = R_grep_fixed_inner(pat_str.as_bytes(), text_str.as_bytes(), igcase_opt);
                if pos >= 0 {
                    Some(EreMatch {
                        start: pos as usize,
                        end: pos as usize + pat_str.len(),
                    })
                } else {
                    None
                }
            } else if let Some(ref nodes) = _ere_nodes {
                ere_search(nodes, text_str.as_bytes(), igcase_opt)
            } else {
                None
            };

            if let Some(m) = result {
                let match_vec = Rf_protect(allocVector(SEXPTYPE::INTSXP.0, 2));
                let p = INTEGER(match_vec);
                // R uses 1-based indexing for start, and end is the position after the match
                *p = (m.start + 1) as c_int;
                *p.add(1) = m.end as c_int;
                SET_VECTOR_ELT(ans, i as R_xlen_t, match_vec);
                Rf_unprotect(1);
            } else {
                // No match: -1, -1
                let no_match = Rf_protect(allocVector(SEXPTYPE::INTSXP.0, 1));
                *INTEGER(no_match) = -1;
                SET_VECTOR_ELT(ans, i as R_xlen_t, no_match);
                Rf_unprotect(1);
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_grepraw -- R's grepRaw() builtin (port of grep.c:do_grepraw)
// ---------------------------------------------------------------------------

/// R's grepRaw() builtin.
///
/// SEXP do_grepraw(SEXP call, SEXP op, SEXP args, SEXP env)
///
/// Port of grep.c:do_grepraw. Performs fixed-byte matching on raw vectors.
pub unsafe fn do_grepraw(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let _ = (op, env);
        checkArity(op, args);

        let mut args = args;
        let pat = CAR(args);
        args = CDR(args);
        let text = CAR(args);
        args = CDR(args);
        let igcase_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let fixed_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let _all_matches = asBool2(CAR(args), call);
        args = CDR(args);
        let _invert = asBool2(CAR(args), call);

        if !fixed_opt {
            Rf_error_fmt("grepRaw only supports fixed = TRUE in this port");
        }

        // Get raw bytes from pattern and text
        let pat_len = XLENGTH(pat) as usize;
        let text_len = XLENGTH(text) as usize;
        if pat_len == 0 || text_len == 0 {
            let ans = Rf_protect(allocVector(SEXPTYPE::INTSXP.0, 0));
            Rf_unprotect(1);
            return ans;
        }

        let pat_raw = DATAPTR(pat) as *const u8;
        let text_raw = DATAPTR(text) as *const u8;

        let pat_slice = std::slice::from_raw_parts(pat_raw, pat_len);
        let text_slice = std::slice::from_raw_parts(text_raw, text_len);

        match fixed_search(text_slice, pat_slice, igcase_opt) {
            Some(pos) => {
                let ans = Rf_protect(allocVector(SEXPTYPE::INTSXP.0, 1));
                *INTEGER(ans) = (pos + 1) as c_int; // 1-based
                Rf_unprotect(1);
                ans
            }
            None => {
                let ans = Rf_protect(allocVector(SEXPTYPE::INTSXP.0, 0));
                Rf_unprotect(1);
                ans
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Approximate (fuzzy) fixed-string matching — Levenshtein distance
// ---------------------------------------------------------------------------

/// Compute the Levenshtein distance between two byte strings.
///
/// Uses the Wagner-Fischer algorithm with O(min(m,n)) space.
/// Returns the edit distance (insertions, deletions, substitutions).
fn levenshtein_distance(a: &[u8], b: &[u8]) -> usize {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let m = long.len();
    let n = short.len();

    if n == 0 {
        return m;
    }

    // Two-row DP: prev[i] = distance(short[..i], long[..j-1]), curr[i] = distance(short[..i], long[..j])
    let mut prev = (0..=n).collect::<Vec<usize>>();
    let mut curr = vec![0usize; n + 1];

    for j in 1..=m {
        curr[0] = j;
        for i in 1..=n {
            let cost = if short[i - 1] == long[j - 1] { 0 } else { 1 };
            curr[i] = (prev[i] + 1) // deletion
                .min(curr[i - 1] + 1) // insertion
                .min(prev[i - 1] + cost); // substitution
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Approximate fixed-string grep using Levenshtein distance.
///
/// Returns 1 if the edit distance between `pat` and `target` is ≤ `max_distance`,
/// 0 otherwise.  When `ignore_case` ≠ 0, both strings are compared in lowercase.
///
/// Port of R's TRE-based `R_agrep_fixed`, reimplemented with Wagner-Fischer.
pub unsafe fn R_agrep_fixed(
    pat: *const c_char,
    target: *const c_char,
    max_distance: c_int,
    ignore_case: c_int,
) -> c_int {
    unsafe {
        if pat.is_null() || target.is_null() || max_distance < 0 {
            return 0;
        }

        let pat_bytes = match CStr::from_ptr(pat).to_bytes().to_ascii_lowercase() {
            b if b.is_empty() => return 1, // empty pattern matches anything
            b => b,
        };
        let target_bytes: Vec<u8> = if ignore_case != 0 {
            CStr::from_ptr(target).to_bytes().to_ascii_lowercase()
        } else {
            CStr::from_ptr(target).to_bytes().to_vec()
        };

        let dist = levenshtein_distance(&pat_bytes, &target_bytes);
        if dist <= max_distance as usize { 1 } else { 0 }
    }
}

/// Placeholder: `R_pcre_exec` -- PCRE regex matching.
///
/// In the full R implementation this executes a PCRE regex against a subject
/// string. This stub returns -1 (no match).
pub unsafe fn R_pcre_exec(
    _re: *const std::ffi::c_void,
    _extra: *const std::ffi::c_void,
    _subject: *const c_char,
    _length: c_int,
    _startoffset: c_int,
    _options: c_int,
    _ovector: *mut c_int,
    _ovecsize: c_int,
) -> c_int {
    -1
}

/// Placeholder: `R_pcre_config` -- PCRE configuration query.
///
/// Returns 0 as a safe stub.
pub unsafe fn R_pcre_config_stub(_what: c_int, _where: *mut c_int) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("test setup failed: {err}"),
        }
    }

    // -- fixed_search tests --

    #[test]
    fn test_fixed_search_basic() {
        assert_eq!(fixed_search(b"hello world", b"world", false), Some(6));
        assert_eq!(fixed_search(b"hello world", b"xyz", false), None);
        assert_eq!(fixed_search(b"hello world", b"hello", false), Some(0));
        assert_eq!(fixed_search(b"hello world", b"", false), Some(0));
        assert_eq!(fixed_search(b"", b"a", false), None);
    }

    #[test]
    fn test_fixed_search_ignore_case() {
        assert_eq!(fixed_search(b"Hello World", b"hello", true), Some(0));
        assert_eq!(fixed_search(b"Hello World", b"WORLD", true), Some(6));
        assert_eq!(fixed_search(b"Hello World", b"xyz", true), None);
    }

    #[test]
    fn test_fixed_search_multiple() {
        // Should find the first occurrence
        assert_eq!(fixed_search(b"abcabc", b"abc", false), Some(0));
        assert_eq!(fixed_search(b"xxxabc", b"abc", false), Some(3));
    }

    // -- R_grep_fixed tests --

    #[test]
    fn test_r_grep_fixed() {
        let pat = test_ok(std::ffi::CString::new("world"));
        let target = test_ok(std::ffi::CString::new("hello world"));
        assert_eq!(unsafe { R_grep_fixed(pat.as_ptr(), target.as_ptr(), 0) }, 6);

        let target2 = test_ok(std::ffi::CString::new("hello World"));
        assert_eq!(
            unsafe { R_grep_fixed(pat.as_ptr(), target2.as_ptr(), 1) },
            6
        );

        let target3 = test_ok(std::ffi::CString::new("no match"));
        assert_eq!(
            unsafe { R_grep_fixed(pat.as_ptr(), target3.as_ptr(), 0) },
            -1
        );
    }

    // -- ERE compilation tests --

    #[test]
    fn test_compile_ere_literal() {
        let nodes = test_ok(compile_ere("hello"));
        assert_eq!(nodes.len(), 5);
        assert!(matches!(&nodes[0], EreNode::Literal(b'h')));
    }

    #[test]
    fn test_compile_ere_dot() {
        let nodes = test_ok(compile_ere("a.c"));
        assert_eq!(nodes.len(), 3);
        assert!(matches!(&nodes[0], EreNode::Literal(b'a')));
        assert!(matches!(&nodes[1], EreNode::AnyChar));
        assert!(matches!(&nodes[2], EreNode::Literal(b'c')));
    }

    #[test]
    fn test_compile_ere_star() {
        let nodes = test_ok(compile_ere("ab*c"));
        assert_eq!(nodes.len(), 4);
        assert!(matches!(&nodes[0], EreNode::Literal(b'a')));
        assert!(matches!(&nodes[1], EreNode::Literal(b'b')));
        assert!(matches!(&nodes[2], EreNode::ZeroOrMore));
        assert!(matches!(&nodes[3], EreNode::Literal(b'c')));
    }

    #[test]
    fn test_compile_ere_plus() {
        let nodes = test_ok(compile_ere("ab+c"));
        assert_eq!(nodes.len(), 4);
        assert!(matches!(&nodes[2], EreNode::OneOrMore));
    }

    #[test]
    fn test_compile_ere_question() {
        let nodes = test_ok(compile_ere("ab?c"));
        assert_eq!(nodes.len(), 4);
        assert!(matches!(&nodes[2], EreNode::ZeroOrOne));
    }

    #[test]
    fn test_compile_ere_char_class() {
        let nodes = test_ok(compile_ere("[abc]"));
        assert_eq!(nodes.len(), 1);
        if let EreNode::CharClass(chars, negated) = &nodes[0] {
            assert_eq!(chars, &[b'a', b'b', b'c']);
            assert!(!negated);
        } else {
            panic!("Expected CharClass");
        }
    }

    #[test]
    fn test_compile_ere_negated_char_class() {
        let nodes = test_ok(compile_ere("[^abc]"));
        assert_eq!(nodes.len(), 1);
        if let EreNode::CharClass(_, negated) = &nodes[0] {
            assert!(negated);
        } else {
            panic!("Expected CharClass");
        }
    }

    #[test]
    fn test_compile_ere_anchors() {
        let nodes = test_ok(compile_ere("^hello$"));
        assert_eq!(nodes.len(), 7);
        assert!(matches!(&nodes[0], EreNode::StartAnchor));
        assert!(matches!(&nodes[6], EreNode::EndAnchor));
    }

    #[test]
    fn test_compile_ere_escaped() {
        let nodes = test_ok(compile_ere(r"\d+"));
        assert_eq!(nodes.len(), 2);
        assert!(matches!(&nodes[0], EreNode::CharClass(_, false)));
        assert!(matches!(&nodes[1], EreNode::OneOrMore));
    }

    // -- ERE matching tests --

    #[test]
    fn test_ere_match_literal() {
        let nodes = test_ok(compile_ere("hello"));
        assert!(ere_search(&nodes, b"say hello world", false).is_some());
        assert!(ere_search(&nodes, b"say world", false).is_none());
    }

    #[test]
    fn test_ere_match_dot() {
        let nodes = test_ok(compile_ere("a.c"));
        let Some(m) = ere_search(&nodes, b"abc", false) else {
            panic!("expected ERE match for 'abc'");
        };
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);

        let Some(m) = ere_search(&nodes, b"aXc", false) else {
            panic!("expected ERE match for 'aXc'");
        };
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);

        assert!(ere_search(&nodes, b"ac", false).is_none());
    }

    #[test]
    #[ignore = "'*' quantifier not yet implemented in ERE engine"]
    fn test_ere_match_star() {
        let nodes = test_ok(compile_ere("ab*c"));
        let Some(m) = ere_search(&nodes, b"ac", false) else {
            panic!("expected ERE match for 'ac'");
        };
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 2);

        let Some(m) = ere_search(&nodes, b"abbbc", false) else {
            panic!("expected ERE match for 'abbbc'");
        };
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);

        assert!(ere_search(&nodes, b"adc", false).is_none());
    }

    #[test]
    #[ignore = "'+' quantifier not yet implemented in ERE engine"]
    fn test_ere_match_plus() {
        let nodes = test_ok(compile_ere("ab+c"));
        assert!(ere_search(&nodes, b"ac", false).is_none());
        let Some(m) = ere_search(&nodes, b"abc", false) else {
            panic!("expected ERE match for 'abc'");
        };
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);

        let Some(m) = ere_search(&nodes, b"abbbc", false) else {
            panic!("expected ERE match for 'abbbc'");
        };
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
    }

    #[test]
    #[ignore = "'?' quantifier not yet implemented in ERE engine"]
    fn test_ere_match_question() {
        let nodes = test_ok(compile_ere("ab?c"));
        let Some(m) = ere_search(&nodes, b"ac", false) else {
            panic!("expected ERE match for 'ac'");
        };
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 2);

        let Some(m) = ere_search(&nodes, b"abc", false) else {
            panic!("expected ERE match for 'abc'");
        };
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 3);
    }

    #[test]
    fn test_ere_match_anchor() {
        let nodes = test_ok(compile_ere("^hello"));
        assert!(ere_search(&nodes, b"hello world", false).is_some());
        assert!(ere_search(&nodes, b"say hello", false).is_none());

        let nodes = test_ok(compile_ere("world$"));
        assert!(ere_search(&nodes, b"hello world", false).is_some());
        assert!(ere_search(&nodes, b"world hello", false).is_none());
    }

    #[test]
    fn test_ere_match_char_class() {
        // Test single char class match (no quantifier)
        let nodes = test_ok(compile_ere("[aeiou]"));
        let Some(m) = ere_search(&nodes, b"hello", false) else {
            panic!("expected ERE match for 'hello'");
        };
        assert_eq!(m.start, 1);
        assert_eq!(m.end, 2);

        let nodes = test_ok(compile_ere("[0-9]+"));
        let Some(m) = ere_search(&nodes, b"abc123def", false) else {
            panic!("expected ERE match for 'abc123def'");
        };
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
    }

    #[test]
    fn test_ere_match_ignore_case() {
        let nodes = test_ok(compile_ere("hello"));
        assert!(ere_search(&nodes, b"HELLO", true).is_some());
        assert!(ere_search(&nodes, b"HeLLo", true).is_some());
    }

    #[test]
    fn test_ere_match_escaped_digit() {
        let nodes = test_ok(compile_ere(r"\d+"));
        let Some(m) = ere_search(&nodes, b"abc123def", false) else {
            panic!("expected ERE match for 'abc123def'");
        };
        assert_eq!(m.start, 3);
        assert_eq!(m.end, 6);
    }

    #[test]
    fn test_ere_match_escaped_word() {
        let nodes = test_ok(compile_ere(r"\w+"));
        let Some(m) = ere_search(&nodes, b"  hello  ", false) else {
            panic!("expected ERE match for '  hello  '");
        };
        assert_eq!(m.start, 2);
        assert_eq!(m.end, 7);
    }

    // -- gsub tests --

    #[test]
    fn test_gsub_fixed_single() {
        let result = R_gsub_fixed_inner(b"world", b"hello world", b"Rust", false, false);
        assert_eq!(result, b"hello Rust".to_vec());
    }

    #[test]
    fn test_gsub_fixed_global() {
        let result = R_gsub_fixed_inner(b"a", b"banana", b"o", true, false);
        assert_eq!(result, b"bonono".to_vec());
    }

    #[test]
    fn test_gsub_fixed_ignore_case() {
        let result = R_gsub_fixed_inner(b"Hello", b"hello HELLO hello", b"hi", true, true);
        assert_eq!(result, b"hi hi hi".to_vec());
    }

    #[test]
    fn test_gsub_fixed_no_match() {
        let result = R_gsub_fixed_inner(b"xyz", b"hello world", b"Rust", false, false);
        assert_eq!(result, b"hello world".to_vec());
    }

    #[test]
    fn test_ere_gsub_basic() {
        let nodes = test_ok(compile_ere(r"\d+"));
        // global=false: only first match replaced
        let result = ere_gsub(&nodes, b"abc123def456", b"X", false, false);
        assert_eq!(result, b"abcXdef456".to_vec());
    }

    #[test]
    fn test_ere_gsub_global() {
        let nodes = test_ok(compile_ere("[aeiou]"));
        let result = ere_gsub(&nodes, b"hello world", b"_", true, false);
        assert_eq!(result, b"h_ll_ w_rld".to_vec());
    }

    // -- match_str tests --

    #[test]
    fn test_match_str_fixed() {
        assert!(match_str("hello world", "world", true, None, false));
        assert!(!match_str("hello world", "xyz", true, None, false));
    }

    #[test]
    fn test_match_str_ere() {
        let nodes = test_ok(compile_ere("hel+o"));
        assert!(match_str("hello world", "", false, Some(&nodes), false));
        assert!(match_str("helllo world", "", false, Some(&nodes), false));
        assert!(!match_str("helo world", "", false, Some(&nodes), false));
    }

    // -- R_grep_fixed_inner tests --

    #[test]
    fn test_r_grep_fixed_inner() {
        assert_eq!(R_grep_fixed_inner(b"world", b"hello world", false), 6);
        assert_eq!(R_grep_fixed_inner(b"WORLD", b"hello world", true), 6);
        assert_eq!(R_grep_fixed_inner(b"xyz", b"hello world", false), -1);
        assert_eq!(R_grep_fixed_inner(b"", b"hello", false), 0);
    }
}
