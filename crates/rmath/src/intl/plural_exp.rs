#![warn(unused_unsafe)]
//! Plural expression evaluation for GNU gettext.
//!
//! Ported from `plural-exp.c` in the GNU gettext `intl/` library.
//! Provides the Germanic (default) plural form and expression extraction
//! from the null entry of a .mo file.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int, c_ulong};
use std::ptr;

use super::types::*;

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn PLURAL_PARSE(_arg: *mut parse_args) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `libintl_gettextparse`'s contract per this function's own `# Safety` requirements.
    unsafe { super::plural_parse::libintl_gettextparse(_arg) }
}

fn boxed_expression(
    nargs: c_int,
    operation: expression_operator,
    val: expression_val,
) -> *mut expression {
    Box::into_raw(Box::new(expression {
        nargs,
        operation,
        val,
    }))
}

fn boxed_var_expression() -> *mut expression {
    boxed_expression(0, expression_operator::var, expression_val { num: 0 })
}

fn boxed_num_expression(value: c_ulong) -> *mut expression {
    boxed_expression(0, expression_operator::num, expression_val { num: value })
}

fn boxed_binary_expression(
    operation: expression_operator,
    left: *mut expression,
    right: *mut expression,
) -> *mut expression {
    boxed_expression(
        2,
        operation,
        expression_val {
            args: [left, right, ptr::null_mut()],
        },
    )
}

/// Build gettext's default Germanic plural expression (`n != 1`).
///
/// The C implementation points every fallback catalog at file-scope mutable
/// expression nodes. Here each loaded catalog owns its fallback tree for the
/// catalog lifetime, so there is no shared mutable process or thread state.
fn boxed_germanic_plural() -> *const expression {
    let var = boxed_var_expression();
    let one = boxed_num_expression(1);
    boxed_binary_expression(expression_operator::not_equal, var, one)
}

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// Parse a simple unsigned integer from a C string, returning the value
/// and advancing `endp` past the consumed digits.
///
/// Equivalent to `strtoul(nplurals, &endp, 10)` for decimal parsing.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn parse_ulong(s: *const c_char, endp: &mut *const c_char) -> c_ulong {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(text) = (unsafe { cstr(s) }) else {
        *endp = s;
        return 0;
    };
    let bytes = text.to_bytes();
    let mut n: c_ulong = 0;
    let mut i = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        n = n
            .wrapping_mul(10)
            .wrapping_add((bytes[i] - b'0') as c_ulong);
        i += 1;
    }
    // Point `endp` just past the consumed digits inside `s`.
    *endp = bytes[i..].as_ptr() as *const c_char;
    n
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract the plural expression and nplurals count from the metadata
/// (null entry) of a .mo file.
///
/// If `nullentry` is null or does not contain valid `plural=` and `nplurals=`
/// fields, falls back to the Germanic plural form (`n != 1`, nplurals = 2).
///
/// # Arguments
/// * `nullentry` - The null entry (metadata string) from the .mo file header.
/// * `pluralp`   - Output: pointer to the plural expression.
/// * `npluralsp` - Output: pointer to the number of plural forms.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn libintl_gettext_extract_plural(
    nullentry: *const c_char,
    pluralp: *mut *const expression,
    npluralsp: *mut c_ulong,
) {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    if let Some(entry_cstr) = unsafe { cstr(nullentry) } {
        let entry = entry_cstr.to_bytes();
        let entry_str = std::str::from_utf8(entry).unwrap_or("");

        let plural_pos = entry_str.find("plural=");
        let nplurals_pos = entry_str.find("nplurals=");

        if let (Some(plural_p), Some(nplurals_p)) = (plural_pos, nplurals_pos) {
            let nplurals_start = nplurals_p + 9; // skip "nplurals="

            // Skip leading whitespace.
            let bytes = entry_str.as_bytes();
            let mut offset = nplurals_start;
            while offset < bytes.len() && (bytes[offset] as char).is_whitespace() {
                offset += 1;
            }

            // Must start with a digit.
            if offset < bytes.len() && bytes[offset].is_ascii_digit() {
                // `nplurals_start` is inside `entry`, so this pointer is
                // still inside `nullentry`'s NUL-terminated buffer.
                let startp = bytes[nplurals_start..].as_ptr() as *const c_char;
                let mut endp: *const c_char = ptr::null();
                // SAFETY: forwarded raw pointers satisfy `parse_ulong`'s contract per this function's own `# Safety` requirements.
                let n = unsafe { parse_ulong(startp, &mut endp) };

                if endp != startp {
                    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                    if let Some(p) = unsafe { ptr_mut(npluralsp) } {
                        *p = n;
                    }

                    // Try to parse the plural expression.
                    let plural_start = plural_p + 7; // skip "plural="
                    let mut args = parse_args {
                        cp: bytes[plural_start..].as_ptr() as *const c_char,
                        res: ptr::null_mut(),
                    };

                    // SAFETY: forwarded raw pointers satisfy `PLURAL_PARSE`'s contract per this function's own `# Safety` requirements.
                    if unsafe { PLURAL_PARSE(&mut args) } == 0 {
                        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
                        if let Some(p) = unsafe { ptr_mut(pluralp) } {
                            *p = args.res;
                        }
                        return;
                    }
                }
            }
        }
    }

    // Fall back to Germanic plural form.
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(p) = unsafe { ptr_mut(pluralp) } {
        *p = boxed_germanic_plural();
    }
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(p) = unsafe { ptr_mut(npluralsp) } {
        *p = 2;
    }
}

/// Evaluate a plural expression for a given value of `n`.
///
/// Returns the plural form index (0-indexed).
///
/// This is the standalone (non-_LIBC, non-IN_LIBINTL) implementation.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn plural_eval(pexp: *const expression, n: c_ulong) -> c_ulong {
    if pexp.is_null() {
        return 0;
    }
    // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
    unsafe { plural_eval_internal(pexp, n) }
}

/// Recursive internal evaluator for plural expression trees.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn plural_eval_internal(pexp: *const expression, n: c_ulong) -> c_ulong {
    // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
    let Some(exp) = (unsafe { ptr_ref(pexp) }) else {
        return 0;
    };
    match exp.operation {
        expression_operator::var => n,

        expression_operator::num => exp.val.get_num(),

        expression_operator::lnot => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            if left == 0 { 1 } else { 0 }
        }

        expression_operator::mult => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            left.wrapping_mul(right)
        }

        expression_operator::divide => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            if right == 0 { 0 } else { left / right }
        }

        expression_operator::module => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            if right == 0 { 0 } else { left % right }
        }

        expression_operator::plus => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            left.wrapping_add(right)
        }

        expression_operator::minus => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            left.wrapping_sub(right)
        }

        expression_operator::less_than => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            if left < right { 1 } else { 0 }
        }

        expression_operator::greater_than => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            if left > right { 1 } else { 0 }
        }

        expression_operator::less_or_equal => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            if left <= right { 1 } else { 0 }
        }

        expression_operator::greater_or_equal => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            if left >= right { 1 } else { 0 }
        }

        expression_operator::equal => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            if left == right { 1 } else { 0 }
        }

        expression_operator::not_equal => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            if left != right { 1 } else { 0 }
        }

        expression_operator::land => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            if left != 0 && right != 0 { 1 } else { 0 }
        }

        expression_operator::lor => {
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let left = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let right = unsafe { plural_eval_internal(exp.val.get_args()[1], n) };
            if left != 0 || right != 0 { 1 } else { 0 }
        }

        expression_operator::qmop => {
            // Ternary: condition ? left : right
            // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
            let cond = unsafe { plural_eval_internal(exp.val.get_args()[0], n) };
            if cond != 0 {
                // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
                unsafe { plural_eval_internal(exp.val.get_args()[1], n) }
            } else {
                // SAFETY: forwarded raw pointers satisfy `plural_eval_internal`'s contract per this function's own `# Safety` requirements.
                unsafe { plural_eval_internal(exp.val.get_args()[2], n) }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_germanic_plural_tree() {
        let plural = boxed_germanic_plural();
        assert!(!plural.is_null());
        unsafe {
            let args = (*plural).val.get_args();
            assert_eq!((*args[0]).operation, expression_operator::var);
            assert_eq!((*args[1]).operation, expression_operator::num);
            assert_eq!((*args[1]).val.get_num(), 1);
        }
    }

    #[test]
    fn test_plural_eval_germanic() {
        let plural = boxed_germanic_plural();

        // n=1 -> n!=1 is false -> plural form 0 (singular)
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { plural_eval(plural, 1) };
        assert_eq!(result, 0);

        // n=0 -> n!=1 is true -> plural form 1 (plural)
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { plural_eval(plural, 0) };
        assert_eq!(result, 1);

        // n=2 -> n!=1 is true -> plural form 1 (plural)
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { plural_eval(plural, 2) };
        assert_eq!(result, 1);

        // n=5 -> n!=1 is true -> plural form 1 (plural)
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { plural_eval(plural, 5) };
        assert_eq!(result, 1);
    }

    #[test]
    fn test_plural_eval_null() {
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { plural_eval(ptr::null(), 42) };
        assert_eq!(result, 0);
    }

    #[test]
    fn test_extract_plural_null_entry() {
        let mut pluralp: *const expression = ptr::null();
        let mut npluralsp: c_ulong = 0;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { libintl_gettext_extract_plural(ptr::null(), &mut pluralp, &mut npluralsp) };
        assert_eq!(npluralsp, 2);
        assert!(!pluralp.is_null());
    }

    #[test]
    fn test_extract_plural_no_fields() {
        let entry = b"Project-Id-Version: foo 1.0\n\0".as_ptr() as *const c_char;
        let mut pluralp: *const expression = ptr::null();
        let mut npluralsp: c_ulong = 0;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { libintl_gettext_extract_plural(entry, &mut pluralp, &mut npluralsp) };
        // No plural= or nplurals= fields -> fall back to Germanic.
        assert_eq!(npluralsp, 2);
        assert!(!pluralp.is_null());
    }

    #[test]
    fn test_extract_plural_valid_nplurals() {
        let entry = b"nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\0"
            .as_ptr() as *const c_char;
        let mut pluralp: *const expression = ptr::null();
        let mut npluralsp: c_ulong = 0;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { libintl_gettext_extract_plural(entry, &mut pluralp, &mut npluralsp) };
        assert_eq!(npluralsp, 3);
        assert!(!pluralp.is_null());
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert_eq!(unsafe { plural_eval(pluralp, 1) }, 0);
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert_eq!(unsafe { plural_eval(pluralp, 5) }, 2);
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert_eq!(unsafe { plural_eval(pluralp, 21) }, 0);
    }

    #[test]
    fn test_parse_ulong() {
        unsafe {
            let s = b"42abc\0".as_ptr() as *const c_char;
            let mut endp: *const c_char = ptr::null();
            let n = parse_ulong(s, &mut endp);
            assert_eq!(n, 42);
            assert_eq!(*endp as u8, b'a');
        }
    }
}
