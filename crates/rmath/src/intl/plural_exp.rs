//! Plural expression evaluation for GNU gettext.
//!
//! Ported from `plural-exp.c` in the GNU gettext `intl/` library.
//! Provides the Germanic (default) plural form and expression extraction
//! from the null entry of a .mo file.

#![allow(non_snake_case)]

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_ulong};
use std::ptr;

use super::types::*;

unsafe fn PLURAL_PARSE(_arg: *mut parse_args) -> c_int {
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
unsafe fn parse_ulong(s: *const c_char, endp: &mut *const c_char) -> c_ulong {
    unsafe {
        let mut n: c_ulong = 0;
        let mut p = s;
        while !p.is_null() && *p != 0 {
            let ch = *p as u8;
            if ch < b'0' || ch > b'9' {
                break;
            }
            n = n.wrapping_mul(10).wrapping_add((ch - b'0') as c_ulong);
            p = p.add(1);
        }
        *endp = p;
        n
    }
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
pub unsafe fn libintl_gettext_extract_plural(
    nullentry: *const c_char,
    pluralp: *mut *const expression,
    npluralsp: *mut c_ulong,
) {
    unsafe {
        if !nullentry.is_null() {
            let entry = CStr::from_ptr(nullentry).to_bytes();
            let entry_str = std::str::from_utf8_unchecked(entry);

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
                if offset < bytes.len() && bytes[offset] >= b'0' && bytes[offset] <= b'9' {
                    let mut endp: *const c_char = ptr::null();
                    let n = parse_ulong(nullentry.add(nplurals_start), &mut endp);

                    if endp != nullentry.add(nplurals_start) {
                        *npluralsp = n;

                        // Try to parse the plural expression.
                        let plural_start = plural_p + 7; // skip "plural="
                        let mut args = parse_args {
                            cp: nullentry.add(plural_start),
                            res: ptr::null_mut(),
                        };

                        if PLURAL_PARSE(&mut args) == 0 {
                            *pluralp = args.res;
                            return;
                        }
                    }
                }
            }
        }

        // Fall back to Germanic plural form.
        *pluralp = boxed_germanic_plural();
        *npluralsp = 2;
    }
}

/// Evaluate a plural expression for a given value of `n`.
///
/// Returns the plural form index (0-indexed).
///
/// This is the standalone (non-_LIBC, non-IN_LIBINTL) implementation.
pub unsafe fn plural_eval(pexp: *const expression, n: c_ulong) -> c_ulong {
    unsafe {
        if pexp.is_null() {
            return 0;
        }
        plural_eval_internal(pexp, n)
    }
}

/// Recursive internal evaluator for plural expression trees.
unsafe fn plural_eval_internal(pexp: *const expression, n: c_ulong) -> c_ulong {
    unsafe {
        let exp = &*pexp;
        match exp.operation {
            expression_operator::var => n,

            expression_operator::num => exp.val.get_num(),

            expression_operator::lnot => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                if left == 0 { 1 } else { 0 }
            }

            expression_operator::mult => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                left.wrapping_mul(right)
            }

            expression_operator::divide => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                if right == 0 { 0 } else { left / right }
            }

            expression_operator::module => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                if right == 0 { 0 } else { left % right }
            }

            expression_operator::plus => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                left.wrapping_add(right)
            }

            expression_operator::minus => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                left.wrapping_sub(right)
            }

            expression_operator::less_than => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                if left < right { 1 } else { 0 }
            }

            expression_operator::greater_than => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                if left > right { 1 } else { 0 }
            }

            expression_operator::less_or_equal => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                if left <= right { 1 } else { 0 }
            }

            expression_operator::greater_or_equal => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                if left >= right { 1 } else { 0 }
            }

            expression_operator::equal => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                if left == right { 1 } else { 0 }
            }

            expression_operator::not_equal => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                if left != right { 1 } else { 0 }
            }

            expression_operator::land => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                if left != 0 && right != 0 { 1 } else { 0 }
            }

            expression_operator::lor => {
                let left = plural_eval_internal(exp.val.get_args()[0], n);
                let right = plural_eval_internal(exp.val.get_args()[1], n);
                if left != 0 || right != 0 { 1 } else { 0 }
            }

            expression_operator::qmop => {
                // Ternary: condition ? left : right
                let cond = plural_eval_internal(exp.val.get_args()[0], n);
                if cond != 0 {
                    plural_eval_internal(exp.val.get_args()[1], n)
                } else {
                    plural_eval_internal(exp.val.get_args()[2], n)
                }
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
        let result = unsafe { plural_eval(plural, 1) };
        assert_eq!(result, 0);

        // n=0 -> n!=1 is true -> plural form 1 (plural)
        let result = unsafe { plural_eval(plural, 0) };
        assert_eq!(result, 1);

        // n=2 -> n!=1 is true -> plural form 1 (plural)
        let result = unsafe { plural_eval(plural, 2) };
        assert_eq!(result, 1);

        // n=5 -> n!=1 is true -> plural form 1 (plural)
        let result = unsafe { plural_eval(plural, 5) };
        assert_eq!(result, 1);
    }

    #[test]
    fn test_plural_eval_null() {
        unsafe {
            let result = plural_eval(ptr::null(), 42);
            assert_eq!(result, 0);
        }
    }

    #[test]
    fn test_extract_plural_null_entry() {
        unsafe {
            let mut pluralp: *const expression = ptr::null();
            let mut npluralsp: c_ulong = 0;
            libintl_gettext_extract_plural(ptr::null(), &mut pluralp, &mut npluralsp);
            assert_eq!(npluralsp, 2);
            assert!(!pluralp.is_null());
        }
    }

    #[test]
    fn test_extract_plural_no_fields() {
        unsafe {
            let entry = b"Project-Id-Version: foo 1.0\n\0".as_ptr() as *const c_char;
            let mut pluralp: *const expression = ptr::null();
            let mut npluralsp: c_ulong = 0;
            libintl_gettext_extract_plural(entry, &mut pluralp, &mut npluralsp);
            // No plural= or nplurals= fields -> fall back to Germanic.
            assert_eq!(npluralsp, 2);
            assert!(!pluralp.is_null());
        }
    }

    #[test]
    fn test_extract_plural_valid_nplurals() {
        unsafe {
            let entry = b"nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\0"
                .as_ptr() as *const c_char;
            let mut pluralp: *const expression = ptr::null();
            let mut npluralsp: c_ulong = 0;
            libintl_gettext_extract_plural(entry, &mut pluralp, &mut npluralsp);
            assert_eq!(npluralsp, 3);
            assert!(!pluralp.is_null());
            assert_eq!(plural_eval(pluralp, 1), 0);
            assert_eq!(plural_eval(pluralp, 5), 2);
            assert_eq!(plural_eval(pluralp, 21), 0);
        }
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
