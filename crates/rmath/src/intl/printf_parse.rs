//! Port of printf-parse.c -- Printf format string parsing.
//!
//! Parses a printf format string into a structured representation
//! (`char_directives`) and extracts argument types (`arguments`).
//!
//! This handles the POSIX/XSI format string extensions with positional
//! arguments (e.g., "%2$d", "%1$s") used by gettext.

#![allow(non_snake_case)]

use std::alloc::{self, Layout};
use std::os::raw::{c_char, c_int};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Printf directive flags (matching printf-parse.h)
// ---------------------------------------------------------------------------

pub(crate) const FLAG_GROUP: c_int = 1; /* ' flag */
pub(crate) const FLAG_LEFT: c_int = 2; /* - flag */
pub(crate) const FLAG_SHOWSIGN: c_int = 4; /* + flag */
pub(crate) const FLAG_SPACE: c_int = 8; /* space flag */
pub(crate) const FLAG_ALT: c_int = 16; /* # flag */
pub(crate) const FLAG_ZERO: c_int = 32; /* 0 flag */

/// Sentinel value indicating no argument is consumed.
pub(crate) const ARG_NONE: usize = usize::MAX;

// ---------------------------------------------------------------------------
// Directive and directives types (matching printf-parse.h)
// ---------------------------------------------------------------------------

/// A parsed printf directive.
#[repr(C)]
pub(crate) struct char_directive {
    pub dir_start: *const c_char,
    pub dir_end: *const c_char,
    pub flags: c_int,
    pub width_start: *const c_char,
    pub width_end: *const c_char,
    pub width_arg_index: usize,
    pub precision_start: *const c_char,
    pub precision_end: *const c_char,
    pub precision_arg_index: usize,
    pub conversion: c_char,
    pub arg_index: usize,
}

/// A set of parsed directives from a format string.
#[repr(C)]
pub(crate) struct char_directives {
    pub count: usize,
    pub dir: *mut char_directive,
    pub max_width_length: usize,
    pub max_precision_length: usize,
}

// ---------------------------------------------------------------------------
// Checked size_t computations (matching xsize.h)
// ---------------------------------------------------------------------------

const SIZE_MAX: usize = usize::MAX;

#[inline]
fn xsum(a: usize, b: usize) -> usize {
    let sum = a.wrapping_add(b);
    if sum >= a { sum } else { SIZE_MAX }
}

#[inline]
fn xtimes(n: usize, elsize: usize) -> usize {
    if elsize == 0 {
        return 0;
    }
    if n <= SIZE_MAX / elsize {
        n * elsize
    } else {
        SIZE_MAX
    }
}

#[inline]
fn size_overflow_p(size: usize) -> bool {
    size == SIZE_MAX
}

// ---------------------------------------------------------------------------
// Public API: printf_parse
// ---------------------------------------------------------------------------

/// Parse a printf format string.
///
/// Fills in the directives array and the argument types. Returns 0 on success,
/// -1 on error (invalid format or out of memory).
///
/// # Safety
/// `format` must be a valid pointer to a NUL-terminated C string.
/// `d` and `a` must be valid pointers.
pub unsafe fn printf_parse(
    format: *const c_char,
    d: *mut char_directives,
    a: *mut arguments,
) -> c_int {
    unsafe {
        if format.is_null() || d.is_null() || a.is_null() {
            return -1;
        }

        let mut cp = format;
        let mut arg_posn: usize = 0;
        let mut d_allocated: usize = 1;
        let mut a_allocated: usize = 0;
        let mut max_width_length: usize = 0;
        let mut max_precision_length: usize = 0;

        (*d).count = 0;
        (*d).dir = alloc::alloc(
            Layout::from_size_align(d_allocated * std::mem::size_of::<char_directive>(), 1)
                .unwrap_or(Layout::new::<u8>()),
        ) as *mut char_directive;
        if (*d).dir.is_null() {
            return -1;
        }

        (*a).count = 0;
        (*a).arg = ptr::null_mut();

        // Helper macro inlined: register argument type at given index.
        let mut register_arg = |index: usize, atype: arg_type| -> bool {
            let n = index;
            if n >= a_allocated {
                let new_alloc = if xtimes(a_allocated, 2) <= n {
                    n + 1
                } else {
                    xtimes(a_allocated, 2)
                };
                a_allocated = new_alloc;
                let mem_size = xtimes(a_allocated, std::mem::size_of::<argument>());
                if size_overflow_p(mem_size) {
                    return false;
                }
                let memory = if (*a).arg.is_null() {
                    alloc::alloc(
                        Layout::from_size_align(mem_size, 1).unwrap_or(Layout::new::<u8>()),
                    ) as *mut argument
                } else {
                    alloc::realloc(
                        (*a).arg as *mut u8,
                        Layout::from_size_align(mem_size, 1).unwrap_or(Layout::new::<u8>()),
                        mem_size,
                    ) as *mut argument
                };
                if memory.is_null() {
                    return false;
                }
                (*a).arg = memory;
            }
            while (*a).count <= n {
                (*(*a).arg.add((*a).count)).type_ = arg_type::TYPE_NONE;
                (*a).count += 1;
            }
            if (*(*a).arg.add(n)).type_ == arg_type::TYPE_NONE {
                (*(*a).arg.add(n)).type_ = atype;
            } else if (*(*a).arg.add(n)).type_ != atype {
                return false; // Ambiguous type.
            }
            true
        };

        while *cp != 0 {
            let mut c = *cp;
            cp = cp.add(1);
            if c == b'%' as c_char {
                let mut arg_index: usize = ARG_NONE;
                let dp = &mut *(*d).dir.add((*d).count);

                // Initialize the directive.
                dp.dir_start = cp.sub(1);
                dp.flags = 0;
                dp.width_start = ptr::null();
                dp.width_end = ptr::null();
                dp.width_arg_index = ARG_NONE;
                dp.precision_start = ptr::null();
                dp.precision_end = ptr::null();
                dp.precision_arg_index = ARG_NONE;
                dp.arg_index = ARG_NONE;

                // Test for positional argument.
                if *cp >= b'0' as c_char && *cp <= b'9' as c_char {
                    let mut np = cp;
                    while *np >= b'0' as c_char && *np <= b'9' as c_char {
                        np = np.add(1);
                    }
                    if *np == b'$' as c_char {
                        let mut n: usize = 0;
                        let mut np2 = cp;
                        while *np2 >= b'0' as c_char && *np2 <= b'9' as c_char {
                            n = xsum(xtimes(n, 10), (*np2 as u8 - b'0') as usize);
                            np2 = np2.add(1);
                        }
                        if n == 0 || size_overflow_p(n) {
                            // Error cleanup.
                            if !(*a).arg.is_null() {
                                alloc::dealloc(
                                    (*a).arg as *mut u8,
                                    Layout::from_size_align_unchecked(
                                        a_allocated * std::mem::size_of::<argument>(),
                                        1,
                                    ),
                                );
                            }
                            if !(*d).dir.is_null() {
                                alloc::dealloc(
                                    (*d).dir as *mut u8,
                                    Layout::from_size_align_unchecked(
                                        d_allocated * std::mem::size_of::<char_directive>(),
                                        1,
                                    ),
                                );
                            }
                            return -1;
                        }
                        arg_index = n - 1;
                        cp = np.add(1);
                    }
                }

                // Read the flags.
                loop {
                    match *cp {
                        x if x == b'\'' as c_char => {
                            dp.flags |= FLAG_GROUP;
                            cp = cp.add(1);
                        }
                        x if x == b'-' as c_char => {
                            dp.flags |= FLAG_LEFT;
                            cp = cp.add(1);
                        }
                        x if x == b'+' as c_char => {
                            dp.flags |= FLAG_SHOWSIGN;
                            cp = cp.add(1);
                        }
                        x if x == b' ' as c_char => {
                            dp.flags |= FLAG_SPACE;
                            cp = cp.add(1);
                        }
                        x if x == b'#' as c_char => {
                            dp.flags |= FLAG_ALT;
                            cp = cp.add(1);
                        }
                        x if x == b'0' as c_char => {
                            dp.flags |= FLAG_ZERO;
                            cp = cp.add(1);
                        }
                        _ => break,
                    }
                }

                // Parse the field width.
                if *cp == b'*' as c_char {
                    dp.width_start = cp;
                    cp = cp.add(1);
                    dp.width_end = cp;
                    if max_width_length < 1 {
                        max_width_length = 1;
                    }
                    // Test for positional argument for width.
                    if *cp >= b'0' as c_char && *cp <= b'9' as c_char {
                        let mut np = cp;
                        while *np >= b'0' as c_char && *np <= b'9' as c_char {
                            np = np.add(1);
                        }
                        if *np == b'$' as c_char {
                            let mut n: usize = 0;
                            let mut np2 = cp;
                            while *np2 >= b'0' as c_char && *np2 <= b'9' as c_char {
                                n = xsum(xtimes(n, 10), (*np2 as u8 - b'0') as usize);
                                np2 = np2.add(1);
                            }
                            if n == 0 || size_overflow_p(n) {
                                if !(*a).arg.is_null() {
                                    alloc::dealloc(
                                        (*a).arg as *mut u8,
                                        Layout::from_size_align_unchecked(
                                            a_allocated * std::mem::size_of::<argument>(),
                                            1,
                                        ),
                                    );
                                }
                                if !(*d).dir.is_null() {
                                    alloc::dealloc(
                                        (*d).dir as *mut u8,
                                        Layout::from_size_align_unchecked(
                                            d_allocated * std::mem::size_of::<char_directive>(),
                                            1,
                                        ),
                                    );
                                }
                                return -1;
                            }
                            dp.width_arg_index = n - 1;
                            cp = np.add(1);
                        }
                    }
                    if dp.width_arg_index == ARG_NONE {
                        dp.width_arg_index = arg_posn;
                        arg_posn = arg_posn.wrapping_add(1);
                        if arg_posn == ARG_NONE {
                            if !(*a).arg.is_null() {
                                alloc::dealloc(
                                    (*a).arg as *mut u8,
                                    Layout::from_size_align_unchecked(
                                        a_allocated * std::mem::size_of::<argument>(),
                                        1,
                                    ),
                                );
                            }
                            if !(*d).dir.is_null() {
                                alloc::dealloc(
                                    (*d).dir as *mut u8,
                                    Layout::from_size_align_unchecked(
                                        d_allocated * std::mem::size_of::<char_directive>(),
                                        1,
                                    ),
                                );
                            }
                            return -1;
                        }
                    }
                    if !register_arg(dp.width_arg_index, arg_type::TYPE_INT) {
                        if !(*a).arg.is_null() {
                            alloc::dealloc(
                                (*a).arg as *mut u8,
                                Layout::from_size_align_unchecked(
                                    a_allocated * std::mem::size_of::<argument>(),
                                    1,
                                ),
                            );
                        }
                        if !(*d).dir.is_null() {
                            alloc::dealloc(
                                (*d).dir as *mut u8,
                                Layout::from_size_align_unchecked(
                                    d_allocated * std::mem::size_of::<char_directive>(),
                                    1,
                                ),
                            );
                        }
                        return -1;
                    }
                } else if *cp >= b'0' as c_char && *cp <= b'9' as c_char {
                    dp.width_start = cp;
                    while *cp >= b'0' as c_char && *cp <= b'9' as c_char {
                        cp = cp.add(1);
                    }
                    dp.width_end = cp;
                    let width_length =
                        (dp.width_end as usize).wrapping_sub(dp.width_start as usize);
                    if max_width_length < width_length {
                        max_width_length = width_length;
                    }
                }

                // Parse the precision.
                if *cp == b'.' as c_char {
                    cp = cp.add(1);
                    if *cp == b'*' as c_char {
                        dp.precision_start = cp.sub(1);
                        cp = cp.add(1);
                        dp.precision_end = cp;
                        if max_precision_length < 2 {
                            max_precision_length = 2;
                        }
                        // Test for positional argument for precision.
                        if *cp >= b'0' as c_char && *cp <= b'9' as c_char {
                            let mut np = cp;
                            while *np >= b'0' as c_char && *np <= b'9' as c_char {
                                np = np.add(1);
                            }
                            if *np == b'$' as c_char {
                                let mut n: usize = 0;
                                let mut np2 = cp;
                                while *np2 >= b'0' as c_char && *np2 <= b'9' as c_char {
                                    n = xsum(xtimes(n, 10), (*np2 as u8 - b'0') as usize);
                                    np2 = np2.add(1);
                                }
                                if n == 0 || size_overflow_p(n) {
                                    if !(*a).arg.is_null() {
                                        alloc::dealloc(
                                            (*a).arg as *mut u8,
                                            Layout::from_size_align_unchecked(
                                                a_allocated * std::mem::size_of::<argument>(),
                                                1,
                                            ),
                                        );
                                    }
                                    if !(*d).dir.is_null() {
                                        alloc::dealloc(
                                            (*d).dir as *mut u8,
                                            Layout::from_size_align_unchecked(
                                                d_allocated * std::mem::size_of::<char_directive>(),
                                                1,
                                            ),
                                        );
                                    }
                                    return -1;
                                }
                                dp.precision_arg_index = n - 1;
                                cp = np.add(1);
                            }
                        }
                        if dp.precision_arg_index == ARG_NONE {
                            dp.precision_arg_index = arg_posn;
                            arg_posn = arg_posn.wrapping_add(1);
                            if arg_posn == ARG_NONE {
                                if !(*a).arg.is_null() {
                                    alloc::dealloc(
                                        (*a).arg as *mut u8,
                                        Layout::from_size_align_unchecked(
                                            a_allocated * std::mem::size_of::<argument>(),
                                            1,
                                        ),
                                    );
                                }
                                if !(*d).dir.is_null() {
                                    alloc::dealloc(
                                        (*d).dir as *mut u8,
                                        Layout::from_size_align_unchecked(
                                            d_allocated * std::mem::size_of::<char_directive>(),
                                            1,
                                        ),
                                    );
                                }
                                return -1;
                            }
                        }
                        if !register_arg(dp.precision_arg_index, arg_type::TYPE_INT) {
                            if !(*a).arg.is_null() {
                                alloc::dealloc(
                                    (*a).arg as *mut u8,
                                    Layout::from_size_align_unchecked(
                                        a_allocated * std::mem::size_of::<argument>(),
                                        1,
                                    ),
                                );
                            }
                            if !(*d).dir.is_null() {
                                alloc::dealloc(
                                    (*d).dir as *mut u8,
                                    Layout::from_size_align_unchecked(
                                        d_allocated * std::mem::size_of::<char_directive>(),
                                        1,
                                    ),
                                );
                            }
                            return -1;
                        }
                    } else {
                        dp.precision_start = cp.sub(1);
                        while *cp >= b'0' as c_char && *cp <= b'9' as c_char {
                            cp = cp.add(1);
                        }
                        dp.precision_end = cp;
                        let precision_length =
                            (dp.precision_end as usize).wrapping_sub(dp.precision_start as usize);
                        if max_precision_length < precision_length {
                            max_precision_length = precision_length;
                        }
                    }
                }

                // Parse argument type/size specifiers.
                let mut flags: c_int = 0;
                loop {
                    match *cp {
                        x if x == b'h' as c_char => {
                            flags |= 1 << (flags & 1);
                            cp = cp.add(1);
                        }
                        x if x == b'L' as c_char => {
                            flags |= 4;
                            cp = cp.add(1);
                        }
                        x if x == b'l' as c_char => {
                            flags += 8;
                            cp = cp.add(1);
                        }
                        x if x == b'j' as c_char => {
                            if std::mem::size_of::<i64>() > std::mem::size_of::<i64>() {
                                flags += 16;
                            } else if std::mem::size_of::<i64>() > std::mem::size_of::<c_int>() {
                                flags += 8;
                            }
                            cp = cp.add(1);
                        }
                        x if x == b'z' as c_char || x == b'Z' as c_char => {
                            if std::mem::size_of::<usize>() > std::mem::size_of::<i64>() {
                                flags += 16;
                            } else if std::mem::size_of::<usize>() > std::mem::size_of::<c_int>() {
                                flags += 8;
                            }
                            cp = cp.add(1);
                        }
                        x if x == b't' as c_char => {
                            if std::mem::size_of::<isize>() > std::mem::size_of::<i64>() {
                                flags += 16;
                            } else if std::mem::size_of::<isize>() > std::mem::size_of::<c_int>() {
                                flags += 8;
                            }
                            cp = cp.add(1);
                        }
                        _ => break,
                    }
                }

                // Read the conversion character.
                c = *cp;
                cp = cp.add(1);

                let atype = match c as u8 {
                    b'd' | b'i' => {
                        if flags >= 16 || (flags & 4) != 0 {
                            arg_type::TYPE_LONGLONGINT
                        } else if flags >= 8 {
                            arg_type::TYPE_LONGINT
                        } else if (flags & 2) != 0 {
                            arg_type::TYPE_SCHAR
                        } else if (flags & 1) != 0 {
                            arg_type::TYPE_SHORT
                        } else {
                            arg_type::TYPE_INT
                        }
                    }
                    b'o' | b'u' | b'x' | b'X' => {
                        if flags >= 16 || (flags & 4) != 0 {
                            arg_type::TYPE_ULONGLONGINT
                        } else if flags >= 8 {
                            arg_type::TYPE_ULONGINT
                        } else if (flags & 2) != 0 {
                            arg_type::TYPE_UCHAR
                        } else if (flags & 1) != 0 {
                            arg_type::TYPE_USHORT
                        } else {
                            arg_type::TYPE_UINT
                        }
                    }
                    b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => {
                        if flags >= 16 || (flags & 4) != 0 {
                            arg_type::TYPE_LONGDOUBLE
                        } else {
                            arg_type::TYPE_DOUBLE
                        }
                    }
                    b'c' => {
                        if flags >= 8 {
                            arg_type::TYPE_WIDE_CHAR
                        } else {
                            arg_type::TYPE_CHAR
                        }
                    }
                    b's' => {
                        if flags >= 8 {
                            arg_type::TYPE_WIDE_STRING
                        } else {
                            arg_type::TYPE_STRING
                        }
                    }
                    b'p' => arg_type::TYPE_POINTER,
                    b'n' => {
                        if flags >= 16 || (flags & 4) != 0 {
                            arg_type::TYPE_COUNT_LONGLONGINT_POINTER
                        } else if flags >= 8 {
                            arg_type::TYPE_COUNT_LONGINT_POINTER
                        } else if (flags & 2) != 0 {
                            arg_type::TYPE_COUNT_SCHAR_POINTER
                        } else if (flags & 1) != 0 {
                            arg_type::TYPE_COUNT_SHORT_POINTER
                        } else {
                            arg_type::TYPE_COUNT_INT_POINTER
                        }
                    }
                    b'%' => arg_type::TYPE_NONE,
                    _ => {
                        // Unknown conversion character.
                        if !(*a).arg.is_null() {
                            alloc::dealloc(
                                (*a).arg as *mut u8,
                                Layout::from_size_align_unchecked(
                                    a_allocated * std::mem::size_of::<argument>(),
                                    1,
                                ),
                            );
                        }
                        if !(*d).dir.is_null() {
                            alloc::dealloc(
                                (*d).dir as *mut u8,
                                Layout::from_size_align_unchecked(
                                    d_allocated * std::mem::size_of::<char_directive>(),
                                    1,
                                ),
                            );
                        }
                        return -1;
                    }
                };

                if atype != arg_type::TYPE_NONE {
                    dp.arg_index = arg_index;
                    if dp.arg_index == ARG_NONE {
                        dp.arg_index = arg_posn;
                        arg_posn = arg_posn.wrapping_add(1);
                        if dp.arg_index == ARG_NONE {
                            if !(*a).arg.is_null() {
                                alloc::dealloc(
                                    (*a).arg as *mut u8,
                                    Layout::from_size_align_unchecked(
                                        a_allocated * std::mem::size_of::<argument>(),
                                        1,
                                    ),
                                );
                            }
                            if !(*d).dir.is_null() {
                                alloc::dealloc(
                                    (*d).dir as *mut u8,
                                    Layout::from_size_align_unchecked(
                                        d_allocated * std::mem::size_of::<char_directive>(),
                                        1,
                                    ),
                                );
                            }
                            return -1;
                        }
                    }
                    if !register_arg(dp.arg_index, atype) {
                        if !(*a).arg.is_null() {
                            alloc::dealloc(
                                (*a).arg as *mut u8,
                                Layout::from_size_align_unchecked(
                                    a_allocated * std::mem::size_of::<argument>(),
                                    1,
                                ),
                            );
                        }
                        if !(*d).dir.is_null() {
                            alloc::dealloc(
                                (*d).dir as *mut u8,
                                Layout::from_size_align_unchecked(
                                    d_allocated * std::mem::size_of::<char_directive>(),
                                    1,
                                ),
                            );
                        }
                        return -1;
                    }
                }
                dp.conversion = c;
                dp.dir_end = cp;

                (*d).count += 1;
                if (*d).count >= d_allocated {
                    d_allocated = xtimes(d_allocated, 2);
                    let mem_size = xtimes(d_allocated, std::mem::size_of::<char_directive>());
                    if size_overflow_p(mem_size) {
                        if !(*a).arg.is_null() {
                            alloc::dealloc(
                                (*a).arg as *mut u8,
                                Layout::from_size_align_unchecked(
                                    a_allocated * std::mem::size_of::<argument>(),
                                    1,
                                ),
                            );
                        }
                        if !(*d).dir.is_null() {
                            alloc::dealloc(
                                (*d).dir as *mut u8,
                                Layout::from_size_align_unchecked(
                                    d_allocated * std::mem::size_of::<char_directive>(),
                                    1,
                                ),
                            );
                        }
                        return -1;
                    }
                    let memory = alloc::realloc(
                        (*d).dir as *mut u8,
                        Layout::from_size_align(mem_size, 1).unwrap_or(Layout::new::<u8>()),
                        mem_size,
                    ) as *mut char_directive;
                    if memory.is_null() {
                        if !(*a).arg.is_null() {
                            alloc::dealloc(
                                (*a).arg as *mut u8,
                                Layout::from_size_align_unchecked(
                                    a_allocated * std::mem::size_of::<argument>(),
                                    1,
                                ),
                            );
                        }
                        if !(*d).dir.is_null() {
                            alloc::dealloc(
                                (*d).dir as *mut u8,
                                Layout::from_size_align_unchecked(
                                    d_allocated * std::mem::size_of::<char_directive>(),
                                    1,
                                ),
                            );
                        }
                        return -1;
                    }
                    (*d).dir = memory;
                }
            }
        }

        // Set the sentinel.
        if !(*d).dir.is_null() {
            (*(*d).dir.add((*d).count)).dir_start = cp;
        }

        (*d).max_width_length = max_width_length;
        (*d).max_precision_length = max_precision_length;
        0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_string() {
        unsafe {
            let fmt = b"hello world\0".as_ptr() as *const c_char;
            let mut dirs = char_directives {
                count: 0,
                dir: ptr::null_mut(),
                max_width_length: 0,
                max_precision_length: 0,
            };
            let mut args = arguments {
                count: 0,
                arg: ptr::null_mut(),
            };
            let result = printf_parse(fmt, &mut dirs, &mut args);
            assert_eq!(result, 0);
            assert_eq!(dirs.count, 0);
            assert_eq!(args.count, 0);
        }
    }

    #[test]
    fn test_parse_simple_format() {
        unsafe {
            let fmt = b"hello %s\0".as_ptr() as *const c_char;
            let mut dirs = char_directives {
                count: 0,
                dir: ptr::null_mut(),
                max_width_length: 0,
                max_precision_length: 0,
            };
            let mut args = arguments {
                count: 0,
                arg: ptr::null_mut(),
            };
            let result = printf_parse(fmt, &mut dirs, &mut args);
            assert_eq!(result, 0);
            assert_eq!(dirs.count, 1);
            assert_eq!(args.count, 1);
            assert_eq!((*args.arg.add(0)).type_, arg_type::TYPE_STRING);
        }
    }

    #[test]
    fn test_parse_positional() {
        unsafe {
            let fmt = b"%1$s %2$d\0".as_ptr() as *const c_char;
            let mut dirs = char_directives {
                count: 0,
                dir: ptr::null_mut(),
                max_width_length: 0,
                max_precision_length: 0,
            };
            let mut args = arguments {
                count: 0,
                arg: ptr::null_mut(),
            };
            let result = printf_parse(fmt, &mut dirs, &mut args);
            assert_eq!(result, 0);
            assert_eq!(dirs.count, 2);
            assert_eq!(args.count, 2);
            assert_eq!((*args.arg.add(0)).type_, arg_type::TYPE_STRING);
            assert_eq!((*args.arg.add(1)).type_, arg_type::TYPE_INT);
        }
    }

    #[test]
    fn test_parse_width_and_precision() {
        unsafe {
            let fmt = b"%10.5d\0".as_ptr() as *const c_char;
            let mut dirs = char_directives {
                count: 0,
                dir: ptr::null_mut(),
                max_width_length: 0,
                max_precision_length: 0,
            };
            let mut args = arguments {
                count: 0,
                arg: ptr::null_mut(),
            };
            let result = printf_parse(fmt, &mut dirs, &mut args);
            assert_eq!(result, 0);
            assert_eq!(dirs.count, 1);
            assert_eq!(dirs.max_width_length, 2);
        }
    }

    #[test]
    fn test_parse_flags() {
        unsafe {
            let fmt = b"%-+10d\0".as_ptr() as *const c_char;
            let mut dirs = char_directives {
                count: 0,
                dir: ptr::null_mut(),
                max_width_length: 0,
                max_precision_length: 0,
            };
            let mut args = arguments {
                count: 0,
                arg: ptr::null_mut(),
            };
            let result = printf_parse(fmt, &mut dirs, &mut args);
            assert_eq!(result, 0);
            assert_eq!(dirs.count, 1);
            let flags = (*dirs.dir.add(0)).flags;
            assert_ne!(flags & FLAG_LEFT, 0);
            assert_ne!(flags & FLAG_SHOWSIGN, 0);
        }
    }
}
