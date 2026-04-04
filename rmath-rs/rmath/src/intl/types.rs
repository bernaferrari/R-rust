#![allow(non_camel_case_types)]
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

/// An entry in the list of already loaded locale files.
///
/// Corresponds to `struct loaded_l10nfile` in loadinfo.h.
/// The `successor` field is a flexible array member in C (size 1 minimum).
#[repr(C)]
pub(crate) struct loaded_l10nfile {
    pub filename: *const c_char,
    pub decided: c_int,
    pub data: *const c_void,
    pub next: *mut loaded_l10nfile,
    /// Flexible array: successors[0] in C. We store just one pointer here;
    /// callers that need more must allocate extra space after this struct.
    pub successor: [*mut loaded_l10nfile; 1],
}

// ---------------------------------------------------------------------------
// Printf argument types (from printf-args.h)
// ---------------------------------------------------------------------------

/// Magic numbers for .mo files.
pub(crate) const MO_MAGIC: nls_uint32 = 0x9504_12de;
pub(crate) const MO_MAGIC_SWAPPED: nls_uint32 = 0xde12_0495;

/// Number of bits in the hash word (assumes unsigned long has at least 32 bits).
pub(crate) const HASHWORDBITS: c_int = 32;

/// Descriptor for a static string contained in a binary .mo file.
#[repr(C)]
pub(crate) struct string_desc {
    pub length: nls_uint32,
    pub offset: nls_uint32,
}

/// In-memory representation of a system-dependent string.
#[repr(C)]
pub(crate) struct sysdep_string_desc {
    pub length: usize,
    pub pointer: *const c_char,
}

/// Cache of translated strings after charset conversion.
#[repr(C)]
pub(crate) struct converted_domain {
    pub encoding: *const c_char,
    /// Opaque conversion descriptor (iconv_t equivalent).
    pub conv: *mut c_void,
    pub conv_tab: *mut *mut c_char,
}

// ---------------------------------------------------------------------------
// Plural expression types (from plural-exp.h)
// ---------------------------------------------------------------------------

/// Operators for plural-form expression trees.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum expression_operator {
    var,
    num,
    lnot,
    mult,
    divide,
    module,
    plus,
    minus,
    less_than,
    greater_than,
    less_or_equal,
    greater_or_equal,
    equal,
    not_equal,
    land,
    lor,
    qmop,
}

/// A node in the plural-form expression tree.
#[repr(C)]
pub(crate) struct expression {
    pub nargs: c_int,
    pub operation: expression_operator,
    /// Union: either a numeric literal or up to three child expression pointers.
    pub val: expression_val,
}

/// Union variant for `expression.val`.
#[repr(C)]
pub(crate) union expression_val {
    pub num: std::os::raw::c_ulong,
    pub args: [*mut expression; 3],
}

impl expression_val {
    pub(crate) unsafe fn get_num(&self) -> std::os::raw::c_ulong {
        unsafe { self.num }
    }

    pub(crate) unsafe fn get_args(&self) -> &[*mut expression; 3] {
        unsafe { &self.args }
    }
}

/// Parser state passed between the scanner and the plural-form parser.
#[repr(C)]
pub(crate) struct parse_args {
    pub cp: *const c_char,
    pub res: *mut expression,
}

// ---------------------------------------------------------------------------
// Loaded domain (from gettextP.h)
// ---------------------------------------------------------------------------

/// The in-memory representation of an opened message catalog (.mo file).
#[repr(C)]
pub(crate) struct loaded_domain {
    pub data: *const c_char,
    pub use_mmap: c_int,
    pub mmap_size: usize,
    pub must_swap: c_int,
    pub malloced: *mut c_void,

    pub nstrings: nls_uint32,
    pub orig_tab: *const string_desc,
    pub trans_tab: *const string_desc,

    pub n_sysdep_strings: nls_uint32,
    pub orig_sysdep_tab: *const sysdep_string_desc,
    pub trans_sysdep_tab: *const sysdep_string_desc,

    pub hash_size: nls_uint32,
    pub hash_tab: *const nls_uint32,
    pub must_swap_hash_tab: c_int,

    pub conversions: *mut converted_domain,
    pub nconversions: usize,
    /// Stub for gl_rwlock_t conversions_lock.
    pub conversions_lock: [u8; 0],

    pub plural: *const expression,
    pub nplurals: std::os::raw::c_ulong,
}

// ---------------------------------------------------------------------------
// Binding (from gettextP.h)
// ---------------------------------------------------------------------------

/// A binding of domain settings (dirname, codeset).
///
/// In C this uses a zero-length array `domainname[0]` at the end.
/// In Rust we represent this with a flexible Vec for the domain name, stored
/// separately via an allocated buffer. For FFI compatibility the struct layout
/// is kept simple.
#[repr(C)]
pub(crate) struct binding {
    pub next: *mut binding,
    pub dirname: *mut c_char,
    pub codeset: *mut c_char,
    /// Zero-length array placeholder for the domain name string.
    /// The actual allocation includes extra bytes after this struct.
    pub domainname: [c_char; 0],
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
// Global state (stubs matching the C extern declarations in gettextP.h)
// ---------------------------------------------------------------------------

/// Default message catalog directory.
pub(crate) static mut _nl_default_dirname: [c_char; 2] = [0x2f, 0]; // "/"

/// Linked list of domain bindings.
pub(crate) static mut _nl_domain_bindings: *mut binding = ptr::null_mut();

/// Counter incremented when bindings change (flush caches).
pub(crate) static mut _nl_msg_cat_cntr: c_int = 0;

/// Default default text domain name ("messages").
pub(crate) static mut _nl_default_default_domain: [c_char; 9] =
    [0x6d, 0x65, 0x73, 0x73, 0x61, 0x67, 0x65, 0x73, 0]; // "messages\0"

/// Current default text domain (initially points to the default).
pub(crate) static mut _nl_current_default_domain: *const c_char =
    unsafe { (*std::ptr::addr_of!(_nl_default_default_domain)).as_ptr() };

/// Global state lock (no-op in standalone mode).
pub(crate) static mut _nl_state_lock: [u8; 0] = [];

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Byte-swap a 32-bit value (for .mo files with opposite endianness).
#[inline]
pub(crate) fn SWAP(i: nls_uint32) -> nls_uint32 {
    (i << 24) | ((i & 0xff00) << 8) | ((i >> 8) & 0xff00) | (i >> 24)
}

/// Thread-safety stubs (no-op in standalone mode).
pub(crate) unsafe fn gl_rwlock_wrlock(_lock: &mut [u8; 0]) {
    // No-op in standalone mode.
}

pub(crate) unsafe fn gl_rwlock_unlock(_lock: &mut [u8; 0]) {
    // No-op in standalone mode.
}

/// Lock the global state lock (no-op in standalone mode).
pub(crate) unsafe fn nl_state_lock_wrlock() {
    unsafe {
        gl_rwlock_wrlock(&mut *std::ptr::addr_of_mut!(_nl_state_lock));
    }
}

/// Unlock the global state lock (no-op in standalone mode).
pub(crate) unsafe fn nl_state_lock_unlock() {
    unsafe {
        gl_rwlock_unlock(&mut *std::ptr::addr_of_mut!(_nl_state_lock));
    }
}

/// Duplicate a C string using `std::alloc`.
pub(crate) unsafe fn c_strdup(s: *const c_char) -> *mut c_char {
    unsafe {
        if s.is_null() {
            return ptr::null_mut();
        }
        let len = std::ffi::CStr::from_ptr(s).to_bytes().len() + 1;
        let layout = std::alloc::Layout::from_size_align(len, 1).unwrap();
        let ptr = std::alloc::alloc(layout) as *mut c_char;
        if !ptr.is_null() {
            ptr::copy_nonoverlapping(s, ptr, len);
        }
        ptr
    }
}

/// Free memory previously allocated via c_strdup or std::alloc.
pub(crate) unsafe fn c_free(p: *mut c_char) {
    unsafe {
        if p.is_null() {
            return;
        }
        let len = std::ffi::CStr::from_ptr(p).to_bytes().len() + 1;
        let layout = std::alloc::Layout::from_size_align(len, 1).unwrap();
        std::alloc::dealloc(p as *mut u8, layout);
    }
}
