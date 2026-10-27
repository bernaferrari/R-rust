#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/grep.c
//!
//! Implements R's grep(), grepl(), sub(), gsub(), regexpr(), regexec(),
//! and related pattern matching functions.
//!
//! Since this port does not link against PCRE2, the implementation uses:
//! - Rust's standard library for fixed-string matching (fixed = TRUE)
//! - The ported TRE engine for non-perl, non-fixed ERE mode
//! - The pure-Rust fancy-regex engine for the supported Perl syntax subset
//!   (full PCRE2 compatibility remains tracked separately)
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

use fancy_regex::{Regex as PerlInnerRegex, RegexBuilder as PerlRegexBuilder};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::*;
use crate::sexp::protect::protect;
use crate::tre::ast::{REG_EXTENDED, REG_ICASE, REG_OK, regmatch_t};
use crate::tre::regapi::tre_regnexec;

#[cfg(test)]
mod capture_publication_tests;
#[cfg(test)]
mod compile_memory_tests;
#[cfg(test)]
mod group_names_memory_tests;
#[cfg(test)]
mod perl_class_tests;
mod perl_dialect;
mod positions;
pub(crate) use positions::{EmptyPatternMode, empty_pattern_positions};
#[cfg(test)]
mod replacement_memory_tests;
#[cfg(test)]
mod replacement_tests;
pub(crate) mod workspace;
pub(crate) use workspace::RegexWorkspace;
use workspace::{TextBuilder, WorkspaceBuffer, WorkspaceText};

pub(crate) type RegexIndices = WorkspaceBuffer<R_xlen_t>;
pub(crate) type RegexCaptures = WorkspaceBuffer<Option<RegexMatch>>;
pub(crate) type RegexMatches = WorkspaceBuffer<RegexCaptures>;

pub(crate) fn single_capture(value: RegexMatch) -> RegexCaptures {
    let mut captures = regex_workspace().buffer(1);
    captures.push(Some(value));
    captures
}

pub(crate) fn regex_workspace() -> RegexWorkspace {
    // Pure kernel unit tests may run without an interpreter. Production regex
    // entrypoints require an original active owner, rather than an unlimited fallback.
    #[cfg(test)]
    if crate::sexp::instance::current_instance_ptr().is_none() {
        return RegexWorkspace::standalone();
    }
    unsafe {
        let owner =
            crate::sexp::owner::OwnerToken::current().unwrap_or_else(|_| workspace::memory_error());
        let _pin = owner.pin().unwrap_or_else(|_| workspace::memory_error());
        // This immutable sibling capability remains valid during a registered
        // mutable arena lend and observes the same live original budget cells.
        let ledger = (*owner.as_ptr()).heap_identity.byte_ledger();
        let availability = crate::sexp::instance::instance_liveness(owner.as_ptr());
        RegexWorkspace::new(ledger, availability)
    }
}

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

#[inline(always)]
unsafe fn allocVector<T: Into<crate::sexp::ffi::SEXPTYPE>>(
    sexptype: T,
    length: crate::sexp::ffi::R_xlen_t,
) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_allocVector3(sexptype, length) }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegexMatch {
    pub start: usize,
    pub end: usize,
}

struct TreRegex {
    preg: crate::tre::compile::CompiledTre,
    bytes: bool,
    workspace: RegexWorkspace,
}

struct PerlRegex {
    regex: PerlInnerRegex,
    workspace: RegexWorkspace,
}

impl TreRegex {
    fn compile(pattern: &str, ignore_case: bool) -> Result<Self, String> {
        Self::compile_mode(pattern, ignore_case, false)
    }

    fn compile_mode(pattern: &str, ignore_case: bool, bytes: bool) -> Result<Self, String> {
        let workspace = regex_workspace();
        unsafe {
            let mut cflags = REG_EXTENDED;
            if ignore_case {
                cflags |= REG_ICASE;
            }
            let length = if bytes {
                pattern.len()
            } else {
                pattern.chars().count()
            };
            let mut wide = workspace.buffer(length);
            if bytes {
                for byte in pattern.bytes() {
                    wide.push(u32::from(byte));
                }
            } else {
                for ch in pattern.chars() {
                    wide.push(u32::from(ch));
                }
            }
            let preg = crate::tre::compile::tre_compile_owned(
                wide.as_ptr(),
                wide.len(),
                cflags,
                workspace.clone(),
            )
            .map_err(|rc| {
                // Option-based matching adapters may discard syntax failures, but
                // exhaustion must follow the same typed error path as buffers.
                if rc == crate::tre::ast::REG_ESPACE {
                    workspace::memory_error();
                }
                format!("TRE compilation failed with code {rc}")
            })?;
            Ok(Self {
                preg,
                bytes,
                workspace,
            })
        }
    }

    fn captures(&self, text: &str) -> Option<RegexCaptures> {
        self.captures_at(text, 0)
    }

    fn captures_at(&self, original: &str, offset: usize) -> Option<RegexCaptures> {
        let text = if self.bytes { "" } else { &original[offset..] };
        let input = &original.as_bytes()[offset..];
        unsafe {
            let nmatch = self.preg.regex().re_nsub.saturating_add(1).max(1);
            let mut pmatch = self.workspace.buffer(nmatch);
            pmatch.resize(nmatch, regmatch_t::default());
            let wide = if !self.bytes && !text.is_ascii() {
                let mut wide = self.workspace.buffer(text.chars().count());
                for ch in text.chars() {
                    wide.push(u32::from(ch));
                }
                Some(wide)
            } else {
                None
            };
            let flags = if offset == 0 {
                0
            } else {
                crate::tre::ast::REG_NOTBOL
            };
            let status = if let Some(wide) = &wide {
                crate::tre::regapi::tre_regwnexec(self.preg.regex(), wide, &mut pmatch, flags)
            } else {
                tre_regnexec(
                    self.preg.regex(),
                    input.as_ptr().cast(),
                    input.len(),
                    nmatch,
                    pmatch.as_mut_ptr(),
                    flags,
                )
            };
            let byte_offset = |index: usize| {
                if wide.is_some() {
                    text.char_indices()
                        .nth(index)
                        .map_or(text.len(), |(i, _)| i)
                } else {
                    index
                }
            };
            if status != REG_OK {
                return None;
            }
            let mut captures = self.workspace.buffer(nmatch);
            for m in pmatch.iter() {
                captures.push(if m.rm_so >= 0 && m.rm_eo >= m.rm_so {
                    Some(RegexMatch {
                        start: offset + byte_offset(m.rm_so as usize),
                        end: offset + byte_offset(m.rm_eo as usize),
                    })
                } else {
                    None
                });
            }
            Some(captures)
        }
    }

    fn find(&self, text: &str) -> Option<RegexMatch> {
        self.captures(text)
            .and_then(|mut captures| captures.get_mut(0).and_then(|whole| whole.take()))
    }
}

impl PerlRegex {
    fn compile(pattern: &str, ignore_case: bool) -> Result<Self, String> {
        Self::compile_mode(pattern, ignore_case, false)
    }

    fn compile_mode(pattern: &str, ignore_case: bool, bytes: bool) -> Result<Self, String> {
        let workspace = regex_workspace();
        let prepared = if bytes {
            None
        } else {
            Some(perl_dialect::prepare(pattern, &workspace, ignore_case).map_err(str::to_owned)?)
        };
        let pattern = prepared.as_deref().unwrap_or(pattern);
        PerlRegexBuilder::new(pattern)
            .case_insensitive(ignore_case)
            .bytes_mode(if bytes {
                fancy_regex::BytesMode::Ascii
            } else {
                fancy_regex::BytesMode::Unicode
            })
            .build()
            .map(|regex| Self { regex, workspace })
            .map_err(|err| err.to_string())
    }

    fn captures(&self, text: &str) -> Option<RegexCaptures> {
        self.captures_at(text, 0)
    }

    fn captures_at(&self, text: &str, offset: usize) -> Option<RegexCaptures> {
        let captures = self
            .regex
            .captures_from_pos(text.as_bytes(), offset)
            .ok()??;
        let mut matches = self.workspace.buffer(captures.len());
        for idx in 0..captures.len() {
            matches.push(captures.get(idx).map(|m| RegexMatch {
                start: m.start(),
                end: m.end(),
            }));
        }
        Some(matches)
    }

    fn find(&self, text: &str) -> Option<RegexMatch> {
        self.regex
            .find(text.as_bytes())
            .ok()
            .flatten()
            .map(|m| RegexMatch {
                start: m.start(),
                end: m.end(),
            })
    }
}

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
    ere_match_sequence(nodes, 0, text, pos, ignore_case)
}

fn is_ere_quantifier(node: &EreNode) -> bool {
    matches!(
        node,
        EreNode::ZeroOrMore | EreNode::OneOrMore | EreNode::ZeroOrOne
    )
}

fn ere_match_sequence(
    nodes: &[EreNode],
    node_pos: usize,
    text: &[u8],
    text_pos: usize,
    ignore_case: bool,
) -> Option<usize> {
    let text_len = text.len();
    if text_pos > text_len {
        return None;
    }
    if node_pos >= nodes.len() {
        return Some(text_pos);
    }

    let node = &nodes[node_pos];
    if node_pos + 1 < nodes.len() && is_ere_quantifier(&nodes[node_pos + 1]) {
        return ere_match_quantified_sequence(
            node,
            &nodes[node_pos + 1],
            nodes,
            node_pos + 2,
            text,
            text_pos,
            ignore_case,
        );
    }

    match node {
        EreNode::Alternation => {
            let right_start = node_pos + 1;
            let right_len = find_alternation_end(&nodes[right_start..]);
            if let Some(end_pos) = ere_match_sequence(
                &nodes[right_start..right_start + right_len],
                0,
                text,
                text_pos,
                ignore_case,
            ) {
                return Some(end_pos);
            }
            let after_right = right_start + right_len + 1;
            if after_right < nodes.len() {
                return ere_match_sequence(nodes, after_right, text, text_pos, ignore_case);
            }
            None
        }
        EreNode::OpenGroup | EreNode::CloseGroup => {
            ere_match_sequence(nodes, node_pos + 1, text, text_pos, ignore_case)
        }
        EreNode::ZeroOrMore | EreNode::OneOrMore | EreNode::ZeroOrOne => {
            ere_match_sequence(nodes, node_pos + 1, text, text_pos, ignore_case)
        }
        _ => {
            let next_pos = match_single_node(node, text, text_pos, ignore_case)?;
            ere_match_sequence(nodes, node_pos + 1, text, next_pos, ignore_case)
        }
    }
}

fn ere_match_quantified_sequence(
    atom: &EreNode,
    quantifier: &EreNode,
    nodes: &[EreNode],
    rest_pos: usize,
    text: &[u8],
    text_pos: usize,
    ignore_case: bool,
) -> Option<usize> {
    let min_repetitions = match quantifier {
        EreNode::OneOrMore => 1,
        EreNode::ZeroOrMore | EreNode::ZeroOrOne => 0,
        _ => return None,
    };
    let max_repetitions = match quantifier {
        EreNode::ZeroOrOne => Some(1),
        EreNode::ZeroOrMore | EreNode::OneOrMore => None,
        _ => return None,
    };

    let mut positions = vec![text_pos];
    let mut current = text_pos;
    while max_repetitions.is_none_or(|max| positions.len() - 1 < max) {
        let Some(next) = match_single_node(atom, text, current, ignore_case) else {
            break;
        };
        if next == current {
            break;
        }
        positions.push(next);
        current = next;
    }

    if positions.len() - 1 < min_repetitions {
        return None;
    }

    for &candidate in positions[min_repetitions..].iter().rev() {
        if let Some(end_pos) = ere_match_sequence(nodes, rest_pos, text, candidate, ignore_case) {
            return Some(end_pos);
        }
    }
    None
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
                if !tc.eq_ignore_ascii_case(b) {
                    return None;
                }
            } else {
                if tc != *b {
                    return None;
                }
            }
            Some(pos + 1)
        }
        EreNode::AnyChar => {
            if pos >= text.len() {
                return None;
            }
            Some(pos + 1)
        }
        EreNode::StartAnchor => {
            if pos != 0 {
                return None;
            }
            Some(pos)
        }
        EreNode::EndAnchor => {
            if pos != text.len() {
                return None;
            }
            Some(pos)
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
                if found {
                    return None;
                }
            } else {
                if !found {
                    return None;
                }
            }
            Some(pos + 1)
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

/// Parse the replacement for a literal regex pattern with no capture groups.
/// Keep opaque bytes intact; fixed=TRUE bypasses this regex escape grammar.
pub(crate) fn literal_replacement_bytes(replacement: &[u8], perl: bool) -> WorkspaceBuffer<u8> {
    fn visit(bytes: &[u8], perl: bool, mut emit: impl FnMut(&[u8])) {
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] == b'\\' {
                let Some(&next) = bytes.get(at + 1) else {
                    break;
                };
                if !matches!(next, b'1'..=b'9') && !(perl && matches!(next, b'U' | b'L' | b'E')) {
                    emit(&bytes[at + 1..at + 2]);
                }
                at += 2;
            } else {
                let count = bytes[at..]
                    .iter()
                    .position(|&byte| byte == b'\\')
                    .unwrap_or(bytes.len() - at);
                emit(&bytes[at..at + count]);
                at += count;
            }
        }
    }
    let mut count = 0usize;
    visit(replacement, perl, |part| {
        count = count
            .checked_add(part.len())
            .unwrap_or_else(|| workspace::memory_error())
    });
    let mut output = regex_workspace().buffer(count);
    visit(replacement, perl, |part| output.extend_bytes(part));
    output
}

/// Casing policy for expanded Perl backreferences, independent of matching.
#[derive(Clone, Copy)]
pub(crate) enum ReplacementCase {
    Unicode,
    Bytes,
}
#[derive(Clone, Copy)]
enum ReplacementMode {
    Ere,
    Perl(ReplacementCase),
}

pub(crate) fn ere_replace(
    pattern: &str,
    text: &str,
    replacement: &str,
    global: bool,
    ignore_case: bool,
) -> Option<WorkspaceText> {
    let regex = TreRegex::compile(pattern, ignore_case).ok()?;
    replace_with_captures(
        text,
        replacement,
        global,
        ReplacementMode::Ere,
        &regex.workspace,
        |original, offset| regex.captures_at(original, offset),
    )
}

pub(crate) fn perl_replace(
    pattern: &str,
    text: &str,
    replacement: &str,
    global: bool,
    ignore_case: bool,
) -> Option<WorkspaceText> {
    perl_replace_with_case(
        pattern,
        text,
        replacement,
        global,
        ignore_case,
        ReplacementCase::Unicode,
    )
}

pub(crate) fn perl_replace_with_case(
    pattern: &str,
    text: &str,
    replacement: &str,
    global: bool,
    ignore_case: bool,
    case: ReplacementCase,
) -> Option<WorkspaceText> {
    let mode = ReplacementMode::Perl(case);
    if pattern == "\\b" {
        // PCRE's default word class is ASCII even for UTF8 subjects. Preserve
        // byte offsets and literal text without constructing a Vec<char>.
        let bytes = text.as_bytes();
        let mut out = regex_workspace().text(text.len());
        let mut copied = 0;
        for offset in ascii_word_boundaries(bytes) {
            out.push_str(&text[copied..offset]);
            append_r_replacement(&mut out, replacement, text, &[], mode);
            copied = offset;
            if !global {
                break;
            }
        }
        out.push_str(&text[copied..]);
        return out.finish();
    }
    let regex = PerlRegex::compile(pattern, ignore_case).ok()?;
    replace_with_captures(
        text,
        replacement,
        global,
        mode,
        &regex.workspace,
        |original, offset| regex.captures_at(original, offset),
    )
}

fn ascii_word_boundaries(bytes: &[u8]) -> impl Iterator<Item = usize> + '_ {
    (0..=bytes.len()).filter(|&offset| {
        let word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
        let left = offset
            .checked_sub(1)
            .and_then(|i| bytes.get(i))
            .is_some_and(|&b| word(b));
        let right = bytes.get(offset).is_some_and(|&b| word(b));
        left != right
    })
}

fn replace_with_captures<F>(
    text: &str,
    replacement: &str,
    global: bool,
    mode: ReplacementMode,
    workspace: &RegexWorkspace,
    mut captures_at: F,
) -> Option<WorkspaceText>
where
    F: FnMut(&str, usize) -> Option<RegexCaptures>,
{
    let mut result = workspace.text(text.len());
    let mut search_from = 0usize;
    let mut prev_nonzero = false;
    loop {
        let remaining = &text[search_from..];
        let Some(captures) = captures_at(text, search_from) else {
            result.push_str(remaining);
            break;
        };
        let m = captures.first().and_then(|whole| *whole)?;
        let zero = m.start == m.end;
        if zero && prev_nonzero && m.start == search_from && !remaining.is_empty() {
            let ch = remaining.chars().next().unwrap();
            result.push(ch);
            search_from += ch.len_utf8();
            prev_nonzero = false;
        } else {
            result.push_str(&text[search_from..m.start]);
            append_r_replacement(&mut result, replacement, text, &captures, mode);
            search_from = m.end;
            if !global {
                result.push_str(&text[search_from..]);
                break;
            }
            if zero {
                let Some(ch) = text[search_from..].chars().next() else {
                    break;
                };
                result.push(ch);
                search_from += ch.len_utf8();
            }
            prev_nonzero = !zero;
        }
        if search_from >= text.len() && !zero {
            break;
        }
        if search_from > text.len() {
            break;
        }
    }

    result.finish()
}

fn append_r_replacement(
    out: &mut TextBuilder,
    replacement: &str,
    matched_text: &str,
    captures: &[Option<RegexMatch>],
    mode: ReplacementMode,
) {
    let perl = matches!(mode, ReplacementMode::Perl(_));
    let unicode_case = !matches!(mode, ReplacementMode::Perl(ReplacementCase::Bytes));
    #[derive(Clone, Copy)]
    enum Case {
        Original,
        Upper,
        Lower,
    }
    let mut mode = Case::Original;
    let mut chars = replacement.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        // GNU discards a final escape introducer in both ERE and Perl modes.
        let Some(next) = chars.next() else {
            break;
        };
        match next {
            '1'..='9' => {
                if let Some(Some(span)) = captures.get(next as usize - '0' as usize) {
                    let captured = &matched_text[span.start..span.end];
                    match mode {
                        Case::Original => out.push_str(captured),
                        Case::Upper => {
                            for character in captured.chars() {
                                out.push(if unicode_case {
                                    crate::mainutils::rlocale::simple_upper(character)
                                } else {
                                    character.to_ascii_uppercase()
                                });
                            }
                        }
                        Case::Lower => {
                            for character in captured.chars() {
                                out.push(if unicode_case {
                                    crate::mainutils::rlocale::simple_lower(character)
                                } else {
                                    character.to_ascii_lowercase()
                                });
                            }
                        }
                    }
                }
            }
            'U' if perl => mode = Case::Upper,
            'L' if perl => mode = Case::Lower,
            'E' if perl => mode = Case::Original,
            _ => out.push(next),
        }
    }
}

pub(crate) fn ere_find_at(
    pattern: &str,
    text: &str,
    ignore_case: bool,
    offset: usize,
    bytes: bool,
) -> Option<RegexMatch> {
    TreRegex::compile_mode(pattern, ignore_case, bytes)
        .ok()?
        .captures_at(text, offset)?
        .first()
        .copied()
        .flatten()
}

pub(crate) fn mode_captures(
    pattern: &str,
    text: &str,
    ignore_case: bool,
    perl: bool,
    bytes: bool,
) -> Option<RegexCaptures> {
    if perl {
        PerlRegex::compile_mode(pattern, ignore_case, bytes)
            .ok()?
            .captures(text)
    } else {
        TreRegex::compile_mode(pattern, ignore_case, bytes)
            .ok()?
            .captures(text)
    }
}

pub(crate) fn mode_captures_all(
    pattern: &str,
    text: &str,
    ignore_case: bool,
    bytes: bool,
) -> Option<RegexMatches> {
    let re = PerlRegex::compile_mode(pattern, ignore_case, bytes).ok()?;
    let mut all = re.workspace.buffer(0);
    let mut offset = 0;
    while let Some(captures) = re.regex.captures_from_pos(text.as_bytes(), offset).ok()? {
        let whole = captures.get(0)?;
        let next = if whole.start() == whole.end() {
            whole.end().checked_add(if bytes {
                1
            } else {
                text.get(whole.end()..)?
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8)
            })?
        } else {
            whole.end()
        };
        let mut matches = re.workspace.buffer(captures.len());
        for index in 0..captures.len() {
            matches.push(captures.get(index).map(|matched| RegexMatch {
                start: matched.start(),
                end: matched.end(),
            }));
        }
        all.push(matches);
        // GNU gregexpr does not initiate another search after advancing to
        // the terminal offset. The first search may still find an end anchor.
        if next >= text.len() {
            break;
        }
        offset = next;
    }
    Some(all)
}

/// Validate before any element is classified: invalid patterns are errors,
/// including for empty input vectors, rather than successful nonmatches.
pub(crate) fn validate_pattern(
    pattern: &str,
    ignore_case: bool,
    perl: bool,
    fixed: bool,
) -> Result<(), String> {
    if fixed {
        return Ok(());
    }
    let result = if perl {
        PerlRegex::compile(pattern, ignore_case).map(|_| ())
    } else {
        TreRegex::compile(pattern, ignore_case).map(|_| ())
    };
    result.map_err(|_| format!("invalid regular expression '{pattern}'"))
}

pub(crate) fn ere_find(pattern: &str, text: &str, ignore_case: bool) -> Option<RegexMatch> {
    TreRegex::compile(pattern, ignore_case)
        .ok()
        .and_then(|regex| regex.find(text))
}

pub(crate) fn perl_find(pattern: &str, text: &str, ignore_case: bool) -> Option<RegexMatch> {
    if pattern == "\\b" {
        return ascii_word_boundaries(text.as_bytes())
            .next()
            .map(|offset| RegexMatch {
                start: offset,
                end: offset,
            });
    }
    PerlRegex::compile(pattern, ignore_case)
        .ok()
        .and_then(|regex| regex.find(text))
}

pub(crate) fn ere_captures(pattern: &str, text: &str, ignore_case: bool) -> Option<RegexCaptures> {
    TreRegex::compile(pattern, ignore_case)
        .ok()
        .and_then(|regex| regex.captures(text))
}

pub(crate) fn perl_captures(pattern: &str, text: &str, ignore_case: bool) -> Option<RegexCaptures> {
    PerlRegex::compile(pattern, ignore_case)
        .ok()
        .and_then(|regex| regex.captures(text))
}

/// All non-overlapping perl matches with their capture spans, in order —
/// the iteration do_gregexpr's perl branch needs (grep.c re-runs pcre2_exec
/// per global match). Unset groups are `None`, like `perl_captures`.
pub(crate) fn perl_captures_all(
    pattern: &str,
    text: &str,
    ignore_case: bool,
) -> Option<RegexMatches> {
    mode_captures_all(pattern, text, ignore_case, false)
}

/// Capture-group metadata for a perl pattern: `(group count excluding the
/// implicit whole-match group 0, group names)` where unnamed groups map to
/// "". Port of grep.c's pcre2_pattern_info(PCRE2_INFO_CAPTURECOUNT /
/// NAMECOUNT / NAMETABLE) block in do_regexpr. Returns None when the
/// pattern fails to compile.
pub(crate) fn perl_group_info(pattern: &str, ignore_case: bool) -> Option<(usize, GroupNames)> {
    let re = PerlRegex::compile(pattern, ignore_case).ok()?;
    let names = GroupNames::copy(
        &re.workspace,
        re.regex.capture_names().skip(1).count(),
        re.regex
            .capture_names()
            .skip(1)
            .map(|name| name.unwrap_or("")),
    );
    Some((names.iter().count(), names))
}

/// Immutable metadata retains both string and container admission until drop.
#[derive(Default)]
pub(crate) struct GroupNames(Option<WorkspaceBuffer<WorkspaceText>>);
impl GroupNames {
    fn copy<'a>(
        workspace: &RegexWorkspace,
        count: usize,
        source: impl Iterator<Item = &'a str>,
    ) -> Self {
        let mut names = workspace.buffer(count);
        for name in source {
            let mut text = workspace.text(name.len());
            text.push_str(name);
            names.push(text.finish().expect("copied valid UTF-8 capture name"));
        }
        Self(Some(names))
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &str> {
        self.0
            .iter()
            .flat_map(|names| names.iter())
            .map(|name| &**name)
    }
    #[cfg(test)]
    fn physical_capacity(&self) -> usize {
        self.0.as_ref().map_or(0, |names| {
            names.capacity() * std::mem::size_of::<WorkspaceText>()
                + names.iter().map(WorkspaceText::capacity).sum::<usize>()
        })
    }
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
                let ans = allocVector(SEXPTYPE::STRSXP, n);
                let _ans_guard = protect(ans);
                for i in 0..n {
                    SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
                }
                return ans;
            } else if PRIMVAL(op) != 0 {
                // grepl case
                let ans = allocVector(SEXPTYPE::LGLSXP, n);
                let _ans_guard = protect(ans);
                let p = LOGICAL(ans);
                for i in 0..n {
                    *p.add(i as usize) = NA_LOGICAL;
                }
                return ans;
            } else {
                let ans = allocVector(SEXPTYPE::INTSXP, n);
                let _ans_guard = protect(ans);
                let p = INTEGER(ans);
                for i in 0..n {
                    *p.add(i as usize) = NA_INTEGER;
                }
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
            let mut matched_at: Vec<R_xlen_t> = Vec::new();
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
                if if invert { !matched } else { matched } {
                    matched_at.push(i);
                }
            }
            let ans = allocVector(SEXPTYPE::STRSXP, matched_at.len() as R_xlen_t);
            let _ans_guard = protect(ans);
            for (j, i) in matched_at.iter().enumerate() {
                SET_STRING_ELT(ans, j as R_xlen_t, STRING_ELT(text, *i));
            }
            let names = getAttrib(text, crate::eval::attrib_core::R_NamesSymbol());
            if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && XLENGTH(names) == n
            {
                let out_names = allocVector(SEXPTYPE::STRSXP, matched_at.len() as R_xlen_t);
                let _names_guard = protect(out_names);
                for (j, i) in matched_at.iter().enumerate() {
                    SET_STRING_ELT(out_names, j as R_xlen_t, STRING_ELT(names, *i));
                }
                setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), out_names);
            }
            return ans;
        } else if PRIMVAL(op) != 0 {
            // grepl: return logical vector
            let ans = allocVector(SEXPTYPE::LGLSXP, n);
            let _ans_guard = protect(ans);
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
            let ans = allocVector(SEXPTYPE::INTSXP, cnt);
            let _ans_guard = protect(ans);
            let p = INTEGER(ans);
            for (j, &idx) in match_indices.iter().enumerate() {
                *p.add(j) = idx as c_int;
            }
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

/// Shared ERE predicate for base helpers that accept R-style regex patterns.
pub(crate) fn ere_is_match(pattern: &str, text: &str, ignore_case: bool) -> bool {
    ere_find(pattern, text, ignore_case).is_some()
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
            let ans = allocVector(SEXPTYPE::STRSXP, n);
            let _ans_guard = protect(ans);
            for i in 0..n {
                SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
            }
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
        let ans = allocVector(SEXPTYPE::STRSXP, n);
        let _ans_guard = protect(ans);
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
        crate::mainutils::coerce::SHALLOW_DUPLICATE_ATTRIB(ans, text);
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
        let ans = allocVector(SEXPTYPE::INTSXP, n);
        let _ans_guard = protect(ans);
        let matchlen = allocVector(SEXPTYPE::INTSXP, n);
        let _matchlen_guard = protect(matchlen);
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
        let match_len_str = c"match.length";
        let names_sym = Rf_mkChar(match_len_str.as_ptr());
        setAttrib(ans as SEXP, names_sym, matchlen);

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
        let ans = allocVector(SEXPTYPE::VECSXP, n);
        let _ans_guard = protect(ans);

        for i in 0..n {
            let text_charsxp = STRING_ELT(text, i as R_xlen_t);
            if isNA_STRING(text_charsxp) {
                let na_vec = allocVector(SEXPTYPE::INTSXP, 1);
                let _na_vec_guard = protect(na_vec);
                *INTEGER(na_vec) = NA_INTEGER;
                SET_VECTOR_ELT(ans, i as R_xlen_t, na_vec);
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
                let match_vec = allocVector(SEXPTYPE::INTSXP, 2);
                let _match_vec_guard = protect(match_vec);
                let p = INTEGER(match_vec);
                // R uses 1-based indexing for start, and end is the position after the match
                *p = (m.start + 1) as c_int;
                *p.add(1) = m.end as c_int;
                SET_VECTOR_ELT(ans, i as R_xlen_t, match_vec);
            } else {
                // No match: -1, -1
                let no_match = allocVector(SEXPTYPE::INTSXP, 1);
                let _no_match_guard = protect(no_match);
                *INTEGER(no_match) = -1;
                SET_VECTOR_ELT(ans, i as R_xlen_t, no_match);
            }
        }

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
        let _offset = CAR(args);
        args = CDR(args);
        let igcase_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let fixed_opt = asBool2(CAR(args), call);
        args = CDR(args);
        let _value = CAR(args);
        args = CDR(args);
        let _all_matches = asBool2(CAR(args), call);
        args = CDR(args);
        let _invert = asBool2(CAR(args), call);

        if !fixed_opt {
            Rf_error_fmt("grepRaw only supports fixed = TRUE in this port");
        }

        if TYPEOF(pat) != SEXPTYPE::RAWSXP {
            Rf_error_fmt("'pattern' must be a raw vector");
        }
        if TYPEOF(text) != SEXPTYPE::RAWSXP {
            Rf_error_fmt("'text' must be a raw vector");
        }

        // Get raw bytes from pattern and text
        let pat_len = XLENGTH(pat) as usize;
        let text_len = XLENGTH(text) as usize;
        if pat_len == 0 || text_len == 0 {
            let ans = allocVector(SEXPTYPE::INTSXP, 0);
            let _ans_guard = protect(ans);
            return ans;
        }

        let pat_raw = DATAPTR(pat) as *const u8;
        let text_raw = DATAPTR(text) as *const u8;

        let pat_slice = std::slice::from_raw_parts(pat_raw, pat_len);
        let text_slice = std::slice::from_raw_parts(text_raw, text_len);

        match fixed_search(text_slice, pat_slice, igcase_opt) {
            Some(pos) => {
                let ans = allocVector(SEXPTYPE::INTSXP, 1);
                let _ans_guard = protect(ans);
                *INTEGER(ans) = (pos + 1) as c_int; // 1-based
                ans
            }
            None => {
                let ans = allocVector(SEXPTYPE::INTSXP, 0);
                let _ans_guard = protect(ans);
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
pub(crate) fn levenshtein_distance(a: &[u8], b: &[u8]) -> usize {
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

        let pat_bytes = if ignore_case != 0 {
            CStr::from_ptr(pat).to_bytes().to_ascii_lowercase()
        } else {
            CStr::from_ptr(pat).to_bytes().to_vec()
        };
        let pat_bytes = match pat_bytes {
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

    #[test]
    fn unicode_capture_buffers_reject_under_session_cap() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let regex = TreRegex::compile("(é)", false).unwrap();
        let owner = session.owner_token().unwrap();
        let compiled_charge = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
        assert!(compiled_charge > 0);
        owner
            .with_arena(|arena| {
                let used = arena.total_bytes_allocated();
                arena.set_budget(crate::sexp::memory::ArenaBudget::new(used + 8, 0));
            })
            .unwrap();
        let rejected =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| regex.captures("ééé")));
        let error = rejected
            .expect_err("native capture storage must share the session cap")
            .downcast::<RError>()
            .expect("R memory error");
        assert_eq!(
            error.message,
            "regex workspace exceeds session memory limit"
        );
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            compiled_charge
        );
        drop(regex);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }

    #[test]
    fn retained_captures_charge_original_session_until_drop() {
        let left = crate::sexp::session::RSession::new_for_gc_tests();
        let right = crate::sexp::session::RSession::new_for_gc_tests();
        let left_owner = left.owner_token().unwrap();
        let right_owner = right.owner_token().unwrap();
        let captures = left.with_active(|| {
            let captures = perl_captures("(a)", "a", false).unwrap();
            assert!(
                left_owner
                    .with_arena(|arena| arena.workspace_bytes())
                    .unwrap()
                    > 0,
                "returned capture buffers must retain their native admission"
            );
            captures
        });
        right.with_active(|| {
            assert_eq!(
                right_owner
                    .with_arena(|arena| arena.workspace_bytes())
                    .unwrap(),
                0
            );
            drop(captures);
        });
        left.with_active(|| {
            assert_eq!(
                left_owner
                    .with_arena(|arena| arena.workspace_bytes())
                    .unwrap(),
                0
            );
        });
    }

    #[test]
    fn global_capture_buffers_reject_under_session_cap_and_recover() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let subject = "a".repeat(256);
        owner
            .with_arena(|arena| {
                let used = arena.total_bytes_allocated();
                arena.set_budget(crate::sexp::memory::ArenaBudget::new(used + 512, 0));
            })
            .unwrap();
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            mode_captures_all("a", &subject, false, false)
        }));
        let error = rejected
            .expect_err("global native match growth must be admitted")
            .downcast::<RError>()
            .expect("R memory error");
        assert_eq!(
            error.message,
            "regex workspace exceeds session memory limit"
        );
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
        let recovered = mode_captures("a", "a", false, true, false).unwrap();
        assert_eq!(recovered[0], Some(RegexMatch { start: 0, end: 1 }));
        drop(recovered);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }

    #[test]
    fn grepraw_rejects_reference_vectors_before_reading_payload_bytes() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active(|| {
            for (pattern_type, text_type, error) in [
                (
                    SEXPTYPE::STRSXP,
                    SEXPTYPE::RAWSXP,
                    Some("'pattern' must be a raw vector"),
                ),
                (
                    SEXPTYPE::RAWSXP,
                    SEXPTYPE::VECSXP,
                    Some("'text' must be a raw vector"),
                ),
                (SEXPTYPE::RAWSXP, SEXPTYPE::RAWSXP, None),
            ] {
                let raw_args = unsafe {
                    crate::sexp::memory::with_arena(|arena| {
                        let pattern = arena.alloc_vector(pattern_type, 1);
                        let text = arena.alloc_vector(text_type, 1);
                        let nil = R_NilValue();
                        let yes = crate::sexp::globals::R_True();
                        let no = crate::sexp::globals::R_False();
                        [pattern, text, nil, no, yes, no, no, no]
                            .into_iter()
                            .rev()
                            .fold(nil, |tail, value| {
                                arena.cons(value, tail, std::ptr::null_mut())
                            })
                    })
                };
                let args = session.sexp(raw_args).expect("owned grepRaw arguments");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    do_grepraw(
                        R_NilValue(),
                        R_NilValue(),
                        args.clone().as_raw(),
                        R_NilValue(),
                    )
                }));
                match error {
                    Some(expected) => {
                        let payload = result.expect_err("reference vector must be rejected");
                        let error = payload.downcast::<RError>().expect("R type error");
                        assert_eq!(error.message, expected);
                    }
                    None => {
                        let value = session
                            .sexp(result.expect("valid raw search"))
                            .expect("owned result");
                        assert_eq!(value.integer_elt(0), Some(1));
                    }
                }
            }
        });
    }

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
            assert_eq!(chars, b"abc");
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

    #[test]
    fn test_ere_replace_expands_r_style_backrefs() {
        assert_eq!(
            ere_replace("(a)(b)", "ab", r"\2-\1", false, false).as_deref(),
            Some("b-a")
        );
        assert_eq!(
            ere_replace("(a)", "banana", r"[\1]", true, false).as_deref(),
            Some("b[a]n[a]n[a]")
        );
        assert_eq!(
            ere_replace("(a)(b)", "ab", r"\3", false, false).as_deref(),
            Some("")
        );
    }

    #[test]
    fn test_perl_regex_supports_lookbehind_and_captures() {
        assert_eq!(
            perl_find(r"(?<=a)b", "ab", false),
            Some(RegexMatch { start: 1, end: 2 })
        );
        assert!(perl_find(r"(?<=a)b", "cb", false).is_none());

        let captures = perl_captures("(a)(b)", "ab", false).expect("expected captures");
        assert_eq!(captures[0], Some(RegexMatch { start: 0, end: 2 }));
        assert_eq!(captures[1], Some(RegexMatch { start: 0, end: 1 }));
        assert_eq!(captures[2], Some(RegexMatch { start: 1, end: 2 }));
    }

    #[test]
    fn test_perl_replace_expands_r_style_backrefs() {
        assert_eq!(
            perl_replace("(a)(b)", "ab", r"\2-\1", false, false).as_deref(),
            Some("b-a")
        );
        assert_eq!(
            perl_replace(r"(?<=a)b", "ab cb", "B", true, false).as_deref(),
            Some("aB cb")
        );
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
        assert!(match_str("helo world", "", false, Some(&nodes), false));
        assert!(match_str("hello world", "", false, Some(&nodes), false));
        assert!(match_str("helllo world", "", false, Some(&nodes), false));
        assert!(!match_str("heo world", "", false, Some(&nodes), false));
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
