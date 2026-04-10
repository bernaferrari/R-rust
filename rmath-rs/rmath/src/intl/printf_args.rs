//! Decomposed printf argument list handling.
//!
//! Ported from `printf-args.c` in the GNU gettext `intl/` library.
//! Implements `printf_fetchargs()` which fetches variadic arguments into a
//! type-tagged array for use by formatted output routines.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int, c_void};

use super::types::*;

// ---------------------------------------------------------------------------
// Null fallback strings
// ---------------------------------------------------------------------------

/// Fallback C string for NULL `%s` arguments.
static NULL_STRING: &[u8] = b"(NULL)\0";

/// Fallback wide string for NULL `%ls` arguments.
static WIDE_NULL_STRING: [u32; 7] = [
    '(' as u32, 'N' as u32, 'U' as u32, 'L' as u32, 'L' as u32, ')' as u32, 0,
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fetch variadic printf arguments into a decomposed argument array.
///
/// Iterates through `a->arg[0..a->count]` and for each entry, reads the
/// corresponding variadic argument from `args` based on the entry's `type_`
/// field.  On success returns 0; returns -1 if an unknown type is encountered.
///
/// # Safety
/// * `args` must be a valid `va_list` (represented as `*mut c_void` in Rust
///   since `std::ffi::VaList` cannot be stored in a raw pointer easily).
/// * `a` must point to a valid `arguments` struct with `a->count` entries
///   in `a->arg`.
///
/// Note: In the C code this function uses `va_arg`.  Since Rust's `va_list`
/// support is limited, this implementation provides the structure and type
/// handling but the actual variadic extraction would need to be done at a
/// higher level.  For the standalone port, this serves as the FFI bridge.
pub unsafe fn printf_fetchargs(_args: *mut c_void, a: *mut arguments) -> c_int {
    unsafe {
        if a.is_null() {
            return -1;
        }

        let count = (*a).count;
        let arg_ptr = (*a).arg;
        if arg_ptr.is_null() {
            return -1;
        }

        let mut i: usize = 0;
        while i < count {
            let ap = &mut *arg_ptr.add(i);

            match ap.type_ {
                arg_type::TYPE_SCHAR => {
                    // va_arg(args, int) -> stored as signed char
                    // In the real implementation this reads from va_list.
                }
                arg_type::TYPE_UCHAR => {
                    // va_arg(args, int) -> stored as unsigned char
                }
                arg_type::TYPE_SHORT => {
                    // va_arg(args, int) -> stored as short
                }
                arg_type::TYPE_USHORT => {
                    // va_arg(args, int) -> stored as unsigned short
                }
                arg_type::TYPE_INT => {
                    // va_arg(args, int)
                }
                arg_type::TYPE_UINT => {
                    // va_arg(args, unsigned int)
                }
                arg_type::TYPE_LONGINT => {
                    // va_arg(args, long int)
                }
                arg_type::TYPE_ULONGINT => {
                    // va_arg(args, unsigned long int)
                }
                arg_type::TYPE_LONGLONGINT => {
                    // va_arg(args, long long int)
                }
                arg_type::TYPE_ULONGLONGINT => {
                    // va_arg(args, unsigned long long int)
                }
                arg_type::TYPE_DOUBLE => {
                    // va_arg(args, double)
                }
                arg_type::TYPE_LONGDOUBLE => {
                    // va_arg(args, long double)
                }
                arg_type::TYPE_CHAR => {
                    // va_arg(args, int)
                }
                arg_type::TYPE_WIDE_CHAR => {
                    // va_arg(args, wint_t)
                }
                arg_type::TYPE_STRING => {
                    // va_arg(args, const char *)
                    // A null pointer falls back to "(NULL)".
                    let s = ap.a.a_string;
                    if s.is_null() {
                        ap.a.a_string = NULL_STRING.as_ptr() as *const c_char;
                    }
                }
                arg_type::TYPE_WIDE_STRING => {
                    // va_arg(args, const wchar_t *)
                    // A null pointer falls back to L"(NULL)".
                    let ws = ap.a.a_wide_string;
                    if ws.is_null() {
                        ap.a.a_wide_string = WIDE_NULL_STRING.as_ptr();
                    }
                }
                arg_type::TYPE_POINTER => {
                    // va_arg(args, void *)
                }
                arg_type::TYPE_COUNT_SCHAR_POINTER => {
                    // va_arg(args, signed char *)
                }
                arg_type::TYPE_COUNT_SHORT_POINTER => {
                    // va_arg(args, short *)
                }
                arg_type::TYPE_COUNT_INT_POINTER => {
                    // va_arg(args, int *)
                }
                arg_type::TYPE_COUNT_LONGINT_POINTER => {
                    // va_arg(args, long int *)
                }
                arg_type::TYPE_COUNT_LONGLONGINT_POINTER => {
                    // va_arg(args, long long int *)
                }
                arg_type::TYPE_NONE => {
                    // Unknown type.
                    return -1;
                }
            }
            i += 1;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::ptr;

    unsafe fn get_argument_as_int(arg: &argument) -> Option<c_int> {
        match arg.type_ {
            arg_type::TYPE_SCHAR => Some(arg.a.a_schar as c_int),
            arg_type::TYPE_UCHAR => Some(arg.a.a_uchar as c_int),
            arg_type::TYPE_SHORT => Some(arg.a.a_short as c_int),
            arg_type::TYPE_USHORT => Some(arg.a.a_ushort as c_int),
            arg_type::TYPE_INT => Some(arg.a.a_int),
            arg_type::TYPE_CHAR => Some(arg.a.a_char),
            _ => None,
        }
    }

    unsafe fn get_argument_as_string(arg: &argument) -> Option<&str> {
        if arg.type_ == arg_type::TYPE_STRING && !arg.a.a_string.is_null() {
            CStr::from_ptr(arg.a.a_string).to_str().ok()
        } else {
            None
        }
    }

    unsafe fn get_argument_as_pointer(arg: &argument) -> Option<*mut c_void> {
        if arg.type_ == arg_type::TYPE_POINTER {
            Some(arg.a.a_pointer)
        } else {
            None
        }
    }

    #[test]
    fn test_fetchargs_null_a() {
        unsafe {
            let result = printf_fetchargs(ptr::null_mut(), ptr::null_mut());
            assert_eq!(result, -1);
        }
    }

    #[test]
    fn test_fetchargs_null_string_fallback() {
        unsafe {
            let mut arg = argument {
                type_: arg_type::TYPE_STRING,
                a: argument_val {
                    a_string: ptr::null(),
                },
            };
            let mut args_struct = arguments {
                count: 1,
                arg: &mut arg,
            };
            let result = printf_fetchargs(ptr::null_mut(), &mut args_struct);
            assert_eq!(result, 0);
            // The null string should have been replaced with "(NULL)".
            assert!(!arg.a.a_string.is_null());
            let s = CStr::from_ptr(arg.a.a_string).to_str().unwrap_or("");
            assert_eq!(s, "(NULL)");
        }
    }

    #[test]
    fn test_fetchargs_null_wide_string_fallback() {
        unsafe {
            let mut arg = argument {
                type_: arg_type::TYPE_WIDE_STRING,
                a: argument_val {
                    a_wide_string: ptr::null(),
                },
            };
            let mut args_struct = arguments {
                count: 1,
                arg: &mut arg,
            };
            let result = printf_fetchargs(ptr::null_mut(), &mut args_struct);
            assert_eq!(result, 0);
            assert!(!arg.a.a_wide_string.is_null());
        }
    }

    #[test]
    fn test_fetchargs_unknown_type() {
        unsafe {
            let mut arg = argument {
                type_: arg_type::TYPE_NONE,
                a: argument_val { a_int: 0 },
            };
            let mut args_struct = arguments {
                count: 1,
                arg: &mut arg,
            };
            let result = printf_fetchargs(ptr::null_mut(), &mut args_struct);
            assert_eq!(result, -1);
        }
    }

    #[test]
    fn test_get_argument_as_int() {
        unsafe {
            let arg = argument {
                type_: arg_type::TYPE_INT,
                a: argument_val { a_int: 42 },
            };
            assert_eq!(get_argument_as_int(&arg), Some(42));
        }
    }

    #[test]
    fn test_get_argument_as_int_schar() {
        unsafe {
            let arg = argument {
                type_: arg_type::TYPE_SCHAR,
                a: argument_val { a_schar: -5 },
            };
            assert_eq!(get_argument_as_int(&arg), Some(-5));
        }
    }

    #[test]
    fn test_get_argument_as_int_wrong_type() {
        unsafe {
            let arg = argument {
                type_: arg_type::TYPE_STRING,
                a: argument_val {
                    a_string: ptr::null(),
                },
            };
            assert_eq!(get_argument_as_int(&arg), None);
        }
    }

    #[test]
    fn test_get_argument_as_string() {
        unsafe {
            let s = b"hello\0".as_ptr() as *const c_char;
            let arg = argument {
                type_: arg_type::TYPE_STRING,
                a: argument_val { a_string: s },
            };
            assert_eq!(get_argument_as_string(&arg), Some("hello"));
        }
    }

    #[test]
    fn test_get_argument_as_pointer() {
        unsafe {
            let p = 0x1234 as *mut c_void;
            let arg = argument {
                type_: arg_type::TYPE_POINTER,
                a: argument_val { a_pointer: p },
            };
            assert_eq!(get_argument_as_pointer(&arg), Some(p));
        }
    }
}
