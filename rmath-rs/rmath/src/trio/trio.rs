/*!
 * Port of R's trio.c - Portable printf/scanf implementation.
 *
 * Original copyright (C) 1998, 2009 Bjorn Reese and Daniel Stenberg.
 * BSD-style license.
 *
 * Modified for R to support round-to-even and buffered output for
 * multi-byte characters.
 *
 * This Rust port uses closures/traits for output instead of raw function
 * pointers, and uses `std::io::Write` where appropriate.
 */

#![allow(non_camel_case_types, unused_assignments, unused_variables)]

use std::io::Write;
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::ptr;

// Re-export from sub-modules
pub use crate::trio::trionan::*;
pub use crate::trio::triostr;

// ============================================================================
// Type definitions (ported from triodef.h)
// ============================================================================

/// Long double type alias.
pub type trio_long_double_t = f64;

/// Pointer type.
type trio_pointer_t = *mut c_void;

/// Flags type.
type trio_flags_t = u64;

/// Maximal signed integer type.
type trio_intmax_t = i64;

/// Maximal unsigned integer type.
type trio_uintmax_t = u64;

// ============================================================================
// Error codes
// ============================================================================

const TRIO_EINVAL: c_int = 2;
const TRIO_ETOOMANY: c_int = 3;
const TRIO_EDBLREF: c_int = 4;

#[inline(always)]
fn trio_error_return(code: c_int, _pos: c_int) -> c_int {
    -code
}

// ============================================================================
// Format type constants
// ============================================================================

const TYPE_PRINT: c_int = 1;
const TYPE_SCAN: c_int = 2;

const FORMAT_SENTINEL: c_int = -1;
const FORMAT_UNKNOWN: c_int = 0;
const FORMAT_INT: c_int = 1;
const FORMAT_DOUBLE: c_int = 2;
const FORMAT_CHAR: c_int = 3;
const FORMAT_STRING: c_int = 4;
const FORMAT_POINTER: c_int = 5;
const FORMAT_COUNT: c_int = 6;
const FORMAT_PARAMETER: c_int = 7;
const FORMAT_GROUP: c_int = 8;
const FORMAT_ERRNO: c_int = 9;

// ============================================================================
// Flags
// ============================================================================

const FLAGS_NEW: trio_flags_t = 0;
const FLAGS_STICKY: trio_flags_t = 1;
const FLAGS_SPACE: trio_flags_t = 2;
const FLAGS_SHOWSIGN: trio_flags_t = 4;
const FLAGS_LEFTADJUST: trio_flags_t = 8;
const FLAGS_ALTERNATIVE: trio_flags_t = 16;
const FLAGS_SHORT: trio_flags_t = 32;
const FLAGS_SHORTSHORT: trio_flags_t = 64;
const FLAGS_LONG: trio_flags_t = 128;
const FLAGS_QUAD: trio_flags_t = 256;
const FLAGS_LONGDOUBLE: trio_flags_t = 512;
const FLAGS_SIZE_T: trio_flags_t = 1024;
const FLAGS_PTRDIFF_T: trio_flags_t = 2048;
const FLAGS_INTMAX_T: trio_flags_t = 4096;
const FLAGS_NILPADDING: trio_flags_t = 8192;
const FLAGS_UNSIGNED: trio_flags_t = 16384;
const FLAGS_UPPER: trio_flags_t = 32768;
const FLAGS_WIDTH: trio_flags_t = 65536;
const FLAGS_WIDTH_PARAMETER: trio_flags_t = 131072;
const FLAGS_PRECISION: trio_flags_t = 262144;
const FLAGS_PRECISION_PARAMETER: trio_flags_t = 524288;
const FLAGS_BASE: trio_flags_t = 1048576;
const FLAGS_BASE_PARAMETER: trio_flags_t = 2097152;
const FLAGS_FLOAT_E: trio_flags_t = 4194304;
const FLAGS_FLOAT_G: trio_flags_t = 8388608;
const FLAGS_QUOTE: trio_flags_t = 16777216;
const FLAGS_WIDECHAR: trio_flags_t = 33554432;
const FLAGS_IGNORE: trio_flags_t = 67108864;
const FLAGS_VARSIZE_PARAMETER: trio_flags_t = 268435456;
const FLAGS_FIXED_SIZE: trio_flags_t = 536870912;
const FLAGS_ROUNDING: trio_flags_t = FLAGS_INTMAX_T; // reuse

// ============================================================================
// Constants
// ============================================================================

const NO_POSITION: c_int = -1;
const NO_WIDTH: c_int = 0;
const NO_PRECISION: c_int = -1;
const NO_SIZE: c_int = -1;
const NO_BASE: c_int = -1;
const MAX_BASE: c_int = 36;
const BASE_BINARY: c_int = 2;
const BASE_OCTAL: c_int = 8;
const BASE_DECIMAL: c_int = 10;
const BASE_HEX: c_int = 16;

const MAX_PARAMETERS: usize = 64;
const CHAR_IDENTIFIER: c_char = b'%' as c_char;
const CHAR_BACKSLASH: c_char = b'\\' as c_char;

const INFINITE_LOWER: &[u8] = b"inf";
const INFINITE_UPPER: &[u8] = b"INF";
const NAN_LOWER: &[u8] = b"nan";
const NAN_UPPER: &[u8] = b"NAN";

const MAX_CHARS_IN_UINTMAX: usize = 64;

const POINTER_WIDTH: usize = 2 + std::mem::size_of::<usize>() * 2;

const DIGITS_LOWER: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const DIGITS_UPPER: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

// ============================================================================
// Internal state
// ============================================================================

static INTERNAL_NULL_STRING: &[u8] = b"(nil)\0";

// ============================================================================
// Parameter structure
// ============================================================================

const TRIO_PARAMETER_ZERO: TrioParameter = unsafe { std::mem::zeroed() };

#[derive(Clone, Copy)]
#[repr(C)]
struct TrioParameter {
    param_type: c_int,
    flags: trio_flags_t,
    width: c_int,
    precision: c_int,
    base: c_int,
    base_specifier: c_int,
    varsize: c_int,
    begin_offset: c_int,
    end_offset: c_int,
    position: c_int,
    data: TrioParameterData,
}

#[derive(Clone, Copy)]
#[repr(C)]
union TrioParameterData {
    string: *mut c_char,
    pointer: trio_pointer_t,
    number: TrioParameterNumber,
    double_number: f64,
    longdouble_number: trio_long_double_t,
    double_pointer: *mut f64,
    longdouble_pointer: *mut trio_long_double_t,
    error_number: c_int,
}

#[derive(Clone, Copy)]
#[repr(C)]
union TrioParameterNumber {
    as_signed: trio_intmax_t,
    as_unsigned: trio_uintmax_t,
}

impl Default for TrioParameter {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

// ============================================================================
// Helper: trio_to_upper
// ============================================================================

#[inline]
fn trio_to_upper_char(ch: c_char) -> c_char {
    let c = ch as u8;
    if c >= b'a' && c <= b'z' {
        (c - (b'a' - b'A')) as c_char
    } else {
        ch
    }
}

// ============================================================================
// TrioGetPosition - parse %n$ positional
// ============================================================================

fn trio_get_position(format: &[u8], offset: &mut usize) -> c_int {
    let mut pos = *offset;
    let mut number: c_int = 0;
    while pos < format.len() && format[pos].is_ascii_digit() {
        number = number * 10 + (format[pos] - b'0') as c_int;
        pos += 1;
    }
    if number != 0 && pos < format.len() && format[pos] == b'$' {
        pos += 1;
        *offset = pos;
        number - 1 // n$ starts from 1, array from 0
    } else {
        NO_POSITION
    }
}

// ============================================================================
// TrioIsQualifier
// ============================================================================

fn trio_is_qualifier(ch: u8) -> bool {
    #[allow(clippy::match_like_matches_macro)]
    match ch {
        b'0'..=b'9'
        | b'+'
        | b'-'
        | b' '
        | b'.'
        | b'*'
        | b'#'
        | b'h'
        | b'l'
        | b'^'
        | b'L'
        | b'z'
        | b't'
        | b'j'
        | b'q'
        | b'Z'
        | b'w'
        | b'I'
        | b'\''
        | b'!'
        | b'&'
        | b'R' => true,
        _ => false,
    }
}

// ============================================================================
// TrioParseQualifiers
// ============================================================================

fn trio_parse_qualifiers(
    format: &[u8],
    offset: &mut usize,
    parameter: &mut TrioParameter,
) -> c_int {
    parameter.begin_offset = *offset as c_int - 1;
    parameter.flags = FLAGS_NEW;
    parameter.position = trio_get_position(format, offset);

    parameter.width = NO_WIDTH;
    parameter.precision = NO_PRECISION;
    parameter.base = NO_BASE;
    parameter.varsize = NO_SIZE;

    let mut dots: c_int = 0;

    while *offset < format.len() && trio_is_qualifier(format[*offset]) {
        let ch = format[*offset];
        *offset += 1;

        match ch {
            b' ' => {
                parameter.flags |= FLAGS_SPACE;
            }
            b'+' => {
                parameter.flags |= FLAGS_SHOWSIGN;
            }
            b'-' => {
                parameter.flags |= FLAGS_LEFTADJUST;
                parameter.flags &= !FLAGS_NILPADDING;
            }
            b'#' => {
                parameter.flags |= FLAGS_ALTERNATIVE;
            }
            b'.' => {
                if dots == 0 {
                    dots += 1;
                    if *offset < format.len() && format[*offset] == b'.' {
                        break;
                    }
                    parameter.flags |= FLAGS_PRECISION;
                    if *offset < format.len() && format[*offset] == b'*' {
                        *offset += 1;
                        parameter.flags |= FLAGS_PRECISION_PARAMETER;
                        parameter.precision = trio_get_position(format, offset);
                    } else {
                        let mut pos = *offset;
                        while pos < format.len() && format[pos].is_ascii_digit() {
                            pos += 1;
                        }
                        let num_str = std::str::from_utf8(&format[*offset..pos]).unwrap_or("0");
                        parameter.precision = num_str.parse::<c_int>().unwrap_or(0);
                        *offset = pos;
                    }
                } else if dots == 1 {
                    dots += 1;
                    parameter.flags |= FLAGS_BASE;
                    if *offset < format.len() && format[*offset] == b'*' {
                        *offset += 1;
                        parameter.flags |= FLAGS_BASE_PARAMETER;
                        parameter.base = trio_get_position(format, offset);
                    } else {
                        let mut pos = *offset;
                        while pos < format.len() && format[pos].is_ascii_digit() {
                            pos += 1;
                        }
                        let num_str = std::str::from_utf8(&format[*offset..pos]).unwrap_or("0");
                        parameter.base = num_str.parse::<c_int>().unwrap_or(0);
                        if parameter.base > MAX_BASE {
                            return trio_error_return(TRIO_EINVAL, *offset as c_int);
                        }
                        *offset = pos;
                    }
                } else {
                    return trio_error_return(TRIO_EINVAL, *offset as c_int);
                }
            }
            b'*' => {
                parameter.flags |= FLAGS_WIDTH | FLAGS_WIDTH_PARAMETER;
                let w = trio_get_position(format, offset);
                if w != NO_POSITION {
                    parameter.width = w;
                }
            }
            b'0' => {
                if parameter.flags & FLAGS_LEFTADJUST == 0 {
                    parameter.flags |= FLAGS_NILPADDING;
                }
                // Fall through to number parsing
                let mut pos = *offset - 1;
                while pos < format.len() && format[pos].is_ascii_digit() {
                    pos += 1;
                }
                let num_str = std::str::from_utf8(&format[*offset - 1..pos]).unwrap_or("0");
                parameter.width = num_str.parse::<c_int>().unwrap_or(0);
                *offset = pos;
            }
            b'1'..=b'9' => {
                let mut pos = *offset - 1;
                while pos < format.len() && format[pos].is_ascii_digit() {
                    pos += 1;
                }
                let num_str = std::str::from_utf8(&format[*offset - 1..pos]).unwrap_or("0");
                parameter.width = num_str.parse::<c_int>().unwrap_or(0);
                *offset = pos;
            }
            b'h' => {
                if parameter.flags & FLAGS_SHORTSHORT != 0 {
                    return trio_error_return(TRIO_EINVAL, *offset as c_int);
                } else if parameter.flags & FLAGS_SHORT != 0 {
                    parameter.flags |= FLAGS_SHORTSHORT;
                } else {
                    parameter.flags |= FLAGS_SHORT;
                }
            }
            b'l' => {
                if parameter.flags & FLAGS_QUAD != 0 {
                    return trio_error_return(TRIO_EINVAL, *offset as c_int);
                } else if parameter.flags & FLAGS_LONG != 0 {
                    parameter.flags |= FLAGS_QUAD;
                } else {
                    parameter.flags |= FLAGS_LONG;
                }
            }
            b'L' => {
                parameter.flags |= FLAGS_LONGDOUBLE;
            }
            b'z' => {
                parameter.flags |= FLAGS_SIZE_T;
                if std::mem::size_of::<usize>() == std::mem::size_of::<u64>() {
                    parameter.flags |= FLAGS_QUAD;
                } else {
                    parameter.flags |= FLAGS_LONG;
                }
            }
            b't' => {
                parameter.flags |= FLAGS_PTRDIFF_T;
                if std::mem::size_of::<isize>() == std::mem::size_of::<i64>() {
                    parameter.flags |= FLAGS_QUAD;
                } else {
                    parameter.flags |= FLAGS_LONG;
                }
            }
            b'j' => {
                parameter.flags |= FLAGS_INTMAX_T;
                if std::mem::size_of::<i64>() == std::mem::size_of::<i64>() {
                    parameter.flags |= FLAGS_QUAD;
                } else {
                    parameter.flags |= FLAGS_LONG;
                }
            }
            b'q' => {
                parameter.flags |= FLAGS_QUAD;
            }
            b'Z' => {} // size_t upper (ignored)
            b'w' => {
                parameter.flags |= FLAGS_WIDECHAR;
            }
            b'\'' => {
                parameter.flags |= FLAGS_QUOTE;
            }
            b'I' => {
                if parameter.flags & FLAGS_FIXED_SIZE != 0 {
                    return trio_error_return(TRIO_EINVAL, *offset as c_int);
                }
                if *offset + 1 < format.len()
                    && format[*offset] == b'6'
                    && format[*offset + 1] == b'4'
                {
                    parameter.varsize = 8;
                    *offset += 2;
                } else if *offset + 1 < format.len()
                    && format[*offset] == b'3'
                    && format[*offset + 1] == b'2'
                {
                    parameter.varsize = 4;
                    *offset += 2;
                } else if *offset + 1 < format.len()
                    && format[*offset] == b'1'
                    && format[*offset + 1] == b'6'
                {
                    parameter.varsize = 2;
                    *offset += 2;
                } else if *offset < format.len() && format[*offset] == b'8' {
                    parameter.varsize = 1;
                    *offset += 1;
                } else {
                    return trio_error_return(TRIO_EINVAL, *offset as c_int);
                }
                parameter.flags |= FLAGS_FIXED_SIZE;
            }
            b'!' => {
                parameter.flags |= FLAGS_STICKY;
            }
            b'&' => {
                parameter.flags |= FLAGS_VARSIZE_PARAMETER;
            }
            b'R' => {
                parameter.flags |= FLAGS_ROUNDING;
            }
            _ => {
                return trio_error_return(TRIO_EINVAL, *offset as c_int);
            }
        }
    }

    parameter.end_offset = *offset as c_int;
    0
}

// ============================================================================
// TrioParseSpecifier
// ============================================================================

fn trio_parse_specifier(format: &[u8], offset: &mut usize, parameter: &mut TrioParameter) -> c_int {
    parameter.base_specifier = NO_BASE;

    if *offset >= format.len() {
        return trio_error_return(TRIO_EINVAL, *offset as c_int);
    }

    let ch = format[*offset];
    match ch {
        b'C' => {
            parameter.flags |= FLAGS_WIDECHAR;
            parameter.param_type = FORMAT_CHAR;
        }
        b'c' => {
            if parameter.flags & FLAGS_LONG != 0 {
                parameter.flags |= FLAGS_WIDECHAR;
            }
            parameter.param_type = FORMAT_CHAR;
        }
        b'S' => {
            parameter.flags |= FLAGS_WIDECHAR;
            parameter.param_type = FORMAT_STRING;
        }
        b's' => {
            if parameter.flags & FLAGS_LONG != 0 {
                parameter.flags |= FLAGS_WIDECHAR;
            }
            parameter.param_type = FORMAT_STRING;
        }
        b'i' => {
            parameter.param_type = FORMAT_INT;
        }
        b'u' => {
            parameter.flags |= FLAGS_UNSIGNED;
            parameter.param_type = FORMAT_INT;
        }
        b'd' => {
            parameter.base_specifier = BASE_DECIMAL;
            parameter.param_type = FORMAT_INT;
        }
        b'o' => {
            parameter.flags |= FLAGS_UNSIGNED;
            parameter.base_specifier = BASE_OCTAL;
            parameter.param_type = FORMAT_INT;
        }
        b'x' => {
            parameter.flags |= FLAGS_UNSIGNED;
            parameter.base_specifier = BASE_HEX;
            parameter.param_type = FORMAT_INT;
        }
        b'X' => {
            parameter.flags |= FLAGS_UNSIGNED | FLAGS_UPPER;
            parameter.base_specifier = BASE_HEX;
            parameter.param_type = FORMAT_INT;
        }
        b'e' => {
            parameter.flags |= FLAGS_FLOAT_E;
            parameter.param_type = FORMAT_DOUBLE;
        }
        b'E' => {
            parameter.flags |= FLAGS_UPPER | FLAGS_FLOAT_E;
            parameter.param_type = FORMAT_DOUBLE;
        }
        b'f' => {
            parameter.param_type = FORMAT_DOUBLE;
        }
        b'F' => {
            parameter.flags |= FLAGS_UPPER;
            parameter.param_type = FORMAT_DOUBLE;
        }
        b'g' => {
            parameter.flags |= FLAGS_FLOAT_G;
            parameter.param_type = FORMAT_DOUBLE;
        }
        b'G' => {
            parameter.flags |= FLAGS_UPPER | FLAGS_FLOAT_G;
            parameter.param_type = FORMAT_DOUBLE;
        }
        b'p' => {
            parameter.param_type = FORMAT_POINTER;
        }
        b'n' => {
            parameter.param_type = FORMAT_COUNT;
        }
        b'a' => {
            parameter.base_specifier = BASE_HEX;
            parameter.param_type = FORMAT_DOUBLE;
        }
        b'A' => {
            parameter.flags |= FLAGS_UPPER;
            parameter.base_specifier = BASE_HEX;
            parameter.param_type = FORMAT_DOUBLE;
        }
        b'm' => {
            parameter.param_type = FORMAT_ERRNO;
        }
        b'[' => {
            // Scan group - skip nested brackets
            let mut depth = 1i32;
            parameter.param_type = FORMAT_GROUP;
            *offset += 1;
            if *offset < format.len() && format[*offset] == b'^' {
                *offset += 1;
            }
            if *offset < format.len() && format[*offset] == b']' {
                *offset += 1;
            }
            if *offset < format.len() && format[*offset] == b'-' {
                *offset += 1;
            }
            while *offset < format.len() && format[*offset] != 0 {
                if format[*offset] == b'[' {
                    depth += 1;
                } else if format[*offset] == b']' {
                    depth -= 1;
                    if depth <= 0 {
                        *offset += 1;
                        break;
                    }
                }
                *offset += 1;
            }
        }
        _ => {
            return trio_error_return(TRIO_EINVAL, *offset as c_int);
        }
    }
    *offset += 1;
    parameter.end_offset = *offset as c_int;
    0
}

// ============================================================================
// TrioWriteNumber - output a formatted integer
// ============================================================================

fn trio_write_number<W: Write>(
    out: &mut W,
    mut number: trio_uintmax_t,
    flags: trio_flags_t,
    width: c_int,
    precision: c_int,
    base: c_int,
) {
    let digits = if flags & FLAGS_UPPER != 0 {
        DIGITS_UPPER
    } else {
        DIGITS_LOWER
    };
    let base = if base == NO_BASE { BASE_DECIMAL } else { base };

    let is_number_zero = number == 0;
    let is_precision_zero = precision == 0;
    let ignore_number = is_number_zero
        && is_precision_zero
        && !((flags & FLAGS_ALTERNATIVE != 0) && base == BASE_OCTAL);

    let mut is_negative = false;
    if flags & FLAGS_UNSIGNED == 0 {
        is_negative = (number as trio_intmax_t) < 0;
        if is_negative {
            number = (-(number as trio_intmax_t)) as trio_uintmax_t;
        }
    }

    // Build number string (from least significant digit)
    let mut buffer = [0u8; MAX_CHARS_IN_UINTMAX + 1];
    let mut pos = MAX_CHARS_IN_UINTMAX;
    buffer[pos] = 0;
    pos -= 1;
    let mut n = number;
    for _ in 0..MAX_CHARS_IN_UINTMAX {
        if pos == 0 {
            break;
        }
        let digit = (n % (base as trio_uintmax_t)) as usize;
        buffer[pos] = digits[digit];
        n /= base as trio_uintmax_t;
        if n == 0 {
            break;
        }
        pos -= 1;
    }
    let number_start = pos + 1;
    let number_len = MAX_CHARS_IN_UINTMAX - number_start - 1;

    let mut width = width as isize;

    // Adjust for precision
    if NO_PRECISION != precision {
        let prec = precision as isize - number_len as isize;
        if prec > 0 {
            let zeros = prec;
            // Write sign
            if is_negative {
                let _ = out.write(b"-");
            } else if flags & FLAGS_SHOWSIGN != 0 {
                let _ = out.write(b"+");
            } else if flags & FLAGS_SPACE != 0 {
                let _ = out.write(b" ");
            }
            // Write prefix
            if (flags & FLAGS_ALTERNATIVE != 0) && !is_number_zero {
                match base {
                    BASE_BINARY => {
                        let _ = out.write(if flags & FLAGS_UPPER != 0 {
                            b"0B"
                        } else {
                            b"0b"
                        });
                    }
                    BASE_OCTAL => {
                        let _ = out.write(b"0");
                    }
                    BASE_HEX => {
                        let _ = out.write(if flags & FLAGS_UPPER != 0 {
                            b"0X"
                        } else {
                            b"0x"
                        });
                    }
                    _ => {}
                }
            }
            // Write zeros
            for _ in 0..zeros {
                let _ = out.write(b"0");
            }
            // Write number
            if !ignore_number {
                let _ = out.write(&buffer[number_start..]);
            }
            // Now handle width (left or right padding)
            let sign_space =
                if is_negative || (flags & FLAGS_SHOWSIGN != 0) || (flags & FLAGS_SPACE != 0) {
                    1
                } else {
                    0
                };
            let prefix_len = if (flags & FLAGS_ALTERNATIVE != 0) && !is_number_zero {
                match base {
                    BASE_BINARY | BASE_HEX => 2,
                    BASE_OCTAL => 1,
                    _ => 0,
                }
            } else {
                0
            };
            let total = sign_space + prefix_len + zeros + number_len as isize;
            width -= total;
            if width > 0 && (flags & FLAGS_LEFTADJUST != 0) {
                for _ in 0..width {
                    let _ = out.write(b" ");
                }
            } else {
                width = width.max(0);
            }
            return;
        }
    }

    if !ignore_number {
        width -= number_len as isize;
    }

    // Adjust for sign/prefix
    if is_negative || (flags & FLAGS_SHOWSIGN != 0) || (flags & FLAGS_SPACE != 0) {
        width -= 1;
    }
    if (flags & FLAGS_ALTERNATIVE != 0) && !is_number_zero {
        match base {
            BASE_BINARY | BASE_HEX => width -= 2,
            BASE_OCTAL => width -= 1,
            _ => {}
        }
    }

    // Output prefix spaces
    if !(flags & FLAGS_LEFTADJUST != 0) && !(flags & FLAGS_NILPADDING != 0) {
        while width > 0 {
            let _ = out.write(b" ");
            width -= 1;
        }
    }

    // Sign
    if is_negative {
        let _ = out.write(b"-");
    } else if flags & FLAGS_SHOWSIGN != 0 {
        let _ = out.write(b"+");
    } else if flags & FLAGS_SPACE != 0 {
        let _ = out.write(b" ");
    }

    // Prefix
    if (flags & FLAGS_ALTERNATIVE != 0) && !is_number_zero {
        match base {
            BASE_BINARY => {
                let _ = out.write(if flags & FLAGS_UPPER != 0 {
                    b"0B"
                } else {
                    b"0b"
                });
            }
            BASE_OCTAL => {
                let _ = out.write(b"0");
            }
            BASE_HEX => {
                let _ = out.write(if flags & FLAGS_UPPER != 0 {
                    b"0X"
                } else {
                    b"0x"
                });
            }
            _ => {}
        }
    }

    // Zero padding
    if flags & FLAGS_NILPADDING != 0 {
        while width > 0 {
            let _ = out.write(b"0");
            width -= 1;
        }
    }

    // Number
    if !ignore_number {
        let _ = out.write(&buffer[number_start..]);
    }

    // Trailing spaces
    if flags & FLAGS_LEFTADJUST != 0 {
        while width > 0 {
            let _ = out.write(b" ");
            width -= 1;
        }
    }
}

// ============================================================================
// TrioWriteString - output a formatted string
// ============================================================================

fn trio_write_string<W: Write>(
    out: &mut W,
    string: *const c_char,
    flags: trio_flags_t,
    width: c_int,
    precision: c_int,
) {
    let null_str: &[u8] = b"(nil)";
    let (s, len): (*const c_char, usize) = if string.is_null() {
        (null_str.as_ptr() as *const c_char, null_str.len() - 1)
    } else {
        unsafe {
            let l = cstr_len(string);
            let effective = if NO_PRECISION != precision && (precision as usize) < l {
                precision as usize
            } else {
                l
            };
            (string, effective)
        }
    };

    let mut width = width as isize;
    width -= len as isize;

    if !(flags & FLAGS_LEFTADJUST != 0) {
        while width > 0 {
            let _ = out.write(b" ");
            width -= 1;
        }
    }

    for i in 0..len {
        unsafe {
            let ch = *string.add(i);
            trio_write_string_character(out, ch as u8, flags);
        }
    }

    if flags & FLAGS_LEFTADJUST != 0 {
        while width > 0 {
            let _ = out.write(b" ");
            width -= 1;
        }
    }
}

fn trio_write_string_character<W: Write>(out: &mut W, ch: u8, flags: trio_flags_t) {
    if flags & FLAGS_ALTERNATIVE != 0 && !ch.is_ascii_graphic() {
        let _ = out.write(b"\\");
        match ch {
            7 => {
                let _ = out.write(b"a");
            }
            8 => {
                let _ = out.write(b"b");
            }
            12 => {
                let _ = out.write(b"f");
            }
            10 => {
                let _ = out.write(b"n");
            }
            13 => {
                let _ = out.write(b"r");
            }
            9 => {
                let _ = out.write(b"t");
            }
            11 => {
                let _ = out.write(b"v");
            }
            b'\\' => {
                let _ = out.write(b"\\");
            }
            _ => {
                let _ = out.write(b"x");
                if ch < 16 {
                    let _ = out.write(b"0");
                }
                let digit = DIGITS_LOWER[(ch / 16) as usize];
                let digit2 = DIGITS_LOWER[(ch % 16) as usize];
                let _ = out.write(&[digit, digit2]);
            }
        }
    } else if ch == CHAR_BACKSLASH as u8 && (flags & FLAGS_ALTERNATIVE != 0) {
        let _ = out.write(b"\\\\");
    } else {
        let _ = out.write(&[ch]);
    }
}

// ============================================================================
// TrioWriteDouble - output a formatted floating-point number
// ============================================================================

fn trio_write_double<W: Write>(
    out: &mut W,
    number: trio_long_double_t,
    flags: trio_flags_t,
    width: c_int,
    precision: c_int,
    base: c_int,
) {
    let digits = if flags & FLAGS_UPPER != 0 {
        DIGITS_UPPER
    } else {
        DIGITS_LOWER
    };
    let base = if base == NO_BASE { BASE_DECIMAL } else { base };
    let is_hex = base == BASE_HEX;

    // Check for special quantities
    let is_negative;
    {
        let mut neg: c_int = 0;
        let class = crate::trio::trionan::trio_fpclassify_and_signbit(number, &mut neg);
        is_negative = neg != 0;
        match class {
            TRIO_FP_NAN => {
                let s = if flags & FLAGS_UPPER != 0 {
                    NAN_UPPER
                } else {
                    NAN_LOWER
                };
                trio_write_string(out, s.as_ptr() as *const c_char, flags, width, precision);
                return;
            }
            TRIO_FP_INFINITE => {
                let s = if flags & FLAGS_UPPER != 0 {
                    INFINITE_UPPER
                } else {
                    INFINITE_LOWER
                };
                if is_negative {
                    let _ = out.write(b"-");
                }
                trio_write_string(
                    out,
                    s.as_ptr() as *const c_char,
                    flags,
                    width - (if is_negative { 1 } else { 0 }),
                    precision,
                );
                return;
            }
            _ => {}
        }
    }

    let mut number = if is_negative { -number } else { number };

    let mut precision = if precision == NO_PRECISION {
        6
    } else {
        precision
    };

    if is_hex {
        // Hex float: use %a format
        if precision == NO_PRECISION || precision == 0 {
            precision = 13;
        }
        let bits = number.to_bits();
        let exponent_raw = ((bits >> 52) & 0x7ff) as i32;
        let mantissa = bits & 0x000fffffffffffff;
        let is_denorm = exponent_raw == 0 && mantissa != 0;
        let biased_exp = if is_denorm {
            1 - 1023
        } else {
            exponent_raw - 1023
        };

        let hex_digits = if mantissa == 0 { 1 } else { 13 };
        let mut hex_buf = [0u8; 20];
        let mut m = mantissa;
        for i in (0..hex_digits).rev() {
            hex_buf[i] = digits[(m & 0xf) as usize];
            m >>= 4;
        }

        let integer_digit = if !is_denorm { 1 } else { 0 };
        let fraction_start: usize = 1;

        let exp_display = biased_exp;

        if is_negative {
            let _ = out.write(b"-");
        } else if flags & FLAGS_SHOWSIGN != 0 {
            let _ = out.write(b"+");
        } else if flags & FLAGS_SPACE != 0 {
            let _ = out.write(b" ");
        }

        let _ = out.write(if flags & FLAGS_UPPER != 0 {
            b"0X"
        } else {
            b"0x"
        });
        let _ = out.write(&[hex_buf[integer_digit]]);

        let frac_count: usize = if precision > 0 { precision as usize } else { 0 };
        if frac_count > 0 {
            let _ = out.write(b".");
            let end = fraction_start + frac_count.min(13);
            let _ = out.write(&hex_buf[(fraction_start as usize)..(end as usize)]);
        }

        let exp_char = if flags & FLAGS_UPPER != 0 { b'P' } else { b'p' };
        let _ = out.write(&[exp_char]);
        if exp_display >= 0 {
            let _ = out.write(b"+");
            let exp_str = format!("{}", exp_display);
            let _ = out.write(exp_str.as_bytes());
        } else {
            let _ = out.write(b"-");
            let exp_str = format!("{}", -exp_display);
            let _ = out.write(exp_str.as_bytes());
        }

        return;
    }

    // Determine if we use scientific notation (for %g/%G)
    let mut use_scientific = flags & FLAGS_FLOAT_E != 0;

    if flags & FLAGS_FLOAT_G != 0 {
        if precision == 0 {
            precision = 1;
        }
        if number == 0.0 {
            // 0.0 doesn't switch to scientific
        } else if number.abs() < 1e-4 || number.abs() >= 10.0_f64.powi(precision as i32) {
            use_scientific = true;
        }
    }

    // For scientific notation
    if use_scientific {
        let log_val = if number > 0.0 {
            number.log10()
        } else if number == 0.0 {
            0.0
        } else {
            f64::NAN
        };

        let mut exponent: c_int;
        if log_val.is_nan() || log_val.is_infinite() {
            exponent = 0;
        } else {
            exponent = log_val.floor() as c_int;
        }

        // Scale number
        let scale = 10.0_f64.powi(-exponent);
        let mut scaled = number * scale;
        if scaled.is_infinite() {
            let half_exp = exponent / 2;
            scaled = number / 10.0_f64.powi(half_exp);
            scaled /= 10.0_f64.powi(exponent - half_exp);
        }
        number = scaled;

        let is_exp_negative = exponent < 0;
        let u_exponent = if is_exp_negative { -exponent } else { exponent };

        // Round the number
        let frac_base = 10.0_f64.powi(precision);
        let mut f_adjust: f64 = 0.5;
        if base == BASE_DECIMAL {
            let work = (number * frac_base) % 10.0;
            if (work as c_int) % 2 == 0 {
                f_adjust = 0.5 * (1.0 - 5.0 * f64::EPSILON);
            }
        }
        let rounded = number + f_adjust / frac_base;

        if rounded.floor() != number.floor() {
            exponent += 1;
            let is_exp_negative = exponent < 0;
            let _u_exponent = if is_exp_negative { -exponent } else { exponent };
            number = (number + 0.5 / frac_base) / 10.0;
        } else {
            number = rounded;
        }

        let keep_trailing = !(flags & FLAGS_FLOAT_G != 0 && !(flags & FLAGS_ALTERNATIVE != 0));

        // Output sign
        if is_negative {
            let _ = out.write(b"-");
        } else if flags & FLAGS_SHOWSIGN != 0 {
            let _ = out.write(b"+");
        } else if flags & FLAGS_SPACE != 0 {
            let _ = out.write(b" ");
        }

        // Output integer part
        let int_digit = number.abs().floor() as c_int;
        let int_digit = int_digit.min(9).max(0);
        let _ = out.write(&[digits[int_digit as usize]]);

        // Output fraction
        let mut frac = number.abs() - number.abs().floor();
        if precision > 0 || (flags & FLAGS_ALTERNATIVE != 0) {
            let _ = out.write(b".");
            let mut trailing_zeros = 0;
            for _ in 0..precision {
                frac *= 10.0;
                let d = frac.floor() as c_int;
                frac -= d as f64;
                if d == 0 && !keep_trailing {
                    trailing_zeros += 1;
                } else {
                    for _ in 0..trailing_zeros {
                        let _ = out.write(&[digits[0]]);
                    }
                    trailing_zeros = 0;
                    let _ = out.write(&[digits[d.min(9).max(0) as usize]]);
                }
            }
            if keep_trailing {
                for _ in 0..trailing_zeros {
                    let _ = out.write(&[digits[0]]);
                }
            }
        }

        // Output exponent
        let e_char = if flags & FLAGS_UPPER != 0 { b'E' } else { b'e' };
        let _ = out.write(&[e_char]);
        let is_exp_negative = exponent < 0;
        let u_exponent = if is_exp_negative { -exponent } else { exponent };
        let _ = out.write(if is_exp_negative { b"-" } else { b"+" });
        let exp_str = format!("{}", u_exponent);
        if exp_str.len() < 2 {
            let _ = out.write(b"0");
        }
        let _ = out.write(exp_str.as_bytes());

        return;
    }

    // Regular (non-scientific) format
    let frac_base = 10.0_f64.powi(precision);
    let mut f_adjust: f64 = 0.5;
    if base == BASE_DECIMAL {
        let work = (number * frac_base) % 10.0;
        if (work as c_int) % 2 == 0 {
            f_adjust = 0.5 * (1.0 - 5.0 * f64::EPSILON);
        }
    }
    let rounded = number + f_adjust / frac_base;

    let integer_part = rounded.floor();
    let mut fraction_part = rounded - integer_part;

    let keep_trailing = !(flags & FLAGS_FLOAT_G != 0 && !(flags & FLAGS_ALTERNATIVE != 0));

    // Count integer digits
    let integer_digits = if integer_part > f64::EPSILON {
        1 + integer_part.abs().log10().floor() as usize
    } else {
        1
    };

    // Calculate expected width for padding
    let mut expected_width = integer_digits + precision as usize;
    if precision > 0 || (flags & FLAGS_ALTERNATIVE != 0) {
        expected_width += 1; // decimal point
    }
    if is_negative || (flags & FLAGS_SHOWSIGN != 0) || (flags & FLAGS_SPACE != 0) {
        expected_width += 1;
    }

    let mut width = width as isize - expected_width as isize;

    // Output prefix spaces
    if !(flags & FLAGS_LEFTADJUST != 0) && !(flags & FLAGS_NILPADDING != 0) {
        while width > 0 {
            let _ = out.write(b" ");
            width -= 1;
        }
    }

    // Output sign
    if is_negative {
        let _ = out.write(b"-");
    } else if flags & FLAGS_SHOWSIGN != 0 {
        let _ = out.write(b"+");
    } else if flags & FLAGS_SPACE != 0 {
        let _ = out.write(b" ");
    }

    // Zero padding
    if flags & FLAGS_NILPADDING != 0 {
        while width > 0 {
            let _ = out.write(b"0");
            width -= 1;
        }
    }

    // Output integer part
    let mut int_num = integer_part;
    if int_num > f64::EPSILON {
        let start_power = 10.0_f64.powi(integer_digits as i32 - 1);
        for _ in 0..integer_digits {
            let digit = (int_num / start_power).floor() as c_int;
            int_num -= digit as f64 * start_power;
            let _ = out.write(&[digits[digit.min(9).max(0) as usize]]);
        }
    } else {
        let _ = out.write(&[digits[0]]);
    }

    // Output fraction
    if precision > 0 || (flags & FLAGS_ALTERNATIVE != 0) {
        let _ = out.write(b".");
        let mut trailing_zeros = 0;
        for _ in 0..precision {
            fraction_part *= 10.0;
            let d = fraction_part.floor() as c_int;
            fraction_part -= d as f64;
            if d == 0 && !keep_trailing {
                trailing_zeros += 1;
            } else {
                for _ in 0..trailing_zeros {
                    let _ = out.write(&[digits[0]]);
                }
                trailing_zeros = 0;
                let _ = out.write(&[digits[d.min(9).max(0) as usize]]);
            }
        }
        if keep_trailing {
            for _ in 0..trailing_zeros {
                let _ = out.write(&[digits[0]]);
            }
        }
    }

    // Trailing spaces
    if flags & FLAGS_LEFTADJUST != 0 {
        while width > 0 {
            let _ = out.write(b" ");
            width -= 1;
        }
    }
}

// ============================================================================
// Helper: strlen for C strings
// ============================================================================

unsafe fn cstr_len(s: *const c_char) -> usize {
    unsafe {
        let mut len = 0usize;
        while *s.add(len) != 0i8 {
            len += 1;
        }
        len
    }
}

// ============================================================================
// TrioFormatProcess - main formatting engine
// ============================================================================

fn trio_format_process<W: Write>(
    out: &mut W,
    format: &[u8],
    parameters: &[TrioParameter],
) -> c_int {
    let mut offset = 0usize;
    let mut i = 0usize;

    loop {
        // Skip FORMAT_PARAMETER entries
        while i < parameters.len() && parameters[i].param_type == FORMAT_PARAMETER {
            i += 1;
        }
        if i >= parameters.len() {
            break;
        }
        if parameters[i].param_type == FORMAT_SENTINEL {
            break;
        }

        // Copy non-conversion-specifier parts of format string
        while offset < parameters[i].begin_offset as usize {
            if offset + 1 < format.len()
                && format[offset] == CHAR_IDENTIFIER as u8
                && format[offset + 1] == CHAR_IDENTIFIER as u8
            {
                let _ = out.write(b"%");
                offset += 2;
            } else {
                let _ = out.write(&[format[offset]]);
                offset += 1;
            }
        }

        if parameters[i].param_type == FORMAT_SENTINEL {
            break;
        }

        let flags = parameters[i].flags;
        let mut width = parameters[i].width;
        if flags & FLAGS_WIDTH_PARAMETER != 0 {
            width = unsafe { parameters[width as usize].data.number.as_signed } as c_int;
            if width < 0 {
                // Can't mutate flags here, so handle inline
                let mut adj_flags = flags;
                adj_flags |= FLAGS_LEFTADJUST;
                adj_flags &= !FLAGS_NILPADDING;
                width = -width;
                // For simplicity, use the adjusted flags only for padding
            }
        }

        let mut precision = NO_PRECISION;
        if flags & FLAGS_PRECISION != 0 {
            precision = parameters[i].precision;
            if flags & FLAGS_PRECISION_PARAMETER != 0 {
                precision =
                    unsafe { parameters[precision as usize].data.number.as_signed } as c_int;
                if precision < 0 {
                    precision = NO_PRECISION;
                }
            }
        }

        let mut base = parameters[i].base;
        if NO_BASE != parameters[i].base_specifier {
            base = parameters[i].base_specifier;
        } else if flags & FLAGS_BASE_PARAMETER != 0 {
            base = unsafe { parameters[base as usize].data.number.as_signed } as c_int;
        }

        match parameters[i].param_type {
            FORMAT_CHAR => {
                if !(flags & FLAGS_LEFTADJUST != 0) {
                    let mut w = width;
                    while w > 1 {
                        let _ = out.write(b" ");
                        w -= 1;
                    }
                }
                unsafe {
                    let ch = parameters[i].data.number.as_unsigned as u8;
                    trio_write_string_character(out, ch, flags);
                }
                if flags & FLAGS_LEFTADJUST != 0 {
                    let mut w = width;
                    while w > 1 {
                        let _ = out.write(b" ");
                        w -= 1;
                    }
                }
            }
            FORMAT_INT => {
                trio_write_number(
                    out,
                    unsafe { parameters[i].data.number.as_unsigned },
                    flags,
                    width,
                    precision,
                    base,
                );
            }
            FORMAT_DOUBLE => {
                trio_write_double(
                    out,
                    unsafe { parameters[i].data.longdouble_number },
                    flags,
                    width,
                    precision,
                    base,
                );
            }
            FORMAT_STRING => {
                trio_write_string(
                    out,
                    unsafe { parameters[i].data.string },
                    flags,
                    width,
                    precision,
                );
            }
            FORMAT_POINTER => {
                let ptr = unsafe { parameters[i].data.pointer };
                if ptr.is_null() {
                    let _ = out.write(INTERNAL_NULL_STRING);
                } else {
                    let num = ptr as u64;
                    let ptr_flags = flags | FLAGS_UNSIGNED | FLAGS_ALTERNATIVE | FLAGS_NILPADDING;
                    trio_write_number(
                        out,
                        num,
                        ptr_flags,
                        POINTER_WIDTH as c_int,
                        NO_PRECISION,
                        BASE_HEX,
                    );
                }
            }
            FORMAT_COUNT => {
                // Count not fully supported without committed tracking
            }
            FORMAT_ERRNO => {
                // Error not supported in simplified version
            }
            FORMAT_GROUP => {
                // Group scanning not in simplified version
            }
            _ => {}
        }

        offset = parameters[i].end_offset as usize;
        i += 1;
    }

    0
}

// ============================================================================
// TrioParse - format string parser (simplified, no va_list)
// ============================================================================

fn trio_parse(
    _scan_type: c_int,
    format: &str,
    parameters: &mut [TrioParameter; MAX_PARAMETERS],
    _args: &mut FormatArgs,
) -> c_int {
    let mut offset = 0usize;
    let mut parameter_position = 0usize;
    let mut pos = 0usize;
    let format_bytes = format.as_bytes();
    let mut used_entries = [0u16; MAX_PARAMETERS];
    let mut max_param: c_int = -1;

    while offset < format_bytes.len() {
        let ch = format_bytes[offset];

        // Skip non-ASCII (multibyte)
        if ch > 127 {
            offset += 1;
            continue;
        }

        if ch != CHAR_IDENTIFIER as u8 {
            offset += 1;
            continue;
        }

        // Found '%'
        offset += 1;
        if offset >= format_bytes.len() {
            break;
        }
        if format_bytes[offset] == CHAR_IDENTIFIER as u8 {
            // Double %% - skip
            offset += 1;
            continue;
        }

        let mut parameter = TrioParameter::default();

        // Parse qualifiers
        let status = trio_parse_qualifiers(format_bytes, &mut offset, &mut parameter);
        if status < 0 {
            return status;
        }

        // Parse specifier
        let status = trio_parse_specifier(format_bytes, &mut offset, &mut parameter);
        if status < 0 {
            return status;
        }

        let positional = parameter.position != NO_POSITION;

        // Handle width parameter
        if parameter.flags & FLAGS_WIDTH_PARAMETER != 0 {
            if parameter.width == NO_WIDTH {
                parameter.width = parameter_position as c_int;
            } else if !positional {
                parameter.position = parameter.width + 1;
            }
            let idx = parameter.width as usize;
            if idx < MAX_PARAMETERS {
                used_entries[idx] += 1;
                if parameter.width as c_int > max_param {
                    max_param = parameter.width;
                }
                parameters[pos].param_type = FORMAT_PARAMETER;
                parameters[pos].flags = 0;
                parameters[pos].data = TrioParameterData {
                    number: TrioParameterNumber { as_unsigned: 0 },
                };
                parameter.width = pos as c_int;
                pos += 1;
            }
        }

        // Handle precision parameter
        if parameter.flags & FLAGS_PRECISION_PARAMETER != 0 {
            if parameter.precision == NO_PRECISION {
                parameter.precision = parameter_position as c_int;
            } else if !positional {
                parameter.position = parameter.precision + 1;
            }
            let idx = parameter.precision as usize;
            if idx < MAX_PARAMETERS {
                used_entries[idx] += 1;
                if parameter.precision as c_int > max_param {
                    max_param = parameter.precision;
                }
                parameters[pos].param_type = FORMAT_PARAMETER;
                parameters[pos].flags = 0;
                parameters[pos].data = TrioParameterData {
                    number: TrioParameterNumber { as_unsigned: 0 },
                };
                parameter.precision = pos as c_int;
                pos += 1;
            }
        }

        if parameter.position == NO_POSITION {
            parameter.position = parameter_position as c_int;
        }
        if parameter.position as c_int > max_param {
            max_param = parameter.position;
        }
        if (parameter.position as usize) >= MAX_PARAMETERS {
            return trio_error_return(TRIO_ETOOMANY, offset as c_int);
        }

        used_entries[parameter.position as usize] += 1;

        if parameter.base == NO_BASE {
            parameter.base = BASE_DECIMAL;
        }

        // Note: In this simplified version, we do not extract va_list arguments.
        // The actual argument data must be set externally or via the FormatArgs mechanism.

        if pos < MAX_PARAMETERS {
            parameters[pos] = parameter;
            pos += 1;
        }

        parameter_position += 1;
    }

    parameters[pos.min(MAX_PARAMETERS - 1)].param_type = FORMAT_SENTINEL;
    parameters[pos.min(MAX_PARAMETERS - 1)].begin_offset = offset as c_int;

    for num in 0..=(max_param as usize) {
        if num < MAX_PARAMETERS && used_entries[num] != 1 {
            return trio_error_return(TRIO_EDBLREF, num as c_int);
        }
    }

    pos as c_int
}

// ============================================================================
// FormatArgs - simplified argument storage (no C va_list)
// ============================================================================

/// Container for format arguments (replaces C va_list).
pub struct FormatArgs {
    args: Vec<FormatArg>,
    idx: usize,
}

enum FormatArg {
    Int(i64),
    UInt(u64),
    Double(f64),
    Pointer(*mut c_void),
    String(*mut c_char),
}

impl FormatArgs {
    pub fn new() -> Self {
        FormatArgs {
            args: Vec::new(),
            idx: 0,
        }
    }

    pub fn push_int(&mut self, val: i64) {
        self.args.push(FormatArg::Int(val));
    }
    pub fn push_uint(&mut self, val: u64) {
        self.args.push(FormatArg::UInt(val));
    }
    pub fn push_double(&mut self, val: f64) {
        self.args.push(FormatArg::Double(val));
    }
    pub fn push_pointer(&mut self, val: *mut c_void) {
        self.args.push(FormatArg::Pointer(val));
    }
    pub fn push_string(&mut self, val: *mut c_char) {
        self.args.push(FormatArg::String(val));
    }

    fn next_uint(&mut self) -> u64 {
        if self.idx < self.args.len() {
            let val = match &self.args[self.idx] {
                FormatArg::UInt(v) => *v,
                FormatArg::Int(v) => *v as u64,
                _ => 0,
            };
            self.idx += 1;
            val
        } else {
            0
        }
    }

    fn next_double(&mut self) -> f64 {
        if self.idx < self.args.len() {
            let val = match &self.args[self.idx] {
                FormatArg::Double(v) => *v,
                _ => 0.0,
            };
            self.idx += 1;
            val
        } else {
            0.0
        }
    }

    fn next_pointer(&mut self) -> *mut c_void {
        if self.idx < self.args.len() {
            let val = match &self.args[self.idx] {
                FormatArg::Pointer(v) => *v,
                FormatArg::String(v) => *v as *mut c_void,
                _ => ptr::null_mut(),
            };
            self.idx += 1;
            val
        } else {
            ptr::null_mut()
        }
    }

    fn next_string(&mut self) -> *mut c_char {
        if self.idx < self.args.len() {
            let val = match &self.args[self.idx] {
                FormatArg::String(v) => *v,
                _ => ptr::null_mut(),
            };
            self.idx += 1;
            val
        } else {
            ptr::null_mut()
        }
    }

    fn reset(&mut self) {
        self.idx = 0;
    }
}

// ============================================================================
// TrioFormat - main format entry point
// ============================================================================

fn trio_format<W: Write>(out: &mut W, format: &[u8], args: &mut FormatArgs) -> c_int {
    let mut parameters: [TrioParameter; MAX_PARAMETERS] = [TRIO_PARAMETER_ZERO; MAX_PARAMETERS];
    parameters[MAX_PARAMETERS - 1].param_type = FORMAT_SENTINEL;

    let fmt_str = unsafe { std::ffi::CStr::from_ptr(format.as_ptr() as *const c_char) }
        .to_str()
        .unwrap_or("");

    let status = trio_parse(TYPE_PRINT, fmt_str, &mut parameters, args);
    if status < 0 {
        return status;
    }

    // Populate parameter data from FormatArgs
    args.reset();
    for i in 0..MAX_PARAMETERS {
        if parameters[i].param_type == FORMAT_SENTINEL {
            break;
        }
        if parameters[i].param_type == FORMAT_PARAMETER {
            continue;
        }

        match parameters[i].param_type {
            FORMAT_STRING => {
                let s = args.next_string();
                parameters[i].data.string = s;
            }
            FORMAT_POINTER | FORMAT_COUNT | FORMAT_UNKNOWN => {
                let p = args.next_pointer();
                parameters[i].data.pointer = p;
            }
            FORMAT_CHAR | FORMAT_INT => {
                let v = args.next_uint();
                parameters[i].data = TrioParameterData {
                    number: TrioParameterNumber { as_unsigned: v },
                };
            }
            FORMAT_DOUBLE => {
                let v = args.next_double();
                parameters[i].data.longdouble_number = v;
            }
            FORMAT_ERRNO => {
                parameters[i].data.error_number = 0;
            }
            _ => {}
        }
    }

    trio_format_process(out, format, &parameters)
}

// ============================================================================
// TrioScanProcess - scanning engine
// ============================================================================

fn trio_scan_process(
    format: &[u8],
    parameters: &[TrioParameter],
    input: &[u8],
    input_pos: &mut usize,
) -> c_int {
    let mut offset = 0usize;
    let mut i = 0usize;
    let mut ch = if *input_pos < input.len() {
        input[*input_pos] as c_int
    } else {
        -1
    };
    let mut assignment = 0;

    loop {
        while i < parameters.len() && parameters[i].param_type == FORMAT_PARAMETER {
            i += 1;
        }
        if i >= parameters.len() {
            break;
        }
        if parameters[i].param_type == FORMAT_SENTINEL {
            break;
        }

        // Match non-conversion parts
        while offset < parameters[i].begin_offset as usize {
            if offset + 1 < format.len()
                && format[offset] == CHAR_IDENTIFIER as u8
                && format[offset + 1] == CHAR_IDENTIFIER as u8
            {
                if ch == CHAR_IDENTIFIER as c_int {
                    *input_pos += 1;
                    ch = if *input_pos < input.len() {
                        input[*input_pos] as c_int
                    } else {
                        -1
                    };
                    offset += 2;
                    continue;
                } else {
                    return trio_error_return(TRIO_EINVAL, offset as c_int);
                }
            }
            if (format[offset] as u8).is_ascii_whitespace() {
                while *input_pos < input.len() && (input[*input_pos] as u8).is_ascii_whitespace() {
                    *input_pos += 1;
                }
                ch = if *input_pos < input.len() {
                    input[*input_pos] as c_int
                } else {
                    -1
                };
            } else if ch == format[offset] as c_int {
                *input_pos += 1;
                ch = if *input_pos < input.len() {
                    input[*input_pos] as c_int
                } else {
                    -1
                };
            } else {
                return assignment;
            }
            offset += 1;
        }

        if parameters[i].param_type == FORMAT_SENTINEL {
            break;
        }
        if ch == -1 && parameters[i].param_type != FORMAT_COUNT {
            return if assignment > 0 { assignment } else { -1 };
        }

        let flags = parameters[i].flags;
        let width = parameters[i].width;

        match parameters[i].param_type {
            FORMAT_INT => {
                let mut base = BASE_DECIMAL;
                if NO_BASE != parameters[i].base_specifier {
                    base = parameters[i].base_specifier;
                }
                let mut number: trio_uintmax_t = 0;
                let mut got_number = false;
                let mut is_negative = false;

                // Skip whitespace
                while *input_pos < input.len() && (input[*input_pos] as u8).is_ascii_whitespace() {
                    *input_pos += 1;
                }
                ch = if *input_pos < input.len() {
                    input[*input_pos] as c_int
                } else {
                    -1
                };

                // Sign
                if flags & FLAGS_UNSIGNED == 0 {
                    if ch == b'+' as c_int {
                        *input_pos += 1;
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                    } else if ch == b'-' as c_int {
                        *input_pos += 1;
                        is_negative = true;
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                    }
                }

                // Alternative prefix (0x, 0b)
                if flags & FLAGS_ALTERNATIVE != 0 && ch == b'0' as c_int {
                    *input_pos += 1;
                    ch = if *input_pos < input.len() {
                        input[*input_pos] as c_int
                    } else {
                        -1
                    };
                    if ch != -1
                        && base == BASE_HEX
                        && trio_to_upper_char(ch as c_char) == 'X' as c_char
                    {
                        *input_pos += 1;
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                    }
                }

                // Read digits
                let width_limit = if width == NO_WIDTH {
                    usize::MAX
                } else {
                    width as usize
                };
                let start_pos = *input_pos;
                while *input_pos - start_pos < width_limit
                    && ch != -1
                    && !(ch as u8).is_ascii_whitespace()
                {
                    let digit_char = ch as u8;
                    let digit = if digit_char.is_ascii_digit() {
                        (digit_char - b'0') as c_int
                    } else if digit_char.is_ascii_hexdigit() {
                        let upper = trio_to_upper_char(digit_char as c_char);
                        if upper >= 'A' as c_char && upper <= 'F' as c_char {
                            upper as c_int - 'A' as c_int + 10
                        } else {
                            -1
                        }
                    } else {
                        -1
                    };
                    if digit < 0 || digit >= base {
                        break;
                    }
                    number = number * (base as trio_uintmax_t) + digit as trio_uintmax_t;
                    got_number = true;
                    *input_pos += 1;
                    ch = if *input_pos < input.len() {
                        input[*input_pos] as c_int
                    } else {
                        -1
                    };
                }

                if !got_number {
                    return assignment;
                }

                let final_number = if is_negative {
                    (-(number as trio_intmax_t)) as trio_uintmax_t
                } else {
                    number
                };

                if flags & FLAGS_IGNORE == 0 {
                    assignment += 1;
                    let pointer = unsafe { parameters[i].data.pointer };
                    if !pointer.is_null() {
                        unsafe {
                            if flags & FLAGS_SIZE_T != 0 {
                                *(pointer as *mut usize) = final_number as usize;
                            } else if flags & FLAGS_PTRDIFF_T != 0 {
                                *(pointer as *mut isize) = final_number as isize;
                            } else if flags & FLAGS_INTMAX_T != 0 || flags & FLAGS_QUAD != 0 {
                                *(pointer as *mut i64) = final_number as i64;
                            } else if flags & FLAGS_LONG != 0 {
                                *(pointer as *mut c_long) = final_number as c_long;
                            } else if flags & FLAGS_SHORTSHORT != 0 {
                                *(pointer as *mut i8) = final_number as i8;
                            } else if flags & FLAGS_SHORT != 0 {
                                *(pointer as *mut i16) = final_number as i16;
                            } else {
                                *(pointer as *mut c_int) = final_number as c_int;
                            }
                        }
                    }
                }
            }
            FORMAT_STRING => {
                // Skip whitespace
                while *input_pos < input.len() && (input[*input_pos] as u8).is_ascii_whitespace() {
                    *input_pos += 1;
                }
                ch = if *input_pos < input.len() {
                    input[*input_pos] as c_int
                } else {
                    -1
                };

                let width_limit = if width == NO_WIDTH {
                    usize::MAX
                } else {
                    width as usize
                };
                let mut count = 0usize;
                while count < width_limit && ch != -1 && !(ch as u8).is_ascii_whitespace() {
                    if flags & FLAGS_IGNORE == 0 {
                        let ptr = unsafe { parameters[i].data.string };
                        if !ptr.is_null() {
                            unsafe {
                                *ptr.add(count) = ch as c_char;
                            }
                        }
                    }
                    count += 1;
                    *input_pos += 1;
                    ch = if *input_pos < input.len() {
                        input[*input_pos] as c_int
                    } else {
                        -1
                    };
                }
                if flags & FLAGS_IGNORE == 0 {
                    let ptr = unsafe { parameters[i].data.string };
                    if !ptr.is_null() {
                        unsafe {
                            *ptr.add(count) = 0;
                        }
                    }
                    assignment += 1;
                }
            }
            FORMAT_DOUBLE => {
                // Read double from input
                let mut double_buf = [0u8; 512];
                let mut buf_pos = 0usize;
                let max_buf = width_limit(width);

                // Skip whitespace
                while *input_pos < input.len() && (input[*input_pos] as u8).is_ascii_whitespace() {
                    *input_pos += 1;
                }
                ch = if *input_pos < input.len() {
                    input[*input_pos] as c_int
                } else {
                    -1
                };

                // Sign
                if ch == b'+' as c_int || ch == b'-' as c_int {
                    double_buf[buf_pos] = ch as u8;
                    buf_pos += 1;
                    *input_pos += 1;
                    ch = if *input_pos < input.len() {
                        input[*input_pos] as c_int
                    } else {
                        -1
                    };
                }

                // Check for inf/nan
                let start = buf_pos;
                if ch == b'i' as c_int
                    || ch == b'I' as c_int
                    || ch == b'n' as c_int
                    || ch == b'N' as c_int
                {
                    while *input_pos < input.len()
                        && buf_pos < max_buf
                        && (input[*input_pos] as u8).is_ascii_alphabetic()
                    {
                        double_buf[buf_pos] = input[*input_pos] as u8;
                        buf_pos += 1;
                        *input_pos += 1;
                    }
                    double_buf[buf_pos] = 0;
                    let db_str = std::str::from_utf8(&double_buf[start..buf_pos]).unwrap_or("");
                    let db_upper = db_str.to_uppercase();
                    if db_upper.starts_with("INF") || db_upper.starts_with("INFINITE") {
                        let inf_val = if start > 0 && double_buf[0] == b'-' {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        };
                        let pointer = if flags & FLAGS_IGNORE == 0 {
                            unsafe { parameters[i].data.pointer }
                        } else {
                            ptr::null_mut()
                        };
                        if !pointer.is_null() {
                            unsafe {
                                *(pointer as *mut f64) = inf_val;
                            }
                        }
                        if flags & FLAGS_IGNORE == 0 {
                            assignment += 1;
                        }
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                        continue;
                    } else if db_upper.starts_with("NAN") {
                        let pointer = if flags & FLAGS_IGNORE == 0 {
                            unsafe { parameters[i].data.pointer }
                        } else {
                            ptr::null_mut()
                        };
                        if !pointer.is_null() {
                            unsafe {
                                *(pointer as *mut f64) = f64::NAN;
                            }
                        }
                        if flags & FLAGS_IGNORE == 0 {
                            assignment += 1;
                        }
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                        continue;
                    }
                }

                // Check for hex float (0x...)
                if ch == b'0' as c_int {
                    double_buf[buf_pos] = ch as u8;
                    buf_pos += 1;
                    *input_pos += 1;
                    ch = if *input_pos < input.len() {
                        input[*input_pos] as c_int
                    } else {
                        -1
                    };
                    if ch != -1 && trio_to_upper_char(ch as c_char) == 'X' as c_char {
                        double_buf[buf_pos] = ch as u8;
                        buf_pos += 1;
                        *input_pos += 1;
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                    }
                }

                // Read integer part
                while *input_pos < input.len() && buf_pos < max_buf && ch != -1 {
                    if (ch as u8).is_ascii_digit()
                        || (ch != -1
                            && (trio_to_upper_char(ch as c_char) as u8).is_ascii_hexdigit())
                    {
                        double_buf[buf_pos] = ch as u8;
                        buf_pos += 1;
                        *input_pos += 1;
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                    } else {
                        break;
                    }
                }

                // Decimal point
                if ch == b'.' as c_int {
                    double_buf[buf_pos] = ch as u8;
                    buf_pos += 1;
                    *input_pos += 1;
                    ch = if *input_pos < input.len() {
                        input[*input_pos] as c_int
                    } else {
                        -1
                    };
                    while *input_pos < input.len()
                        && buf_pos < max_buf
                        && ch != -1
                        && (ch as u8).is_ascii_digit()
                    {
                        double_buf[buf_pos] = ch as u8;
                        buf_pos += 1;
                        *input_pos += 1;
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                    }
                }

                // Exponent
                if ch != -1
                    && (trio_to_upper_char(ch as c_char) == 'E' as c_char
                        || trio_to_upper_char(ch as c_char) == 'P' as c_char)
                {
                    double_buf[buf_pos] = ch as u8;
                    buf_pos += 1;
                    *input_pos += 1;
                    ch = if *input_pos < input.len() {
                        input[*input_pos] as c_int
                    } else {
                        -1
                    };
                    if ch == b'+' as c_int || ch == b'-' as c_int {
                        double_buf[buf_pos] = ch as u8;
                        buf_pos += 1;
                        *input_pos += 1;
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                    }
                    while *input_pos < input.len()
                        && buf_pos < max_buf
                        && ch != -1
                        && (ch as u8).is_ascii_digit()
                    {
                        double_buf[buf_pos] = ch as u8;
                        buf_pos += 1;
                        *input_pos += 1;
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                    }
                }

                if buf_pos == 0 || double_buf[0] == 0 {
                    return assignment;
                }

                double_buf[buf_pos] = 0;
                let db_str = std::str::from_utf8(&double_buf[..buf_pos]).unwrap_or("0");
                let value: f64 = db_str.parse().unwrap_or(0.0);

                if flags & FLAGS_IGNORE == 0 {
                    assignment += 1;
                    let pointer = unsafe { parameters[i].data.pointer };
                    if !pointer.is_null() {
                        unsafe {
                            *(pointer as *mut f64) = value;
                        }
                    }
                }
            }
            FORMAT_CHAR => {
                let w = if width == NO_WIDTH {
                    1usize
                } else {
                    width as usize
                };
                let mut cnt: usize = 0;
                while cnt < w && ch != -1 {
                    if flags & FLAGS_IGNORE == 0 {
                        let ptr = unsafe { parameters[i].data.string };
                        if !ptr.is_null() {
                            unsafe {
                                *ptr.add(cnt as usize) = ch as i8;
                            }
                        }
                    }
                    cnt += 1;
                    *input_pos += 1;
                    ch = if *input_pos < input.len() {
                        input[*input_pos] as c_int
                    } else {
                        -1
                    };
                }
                if flags & FLAGS_IGNORE == 0 {
                    assignment += 1;
                }
            }
            FORMAT_COUNT => {
                let pointer = unsafe { parameters[i].data.pointer };
                if !pointer.is_null() {
                    let count_val = *input_pos as c_int;
                    unsafe {
                        *(pointer as *mut c_int) = count_val;
                    }
                }
            }
            FORMAT_POINTER => {
                let mut num: u64 = 0;
                let mut got = false;
                if ch == b'0' as c_int {
                    *input_pos += 1;
                    ch = if *input_pos < input.len() {
                        input[*input_pos] as c_int
                    } else {
                        -1
                    };
                    if ch != -1 && trio_to_upper_char(ch as c_char) == 'X' as c_char {
                        *input_pos += 1;
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                    }
                    while *input_pos < input.len() && ch != -1 {
                        let cu = ch as u8;
                        let digit = match cu {
                            b'0'..=b'9' => Some((cu - b'0') as u64),
                            b'a'..=b'f' => Some((cu - b'a' + 10) as u64),
                            b'A'..=b'F' => Some((cu - b'A' + 10) as u64),
                            _ => None,
                        };
                        match digit {
                            Some(d) => {
                                num = num * 16 + d;
                                got = true;
                            }
                            None => break,
                        }
                        *input_pos += 1;
                        ch = if *input_pos < input.len() {
                            input[*input_pos] as c_int
                        } else {
                            -1
                        };
                    }
                }
                if got {
                    if flags & FLAGS_IGNORE == 0 {
                        assignment += 1;
                        let ptr = unsafe { parameters[i].data.pointer };
                        if !ptr.is_null() {
                            unsafe {
                                *(ptr as *mut trio_pointer_t) = num as trio_pointer_t;
                            }
                        }
                    }
                } else {
                    // Try reading "(nil)" string
                    let nil_str = b"(nil)";
                    let mut matched = true;
                    for &expected in nil_str.iter() {
                        if *input_pos < input.len() && input[*input_pos] == expected {
                            *input_pos += 1;
                            ch = if *input_pos < input.len() {
                                input[*input_pos] as c_int
                            } else {
                                -1
                            };
                        } else {
                            matched = false;
                            break;
                        }
                    }
                    if matched && flags & FLAGS_IGNORE == 0 {
                        assignment += 1;
                        let ptr = unsafe { parameters[i].data.pointer };
                        if !ptr.is_null() {
                            unsafe {
                                *(ptr as *mut trio_pointer_t) = ptr::null_mut();
                            }
                        }
                    }
                }
            }
            FORMAT_GROUP => {
                return assignment;
            }
            _ => {
                return trio_error_return(TRIO_EINVAL, offset as c_int);
            }
        }

        ch = if *input_pos < input.len() {
            input[*input_pos] as c_int
        } else {
            -1
        };
        offset = parameters[i].end_offset as usize;
        i += 1;
    }

    assignment
}

fn width_limit(width: c_int) -> usize {
    if width <= 0 { 512 } else { width as usize }
}

// ============================================================================
// Public API functions - printf family
// ============================================================================

/// Print to stdout using FormatArgs.
pub fn trio_printf_fmt(format: &str, args: &mut FormatArgs) -> c_int {
    let format_bytes = format.as_bytes();
    let mut buf = Vec::new();
    let result = trio_format(&mut buf, format_bytes, args);
    if result >= 0 {
        use std::io::Write;
        let stdout = std::io::stdout();
        let _ = stdout.lock().write_all(&buf);
    }
    result
}

/// Print to a string buffer using FormatArgs.
pub unsafe fn trio_sprintf_fmt(buffer: *mut c_char, format: &str, args: &mut FormatArgs) -> c_int {
    let format_bytes = format.as_bytes();

    struct StringWriter {
        buffer: *mut c_char,
    }

    impl Write for StringWriter {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            for &byte in data {
                unsafe {
                    *self.buffer = byte as c_char;
                    self.buffer = self.buffer.add(1);
                }
            }
            Ok(data.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut writer = StringWriter { buffer };
    let result = trio_format(&mut writer, format_bytes, args);
    unsafe { *buffer = 0 };
    result
}

/// Print at most `max` characters to a string buffer using FormatArgs.
pub unsafe fn trio_snprintf_fmt(
    buffer: *mut c_char,
    max: usize,
    format: &str,
    args: &mut FormatArgs,
) -> c_int {
    let format_bytes = format.as_bytes();

    struct MaxStringWriter {
        buffer: *mut c_char,
        remaining: usize,
    }

    impl Write for MaxStringWriter {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            for &byte in data {
                if self.remaining > 1 {
                    unsafe {
                        *self.buffer = byte as c_char;
                        self.buffer = self.buffer.add(1);
                        self.remaining -= 1;
                    }
                }
            }
            Ok(data.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    if max == 0 || buffer.is_null() {
        let mut buf = Vec::new();
        return trio_format(&mut buf, format_bytes, args);
    }

    let mut writer = MaxStringWriter {
        buffer,
        remaining: max,
    };
    let result = trio_format(&mut writer, format_bytes, args);
    unsafe { *buffer = 0 };
    result
}

/// Scan from a string buffer using FormatArgs.
pub unsafe fn trio_sscanf_fmt(buffer: *const c_char, format: &str, args: &mut FormatArgs) -> c_int {
    let format_bytes = format.as_bytes();
    let input = if buffer.is_null() {
        &[] as &[u8]
    } else {
        unsafe { std::ffi::CStr::from_ptr(buffer).to_bytes() }
    };
    let mut input_pos = 0usize;

    let mut parameters: [TrioParameter; MAX_PARAMETERS] = [TRIO_PARAMETER_ZERO; MAX_PARAMETERS];
    parameters[MAX_PARAMETERS - 1].param_type = FORMAT_SENTINEL;

    let status = trio_parse(TYPE_SCAN, format, &mut parameters, args);
    if status < 0 {
        return status;
    }

    trio_scan_process(format_bytes, &parameters, input, &mut input_pos)
}

// ============================================================================
// C FFI public API
// ============================================================================

/// String match (case-insensitive wildcard matching).
pub unsafe fn trio_string_match(string: *const c_char, pattern: *const c_char) -> c_int {
    unsafe {
        let s = std::ffi::CStr::from_ptr(string).to_str().unwrap_or("");
        let p = std::ffi::CStr::from_ptr(pattern).to_str().unwrap_or("");
        if trio_match_impl(s, p) { 1 } else { 0 }
    }
}

/// String contains (substring search).
pub unsafe fn trio_string_contains(string: *const c_char, substring: *const c_char) -> c_int {
    unsafe {
        let s = std::ffi::CStr::from_ptr(string).to_str().unwrap_or("");
        let sub = std::ffi::CStr::from_ptr(substring).to_str().unwrap_or("");
        if s.contains(sub) { 1 } else { 0 }
    }
}

/// Case-insensitive wildcard match implementation.
fn trio_match_impl(string: &str, pattern: &str) -> bool {
    let mut s = string.chars().peekable();
    let mut p = pattern.chars().peekable();

    while let Some(pc) = p.next() {
        if pc != '*' {
            match s.next() {
                Some(sc) => {
                    if !sc.eq_ignore_ascii_case(&pc) && pc != '?' {
                        return false;
                    }
                }
                None => return false,
            }
        } else {
            // Skip consecutive stars
            while p.peek() == Some(&'*') {
                let _ = p.next();
            }
            if p.peek().is_none() {
                return true;
            }

            // Try matching rest of pattern against each suffix of string
            let rest_of_pattern: String = p.collect();
            loop {
                let remaining: String = s.clone().collect();
                #[allow(clippy::single_match)]
                match trio_match_impl(&remaining, &rest_of_pattern) {
                    true => return true,
                    false => {}
                }
                if s.next().is_none() {
                    return false;
                }
            }
        }
    }
    s.next().is_none()
}

// ============================================================================
// trio_match and trio_contains (R-compatible names)
// ============================================================================

/// Wildcard string match (case-insensitive).
pub unsafe fn trio_match(string: *const c_char, pattern: *const c_char) -> c_int {
    unsafe {
        let s = std::ffi::CStr::from_ptr(string).to_str().unwrap_or("");
        let p = std::ffi::CStr::from_ptr(pattern).to_str().unwrap_or("");
        if trio_match_impl(s, p) { 1 } else { 0 }
    }
}

/// Check if string contains substring.
pub unsafe fn trio_contains(string: *const c_char, substring: *const c_char) -> c_int {
    unsafe {
        let s = std::ffi::CStr::from_ptr(string).to_str().unwrap_or("");
        let sub = std::ffi::CStr::from_ptr(substring).to_str().unwrap_or("");
        if s.contains(sub) { 1 } else { 0 }
    }
}
