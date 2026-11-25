// Port of R's modules/internet/Rsock.c (705 lines)
// Unix socket operations used by the utils socket wrappers.

use crate::sexp::memory_ext::R_alloc;
use crate::nmath::special::mlutils::REprintf;
use core::ffi::{c_char, c_int, c_void};
use libc::size_t;

use super::sock::{
    R_socket_strerror, Sock_close, Sock_connect, Sock_init, Sock_listen, Sock_open, Sock_read,
    Sock_write,
};

// Socket port type (matches C: typedef unsigned short Sock_port_t)
type Sock_port_t = u16;

// --- Internal helper functions (module-private, no #[no_mangle]) ---

/// enter_sock - validate socket fd
/// Returns 0 if fd == -1 (invalid), otherwise returns fd.
fn enter_sock(fd: c_int) -> c_int {
    if fd == -1 { 0 } else { fd }
}

/// close_sock - close a socket via Sock_close with error reporting
fn close_sock(fd: c_int) -> c_int {
    let mut perr = super::sock::Sock_error_t::default();
    // SAFETY: `perr` is a live local `Sock_error_t`.
    let res = unsafe { Sock_close(fd, &mut perr) };
    if res == -1 {
        REprintf(c"socket error: %s\n");
        // Note: the C code uses REprintf with format string for the error message.
        // Since our REprintf only takes a plain string (no varargs), we print a generic message.
        // The error number is available via perr.error.
        let _ = perr;
        return -1;
    }
    0
}

/// check_init - ensure socket subsystem is initialized (once)
fn check_init() {
    super::internet::with_internet_state(|state| {
        if state.sock_inited == 0 {
            Sock_init();
            state.sock_inited = 1;
        }
    });
}

// --- Exported R interface functions (from sock.h) ---

/// in_Rsockopen - open a socket for listening (R .C interface)
/// Signature: void in_Rsockopen(int *port)
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub(crate) unsafe fn in_Rsockopen(port: *mut c_int) {
    unsafe {
        if port.is_null() {
            return;
        }
        check_init();
        let mut perr = super::sock::Sock_error_t::default();
        let fd = Sock_open(*port as Sock_port_t, 1 /* blocking */, &mut perr);
        *port = enter_sock(fd);
        if perr.error != 0 {
            let errstr = R_socket_strerror(perr.error);
            // Print error message via eprintln (REprintf has no varargs in our port)
            if !errstr.is_null() {
                let cstr = core::ffi::CStr::from_ptr(errstr);
                if let Ok(s) = cstr.to_str() {
                    eprintln!("socket error: {}", s);
                }
            }
        }
    }
}

/// in_Rsocklisten - listen on a socket (R .C interface)
/// Signature: void in_Rsocklisten(int *sockp, char **buf, int *len)
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub(crate) unsafe fn in_Rsocklisten(sockp: *mut c_int, buf: *mut *mut c_char, len: *mut c_int) {
    unsafe {
        if sockp.is_null() || buf.is_null() || len.is_null() {
            return;
        }
        check_init();
        let mut perr = super::sock::Sock_error_t::default();
        let fd = Sock_listen(*sockp, *buf, *len, &mut perr);
        *sockp = enter_sock(fd);
        if perr.error != 0 {
            let errstr = R_socket_strerror(perr.error);
            if !errstr.is_null() {
                let cstr = core::ffi::CStr::from_ptr(errstr);
                if let Ok(s) = cstr.to_str() {
                    eprintln!("socket error: {}", s);
                }
            }
        }
    }
}

/// in_Rsockconnect - connect to a socket (R .C interface)
/// Signature: void in_Rsockconnect(int *port, char **host)
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub(crate) unsafe fn in_Rsockconnect(port: *mut c_int, host: *mut *mut c_char) {
    unsafe {
        if port.is_null() || host.is_null() {
            return;
        }
        check_init();
        let mut perr = super::sock::Sock_error_t::default();
        let fd = Sock_connect(*port as Sock_port_t, *host, &mut perr);
        *port = enter_sock(fd);
        if perr.error != 0 {
            let errstr = R_socket_strerror(perr.error);
            if !errstr.is_null() {
                let cstr = core::ffi::CStr::from_ptr(errstr);
                if let Ok(s) = cstr.to_str() {
                    eprintln!("socket error: {}", s);
                }
            }
        }
    }
}

/// in_Rsockclose - close a socket (R .C interface)
/// Signature: void in_Rsockclose(int *sockp)
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub(crate) unsafe fn in_Rsockclose(sockp: *mut c_int) {
    unsafe {
        if sockp.is_null() {
            return;
        }
        *sockp = close_sock(*sockp);
    }
}

/// in_Rsockread - read from a socket (R .C interface)
/// Allocates a buffer via R_alloc, copies read data into it, writes pointer to *buf.
/// Signature: void in_Rsockread(int *sockp, char **buf, int *maxlen)
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub(crate) unsafe fn in_Rsockread(sockp: *mut c_int, buf: *mut *mut c_char, maxlen: *mut c_int) {
    unsafe {
        if sockp.is_null() || buf.is_null() || maxlen.is_null() {
            return;
        }
        check_init();
        let mut perr = super::sock::Sock_error_t::default();
        let sz = *maxlen as size_t;

        // Allocate buffer via R_alloc (1-byte elements, sz count)
        let ptr = R_alloc(1, sz) as *mut c_char;
        if ptr.is_null() {
            *maxlen = -1;
            return;
        }

        let nread = Sock_read(*sockp, ptr as *mut c_void, sz, &mut perr);
        *maxlen = nread as c_int;
        *buf = ptr;

        if perr.error != 0 {
            let errstr = R_socket_strerror(perr.error);
            if !errstr.is_null() {
                let cstr = core::ffi::CStr::from_ptr(errstr);
                if let Ok(s) = cstr.to_str() {
                    eprintln!("socket error: {}", s);
                }
            }
        }
    }
}

/// in_Rsockwrite - write to a socket (R .C interface)
/// Signature: void in_Rsockwrite(int *sockp, char **buf, int *start, int *end, int *len)
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub(crate) unsafe fn in_Rsockwrite(
    sockp: *mut c_int,
    buf: *mut *mut c_char,
    start: *mut c_int,
    end: *mut c_int,
    len: *mut c_int,
) {
    unsafe {
        if sockp.is_null() || buf.is_null() || start.is_null() || end.is_null() || len.is_null() {
            return;
        }

        // Clamp end and start
        if *end > *len {
            *end = *len;
        }
        if *start < 0 {
            *start = 0;
        }
        if *end < *start {
            *len = -1;
            return;
        }

        check_init();
        let mut perr = super::sock::Sock_error_t::default();
        // Write from buf + start, count = end - start
        let write_ptr = (*buf).add(*start as usize) as *const c_void;
        let write_len = (*end - *start) as size_t;
        let n = Sock_write(*sockp, write_ptr, write_len, &mut perr);
        *len = n as c_int;

        if perr.error != 0 {
            let errstr = R_socket_strerror(perr.error);
            if !errstr.is_null() {
                let cstr = core::ffi::CStr::from_ptr(errstr);
                if let Ok(s) = cstr.to_str() {
                    eprintln!("socket error: {}", s);
                }
            }
        }
    }
}
