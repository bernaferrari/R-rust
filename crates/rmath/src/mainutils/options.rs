#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/options.c
//!
//! This file implements the interface to the R `options(...)` command.
//! Options are stored on the active R instance, mirroring R's .Options
//! dotted-pair list but using Rust-native storage.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::eval::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::memory_ext::allocLang;
use crate::sexp::object::{PairlistBuilder, SessionNodeFactory, Sexp, SexpMut, SexpResult};
use crate::sexp::owner::OwnerToken;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OptionsInitialization {
    Uninitialized,
    Initializing,
    Initialized,
}

/// Only this owner-local phase is borrowed, and only between callbacks.
struct OptionsInitializationGuard {
    owner: *mut crate::sexp::instance::RInstance,
    availability: crate::sexp::instance::InstanceLiveness,
    _pin: Option<crate::sexp::owner::OwnerPin>,
}

impl OptionsInitializationGuard {
    unsafe fn begin(owner: *mut crate::sexp::instance::RInstance) -> Option<Self> {
        unsafe {
            if (*owner).options_initialization != OptionsInitialization::Uninitialized {
                return None;
            }
            let availability = crate::sexp::instance::instance_liveness(owner);
            let pin = require_options(OwnerToken::from_raw(owner).pin());
            (*owner).options_initialization = OptionsInitialization::Initializing;
            Some(Self {
                owner,
                availability,
                _pin: pin,
            })
        }
    }
}

impl Drop for OptionsInitializationGuard {
    fn drop(&mut self) {
        if self._pin.is_some() || self.availability.is_live() {
            // No R call occurs between this availability check and field access.
            unsafe {
                if (*self.owner).options_initialization == OptionsInitialization::Initializing {
                    (*self.owner).options_initialization = OptionsInitialization::Uninitialized;
                }
            }
        }
    }
}

fn require_options<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|error| {
        std::panic::panic_any(RError {
            message: error.to_string(),
        })
    })
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// From Print.h -- minimum valid printing width.
const R_MIN_WIDTH_OPT: c_int = 10;

/// From Print.h -- maximum valid printing width.
const R_MAX_WIDTH_OPT: c_int = 10000;

/// From Print.h -- minimum valid printing digits.
const R_MIN_DIGITS_OPT: c_int = 1;

/// From Print.h -- maximum valid printing digits.
const R_MAX_DIGITS_OPT: c_int = 22;

/// From Defn.h -- minimum valid expressions limit.
pub const R_MIN_EXPRESSIONS_OPT: c_int = 25;

/// From Defn.h -- maximum valid expressions limit.
pub const R_MAX_EXPRESSIONS_OPT: c_int = 500000;

/// From Print.h -- minimum valid scipen.
const R_MIN_SCIPEN_OPT: c_int = -9;
const R_MAX_SCIPEN_OPT: c_int = 9999;

/// warn_type enumeration (from Defn.h / Rinternals.h).
pub type warn_type = c_int;

pub const iWARN: warn_type = 0;
pub const iSILENT: warn_type = 1;
pub const iERROR: warn_type = 2;

// ---------------------------------------------------------------------------
// Options storage
// ---------------------------------------------------------------------------

/// Get the symbol for ".Options" -- cached via Rf_install.
fn options_symbol() -> SEXP {
    unsafe { Rf_install(c".Options".as_ptr()) }
}

// ---------------------------------------------------------------------------
// Local helper functions (not exported, matching pattern in other modules)
// ---------------------------------------------------------------------------

/// Raise an R error (via panic).
unsafe fn r_error(msg: &str) {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

/// Check arity of a call against the canonical primitive table.
unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe {
        crate::mainutils::relop::checkArity(op, args);
    }
}

/// Convert SEXP to c_int (asInteger).
unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return NA_INTEGER;
        }
        let addr = x as usize;
        if addr < 0x1000 {
            return NA_INTEGER;
        }
        if (addr & 0x7) != 0 {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP {
            if LENGTH(x) >= 1 {
                let data = INTEGER(x);
                let data_addr = data as usize;
                if data.is_null()
                    || data_addr < 0x1000
                    || (data_addr & (std::mem::align_of::<c_int>() - 1)) != 0
                {
                    return NA_INTEGER;
                }
                return *data;
            }
        } else if t == SEXPTYPE::REALSXP {
            if LENGTH(x) >= 1 {
                let data = REAL(x);
                let data_addr = data as usize;
                if data.is_null()
                    || data_addr < 0x1000
                    || (data_addr & (std::mem::align_of::<c_double>() - 1)) != 0
                {
                    return NA_INTEGER;
                }
                let v = *data;
                if ISNAN(v) {
                    return NA_INTEGER;
                }
                if v > c_int::MAX as c_double || v < c_int::MIN as c_double {
                    return NA_INTEGER;
                }
                return v as c_int;
            }
        } else if t == SEXPTYPE::LGLSXP && LENGTH(x) >= 1 {
            let data = LOGICAL(x);
            let data_addr = data as usize;
            if data.is_null()
                || data_addr < 0x1000
                || (data_addr & (std::mem::align_of::<c_int>() - 1)) != 0
            {
                return NA_INTEGER;
            }
            return *data;
        }
        NA_INTEGER
    }
}

/// Convert SEXP to logical (asLogical).
unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return NA_LOGICAL;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP {
            if LENGTH(x) >= 1 {
                return *LOGICAL(x).add(0);
            }
        } else if t == SEXPTYPE::INTSXP {
            if LENGTH(x) >= 1 {
                return *INTEGER(x).add(0);
            }
        } else if t == SEXPTYPE::REALSXP && LENGTH(x) >= 1 {
            let v = *REAL(x).add(0);
            if ISNAN(v) {
                return NA_LOGICAL;
            }
            return if v != 0.0 { 1 } else { 0 };
        }
        NA_LOGICAL
    }
}

/// Check if x is numeric (integer or real).
unsafe fn isNumeric(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP || t == SEXPTYPE::CPLXSXP) as c_int
    }
}

/// Check if x is a pairlist.
unsafe fn isPairList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (TYPEOF(x) == SEXPTYPE::LISTSXP) as c_int
    }
}

/// Check if x is a vector list (VECSXP or EXPRSXP).
unsafe fn isVectorList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP) as c_int
    }
}

/// Check if x is a language object.
unsafe fn isLanguage(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (TYPEOF(x) == SEXPTYPE::LANGSXP) as c_int
    }
}

/// Check if x is an expression.
unsafe fn isExpression(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        (TYPEOF(x) == SEXPTYPE::EXPRSXP) as c_int
    }
}

/// Check if x is a function (CLOSXP, BUILTINSXP, SPECIALSXP).
unsafe fn isFunction(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        (t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP) as c_int
    }
}

/// Get length of a SEXP (pairlist or vector).
unsafe fn length(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP || t == DOTSXP {
            let mut count = 0i32;
            let mut current = x;
            while !current.is_null() && current != R_NilValue() {
                count += 1;
                current = CDR(current);
            }
            count
        } else {
            LENGTH(x)
        }
    }
}

/// Duplicate a SEXP (shallow -- for our purposes, just return the same pointer
/// since we don't have a full duplicate implementation available).
unsafe fn duplicate_sexp(x: SEXP) -> SEXP {
    unsafe {
        // For a full implementation this would deep-copy; for now return the
        // same pointer as the C code would have used Rf_duplicate.
        // The real Rf_duplicate is in mainutils/duplicate.rs.
        crate::mainutils::duplicate::Rf_duplicate(x)
    }
}

/// installTrChar -- install a symbol from a CHARSXP.
unsafe fn installTrChar(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            return ptr::null_mut();
        }
        Rf_install(CHAR(x))
    }
}

/// EnsureString -- coerce to CHARSXP (print name).
unsafe fn EnsureString(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            return ptr::null_mut();
        }
        // If it's a SYMSXP, return its print name
        if TYPEOF(x) == SEXPTYPE::SYMSXP {
            return PRINTNAME(x);
        }
        // If it's a CHARSXP, return as-is
        if TYPEOF(x) == SEXPTYPE::CHARSXP {
            return x;
        }
        // Otherwise return a null CHARSXP
        ptr::null_mut()
    }
}

unsafe fn translateChar(x: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(x) }
}

/// asChar -- get the first string element as a CHARSXP.
unsafe fn asChar(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            return ptr::null_mut();
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP && LENGTH(x) >= 1 {
            return STRING_ELT(x, 0);
        }
        if TYPEOF(x) == SEXPTYPE::CHARSXP {
            return x;
        }
        ptr::null_mut()
    }
}

/// String comparison (like streql in R).
unsafe fn streql(a: *const c_char, b: *const c_char) -> bool {
    unsafe {
        if a.is_null() || b.is_null() {
            return false;
        }
        CStr::from_ptr(a).to_bytes() == CStr::from_ptr(b).to_bytes()
    }
}

/// Check if a character pointer points to an empty string.
unsafe fn char_is_empty(s: *const c_char) -> bool {
    unsafe { !s.is_null() && *s == 0 }
}

// ---------------------------------------------------------------------------
// Standalone numeric clamping utilities
// ---------------------------------------------------------------------------

/// Clamp a printing width value to the valid range [R_MIN_WIDTH_OPT, R_MAX_WIDTH_OPT].
pub unsafe fn fixup_width(w: c_int) -> c_int {
    if w == c_int::MIN || w < R_MIN_WIDTH_OPT || w > R_MAX_WIDTH_OPT {
        80
    } else {
        w
    }
}

/// Clamp a printing digits value to the valid range [R_MIN_DIGITS_OPT, R_MAX_DIGITS_OPT].
pub unsafe fn fixup_digits(d: c_int) -> c_int {
    if d == c_int::MIN || d < R_MIN_DIGITS_OPT || d > R_MAX_DIGITS_OPT {
        7
    } else {
        d
    }
}

/// Clamp a scipen value to the valid range [R_MIN_SCIPEN_OPT, R_MAX_SCIPEN_OPT].
pub unsafe fn fixup_scipen(d: c_int) -> c_int {
    if d == c_int::MIN || d < R_MIN_SCIPEN_OPT || d > R_MAX_SCIPEN_OPT {
        if d == c_int::MIN {
            0
        } else if d < R_MIN_SCIPEN_OPT {
            R_MIN_SCIPEN_OPT
        } else {
            R_MAX_SCIPEN_OPT
        }
    } else {
        d
    }
}

/// Clamp a deparse.cutoff value: must be positive.
pub unsafe fn fixup_deparse_cutoff(w: c_int) -> c_int {
    if w == c_int::MIN || w <= 0 { 60 } else { w }
}

// ---------------------------------------------------------------------------
// Internal FixupWidth / FixupDigits / FixupScipen (SEXP versions)
// ---------------------------------------------------------------------------

/// FixupWidth: clamp width SEXP value, returning clamped integer.
unsafe fn FixupWidth(width: SEXP, warn: warn_type) -> c_int {
    unsafe {
        let w = asInteger(width);
        if w == NA_INTEGER || w < R_MIN_WIDTH_OPT || w > R_MAX_WIDTH_OPT {
            match warn {
                iWARN | iSILENT => return 80,
                iERROR => r_error("invalid printing width"),
                _ => return 80,
            }
        }
        w
    }
}

/// FixupDigits: clamp digits SEXP value, returning clamped integer.
unsafe fn FixupDigits(digits: SEXP, warn: warn_type) -> c_int {
    unsafe {
        let d = asInteger(digits);
        if d == NA_INTEGER || d < R_MIN_DIGITS_OPT || d > R_MAX_DIGITS_OPT {
            match warn {
                iWARN | iSILENT => return 7,
                iERROR => r_error("invalid printing digits"),
                _ => return 7,
            }
        }
        d
    }
}

/// FixupScipen: clamp scipen SEXP value, returning clamped integer.
#[allow(clippy::if_same_then_else)]
unsafe fn FixupScipen(scipen: SEXP, warn: warn_type) -> c_int {
    unsafe {
        if isNumeric(scipen) == 0 || LENGTH(scipen) != 1 {
            r_error("invalid 'scipen'");
        }
        let d;
        if TYPEOF(scipen) == SEXPTYPE::REALSXP {
            let x = *REAL(scipen);
            if !x.is_finite() || x.abs() > i32::MAX as f64 {
                r_error("invalid 'scipen'");
            }
            d = asInteger(scipen);
        } else {
            d = asInteger(scipen);
        }
        if d == NA_INTEGER || d < R_MIN_SCIPEN_OPT || d > R_MAX_SCIPEN_OPT {
            let dnew = if d == NA_INTEGER {
                0
            } else if d < R_MIN_SCIPEN_OPT {
                R_MIN_SCIPEN_OPT
            } else {
                R_MAX_SCIPEN_OPT
            };
            match warn {
                iWARN => {
                    let msg = std::ffi::CString::new(format!("invalid 'scipen' {d}, used {dnew}"))
                        .unwrap_or_default();
                    crate::mainutils::errors::Rf_warning(msg.as_ptr());
                    return dnew;
                }
                iSILENT => return dnew,
                iERROR => r_error("invalid 'scipen'"),
                _ => return dnew,
            }
        }
        d
    }
}

// ---------------------------------------------------------------------------
// Options lookup from the storage
// ---------------------------------------------------------------------------

/// Get the value of a single option by name (CString).
/// Returns the SEXP value or R_NilValue if not found.
unsafe fn GetOptionByName(name: &str) -> SEXP {
    unsafe {
        InitOptions();
        let nil = R_NilValue();

        crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst).options.get(name).map_or(nil, Sexp::as_raw)
        })
    }
}

/// Resolve an option tag symbol to its UTF-8 name.
///
/// Prefer symbol-table reverse lookup to avoid dereferencing a potentially
/// stale PRINTNAME pointer in long test runs with mixed global state.
unsafe fn option_name_from_tag(tag: SEXP) -> Option<String> {
    unsafe {
        if tag.is_null() {
            return None;
        }
        if let Some(name) = crate::sexp::symbol::symbol_name_from_ptr(tag) {
            return Some(name);
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return None;
        }
        let c_name = std::ffi::CStr::from_ptr(CHAR(pname));
        c_name.to_str().ok().map(|s| s.to_string())
    }
}

/// Get the value of a single option by tag (SEXP symbol).
/// This is the primary lookup used throughout R's C code (e.g. GetOption1(install("width"))).
pub unsafe fn GetOption1(tag: SEXP) -> SEXP {
    unsafe {
        if let Some(name) = option_name_from_tag(tag) {
            GetOptionByName(&name)
        } else {
            R_NilValue()
        }
    }
}

/// Get the value of an option by its string name.
/// Convenience wrapper used by C code that has the name as a C string.
/// Returns R_NilValue if not found.
pub unsafe fn GetOption(name: *const c_char) -> SEXP {
    unsafe {
        if name.is_null() {
            return R_NilValue();
        }
        let name_cstr = std::ffi::CStr::from_ptr(name);
        let name_str = match name_cstr.to_str() {
            Ok(s) => s,
            Err(_) => return R_NilValue(),
        };
        GetOptionByName(name_str)
    }
}

/// Read a logical option by name: true iff the option is set and its value
/// is `TRUE` — the `asLogical(GetOption1(install(name)))` probe used
/// throughout R's C code (e.g. the warnPartialMatch* option readers).
pub(crate) unsafe fn logical_option_enabled(name: &CStr) -> bool {
    unsafe {
        let s = GetOption1(Rf_install(name.as_ptr()));
        !s.is_null() && s != R_NilValue() && crate::mainutils::coerce::asLogical(s) == 1
    }
}

/// R_Options: get the options list as a pairlist.
/// Reconstructs from the HashMap for FFI compatibility.
/// The C code stores options as SYMVALUE(install(".Options")), a dotted-pair list.
pub unsafe fn R_Options() -> SEXP {
    let owner = require_options(unsafe { OwnerToken::current() });
    require_options(options_pairlist(&owner.node_factory())).as_raw()
}

fn options_pairlist<'s>(factory: &SessionNodeFactory<'s>) -> SexpResult<Sexp<'s>> {
    let entries = options_snapshot(factory)?;
    let mut result = PairlistBuilder::from_factory(factory.clone());
    for (name, value) in entries {
        let name = CString::new(name).unwrap_or_default();
        let tag = factory.wrap(unsafe { Rf_install(name.as_ptr()) })?;
        result.push(value, Some(tag))?;
    }
    result.finish()
}

/// Own the selected values before any R allocation or callback can replace them.
fn options_snapshot(factory: &SessionNodeFactory<'_>) -> SexpResult<Vec<(String, Sexp<'static>)>> {
    factory.require_active()?;
    let mut entries: Vec<(String, Sexp<'static>)> =
        crate::sexp::instance::with_required_current_instance(|owner| unsafe {
            (*owner)
                .options
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        });
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries)
}

fn options_vector<'s>(factory: &SessionNodeFactory<'s>) -> SexpResult<Sexp<'s>> {
    let entries = options_snapshot(factory)?;
    let length = R_xlen_t::try_from(entries.len()).map_err(|_| {
        crate::sexp::object::SexpError::AllocationFailed {
            object: "options list",
        }
    })?;
    let value = factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, length)))?;
    let mut value = SexpMut::try_from_checked(value)?;
    let keys: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
    let names = factory.strings(&keys)?;
    for (index, (_, entry)) in entries.iter().enumerate() {
        let copy = unsafe { duplicate_sexp(entry.as_raw()) };
        factory.require_active()?;
        value.try_set_vector_elt(index as R_xlen_t, factory.wrap(copy)?)?;
    }
    let value = value.freeze();
    set_option_names(factory, &value, names)?;
    factory.require_active()?;
    Ok(value)
}

/// Refresh the base `.Options` snapshot while its complete graph is owned.
unsafe fn refresh_options_binding() {
    let owner = require_options(unsafe { OwnerToken::current() });
    let factory = owner.node_factory();
    let options = require_options(options_pairlist(&factory));
    let symbol = require_options(factory.wrap(options_symbol()));
    require_options(factory.require_active());
    unsafe {
        defineVar(symbol.as_raw(), options.as_raw(), R_BaseEnv());
    }
}

/// Find a tagged item in the options (equivalent to C's FindTaggedItem).
/// Returns R_NilValue if not found.
pub unsafe fn FindTaggedItem(_lst: SEXP, tag: SEXP) -> SEXP {
    unsafe {
        let nil = R_NilValue();
        // In our implementation, we use the HashMap directly.
        // This function is kept for FFI compatibility.
        let name_str = match option_name_from_tag(tag) {
            Some(s) => s,
            None => return nil,
        };

        let owner = require_options(OwnerToken::current());
        let _pin = require_options(owner.pin());
        let factory = owner.node_factory();
        let value = (*owner.as_ptr()).options.get(name_str.as_str()).cloned();
        match value {
            Some(value) => {
                let tag = require_options(factory.wrap(tag));
                require_options(factory.pairlist_cell(&value, &factory.nil(), &tag)).as_raw()
            }
            None => nil,
        }
    }
}

/// Set an option by tag and value. Returns the old value.
unsafe fn SetOption(tag: SEXP, value: SEXP) -> SEXP {
    unsafe {
        let name_str = match option_name_from_tag(tag) {
            Some(s) => s,
            None => return R_NilValue(),
        };
        SetOptionByName(name_str.as_str(), value)
    }
}

/// Set or remove an option by plain string key. Returns old value.
pub unsafe fn SetOptionByName(name: &str, value: SEXP) -> SEXP {
    unsafe {
        let owner = require_options(OwnerToken::current());
        let _pin = require_options(owner.pin());
        let factory = owner.node_factory();
        // Capture the new value before initialization or binding refresh can collect.
        let value = require_options(factory.wrap(value).and_then(Sexp::into_owned));
        let nil = factory.nil();
        InitOptions();
        require_options(factory.require_active());
        let old = if value.as_raw() == nil.as_raw() {
            (*owner.as_ptr()).options.remove(name)
        } else {
            (*owner.as_ptr()).options.insert(name.to_string(), value)
        };
        refresh_options_binding();
        require_options(factory.require_active());
        old.map_or_else(|| nil.as_raw(), |value| value.as_raw())
    }
}

/// Set an option (FFI wrapper).
pub unsafe fn R_SetOption(tag: SEXP, value: SEXP) -> SEXP {
    unsafe { SetOption(tag, value) }
}

// ---------------------------------------------------------------------------
// C-level option accessors
// ---------------------------------------------------------------------------

/// Get the current printing width from options.
pub unsafe fn GetOptionWidth() -> c_int {
    unsafe {
        let width_sym = Rf_install(c"width".as_ptr());
        let val = GetOptionByName("width");
        if val == R_NilValue() {
            return 80;
        }
        FixupWidth(val, iWARN)
    }
}

/// Set the printing width option. Returns the previous value.
pub unsafe fn R_SetOptionWidth(w: c_int) -> c_int {
    unsafe {
        let mut w = w;
        if w < R_MIN_WIDTH_OPT {
            w = R_MIN_WIDTH_OPT;
        }
        if w > R_MAX_WIDTH_OPT {
            w = R_MAX_WIDTH_OPT;
        }
        let val = Rf_ScalarInteger(w);
        let _val_guard = protect(val);
        let old = SetOptionByName("width", val);
        let old_w = asInteger(old);
        if old_w == NA_INTEGER { 80 } else { old_w }
    }
}

/// Get the current printing digits from options.
pub unsafe fn GetOptionDigits() -> c_int {
    unsafe {
        let val = GetOptionByName("digits");
        if val == R_NilValue() {
            return 7;
        }
        FixupDigits(val, iWARN)
    }
}

/// Get the current scipen (significant digits penalty) option.
pub unsafe fn GetOptionScipen() -> c_int {
    unsafe {
        let val = GetOptionByName("scipen");
        if val == R_NilValue() {
            return 0;
        }
        let w = asInteger(val);
        if w == NA_INTEGER { 0 } else { w }
    }
}

/// Get the current max.print option (R_print.max).
pub unsafe fn GetOptionMaxPrint() -> c_int {
    unsafe {
        let val = GetOptionByName("max.print");
        if val == R_NilValue() {
            return 99999;
        }
        let w = asInteger(val);
        if w == NA_INTEGER { 99999 } else { w }
    }
}

/// Get the deparse.cutoff option.
pub unsafe fn GetOptionCutoff() -> c_int {
    unsafe {
        let val = GetOptionByName("deparse.cutoff");
        if val == R_NilValue() {
            return 60;
        }
        let w = asInteger(val);
        if w == NA_INTEGER || w <= 0 { 60 } else { w }
    }
}

/// Get the warn option value.
pub unsafe fn R_ShowWarningOption() -> c_int {
    unsafe {
        let val = GetOptionByName("warn");
        if val == R_NilValue() {
            return 0;
        }
        asInteger(val)
    }
}

/// Get the error option value.
pub unsafe fn R_ShowErrorOption() -> c_int {
    unsafe {
        let val = GetOptionByName("show.error.messages");
        if val == R_NilValue() {
            return 1;
        }
        asLogical(val)
    }
}

/// Set the warn option. Returns the previous value.
pub unsafe fn R_SetOptionWarn(w: c_int) -> c_int {
    unsafe {
        let val = Rf_ScalarInteger(w);
        let _val_guard = protect(val);
        let old = SetOptionByName("warn", val);
        let old_w = asInteger(old);
        if old_w == NA_INTEGER { 0 } else { old_w }
    }
}

/// Get the device.ask.default option as a boolean.
pub unsafe fn Rf_GetOptionDeviceAsk() -> Rboolean {
    unsafe {
        let val = GetOptionByName("device.ask.default");
        if val == R_NilValue() {
            return FALSE;
        }
        let ask = asLogical(val);
        if ask == NA_LOGICAL {
            return FALSE;
        }
        if ask != 0 { TRUE } else { FALSE }
    }
}

// ---------------------------------------------------------------------------
// Initialize default options
// ---------------------------------------------------------------------------

/// Build each scalar completely inside its allocation lend and root it before
/// deferred GC. The initializer receives only the fresh scalar capability.
fn option_scalar<'s>(
    factory: &SessionNodeFactory<'s>,
    kind: SEXPTYPE,
    initialize: impl FnOnce(&mut SexpMut<'_>) -> SexpResult<()>,
) -> SexpResult<Sexp<'s>> {
    factory.allocate(|arena| {
        let value = arena.alloc_vector_sexp(kind, 1)?;
        let mut value = SexpMut::try_from_checked(value).ok()?;
        initialize(&mut value).ok()?;
        Some(value.freeze().as_raw())
    })
}

fn set_option_names<'s>(
    factory: &SessionNodeFactory<'s>,
    value: &Sexp<'s>,
    names: Sexp<'s>,
) -> SexpResult<()> {
    let tag = factory.wrap(unsafe { R_NamesSymbol() })?;
    let mut attributes = PairlistBuilder::from_factory(factory.clone());
    attributes.push(names, Some(tag))?;
    let attributes = attributes.finish()?;
    factory.require_active()?;
    unsafe {
        SET_ATTRIB(value.as_raw(), attributes.as_raw());
    }
    Ok(())
}

/// Detached owning defaults stay alive across every allocation and callback.
fn populate_options<'s>(factory: &SessionNodeFactory<'s>) -> SexpResult<HashMap<String, Sexp<'s>>> {
    let mut options = HashMap::new();
    let pi = |value| {
        option_scalar(factory, SEXPTYPE::INTSXP, |scalar| {
            scalar.try_set_integer_elt(0, value)
        })
    };
    let pl = |value| {
        option_scalar(factory, SEXPTYPE::LGLSXP, |scalar| {
            scalar.try_set_logical_elt(0, value)
        })
    };
    let pm = |text: &str| factory.strings(&[text]);
    let val = pm("> ")?;
    options.insert("prompt".to_string(), val);

    let val = pm("+ ")?;
    options.insert("continue".to_string(), val);

    options.insert("expressions".to_string(), pi(5000)?);
    options.insert("width".to_string(), pi(80)?);
    options.insert("deparse.cutoff".to_string(), pi(60)?);
    options.insert("digits".to_string(), pi(7)?);
    options.insert("na.action".to_string(), pm("na.omit")?);
    options.insert("device".to_string(), pm("pdf")?);
    options.insert("show.coef.Pvalues".to_string(), pl(TRUE)?);
    options.insert("show.signif.stars".to_string(), pl(TRUE)?);
    options.insert("echo".to_string(), pl(TRUE)?);
    options.insert("quiet".to_string(), pl(FALSE)?);
    options.insert("verbose".to_string(), pl(FALSE)?);
    options.insert("check.bounds".to_string(), pl(FALSE)?);
    options.insert("keep.source".to_string(), pl(FALSE)?);
    options.insert("keep.source.pkgs".to_string(), pl(FALSE)?);
    options.insert("keep.parse.data".to_string(), pl(TRUE)?);
    options.insert("keep.parse.data.pkgs".to_string(), pl(FALSE)?);
    options.insert("example.ask".to_string(), pm("default")?);
    options.insert("demo.ask".to_string(), pm("default")?);

    options.insert("warning.length".to_string(), pi(1000)?);
    options.insert("nwarnings".to_string(), pi(50)?);

    let val = pm(".")?;
    options.insert("OutDec".to_string(), val);

    options.insert("CBoundsCheck".to_string(), pl(FALSE)?);

    let val = pm("default")?;
    options.insert("matprod".to_string(), val);

    options.insert("PCRE_study".to_string(), pl(TRUE)?);
    options.insert("PCRE_use_JIT".to_string(), pl(TRUE)?);
    options.insert("PCRE_limit_recursion".to_string(), pl(NA_LOGICAL)?);
    options.insert("max.contour.segments".to_string(), pi(25000)?);
    options.insert("warnPartialMatchDollar".to_string(), pl(FALSE)?);
    options.insert("warnPartialMatchArgs".to_string(), pl(FALSE)?);
    options.insert("warnPartialMatchAttr".to_string(), pl(FALSE)?);
    options.insert("showWarnCalls".to_string(), pl(FALSE)?);
    options.insert("showErrorCalls".to_string(), pl(FALSE)?);
    options.insert("showNCalls".to_string(), pi(50)?);
    options.insert("browserNLdisabled".to_string(), pl(FALSE)?);
    options.insert("warn".to_string(), pi(0)?);
    options.insert("max.print".to_string(), pi(99999)?);
    options.insert("show.error.messages".to_string(), pl(TRUE)?);
    options.insert("scipen".to_string(), pi(0)?);
    options.insert("height".to_string(), pi(60)?);
    options.insert("add.smooth".to_string(), pl(TRUE)?);
    options.insert(
        "ts.eps".to_string(),
        option_scalar(factory, SEXPTYPE::REALSXP, |value| {
            value.try_set_real_elt(0, 1e-5)
        })?,
    );
    let contrasts = factory.strings(&["contr.treatment", "contr.poly"])?;
    let cnames = factory.strings(&["unordered", "ordered"])?;
    set_option_names(factory, &contrasts, cnames)?;
    options.insert("contrasts".to_string(), contrasts);
    options.insert("pkgType".to_string(), pm("source")?);
    Ok(options)
}

/// Initialize the default options list.
pub unsafe fn InitOptions() {
    let owner = require_options(unsafe { OwnerToken::current() });
    let Some(_initialization) = (unsafe { OptionsInitializationGuard::begin(owner.as_ptr()) })
    else {
        return;
    };
    let factory = owner.node_factory();
    let defaults = require_options(populate_options(&factory));
    require_options(factory.require_active());
    // Move defaults into owning storage before either binding publication.
    // Reentrant mutations made while defaults were built take precedence.
    unsafe {
        let options = &mut (*owner.as_ptr()).options;
        for (name, value) in defaults {
            options
                .entry(name)
                .or_insert(require_options(value.into_owned()));
        }
    }
    unsafe {
        refresh_options_binding();
    }
    require_options(define_platform_binding(&factory));
    require_options(factory.require_active());
    unsafe {
        (*owner.as_ptr()).options_initialization = OptionsInitialization::Initialized;
    }
}

/// Build the base `.Platform` list without borrowing any interpreter field.
fn define_platform_binding(factory: &SessionNodeFactory<'_>) -> SexpResult<()> {
    let fields: [(&str, &str); 9] = [
        ("OS.type", "unix"),
        ("file.sep", "/"),
        ("dynlib.ext", ".so"),
        ("GUI", "unknown"),
        (
            "endian",
            if cfg!(target_endian = "little") {
                "little"
            } else {
                "big"
            },
        ),
        ("type", "unix"),
        ("pkgType", "source"),
        ("path.sep", ":"),
        ("r_arch", ""),
    ];
    let platform =
        factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, fields.len() as _)))?;
    let mut platform = SexpMut::try_from_checked(platform)?;
    for (index, (_, text)) in fields.iter().enumerate() {
        let value = factory.strings(&[text])?;
        platform.try_set_vector_elt(index as _, value)?;
    }
    let platform = platform.freeze();
    let keys: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
    let names = factory.strings(&keys)?;
    set_option_names(factory, &platform, names)?;
    let symbol = factory.wrap(unsafe { Rf_install(c".Platform".as_ptr()) })?;
    factory.require_active()?;
    unsafe {
        defineVar(symbol.as_raw(), platform.as_raw(), R_BaseEnv());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helper for validating TRUE/FALSE logical values
// ---------------------------------------------------------------------------

unsafe fn check_TRUE_FALSE(arg: SEXP, chname: *const c_char) {
    unsafe {
        let mut name_buf = [0u8; 256];
        if !chname.is_null() {
            let src = std::ffi::CStr::from_ptr(chname);
            let bytes = src.to_bytes();
            let len = bytes.len().min(255);
            name_buf[..len].copy_from_slice(&bytes[..len]);
            let name_str = std::str::from_utf8(&name_buf[..len]).unwrap_or("?");
            r_error(&format!("invalid value for '{}'", name_str));
        }
    }
}

// ---------------------------------------------------------------------------
// do_getOption -- C-level entry for getOption(name)
// ---------------------------------------------------------------------------

pub unsafe fn do_getOption(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Trunk dispatches getOption through the closure
        // function(x, default = NULL), so the internal always receives one
        // argument plus an optional default. The flattened builtin accepts
        // both shapes; anything else mirrors trunk's missing-argument error.
        let nargs = Rf_length(args);
        if nargs < 1 {
            r_error("argument \"x\" is missing, with no default");
        }
        if nargs > 2 {
            let noun = if nargs == 1 { "argument" } else { "arguments" };
            r_error(&format!(
                "{nargs} {noun} passed to .Internal(getOption) which requires 1"
            ));
        }
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::STRSXP || LENGTH(x) != 1 {
            r_error("'x' must be a character string");
        }
        let name_charsxp = STRING_ELT(x, 0);
        let tag = installTrChar(name_charsxp);
        let val = GetOptionByName(
            std::ffi::CStr::from_ptr(CHAR(name_charsxp))
                .to_str()
                .unwrap_or(""),
        );
        if val == R_NilValue() {
            let default_cell = CDR(args);
            if default_cell.is_null() || default_cell == R_NilValue() {
                R_NilValue()
            } else {
                duplicate_sexp(CAR(default_cell))
            }
        } else {
            duplicate_sexp(val)
        }
    }
}

// ---------------------------------------------------------------------------
// do_options -- C-level entry for options(...)
// ---------------------------------------------------------------------------

pub unsafe fn do_options(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        InitOptions();
        // GNU `options` is a closure; warning() from asInteger therefore
        // stores `options(...)` as the call. The port's primitive has no
        // function context, so pin the LANGSXP for CoercionWarning.
        crate::main::coerce::set_coercion_warning_call(call);
        let _coercion_call = crate::main::coerce::CoercionWarningCallGuard;

        // Zero-argument case: return all options sorted alphabetically
        if args == R_NilValue() {
            let owner = require_options(OwnerToken::current());
            let _pin = require_options(owner.pin());
            let value = require_options(options_vector(&owner.node_factory()));
            set_R_Visible(TRUE);
            return value.as_raw();
        }

        // The arguments to "options" can either be a sequence of
        // name = value form, or can be a single list.
        let mut n = length(args);
        let mut args = args;

        let single_arg_tag = if n == 1 { TAG(args) } else { R_NilValue() };
        let single = if n == 1 { CAR(args) } else { R_NilValue() };
        if n == 1
            && (single == R_NilValue() || isPairList(single) != 0 || isVectorList(single) != 0)
            && (single_arg_tag.is_null() || single_arg_tag == R_NilValue())
        {
            // options(NULL) and options(list()) set nothing.
            args = single;
            n = if single == R_NilValue() {
                0
            } else {
                length(args)
            };
        }

        let value = Rf_allocVector(SEXPTYPE::VECSXP, n);
        let _value_guard = protect(value);
        let names = Rf_allocVector(SEXPTYPE::STRSXP, n);
        let _names_guard = protect(names);

        // Get argnames for VECSXP args
        let mut argnames: SEXP = R_NilValue();
        match TYPEOF(args) {
            t if t == SEXPTYPE::NILSXP || t == SEXPTYPE::LISTSXP => {}
            t if t == SEXPTYPE::VECSXP => {
                if n > 0 {
                    argnames = getAttrib(args, R_NamesSymbol());
                    if LENGTH(argnames) != n {
                        r_error("list argument has no valid names");
                    }
                }
            }
            _ => {
                r_error("invalid argument type for options");
            }
        }
        let _argnames_guard = protect(argnames);

        let mut visible: c_int = FALSE;

        for i in 0..n as c_int {
            let mut argi: SEXP = R_NilValue();
            let mut namei: SEXP = R_NilValue();

            match TYPEOF(args) {
                t if t == SEXPTYPE::LISTSXP => {
                    argi = CAR(args);
                    namei = EnsureString(TAG(args));
                    args = CDR(args);
                }
                t if t == SEXPTYPE::VECSXP => {
                    argi = VECTOR_ELT(args, i as R_xlen_t);
                    if !argnames.is_null() && LENGTH(argnames) > i {
                        namei = STRING_ELT(argnames, i as R_xlen_t);
                    }
                }
                _ => {} // intentionally unhandled: unknown option type
            }

            // Check if this is a name=value assignment or a query
            let is_assignment = if !namei.is_null() {
                !char_is_empty(CHAR(namei))
            } else {
                false
            };

            if is_assignment {
                // name = value assignment
                let tag = installTrChar(namei);
                SET_STRING_ELT(names, i as R_xlen_t, namei);

                let name_cstr = CHAR(namei);

                if argi == R_NilValue() {
                    // Option removal
                    let mandatory = [
                        "prompt",
                        "continue",
                        "expressions",
                        "width",
                        "deparse.cutoff",
                        "digits",
                        "echo",
                        "quiet",
                        "verbose",
                        "check.bounds",
                        "keep.source",
                        "keep.source.pkgs",
                        "keep.parse.data",
                        "keep.parse.data.pkgs",
                        "warning.length",
                        "nwarnings",
                        "OutDec",
                        "CBoundsCheck",
                        "matprod",
                        "PCRE_study",
                        "PCRE_use_JIT",
                        "PCRE_limit_recursion",
                        "max.contour.segments",
                        "warnPartialMatchDollar",
                        "warnPartialMatchArgs",
                        "warnPartialMatchAttr",
                        "showWarnCalls",
                        "showErrorCalls",
                        "showNCalls",
                        "browserNLdisabled",
                        "warn",
                        "max.print",
                        "show.error.messages",
                        "scipen",
                    ];
                    let name_str = std::ffi::CStr::from_ptr(name_cstr).to_str().unwrap_or("");
                    for &m in &mandatory {
                        if name_str == m {
                            r_error(&format!("option '{}' cannot be deleted", name_str));
                        }
                    }
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, R_NilValue()));
                } else if streql(name_cstr, c"width".as_ptr()) {
                    let k = asInteger(argi);
                    if k < R_MIN_WIDTH_OPT || k > R_MAX_WIDTH_OPT {
                        r_error(&format!(
                            "invalid 'width' parameter, allowed {}...{}",
                            R_MIN_WIDTH_OPT, R_MAX_WIDTH_OPT
                        ));
                    }
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"deparse.cutoff".as_ptr()) {
                    let k = asInteger(argi);
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"digits".as_ptr()) {
                    let k = asInteger(argi);
                    if k < R_MIN_DIGITS_OPT || k > R_MAX_DIGITS_OPT {
                        r_error(&format!(
                            "invalid 'digits' parameter, allowed {}...{}",
                            R_MIN_DIGITS_OPT, R_MAX_DIGITS_OPT
                        ));
                    }
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"expressions".as_ptr()) {
                    let k = asInteger(argi);
                    if k < R_MIN_EXPRESSIONS_OPT || k > R_MAX_EXPRESSIONS_OPT {
                        r_error(&format!(
                            "invalid 'expressions' parameter, allowed {}...{}",
                            R_MIN_EXPRESSIONS_OPT, R_MAX_EXPRESSIONS_OPT
                        ));
                    }
                    // Update the R_Expressions global (used by error handling)
                    crate::mainutils::errors::R_SetExpressions(k);
                    crate::mainutils::errors::R_SetExpressionsKeep(k);
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"keep.source".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'keep.source'");
                    }
                    let k = asLogical(argi);
                    if k == NA_LOGICAL {
                        r_error("invalid value for 'keep.source'");
                    }
                    let v = Rf_ScalarLogical(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"continue".as_ptr()) {
                    let s = asChar(argi);
                    if s.is_null() {
                        r_error("invalid value for 'continue'");
                    }
                    let new_val = Rf_mkString(translateChar(s));
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"prompt".as_ptr()) {
                    let s = asChar(argi);
                    if s.is_null() {
                        r_error("invalid value for 'prompt'");
                    }
                    let new_val = Rf_mkString(translateChar(s));
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"contrasts".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::STRSXP || LENGTH(argi) != 2 {
                        r_error("invalid value for 'contrasts'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"check.bounds".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'check.bounds'");
                    }
                    let k = asLogical(argi);
                    let v = Rf_ScalarLogical(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"warn".as_ptr()) {
                    if isNumeric(argi) == 0 || LENGTH(argi) != 1 {
                        r_error("invalid value for 'warn'");
                    }
                    let k = asInteger(argi);
                    if k == NA_INTEGER {
                        r_error("invalid value for 'warn'");
                    }
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOptionByName("warn", v));
                } else if streql(name_cstr, c"warning.length".as_ptr()) {
                    let k = asInteger(argi);
                    if k < 100 || k > 8170 {
                        r_error("invalid value for 'warning.length'");
                    }
                    crate::mainutils::errors::R_SetWarnLength(k);
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"warning.expression".as_ptr()) {
                    if isLanguage(argi) == 0 && isExpression(argi) == 0 {
                        r_error("invalid value for 'warning.expression'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"max.print".as_ptr()) {
                    let k = crate::mainutils::coerce::asInteger(argi);
                    if k < 1 {
                        r_error("invalid value for 'max.print'");
                    }

                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"scipen".as_ptr()) {
                    let k = FixupScipen(argi, iWARN);
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"nwarnings".as_ptr()) {
                    let k = asInteger(argi);
                    if k < 1 {
                        r_error("invalid value for 'nwarnings'");
                    }
                    crate::mainutils::main::R_SetCollectWarnings(0); // force a reset
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"error".as_ptr()) {
                    if isFunction(argi) != 0 {
                        // Wrap in a call: makeErrorCall
                        let error_call = allocLang(1);
                        let _guard = protect(error_call);
                        SETCAR(error_call, argi);
                        SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, error_call));
                    } else if isLanguage(argi) != 0 || isExpression(argi) != 0 {
                        let new_val = duplicate_sexp(argi);
                        let _guard = protect(new_val);
                        SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                    } else {
                        r_error("invalid value for 'error'");
                    }
                } else if streql(name_cstr, c"show.error.messages".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'show.error.messages'");
                    }
                    crate::mainutils::errors::R_SetShowErrorMessages(*LOGICAL(argi).add(0) != 0);
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"catch.script.errors".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'catch.script.errors'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"echo".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'echo'");
                    }
                    let k = asLogical(argi);
                    let v = Rf_ScalarLogical(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"OutDec".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::STRSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'OutDec'");
                    }
                    let ch = STRING_ELT(argi, 0);
                    let nchars = if ch.is_null() {
                        0
                    } else {
                        std::ffi::CStr::from_ptr(CHAR(ch)).to_bytes().len()
                    };
                    if nchars != 1 {
                        let msg =
                            std::ffi::CString::new("'OutDec' must be a string of one character")
                                .unwrap_or_default();
                        crate::mainutils::errors::Rf_warning(msg.as_ptr());
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"max.contour.segments".as_ptr()) {
                    let k = asInteger(argi);
                    if k < 0 {
                        r_error("invalid value for 'max.contour.segments'");
                    }
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"warnPartialMatchDollar".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'warnPartialMatchDollar'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"warnPartialMatchArgs".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'warnPartialMatchArgs'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"warnPartialMatchAttr".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'warnPartialMatchAttr'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"showWarnCalls".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'showWarnCalls'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"showErrorCalls".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'showErrorCalls'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"showNCalls".as_ptr()) {
                    let k = asInteger(argi);
                    if k < 30 || k > 500 || k == NA_INTEGER || LENGTH(argi) != 1 {
                        r_error("invalid value for 'showNCalls'");
                    }
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"browserNLdisabled".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'browserNLdisabled'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"CBoundsCheck".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'CBoundsCheck'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"quiet".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'quiet'");
                    }
                    let k = asLogical(argi);
                    let v = Rf_ScalarLogical(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"verbose".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'verbose'");
                    }
                    let k = asLogical(argi);
                    let v = Rf_ScalarLogical(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"matprod".as_ptr()) {
                    let s = asChar(argi);
                    if s.is_null() {
                        r_error("invalid value for 'matprod'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"PCRE_study".as_ptr()) {
                    if TYPEOF(argi) == SEXPTYPE::LGLSXP {
                        let k = asLogical(argi);
                        let v = Rf_ScalarLogical(k);
                        let _guard = protect(v);
                        SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                    } else {
                        let k = asInteger(argi);
                        let v = Rf_ScalarInteger(k);
                        let _guard = protect(v);
                        SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                    }
                } else if streql(name_cstr, c"PCRE_use_JIT".as_ptr()) {
                    let use_jit = asLogical(argi);
                    let v = Rf_ScalarLogical(use_jit);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"PCRE_limit_recursion".as_ptr()) {
                    let v = Rf_ScalarLogical(asLogical(argi));
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"stringsAsFactors".as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'stringsAsFactors'");
                    }
                    let k = asLogical(argi);
                    let v = Rf_ScalarLogical(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, c"editor".as_ptr()) {
                    let s = asChar(argi);
                    if s.is_null() {
                        r_error("invalid value for 'editor'");
                    }
                    let new_val = Rf_mkString(translateChar(s));
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, c"par.ask.default".as_ptr()) {
                    r_error("\"par.ask.default\" has been replaced by \"device.ask.default\"");
                } else {
                    // Generic: accept any value
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                }
            } else {
                // Querying: get the value of the named option
                if !argi.is_null() && TYPEOF(argi) == SEXPTYPE::STRSXP && LENGTH(argi) > 0 {
                    let name_charsxp = STRING_ELT(argi, 0);
                    let name_str = std::ffi::CStr::from_ptr(CHAR(name_charsxp))
                        .to_str()
                        .unwrap_or("");
                    if name_str == "par.ask.default" {
                        r_error("\"par.ask.default\" has been replaced by \"device.ask.default\"");
                    }
                    let val = GetOptionByName(name_str);
                    SET_VECTOR_ELT(value, i as R_xlen_t, duplicate_sexp(val));
                    SET_STRING_ELT(names, i as R_xlen_t, name_charsxp);
                    visible = TRUE;
                } else {
                    r_error("invalid argument");
                }
            }
        }

        setAttrib(value, R_NamesSymbol(), names);
        set_R_Visible(visible);
        value
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::protect::{R_ProtectCount, protect_n};

    #[test]
    fn owned_options_vector_keeps_selected_values_after_callback_clears_storage() {
        use std::{cell::Cell, rc::Rc};
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            (*instance).options_initialization = OptionsInitialization::Initialized;
            let last = factory.strings(&["last"]).unwrap().into_owned().unwrap();
            let first = factory.strings(&["first"]).unwrap().into_owned().unwrap();
            (*instance).options.insert("zeta".into(), last);
            (*instance).options.insert("alpha".into(), first);
            let cleared = Rc::new(Cell::new(false));
            let observed = cleared.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !(*instance).options.is_empty() {
                    (*instance).options.clear();
                    observed.set(true);
                }
                crate::sexp::gengc::full_gc_in(instance);
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = options_vector(&factory).unwrap();
            assert!(cleared.get());
            let names = result.try_attrib().unwrap().try_car().unwrap();
            assert_eq!(
                names.try_string_value_elt(0).unwrap().as_deref(),
                Some("alpha")
            );
            assert_eq!(
                names.try_string_value_elt(1).unwrap().as_deref(),
                Some("zeta")
            );
            assert_eq!(
                result
                    .try_vector_elt(0)
                    .unwrap()
                    .try_string_value_elt(0)
                    .unwrap()
                    .as_deref(),
                Some("first")
            );
            assert_eq!(
                result
                    .try_vector_elt(1)
                    .unwrap()
                    .try_string_value_elt(0)
                    .unwrap()
                    .as_deref(),
                Some("last")
            );
        });
    }

    #[test]
    fn owned_options_replacement_keeps_old_value_through_collecting_refresh() {
        use std::{cell::Cell, rc::Rc};
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            (*instance).options_initialization = OptionsInitialization::Initialized;
            let old = factory
                .strings(&["previous"])
                .unwrap()
                .into_owned()
                .unwrap();
            let old_node = old.allocation().unwrap().clone();
            (*instance).options.insert("owned_option".into(), old);
            let replacement = factory.strings(&["replacement"]).unwrap();
            let collected = Rc::new(Cell::new(false));
            let observed = collected.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                (*instance).options.clear();
                crate::sexp::gengc::full_gc_in(instance);
                observed.set(true);
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let previous = factory
                .wrap(SetOptionByName("owned_option", replacement.as_raw()))
                .unwrap();
            assert!(collected.get());
            assert_eq!(
                previous.try_string_value_elt(0).unwrap().as_deref(),
                Some("previous")
            );
            drop(previous);
            crate::sexp::gengc::full_gc_in(instance);
            assert!(
                !old_node.is_live(),
                "replacement must release the old option root"
            );
        });
    }

    #[test]
    fn owned_options_drop_removes_root_without_permanent_preservation() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            (*instance).options_initialization = OptionsInitialization::Initialized;
            let value = factory.strings(&["released option"]).unwrap();
            let node = value.allocation().unwrap().clone();
            let preserved = (*instance).preserve_stack.checked_entries_snapshot().len();
            SetOptionByName("owned_removed_option", value.as_raw());
            drop(value);
            crate::sexp::gengc::full_gc_in(instance);
            assert!(node.is_live());
            let old = factory
                .wrap(SetOptionByName(
                    "owned_removed_option",
                    factory.nil().as_raw(),
                ))
                .unwrap();
            assert_eq!(
                old.try_string_value_elt(0).unwrap().as_deref(),
                Some("released option")
            );
            drop(old);
            crate::sexp::gengc::full_gc_in(instance);
            assert!(!node.is_live());
            assert_eq!(
                (*instance).preserve_stack.checked_entries_snapshot().len(),
                preserved
            );
        });
    }

    #[test]
    fn options_initialization_retains_defaults_through_reentrant_gc() {
        use std::{cell::Cell, rc::Rc};
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let calls = Rc::new(Cell::new(0));
        session.with_active(|| unsafe {
            let observed = calls.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                let previous = observed.get();
                observed.set(previous + 1);
                InitOptions();
                if previous == 0 {
                    // A genuine nested mutation must survive default publication.
                    R_SetOptionWidth(123);
                }
                crate::sexp::gengc::full_gc();
            }));
            crate::mainutils::memory_main::R_gc_torture(1, 1, 0);
            InitOptions();
            crate::mainutils::memory_main::R_gc_torture(0, 0, 0);
            assert!(calls.get() >= 45, "each default allocation must notify");
            assert_eq!(GetOptionWidth(), 123);
            assert_eq!(GetOptionDigits(), 7);
            assert_eq!(GetOptionCutoff(), 60);
            assert_eq!(R_ShowWarningOption(), 0);
            assert_eq!(asInteger(GetOptionByName("expressions")), 5000);
            assert_eq!(asInteger(GetOptionByName("max.print")), 99999);
            let owner = session.owner_token().unwrap();
            assert_eq!((*owner.as_ptr()).options.len(), 45);
            assert_eq!(
                (*owner.as_ptr()).options_initialization,
                OptionsInitialization::Initialized
            );

            let prompt = session.sexp(GetOptionByName("prompt")).unwrap();
            assert_eq!(
                prompt.try_string_value_elt(0).unwrap().as_deref(),
                Some("> ")
            );
            let contrasts = session.sexp(GetOptionByName("contrasts")).unwrap();
            assert_eq!(
                contrasts.try_string_value_elt(0).unwrap().as_deref(),
                Some("contr.treatment")
            );
            assert_eq!(
                contrasts.try_string_value_elt(1).unwrap().as_deref(),
                Some("contr.poly")
            );
            let names = session
                .sexp(getAttrib(contrasts.as_raw(), R_NamesSymbol()))
                .unwrap();
            assert_eq!(
                names.try_string_value_elt(0).unwrap().as_deref(),
                Some("unordered")
            );
            assert_eq!(
                names.try_string_value_elt(1).unwrap().as_deref(),
                Some("ordered")
            );

            let binding = crate::sexp::envir::R_findVarInFrame(R_BaseEnv(), options_symbol());
            assert_eq!(Rf_length(binding), 45);
            let platform = session
                .sexp(crate::sexp::envir::R_findVarInFrame(
                    R_BaseEnv(),
                    Rf_install(c".Platform".as_ptr()),
                ))
                .unwrap();
            assert_eq!(platform.len(), 9);
            assert_eq!(
                platform
                    .try_vector_elt(0)
                    .unwrap()
                    .try_string_value_elt(0)
                    .unwrap()
                    .as_deref(),
                Some("unix")
            );
            assert_eq!(
                platform
                    .try_vector_elt(3)
                    .unwrap()
                    .try_string_value_elt(0)
                    .unwrap()
                    .as_deref(),
                Some("unknown")
            );
            crate::sexp::gengc::full_gc();
            assert_eq!(GetOptionWidth(), 123);
            assert!(contrasts.is_live());
            assert!(platform.is_live());
        });
    }

    #[test]
    fn options_initialization_can_retry_after_callback_unwind() {
        use std::{cell::Cell, rc::Rc};
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let once = Rc::new(Cell::new(true));
        session.with_active(|| unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if once.replace(false) {
                    panic!("injected options initialization callback failure");
                }
            }));
            crate::mainutils::memory_main::R_gc_torture(1, 1, 0);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| InitOptions()));
            crate::mainutils::memory_main::R_gc_torture(0, 0, 0);
            assert!(result.is_err());
            let owner = session.owner_token().unwrap();
            assert_eq!(
                (*owner.as_ptr()).options_initialization,
                OptionsInitialization::Uninitialized
            );
            InitOptions();
            assert_eq!(
                (*owner.as_ptr()).options_initialization,
                OptionsInitialization::Initialized
            );
            assert_eq!(GetOptionWidth(), 80);
            assert_eq!(GetOptionDigits(), 7);
        });
    }

    fn reset_protect_stack() {
        crate::sexp::instance::with_current_instance(|_| {
            let n = R_ProtectCount();
            if n > 0 {
                drop(protect_n(n));
            }
        });
    }

    struct ProtectStackGuard;

    impl ProtectStackGuard {
        fn new() -> Self {
            reset_protect_stack();
            Self
        }
    }

    impl Drop for ProtectStackGuard {
        fn drop(&mut self) {
            reset_protect_stack();
        }
    }

    struct TestInstance {
        _session: crate::sexp::session::RSession,
    }

    impl TestInstance {
        fn new() -> Self {
            TestInstance {
                _session: crate::sexp::session::RSession::new_for_gc_tests(),
            }
        }
    }

    #[test]
    fn test_fixup_width() {
        let _guard = ProtectStackGuard::new();
        unsafe {
            assert_eq!(fixup_width(80), 80);
            assert_eq!(fixup_width(10), 10);
            assert_eq!(fixup_width(10000), 10000);
            assert_eq!(fixup_width(9), 80);
            assert_eq!(fixup_width(10001), 80);
            assert_eq!(fixup_width(c_int::MIN), 80);
        }
    }

    #[test]
    fn test_fixup_digits() {
        let _guard = ProtectStackGuard::new();
        unsafe {
            assert_eq!(fixup_digits(7), 7);
            assert_eq!(fixup_digits(1), 1);
            assert_eq!(fixup_digits(22), 22);
            assert_eq!(fixup_digits(0), 7);
            assert_eq!(fixup_digits(23), 7);
            assert_eq!(fixup_digits(c_int::MIN), 7);
        }
    }

    #[test]
    fn test_fixup_scipen() {
        let _guard = ProtectStackGuard::new();
        unsafe {
            // GNU R FixupScipen preserves values in -9..9999 and maps NA to 0:
            // https://svn.r-project.org/R/trunk/src/main/options.c
            assert_eq!(fixup_scipen(-10), -9);
            assert_eq!(fixup_scipen(-9), -9);
            assert_eq!(fixup_scipen(-1), -1);
            assert_eq!(fixup_scipen(0), 0);
            assert_eq!(fixup_scipen(50), 50);
            assert_eq!(fixup_scipen(51), 51);
            assert_eq!(fixup_scipen(9999), 9999);
            assert_eq!(fixup_scipen(10000), 9999);
            assert_eq!(fixup_scipen(NA_INTEGER), 0);
        }
    }

    #[test]
    fn test_fixup_deparse_cutoff() {
        let _guard = ProtectStackGuard::new();
        unsafe {
            assert_eq!(fixup_deparse_cutoff(60), 60);
            assert_eq!(fixup_deparse_cutoff(100), 100);
            assert_eq!(fixup_deparse_cutoff(0), 60);
            assert_eq!(fixup_deparse_cutoff(-1), 60);
            assert_eq!(fixup_deparse_cutoff(c_int::MIN), 60);
        }
    }

    #[test]
    fn test_init_options() {
        let _guard = ProtectStackGuard::new();
        let _inst = TestInstance::new();
        unsafe {
            InitOptions();
            assert_eq!(GetOptionWidth(), 80);
            assert_eq!(GetOptionDigits(), 7);
            assert_eq!(GetOptionCutoff(), 60);
            assert_eq!(R_ShowWarningOption(), 0);
            let prompt_opt = GetOptionByName("prompt");
            assert!(!prompt_opt.is_null());
            let cont_opt = GetOptionByName("continue");
            assert!(!cont_opt.is_null());
            assert_eq!(GetOptionByName("expressions") == R_NilValue(), false);
            assert_eq!(GetOptionByName("warn") == R_NilValue(), false);
            assert_eq!(GetOptionByName("max.print") == R_NilValue(), false);
            assert_eq!(GetOptionByName("scipen") == R_NilValue(), false);
            assert_eq!(GetOptionByName("echo") == R_NilValue(), false);
            assert_eq!(GetOptionByName("verbose") == R_NilValue(), false);
            assert_eq!(GetOptionByName("height") == R_NilValue(), false);
            assert_eq!(GetOptionByName("OutDec") == R_NilValue(), false);
            assert_eq!(GetOptionByName("add.smooth") == R_NilValue(), false);
        }
    }

    #[test]
    fn test_get_option_width_default() {
        let _guard = ProtectStackGuard::new();
        let _inst = TestInstance::new();
        unsafe {
            InitOptions();
            assert_eq!(GetOptionWidth(), 80);
        }
    }

    #[test]
    fn test_get_option_digits_default() {
        let _guard = ProtectStackGuard::new();
        let _inst = TestInstance::new();
        unsafe {
            InitOptions();
            assert_eq!(GetOptionDigits(), 7);
        }
    }

    #[test]
    fn test_get_option_cutoff_default() {
        let _guard = ProtectStackGuard::new();
        let _inst = TestInstance::new();
        unsafe {
            InitOptions();
            assert_eq!(GetOptionCutoff(), 60);
        }
    }

    #[test]
    fn test_set_option_width() {
        let _guard = ProtectStackGuard::new();
        let _inst = TestInstance::new();
        unsafe {
            InitOptions();
            let old = R_SetOptionWidth(123);
            assert!(old >= R_MIN_WIDTH_OPT && old <= R_MAX_WIDTH_OPT);
            assert_eq!(GetOptionWidth(), 123);
        }
    }

    #[test]
    fn test_options_are_session_local_on_same_thread() {
        let _guard = ProtectStackGuard::new();
        let left = crate::sexp::session::RSession::new_without_default_packages();
        let right = crate::sexp::session::RSession::new_without_default_packages();

        left.with_active(|| unsafe {
            InitOptions();
            R_SetOptionWidth(111);
            assert_eq!(GetOptionWidth(), 111);
        });

        right.with_active(|| unsafe {
            InitOptions();
            assert_eq!(GetOptionWidth(), 80);
            R_SetOptionWidth(222);
            assert_eq!(GetOptionWidth(), 222);
        });

        left.with_active(|| unsafe {
            assert_eq!(GetOptionWidth(), 111);
        });
        right.with_active(|| unsafe {
            assert_eq!(GetOptionWidth(), 222);
        });
    }

    #[test]
    fn test_options_accepts_unnamed_list_restore() {
        let mut session = crate::sexp::session::RSession::new_without_default_packages();
        let (result, output, _) = session.eval_code_with_output_capture(
            "old <- options(keep.parse.data = FALSE); \
             options(old); \
             identical(getOption('keep.parse.data'), TRUE)",
        );

        let result = result.expect("an unnamed options list should restore its values");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(output.stdout.is_empty());
    }

    #[test]
    fn test_set_option_warn() {
        let _guard = ProtectStackGuard::new();
        let _inst = TestInstance::new();
        unsafe {
            InitOptions();
            let old = R_SetOptionWarn(1);
            assert_eq!(old, 0);
            assert_eq!(R_ShowWarningOption(), 1);
        }
    }

    #[test]
    fn test_get_option_device_ask_default() {
        let _guard = ProtectStackGuard::new();
        let _inst = TestInstance::new();
        unsafe {
            InitOptions();
            assert_eq!(Rf_GetOptionDeviceAsk(), FALSE);
        }
    }

    #[test]
    fn test_show_error_option() {
        let _guard = ProtectStackGuard::new();
        let _inst = TestInstance::new();
        unsafe {
            InitOptions();
            let val = R_ShowErrorOption();
            assert!(val == 1 || val == 0 || val == NA_INTEGER);
        }
    }

    #[test]
    fn test_set_get_option_roundtrip() {
        let _guard = ProtectStackGuard::new();
        let _inst = TestInstance::new();
        unsafe {
            InitOptions();

            // Set a custom option
            let tag = Rf_install(c"my_custom_option".as_ptr());
            let val = Rf_ScalarInteger(42);
            let _guard = protect(val);
            let old = R_SetOption(tag, val);

            // Should not have existed before
            assert_eq!(old, R_NilValue());

            // Get it back
            let retrieved = GetOptionByName("my_custom_option");
            assert!(!retrieved.is_null());
            assert_eq!(TYPEOF(retrieved), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(retrieved).add(0), 42);

            // Clean up
            R_SetOption(tag, R_NilValue());
        }
    }
}
