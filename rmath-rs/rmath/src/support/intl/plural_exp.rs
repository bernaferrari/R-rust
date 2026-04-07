//! Plural expression evaluation for GNU gettext.
//!
//! Ported from `plural-exp.c` in the GNU gettext `intl/` library.
//! Provides the Germanic (default) plural form and expression extraction
//! from the null entry of a .mo file.

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_ulong};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Static Germanic plural expression
// ---------------------------------------------------------------------------

/// Represents the variable `n` in plural expressions.
thread_local! { static plvar: RefCell<expression> = RefCell::new(expression {
    nargs: 0,
    operation: expression_operator::var,
    val: expression_val { num: 0 },
}); }

thread_local! { static plone: RefCell<expression> = RefCell::new(expression {
    nargs: 0,
    operation: expression_operator::num,
    val: expression_val { num: 1 },
}); }

thread_local! { static GERMANIC_PLURAL: RefCell<expression> = RefCell::new(expression {
    nargs: 2,
    operation: expression_operator::not_equal,
    val: expression_val {
        args: [ptr::null_mut(), ptr::null_mut(), ptr::null_mut()],
    },
}); }

/// Ensure the static Germanic plural expression is initialized.
///
/// This replaces the C99 designated-initializer approach and the runtime
/// `init_germanic_plural()` fallback, using a simple initialization guard.
fn init_germanic_plural() {
    GERMANIC_PLURAL.with(|ger| {
        let g = ger.borrow();
        if g.val.args[0].is_null() {
            drop(g);
            plvar.with(|pv| {
                let mut p = pv.borrow_mut();
                p.nargs = 0;
                p.operation = expression_operator::var;
            });
            plone.with(|po| {
                let mut p = po.borrow_mut();
                p.nargs = 0;
                p.operation = expression_operator::num;
                p.val = expression_val { num: 1 };
            });
            let plvar_ptr: *mut expression =
                plvar.with(|v| &*v.borrow() as *const expression as *mut expression);
            let plone_ptr: *mut expression =
                plone.with(|v| &*v.borrow() as *const expression as *mut expression);
            let mut g = ger.borrow_mut();
            g.nargs = 2;
            g.operation = expression_operator::not_equal;
            g.val = expression_val {
                args: [plvar_ptr, plone_ptr, ptr::null_mut()],
            };
        }
    });
}

// ---------------------------------------------------------------------------
// Plural expression parser stub
// ---------------------------------------------------------------------------

/// Plural expression parser (stub).
///
/// In the full C implementation this calls into a bison-generated parser
/// (`libintl_gettextparse`). For the standalone port we provide a stub
/// that always returns failure (-1), causing the caller to fall back to
/// the Germanic plural form.
///
/// Returns 0 on success, non-zero on parse error.
unsafe extern "C" fn PLURAL_PARSE(_arg: *mut parse_args) -> c_int {
    // Stub: indicate parse failure so the caller uses Germanic plural.
    -1
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn libintl_gettext_extract_plural(
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

            if plural_pos.is_some() && nplurals_pos.is_some() {
                let nplurals_start = nplurals_pos.expect("unwrap on None/Err") + 9; // skip "nplurals="

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
                        let plural_start = plural_pos.expect("unwrap on None/Err") + 7; // skip "plural="
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
        init_germanic_plural();
        *pluralp = GERMANIC_PLURAL.with(|v| &*v.borrow() as *const expression);
        *npluralsp = 2;
    }
}

/// Evaluate a plural expression for a given value of `n`.
///
/// Returns the plural form index (0-indexed).
///
/// This is the standalone (non-_LIBC, non-IN_LIBINTL) implementation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plural_eval(pexp: *const expression, n: c_ulong) -> c_ulong {
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
    fn test_germanic_plural_init() {
        init_germanic_plural();
        // After initialization, args[0] should point to plvar.
        GERMANIC_PLURAL.with(|g| {
            let g = g.borrow();
            assert!(!g.val.args[0].is_null());
            // plvar should be the variable operator.
            assert_eq!((*g.val.args[0]).operation, expression_operator::var);
        });
    }

    #[test]
    fn test_plural_eval_germanic() {
        init_germanic_plural();

        // n=1 -> n!=1 is false -> plural form 0 (singular)
        let result = unsafe {
            plural_eval(
                GERMANIC_PLURAL.with(|v| &*v.borrow() as *const expression),
                1,
            )
        };
        assert_eq!(result, 0);

        // n=0 -> n!=1 is true -> plural form 1 (plural)
        let result = unsafe {
            plural_eval(
                GERMANIC_PLURAL.with(|v| &*v.borrow() as *const expression),
                0,
            )
        };
        assert_eq!(result, 1);

        // n=2 -> n!=1 is true -> plural form 1 (plural)
        let result = unsafe {
            plural_eval(
                GERMANIC_PLURAL.with(|v| &*v.borrow() as *const expression),
                2,
            )
        };
        assert_eq!(result, 1);

        // n=5 -> n!=1 is true -> plural form 1 (plural)
        let result = unsafe {
            plural_eval(
                GERMANIC_PLURAL.with(|v| &*v.borrow() as *const expression),
                5,
            )
        };
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
            // Parser is a stub so it will fail and fall back to Germanic.
            // The C code does the same: if PLURAL_PARSE fails, goto no_plural
            // which resets npluralsp to 2.
            assert_eq!(npluralsp, 2);
            assert!(!pluralp.is_null());
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
