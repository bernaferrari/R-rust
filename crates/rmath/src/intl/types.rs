#![allow(non_camel_case_types)]
#![warn(unused_unsafe)]
//! Internal types for the GNU gettext internationalization subsystem.
//!
//! Ported from the C headers: gettextP.h, plural-exp.h, loadinfo.h, gmo.h.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_void};
use std::ptr;

// Re-export c_int for use across intl/ modules.
pub(crate) use std::os::raw::c_int;

// ---------------------------------------------------------------------------
// Basic integer types matching the C originals
// ---------------------------------------------------------------------------

/// 32-bit unsigned integer used throughout .mo file structures.
pub(crate) type nls_uint32 = u32;

// ---------------------------------------------------------------------------
// Locale category constants (from <locale.h>)
// ---------------------------------------------------------------------------

/// LC_MESSAGES category constant (value on most Unix systems).
pub(crate) const LC_MESSAGES: c_int = 5;

// ---------------------------------------------------------------------------
// Loadinfo types (from loadinfo.h)
// ---------------------------------------------------------------------------

/// Bitmask flags for locale name components returned by `_nl_explode_name`.
pub(crate) const XPG_NORM_CODESET: c_int = 1;
pub(crate) const XPG_CODESET: c_int = 2;
pub(crate) const XPG_TERRITORY: c_int = 4;
pub(crate) const XPG_MODIFIER: c_int = 8;

// ---------------------------------------------------------------------------
// Message catalog constants (from gmo.h)
// ---------------------------------------------------------------------------

/// Magic numbers for .mo files.
pub(crate) const MO_MAGIC: nls_uint32 = 0x9504_12de;
pub(crate) const MO_MAGIC_SWAPPED: nls_uint32 = 0xde12_0495;

/// Number of bits in the hash word (assumes unsigned long has at least 32 bits).
pub(crate) const HASHWORDBITS: c_int = 32;

// ---------------------------------------------------------------------------
// Binding (from gettextP.h)
// ---------------------------------------------------------------------------

/// Owned directory and encoding settings for one message domain.
pub(crate) struct DomainBinding {
    pub dirname: std::ffi::CString,
    pub codeset: Option<std::ffi::CString>,
}

impl Default for DomainBinding {
    fn default() -> Self {
        Self {
            dirname: c"/".to_owned(),
            codeset: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Printf argument types (from printf-args.h)
// ---------------------------------------------------------------------------

/// Argument type discriminator for decomposed printf argument lists.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum arg_type {
    TYPE_NONE,
    TYPE_SCHAR,
    TYPE_UCHAR,
    TYPE_SHORT,
    TYPE_USHORT,
    TYPE_INT,
    TYPE_UINT,
    TYPE_LONGINT,
    TYPE_ULONGINT,
    TYPE_LONGLONGINT,
    TYPE_ULONGLONGINT,
    TYPE_DOUBLE,
    TYPE_LONGDOUBLE,
    TYPE_CHAR,
    TYPE_WIDE_CHAR,
    TYPE_STRING,
    TYPE_WIDE_STRING,
    TYPE_POINTER,
    TYPE_COUNT_SCHAR_POINTER,
    TYPE_COUNT_SHORT_POINTER,
    TYPE_COUNT_INT_POINTER,
    TYPE_COUNT_LONGINT_POINTER,
    TYPE_COUNT_LONGLONGINT_POINTER,
}

/// Polymorphic argument value for printf.
#[repr(C)]
pub(crate) union argument_val {
    pub a_schar: i8,
    pub a_uchar: u8,
    pub a_short: i16,
    pub a_ushort: u16,
    pub a_int: c_int,
    pub a_uint: std::os::raw::c_uint,
    pub a_longint: std::os::raw::c_long,
    pub a_ulongint: std::os::raw::c_ulong,
    pub a_longlongint: i64,
    pub a_ulonglongint: u64,
    pub a_float: f32,
    pub a_double: f64,
    // Note: long double is not directly representable in Rust FFI in a portable
    // way; we use u128 as a placeholder with sufficient size.
    pub a_longdouble: u128,
    pub a_char: c_int,
    pub a_wide_char: std::os::raw::c_int,
    pub a_string: *const c_char,
    pub a_wide_string: *const u32,
    pub a_pointer: *mut c_void,
    pub a_count_schar_pointer: *mut i8,
    pub a_count_short_pointer: *mut i16,
    pub a_count_int_pointer: *mut c_int,
    pub a_count_longint_pointer: *mut std::os::raw::c_long,
    pub a_count_longlongint_pointer: *mut i64,
}

/// A single decomposed printf argument.
#[repr(C)]
pub(crate) struct argument {
    pub type_: arg_type,
    pub a: argument_val,
}

/// Collection of decomposed printf arguments.
#[repr(C)]
pub(crate) struct arguments {
    pub count: usize,
    pub arg: *mut argument,
}

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

/// Default message catalog directory.
pub(crate) static _nl_default_dirname: [c_char; 2] = [0x2f, 0];

/// Default default text domain name ("messages").
pub(crate) const DEFAULT_TEXT_DOMAIN: &std::ffi::CStr = c"messages";

pub(crate) struct IntlRuntimeState {
    pub domain_bindings: std::collections::HashMap<std::ffi::CString, DomainBinding>,
    pub msg_cat_cntr: c_int,
    pub current_default_domain: std::ffi::CString,
}

impl Default for IntlRuntimeState {
    fn default() -> Self {
        Self {
            domain_bindings: std::collections::HashMap::default(),
            msg_cat_cntr: 0,
            current_default_domain: DEFAULT_TEXT_DOMAIN.to_owned(),
        }
    }
}

pub(crate) fn with_intl_runtime<R>(f: impl FnOnce(&mut IntlRuntimeState) -> R) -> R {
    crate::sexp::instance::with_required_current_instance(|inst| {
        f(unsafe { &mut (*inst).intl_state })
    })
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Null-checked shared borrow of an interior pointer.
///
/// # Safety
///
/// `ptr` must be null or valid for shared reads of `T` for the caller-chosen lifetime `'a`.
#[inline]
pub(crate) unsafe fn ptr_ref<'a, T>(ptr: *const T) -> Option<&'a T> {
    unsafe { ptr.as_ref() }
}

/// Null-checked mutable borrow of an interior pointer.
///
/// Callers must ensure no conflicting borrow of the same object exists,
/// matching the C code's single-threaded access model.
///
/// # Safety
///
/// `ptr` must be null or valid for exclusive reads/writes of `T` for the lifetime `'a`, with no
/// other live reference to the pointee.
#[inline]
pub(crate) unsafe fn ptr_mut<'a, T>(ptr: *mut T) -> Option<&'a mut T> {
    unsafe { ptr.as_mut() }
}

/// Null-checked borrow of a NUL-terminated C string.
///
/// Callers guarantee the pointer is valid and NUL-terminated, matching
/// the C API's `char *` contract.
///
/// # Safety
///
/// `s` must be null or point to a NUL-terminated string that stays live and unmodified for `'a`.
#[inline]
pub(crate) unsafe fn cstr<'a>(s: *const c_char) -> Option<&'a std::ffi::CStr> {
    if s.is_null() {
        None
    } else {
        unsafe { Some(std::ffi::CStr::from_ptr(s)) }
    }
}

/// Null-checked borrow of a `len`-element buffer.
///
/// Callers guarantee the buffer holds at least `len` elements, matching
/// the C API's buffer-and-length contract.
///
/// # Safety
///
/// `ptr` must be null or valid for shared reads of `len` elements for the lifetime `'a`.
#[inline]
pub(crate) unsafe fn slice_ref<'a, T>(ptr: *const T, len: usize) -> Option<&'a [T]> {
    if ptr.is_null() {
        if len == 0 { Some(&[]) } else { None }
    } else {
        Some(unsafe { std::slice::from_raw_parts(ptr, len) })
    }
}

/// Null-checked mutable borrow of a `len`-element buffer.
///
/// Callers must ensure no conflicting borrow of the buffer exists,
/// matching the C code's single-threaded access model.
///
/// # Safety
///
/// `ptr` must be null or valid for exclusive reads/writes of `len` elements for the lifetime `'a`.
#[inline]
pub(crate) unsafe fn slice_mut<'a, T>(ptr: *mut T, len: usize) -> Option<&'a mut [T]> {
    if ptr.is_null() {
        if len == 0 { Some(&mut []) } else { None }
    } else {
        Some(unsafe { std::slice::from_raw_parts_mut(ptr, len) })
    }
}

/// Duplicate a C string using `std::alloc`.
///
/// # Safety
///
/// `s` must be null or point to a NUL-terminated string.
pub(crate) unsafe fn c_strdup(s: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(s) = (unsafe { cstr(s) }) else {
        return ptr::null_mut();
    };
    let len = s.to_bytes().len() + 1;
    let layout = std::alloc::Layout::from_size_align(len, 1)
        .unwrap_or_else(|_| std::alloc::Layout::new::<u8>());
    // Allocate `len` bytes and copy the source string, NUL included.
    unsafe {
        let out = std::alloc::alloc(layout) as *mut c_char;
        if !out.is_null() {
            ptr::copy_nonoverlapping(s.as_ptr(), out, len);
        }
        out
    }
}

/// Free memory previously allocated via c_strdup or std::alloc.
///
/// # Safety
///
/// `p` must be null or a live allocation returned by `c_strdup` (sized by its NUL-terminated
/// contents) that is not used afterwards.
pub(crate) unsafe fn c_free(p: *mut c_char) {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(s) = (unsafe { cstr(p) }) else {
        return;
    };
    let len = s.to_bytes().len() + 1;
    let layout = std::alloc::Layout::from_size_align(len, 1)
        .unwrap_or_else(|_| std::alloc::Layout::new::<u8>());
    // Free the `len`-byte block allocated by `c_strdup`.
    unsafe { std::alloc::dealloc(p as *mut u8, layout) };
}
