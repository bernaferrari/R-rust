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

use crate::mainutils::essentials::{set_integer_checked, set_logical_checked};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::*;
use crate::sexp::protect::protect;

#[cfg(test)]
mod capture_publication_tests;
#[cfg(test)]
mod compile_memory_tests;
#[cfg(test)]
mod group_names_memory_tests;
#[cfg(test)]
mod perl_class_tests;
mod perl_dialect;
mod posix_dialect;
mod posix_nfa;
mod positions;
pub(crate) use positions::{EmptyPatternMode, empty_pattern_positions};
#[cfg(test)]
mod replacement_memory_tests;
#[cfg(test)]
mod replacement_tests;
pub(crate) mod workspace;
pub(crate) use workspace::{RegexWorkspace, WorkspaceBuffer};
use workspace::{TextBuilder, WorkspaceText};

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
        crate::sexp::owner::OwnerToken::with_current(|owner| {
            let _pin = owner.pin().unwrap_or_else(|_| workspace::memory_error());
            // This immutable sibling capability remains valid during a registered
            // mutable arena lend and observes the same live original budget cells.
            let ledger = (*owner.as_ptr()).heap_identity.byte_ledger();
            let availability = crate::sexp::instance::instance_liveness(owner.as_ptr());
            RegexWorkspace::new(ledger, availability)
        })
        .unwrap_or_else(|_| workspace::memory_error())
    }
}

// ---------------------------------------------------------------------------
// Local helper functions (matching patterns from match_mod.rs etc.)
// ---------------------------------------------------------------------------

#[inline(always)]
fn NA_STRING() -> SEXP {
    crate::mainutils::relop::NA_STRING()
}

#[inline(always)]
fn isNA_STRING(s: SEXP) -> bool {
    if s.is_null() {
        return true;
    }
    LEVELS(s) & 1 != 0
}

/// isString check -- STRSXP type.
#[inline(always)]
fn isString(x: SEXP) -> bool {
    !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP
}

/// isNull check.
#[inline(always)]
fn isNull(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue()
}

/// checkArity -- stub, no-op.
#[inline(always)]
fn checkArity(op: SEXP, args: SEXP) {
    crate::mainutils::relop::checkArity(op, args)
}

/// asLogical -- extract logical value from scalar.
#[inline(always)]
fn asLogical(x: SEXP) -> c_int {
    if x.is_null() || XLENGTH(x) == 0 {
        return NA_INTEGER;
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP {
        LOGICAL_ELT(x, 0)
    } else if t == SEXPTYPE::INTSXP {
        INTEGER_ELT(x, 0)
    } else {
        NA_INTEGER
    }
}

/// asBool2 -- extract boolean from scalar (with error on NA).
#[inline(always)]
fn asBool2(x: SEXP, _call: SEXP) -> bool {
    let v = asLogical(x);
    if v == NA_INTEGER {
        std::panic::panic_any(RError {
            message: "invalid 'logical' argument, NA value".to_string(),
        });
    }
    v != 0
}

#[inline(always)]
fn PRIMVAL(op: SEXP) -> c_int {
    crate::mainutils::relop::PRIMVAL(op)
}

#[inline(always)]
fn R_NamesSymbol() -> SEXP {
    crate::sexp::attrib_core::R_NamesSymbol()
}

#[inline(always)]
fn getAttrib(x: SEXP, which: SEXP) -> SEXP {
    crate::sexp::attrib_core::getAttrib(x, which)
}

#[inline(always)]
fn setAttrib(x: SEXP, which: SEXP, value: SEXP) {
    crate::eval::attrib_core::setAttrib(x, which, value);
}

/// Text of a CHARSXP as GNU `CHAR(x)` exposes it, lossily decoded.
fn char_string(x: SEXP) -> String {
    String::from_utf8_lossy(&crate::mainutils::printutils::charsxp_bytes(x)).into_owned()
}

/// Checked CHARSXP holding `bytes`, matching
/// `Rf_mkChar(CString::new(bytes).unwrap_or_default())`: an interior NUL
/// yields the empty string.
fn mk_char_cstring(bytes: &[u8]) -> SEXP {
    let bytes = if bytes.contains(&0) { &[][..] } else { bytes };
    crate::sexp::constructors::mk_char_bytes(bytes)
}

/// ScalarString -- create scalar STRSXP.
#[inline(always)]
fn ScalarString(x: SEXP) -> SEXP {
    Rf_ScalarString(x)
}

#[inline(always)]
fn allocVector<T: Into<crate::sexp::ffi::SEXPTYPE>>(
    sexptype: T,
    length: crate::sexp::ffi::R_xlen_t,
) -> SEXP {
    crate::sexp::constructors::Rf_allocVector3(sexptype, length)
}

/// Rf_warning -- issue a warning (forward to errors module).
#[inline(always)]
fn Rf_warning_fmt(msg: &str) {
    // We just print to stderr as a simple warning
    eprintln!("Warning: {}", msg);
}

/// Rf_error_fmt -- panic with an RError.
#[inline(always)]
fn Rf_error_fmt(msg: &str) -> ! {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
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
    /// POSIX leftmost-longest tagged NFA; `None` when the pattern contains
    /// backreferences, which no finite automaton can execute. Backref
    /// patterns fall back to the backtracking engine, matching TRE's own
    /// hybrid behavior.
    nfa: Option<posix_nfa::PosixNfa>,
    regex: PerlInnerRegex,
    bytes: bool,
    workspace: RegexWorkspace,
    /// Workspace charge for the compiled pattern machinery, held for the
    /// lifetime of the compiled object and released on drop — the budget
    /// contract previously enforced through TRE's arena allocations.
    _charge: crate::sexp::memory::WorkspaceReservation,
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
        let prepared = posix_dialect::prepare(pattern)?;
        // Budget charge for the compiled machinery before any allocation:
        // proportional to pattern size like the TNFA tables it replaces —
        // compilation of oversized patterns must fail as a budget error
        // rather than consuming unaccounted memory in the automaton.
        let chars = if bytes {
            pattern.len()
        } else {
            pattern.chars().count()
        };
        let charge = match workspace.try_reservation(chars.max(1) * 64 + 1024) {
            Ok(reservation) => reservation,
            Err(()) => workspace::memory_error(),
        };
        // Backreferences make the pattern non-regular: the finite automaton
        // cannot parse them, so those patterns run on the backtracking
        // engine with Perl-preference semantics — the same fallback TRE
        // applies when its backref machinery engages.
        let has_backrefs = posix_dialect::has_backrefs(&prepared);
        let nfa = if has_backrefs {
            None
        } else {
            Some(posix_nfa::compile(&prepared, ignore_case, bytes)?)
        };
        let regex = PerlRegexBuilder::new(&prepared)
            .case_insensitive(ignore_case)
            .bytes_mode(if bytes {
                fancy_regex::BytesMode::Ascii
            } else {
                fancy_regex::BytesMode::Unicode
            })
            .build()
            .map_err(|err| err.to_string())?;
        Ok(Self {
            nfa,
            regex,
            bytes,
            workspace,
            _charge: charge,
        })
    }

    fn captures(&self, text: &str) -> Option<RegexCaptures> {
        self.captures_at(text, 0)
    }

    fn captures_at(&self, text: &str, offset: usize) -> Option<RegexCaptures> {
        if let Some(nfa) = &self.nfa {
            return self.nfa_captures_at(nfa, text, offset);
        }
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

    /// POSIX leftmost-longest capture extraction through the tagged NFA.
    /// Positions are computed over the u32 alphabet (chars or bytes) and
    /// converted back to byte offsets of the original text.
    fn nfa_captures_at(
        &self,
        nfa: &posix_nfa::PosixNfa,
        text: &str,
        offset: usize,
    ) -> Option<RegexCaptures> {
        // Build the u32 alphabet plus the char-index -> byte-offset table.
        let mut alphabet: Vec<u32> = Vec::new();
        let mut byte_of_char: Vec<usize> = Vec::new();
        if self.bytes {
            for (byte_pos, byte) in text.as_bytes().iter().enumerate() {
                alphabet.push(u32::from(*byte));
                byte_of_char.push(byte_pos);
            }
        } else {
            for (byte_pos, ch) in text.char_indices() {
                alphabet.push(ch as u32);
                byte_of_char.push(byte_pos);
            }
        }
        let text_len = text.len();
        let byte_at = |char_pos: usize| -> usize {
            if char_pos >= byte_of_char.len() {
                text_len
            } else {
                byte_of_char[char_pos]
            }
        };
        if offset > text_len {
            return None;
        }
        let char_offset = alphabet
            .iter()
            .zip(byte_of_char.iter())
            .position(|(_, &b)| b >= offset)
            .unwrap_or(alphabet.len());
        let slots = nfa.captures_chars(&alphabet, char_offset)?;
        let ngroups = slots.len() / 2;
        let mut matches = self.workspace.buffer(ngroups);
        for group in 0..ngroups {
            let pair = match (slots.get(2 * group), slots.get(2 * group + 1)) {
                (Some(Some(s)), Some(Some(e))) if e >= s => Some(RegexMatch {
                    start: byte_at(*s),
                    end: byte_at(*e),
                }),
                _ => None,
            };
            matches.push(pair);
        }
        Some(matches)
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
        self.captures(text)
            .and_then(|mut captures| captures.get_mut(0).and_then(|whole| whole.take()))
    }
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
pub fn R_grep_fixed(pat: Option<&CStr>, target: Option<&CStr>, ignore_case: c_int) -> c_int {
    let (Some(pat), Some(target)) = (pat, target) else {
        return -1;
    };
    R_grep_fixed_inner(pat.to_bytes(), target.to_bytes(), ignore_case != 0)
}

// ---------------------------------------------------------------------------
// do_grep -- R's grep() / grepl() builtin (port of grep.c:do_grep)
// ---------------------------------------------------------------------------



/// Shared ERE predicate for base helpers that accept R-style regex patterns.
pub(crate) fn ere_is_match(pattern: &str, text: &str, ignore_case: bool) -> bool {
    ere_find(pattern, text, ignore_case).is_some()
}

// ---------------------------------------------------------------------------
// do_gsub -- R's sub() / gsub() builtin (port of grep.c:do_gsub)
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// do_regexpr -- R's regexpr() builtin (port of grep.c:do_regexpr)
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// do_regexec -- R's regexec() builtin (port of grep.c:do_regexec)
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// do_grepraw -- R's grepRaw() builtin (port of grep.c:do_grepraw)
// ---------------------------------------------------------------------------

/// R's grepRaw() builtin.
///
/// SEXP do_grepraw(SEXP call, SEXP op, SEXP args, SEXP env)
///
/// Port of grep.c:do_grepraw. Performs fixed-byte matching on raw vectors.
pub fn do_grepraw(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
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

    let owner = crate::mainutils::essentials::active_owner();
    let pat_bytes: Vec<u8> = crate::mainutils::essentials::owner_sexp(owner, pat)
        .iter_raw()
        .collect();
    let text_bytes: Vec<u8> = crate::mainutils::essentials::owner_sexp(owner, text)
        .iter_raw()
        .collect();

    match fixed_search(&text_bytes, &pat_bytes, igcase_opt) {
        Some(pos) => {
            let ans = allocVector(SEXPTYPE::INTSXP, 1);
            let _ans_guard = protect(ans);
            set_integer_checked(ans, 0, (pos + 1) as c_int); // 1-based
            ans
        }
        None => {
            let ans = allocVector(SEXPTYPE::INTSXP, 0);
            let _ans_guard = protect(ans);
            ans
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
pub fn R_pcre_exec(
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
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
        assert_eq!(R_grep_fixed(Some(&pat), Some(&target), 0), 6);

        let target2 = test_ok(std::ffi::CString::new("hello World"));
        assert_eq!(R_grep_fixed(Some(&pat), Some(&target2), 1), 6);

        let target3 = test_ok(std::ffi::CString::new("no match"));
        assert_eq!(R_grep_fixed(Some(&pat), Some(&target3), 0), -1);
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

    // -- R_grep_fixed_inner tests --

    #[test]
    fn test_r_grep_fixed_inner() {
        assert_eq!(R_grep_fixed_inner(b"world", b"hello world", false), 6);
        assert_eq!(R_grep_fixed_inner(b"WORLD", b"hello world", true), 6);
        assert_eq!(R_grep_fixed_inner(b"xyz", b"hello world", false), -1);
        assert_eq!(R_grep_fixed_inner(b"", b"hello", false), 0);
    }
}
