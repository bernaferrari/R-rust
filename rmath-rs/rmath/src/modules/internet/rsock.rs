// Port of R's modules/internet/Rsock.c (705 lines)
// R socket interface: Sock_open/listen/connect/close/read/write for R level
// and R_SockOpen/Listen/Connect/Close/Read/Write for connection-level use.
// Unix implementation using libc system calls and lower-level sock.rs functions.

use crate::sexp::*;
use core::cell::Cell;
use core::ffi::{c_char, c_double, c_int, c_void};
use libc::{
    AF_INET,
    EAGAIN,
    ECONNABORTED,
    EINPROGRESS,
    // error codes
    EINTR,
    EINVAL,
    EPROTO,
    EWOULDBLOCK,
    FD_ISSET,
    FD_SET,
    FD_SETSIZE,
    FD_ZERO,
    IPPROTO_TCP,
    PF_INET,
    SO_ERROR,
    SOCK_STREAM,
    SOL_SOCKET,
    connect,
    // fd_set and select
    fd_set,
    getsockopt,
    // hostent is re-exported from libc
    hostent,
    htons,
    recv,
    select,
    send,
    size_t,
    sockaddr,
    // network types
    sockaddr_in,
    // socket/networking
    socket,
    ssize_t,
    // string comparison
    strcmp,
    timeval,
};

// Socket port type (matches C: typedef unsigned short Sock_port_t)
type Sock_port_t = u16;

// Declare functions from sibling module sock.rs via extern "C"
// These are #[unsafe(no_mangle)] pub(crate) functions in sock.rs
unsafe extern "C" {
    fn Sock_init() -> c_int;
    fn Sock_open(port: Sock_port_t, blocking: c_int, perr: *mut super::sock::Sock_error_t)
    -> c_int;
    fn Sock_listen(
        fd: c_int,
        cname: *mut c_char,
        buflen: c_int,
        perr: *mut super::sock::Sock_error_t,
    ) -> c_int;
    fn Sock_connect(
        port: Sock_port_t,
        sname: *mut c_char,
        perr: *mut super::sock::Sock_error_t,
    ) -> c_int;
    fn Sock_close(fd: c_int, perr: *mut super::sock::Sock_error_t) -> c_int;
    fn Sock_read(
        fd: c_int,
        buf: *mut c_void,
        sz: size_t,
        perr: *mut super::sock::Sock_error_t,
    ) -> ssize_t;
    fn Sock_write(
        fd: c_int,
        buf: *const c_void,
        sz: size_t,
        perr: *mut super::sock::Sock_error_t,
    ) -> ssize_t;
    fn R_close_socket(s: c_int) -> c_int;
    fn R_invalid_socket(s: c_int) -> c_int;
    fn R_socket_error(s: c_int) -> c_int;
    fn R_socket_error_eintr(s: c_int) -> c_int;
    fn R_socket_errno() -> c_int;
    fn R_socket_strerror(errnum: c_int) -> *mut c_char;
    fn R_set_nonblocking(s: c_int) -> c_int;
    fn REprintf(format: *const i8);
    // R_alloc for in_Rsockread buffer allocation
    fn R_alloc(size: usize, nelem: usize) -> *mut c_void;
    // libc gethostbyname (deprecated but available on all Unix)
    fn gethostbyname(name: *const c_char) -> *mut hostent;
}

thread_local! { static sock_inited: Cell<c_int> = Cell::new(0); }

thread_local! { static R_wait_usec_val: Cell<c_int> = Cell::new(0); }

// --- Internal helper functions (module-private, no #[no_mangle]) ---

/// enter_sock - validate socket fd
/// Returns 0 if fd == -1 (invalid), otherwise returns fd.
unsafe fn enter_sock(fd: c_int) -> c_int {
    if fd == -1 { 0 } else { fd }
}

/// close_sock - close a socket via Sock_close with error reporting
unsafe fn close_sock(fd: c_int) -> c_int {
    let mut perr = super::sock::Sock_error_t::default();
    let res = Sock_close(fd, &mut perr);
    if res == -1 {
        REprintf(b"socket error: %s\n\0".as_ptr() as *const i8);
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
    sock_inited.with(|v| {
        if v.get() == 0 {
            unsafe {
                Sock_init();
            }
            v.set(1);
        }
    });
}

/// set_timeval - populate a timeval struct for select(), respecting R_wait_usec
/// When R_wait_usec > 0, each select() polls for that interval; otherwise
/// the full timeout is used.
unsafe fn set_timeval(tv: *mut timeval, timeout: c_int) {
    let wait_usec = R_wait_usec_val.with(|v| v.get());
    if wait_usec > 0 {
        (*tv).tv_sec = (wait_usec / 1_000_000) as libc::time_t;
        (*tv).tv_usec = (wait_usec - (wait_usec / 1_000_000) * 1_000_000) as libc::suseconds_t;
    } else {
        (*tv).tv_sec = timeout as libc::time_t;
        (*tv).tv_usec = 0;
    }
}

/// R_SocketWait - wait for a single socket to become readable or writable
/// Uses select() with timeout. Returns 0 on success, 1 on timeout, negative on error.
/// This is the Unix path without InputHandler support (which requires the full R event loop).
unsafe fn R_SocketWait(sockfd: c_int, write: c_int, timeout: c_int) -> c_int {
    let mut rfd: fd_set = core::mem::zeroed();
    let mut wfd: fd_set = core::mem::zeroed();
    let mut tv: timeval;
    let mut used: c_double = 0.0;

    loop {
        let mut maxfd: c_int = 0;
        tv = core::mem::zeroed();
        set_timeval(&mut tv, timeout);

        FD_ZERO(&mut rfd);
        FD_ZERO(&mut wfd);
        if write != 0 {
            FD_SET(sockfd, &mut wfd);
        } else {
            FD_SET(sockfd, &mut rfd);
        }
        if maxfd < sockfd {
            maxfd = sockfd;
        }

        // Increment used value before select() in case select modifies tv (as Linux does)
        used += tv.tv_sec as c_double + 1e-6 * tv.tv_usec as c_double;

        let howmany = select(
            maxfd + 1,
            &mut rfd,
            &mut wfd,
            core::ptr::null_mut(),
            &mut tv,
        );

        if R_socket_error(howmany) != 0 {
            return -R_socket_errno();
        }
        if howmany == 0 {
            if used >= timeout as c_double {
                return 1;
            }
            continue;
        }

        // The socket was ready (no InputHandler extras in our simplified Unix path)
        break;
    }
    0
}

// --- Exported R interface functions (from sock.h) ---

/// in_Rsockopen - open a socket for listening (R .C interface)
/// Signature: void in_Rsockopen(int *port)
pub(crate) unsafe fn in_Rsockopen(port: *mut c_int) {
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
                eprint!("socket error: {}\n", s);
            }
        }
    }
}

/// in_Rsocklisten - listen on a socket (R .C interface)
/// Signature: void in_Rsocklisten(int *sockp, char **buf, int *len)
pub(crate) unsafe fn in_Rsocklisten(
    sockp: *mut c_int,
    buf: *mut *mut c_char,
    len: *mut c_int,
) {
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
                eprint!("socket error: {}\n", s);
            }
        }
    }
}

/// in_Rsockconnect - connect to a socket (R .C interface)
/// Signature: void in_Rsockconnect(int *port, char **host)
pub(crate) unsafe fn in_Rsockconnect(port: *mut c_int, host: *mut *mut c_char) {
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
                eprint!("socket error: {}\n", s);
            }
        }
    }
}

/// in_Rsockclose - close a socket (R .C interface)
/// Signature: void in_Rsockclose(int *sockp)
pub(crate) unsafe fn in_Rsockclose(sockp: *mut c_int) {
    if sockp.is_null() {
        return;
    }
    *sockp = close_sock(*sockp);
}

/// in_Rsockread - read from a socket (R .C interface)
/// Allocates a buffer via R_alloc, copies read data into it, writes pointer to *buf.
/// Signature: void in_Rsockread(int *sockp, char **buf, int *maxlen)
pub(crate) unsafe fn in_Rsockread(
    sockp: *mut c_int,
    buf: *mut *mut c_char,
    maxlen: *mut c_int,
) {
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
                eprint!("socket error: {}\n", s);
            }
        }
    }
}

/// in_Rsockwrite - write to a socket (R .C interface)
/// Signature: void in_Rsockwrite(int *sockp, char **buf, int *start, int *end, int *len)
pub(crate) unsafe fn in_Rsockwrite(
    sockp: *mut c_int,
    buf: *mut *mut c_char,
    start: *mut c_int,
    end: *mut c_int,
    len: *mut c_int,
) {
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
                eprint!("socket error: {}\n", s);
            }
        }
    }
}

/// in_Rsockselect - select on multiple sockets (R .C interface)
/// Signature: int in_Rsockselect(int nsock, int *insockfd, int *ready, int *write, double timeout)
pub(crate) unsafe fn in_Rsockselect(
    nsock: c_int,
    insockfd: *mut c_int,
    ready: *mut c_int,
    write: *mut c_int,
    timeout: c_double,
) -> c_int {
    R_SocketWaitMultiple(nsock, insockfd, ready, write, timeout)
}

// --- Exported connection-level functions (from sock.h, used by sockconn.c) ---

/// R_SocketWaitMultiple - wait for multiple sockets
/// Signature: int R_SocketWaitMultiple(int nsock, int *insockfd, int *ready, int *write, double mytimeout)
pub(crate) unsafe fn R_SocketWaitMultiple(
    nsock: c_int,
    insockfd: *mut c_int,
    ready: *mut c_int,
    write: *mut c_int,
    mytimeout: c_double,
) -> c_int {
    let mut rfd: fd_set = core::mem::zeroed();
    let mut wfd: fd_set = core::mem::zeroed();
    let mut tv: timeval;
    let mut used: c_double = 0.0;
    let mut nready: c_int = 0;

    loop {
        let mut maxfd: c_int = 0;

        // Compute timeout for this iteration
        let wait_usec = R_wait_usec_val.with(|v| v.get());
        tv = core::mem::zeroed();
        if wait_usec > 0 {
            let delta = if mytimeout < 0.0 || (wait_usec as c_double) / 1e6 < mytimeout - used {
                wait_usec
            } else {
                libm::ceil(1e6 * (mytimeout - used)) as c_int
            };
            tv.tv_sec = (delta / 1_000_000) as libc::time_t;
            tv.tv_usec = (delta - (delta / 1_000_000) * 1_000_000) as libc::suseconds_t;
        } else if mytimeout >= 0.0 {
            let remaining = mytimeout - used;
            tv.tv_sec = remaining as libc::time_t;
            tv.tv_usec = libm::ceil(1e6 * (remaining - remaining as c_double)) as libc::suseconds_t;
        } else {
            // Always poll occasionally when no timeout specified
            tv.tv_sec = 60;
            tv.tv_usec = 0;
        }

        FD_ZERO(&mut rfd);
        FD_ZERO(&mut wfd);

        let mut ii = 0;
        while ii < nsock {
            if !insockfd.is_null() {
                let fd = *insockfd.add(ii as usize);
                if !write.is_null() && *write.add(ii as usize) != 0 {
                    FD_SET(fd, &mut wfd);
                } else {
                    FD_SET(fd, &mut rfd);
                }
                if maxfd < fd {
                    maxfd = fd;
                }
            }
            ii += 1;
        }

        // Increment used value before select() in case select modifies tv (as Linux does)
        used += tv.tv_sec as c_double + 1e-6 * tv.tv_usec as c_double;

        let howmany = select(
            maxfd + 1,
            &mut rfd,
            &mut wfd,
            core::ptr::null_mut(),
            &mut tv,
        );

        if R_socket_error(howmany) != 0 {
            return -R_socket_errno();
        }
        if howmany == 0 {
            if mytimeout >= 0.0 && used >= mytimeout {
                // Timeout: mark all as not ready
                let mut j = 0;
                while j < nsock {
                    if !ready.is_null() {
                        *ready.add(j as usize) = 0;
                    }
                    j += 1;
                }
                return 0;
            }
            continue;
        }

        // Check which sockets are ready
        nready = 0;
        let mut j = 0;
        while j < nsock {
            if !insockfd.is_null() && !ready.is_null() {
                let fd = *insockfd.add(j as usize);
                let is_write = !write.is_null() && *write.add(j as usize) != 0;
                if (!is_write && FD_ISSET(fd, &rfd)) || (is_write && FD_ISSET(fd, &wfd)) {
                    *ready.add(j as usize) = 1;
                    nready += 1;
                } else {
                    *ready.add(j as usize) = 0;
                }
            }
            j += 1;
        }

        // Some sockets are ready (no InputHandler extras in our simplified Unix path)
        break;
    }
    nready
}

/// R_SockConnect - connect to a host:port with timeout (non-blocking connect + select)
/// Signature: int R_SockConnect(int port, char *host, int timeout)
pub(crate) unsafe fn R_SockConnect(
    port: c_int,
    host: *mut c_char,
    timeout: c_int,
) -> c_int {
    check_init();

    let s = socket(PF_INET, SOCK_STREAM, IPPROTO_TCP);
    if R_invalid_socket(s) != 0 {
        return -1;
    }

    // Macro CLOSE_N_RETURN equivalent: close socket and return status
    macro_rules! close_and_return {
        ($status:expr) => {
            R_close_socket(s);
            return $status;
        };
    }

    if R_set_nonblocking(s) != 0 {
        return -1;
    }

    // Use R_gethostbyname (which is defined in this file) via direct call
    let hp = R_gethostbyname(host);
    if hp.is_null() {
        close_and_return!(-1);
    }

    let mut server: sockaddr_in = core::mem::zeroed();
    // Copy first address from h_addr_list[0] into sin_addr
    let first_addr = *(*hp).h_addr_list.add(0);
    core::ptr::copy_nonoverlapping(
        first_addr,
        &mut server.sin_addr as *mut _ as *mut c_char,
        (*hp).h_length as usize,
    );
    server.sin_port = htons(port as u16);
    server.sin_family = AF_INET as u8;

    let conn_status = connect(
        s,
        &server as *const sockaddr_in as *const sockaddr,
        core::mem::size_of::<sockaddr_in>() as u32,
    );

    if R_socket_error(conn_status) != 0 {
        match R_socket_errno() {
            e if e == EINPROGRESS || e == EWOULDBLOCK || e == EAGAIN => {
                // Expected for non-blocking connect; fall through to select loop
            }
            _ => {
                close_and_return!(-1);
            }
        }
    } else {
        // Connected immediately
        return s;
    }

    // Wait for the connection to complete using select
    let mut used: c_double = 0.0;
    loop {
        let mut maxfd: c_int = 0;
        let mut tv = core::mem::zeroed::<timeval>();
        set_timeval(&mut tv, timeout);

        let mut rfd: fd_set = core::mem::zeroed();
        let mut wfd: fd_set = core::mem::zeroed();
        FD_ZERO(&mut rfd);
        FD_ZERO(&mut wfd);
        FD_SET(s, &mut wfd);
        if maxfd < s {
            maxfd = s;
        }

        // Increment used before select in case select modifies tv
        used += tv.tv_sec as c_double + 1e-6 * tv.tv_usec as c_double;

        let status = select(
            maxfd + 1,
            &mut rfd,
            &mut wfd,
            core::ptr::null_mut(),
            &mut tv,
        );

        if R_socket_error(status) != 0 {
            close_and_return!(-1);
        }

        if status == 0 {
            // Timeout
            if used < timeout as c_double {
                continue;
            }
            close_and_return!(-1);
        } else if FD_ISSET(s, &wfd) {
            // Socket is writable -- check for connection error via getsockopt
            let mut errval: c_int = 0;
            let mut len: u32 = core::mem::size_of::<c_int>() as u32;
            if getsockopt(
                s,
                SOL_SOCKET,
                SO_ERROR,
                &mut errval as *mut c_int as *mut c_void,
                &mut len as *mut u32 as *mut libc::socklen_t,
            ) < 0
            {
                close_and_return!(-1);
            }
            if errval != 0 {
                close_and_return!(-1);
            } else {
                return s;
            }
        } else {
            // Some other handler needed (simplified: no InputHandler support)
            continue;
        }
    }
}

/// R_SockClose - close a socket
/// Signature: int R_SockClose(int sockp)
pub(crate) unsafe fn R_SockClose(sockp: c_int) -> c_int {
    R_close_socket(sockp)
}

/// R_SockRead - read from a socket with optional blocking and timeout
/// Uses recv() directly (not Sock_read) for non-blocking socket + select loop.
/// Signature: ssize_t R_SockRead(int sockp, void *buf, size_t len, int blocking, int timeout)
pub(crate) unsafe fn R_SockRead(
    sockp: c_int,
    buf: *mut c_void,
    len: size_t,
    blocking: c_int,
    timeout: c_int,
) -> ssize_t {
    let mut res: ssize_t;

    // EINTR is propagated to the caller. When !blocking,
    // the caller expects also EAGAIN/EWOULDBLOCK.
    // sockp is always non-blocking to be robust against spurious readability.
    loop {
        if blocking != 0 {
            let wait_res = R_SocketWait(sockp, 0, timeout);
            if wait_res != 0 {
                return if wait_res < 0 { wait_res as ssize_t } else { 0 };
            }
        }
        res = recv(sockp, buf, len, 0);
        if R_socket_error(res as c_int) != 0 {
            match R_socket_errno() {
                e if e == EWOULDBLOCK || e == EAGAIN => {
                    if blocking != 0 {
                        // Spurious readability, can happen on Linux
                        continue;
                    }
                    // Fall through to return error
                    return -R_socket_errno() as ssize_t;
                }
                _ => {
                    return -R_socket_errno() as ssize_t;
                }
            }
        } else {
            return res;
        }
    }
}

/// R_SockOpen - open a server socket (socket + bind + listen, non-blocking)
/// Signature: int R_SockOpen(int port)
pub(crate) unsafe fn R_SockOpen(port: c_int) -> c_int {
    check_init();
    Sock_open(
        port as Sock_port_t,
        0, /* non-blocking */
        core::ptr::null_mut(),
    )
}

/// R_SockListen - listen on a server socket with timeout (accept via select)
/// Signature: int R_SockListen(int sockp, char *buf, int len, int timeout)
pub(crate) unsafe fn R_SockListen(
    sockp: c_int,
    buf: *mut c_char,
    len: c_int,
    timeout: c_int,
) -> c_int {
    check_init();

    // The listening socket was opened in non-blocking mode via R_SockOpen.
    // We use select() before accept() to avoid race conditions.
    let mut rfd: fd_set = core::mem::zeroed();
    let mut tv: timeval;
    let mut used: c_double = 0.0;
    let mut maxfd: c_int = 0;

    loop {
        tv = core::mem::zeroed();
        set_timeval(&mut tv, timeout);

        FD_ZERO(&mut rfd);
        FD_SET(sockp, &mut rfd);
        if maxfd < sockp {
            maxfd = sockp;
        }

        // Compute maybe_used before select (select may modify tv on Linux)
        let maybe_used = used + tv.tv_sec as c_double + 1e-6 * tv.tv_usec as c_double;

        let status = select(
            maxfd + 1,
            &mut rfd,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &mut tv,
        );

        if R_socket_error_eintr(status) != 0 {
            // Do not advance used on EINTR
            continue;
        }
        if R_socket_error(status) != 0 {
            return -1;
        }

        used = maybe_used;
        if status == 0 {
            // Time out
            if used < timeout as c_double {
                continue;
            }
            return -1;
        } else if FD_ISSET(sockp, &rfd) {
            // Socket is readable -- try to accept
            let mut perr = super::sock::Sock_error_t::default();
            let s = Sock_listen(sockp, buf, len, &mut perr);
            if s == -1 {
                match perr.error {
                    e if e == EINPROGRESS
                        || e == EWOULDBLOCK
                        || e == ECONNABORTED
                        || e == EAGAIN
                        || e == EPROTO =>
                    {
                        continue;
                    }
                    _ => {
                        return -1;
                    }
                }
            }
            // Got a connection -- set it to non-blocking
            if R_set_nonblocking(s) != 0 {
                return -1;
            }
            return s;
        } else {
            // Was one of the extras (simplified: no InputHandler support)
            continue;
        }
    }
}

/// R_SockWrite - write to a socket with timeout (blocking)
/// Loops until all data is written or an error/timeout occurs.
/// Signature: ssize_t R_SockWrite(int sockp, const void *buf, size_t len, int timeout)
pub(crate) unsafe fn R_SockWrite(
    sockp: c_int,
    mut buf: *const c_void,
    mut len: size_t,
    timeout: c_int,
) -> ssize_t {
    let mut res: ssize_t;
    let mut out: ssize_t = 0;

    // This function is always blocking (no blocking flag parameter).
    // It loops until all data is written.
    loop {
        let wait_res = R_SocketWait(sockp, 1, timeout);
        if wait_res != 0 {
            return if wait_res < 0 { wait_res as ssize_t } else { 0 };
        }
        res = send(sockp, buf, len, 0);
        if R_socket_error(res as c_int) != 0 {
            match R_socket_errno() {
                e if e == EWOULDBLOCK || e == EAGAIN => {
                    // Spurious writability, should not happen. Retry.
                    continue;
                }
                _ => {
                    return -R_socket_errno() as ssize_t;
                }
            }
        } else {
            buf = buf.add(res as usize);
            len -= res as size_t;
            out += res;
        }
        if len == 0 {
            break;
        }
    }
    out
}

/// R_gethostbyname - get host entry by name (with localhost fallback)
/// Falls back to "127.0.0.1" if "localhost" lookup fails.
/// Signature: struct hostent *R_gethostbyname(const char *name)
pub(crate) unsafe fn R_gethostbyname(name: *const c_char) -> *mut hostent {
    // Call libc's gethostbyname (declared via extern "C" above)
    let ans = gethostbyname(name);

    // Hard-code IPv4 address for localhost to be robust against misconfigured systems
    if ans.is_null()
        && !name.is_null()
        && strcmp(name, b"localhost\0".as_ptr() as *const c_char) == 0
    {
        return gethostbyname(b"127.0.0.1\0".as_ptr() as *const c_char);
    }
    ans
}
