#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/character.c — character/string utility functions.
//!
//! This module ports the standalone string manipulation functions that don't
//! require SEXP or R interpreter internals.
//!
//! Ported standalone functions:
//!   mystrcpy,
//!   iswvowel,
//!   tr_build_spec, tr_free_spec, tr_get_next_char_from_spec,
//!   wtr_build_spec, wtr_free_spec, wtr_get_next_char_from_spec,
//!   xtable_comp, xtable_key_comp

// ---------------------------------------------------------------------------
// String copy utility
// ---------------------------------------------------------------------------

use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::object::{NodeBody, Sexp, SexpError, SexpMut};
use crate::sexp::owner::OwnerToken;

/// Copy a string using memmove (handles overlapping regions).
pub fn mystrcpy(dest: &mut [u8], src: &[u8]) {
    let len = src.len().min(dest.len());
    dest[..len].copy_from_slice(&src[..len]);
}

// ---------------------------------------------------------------------------
// Wide character vowel check
// ---------------------------------------------------------------------------

/// Vowel codepoints (Latin vowels with diacritics).
const VOWELS: &[u32] = &[
    0x61, 0x65, 0x69, 0x6f, 0x75, 0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe8, 0xe9, 0xea, 0xeb, 0xec,
    0xed, 0xee, 0xef, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0x101,
    0x103, 0x105, 0x113, 0x115, 0x117, 0x119, 0x11b, 0x129, 0x12b, 0x12d, 0x12f, 0x131, 0x14d,
    0x14f, 0x151, 0x169, 0x16b, 0x16d, 0x16f, 0x171, 0x173,
];

/// Check if a wide character is a vowel (Latin vowels with diacritics).
pub fn iswvowel(w: char) -> bool {
    let v = w as u32;
    VOWELS.contains(&v)
}

// ---------------------------------------------------------------------------
// Byte translation specification (tr_spec)
// ---------------------------------------------------------------------------

/// Type of a translation specification entry.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub enum TrSpecType {
    Init = 0,
    Char = 1,
    Range = 2,
}

/// A single entry in a byte translation specification.
#[derive(Clone, Debug)]
pub struct TrSpec {
    pub spec_type: TrSpecType,
    pub next: Option<Box<TrSpec>>,
    pub c: Option<u8>,
    pub first: Option<u8>,
    pub last: Option<u8>,
}

/// Build a translation specification from a byte string.
///
/// Parses ranges like "a-z" into individual character entries.
/// Returns a linked list of TrSpec nodes.
pub fn tr_build_spec(s: &[u8]) -> Option<Box<TrSpec>> {
    let len = s.len();
    if len == 0 {
        return None;
    }

    let mut head: Option<Box<TrSpec>> = None;
    let mut tail: &mut Option<Box<TrSpec>> = &mut head;
    let mut i = 0;

    while i < len.saturating_sub(2) {
        let mut node = Box::new(TrSpec {
            spec_type: TrSpecType::Char,
            next: None,
            c: None,
            first: None,
            last: None,
        });

        if s[i + 1] == b'-' {
            if s[i] > s[i + 2] {
                // Decreasing range — error in R, we just skip
                i += 3;
                continue;
            }
            node.spec_type = TrSpecType::Range;
            node.first = Some(s[i]);
            node.last = Some(s[i + 2]);
            i += 3;
        } else {
            node.spec_type = TrSpecType::Char;
            node.c = Some(s[i]);
            i += 1;
        }

        *tail = Some(node);
        tail = &mut tail
            .as_mut()
            .unwrap_or_else(|| panic!("unexpected None"))
            .next;
    }

    // Remaining characters (0, 1, or 2 left)
    while i < len {
        let node = Box::new(TrSpec {
            spec_type: TrSpecType::Char,
            next: None,
            c: Some(s[i]),
            first: None,
            last: None,
        });
        *tail = Some(node);
        tail = &mut tail
            .as_mut()
            .unwrap_or_else(|| panic!("unexpected None"))
            .next;
        i += 1;
    }

    head
}

/// Free a translation specification (no-op in Rust due to RAII).
pub fn tr_free_spec(_trs: Option<Box<TrSpec>>) {
    // Rust's Drop handles deallocation automatically
}

/// Get the next character from a translation specification.
///
/// Returns the character and advances the pointer.
pub fn tr_get_next_char(p: &mut Option<Box<TrSpec>>) -> u8 {
    let current = match p.take() {
        Some(node) => node,
        None => return 0,
    };

    match current.spec_type {
        TrSpecType::Char => {
            let c = current.c.unwrap_or(0);
            *p = current.next;
            c
        }
        TrSpecType::Range => {
            let c = current.first.unwrap_or(0);
            let last = current.last.unwrap_or(0);
            if c == last {
                *p = current.next;
            } else {
                let mut new_node = current;
                new_node.first = Some(c + 1);
                *p = Some(new_node);
            }
            c
        }
        TrSpecType::Init => {
            *p = current.next;
            0
        }
    }
}

// ---------------------------------------------------------------------------
// Wide character translation specification (wtr_spec)
// ---------------------------------------------------------------------------

/// Type of a wide translation specification entry.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub enum WtrSpecType {
    Init = 0,
    Char = 1,
    Range = 2,
}

/// A single entry in a wide character translation specification.
#[derive(Clone, Debug)]
pub struct WtrSpec {
    pub spec_type: WtrSpecType,
    pub next: Option<Box<WtrSpec>>,
    pub c: Option<char>,
    pub first: Option<char>,
    pub last: Option<char>,
}

/// Build a wide character translation specification from a char slice.
///
/// Parses ranges like "a-z" into individual character entries.
pub fn wtr_build_spec(s: &[char]) -> Option<Box<WtrSpec>> {
    let len = s.len();
    if len == 0 {
        return None;
    }

    let mut head: Option<Box<WtrSpec>> = None;
    let mut tail: &mut Option<Box<WtrSpec>> = &mut head;
    let mut i = 0;

    while i < len.saturating_sub(2) {
        let mut node = Box::new(WtrSpec {
            spec_type: WtrSpecType::Char,
            next: None,
            c: None,
            first: None,
            last: None,
        });

        if s[i + 1] == '-' {
            if (s[i] as u32) > (s[i + 2] as u32) {
                i += 3;
                continue;
            }
            node.spec_type = WtrSpecType::Range;
            node.first = Some(s[i]);
            node.last = Some(s[i + 2]);
            i += 3;
        } else {
            node.spec_type = WtrSpecType::Char;
            node.c = Some(s[i]);
            i += 1;
        }

        *tail = Some(node);
        tail = &mut tail
            .as_mut()
            .unwrap_or_else(|| panic!("unexpected None"))
            .next;
    }

    while i < len {
        let node = Box::new(WtrSpec {
            spec_type: WtrSpecType::Char,
            next: None,
            c: Some(s[i]),
            first: None,
            last: None,
        });
        *tail = Some(node);
        tail = &mut tail
            .as_mut()
            .unwrap_or_else(|| panic!("unexpected None"))
            .next;
        i += 1;
    }

    head
}

/// Free a wide translation specification (no-op in Rust due to RAII).
pub fn wtr_free_spec(_trs: Option<Box<WtrSpec>>) {
    // Rust's Drop handles deallocation automatically
}

/// Get the next wide character from a translation specification.
pub fn wtr_get_next_char(p: &mut Option<Box<WtrSpec>>) -> char {
    let current = match p.take() {
        Some(node) => node,
        None => return '\0',
    };

    match current.spec_type {
        WtrSpecType::Char => {
            let c = current.c.unwrap_or('\0');
            *p = current.next;
            c
        }
        WtrSpecType::Range => {
            let c = current.first.unwrap_or('\0');
            let last = current.last.unwrap_or('\0');
            if c == last {
                *p = current.next;
            } else {
                let mut new_node = current;
                // Increment the Unicode codepoint
                let next = char::from_u32(c as u32 + 1).unwrap_or('\0');
                new_node.first = Some(next);
                *p = Some(new_node);
            }
            c
        }
        WtrSpecType::Init => {
            *p = current.next;
            '\0'
        }
    }
}

// ---------------------------------------------------------------------------
// Translation table comparison functions
// ---------------------------------------------------------------------------

/// Translation table entry.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct XtableT {
    pub c_old: char,
    pub c_new: char,
}

/// Comparison function for sorting a translation table by old character.
pub fn xtable_comp(a: &XtableT, b: &XtableT) -> std::cmp::Ordering {
    a.c_old.cmp(&b.c_old)
}

/// Key comparison for searching a translation table.
pub fn xtable_key_comp(key: char, entry: &XtableT) -> std::cmp::Ordering {
    key.cmp(&entry.c_old)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// The immutable CHARSXP sentinel used for missing strings.
fn get_na_string() -> SEXP {
    crate::sexp::globals::R_NaString()
}

/// Simplified scalar coercion used by the translated nchar entry point.
fn as_logical(x: &Sexp<'_>) -> c_int {
    let na = crate::sexp::ffi::NA_LOGICAL;
    let kind = x.typeof_();
    if !matches!(
        kind,
        SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP | SEXPTYPE::REALSXP
    ) || x.is_empty()
        || x.materialize_compact_payload_for_raw().is_err()
    {
        return na;
    }
    match kind {
        SEXPTYPE::LGLSXP => x.logical_elt(0).unwrap_or(na),
        SEXPTYPE::INTSXP => x.integer_elt(0).unwrap_or(na),
        SEXPTYPE::REALSXP => x
            .real_elt(0)
            .filter(|value| !value.is_nan())
            .map_or(na, |value| c_int::from(value != 0.0)),
        _ => na,
    }
}

/// A bounded view of sealed character cells with the translated routines'
/// original NUL-terminated interpretation. The lease retains storage even
/// when allocation or a provider callback removes the original binding.
struct CharacterBytes {
    payload: Option<crate::sexp::payload::PayloadLease>,
    len: usize,
}

impl CharacterBytes {
    fn from_value(value: &Sexp<'_>) -> Result<Self, String> {
        let invalid = || "invalid character scalar payload".to_string();
        let header = value.header();
        let NodeBody::Vector(vector) = header.body else {
            return Err(invalid());
        };
        if header.type_of() != SEXPTYPE::CHARSXP {
            return Err(invalid());
        }
        let len = usize::try_from(vector.length).map_err(|_| invalid())?;
        if len == 0 {
            return Ok(Self { payload: None, len });
        }
        let payload = header.payload_lease().ok_or_else(invalid)?.clone();
        if !payload.is_immutable() || payload.byte_elt(len) != Some(0) {
            return Err(invalid());
        }
        let len = (0..len)
            .find(|&index| payload.byte_elt(index) == Some(0))
            .unwrap_or(len);
        Ok(Self {
            payload: Some(payload),
            len,
        })
    }

    fn byte(&self, index: usize) -> u8 {
        self.payload
            .as_ref()
            .and_then(|payload| payload.byte_elt(index))
            .expect("in-bounds sealed character cell")
    }

    fn copy(
        &self,
        workspace: &crate::mainutils::grep::RegexWorkspace,
        range: std::ops::Range<usize>,
    ) -> Result<crate::mainutils::grep::WorkspaceBuffer<u8>, String> {
        if range.end > self.len || range.start > range.end {
            return Err("invalid character byte range".into());
        }
        let memory_error = |_| "character workspace exceeds session memory limit".to_string();
        let mut bytes = workspace.try_buffer(range.len()).map_err(memory_error)?;
        for index in range {
            bytes.try_push(self.byte(index)).map_err(memory_error)?;
        }
        Ok(bytes)
    }
}

pub(crate) fn allocate_character_vector<'a>(
    owner: OwnerToken<'a>,
    len: R_xlen_t,
) -> Result<SexpMut<'a>, String> {
    let factory = owner.node_factory();
    let blank = if len > 0 {
        owner.require_active().map_err(|error| error.to_string())?;
        // SAFETY: this original owner is active and no arena or payload is borrowed.
        let blank = crate::mainutils::bind::R_BlankString();
        owner.require_active().map_err(|error| error.to_string())?;
        Some(factory.wrap(blank).map_err(|error| error.to_string())?)
    } else {
        None
    };
    let allocate = || {
        factory.allocate(|arena| {
            if let Some(blank) = &blank
                && !arena.install_blank_string(blank.as_raw())
            {
                return None;
            }
            Some(arena.alloc_vector(SEXPTYPE::STRSXP, len))
        })
    };
    let value = match allocate() {
        Err(SexpError::AllocationFailed { .. }) => {
            owner.full_gc().map_err(|error| error.to_string())?;
            allocate()
        }
        result => result,
    }
    .map_err(|error| error.to_string())?;
    SexpMut::try_from_checked(value).map_err(|error| error.to_string())
}

fn r_nchar(bytes: &CharacterBytes, kind: NcharType) -> c_int {
    use crate::mainutils::essentials::unicode_display_width;
    if kind == NcharType::Bytes {
        return bytes.len as c_int;
    }
    let mut index = 0;
    let mut count = 0;
    while index < bytes.len {
        let width = match bytes.byte(index) {
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return bytes.len as c_int,
        };
        if width > bytes.len - index {
            return bytes.len as c_int;
        }
        let mut encoded = [0; 4];
        for (offset, byte) in encoded[..width].iter_mut().enumerate() {
            *byte = bytes.byte(index + offset);
        }
        let Some(character) = std::str::from_utf8(&encoded[..width])
            .ok()
            .and_then(|text| text.chars().next())
        else {
            return bytes.len as c_int;
        };
        count += if kind == NcharType::Width {
            unicode_display_width(character)
        } else {
            1
        };
        index += width;
    }
    count as c_int
}

// ---------------------------------------------------------------------------
// do_chartr — character translation
// ---------------------------------------------------------------------------

/// Safe version of character translation using `Sexp<'a>`.
///
/// Translate characters in `x`: replace characters in `old` with corresponding
/// characters in `new`.
pub unsafe fn chartr_safe<'a>(x: Sexp<'a>, old: Sexp<'a>, new: Sexp<'a>) -> Result<SEXP, String> {
    OwnerToken::with_current(|owner| {
        let _pin = owner.pin().map_err(|error| error.to_string())?;
        let na = get_na_string();

        if old.clone().typeof_() != SEXPTYPE::STRSXP {
            return Err("invalid 'old' argument".into());
        }
        if old.clone().is_empty() {
            return Err("invalid 'old' argument".into());
        }
        let old_first = old.string_elt(0).ok_or("invalid 'old' argument")?;
        if old_first.clone().as_raw() == na {
            return Err("invalid 'old' argument".into());
        }

        if new.clone().typeof_() != SEXPTYPE::STRSXP {
            return Err("invalid 'new' argument".into());
        }
        if new.clone().is_empty() {
            return Err("invalid 'new' argument".into());
        }
        let new_first = new.string_elt(0).ok_or("invalid 'new' argument")?;
        if new_first.clone().as_raw() == na {
            return Err("invalid 'new' argument".into());
        }

        if x.clone().typeof_() != SEXPTYPE::STRSXP {
            return Err("invalid 'x' argument".into());
        }

        // Build byte-level translation table (256 entries, identity by default)
        let mut xtable: [u8; 256] = {
            let mut t = [0u8; 256];
            for i in 0..256usize {
                t[i] = i as u8;
            }
            t
        };

        owner.require_active().map_err(|error| error.to_string())?;
        let workspace = crate::mainutils::grep::regex_workspace();
        let old_bytes = CharacterBytes::from_value(&old_first)?;
        let new_bytes = CharacterBytes::from_value(&new_first)?;
        let old_spec = tr_build_spec(&old_bytes.copy(&workspace, 0..old_bytes.len)?);
        let new_spec = tr_build_spec(&new_bytes.copy(&workspace, 0..new_bytes.len)?);

        // Walk both specs and build the translation table
        let mut old_p = old_spec;
        let mut new_p = new_spec;
        loop {
            let c_old = tr_get_next_char(&mut old_p);
            let c_new = tr_get_next_char(&mut new_p);
            if c_old == 0 {
                break;
            }
            if c_new == 0 {
                return Err("'old' is longer than 'new'".into());
            }
            xtable[c_old as usize] = c_new;
        }

        let factory = owner.node_factory();
        let n = x.len();
        let mut result = allocate_character_vector(owner, n)?;
        for index in 0..n {
            let element = x.try_string_elt(index).map_err(|error| error.to_string())?;
            let translated = if element.as_raw() == na {
                element
            } else {
                let bytes = CharacterBytes::from_value(&element)?;
                let mut bytes = bytes.copy(&workspace, 0..bytes.len)?;
                for byte in bytes.iter_mut() {
                    *byte = xtable[usize::from(*byte)];
                }
                factory
                    .allocate(|arena| Some(arena.alloc_charsxp(&bytes)))
                    .map_err(|error| error.to_string())?
            };
            result
                .try_set_string_elt(index, translated)
                .map_err(|error| error.to_string())?;
        }
        Ok(result.freeze().as_raw())
    })
    .map_err(|error| error.to_string())?
}

/// Translate characters in `x`: replace characters in `old` with corresponding
/// characters in `new`.
///
/// This is the Rust port of R's `do_chartr` from character.c.
/// Uses the shared original-runtime simple casing implementation.
pub unsafe fn do_chartr(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        match OwnerToken::with_current(|owner| {
            let _pin = match owner.pin() {
                Ok(pin) => pin,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };
            let args_s = match owner.sexp(args) {
                Ok(value) => value,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };
            let old = match args_s.clone().try_pairlist_arg(0).clone() {
                Ok(s) => s,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };
            let new = match args_s.clone().try_pairlist_arg(1).clone() {
                Ok(s) => s,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };
            let x = match args_s.try_pairlist_arg(2) {
                Ok(s) => s,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };

            match chartr_safe(x, old, new) {
                Ok(result) => result,
                Err(_) => crate::sexp::globals::R_NilValue(),
            }
        }) {
            Ok(value) => value,
            Err(_) => return crate::sexp::globals::R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| crate::sexp::globals::R_NilValue())
}

// ---------------------------------------------------------------------------
// do_toupper — convert to uppercase
// ---------------------------------------------------------------------------

/// Safe version of toupper using `Sexp<'a>`.
pub unsafe fn toupper_safe(x: Sexp<'_>) -> Result<SEXP, String> {
    case_transform_safe(x, true).map_err(|e| e.to_string())
}

/// Convert characters in a character vector to uppercase.
///
/// This is the Rust port of R's `do_tolower`/`do_toupper` from character.c.
/// Uses the shared original-runtime simple casing implementation.
pub unsafe fn do_toupper(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe { case_entry(args, true) }
}

// ---------------------------------------------------------------------------
// do_tolower — convert to lowercase
// ---------------------------------------------------------------------------

/// Safe version of tolower using `Sexp<'a>`.
pub unsafe fn tolower_safe(x: Sexp<'_>) -> Result<SEXP, String> {
    case_transform_safe(x, false).map_err(|e| e.to_string())
}

/// Convert characters in a character vector to lowercase.
///
/// This is the Rust port of R's `do_tolower` from character.c.
/// Uses the shared original-runtime simple casing implementation.
pub unsafe fn do_tolower(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe { case_entry(args, false) }
}

/// Shared safe implementation for toupper and tolower.
fn case_transform_safe(
    x: Sexp<'_>,
    upper: bool,
) -> Result<SEXP, crate::sexp::diagnostic::EvaluationError> {
    use crate::sexp::{
        object::SexpError,
        owner::{OwnerToken, with_runtime},
    };
    OwnerToken::with_current(|owner| {
        let _pin = owner.pin()?;
        let x = x.into_owned()?;
        let na = owner
            .sexp(crate::sexp::globals::R_NaString())?
            .into_owned()?;
        let weak = owner.weak_owner().ok_or(SexpError::OwnerNotActive)?;
        let result = with_runtime(&weak, |access| {
            super::case_conversion::transform(access, x, na, upper)
        })?;
        owner.require_active()?;
        result.map(|value| value.as_raw())
    })?
}

/// Shared native boundary; errors and callback panics remain recoverable R errors.
pub(crate) unsafe fn case_entry(args: SEXP, upper: bool) -> SEXP {
    use crate::sexp::owner::OwnerToken;
    OwnerToken::with_current(|owner| {
        let _pin = owner
            .pin()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let args = owner
            .sexp(args)
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let x = args
            .try_pairlist_arg(0)
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        match case_transform_safe(x, upper) {
            Ok(value) => value,
            Err(crate::sexp::diagnostic::EvaluationError::Diagnostic(diagnostic)) => {
                crate::mainutils::errors::raise_owned_diagnostic(&owner, diagnostic)
            }
            Err(error) => crate::sexp::context::r_error(error.to_string()),
        }
    })
    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
}

// ---------------------------------------------------------------------------
// do_nchar — character counting
// ---------------------------------------------------------------------------

/// Enum for nchar type argument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NcharType {
    Bytes,
    Chars,
    Width,
}

/// Safe version of nchar using `Sexp<'a>`.
pub fn nchar_safe(
    x: Sexp<'_>,
    type_: NcharType,
    allow_na: bool,
    keep_na: bool,
) -> Result<SEXP, String> {
    let na = get_na_string();

    if x.clone().typeof_() != SEXPTYPE::STRSXP {
        return Err("'nchar' requires a character vector".into());
    }

    OwnerToken::with_current(|owner| {
        let _pin = owner.pin().map_err(|error| error.to_string())?;
        let len = x.len();
        let value = owner
            .node_factory()
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, len)))
            .map_err(|error| error.to_string())?;
        let mut result = SexpMut::try_from_checked(value).map_err(|error| error.to_string())?;
        for index in 0..len {
            let element = x.try_string_elt(index).map_err(|error| error.to_string())?;
            let value = if element.as_raw() == na {
                if keep_na {
                    crate::sexp::ffi::NA_INTEGER
                } else {
                    2
                }
            } else {
                r_nchar(&CharacterBytes::from_value(&element)?, type_)
            };
            result
                .try_set_integer_elt(index, value)
                .map_err(|error| error.to_string())?;
        }
        Ok(result.freeze().as_raw())
    })
    .map_err(|error| error.to_string())?
}

/// Count the number of characters in each element of a character vector.
///
/// This is the Rust port of R's `do_nchar` from character.c.
/// Supports type = "bytes", "chars", "width".
pub unsafe fn do_nchar(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match OwnerToken::with_current(|owner| {
            let _pin = match owner.pin() {
                Ok(pin) => pin,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };
            let args_s = match owner.sexp(args) {
                Ok(value) => value,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };

            let x_arg = match args_s.clone().try_pairlist_arg(0).clone() {
                Ok(s) => s,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };

            let stype = match args_s.clone().try_pairlist_arg(1).clone() {
                Ok(s) => s,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };

            if stype.clone().typeof_() != SEXPTYPE::STRSXP || stype.clone().len() != 1 {
                return crate::sexp::globals::R_NilValue();
            }
            let type_char = match stype.try_string_elt(0) {
                Ok(s) => s,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };
            let type_str = match type_char.try_as_string() {
                Ok(s) => s,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };
            let type_str_trimmed = type_str.trim();

            let type_code: NcharType = if type_str_trimmed.starts_with("bytes") {
                NcharType::Bytes
            } else if type_str_trimmed.starts_with("chars") {
                NcharType::Chars
            } else if type_str_trimmed.starts_with("width") {
                NcharType::Width
            } else {
                return crate::sexp::globals::R_NilValue();
            };

            let allow_na_arg = match args_s.clone().try_pairlist_arg(2).clone() {
                Ok(s) => s,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };
            let allow_na_val = as_logical(&allow_na_arg);
            let allow_na = if allow_na_val == crate::sexp::ffi::NA_LOGICAL {
                false
            } else {
                allow_na_val != 0
            };

            // Parse keepNA (fourth arg, optional)
            let nargs = XLENGTH(args);
            let keep_na: bool;
            if nargs >= 4 {
                let keep_na_arg = match args_s.try_pairlist_arg(3) {
                    Ok(s) => s,
                    Err(_) => return crate::sexp::globals::R_NilValue(),
                };
                let keep_na_val = as_logical(&keep_na_arg);
                if keep_na_val == crate::sexp::ffi::NA_LOGICAL {
                    keep_na = type_code != NcharType::Width;
                } else {
                    keep_na = keep_na_val != 0;
                }
            } else {
                keep_na = type_code != NcharType::Width;
            }

            if owner.require_active().is_err() {
                return crate::sexp::globals::R_NilValue();
            }
            match nchar_safe(x_arg, type_code, allow_na, keep_na) {
                Ok(result) => result,
                Err(_) => crate::sexp::globals::R_NilValue(),
            }
        }) {
            Ok(value) => value,
            Err(_) => return crate::sexp::globals::R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| crate::sexp::globals::R_NilValue())
}

// ---------------------------------------------------------------------------
// do_substr — substring extraction
// ---------------------------------------------------------------------------

/// Safe version of substr using `Sexp<'a>`.
pub unsafe fn substr_safe<'a>(
    x: Sexp<'a>,
    starts: Sexp<'a>,
    stops: Option<Sexp<'a>>,
) -> Result<SEXP, String> {
    let na = get_na_string();

    if x.clone().typeof_() != SEXPTYPE::STRSXP {
        return Err("extracting substrings from a non-character object".into());
    }

    let len = x.clone().len();

    if starts.clone().typeof_() != SEXPTYPE::INTSXP || starts.clone().is_empty() {
        return Err("invalid substring arguments".into());
    }

    if let Some(ref stops) = stops
        && (stops.clone().typeof_() != SEXPTYPE::INTSXP || stops.clone().is_empty())
    {
        return Err("invalid substring arguments".into());
    }

    let k = starts.clone().len();
    let l_val = stops.as_ref().map(|s| s.clone().len()).unwrap_or(1);

    // Upstream substr.c: recycle x/start/stop to the COMMON length
    // n = max(len(x), len(start), len(stop)) — a vector start/stop with a
    // scalar x yields a vector result (substring(t, first, last) over
    // gregexpr's per-match positions). The port used to iterate x's
    // length only, silently dropping the extra positions.
    let n = if len == 0 { 0 } else { len.max(k).max(l_val) };
    OwnerToken::with_current(|owner| {
        let _pin = owner.pin().map_err(|error| error.to_string())?;
        let workspace = crate::mainutils::grep::regex_workspace();
        let factory = owner.node_factory();
        let blank = factory.character("").map_err(|error| error.to_string())?;
        let mut result = allocate_character_vector(owner, n)?;

        for index in 0..n {
            let start = starts
                .integer_elt(index % k)
                .unwrap_or(crate::sexp::ffi::NA_INTEGER);
            let stop = stops
                .as_ref()
                .and_then(|value| value.integer_elt(index % l_val))
                .unwrap_or(c_int::MAX);
            let element = x
                .try_string_elt(index % len)
                .map_err(|error| error.to_string())?;
            let character = if element.as_raw() == na {
                element
            } else if start == crate::sexp::ffi::NA_INTEGER || stop == crate::sexp::ffi::NA_INTEGER
            {
                owner.sexp(na).map_err(|error| error.to_string())?
            } else {
                let bytes = CharacterBytes::from_value(&element)?;
                let from = (start.max(1) - 1) as usize;
                let to = usize::try_from(stop).unwrap_or(0).min(bytes.len);
                if from >= to {
                    blank.clone()
                } else {
                    let slice = bytes.copy(&workspace, from..to)?;
                    factory
                        .allocate(|arena| Some(arena.alloc_charsxp(&slice)))
                        .map_err(|error| error.to_string())?
                }
            };
            result
                .try_set_string_elt(index, character)
                .map_err(|error| error.to_string())?;
        }

        Ok(result.freeze().as_raw())
    })
    .map_err(|error| error.to_string())?
}

/// Extract substrings from a character vector.
///
/// This is the Rust port of R's `do_substr` from character.c.
/// Uses the shared original-runtime simple casing implementation.
pub unsafe fn do_substr(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        match OwnerToken::with_current(|owner| {
            let _pin = match owner.pin() {
                Ok(pin) => pin,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };
            let args_s = match owner.sexp(args) {
                Ok(value) => value,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };

            let x = match args_s.clone().try_pairlist_arg(0).clone() {
                Ok(s) => s,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };

            let sa = match args_s.clone().try_pairlist_arg(1).clone() {
                Ok(s) => s,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };

            let so = match args_s.try_optional_pairlist_arg(2) {
                Ok(s) => s,
                Err(_) => return crate::sexp::globals::R_NilValue(),
            };

            match substr_safe(x, sa, so) {
                Ok(result) => result,
                Err(_) => crate::sexp::globals::R_NilValue(),
            }
        }) {
            Ok(value) => value,
            Err(_) => return crate::sexp::globals::R_NilValue(),
        }
    }))
    .unwrap_or_else(|_| crate::sexp::globals::R_NilValue())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::constructors::*;
    use crate::sexp::globals::*;
    use std::ptr;

    use super::*;

    #[test]
    fn test_mystrcpy() {
        let _session = crate::sexp::session::RSession::new();
        let mut dest = [0u8; 10];
        let src = b"hello";
        mystrcpy(&mut dest, src);
        assert_eq!(&dest[..5], b"hello");
    }

    #[test]
    fn test_mystrcpy_overlap() {
        let _session = crate::sexp::session::RSession::new();
        let mut buf = b"abcdef".to_vec();
        // Copy buf[2..5] = "cde" into buf[0..3], overlapping
        buf.copy_within(2..5, 0);
        assert_eq!(&buf[..6], b"cdedef");
    }

    #[test]
    fn test_iswvowel() {
        let _session = crate::sexp::session::RSession::new();
        assert!(iswvowel('a'));
        assert!(iswvowel('e'));
        assert!(iswvowel('i'));
        assert!(iswvowel('o'));
        assert!(iswvowel('u'));
        assert!(iswvowel('\u{00e9}')); // é
        assert!(!iswvowel('b'));
        assert!(!iswvowel('z'));
    }

    #[test]
    fn test_tr_build_spec_chars() {
        let _session = crate::sexp::session::RSession::new();
        let spec = tr_build_spec(b"abc");
        assert!(spec.is_some());
        let mut p = spec;
        assert_eq!(tr_get_next_char(&mut p), b'a');
        assert_eq!(tr_get_next_char(&mut p), b'b');
        assert_eq!(tr_get_next_char(&mut p), b'c');
        assert_eq!(tr_get_next_char(&mut p), 0);
    }

    #[test]
    fn test_tr_build_spec_range() {
        let _session = crate::sexp::session::RSession::new();
        let spec = tr_build_spec(b"a-c");
        assert!(spec.is_some());
        let mut p = spec;
        assert_eq!(tr_get_next_char(&mut p), b'a');
        assert_eq!(tr_get_next_char(&mut p), b'b');
        assert_eq!(tr_get_next_char(&mut p), b'c');
        assert_eq!(tr_get_next_char(&mut p), 0);
    }

    #[test]
    fn test_tr_build_spec_mixed() {
        let _session = crate::sexp::session::RSession::new();
        let spec = tr_build_spec(b"a-cx");
        assert!(spec.is_some());
        let mut p = spec;
        assert_eq!(tr_get_next_char(&mut p), b'a');
        assert_eq!(tr_get_next_char(&mut p), b'b');
        assert_eq!(tr_get_next_char(&mut p), b'c');
        assert_eq!(tr_get_next_char(&mut p), b'x');
        assert_eq!(tr_get_next_char(&mut p), 0);
    }

    #[test]
    fn test_tr_build_spec_single() {
        let _session = crate::sexp::session::RSession::new();
        let spec = tr_build_spec(b"z");
        assert!(spec.is_some());
        let mut p = spec;
        assert_eq!(tr_get_next_char(&mut p), b'z');
        assert_eq!(tr_get_next_char(&mut p), 0);
    }

    #[test]
    fn test_tr_build_spec_empty() {
        let _session = crate::sexp::session::RSession::new();
        assert!(tr_build_spec(b"").is_none());
    }

    #[test]
    fn test_wtr_build_spec_range() {
        let _session = crate::sexp::session::RSession::new();
        let chars: Vec<char> = "a-c".chars().collect();
        let spec = wtr_build_spec(&chars);
        assert!(spec.is_some());
        let mut p = spec;
        assert_eq!(wtr_get_next_char(&mut p), 'a');
        assert_eq!(wtr_get_next_char(&mut p), 'b');
        assert_eq!(wtr_get_next_char(&mut p), 'c');
        assert_eq!(wtr_get_next_char(&mut p), '\0');
    }

    #[test]
    fn test_xtable_comp() {
        let _session = crate::sexp::session::RSession::new();
        let a = XtableT {
            c_old: 'a',
            c_new: 'x',
        };
        let b = XtableT {
            c_old: 'b',
            c_new: 'y',
        };
        assert_eq!(xtable_comp(&a, &b), std::cmp::Ordering::Less);
        assert_eq!(xtable_comp(&b, &a), std::cmp::Ordering::Greater);
        assert_eq!(xtable_comp(&a, &a), std::cmp::Ordering::Equal);
    }

    #[test]
    fn test_xtable_key_comp() {
        let _session = crate::sexp::session::RSession::new();
        let entry = XtableT {
            c_old: 'm',
            c_new: 'x',
        };
        assert_eq!(xtable_key_comp('a', &entry), std::cmp::Ordering::Less);
        assert_eq!(xtable_key_comp('z', &entry), std::cmp::Ordering::Greater);
        assert_eq!(xtable_key_comp('m', &entry), std::cmp::Ordering::Equal);
    }

    // ---- Helper: build a STRSXP from Rust string slices ----

    /// Helper to build a STRSXP vector from Rust strings.
    unsafe fn make_strsxp(strs: &[&str]) -> SEXP {
        let n = strs.len() as c_int;
        let s = unsafe { Rf_allocVector(SEXPTYPE::STRSXP, n) };
        for (i, st) in strs.iter().enumerate() {
            let cs = std::ffi::CString::new(*st).unwrap_or_default();
            let ch = unsafe { Rf_mkCharLen(cs.as_ptr(), st.len() as c_int) };
            unsafe { SET_STRING_ELT(s, i as R_xlen_t, ch) };
        }
        s
    }

    /// Helper to build an INTSXP vector from Rust integers.
    unsafe fn make_intsxp(vals: &[c_int]) -> SEXP {
        let n = vals.len() as c_int;
        let s = unsafe { Rf_allocVector(SEXPTYPE::INTSXP, n) };
        let p = unsafe { INTEGER(s) };
        for (i, v) in vals.iter().enumerate() {
            unsafe { *p.add(i) = *v };
        }
        s
    }

    /// Helper to build an LGLSXP vector from Rust integers.
    unsafe fn make_lglsxp(vals: &[c_int]) -> SEXP {
        let n = vals.len() as c_int;
        let s = unsafe { Rf_allocVector(SEXPTYPE::LGLSXP, n) };
        let p = unsafe { LOGICAL(s) };
        for (i, v) in vals.iter().enumerate() {
            unsafe { *p.add(i) = *v };
        }
        s
    }

    /// Helper to read back a STRSXP element as a Rust String.
    unsafe fn strsxp_to_string(s: SEXP, i: R_xlen_t) -> String {
        let el = unsafe { STRING_ELT(s, i) };
        unsafe { std::ffi::CStr::from_ptr(CHAR(el)) }
            .to_string_lossy()
            .into_owned()
    }

    /// Helper to build a pairlist (args) from a vec of SEXP values.
    unsafe fn make_args(items: &[SEXP]) -> SEXP {
        if items.is_empty() {
            return R_NilValue();
        }
        let mut result = R_NilValue();
        for item in items.iter().rev() {
            result = unsafe { Rf_cons(*item, result) };
        }
        result
    }

    // ---- Tests for do_toupper ----

    #[test]
    fn test_do_toupper_basic() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["hello", "world"]);
            let args = make_args(&[x]);
            let result = do_toupper(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 2);
            assert_eq!(strsxp_to_string(result, 0), "HELLO");
            assert_eq!(strsxp_to_string(result, 1), "WORLD");
        }
    }

    #[test]
    fn test_do_toupper_already_upper() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["ABC", "XYZ"]);
            let args = make_args(&[x]);
            let result = do_toupper(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "ABC");
            assert_eq!(strsxp_to_string(result, 1), "XYZ");
        }
    }

    #[test]
    fn test_do_toupper_mixed() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["HeLLo", "WoRlD"]);
            let args = make_args(&[x]);
            let result = do_toupper(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "HELLO");
            assert_eq!(strsxp_to_string(result, 1), "WORLD");
        }
    }

    #[test]
    fn test_do_toupper_empty_string() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&[""]);
            let args = make_args(&[x]);
            let result = do_toupper(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "");
        }
    }

    // ---- Tests for do_tolower ----

    #[test]
    fn test_do_tolower_basic() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["HELLO", "WORLD"]);
            let args = make_args(&[x]);
            let result = do_tolower(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 2);
            assert_eq!(strsxp_to_string(result, 0), "hello");
            assert_eq!(strsxp_to_string(result, 1), "world");
        }
    }

    #[test]
    fn test_do_tolower_already_lower() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["abc", "xyz"]);
            let args = make_args(&[x]);
            let result = do_tolower(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "abc");
            assert_eq!(strsxp_to_string(result, 1), "xyz");
        }
    }

    #[test]
    fn test_do_tolower_mixed() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["HeLLo", "WoRlD"]);
            let args = make_args(&[x]);
            let result = do_tolower(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "hello");
            assert_eq!(strsxp_to_string(result, 1), "world");
        }
    }

    // ---- Tests for do_chartr ----

    #[test]
    fn test_do_chartr_basic() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let old = make_strsxp(&["aeiou"]);
            let new = make_strsxp(&["AEIOU"]);
            let x = make_strsxp(&["hello world"]);
            let args = make_args(&[old, new, x]);
            let result = do_chartr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(strsxp_to_string(result, 0), "hEllO wOrld");
        }
    }

    #[test]
    fn test_do_chartr_range() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let old = make_strsxp(&["a-z"]);
            let new = make_strsxp(&["A-Z"]);
            let x = make_strsxp(&["hello"]);
            let args = make_args(&[old, new, x]);
            let result = do_chartr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "HELLO");
        }
    }

    #[test]
    fn test_do_chartr_multiple_strings() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let old = make_strsxp(&["ab"]);
            let new = make_strsxp(&["BA"]);
            let x = make_strsxp(&["abc", "bad"]);
            let args = make_args(&[old, new, x]);
            let result = do_chartr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "BAc");
            assert_eq!(strsxp_to_string(result, 1), "ABd");
        }
    }

    #[test]
    fn test_do_chartr_no_match() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let old = make_strsxp(&["xyz"]);
            let new = make_strsxp(&["XYZ"]);
            let x = make_strsxp(&["hello"]);
            let args = make_args(&[old, new, x]);
            let result = do_chartr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            // Characters not in 'old' pass through unchanged
            assert_eq!(strsxp_to_string(result, 0), "hello");
        }
    }

    // ---- Tests for do_nchar ----

    #[test]
    fn test_do_nchar_bytes() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["hello", "world", ""]);
            let stype = make_strsxp(&["bytes"]);
            let allow_na = make_lglsxp(&[0]);
            let args = make_args(&[x, stype, allow_na]);
            let result = do_nchar(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 3);
            let p = INTEGER(result);
            assert_eq!(*p.add(0), 5);
            assert_eq!(*p.add(1), 5);
            assert_eq!(*p.add(2), 0);
        }
    }

    #[test]
    fn test_do_nchar_chars() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["abc", "de"]);
            let stype = make_strsxp(&["chars"]);
            let allow_na = make_lglsxp(&[0]);
            let args = make_args(&[x, stype, allow_na]);
            let result = do_nchar(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            let p = INTEGER(result);
            assert_eq!(*p.add(0), 3);
            assert_eq!(*p.add(1), 2);
        }
    }

    #[test]
    fn test_do_nchar_width() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["test"]);
            let stype = make_strsxp(&["width"]);
            let allow_na = make_lglsxp(&[0]);
            let args = make_args(&[x, stype, allow_na]);
            let result = do_nchar(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            let p = INTEGER(result);
            assert_eq!(*p.add(0), 4);
        }
    }

    #[test]
    fn test_do_nchar_keep_na_false() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // For non-width types, default keepNA = TRUE
            let x = make_strsxp(&["hello"]);
            let stype = make_strsxp(&["bytes"]);
            let allow_na = make_lglsxp(&[0]);
            let keep_na = make_lglsxp(&[0]); // FALSE
            let args = make_args(&[x, stype, allow_na, keep_na]);
            let result = do_nchar(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            let p = INTEGER(result);
            assert_eq!(*p.add(0), 5);
        }
    }

    // ---- Tests for do_substr ----

    #[test]
    fn test_do_substr_basic() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["hello"]);
            let start = make_intsxp(&[2]);
            let stop = make_intsxp(&[4]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(LENGTH(result), 1);
            assert_eq!(strsxp_to_string(result, 0), "ell");
        }
    }

    #[test]
    fn test_do_substr_full_string() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["hello"]);
            let start = make_intsxp(&[1]);
            let stop = make_intsxp(&[5]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "hello");
        }
    }

    #[test]
    fn test_do_substr_beyond_end() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["hello"]);
            let start = make_intsxp(&[3]);
            let stop = make_intsxp(&[100]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            // Should return "llo" (from position 3 to end)
            assert_eq!(strsxp_to_string(result, 0), "llo");
        }
    }

    #[test]
    fn test_do_substr_empty_result() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["hello"]);
            let start = make_intsxp(&[4]);
            let stop = make_intsxp(&[3]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            // start > stop => blank string
            assert_eq!(strsxp_to_string(result, 0), "");
        }
    }

    #[test]
    fn test_do_substr_multiple_strings() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["hello", "world"]);
            let start = make_intsxp(&[2]);
            let stop = make_intsxp(&[3]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(LENGTH(result), 2);
            assert_eq!(strsxp_to_string(result, 0), "el");
            assert_eq!(strsxp_to_string(result, 1), "or");
        }
    }

    #[test]
    fn test_do_substr_single_char() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&["hello"]);
            let start = make_intsxp(&[3]);
            let stop = make_intsxp(&[3]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(strsxp_to_string(result, 0), "l");
        }
    }

    #[test]
    fn test_do_substr_empty_input() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_strsxp(&[]);
            let start = make_intsxp(&[1]);
            let stop = make_intsxp(&[1]);
            let args = make_args(&[x, start, stop]);
            let result = do_substr(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(LENGTH(result), 0);
        }
    }

    // ---- Tests for helper functions ----

    #[test]
    fn test_as_logical() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let t = make_lglsxp(&[1]);
            let f = make_lglsxp(&[0]);
            OwnerToken::with_current(|owner| {
                assert_eq!(as_logical(&owner.sexp(t).unwrap()), 1);
                assert_eq!(as_logical(&owner.sexp(f).unwrap()), 0);
            })
            .unwrap()
        }
    }

    #[test]
    fn test_charsxp_copy_stops_at_nul() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let character = session
            .owner_token()
            .unwrap()
            .node_factory()
            .allocate(|arena| Some(arena.alloc_charsxp(b"hello\0ignored")))
            .unwrap();
        let bytes = CharacterBytes::from_value(&character).unwrap();
        let copy = bytes
            .copy(&crate::mainutils::grep::regex_workspace(), 0..bytes.len)
            .unwrap();
        assert_eq!(&*copy, b"hello");
    }

    #[test]
    fn test_na_string_uses_global_sentinel() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(get_na_string(), crate::sexp::globals::R_NaString());
    }
}
