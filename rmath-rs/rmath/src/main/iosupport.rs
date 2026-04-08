#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/iosupport.c
//!
//! Provides IoBuffer and TextBuffer types for R's parser I/O, offering
//! a uniform interface for reading data from the console, files, and
//! internal text strings.

use std::cell::RefCell;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::main::sysutils::translateChar;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::{R_alloc, vmaxget, vmaxset};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Size of each buffer block in IoBuffer.
pub const IOBSIZE: usize = 4096;

// ---------------------------------------------------------------------------
// Stub encoding helpers
// ---------------------------------------------------------------------------

/// Stub: always returns false in this port.
unsafe fn IS_LATIN1(_x: SEXP) -> bool {
    false
}

/// Stub: always returns false in this port.
const mbcslocale: bool = false;

/// Stub: always returns false in this port.
const known_to_be_utf8: bool = false;

// ---------------------------------------------------------------------------
// BufferListItem
// ---------------------------------------------------------------------------

/// A single node in the linked list of I/O buffer blocks.
///
/// Each node holds an IOBSIZE-byte buffer and a pointer to the next node.
#[repr(C)]
pub struct BufferListItem {
    pub buf: [u8; IOBSIZE],
    pub next: *mut BufferListItem,
}

impl BufferListItem {
    fn new() -> *mut BufferListItem {
        let boxed = Box::new(BufferListItem {
            buf: [0u8; IOBSIZE],
            next: ptr::null_mut(),
        });
        Box::into_raw(boxed)
    }
}

// ---------------------------------------------------------------------------
// IoBuffer
// ---------------------------------------------------------------------------

/// A buffered I/O structure backed by a linked list of BufferListItem nodes.
///
/// Supports sequential writing and reading. Used for console I/O and
/// internal string parsing in R.
#[repr(C)]
pub struct IoBuffer {
    pub start_buf: *mut BufferListItem,
    pub write_buf: *mut BufferListItem,
    pub write_ptr: *mut u8,
    pub write_offset: c_int,
    pub read_buf: *mut BufferListItem,
    pub read_ptr: *mut u8,
    pub read_offset: c_int,
}

/// Console IO Buffer (global mutable).
pub thread_local! { static R_ConsoleIob: RefCell<IoBuffer> = RefCell::new(IoBuffer {
    start_buf: ptr::null_mut(),
    write_buf: ptr::null_mut(),
    write_ptr: ptr::null_mut(),
    write_offset: 0,
    read_buf: ptr::null_mut(),
    read_ptr: ptr::null_mut(),
    read_offset: 0,
}); }

// ---------------------------------------------------------------------------
// TextBuffer
// ---------------------------------------------------------------------------

/// A text buffer for reading from an R character vector (STRSXP).
///
/// Translates elements one at a time, appending a newline after each.
#[repr(C)]
pub struct TextBuffer {
    pub vmax: *mut c_void,
    pub buf: *mut u8,
    pub bufp: *mut u8,
    pub text: SEXP,
    pub ntext: c_int,
    pub offset: c_int,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Move the iob write pointer to the next BufferListItem in the chain.
/// If there is no next buffer item, one is allocated.
unsafe fn NextWriteBufferListItem(iob: *mut IoBuffer) -> c_int {
    unsafe {
        let iob = &mut *iob;
        if !(*iob.write_buf).next.is_null() {
            iob.write_buf = (*iob.write_buf).next;
        } else {
            let new_item = BufferListItem::new();
            if new_item.is_null() {
                return 0;
            }
            (*iob.write_buf).next = new_item;
            iob.write_buf = new_item;
        }
        iob.write_ptr = (*iob.write_buf).buf.as_mut_ptr();
        iob.write_offset = 0;
        1
    }
}

/// Move the iob read pointer to the next BufferListItem in the chain.
unsafe fn NextReadBufferListItem(iob: *mut IoBuffer) -> c_int {
    unsafe {
        let iob = &mut *iob;
        iob.read_buf = (*iob.read_buf).next;
        iob.read_ptr = (*iob.read_buf).buf.as_mut_ptr();
        iob.read_offset = 0;
        1
    }
}

/// Copy characters from `q` (null-terminated C string) into `p`,
/// appending a newline and null terminator.
unsafe fn transferChars(p: *mut u8, q: *const c_char) {
    unsafe {
        let mut dst = p;
        let mut src = q;
        while *src != 0 {
            *dst = *src as u8;
            dst = dst.add(1);
            src = src.add(1);
        }
        *dst = b'\n';
        dst = dst.add(1);
        *dst = 0;
    }
}

/// Respect encoding override from parser invocation (do_parse).
///
/// A hack to allow UTF-8 string literals and comments when parsing
/// on Windows. Falls back to translateChar for normal encoding handling.
unsafe fn translateCharWithOverride(x: SEXP) -> *const c_char {
    unsafe {
        if !IS_LATIN1(x) && !mbcslocale && known_to_be_utf8 {
            CHAR(x)
        } else {
            translateChar(x)
        }
    }
}

// ---------------------------------------------------------------------------
// IoBuffer public API
// ---------------------------------------------------------------------------

/// Reset the read/write pointers of an IoBuffer back to the start.
pub unsafe fn R_IoBufferWriteReset(iob: *mut IoBuffer) -> c_int {
    unsafe {
        if iob.is_null() || (*iob).start_buf.is_null() {
            return 0;
        }
        (*iob).write_buf = (*iob).start_buf;
        (*iob).write_ptr = (*(*iob).write_buf).buf.as_mut_ptr();
        (*iob).write_offset = 0;
        (*iob).read_buf = (*iob).start_buf;
        (*iob).read_ptr = (*(*iob).read_buf).buf.as_mut_ptr();
        (*iob).read_offset = 0;
        1
    }
}

/// Reset the read pointer of an IoBuffer back to the start.
pub unsafe fn R_IoBufferReadReset(iob: *mut IoBuffer) -> c_int {
    unsafe {
        if iob.is_null() || (*iob).start_buf.is_null() {
            return 0;
        }
        (*iob).read_buf = (*iob).start_buf;
        (*iob).read_ptr = (*(*iob).read_buf).buf.as_mut_ptr();
        (*iob).read_offset = 0;
        1
    }
}

/// Allocate an initial BufferListItem for IoBuffer and reset pointers.
pub unsafe fn R_IoBufferInit(iob: *mut IoBuffer) -> c_int {
    unsafe {
        if iob.is_null() {
            return 0;
        }
        (*iob).start_buf = BufferListItem::new();
        if (*iob).start_buf.is_null() {
            return 0;
        }
        (*(*iob).start_buf).next = ptr::null_mut();
        R_IoBufferWriteReset(iob)
    }
}

/// Free all BufferListItem nodes associated with an IoBuffer.
pub unsafe fn R_IoBufferFree(iob: *mut IoBuffer) -> c_int {
    unsafe {
        if iob.is_null() || (*iob).start_buf.is_null() {
            return 0;
        }
        let mut this_item = (*iob).start_buf;
        while !this_item.is_null() {
            let next_item = (*this_item).next;
            drop(Box::from_raw(this_item));
            this_item = next_item;
        }
        // Reset pointers to NULL so other calls can detect the freed state
        (*iob).start_buf = ptr::null_mut();
        (*iob).write_buf = ptr::null_mut();
        (*iob).write_ptr = ptr::null_mut();
        (*iob).read_buf = ptr::null_mut();
        (*iob).read_ptr = ptr::null_mut();
        1
    }
}

/// Add a character to an IoBuffer.
pub unsafe fn R_IoBufferPutc(c: c_int, iob: *mut IoBuffer) -> c_int {
    unsafe {
        if (*iob).write_offset == IOBSIZE as c_int {
            NextWriteBufferListItem(iob);
        }
        *(*iob).write_ptr = c as u8;
        (*iob).write_ptr = (*iob).write_ptr.add(1);
        (*iob).write_offset += 1;
        0 // not used
    }
}

/// Add a null-terminated string to an IoBuffer.
pub unsafe fn R_IoBufferPuts(s: *mut c_char, iob: *mut IoBuffer) -> c_int {
    unsafe {
        let mut p = s;
        let mut n: c_int = 0;
        while *p != 0 {
            R_IoBufferPutc(*p as c_int, iob);
            n += 1;
            p = p.add(1);
        }
        n
    }
}

/// Read a character from an IoBuffer.
pub unsafe fn R_IoBufferGetc(iob: *mut IoBuffer) -> c_int {
    unsafe {
        if (*iob).read_buf == (*iob).write_buf && (*iob).read_offset >= (*iob).write_offset {
            return -1; // EOF
        }
        if (*iob).read_offset == IOBSIZE as c_int {
            NextReadBufferListItem(iob);
        }
        (*iob).read_offset += 1;
        let ch = *(*iob).read_ptr;
        (*iob).read_ptr = (*iob).read_ptr.add(1);
        ch as c_int
    }
}

/// Compute the current read offset, taking all buffer blocks into account.
pub unsafe fn R_IoBufferReadOffset(iob: *mut IoBuffer) -> c_int {
    unsafe {
        let mut result = (*iob).read_offset;
        let mut buf = (*iob).start_buf;
        while !buf.is_null() && buf != (*iob).read_buf {
            result += IOBSIZE as c_int;
            buf = (*buf).next;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// TextBuffer public API
// ---------------------------------------------------------------------------

/// Initialize a TextBuffer from an R character vector (STRSXP).
///
/// If `text` is a string vector, prepares for sequential reading.
/// Otherwise, sets the buffer to NULL (returns EOF on read).
pub unsafe fn R_TextBufferInit(txtb: *mut TextBuffer, text: SEXP) -> c_int {
    unsafe {
        if Rf_isString(text) != 0 {
            // translateChar might allocate
            let vmax = vmaxget();
            let n = Rf_length(text);
            let mut l: c_int = 0;
            for i in 0..n {
                let elt = STRING_ELT(text, i as crate::sexp::ffi::R_xlen_t);
                if elt != R_NilValue() {
                    let translated = translateCharWithOverride(elt);
                    let k = libc::strlen(translated) as c_int;
                    if k > l {
                        l = k;
                    }
                }
            }
            vmaxset(vmax);
            (*txtb).vmax = vmaxget();
            (*txtb).buf = R_alloc((l + 2) as usize, std::mem::size_of::<c_char>()) as *mut u8;
            (*txtb).bufp = (*txtb).buf;
            (*txtb).text = text;
            (*txtb).ntext = n;
            (*txtb).offset = 0;
            transferChars(
                (*txtb).buf,
                translateCharWithOverride(STRING_ELT(
                    (*txtb).text,
                    (*txtb).offset as crate::sexp::ffi::R_xlen_t,
                )),
            );
            (*txtb).offset += 1;
            1
        } else {
            (*txtb).vmax = vmaxget();
            (*txtb).buf = ptr::null_mut();
            (*txtb).bufp = ptr::null_mut();
            (*txtb).text = R_NilValue();
            (*txtb).ntext = 0;
            (*txtb).offset = 1;
            0
        }
    }
}

/// Finalize a TextBuffer, releasing transient memory.
pub unsafe fn R_TextBufferFree(txtb: *mut TextBuffer) -> c_int {
    unsafe {
        vmaxset((*txtb).vmax);
        0 // not used
    }
}

/// Read a character from a TextBuffer.
pub unsafe fn R_TextBufferGetc(txtb: *mut TextBuffer) -> c_int {
    unsafe {
        if (*txtb).buf.is_null() {
            return -1; // EOF
        }
        if *(*txtb).bufp == 0 {
            if (*txtb).offset == (*txtb).ntext {
                (*txtb).buf = ptr::null_mut();
                return -1; // EOF
            } else {
                let _vmax = vmaxget();
                transferChars(
                    (*txtb).buf,
                    translateCharWithOverride(STRING_ELT(
                        (*txtb).text,
                        (*txtb).offset as crate::sexp::ffi::R_xlen_t,
                    )),
                );
                (*txtb).bufp = (*txtb).buf;
                (*txtb).offset += 1;
                vmaxset(_vmax);
            }
        }
        let ch = *(*txtb).bufp;
        (*txtb).bufp = (*txtb).bufp.add(1);
        ch as c_int
    }
}
