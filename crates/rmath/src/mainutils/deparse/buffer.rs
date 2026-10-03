#![allow(non_snake_case)]

use super::owned_line_buffer::{LineBufferError, OwnedLineBuffer};
use super::*;

pub(super) fn buffer_error(error: LineBufferError) -> ! {
    let message = match error {
        LineBufferError::TooLong => "deparse line is too long",
        LineBufferError::Allocation => "cannot allocate deparse line buffer",
    };
    // Buffer failure must unwind directly rather than recursively deparsing
    // the same call while trying to format an allocation failure.
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.to_owned(),
    })
}

/// Append bounded payload bytes, including indentation at the start of a line.
/// This is the only path which grows the canonical owned line buffer.
pub(crate) fn append_line_bytes(bytes: &[u8], data: &mut LocalParseData) {
    let indentation = if data.startline {
        OwnedLineBuffer::indentation(data.indent).unwrap_or_else(|error| buffer_error(error))
    } else {
        0
    };
    data.len = data
        .buffer
        .append(bytes, indentation)
        .unwrap_or_else(|error| buffer_error(error));
    data.startline = false;
}

/// Native input adapter. The caller supplies a valid terminated string and a
/// live exclusive parse state. Only the bounded CStr payload is appended.
pub unsafe fn print2buff(string: *const c_char, data: *mut LocalParseData) {
    if string.is_null() {
        return;
    }
    // SAFETY: the translated caller provides the source C string and state.
    let bytes = unsafe { std::ffi::CStr::from_ptr(string) }.to_bytes();
    append_line_bytes(bytes, unsafe { &mut *data });
}

/// Flush the actual owned bytes into an owning character value. The safe
/// factory copies a bounded slice before its deferred allocation callbacks.
pub unsafe fn writeline(data: *mut LocalParseData) {
    let data = unsafe { &mut *data };
    if !unsafe { isNull(data.strvec) } && data.linenumber < data.maxlines {
        // Capture the output's owning token before allocating the character.
        let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|_| buffer_error(LineBufferError::Allocation));
        let factory = owner.node_factory();
        let output = factory
            .wrap(data.strvec)
            .unwrap_or_else(|_| buffer_error(LineBufferError::Allocation));
        let mut output = crate::sexp::object::SexpMut::try_from_checked(output)
            .unwrap_or_else(|_| buffer_error(LineBufferError::Allocation));
        let character = factory
            .allocate(|arena| Some(arena.alloc_charsxp(data.buffer.bytes())))
            .unwrap_or_else(|_| buffer_error(LineBufferError::Allocation));
        output
            .try_set_string_elt(data.linenumber as R_xlen_t, character)
            .unwrap_or_else(|_| buffer_error(LineBufferError::Allocation));
    }
    data.linenumber = data
        .linenumber
        .checked_add(1)
        .unwrap_or_else(|| buffer_error(LineBufferError::TooLong));
    if data.linenumber >= data.maxlines {
        data.active = false;
    }
    data.buffer.clear();
    data.len = 0;
    data.startline = true;
}

pub unsafe fn linebreak(should_break: *mut bool, data: *mut LocalParseData) {
    let data = unsafe { &mut *data };
    if data.len > data.cutoff {
        if !unsafe { *should_break } {
            unsafe {
                *should_break = true;
            }
            data.indent = data
                .indent
                .checked_add(1)
                .unwrap_or_else(|| buffer_error(LineBufferError::TooLong));
        }
        unsafe {
            writeline(data);
        }
    }
}
