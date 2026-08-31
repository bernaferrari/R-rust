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
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::memory_ext::allocLang;
use crate::sexp::protect::{R_PreserveObject, protect};
use crate::sexp::symbol::Rf_install;

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
const R_MIN_SCIPEN_OPT: c_int = 0;

/// From Print.h -- maximum valid scipen.
const R_MAX_SCIPEN_OPT: c_int = 50;

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
    unsafe { Rf_install(CString::new(".Options").unwrap_or_default().as_ptr()) }
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
        libc::strcmp(a, b) == 0
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
                iWARN | iSILENT => return dnew,
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

        crate::sexp::instance::with_required_current_instance(|inst| match inst.options.get(name) {
            Some(&val) if (val as usize) > 0x1000 && (val as usize).trailing_zeros() >= 3 => val,
            _ => nil,
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
    unsafe {
        let nil = R_NilValue();

        crate::sexp::instance::with_required_current_instance(|inst| {
            let n = inst.options.len();
            if n == 0 {
                return nil;
            }
            let mut keys: Vec<String> = inst.options.keys().cloned().collect();
            keys.sort();

            let mut result: SEXP = nil;
            for key in keys.iter().rev() {
                let tag = Rf_install(CString::new(key.as_str()).unwrap_or_default().as_ptr());
                let val = *inst.options.get(key.as_str()).unwrap_or(&nil);
                let cell = Rf_cons(val, result);
                SETTAG(cell, tag);
                result = cell;
            }
            result
        })
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

        crate::sexp::instance::with_required_current_instance(|inst| {
            match inst.options.get(name_str.as_str()) {
                Some(&val) => {
                    let cell = Rf_cons(val, nil);
                    if !cell.is_null() {
                        SETTAG(cell, tag);
                    }
                    cell
                }
                None => nil,
            }
        })
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
unsafe fn SetOptionByName(name: &str, value: SEXP) -> SEXP {
    unsafe {
        let nil = R_NilValue();
        InitOptions();

        crate::sexp::instance::with_required_current_instance(|inst| {
            if value == nil {
                inst.options.remove(name).unwrap_or(nil)
            } else {
                R_PreserveObject(value);
                inst.options.insert(name.to_string(), value).unwrap_or(nil)
            }
        })
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
        let width_sym = Rf_install(CString::new("width").unwrap_or_default().as_ptr());
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

/// Populate an options HashMap with default R option values.
unsafe fn populate_options(options: &mut HashMap<String, SEXP>) {
    unsafe {
        let pi = crate::sexp::constructors::persistent_scalar_integer;
        let pl = crate::sexp::constructors::persistent_scalar_logical;
        let pm = crate::sexp::constructors::persistent_mkstring;

        let val = pm(CString::new("> ").unwrap_or_default().as_ptr());
        options.insert("prompt".to_string(), val);

        let val = pm(CString::new("+ ").unwrap_or_default().as_ptr());
        options.insert("continue".to_string(), val);

        options.insert("expressions".to_string(), pi(5000));
        options.insert("width".to_string(), pi(80));
        options.insert("deparse.cutoff".to_string(), pi(60));
        options.insert("digits".to_string(), pi(7));
        options.insert("echo".to_string(), pl(TRUE));
        options.insert("quiet".to_string(), pl(FALSE));
        options.insert("verbose".to_string(), pl(FALSE));
        options.insert("check.bounds".to_string(), pl(FALSE));
        options.insert("keep.source".to_string(), pl(FALSE));
        options.insert("keep.source.pkgs".to_string(), pl(FALSE));
        options.insert("keep.parse.data".to_string(), pl(TRUE));
        options.insert("keep.parse.data.pkgs".to_string(), pl(FALSE));
        options.insert("warning.length".to_string(), pi(1000));
        options.insert("nwarnings".to_string(), pi(50));

        let val = pm(CString::new(".").unwrap_or_default().as_ptr());
        options.insert("OutDec".to_string(), val);

        options.insert("CBoundsCheck".to_string(), pl(FALSE));

        let val = pm(CString::new("default").unwrap_or_default().as_ptr());
        options.insert("matprod".to_string(), val);

        options.insert("PCRE_study".to_string(), pl(TRUE));
        options.insert("PCRE_use_JIT".to_string(), pl(TRUE));
        options.insert("PCRE_limit_recursion".to_string(), pl(NA_LOGICAL));
        options.insert("max.contour.segments".to_string(), pi(25000));
        options.insert("warnPartialMatchDollar".to_string(), pl(FALSE));
        options.insert("warnPartialMatchArgs".to_string(), pl(FALSE));
        options.insert("warnPartialMatchAttr".to_string(), pl(FALSE));
        options.insert("showWarnCalls".to_string(), pl(FALSE));
        options.insert("showErrorCalls".to_string(), pl(FALSE));
        options.insert("showNCalls".to_string(), pi(50));
        options.insert("browserNLdisabled".to_string(), pl(FALSE));
        options.insert("warn".to_string(), pi(0));
        options.insert("max.print".to_string(), pi(99999));
        options.insert("show.error.messages".to_string(), pl(TRUE));
        options.insert("scipen".to_string(), pi(0));
        options.insert("height".to_string(), pi(60));
        options.insert("add.smooth".to_string(), pl(TRUE));
    }
}

/// Initialize the default options list.
pub unsafe fn InitOptions() {
    unsafe {
        crate::sexp::instance::with_required_current_instance(|inst| {
            if inst.options_initialized {
                return;
            }
            populate_options(&mut inst.options);
            inst.options_initialized = true;
        });
    }
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
        checkArity(op, args);
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

        // Zero-argument case: return all options sorted alphabetically
        if args == R_NilValue() {
            let nil = R_NilValue();

            return crate::sexp::instance::with_required_current_instance(|inst| {
                let n = inst.options.len() as c_int;

                let value = Rf_allocVector(SEXPTYPE::VECSXP, n);
                let _value_guard = protect(value);
                let names = Rf_allocVector(SEXPTYPE::STRSXP, n);
                let _names_guard = protect(names);

                let mut keys: Vec<String> = inst.options.keys().cloned().collect();
                keys.sort();

                for (i, key) in keys.iter().enumerate() {
                    let name_charsxp =
                        Rf_mkChar(CString::new(key.as_str()).unwrap_or_default().as_ptr());
                    SET_STRING_ELT(names, i as R_xlen_t, name_charsxp);
                    if let Some(&val) = inst.options.get(key) {
                        SET_VECTOR_ELT(value, i as R_xlen_t, duplicate_sexp(val));
                    }
                }

                setAttrib(value, R_NamesSymbol(), names);
                set_R_Visible(TRUE);
                value
            });
        }

        // The arguments to "options" can either be a sequence of
        // name = value form, or can be a single list.
        let mut n = length(args);
        let mut args = args;

        if n == 1
            && (isPairList(CAR(args)) != 0 || isVectorList(CAR(args)) != 0)
            && TAG(args) == R_NilValue()
        {
            args = CAR(args);
            n = length(args);
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
                } else if streql(
                    name_cstr,
                    CString::new("width").unwrap_or_default().as_ptr(),
                ) {
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
                } else if streql(
                    name_cstr,
                    CString::new("deparse.cutoff").unwrap_or_default().as_ptr(),
                ) {
                    let k = asInteger(argi);
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("digits").unwrap_or_default().as_ptr(),
                ) {
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
                } else if streql(
                    name_cstr,
                    CString::new("expressions").unwrap_or_default().as_ptr(),
                ) {
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
                } else if streql(
                    name_cstr,
                    CString::new("keep.source").unwrap_or_default().as_ptr(),
                ) {
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
                } else if streql(
                    name_cstr,
                    CString::new("continue").unwrap_or_default().as_ptr(),
                ) {
                    let s = asChar(argi);
                    if s.is_null() {
                        r_error("invalid value for 'continue'");
                    }
                    let new_val = Rf_mkString(translateChar(s));
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("prompt").unwrap_or_default().as_ptr(),
                ) {
                    let s = asChar(argi);
                    if s.is_null() {
                        r_error("invalid value for 'prompt'");
                    }
                    let new_val = Rf_mkString(translateChar(s));
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("contrasts").unwrap_or_default().as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::STRSXP || LENGTH(argi) != 2 {
                        r_error("invalid value for 'contrasts'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("check.bounds").unwrap_or_default().as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'check.bounds'");
                    }
                    let k = asLogical(argi);
                    let v = Rf_ScalarLogical(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(name_cstr, CString::new("warn").unwrap_or_default().as_ptr()) {
                    if isNumeric(argi) == 0 || LENGTH(argi) != 1 {
                        r_error("invalid value for 'warn'");
                    }
                    let k = asInteger(argi);
                    if k == NA_INTEGER {
                        r_error("invalid value for 'warn'");
                    }
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("warning.length").unwrap_or_default().as_ptr(),
                ) {
                    let k = asInteger(argi);
                    if k < 100 || k > 8170 {
                        r_error("invalid value for 'warning.length'");
                    }
                    crate::mainutils::errors::R_SetWarnLength(k);
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("warning.expression")
                        .unwrap_or_default()
                        .as_ptr(),
                ) {
                    if isLanguage(argi) == 0 && isExpression(argi) == 0 {
                        r_error("invalid value for 'warning.expression'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("max.print").unwrap_or_default().as_ptr(),
                ) {
                    let k = asInteger(argi);
                    if k < 1 {
                        r_error("invalid value for 'max.print'");
                    }
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("scipen").unwrap_or_default().as_ptr(),
                ) {
                    let k = FixupScipen(argi, iWARN);
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("nwarnings").unwrap_or_default().as_ptr(),
                ) {
                    let k = asInteger(argi);
                    if k < 1 {
                        r_error("invalid value for 'nwarnings'");
                    }
                    crate::mainutils::main::R_SetCollectWarnings(0); // force a reset
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("error").unwrap_or_default().as_ptr(),
                ) {
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
                } else if streql(
                    name_cstr,
                    CString::new("show.error.messages")
                        .unwrap_or_default()
                        .as_ptr(),
                ) {
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
                } else if streql(
                    name_cstr,
                    CString::new("catch.script.errors")
                        .unwrap_or_default()
                        .as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'catch.script.errors'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(name_cstr, CString::new("echo").unwrap_or_default().as_ptr()) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'echo'");
                    }
                    let k = asLogical(argi);
                    let v = Rf_ScalarLogical(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("OutDec").unwrap_or_default().as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::STRSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'OutDec'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("max.contour.segments")
                        .unwrap_or_default()
                        .as_ptr(),
                ) {
                    let k = asInteger(argi);
                    if k < 0 {
                        r_error("invalid value for 'max.contour.segments'");
                    }
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("warnPartialMatchDollar")
                        .unwrap_or_default()
                        .as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'warnPartialMatchDollar'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("warnPartialMatchArgs")
                        .unwrap_or_default()
                        .as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'warnPartialMatchArgs'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("warnPartialMatchAttr")
                        .unwrap_or_default()
                        .as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'warnPartialMatchAttr'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("showWarnCalls").unwrap_or_default().as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'showWarnCalls'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("showErrorCalls").unwrap_or_default().as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'showErrorCalls'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("showNCalls").unwrap_or_default().as_ptr(),
                ) {
                    let k = asInteger(argi);
                    if k < 30 || k > 500 || k == NA_INTEGER || LENGTH(argi) != 1 {
                        r_error("invalid value for 'showNCalls'");
                    }
                    let v = Rf_ScalarInteger(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("browserNLdisabled")
                        .unwrap_or_default()
                        .as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'browserNLdisabled'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("CBoundsCheck").unwrap_or_default().as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP
                        || LENGTH(argi) != 1
                        || *LOGICAL(argi).add(0) == NA_LOGICAL
                    {
                        r_error("invalid value for 'CBoundsCheck'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("quiet").unwrap_or_default().as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'quiet'");
                    }
                    let k = asLogical(argi);
                    let v = Rf_ScalarLogical(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("verbose").unwrap_or_default().as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'verbose'");
                    }
                    let k = asLogical(argi);
                    let v = Rf_ScalarLogical(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("matprod").unwrap_or_default().as_ptr(),
                ) {
                    let s = asChar(argi);
                    if s.is_null() {
                        r_error("invalid value for 'matprod'");
                    }
                    let new_val = duplicate_sexp(argi);
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("PCRE_study").unwrap_or_default().as_ptr(),
                ) {
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
                } else if streql(
                    name_cstr,
                    CString::new("PCRE_use_JIT").unwrap_or_default().as_ptr(),
                ) {
                    let use_jit = asLogical(argi);
                    let v = Rf_ScalarLogical(use_jit);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("PCRE_limit_recursion")
                        .unwrap_or_default()
                        .as_ptr(),
                ) {
                    let v = Rf_ScalarLogical(asLogical(argi));
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("stringsAsFactors")
                        .unwrap_or_default()
                        .as_ptr(),
                ) {
                    if TYPEOF(argi) != SEXPTYPE::LGLSXP || LENGTH(argi) != 1 {
                        r_error("invalid value for 'stringsAsFactors'");
                    }
                    let k = asLogical(argi);
                    let v = Rf_ScalarLogical(k);
                    let _guard = protect(v);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, v));
                } else if streql(
                    name_cstr,
                    CString::new("editor").unwrap_or_default().as_ptr(),
                ) {
                    let s = asChar(argi);
                    if s.is_null() {
                        r_error("invalid value for 'editor'");
                    }
                    let new_val = Rf_mkString(translateChar(s));
                    let _guard = protect(new_val);
                    SET_VECTOR_ELT(value, i as R_xlen_t, SetOption(tag, new_val));
                } else if streql(
                    name_cstr,
                    CString::new("par.ask.default").unwrap_or_default().as_ptr(),
                ) {
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
    use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};
    use crate::sexp::protect::{R_ProtectCount, protect_n};
    use std::ffi::CString;

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
        _instance: Box<RInstance>,
    }

    impl TestInstance {
        fn new() -> Self {
            let mut instance = Box::new(RInstance::new());
            unsafe {
                set_current_instance(&mut *instance);
            }
            TestInstance {
                _instance: instance,
            }
        }
    }

    impl Drop for TestInstance {
        fn drop(&mut self) {
            clear_current_instance();
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
            assert_eq!(fixup_scipen(0), 0);
            assert_eq!(fixup_scipen(50), 50);
            assert_eq!(fixup_scipen(-1), 0);
            assert_eq!(fixup_scipen(51), 50);
            assert_eq!(fixup_scipen(c_int::MIN), 0);
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
        let left = crate::sexp::session::RSession::new();
        let right = crate::sexp::session::RSession::new();

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
            let tag = Rf_install(
                CString::new("my_custom_option")
                    .unwrap_or_default()
                    .as_ptr(),
            );
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
