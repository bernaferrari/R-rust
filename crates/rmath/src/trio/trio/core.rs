#![allow(non_camel_case_types, unused_assignments, unused_variables)]
use super::*;

/// Long double type alias.
pub type trio_long_double_t = f64;

/// Pointer type.
pub(crate) type trio_pointer_t = *mut c_void;

/// Flags type.
type trio_flags_t = u64;

/// Maximal signed integer type.
pub(crate) type trio_intmax_t = i64;

/// Maximal unsigned integer type.
pub(crate) type trio_uintmax_t = u64;

// ============================================================================
// Error codes
// ============================================================================

pub(crate) const TRIO_EINVAL: c_int = 2;
pub(crate) const TRIO_ETOOMANY: c_int = 3;
pub(crate) const TRIO_EDBLREF: c_int = 4;

#[inline(always)]
pub(crate) fn trio_error_return(code: c_int, _pos: c_int) -> c_int {
    -code
}

// ============================================================================
// Format type constants
// ============================================================================

pub(crate) const TYPE_PRINT: c_int = 1;
pub(crate) const TYPE_SCAN: c_int = 2;

pub(crate) const FORMAT_SENTINEL: c_int = -1;
pub(crate) const FORMAT_UNKNOWN: c_int = 0;
pub(crate) const FORMAT_INT: c_int = 1;
pub(crate) const FORMAT_DOUBLE: c_int = 2;
pub(crate) const FORMAT_CHAR: c_int = 3;
pub(crate) const FORMAT_STRING: c_int = 4;
pub(crate) const FORMAT_POINTER: c_int = 5;
pub(crate) const FORMAT_COUNT: c_int = 6;
pub(crate) const FORMAT_PARAMETER: c_int = 7;
pub(crate) const FORMAT_GROUP: c_int = 8;
pub(crate) const FORMAT_ERRNO: c_int = 9;

// ============================================================================
// Flags
// ============================================================================

const FLAGS_NEW: trio_flags_t = 0;
const FLAGS_STICKY: trio_flags_t = 1;
const FLAGS_SPACE: trio_flags_t = 2;
const FLAGS_SHOWSIGN: trio_flags_t = 4;
pub(crate) const FLAGS_LEFTADJUST: trio_flags_t = 8;
pub(crate) const FLAGS_ALTERNATIVE: trio_flags_t = 16;
pub(crate) const FLAGS_SHORT: trio_flags_t = 32;
pub(crate) const FLAGS_SHORTSHORT: trio_flags_t = 64;
pub(crate) const FLAGS_LONG: trio_flags_t = 128;
pub(crate) const FLAGS_QUAD: trio_flags_t = 256;
const FLAGS_LONGDOUBLE: trio_flags_t = 512;
pub(crate) const FLAGS_SIZE_T: trio_flags_t = 1024;
pub(crate) const FLAGS_PTRDIFF_T: trio_flags_t = 2048;
pub(crate) const FLAGS_INTMAX_T: trio_flags_t = 4096;
pub(crate) const FLAGS_NILPADDING: trio_flags_t = 8192;
pub(crate) const FLAGS_UNSIGNED: trio_flags_t = 16384;
const FLAGS_UPPER: trio_flags_t = 32768;
const FLAGS_WIDTH: trio_flags_t = 65536;
pub(crate) const FLAGS_WIDTH_PARAMETER: trio_flags_t = 131072;
pub(crate) const FLAGS_PRECISION: trio_flags_t = 262144;
pub(crate) const FLAGS_PRECISION_PARAMETER: trio_flags_t = 524288;
const FLAGS_BASE: trio_flags_t = 1048576;
pub(crate) const FLAGS_BASE_PARAMETER: trio_flags_t = 2097152;
const FLAGS_FLOAT_E: trio_flags_t = 4194304;
const FLAGS_FLOAT_G: trio_flags_t = 8388608;
const FLAGS_QUOTE: trio_flags_t = 16777216;
const FLAGS_WIDECHAR: trio_flags_t = 33554432;
pub(crate) const FLAGS_IGNORE: trio_flags_t = 67108864;
const FLAGS_VARSIZE_PARAMETER: trio_flags_t = 268435456;
const FLAGS_FIXED_SIZE: trio_flags_t = 536870912;
const FLAGS_ROUNDING: trio_flags_t = FLAGS_INTMAX_T; // reuse

// ============================================================================
// Constants
// ============================================================================

pub(crate) const NO_POSITION: c_int = -1;
pub(crate) const NO_WIDTH: c_int = 0;
pub(crate) const NO_PRECISION: c_int = -1;
const NO_SIZE: c_int = -1;
pub(crate) const NO_BASE: c_int = -1;
const MAX_BASE: c_int = 36;
const BASE_BINARY: c_int = 2;
const BASE_OCTAL: c_int = 8;
pub(crate) const BASE_DECIMAL: c_int = 10;
pub(crate) const BASE_HEX: c_int = 16;

pub(crate) const MAX_PARAMETERS: usize = 64;
pub(crate) const CHAR_IDENTIFIER: c_char = b'%' as c_char;
const CHAR_BACKSLASH: c_char = b'\\' as c_char;

const INFINITE_LOWER: &[u8] = b"inf\0";
const INFINITE_UPPER: &[u8] = b"INF\0";
const NAN_LOWER: &[u8] = b"nan\0";
const NAN_UPPER: &[u8] = b"NAN\0";

pub(crate) const MAX_CHARS_IN_UINTMAX: usize = 64;

pub(crate) const POINTER_WIDTH: usize = 2 + std::mem::size_of::<usize>() * 2;

const DIGITS_LOWER: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const DIGITS_UPPER: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

// ============================================================================
// Internal state
// ============================================================================

pub(crate) static INTERNAL_NULL_STRING: &[u8] = b"(nil)\0";

// ============================================================================
// Parameter structure
// ============================================================================

pub(crate) const TRIO_PARAMETER_ZERO: TrioParameter = unsafe { std::mem::zeroed() };

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct TrioParameter {
    pub(crate) param_type: c_int,
    pub(crate) flags: trio_flags_t,
    pub(crate) width: c_int,
    pub(crate) precision: c_int,
    pub(crate) base: c_int,
    pub(crate) base_specifier: c_int,
    pub(crate) varsize: c_int,
    pub(crate) begin_offset: c_int,
    pub(crate) end_offset: c_int,
    pub(crate) position: c_int,
    pub(crate) data: TrioParameterData,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) union TrioParameterData {
    pub(crate) string: *mut c_char,
    pub(crate) pointer: trio_pointer_t,
    pub(crate) number: TrioParameterNumber,
    pub(crate) double_number: f64,
    pub(crate) longdouble_number: trio_long_double_t,
    pub(crate) double_pointer: *mut f64,
    pub(crate) longdouble_pointer: *mut trio_long_double_t,
    pub(crate) error_number: c_int,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) union TrioParameterNumber {
    pub(crate) as_signed: trio_intmax_t,
    pub(crate) as_unsigned: trio_uintmax_t,
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
pub(crate) fn trio_to_upper_char(ch: c_char) -> c_char {
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

pub(crate) fn trio_parse_qualifiers(
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

pub(crate) fn trio_parse_specifier(
    format: &[u8],
    offset: &mut usize,
    parameter: &mut TrioParameter,
) -> c_int {
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

pub(crate) fn trio_write_number<W: Write>(
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
    let number_start = pos;
    let number_len = MAX_CHARS_IN_UINTMAX - number_start;

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
                let _ = out.write(&buffer[number_start..MAX_CHARS_IN_UINTMAX]);
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
        let _ = out.write(&buffer[number_start..MAX_CHARS_IN_UINTMAX]);
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

pub(crate) fn trio_write_string<W: Write>(
    out: &mut W,
    string: *const c_char,
    flags: trio_flags_t,
    width: c_int,
    precision: c_int,
) {
    let null_str: &[u8] = b"(nil)";
    let (s, len): (*const c_char, usize) = if string.is_null() {
        (null_str.as_ptr() as *const c_char, null_str.len())
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
            let ch = *s.add(i);
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

pub(crate) fn trio_write_string_character<W: Write>(out: &mut W, ch: u8, flags: trio_flags_t) {
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

pub(crate) fn trio_write_double<W: Write>(
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
        let mut current_power = 10.0_f64.powi(integer_digits as i32 - 1);
        for _ in 0..integer_digits {
            let digit = (int_num / current_power).floor() as c_int;
            int_num -= digit as f64 * current_power;
            let _ = out.write(&[digits[digit.min(9).max(0) as usize]]);
            current_power /= base as f64;
        }
    } else {
        let _ = out.write(&[digits[0]]);
    }

    // Output fraction
    if precision > 0 || (flags & FLAGS_ALTERNATIVE != 0) {
        if keep_trailing {
            let _ = out.write(b".");
            for _ in 0..precision {
                fraction_part *= 10.0;
                let d = fraction_part.floor() as c_int;
                fraction_part -= d as f64;
                let _ = out.write(&[digits[d.min(9).max(0) as usize]]);
            }
        } else {
            // %g mode: collect digits, strip trailing zeros, skip dot if empty
            let mut frac_digits = Vec::new();
            for _ in 0..precision {
                fraction_part *= 10.0;
                let d = fraction_part.floor() as c_int;
                fraction_part -= d as f64;
                frac_digits.push(d.min(9).max(0) as usize);
            }
            while frac_digits.last() == Some(&0) {
                frac_digits.pop();
            }
            if !frac_digits.is_empty() {
                let _ = out.write(b".");
                for &d in &frac_digits {
                    let _ = out.write(&[digits[d]]);
                }
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
        while *s.add(len) != 0 as c_char {
            len += 1;
        }
        len
    }
}

// ============================================================================
// TrioFormatProcess - main formatting engine
// ============================================================================
