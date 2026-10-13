#![allow(non_camel_case_types, unused_assignments, unused_variables)]
use super::*;

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

pub(crate) fn trio_parse(
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

    if max_param >= 0 {
        for num in 0..=(max_param as usize) {
            if num < MAX_PARAMETERS && used_entries[num] != 1 {
                return trio_error_return(TRIO_EDBLREF, num as c_int);
            }
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

pub(crate) fn trio_format<W: Write>(out: &mut W, format: &[u8], args: &mut FormatArgs) -> c_int {
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
                                *(pointer as *mut c_char) = final_number as c_char;
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
                                *ptr.add(cnt as usize) = ch as c_char;
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
pub(crate) fn trio_match_impl(string: &str, pattern: &str) -> bool {
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

// ===========================================================================
// Tests
// ===========================================================================
