// Port of R's modules/internet/Rhttpd.c (1465 lines)
// R's built-in HTTP server - serves requests by evaluating httpd() function

use crate::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};
use crate::eval::eval::Rf_eval;
use crate::main::coerce::asInteger;
use crate::main::errors::Rf_error;
use crate::sexp::accessors::translateChar;
use crate::sexp::accessors::*;
use crate::sexp::constructors::{
    Rf_cons, Rf_lang3 as lang3, Rf_mkChar as mkChar, Rf_mkString as mkString, *,
};
use crate::sexp::envir::R_findVarInFrame as findVarInFrame;
use crate::sexp::ffi::{R_xlen_t, Rbyte, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::memory_ext::{vmaxget, vmaxset};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install as install;
use crate::sexp::*;
use core::ffi::{c_char, c_int, c_long, c_uint, c_void};
use libc::{
    AF_INET, FILE, INADDR_ANY, IPPROTO_TCP, SO_REUSEADDR, SOCK_STREAM, SOL_SOCKET, accept, bind,
    close, htonl, htons, in_addr, listen, recv, send, setsockopt, size_t, sockaddr, sockaddr_in,
    socket, socklen_t, ssize_t,
};
use std::alloc::{Layout, alloc, dealloc};

// ============================================================
// Constants
// ============================================================

const LINE_BUF_SIZE: usize = 1024;
const MAX_WORKERS: usize = 32;

const INVALID_SOCKET: c_int = -1;

/// Activity IDs for input handlers
const HttpdServerActivity: c_int = 8;
const HttpdWorkerActivity: c_int = 9;

// Request parts
const PART_REQUEST: c_char = 0;
const PART_HEADER: c_char = 1;
const PART_BODY: c_char = 2;

// HTTP methods
const METHOD_POST: c_char = 1;
const METHOD_GET: c_char = 2;
const METHOD_HEAD: c_char = 3;
const METHOD_OTHER: c_char = 8;

// Connection attributes
const CONNECTION_CLOSE: c_char = 0x01;
const HOST_HEADER: c_char = 0x02;
const HTTP_1_0: c_char = 0x04;
const CONTENT_LENGTH: c_char = 0x08;
const THREAD_OWNED: c_char = 0x10;
const THREAD_DISPOSE: c_char = 0x20;
const CONTENT_TYPE: c_char = 0x40;
const CONTENT_FORM_UENC: c_char = 0x80u8 as c_char;

// ============================================================
// External libc functions not available in libc crate directly
// ============================================================

unsafe extern "C" {
    fn inet_addr(cp: *const c_char) -> u32;
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn __error() -> *mut c_int;
}

#[cfg(not(target_os = "macos"))]
unsafe extern "C" {
    fn __errno_location() -> *mut c_int;
}

#[inline]
unsafe fn get_errno() -> c_int {
    unsafe {
        #[cfg(target_os = "macos")]
        {
            *__error()
        }
        #[cfg(not(target_os = "macos"))]
        {
            *__errno_location()
        }
    }
}

// ============================================================
// Buffer structure (doubly-linked list)
// ============================================================

#[repr(C)]
struct Buffer {
    next: *mut Buffer,
    prev: *mut Buffer,
    size: size_t,
    length: size_t,
    data: [c_char; 1], // flexible array member
}

// ============================================================
// Worker/connection structure
// ============================================================

#[repr(C)]
struct HttpdConn {
    sock: c_int,
    peer: in_addr,
    ih: *mut c_void, // InputHandler* on Unix
    line_buf: [c_char; LINE_BUF_SIZE],
    url: *mut c_char,
    body: *mut c_char,
    content_type: *mut c_char,
    line_pos: size_t,
    body_pos: size_t,
    content_length: c_long,
    part: c_char,
    method: c_char,
    attr: c_char,
    headers: *mut Buffer,
}

/// IS_HTTP_1_1(C)
#[inline(always)]
fn is_http_1_1(c: &HttpdConn) -> bool {
    (c.attr & HTTP_1_0) == 0
}

/// HTTP_SIG(C) - returns the HTTP/x.x string
#[inline(always)]
fn http_sig(c: &HttpdConn) -> &'static [u8] {
    if is_http_1_1(c) {
        b"HTTP/1.1"
    } else {
        b"HTTP/1.0"
    }
}

// ============================================================
// Runtime state
// ============================================================

pub(crate) struct HttpdRuntimeState {
    workers: [*mut HttpdConn; MAX_WORKERS],
    needs_init: c_int,
    srv_sock: c_int,
    in_process: c_int,
    ignore_sigpipe: c_int,
    content_type_name: SEXP,
    handlers_name: SEXP,
    custom_handlers_env: SEXP,
    srv_handler: *mut c_void,
}

impl Default for HttpdRuntimeState {
    fn default() -> Self {
        Self {
            workers: [std::ptr::null_mut(); MAX_WORKERS],
            needs_init: 1,
            srv_sock: INVALID_SOCKET,
            in_process: 0,
            ignore_sigpipe: 0,
            content_type_name: std::ptr::null_mut(),
            handlers_name: std::ptr::null_mut(),
            custom_handlers_env: std::ptr::null_mut(),
            srv_handler: std::ptr::null_mut(),
        }
    }
}

impl HttpdRuntimeState {
    /// Visit every R object retained by the HTTP server runtime.
    ///
    /// The collector uses this single seam for both tracing and reference
    /// updates, so adding another cached SEXP here cannot silently update only
    /// one half of the GC contract.
    pub(crate) fn visit_roots(&mut self, mut visit: impl FnMut(&mut SEXP)) {
        visit(&mut self.content_type_name);
        visit(&mut self.handlers_name);
        visit(&mut self.custom_handlers_env);
    }
}

impl Drop for HttpdRuntimeState {
    fn drop(&mut self) {
        unsafe {
            if self.srv_sock != INVALID_SOCKET {
                close(self.srv_sock);
                self.srv_sock = INVALID_SOCKET;
            }
            if !self.srv_handler.is_null() {
                removeInputHandler(R_InputHandlers(), self.srv_handler);
                self.srv_handler = std::ptr::null_mut();
            }
            for worker in &mut self.workers {
                if !worker.is_null() {
                    finalize_worker(*worker);
                    let layout = Layout::new::<HttpdConn>();
                    dealloc(*worker as *mut u8, layout);
                    *worker = std::ptr::null_mut();
                }
            }
        }
    }
}

fn with_httpd_state<R>(f: impl FnOnce(&mut HttpdRuntimeState) -> R) -> R {
    with_required_current_instance(|instance| f(&mut instance.httpd_state))
}

// ============================================================
// Local Rust adapters for R runtime services not yet represented as modules
// ============================================================

unsafe fn R_FindNamespace(_name: SEXP) -> SEXP {
    unsafe {
        Rf_error(b"HTTPD namespace lookup is not implemented\0".as_ptr() as *const c_char);
        R_NilValue()
    }
}

unsafe fn R_ToplevelExec(fun: Option<unsafe fn(*mut c_void)>, data: *mut c_void) -> c_int {
    unsafe {
        if let Some(fun) = fun {
            fun(data);
            1
        } else {
            0
        }
    }
}

unsafe fn LCONS(car: SEXP, cdr: SEXP) -> SEXP {
    unsafe {
        let cell = Rf_cons(car, cdr);
        if !cell.is_null() {
            (*cell).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        cell
    }
}

unsafe fn SET_TAG(x: SEXP, y: SEXP) {
    unsafe {
        SETTAG(x, y);
    }
}

unsafe fn list4(a: SEXP, b: SEXP, c: SEXP, d: SEXP) -> SEXP {
    unsafe { Rf_cons(a, Rf_cons(b, Rf_cons(c, Rf_cons(d, R_NilValue())))) }
}

unsafe fn addInputHandler(
    _handlers: *mut c_void,
    _fd: c_int,
    _handler: Option<unsafe fn(*mut c_void)>,
    _activity: c_int,
) -> *mut c_void {
    unsafe {
        Rf_error(b"HTTPD input handlers are not implemented\0".as_ptr() as *const c_char);
        std::ptr::null_mut()
    }
}

unsafe fn removeInputHandler(_handlers: *mut c_void, _ih: *mut c_void) {}

unsafe fn R_InputHandlers() -> *mut c_void {
    std::ptr::null_mut()
}

// ============================================================
// Initialization
// ============================================================

/// One-time initialization (Unix: no-op beyond setting flag)
fn first_init() {
    with_httpd_state(|state| state.needs_init = 0);
}

// ============================================================
// Buffer management
// ============================================================

/// Free a C string allocated by alloc_c_string. Null-safe.
unsafe fn free_c_string(s: *mut c_char) {
    unsafe {
        if s.is_null() {
            return;
        }
        let len = libc::strlen(s) + 1;
        let layout = Layout::from_size_align_unchecked(len, 1);
        dealloc(s as *mut u8, layout);
    }
}

/// Duplicate a C string using std::alloc instead of libc::strdup.
unsafe fn alloc_c_string(s: *const c_char) -> *mut c_char {
    unsafe {
        if s.is_null() {
            return std::ptr::null_mut();
        }
        let len = libc::strlen(s) + 1;
        let layout = Layout::from_size_align_unchecked(len, 1);
        let dst = alloc(layout) as *mut c_char;
        if !dst.is_null() {
            libc::memcpy(dst as *mut c_void, s as *const c_void, len);
        }
        dst
    }
}

/// Allocate `size` bytes of raw memory using std::alloc instead of libc::malloc.
unsafe fn alloc_raw(size: size_t) -> *mut c_void {
    unsafe {
        if size == 0 {
            return std::ptr::null_mut();
        }
        let layout = Layout::from_size_align_unchecked(size as usize, 1);
        alloc(layout) as *mut c_void
    }
}

/// Free memory allocated by alloc_raw.
unsafe fn free_raw(p: *mut c_void, size: size_t) {
    unsafe {
        if p.is_null() || size == 0 {
            return;
        }
        let layout = Layout::from_size_align_unchecked(size as usize, 1);
        dealloc(p as *mut u8, layout);
    }
}

/// Free buffers starting from the tail.
/// Safety: buf must have been allocated by alloc_buffer (via std::alloc).
unsafe fn free_buffer(buf: *mut Buffer) {
    unsafe {
        if buf.is_null() {
            return;
        }
        if !(*buf).prev.is_null() {
            free_buffer((*buf).prev);
        }
        // SAFETY: buf was allocated by alloc_buffer with this same layout.
        let size = (*buf).size as usize;
        let layout = Layout::from_size_align_unchecked(
            core::mem::size_of::<Buffer>() + size,
            core::mem::align_of::<Buffer>(),
        );
        dealloc(buf as *mut u8, layout);
    }
}

/// Allocate a new buffer with `size` bytes of data space after the Buffer header.
/// Uses std::alloc instead of libc::malloc to avoid libc allocator dependency.
unsafe fn alloc_buffer(size: c_int, parent: *mut Buffer) -> *mut Buffer {
    unsafe {
        let total_size = core::mem::size_of::<Buffer>() + size as usize;
        let layout = Layout::from_size_align_unchecked(total_size, core::mem::align_of::<Buffer>());
        let buf = alloc(layout) as *mut Buffer;
        if buf.is_null() {
            return buf;
        }
        (*buf).next = std::ptr::null_mut();
        (*buf).prev = parent;
        if !parent.is_null() {
            (*parent).next = buf;
        }
        (*buf).size = size as size_t;
        (*buf).length = 0;
        buf
    }
}

/// Convert doubly-linked buffers into one big raw vector
unsafe fn collect_buffers(buf: *mut Buffer) -> SEXP {
    unsafe {
        if buf.is_null() {
            return Rf_allocVector(SEXPTYPE::RAWSXP, 0);
        }
        let mut buf = buf;
        let mut len: c_int = 0;
        // count the total length and find the root
        while !(*buf).prev.is_null() {
            len += (*buf).length as c_int;
            buf = (*buf).prev;
        }
        let res = Rf_allocVector(SEXPTYPE::RAWSXP, len + (*buf).length as c_int);
        let _res_guard = protect(res);
        let dst = RAW(res) as *mut c_char;
        let mut pos: isize = 0;
        while !buf.is_null() {
            if (*buf).length > 0 {
                libc::memcpy(
                    dst.offset(pos) as *mut c_void,
                    (*buf).data.as_mut_ptr() as *const c_void,
                    (*buf).length,
                );
            }
            pos += (*buf).length as isize;
            buf = (*buf).next;
        }
        res
    }
}

// ============================================================
// Worker management
// ============================================================

/// Finalize a worker (close socket, free resources)
unsafe fn finalize_worker(c: *mut HttpdConn) {
    unsafe {
        // On Unix, remove input handler
        if !(*c).ih.is_null() {
            removeInputHandler(R_InputHandlers(), (*c).ih);
            (*c).ih = std::ptr::null_mut();
        }
        free_c_string((*c).url);
        (*c).url = std::ptr::null_mut();
        free_c_string((*c).body);
        (*c).body = std::ptr::null_mut();
        free_c_string((*c).content_type);
        (*c).content_type = std::ptr::null_mut();
        if !(*c).headers.is_null() {
            free_buffer((*c).headers);
            (*c).headers = std::ptr::null_mut();
        }
        if (*c).sock != INVALID_SOCKET {
            close((*c).sock);
            (*c).sock = INVALID_SOCKET;
        }
    }
}

/// Add a worker to the worker list. Returns 0 on success, -1 if full.
unsafe fn add_worker(c: *mut HttpdConn) -> c_int {
    unsafe {
        let mut i: c_uint = 0;
        while i < MAX_WORKERS as c_uint {
            if with_httpd_state(|state| state.workers[i as usize].is_null()) {
                with_httpd_state(|state| state.workers[i as usize] = c);
                return 0;
            }
            i += 1;
        }
        // No more space - finalize and free
        finalize_worker(c);
        let layout = Layout::new::<HttpdConn>();
        dealloc(c as *mut u8, layout);
        -1
    }
}

/// Remove a worker from the list. If thread-owned, set THREAD_DISPOSE flag.
unsafe fn remove_worker(c: *mut HttpdConn) {
    unsafe {
        if c.is_null() {
            return;
        }
        if (*c).attr & THREAD_OWNED != 0 {
            (*c).attr |= THREAD_DISPOSE;
            return;
        }
        finalize_worker(c);
        let mut i: c_uint = 0;
        while i < MAX_WORKERS as c_uint {
            if with_httpd_state(|state| state.workers[i as usize]) == c {
                with_httpd_state(|state| state.workers[i as usize] = std::ptr::null_mut());
            }
            i += 1;
        }
        let layout = Layout::new::<HttpdConn>();
        dealloc(c as *mut u8, layout);
    }
}

// ============================================================
// Network I/O helpers
// ============================================================

/// Build a sockaddr_in structure
unsafe fn build_sin(sa: *mut sockaddr_in, ip: *const c_char, port: c_int) -> *mut sockaddr {
    unsafe {
        libc::memset(sa as *mut c_void, 0, core::mem::size_of::<sockaddr_in>());
        (*sa).sin_family = AF_INET as libc::sa_family_t;
        (*sa).sin_port = htons(port as u16);
        (*sa).sin_addr.s_addr = if !ip.is_null() {
            inet_addr(ip)
        } else {
            htonl(INADDR_ANY as u32)
        };
        sa as *mut sockaddr
    }
}

/// Send data on a socket, handling SIGPIPE
unsafe fn send_response(s: c_int, buf: *const c_char, len: size_t) -> c_int {
    unsafe {
        let mut i: c_uint = 0;
        with_httpd_state(|state| state.ignore_sigpipe = 1);
        while i < len as c_uint {
            let flags: c_int = 0;
            let n = send(
                s,
                buf.offset(i as isize) as *const c_void,
                len - i as size_t,
                flags,
            );
            if n < 1 {
                with_httpd_state(|state| state.ignore_sigpipe = 0);
                return -1;
            }
            i += n as c_uint;
        }
        with_httpd_state(|state| state.ignore_sigpipe = 0);
        0
    }
}

/// Send `HTTP/x.x` plus the text (which should be of the form `"status message"`).
unsafe fn send_http_response(c: *mut HttpdConn, text: *const c_char) {
    unsafe {
        let sig = http_sig(&*c);
        let l = libc::strlen(text);
        let mut local_buf = [0 as c_char; 96];
        // reduce the number of packets by sending the payload en-block from buf
        if l < local_buf.len() - 10 {
            libc::memcpy(
                local_buf.as_mut_ptr() as *mut c_void,
                sig.as_ptr() as *const c_void,
                8,
            );
            libc::strcpy(local_buf.as_mut_ptr().offset(8), text);
            send_response((*c).sock, local_buf.as_ptr(), l + 8);
        } else {
            with_httpd_state(|state| state.ignore_sigpipe = 1);
            let res = send((*c).sock, sig.as_ptr() as *const c_void, 8, 0);
            with_httpd_state(|state| state.ignore_sigpipe = 0);
            if res < 8 {
                return;
            }
            send_response((*c).sock, text, l);
        }
    }
}

// ============================================================
// URI decoding
// ============================================================

/// Decode URI in place (decoding never expands)
unsafe fn uri_decode(s: *mut c_char) {
    unsafe {
        let mut src = s;
        let mut dst = s;
        while *src != 0 {
            if *src == b'+' as c_char {
                *dst = b' ' as c_char;
                dst = dst.offset(1);
                src = src.offset(1);
            } else if *src == b'%' as c_char {
                let mut ec: u8 = 0;
                src = src.offset(1);
                let ch = *src;
                if ch >= b'0' as c_char && ch <= b'9' as c_char {
                    ec |= ((ch - b'0' as c_char) as u8) << 4;
                } else if ch >= b'a' as c_char && ch <= b'f' as c_char {
                    ec |= ((ch - b'a' as c_char + 10) as u8) << 4;
                } else if ch >= b'A' as c_char && ch <= b'F' as c_char {
                    ec |= ((ch - b'A' as c_char + 10) as u8) << 4;
                }
                if *src != 0 {
                    src = src.offset(1);
                }
                let ch = *src;
                if ch >= b'0' as c_char && ch <= b'9' as c_char {
                    ec |= (ch - b'0' as c_char) as u8;
                } else if ch >= b'a' as c_char && ch <= b'f' as c_char {
                    ec |= (ch - b'a' as c_char + 10) as u8;
                } else if ch >= b'A' as c_char && ch <= b'F' as c_char {
                    ec |= (ch - b'A' as c_char + 10) as u8;
                }
                if *src != 0 {
                    src = src.offset(1);
                }
                *dst = ec as c_char;
                dst = dst.offset(1);
            } else {
                *dst = *src;
                dst = dst.offset(1);
                src = src.offset(1);
            }
        }
        *dst = 0;
    }
}

/// Decode a single hex character from URI encoding (advances the pointer)
unsafe fn hex_decode(s: *mut *mut c_char) -> u8 {
    unsafe {
        let mut ec: u8 = 0;
        let ch = **s;
        if ch >= b'0' as c_char && ch <= b'9' as c_char {
            ec |= ((ch - b'0' as c_char) as u8) << 4;
        } else if ch >= b'a' as c_char && ch <= b'f' as c_char {
            ec |= ((ch - b'a' as c_char + 10) as u8) << 4;
        } else if ch >= b'A' as c_char && ch <= b'F' as c_char {
            ec |= ((ch - b'A' as c_char + 10) as u8) << 4;
        }
        if **s != 0 {
            *s = (*s).offset(1);
        }
        let ch = **s;
        if ch >= b'0' as c_char && ch <= b'9' as c_char {
            ec |= (ch - b'0' as c_char) as u8;
        } else if ch >= b'a' as c_char && ch <= b'f' as c_char {
            ec |= (ch - b'a' as c_char + 10) as u8;
        } else if ch >= b'A' as c_char && ch <= b'F' as c_char {
            ec |= (ch - b'A' as c_char + 10) as u8;
        }
        if **s != 0 {
            *s = (*s).offset(1);
        }
        ec
    }
}

// ============================================================
// Query string parsing
// ============================================================

/// Parse a query string into a named character vector - must NOT be URI decoded
unsafe fn parse_query(query: *mut c_char) -> SEXP {
    unsafe {
        let mut parts: c_int = 0;
        let mut s = query;
        while *s != 0 {
            if *s == b'&' as c_char {
                parts += 1;
            }
            s = s.offset(1);
        }
        parts += 1;

        let res = Rf_allocVector(SEXPTYPE::STRSXP, parts);
        let _res_guard = protect(res);
        let names = Rf_allocVector(SEXPTYPE::STRSXP, parts);
        let _names_guard = protect(names);

        let mut s = query;
        let mut key: *mut c_char = std::ptr::null_mut();
        let mut value: *mut c_char = query;
        let mut t = query;
        parts = 0;
        loop {
            if *s == b'=' as c_char && key.is_null() {
                // first '=' in a part
                key = value;
                *t = 0;
                t = t.offset(1);
                value = t;
                s = s.offset(1);
            } else if *s == b'&' as c_char || *s == 0 {
                // next part
                let last_entry = *s == 0;
                *t = 0;
                t = t.offset(1);
                let key_str = if !key.is_null() {
                    key
                } else {
                    b"\0".as_ptr() as *mut c_char
                };
                SET_STRING_ELT(names, parts as R_xlen_t, mkChar(key_str));
                SET_STRING_ELT(res, parts as R_xlen_t, mkChar(value));
                parts += 1;
                if last_entry {
                    break;
                }
                key = std::ptr::null_mut();
                value = t;
                s = s.offset(1);
            } else if *s == b'+' as c_char {
                *t = b' ' as c_char;
                t = t.offset(1);
                s = s.offset(1);
            } else if *s == b'%' as c_char {
                // We cannot use uri_decode because we need &/= before decoding
                s = s.offset(1);
                let ec = hex_decode(&mut s);
                *t = ec as c_char;
                t = t.offset(1);
            } else {
                *t = *s;
                t = t.offset(1);
                s = s.offset(1);
            }
        }
        setAttrib(res, R_NamesSymbol(), names);
        res
    }
}

// ============================================================
// Request body parsing
// ============================================================

/// Create an object representing the request body.
unsafe fn parse_request_body(c: *mut HttpdConn) -> SEXP {
    unsafe {
        if c.is_null() || (*c).body.is_null() {
            return R_NilValue();
        }

        if (*c).attr & CONTENT_FORM_UENC != 0 {
            // URL encoded form - return parsed form
            *(*c).body.offset((*c).content_length as isize) = 0;
            return parse_query((*c).body);
        } else {
            // Something else - pass as raw vector
            let res = Rf_allocVector(SEXPTYPE::RAWSXP, (*c).content_length as c_int);
            let _res_guard = protect(res);
            if (*c).content_length > 0 {
                libc::memcpy(
                    RAW(res) as *mut c_void,
                    (*c).body as *const c_void,
                    (*c).content_length as size_t,
                );
            }
            if !(*c).content_type.is_null() {
                if with_httpd_state(|state| state.content_type_name).is_null() {
                    with_httpd_state(|state| {
                        state.content_type_name =
                            install(b"content-type\0".as_ptr() as *const c_char)
                    });
                }
                setAttrib(
                    res,
                    with_httpd_state(|state| state.content_type_name),
                    mkString((*c).content_type),
                );
            }
            res
        }
    }
}

// ============================================================
// Request finalization
// ============================================================

/// Finalize a request - for HTTP/1.0, close the connection
unsafe fn fin_request(c: *mut HttpdConn) {
    unsafe {
        if !is_http_1_1(&*c) {
            (*c).attr |= CONNECTION_CLOSE;
        }
    }
}

// ============================================================
// Custom handler lookup
// ============================================================

/// Returns an httpd handler (closure) for a given path.
unsafe fn handler_for_path(path: *const c_char) -> SEXP {
    unsafe {
        if !path.is_null() && libc::strncmp(path, b"/custom/\0".as_ptr() as *const c_char, 8) == 0 {
            let mut c = path.offset(8);
            let e = c;
            while *c != 0 && *c != b'/' as c_char {
                c = c.offset(1);
            }
            let name_len = c.offset_from(e) as isize;
            if name_len > 0 && name_len < 64 {
                let mut fn_buf = [0 as c_char; 64];
                libc::memcpy(
                    fn_buf.as_mut_ptr() as *mut c_void,
                    e as *const c_void,
                    name_len as size_t,
                );
                *fn_buf.as_mut_ptr().offset(name_len) = 0;

                // Cache custom_handlers_env
                if with_httpd_state(|state| state.custom_handlers_env).is_null() {
                    if with_httpd_state(|state| state.handlers_name).is_null() {
                        with_httpd_state(|state| {
                            state.handlers_name =
                                install(b".httpd.handlers.env\0".as_ptr() as *const c_char)
                        });
                    }
                    let tools_ns = R_FindNamespace(mkString(b"tools\0".as_ptr() as *const c_char));
                    let _tools_ns_guard = protect(tools_ns);
                    let eval_sym = install(b"eval\0".as_ptr() as *const c_char);
                    let call = lang3(
                        eval_sym,
                        with_httpd_state(|state| state.handlers_name),
                        tools_ns,
                    );
                    let _call_guard = protect(call);
                    with_httpd_state(|state| {
                        state.custom_handlers_env = Rf_eval(call, R_NilValue())
                    });
                }
                // Only proceed if .httpd.handlers.env really exists
                if TYPEOF(with_httpd_state(|state| state.custom_handlers_env)) == SEXPTYPE::ENVSXP {
                    let cl = findVarInFrame(
                        with_httpd_state(|state| state.custom_handlers_env),
                        install(fn_buf.as_ptr() as *const c_char),
                    );
                    if cl != R_UnboundValue() && TYPEOF(cl) == SEXPTYPE::CLOSXP {
                        return cl;
                    }
                }
            }
        }
        install(b"httpd\0".as_ptr() as *const c_char)
    }
}

// ============================================================
// Request processing (the actual R evaluation)
// ============================================================

/// Process a request by calling the httpd() function in R
unsafe fn process_request_(ptr: *mut c_void) {
    unsafe {
        let c = ptr as *mut HttpdConn;
        let mut ct: *const c_char = b"text/html\0".as_ptr() as *const c_char;
        let mut query: *mut c_char = std::ptr::null_mut();
        let mut s: *mut c_char;
        let mut s_headers = R_NilValue();
        let mut code: c_int = 200;
        let vmax = vmaxget();

        if c.is_null() || (*c).url.is_null() {
            return;
        }

        s = (*c).url;
        // find the query part
        while *s != 0 && *s != b'?' as c_char {
            s = s.offset(1);
        }
        if *s != 0 {
            *s = 0;
            s = s.offset(1);
            query = s;
        }
        uri_decode((*c).url);

        // construct "try(httpd(url, query, body), silent=TRUE)"
        let s_true = Rf_ScalarLogical(1); // TRUE
        let _s_true_guard = protect(s_true);
        let s_body = parse_request_body(c);
        let _s_body_guard = protect(s_body);
        let s_query = if !query.is_null() {
            parse_query(query)
        } else {
            R_NilValue()
        };
        let _s_query_guard = protect(s_query);
        let s_req_headers = if !(*c).headers.is_null() {
            collect_buffers((*c).headers)
        } else {
            R_NilValue()
        };
        let _s_req_headers_guard = protect(s_req_headers);
        let s_args = list4(mkString((*c).url), s_query, s_body, s_req_headers);
        let _s_args_guard = protect(s_args);
        let s_try = install(b"try\0".as_ptr() as *const c_char);
        let handler = handler_for_path((*c).url);
        let x = lang3(s_try, LCONS(handler, s_args), s_true);
        let _x_call_guard = protect(x);
        SET_TAG(CDR(CDR(x)), install(b"silent\0".as_ptr() as *const c_char));

        // evaluate the above in the tools namespace
        let tools_ns = R_FindNamespace(mkString(b"tools\0".as_ptr() as *const c_char));
        let _tools_ns_guard = protect(tools_ns);
        let eval_sym = install(b"eval\0".as_ptr() as *const c_char);
        let eval_call = lang3(eval_sym, x, tools_ns);
        let _eval_call_guard = protect(eval_call);
        let x = Rf_eval(eval_call, R_NilValue());
        let _x_guard = protect(x);

        // --- Handle the result ---

        if TYPEOF(x) == SEXPTYPE::STRSXP && LENGTH(x) > 0 {
            // String means there was an error
            let s = translateCharUTF8(STRING_ELT(x, 0));
            send_http_response(
                c,
                b" 500 Evaluation error\r\nConnection: close\r\nContent-type: text/plain\r\n\r\n\0"
                    .as_ptr() as *const c_char,
            );
            if (*c).method != METHOD_HEAD {
                send_response((*c).sock, s, libc::strlen(s));
            }
            (*c).attr |= CONNECTION_CLOSE;
            vmaxset(vmax);
            return;
        }

        if TYPEOF(x) == SEXPTYPE::VECSXP && LENGTH(x) > 0 {
            // A list (generic vector) can be a real payload
            let x_names = getAttrib(x, R_NamesSymbol());
            if LENGTH(x) > 1 {
                let s_ct = VECTOR_ELT(x, 1);
                if TYPEOF(s_ct) == SEXPTYPE::STRSXP && LENGTH(s_ct) > 0 {
                    ct = translateCharUTF8(STRING_ELT(s_ct, 0));
                }
                if LENGTH(x) > 2 {
                    s_headers = VECTOR_ELT(x, 2);
                    if TYPEOF(s_headers) != SEXPTYPE::STRSXP {
                        s_headers = R_NilValue();
                    }
                    if LENGTH(x) > 3 {
                        code = asInteger(VECTOR_ELT(x, 3));
                    }
                }
            }
            let y = VECTOR_ELT(x, 0);

            if TYPEOF(y) == SEXPTYPE::STRSXP && LENGTH(y) > 0 {
                // Character payload
                let mut buf = [0 as c_char; 64];
                let cs = translateCharUTF8(STRING_ELT(y, 0));
                let mut fn_ptr: *const c_char = std::ptr::null_mut();

                if code == 200 {
                    send_http_response(c, b" 200 OK\r\nContent-type: \0".as_ptr() as *const c_char);
                } else {
                    let sig = if is_http_1_1(&*c) {
                        b"HTTP/1.1"
                    } else {
                        b"HTTP/1.0"
                    };
                    libc::snprintf(
                        buf.as_mut_ptr(),
                        64,
                        b"%s %d Code %d\r\nContent-type: \0".as_ptr() as *const c_char,
                        sig.as_ptr(),
                        code,
                        code,
                    );
                    send_response((*c).sock, buf.as_ptr(), libc::strlen(buf.as_ptr()));
                }
                send_response((*c).sock, ct, libc::strlen(ct));

                // Append custom headers
                if s_headers != R_NilValue() {
                    let mut i: c_uint = 0;
                    let n = LENGTH(s_headers) as c_uint;
                    while i < n {
                        let hs = translateCharUTF8(STRING_ELT(s_headers, i as R_xlen_t));
                        send_response((*c).sock, b"\r\n\0".as_ptr() as *const c_char, 2);
                        send_response((*c).sock, hs, libc::strlen(hs));
                        i += 1;
                    }
                }

                // Special content - a file: either list(file="") or list(c("*FILE*", ""))
                if TYPEOF(x_names) == SEXPTYPE::STRSXP
                    && LENGTH(x_names) > 0
                    && libc::strcmp(
                        translateChar(STRING_ELT(x_names, 0)),
                        b"file\0".as_ptr() as *const c_char,
                    ) == 0
                {
                    fn_ptr = translateChar(STRING_ELT(y, 0));
                }
                if LENGTH(y) > 1 && libc::strcmp(cs, b"*FILE*\0".as_ptr() as *const c_char) == 0 {
                    fn_ptr = translateChar(STRING_ELT(y, 1));
                }

                if !fn_ptr.is_null() {
                    // Serve a file
                    let f = libc::fopen(fn_ptr, b"rb\0".as_ptr() as *const c_char);
                    let mut fsz: c_long = 0;
                    if f.is_null() {
                        send_response(
                            (*c).sock,
                            b"\r\nContent-length: 0\r\n\r\n\0".as_ptr() as *const c_char,
                            23,
                        );
                        fin_request(c);
                        vmaxset(vmax);
                        return;
                    }
                    libc::fseek(f, 0, libc::SEEK_END);
                    fsz = libc::ftell(f);
                    libc::fseek(f, 0, libc::SEEK_SET);
                    libc::snprintf(
                        buf.as_mut_ptr(),
                        64,
                        b"\r\nContent-length: %ld\r\n\r\n\0".as_ptr() as *const c_char,
                        fsz,
                    );
                    send_response((*c).sock, buf.as_ptr(), libc::strlen(buf.as_ptr()));
                    if (*c).method != METHOD_HEAD {
                        let mut fbuf = vec![0u8; 32768];
                        let mut remaining = fsz as size_t;
                        while remaining > 0 && libc::feof(f) == 0 {
                            let rd = if remaining > 32768 { 32768 } else { remaining };
                            if libc::fread(fbuf.as_mut_ptr() as *mut c_void, 1, rd, f) != rd {
                                (*c).attr |= CONNECTION_CLOSE;
                                libc::fclose(f);
                                vmaxset(vmax);
                                return;
                            }
                            send_response((*c).sock, fbuf.as_ptr() as *const c_char, rd);
                            remaining -= rd;
                        }
                    }
                    libc::fclose(f);
                    fin_request(c);
                    vmaxset(vmax);
                    return;
                }

                // Regular string content
                libc::snprintf(
                    buf.as_mut_ptr(),
                    64,
                    b"\r\nContent-length: %u\r\n\r\n\0".as_ptr() as *const c_char,
                    libc::strlen(cs) as c_uint,
                );
                send_response((*c).sock, buf.as_ptr(), libc::strlen(buf.as_ptr()));
                if (*c).method != METHOD_HEAD {
                    send_response((*c).sock, cs, libc::strlen(cs));
                }
                fin_request(c);
                vmaxset(vmax);
                return;
            }

            if TYPEOF(y) == SEXPTYPE::RAWSXP {
                // Raw payload
                let mut buf = [0 as c_char; 64];
                let cs = RAW(y);
                if code == 200 {
                    send_http_response(c, b" 200 OK\r\nContent-type: \0".as_ptr() as *const c_char);
                } else {
                    let sig = if is_http_1_1(&*c) {
                        b"HTTP/1.1"
                    } else {
                        b"HTTP/1.0"
                    };
                    libc::snprintf(
                        buf.as_mut_ptr(),
                        64,
                        b"%s %d Code %d\r\nContent-type: \0".as_ptr() as *const c_char,
                        sig.as_ptr(),
                        code,
                        code,
                    );
                    send_response((*c).sock, buf.as_ptr(), libc::strlen(buf.as_ptr()));
                }
                send_response((*c).sock, ct, libc::strlen(ct));
                if s_headers != R_NilValue() {
                    let mut i: c_uint = 0;
                    let n = LENGTH(s_headers) as c_uint;
                    while i < n {
                        let hs = translateCharUTF8(STRING_ELT(s_headers, i as R_xlen_t));
                        send_response((*c).sock, b"\r\n\0".as_ptr() as *const c_char, 2);
                        send_response((*c).sock, hs, libc::strlen(hs));
                        i += 1;
                    }
                }
                libc::snprintf(
                    buf.as_mut_ptr(),
                    64,
                    b"\r\nContent-length: %d\r\n\r\n\0".as_ptr() as *const c_char,
                    LENGTH(y),
                );
                send_response((*c).sock, buf.as_ptr(), libc::strlen(buf.as_ptr()));
                if (*c).method != METHOD_HEAD {
                    send_response((*c).sock, cs as *const c_char, LENGTH(y) as size_t);
                }
                fin_request(c);
                vmaxset(vmax);
                return;
            }
        }

        // Invalid response from R
        send_http_response(c, b" 500 Invalid response from R\r\nConnection: close\r\nContent-type: text/plain\r\n\r\nServer error: invalid response from R\r\n\0".as_ptr() as *const c_char);
        (*c).attr |= CONNECTION_CLOSE;
        vmaxset(vmax);
    }
}

/// Process request - wraps the actual call with ToplevelExec
unsafe fn process_request(c: *mut HttpdConn) {
    unsafe {
        with_httpd_state(|state| state.in_process = 1);
        R_ToplevelExec(Some(process_request_), c as *mut c_void);
        with_httpd_state(|state| state.in_process = 0);
    }
}

// ============================================================
// Path normalization (RFC 3986, 5.2.4)
// ============================================================

/// Remove . and (most) .. from "p" following RFC 3986, 5.2.4.
unsafe fn remove_dot_segments(p: *mut c_char) -> *mut c_char {
    unsafe {
        let in_len = libc::strlen(p);
        let mut inp_buf = Vec::with_capacity(in_len + 1);
        libc::memcpy(
            inp_buf.as_mut_ptr() as *mut c_void,
            p as *const c_void,
            in_len + 1,
        );
        inp_buf.set_len(in_len + 1);
        let mut inp: *mut c_char = inp_buf.as_mut_ptr();

        let mut out_buf = vec![0 as c_char; in_len + 1];
        let outbuf = out_buf.as_mut_ptr();
        let mut out = outbuf;
        *out = 0;

        while *inp != 0 {
            // A: remove "../" or "./" prefix
            if *inp == b'.' as c_char
                && *inp.offset(1) == b'.' as c_char
                && *inp.offset(2) == b'/' as c_char
            {
                inp = inp.offset(3);
                continue;
            }
            if *inp == b'.' as c_char && *inp.offset(1) == b'/' as c_char {
                inp = inp.offset(2);
                continue;
            }
            // B: replace "/./" or "/." with "/"
            if *inp == b'/' as c_char
                && *inp.offset(1) == b'.' as c_char
                && *inp.offset(2) == b'/' as c_char
            {
                inp = inp.offset(2);
                continue;
            }
            if *inp == b'/' as c_char && *inp.offset(1) == b'.' as c_char && *inp.offset(2) == 0 {
                *inp.offset(1) = 0;
                continue;
            }
            // C: replace "/../" or "/.." with "/" and remove last segment from output
            if *inp == b'/' as c_char
                && *inp.offset(1) == b'.' as c_char
                && *inp.offset(2) == b'.' as c_char
                && *inp.offset(3) == b'/' as c_char
            {
                inp = inp.offset(3);
                while out > outbuf && *out != b'/' as c_char {
                    out = out.offset(-1);
                }
                *out = 0;
                continue;
            }
            if *inp == b'/' as c_char
                && *inp.offset(1) == b'.' as c_char
                && *inp.offset(2) == b'.' as c_char
                && *inp.offset(3) == 0
            {
                *inp.offset(1) = 0;
                while out > outbuf && *out != b'/' as c_char {
                    out = out.offset(-1);
                }
                *out = 0;
                continue;
            }
            // D: if input is only "." or "..", remove it
            if (*inp == b'.' as c_char && *inp.offset(1) == 0)
                || (*inp == b'.' as c_char
                    && *inp.offset(1) == b'.' as c_char
                    && *inp.offset(2) == 0)
            {
                *inp = 0;
                continue;
            }
            // E: move the first path segment to the end of output
            if *inp == b'/' as c_char {
                *out = b'/' as c_char;
                out = out.offset(1);
                inp = inp.offset(1);
            }
            while *inp != 0 && *inp != b'/' as c_char {
                *out = *inp;
                out = out.offset(1);
                inp = inp.offset(1);
            }
            *out = 0;
        }

        inp_buf.set_len(0); // prevent double-free of Vec data
        let len = libc::strlen(out_buf.as_ptr() as *const c_char) + 1;
        let layout = Layout::from_size_align_unchecked(len, 1);
        let result = alloc(layout) as *mut c_char;
        libc::memcpy(
            result as *mut c_void,
            out_buf.as_ptr() as *const c_void,
            len,
        );
        out_buf.set_len(0); // prevent Vec from freeing
        result
    }
}

// ============================================================
// Worker reset for keep-alive connections
// ============================================================

/// Reset a worker so it can process a new request (keep-alive)
unsafe fn reset_worker(c: *mut HttpdConn) {
    unsafe {
        free_c_string((*c).url);
        (*c).url = std::ptr::null_mut();
        free_c_string((*c).body);
        (*c).body = std::ptr::null_mut();
        free_c_string((*c).content_type);
        (*c).content_type = std::ptr::null_mut();
        if !(*c).headers.is_null() {
            free_buffer((*c).headers);
            (*c).headers = std::ptr::null_mut();
        }
        (*c).line_pos = 0;
        (*c).body_pos = 0;
        (*c).method = 0;
        (*c).part = PART_REQUEST;
        (*c).attr = 0;
        (*c).content_length = 0;
    }
}

// ============================================================
// Worker input handler (processes incoming HTTP data)
// ============================================================

/// This function is called to fetch new data from the client connection socket and process it.
unsafe fn worker_input_handler(data: *mut c_void) {
    unsafe {
        let c = data as *mut HttpdConn;
        if c.is_null() {
            return;
        }

        if with_httpd_state(|state| state.in_process) != 0 {
            return; // don't allow recursive entrance
        }

        // --- Part: REQUEST / HEADERS ---
        if (*c).part < PART_BODY {
            let mut s = (*c).line_buf.as_mut_ptr();
            let n = recv(
                (*c).sock,
                (*c).line_buf.as_mut_ptr().add((*c).line_pos) as *mut c_void,
                LINE_BUF_SIZE - (*c).line_pos - 1,
                0,
            );
            if n < 0 {
                // error, scrap this worker
                remove_worker(c);
                return;
            }
            if n == 0 {
                // connection closed -> try to process and then remove
                process_request(c);
                remove_worker(c);
                return;
            }
            (*c).line_pos += n as size_t;
            (*c).line_buf[(*c).line_pos] = 0;

            while *s != 0 {
                // Empty line - end of headers
                if *s == b'\n' as c_char
                    || (*s == b'\r' as c_char && *s.offset(1) == b'\n' as c_char)
                {
                    // Check request validity
                    if (*c).attr & HTTP_1_0 == 0 && (*c).attr & HOST_HEADER == 0 {
                        send_http_response(
                            c,
                            b" 400 Bad Request (Host: missing)\r\nConnection: close\r\n\r\n\0"
                                .as_ptr() as *const c_char,
                        );
                        remove_worker(c);
                        return;
                    }
                    if (*c).attr & CONTENT_LENGTH != 0 && (*c).content_length != 0 {
                        if (*c).content_length < 0 || (*c).content_length > 2147483640 {
                            send_http_response(c, b" 413 Request Entity Too Large (request body too big)\r\nConnection: close\r\n\r\n\0".as_ptr() as *const c_char);
                            remove_worker(c);
                            return;
                        }
                        let body = alloc_raw((*c).content_length as size_t + 1) as *mut c_char;
                        if body.is_null() {
                            send_http_response(c, b" 413 Request Entity Too Large (request body too big)\r\nConnection: close\r\n\r\n\0".as_ptr() as *const c_char);
                            remove_worker(c);
                            return;
                        }
                        (*c).body = body;
                    }
                    (*c).body_pos = 0;
                    (*c).part = PART_BODY;
                    if *s == b'\r' as c_char {
                        s = s.offset(1);
                    }
                    s = s.offset(1);
                    // move the body part to the beginning of the buffer
                    let shift = s.offset_from((*c).line_buf.as_mut_ptr()) as size_t;
                    (*c).line_pos -= shift;
                    libc::memmove(
                        (*c).line_buf.as_mut_ptr() as *mut c_void,
                        s as *const c_void,
                        (*c).line_pos,
                    );

                    // GET/HEAD or no content length mean no body
                    if (*c).method == METHOD_GET
                        || (*c).method == METHOD_HEAD
                        || (*c).attr & CONTENT_LENGTH == 0
                        || (*c).content_length == 0
                    {
                        if (*c).attr & CONTENT_LENGTH != 0 && (*c).content_length > 0 {
                            send_http_response(
                                c,
                                b" 400 Bad Request (GET/HEAD with body)\r\n\r\n\0".as_ptr()
                                    as *const c_char,
                            );
                            remove_worker(c);
                            return;
                        }
                        process_request(c);
                        if (*c).attr & CONNECTION_CLOSE != 0 {
                            remove_worker(c);
                            return;
                        }
                        // keep-alive - reset the worker
                        reset_worker(c);
                        return;
                    }
                    // copy body content (as far as available)
                    (*c).body_pos = if (*c).content_length < (*c).line_pos as c_long {
                        (*c).content_length as size_t
                    } else {
                        (*c).line_pos
                    };
                    if (*c).body_pos > 0 {
                        libc::memcpy(
                            (*c).body as *mut c_void,
                            (*c).line_buf.as_mut_ptr() as *const c_void,
                            (*c).body_pos,
                        );
                    }
                    // POST will continue into the BODY part
                    break;
                }

                {
                    let bol = s;
                    // find end of line
                    while *s != 0 && *s != b'\r' as c_char && *s != b'\n' as c_char {
                        s = s.offset(1);
                    }
                    if *s == 0 {
                        // incomplete line
                        if bol == (*c).line_buf.as_mut_ptr() {
                            if (*c).line_pos < LINE_BUF_SIZE {
                                return;
                            }
                            // buffer full, line incomplete
                            send_http_response(
                                c,
                                b" 413 Request entity too large\r\nConnection: close\r\n\r\n\0"
                                    .as_ptr() as *const c_char,
                            );
                            remove_worker(c);
                            return;
                        }
                        // move the line to the beginning of the buffer
                        let shift = bol.offset_from((*c).line_buf.as_mut_ptr()) as size_t;
                        (*c).line_pos -= shift;
                        libc::memmove(
                            (*c).line_buf.as_mut_ptr() as *mut c_void,
                            bol as *const c_void,
                            (*c).line_pos,
                        );
                        return;
                    } else {
                        // complete line
                        if *s == b'\r' as c_char {
                            *s = 0;
                            s = s.offset(1);
                        }
                        if *s == b'\n' as c_char {
                            *s = 0;
                            s = s.offset(1);
                        }

                        if (*c).part == PART_REQUEST {
                            // --- Process request line ---
                            let rll = libc::strlen(bol);
                            let mut url = libc::strchr(bol, b' ' as c_int);
                            if url.is_null()
                                || rll < 14
                                || libc::strncmp(
                                    bol.offset(rll as isize - 9),
                                    b" HTTP/1.\0".as_ptr() as *const c_char,
                                    8,
                                ) != 0
                            {
                                send_response(
                                    (*c).sock,
                                    b"HTTP/1.0 400 Bad Request\r\n\r\n\0".as_ptr() as *const c_char,
                                    28,
                                );
                                remove_worker(c);
                                return;
                            }
                            url = url.offset(1);
                            *bol.offset(rll as isize - 9) = 0; // cut off " HTTP/1.x"
                            (*c).url = remove_dot_segments(url);
                            if libc::strncmp(
                                bol.offset(rll as isize - 3),
                                b"1.0\0".as_ptr() as *const c_char,
                                3,
                            ) == 0
                            {
                                (*c).attr |= HTTP_1_0;
                            }
                            if libc::strncmp(bol, b"GET \0".as_ptr() as *const c_char, 4) == 0 {
                                (*c).method = METHOD_GET;
                            }
                            if libc::strncmp(bol, b"POST \0".as_ptr() as *const c_char, 5) == 0 {
                                (*c).method = METHOD_POST;
                            }
                            if libc::strncmp(bol, b"HEAD \0".as_ptr() as *const c_char, 5) == 0 {
                                (*c).method = METHOD_HEAD;
                            }
                            // only custom handlers can use other methods
                            if libc::strncmp((*c).url, b"/custom/\0".as_ptr() as *const c_char, 8)
                                == 0
                            {
                                let mend = url.offset(-1);
                                if (*c).headers.is_null() {
                                    (*c).headers = alloc_buffer(1024, std::ptr::null_mut());
                                }
                                if !(*c).headers.is_null() {
                                    let available = (*(*c).headers).size - (*(*c).headers).length;
                                    let needed = 18 + (mend.offset_from(bol) as size_t);
                                    if available >= needed {
                                        if (*c).method == 0 {
                                            (*c).method = METHOD_OTHER;
                                        }
                                        libc::memcpy(
                                            (*(*c).headers)
                                                .data
                                                .as_mut_ptr()
                                                .add((*(*c).headers).length)
                                                as *mut c_void,
                                            b"Request-Method: \0".as_ptr() as *const c_void,
                                            16,
                                        );
                                        (*(*c).headers).length += 16;
                                        let mlen = mend.offset_from(bol) as size_t;
                                        if mlen > 0 {
                                            libc::memcpy(
                                                (*(*c).headers)
                                                    .data
                                                    .as_mut_ptr()
                                                    .add((*(*c).headers).length)
                                                    as *mut c_void,
                                                bol as *const c_void,
                                                mlen,
                                            );
                                        }
                                        (*(*c).headers).length += mlen;
                                        *(*(*c).headers)
                                            .data
                                            .as_mut_ptr()
                                            .add((*(*c).headers).length) = b'\n' as c_char;
                                        (*(*c).headers).length += 1;
                                    }
                                }
                            }
                            if (*c).method == 0 {
                                send_http_response(
                                    c,
                                    b" 501 Invalid or unimplemented method\r\n\r\n\0".as_ptr()
                                        as *const c_char,
                                );
                                remove_worker(c);
                                return;
                            }
                            (*c).part = PART_HEADER;
                        } else if (*c).part == PART_HEADER {
                            // --- Process headers ---
                            let mut k = bol;
                            if (*c).headers.is_null() {
                                (*c).headers = alloc_buffer(1024, std::ptr::null_mut());
                            }
                            if !(*c).headers.is_null() {
                                let l = libc::strlen(bol);
                                if l > 0 {
                                    if (*(*c).headers).length + l + 1 > (*(*c).headers).size {
                                        let fits = (*(*c).headers).size - (*(*c).headers).length;
                                        if fits > 0 {
                                            libc::memcpy(
                                                (*(*c).headers)
                                                    .data
                                                    .as_mut_ptr()
                                                    .add((*(*c).headers).length)
                                                    as *mut c_void,
                                                bol as *const c_void,
                                                fits,
                                            );
                                        }
                                        let new_buf = alloc_buffer(2048, (*c).headers);
                                        if !new_buf.is_null() {
                                            (*c).headers = new_buf;
                                            let leftover = l - fits;
                                            if leftover > 0 {
                                                libc::memcpy(
                                                    (*(*c).headers).data.as_mut_ptr()
                                                        as *mut c_void,
                                                    bol.add(fits) as *const c_void,
                                                    leftover,
                                                );
                                            }
                                            (*(*c).headers).length = leftover;
                                            *(*(*c).headers)
                                                .data
                                                .as_mut_ptr()
                                                .add((*(*c).headers).length) = b'\n' as c_char;
                                            (*(*c).headers).length += 1;
                                        }
                                    } else {
                                        libc::memcpy(
                                            (*(*c).headers)
                                                .data
                                                .as_mut_ptr()
                                                .add((*(*c).headers).length)
                                                as *mut c_void,
                                            bol as *const c_void,
                                            l,
                                        );
                                        (*(*c).headers).length += l;
                                        *(*(*c).headers)
                                            .data
                                            .as_mut_ptr()
                                            .add((*(*c).headers).length) = b'\n' as c_char;
                                        (*(*c).headers).length += 1;
                                    }
                                }
                            }
                            // Parse header key/value (convert key to lowercase)
                            while *k != 0 && *k != b':' as c_char {
                                if *k >= b'A' as c_char && *k <= b'Z' as c_char {
                                    *k |= 0x20;
                                }
                                k = k.offset(1);
                            }
                            if *k == b':' as c_char {
                                *k = 0;
                                k = k.offset(1);
                                while *k == b' ' as c_char || *k == b'\t' as c_char {
                                    k = k.offset(1);
                                }
                                if libc::strcmp(bol, b"content-length\0".as_ptr() as *const c_char)
                                    == 0
                                {
                                    (*c).attr |= CONTENT_LENGTH;
                                    (*c).content_length = libc::atol(k);
                                }
                                if libc::strcmp(bol, b"content-type\0".as_ptr() as *const c_char)
                                    == 0
                                {
                                    let mut l = k;
                                    // convert to lowercase up to ';'
                                    while *l != 0 && *l != b';' as c_char {
                                        if *l >= b'A' as c_char && *l <= b'Z' as c_char {
                                            *l |= 0x20;
                                        }
                                        l = l.offset(1);
                                    }
                                    (*c).attr |= CONTENT_TYPE;
                                    free_c_string((*c).content_type);
                                    (*c).content_type = alloc_c_string(k);
                                    if libc::strncmp(
                                        k,
                                        b"application/x-www-form-urlencoded\0".as_ptr()
                                            as *const c_char,
                                        33,
                                    ) == 0
                                    {
                                        (*c).attr |= CONTENT_FORM_UENC;
                                    }
                                }
                                if libc::strcmp(bol, b"host\0".as_ptr() as *const c_char) == 0 {
                                    (*c).attr |= HOST_HEADER;
                                }
                                if libc::strcmp(bol, b"connection\0".as_ptr() as *const c_char) == 0
                                {
                                    let mut l = k;
                                    while *l != 0 {
                                        if *l >= b'A' as c_char && *l <= b'Z' as c_char {
                                            *l |= 0x20;
                                        }
                                        l = l.offset(1);
                                    }
                                    if libc::strncmp(k, b"close\0".as_ptr() as *const c_char, 5)
                                        == 0
                                    {
                                        (*c).attr |= CONNECTION_CLOSE;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if (*c).part < PART_BODY {
                // processed a buffer of exactly one line
                (*c).line_pos = 0;
                return;
            }
        }

        // --- Part: BODY ---
        if (*c).part == PART_BODY && !(*c).body.is_null() {
            if ((*c).body_pos as c_long) < (*c).content_length {
                let n = recv(
                    (*c).sock,
                    (*c).body.add((*c).body_pos) as *mut c_void,
                    ((*c).content_length - (*c).body_pos as c_long) as size_t,
                    0,
                );
                (*c).line_pos = 0;
                if n < 0 {
                    remove_worker(c);
                    return;
                }
                if n == 0 {
                    process_request(c);
                    remove_worker(c);
                    return;
                }
                (*c).body_pos += n as size_t;
            }
            if (*c).body_pos as c_long == (*c).content_length {
                process_request(c);
                if (*c).attr & CONNECTION_CLOSE != 0 || (*c).line_pos != 0 {
                    remove_worker(c);
                    return;
                }
                reset_worker(c);
                return;
            }
        }

        // POST with no body yet
        if (*c).part == PART_BODY && (*c).body.is_null() {
            let s = (*c).line_buf.as_mut_ptr();
            if (*c).line_pos > 0 {
                if (*s != b'\r' as c_char || *s.offset(1) != b'\n' as c_char)
                    && *s != b'\n' as c_char
                {
                    send_http_response(
                        c,
                        b" 411 length is required for non-empty body\r\nConnection: close\r\n\r\n\0"
                            .as_ptr() as *const c_char,
                    );
                    remove_worker(c);
                    return;
                }
                process_request(c);
                if (*c).attr & CONNECTION_CLOSE != 0 {
                    remove_worker(c);
                    return;
                } else {
                    let mut sh: size_t = 1;
                    if *s == b'\r' as c_char {
                        sh = 2;
                    }
                    if (*c).line_pos <= sh {
                        (*c).line_pos = 0;
                    } else {
                        libc::memmove(
                            (*c).line_buf.as_mut_ptr() as *mut c_void,
                            (*c).line_buf.as_mut_ptr().add(sh) as *const c_void,
                            (*c).line_pos - sh,
                        );
                        (*c).line_pos -= sh;
                    }
                    reset_worker(c);
                    return;
                }
            }
            let n = recv(
                (*c).sock,
                (*c).line_buf.as_mut_ptr().add((*c).line_pos) as *mut c_void,
                LINE_BUF_SIZE - (*c).line_pos - 1,
                0,
            );
            if n < 0 {
                remove_worker(c);
                return;
            }
            if n == 0 {
                process_request(c);
                remove_worker(c);
                return;
            }
            if (*s != b'\r' as c_char || *s.offset(1) != b'\n' as c_char) && *s != b'\n' as c_char {
                send_http_response(
                    c,
                    b" 411 length is required for non-empty body\r\nConnection: close\r\n\r\n\0"
                        .as_ptr() as *const c_char,
                );
                remove_worker(c);
                return;
            }
        }
    }
}

// ============================================================
// Server input handler (accepts new connections)
// ============================================================

/// Server input handler - accepts new connections
unsafe fn srv_input_handler(_data: *mut c_void) {
    unsafe {
        let mut peer_sa: sockaddr_in = std::mem::zeroed();
        let mut peer_sal: socklen_t = core::mem::size_of::<sockaddr_in>() as socklen_t;
        let cl_sock = accept(
            with_httpd_state(|state| state.srv_sock),
            &mut peer_sa as *mut sockaddr_in as *mut sockaddr,
            &mut peer_sal,
        );
        if cl_sock == INVALID_SOCKET {
            return;
        }
        let c = Box::into_raw(Box::new(unsafe { std::mem::zeroed::<HttpdConn>() }));
        (*c).sock = cl_sock;
        (*c).peer = peer_sa.sin_addr;

        // Register as input handler
        (*c).ih = addInputHandler(
            R_InputHandlers(),
            cl_sock,
            Some(worker_input_handler),
            HttpdWorkerActivity,
        );
        if !(*c).ih.is_null() {
            // Set userData on the handler (it's the last field of InputHandler struct)
            let ih_ptr = (*c).ih as *mut u8;
            // InputHandler layout: activity(i32) -> fd(i32) -> handler(fn ptr) -> userData(*mut c_void)
            let ud_offset =
                core::mem::size_of::<c_int>() * 2 + core::mem::size_of::<*const c_void>();
            let ud_ptr = ih_ptr.add(ud_offset) as *mut *mut c_void;
            *ud_ptr = c as *mut c_void;
        }
        add_worker(c);
    }
}

// ============================================================
// Exported R interface functions
// ============================================================

/// Create an HTTP daemon on the given IP and port.
/// Returns 0 on success, -1 on general error, -2 if address already in use.
pub(crate) unsafe fn in_R_HTTPDCreate(ip: *const c_char, port: c_int) -> c_int {
    unsafe {
        let reuse: c_int = 1;
        let mut srv_sa: sockaddr_in = std::mem::zeroed();

        if with_httpd_state(|state| state.needs_init) != 0 {
            first_init();
        }

        // If already in use, close the current socket
        if with_httpd_state(|state| state.srv_sock) != INVALID_SOCKET {
            close(with_httpd_state(|state| state.srv_sock));
        }

        // Create a new socket
        with_httpd_state(|state| state.srv_sock = socket(AF_INET, SOCK_STREAM, 0));
        if with_httpd_state(|state| state.srv_sock) == INVALID_SOCKET {
            Rf_error(b"unable to create socket\0".as_ptr() as *const c_char);
            unreachable!();
        }

        // Set socket for reuse
        setsockopt(
            with_httpd_state(|state| state.srv_sock),
            SOL_SOCKET,
            SO_REUSEADDR,
            &reuse as *const c_int as *const c_void,
            core::mem::size_of::<c_int>() as u32,
        );

        // Bind to the desired port
        if bind(
            with_httpd_state(|state| state.srv_sock),
            build_sin(&mut srv_sa, ip, port),
            core::mem::size_of::<sockaddr_in>() as u32,
        ) != 0
        {
            let err = get_errno();
            if err == libc::EADDRINUSE {
                close(with_httpd_state(|state| state.srv_sock));
                with_httpd_state(|state| state.srv_sock = INVALID_SOCKET);
                return -2;
            } else {
                close(with_httpd_state(|state| state.srv_sock));
                with_httpd_state(|state| state.srv_sock = INVALID_SOCKET);
                Rf_error(b"unable to bind socket to TCP port\0".as_ptr() as *const c_char);
                unreachable!();
            }
        }

        // Setup listen
        if listen(with_httpd_state(|state| state.srv_sock), 8) != 0 {
            close(with_httpd_state(|state| state.srv_sock));
            with_httpd_state(|state| state.srv_sock = INVALID_SOCKET);
            Rf_error(b"cannot listen to TCP port\0".as_ptr() as *const c_char);
            unreachable!();
        }

        // Register the socket as an input handler
        if !with_httpd_state(|state| state.srv_handler).is_null() {
            removeInputHandler(
                R_InputHandlers(),
                with_httpd_state(|state| state.srv_handler),
            );
        }
        with_httpd_state(|state| {
            state.srv_handler = addInputHandler(
                R_InputHandlers(),
                state.srv_sock,
                Some(srv_input_handler),
                HttpdServerActivity,
            )
        });

        0
    }
}

/// Stop the HTTP daemon.
pub(crate) unsafe fn in_R_HTTPDStop() {
    unsafe {
        if with_httpd_state(|state| state.srv_sock) != INVALID_SOCKET {
            close(with_httpd_state(|state| state.srv_sock));
            with_httpd_state(|state| state.srv_sock = INVALID_SOCKET);
        }
        if !with_httpd_state(|state| state.srv_handler).is_null() {
            removeInputHandler(
                R_InputHandlers(),
                with_httpd_state(|state| state.srv_handler),
            );
            with_httpd_state(|state| state.srv_handler = std::ptr::null_mut());
        }
    }
}

/// R_init_httpd - Create an internal HTTP server in R.
/// @param sIP is the IP to bind to (or R_NilValue for any)
/// @param sPort is the TCP port number to bind to
/// @return integer: 0 on success, -2 means address already in use
pub(crate) unsafe fn R_init_httpd(sIP: SEXP, sPort: SEXP) -> SEXP {
    unsafe {
        let mut ip: *const c_char = std::ptr::null_mut();
        let vmax = vmaxget();

        if sIP != R_NilValue() && (TYPEOF(sIP) != SEXPTYPE::STRSXP || LENGTH(sIP) != 1) {
            Rf_error(b"invalid bind address specification\0".as_ptr() as *const c_char);
            unreachable!();
        }
        if sIP != R_NilValue() {
            ip = translateChar(STRING_ELT(sIP, 0));
        }
        let ans = Rf_ScalarInteger(in_R_HTTPDCreate(ip, asInteger(sPort)));
        vmaxset(vmax);
        ans
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::instance::{RInstance, replace_current_instance};

    #[test]
    fn httpd_runtime_state_is_session_local() {
        let mut first = RInstance::new();
        let mut second = RInstance::new();
        let first_symbol = first.base_env;
        let second_symbol = second.global_env;

        unsafe {
            let previous = replace_current_instance(Some(&mut first as *mut RInstance));
            with_httpd_state(|state| {
                state.needs_init = 0;
                state.in_process = 1;
                state.ignore_sigpipe = 1;
                state.content_type_name = first_symbol;
            });
            replace_current_instance(previous);

            let previous = replace_current_instance(Some(&mut second as *mut RInstance));
            assert_eq!(with_httpd_state(|state| state.needs_init), 1);
            assert_eq!(with_httpd_state(|state| state.in_process), 0);
            with_httpd_state(|state| state.handlers_name = second_symbol);
            replace_current_instance(previous);
        }

        assert_eq!(first.httpd_state.needs_init, 0);
        assert_eq!(first.httpd_state.in_process, 1);
        assert_eq!(first.httpd_state.ignore_sigpipe, 1);
        assert_eq!(first.httpd_state.content_type_name, first_symbol);
        assert_eq!(second.httpd_state.needs_init, 1);
        assert_eq!(second.httpd_state.in_process, 0);
        assert_eq!(second.httpd_state.handlers_name, second_symbol);
        assert!(second.httpd_state.content_type_name.is_null());
    }
}
