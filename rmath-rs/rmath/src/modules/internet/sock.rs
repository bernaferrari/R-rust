// Port of R's modules/internet/sock.c (446 lines)
// Low-level socket operations: Sock_init/open/listen/connect/close/read/write
// and platform wrappers: R_close_socket, R_invalid_socket, R_socket_error, etc.
// Unix implementation using libc system calls.

use core::ffi::{c_char, c_int, c_void};
use libc::{size_t, socklen_t, ssize_t};

use libc::{
    // errno access
    __error,
    // socket constants
    AF_INET,
    // error
    EINTR,
    F_GETFD,
    F_GETFL,
    F_SETFD,
    F_SETFL,
    FD_CLOEXEC,
    IPPROTO_TCP,
    O_NONBLOCK,
    SIG_DFL,
    SIG_IGN,
    // signal
    SIGPIPE,
    SO_REUSEADDR,
    SOCK_STREAM,
    SOL_SOCKET,
    SOMAXCONN,
    TCP_NODELAY,
    accept,
    bind,
    close,
    connect,
    fcntl,
    // host resolution (type only; functions declared via extern "C" below)
    hostent,
    htons,
    in_addr,
    listen,
    recv,
    send,
    setsockopt,
    sigaction,
    sockaddr,
    // network types
    sockaddr_in,
    // socket functions
    socket,
};

use crate::sexp::*;

// Socket port type (matches C: typedef unsigned short Sock_port_t)
type Sock_port_t = u16;

// Sock_error_t structure (matches C: struct Sock_error_t)
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub(crate) struct Sock_error_t {
    pub error: c_int,
    pub h_error: c_int,
}

// hostent is provided by libc; gethostbyname/gethostbyaddr are deprecated in
// POSIX but still available as system library functions. Declare them here.
unsafe extern "C" {
    fn gethostbyname(name: *const c_char) -> *mut hostent;
    fn gethostbyaddr(addr: *const c_char, len: c_int, type_: c_int) -> *mut hostent;
}

// --- Internal helper: get errno (platform-specific) ---

#[inline]
unsafe fn get_errno() -> c_int {
    *__error()
}

// --- Internal helper: get h_errno ---
// On macOS, h_errno is a thread-local accessed via __h_errno().
// On other Unix, it may be a global or macro. We use an extern for portability.

#[cfg(target_os = "macos")]
unsafe fn get_h_errno() -> c_int {
    unsafe extern "C" {
        fn __h_errno() -> *mut c_int;
    }
    *__h_errno()
}

#[cfg(not(target_os = "macos"))]
unsafe fn get_h_errno() -> c_int {
    unsafe extern "C" {
        static h_errno: c_int;
    }
    h_errno
}

// --- Platform socket wrapper implementations (Unix path) ---

/// R_close_socket - close a socket descriptor
/// Signature: int R_close_socket(SOCKET s)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn R_close_socket(s: c_int) -> c_int {
    unsafe { close(s) }
}

/// R_socket_errno - get last socket error number
/// Signature: int R_socket_errno(void)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn R_socket_errno() -> c_int {
    unsafe { get_errno() }
}

/// R_invalid_socket - check if a socket descriptor is invalid
/// Signature: int R_invalid_socket(SOCKET s)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn R_invalid_socket(s: c_int) -> c_int {
    if s < 0 { 1 } else { 0 }
}

/// R_socket_error - check if a socket call returned an error
/// Signature: int R_socket_error(int s)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn R_socket_error(s: c_int) -> c_int {
    if s < 0 { 1 } else { 0 }
}

/// R_invalid_socket_eintr - check if socket is invalid due to EINTR
/// Signature: int R_invalid_socket_eintr(SOCKET s)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn R_invalid_socket_eintr(s: c_int) -> c_int {
    if s == -1 && get_errno() == EINTR {
        1
    } else {
        0
    }
}

/// R_socket_error_eintr - check if socket error is EINTR
/// Signature: int R_socket_error_eintr(int s)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn R_socket_error_eintr(s: c_int) -> c_int {
    if s == -1 && get_errno() == EINTR {
        1
    } else {
        0
    }
}

/// R_socket_strerror - convert socket error number to string
/// Signature: char *R_socket_strerror(int errnum)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn R_socket_strerror(errnum: c_int) -> *mut c_char {
    unsafe { libc::strerror(errnum) as *mut c_char }
}

/// R_set_nonblocking - set a socket to non-blocking mode
/// Signature: int R_set_nonblocking(SOCKET s)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn R_set_nonblocking(s: c_int) -> c_int {
    unsafe {
        let mut status = fcntl(s, F_GETFL, 0);
        if status == -1 {
            return -1;
        }
        status |= O_NONBLOCK;
        status = fcntl(s, F_SETFL, status);
        if status < 0 {
            R_close_socket(s);
            return -1;
        }
        0
    }
}

/// R_set_nodelay - set TCP_NODELAY on a socket
/// Signature: int R_set_nodelay(SOCKET s)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn R_set_nodelay(s: c_int) -> c_int {
    unsafe {
        let val: c_int = 1;
        setsockopt(
            s,
            IPPROTO_TCP,
            TCP_NODELAY,
            &val as *const c_int as *const c_void,
            core::mem::size_of::<c_int>() as socklen_t,
        )
    }
}

// --- Internal helper (module-private, no #[no_mangle]) ---

/// Sock_error - set error fields in a Sock_error_t and return -1
unsafe fn Sock_error(perr: *mut Sock_error_t, e: c_int, he: c_int) -> c_int {
    if !perr.is_null() {
        (*perr).error = e;
        (*perr).h_error = he;
    }
    -1
}

// --- Core socket operation implementations ---

/// Sock_init - initialize socket services
/// On Unix: ignore SIGPIPE so that writes to broken sockets return errors
/// instead of terminating the process.
/// Signature: int Sock_init(void)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn Sock_init() -> c_int {
    unsafe {
        let mut act: libc::sigaction = core::mem::zeroed();
        if sigaction(SIGPIPE, core::ptr::null_mut(), &mut act) < 0 {
            return 1;
        }
        if act.sa_sigaction == SIG_DFL {
            act.sa_sigaction = SIG_IGN;
            if sigaction(SIGPIPE, &act, core::ptr::null_mut()) < 0 {
                return 1;
            }
        }
    }
    0
}

/// Sock_open - open a socket for listening (socket + bind + listen)
/// Signature: int Sock_open(Sock_port_t port, int blocking, Sock_error_t perr)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn Sock_open(
    port: Sock_port_t,
    blocking: c_int,
    perr: *mut Sock_error_t,
) -> c_int {
    unsafe {
        let sock = socket(AF_INET, SOCK_STREAM, 0);
        if R_invalid_socket(sock) != 0 {
            return Sock_error(perr, get_errno(), 0);
        }

        if blocking == 0 && R_set_nonblocking(sock) != 0 {
            R_close_socket(sock);
            return Sock_error(perr, get_errno(), 0);
        }

        let mut server: sockaddr_in = core::mem::zeroed();
        server.sin_family = AF_INET as u8;
        server.sin_addr.s_addr = libc::INADDR_ANY;
        server.sin_port = htons(port as u16);

        // Set SO_REUSEADDR to allow re-binding to a port with lingering connections
        let reuse: c_int = 1;
        setsockopt(
            sock,
            SOL_SOCKET,
            SO_REUSEADDR,
            &reuse as *const c_int as *const c_void,
            core::mem::size_of::<c_int>() as socklen_t,
        );

        // Set FD_CLOEXEC so child processes do not inherit the listening socket
        let mut status = fcntl(sock, F_GETFD, 0);
        if status != -1 {
            status |= FD_CLOEXEC;
            status = fcntl(sock, F_SETFD, status);
        }
        if status == -1 {
            close(sock);
            return Sock_error(perr, get_errno(), 0);
        }

        // Bind to the port
        status = bind(
            sock,
            &server as *const sockaddr_in as *const sockaddr,
            core::mem::size_of::<sockaddr_in>() as socklen_t,
        );
        if R_socket_error(status) != 0 {
            R_close_socket(sock);
            return Sock_error(perr, get_errno(), 0);
        }

        // Listen for connections
        status = listen(sock, SOMAXCONN);
        if R_socket_error(status) != 0 {
            R_close_socket(sock);
            return Sock_error(perr, get_errno(), 0);
        }

        sock
    }
}

/// Sock_listen - accept a connection on a listening socket
/// Returns the file descriptor of the accepted connection, or -1 on error.
/// If cname is non-null and buflen > 0, writes the hostname of the connecting
/// peer into cname (truncated to buflen-1 chars).
/// Signature: int Sock_listen(int fd, char *cname, int buflen, Sock_error_t perr)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn Sock_listen(
    fd: c_int,
    cname: *mut c_char,
    buflen: c_int,
    perr: *mut Sock_error_t,
) -> c_int {
    unsafe {
        let mut net_client: sockaddr_in = core::mem::zeroed();
        let mut len: socklen_t = core::mem::size_of::<sockaddr>() as socklen_t;

        let mut retval: c_int;
        loop {
            retval = accept(
                fd,
                &mut net_client as *mut sockaddr_in as *mut sockaddr,
                &mut len,
            );
            if R_invalid_socket_eintr(retval) == 0 {
                break;
            }
        }
        if R_invalid_socket(retval) != 0 {
            return Sock_error(perr, get_errno(), 0);
        }

        if !cname.is_null() && buflen > 0 {
            let name: *const c_char;
            let hostptr = gethostbyaddr(
                &mut net_client.sin_addr as *mut in_addr as *mut c_char,
                core::mem::size_of::<in_addr>() as i32,
                AF_INET,
            );
            if hostptr.is_null() {
                name = b"unknown\0".as_ptr() as *const c_char;
            } else {
                name = (*hostptr).h_name;
            }

            let mut nlen = libc::strlen(name);
            let max_len = (buflen - 1) as usize;
            if nlen > max_len {
                nlen = max_len;
            }
            libc::strncpy(cname, name, nlen);
            *cname.add(nlen) = 0;
        }

        retval
    }
}

/// Sock_connect - open and connect to a socket on a remote host
/// Signature: int Sock_connect(Sock_port_t port, char *sname, Sock_error_t perr)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn Sock_connect(
    port: Sock_port_t,
    sname: *mut c_char,
    perr: *mut Sock_error_t,
) -> c_int {
    unsafe {
        // R_gethostbyname is defined in rsock.rs; fall back to libc gethostbyname
        // if the R wrapper is not available. We call gethostbyname directly here
        // since R_gethostbyname is just a wrapper with a localhost fallback.
        let hp = gethostbyname(sname);
        if hp.is_null() {
            return Sock_error(perr, get_errno(), get_h_errno());
        }

        let sock = socket(AF_INET, SOCK_STREAM, 0);
        if R_invalid_socket(sock) != 0 {
            return Sock_error(perr, get_errno(), 0);
        }

        let mut server: sockaddr_in = core::mem::zeroed();
        // Copy first address from h_addr_list[0] into sin_addr
        let first_addr = *(*hp).h_addr_list.add(0);
        core::ptr::copy_nonoverlapping(
            first_addr,
            &mut server.sin_addr as *mut in_addr as *mut c_char,
            (*hp).h_length as usize,
        );
        server.sin_port = htons(port as u16);
        server.sin_family = AF_INET as u8;

        let mut retval: c_int;
        loop {
            retval = connect(
                sock,
                &server as *const sockaddr_in as *const sockaddr,
                core::mem::size_of::<sockaddr_in>() as socklen_t,
            );
            if R_socket_error_eintr(retval) == 0 {
                break;
            }
        }
        if R_socket_error(retval) != 0 {
            R_close_socket(sock);
            return Sock_error(perr, get_errno(), 0);
        }

        sock
    }
}

/// Sock_close - close a socket
/// Signature: int Sock_close(int fd, Sock_error_t perr)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn Sock_close(fd: c_int, perr: *mut Sock_error_t) -> c_int {
    unsafe {
        if close(fd) < 0 {
            Sock_error(perr, get_errno(), 0)
        } else {
            0
        }
    }
}

/// Sock_read - read from a socket (using recv, with EINTR retry)
/// Signature: ssize_t Sock_read(int fd, void *buf, size_t size, Sock_error_t perr)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn Sock_read(
    fd: c_int,
    buf: *mut c_void,
    sz: size_t,
    perr: *mut Sock_error_t,
) -> ssize_t {
    unsafe {
        let mut retval: ssize_t;
        loop {
            retval = recv(fd, buf, sz, 0);
            if R_socket_error_eintr(retval as c_int) == 0 {
                break;
            }
        }
        if R_socket_error(retval as c_int) != 0 {
            Sock_error(perr, get_errno(), 0) as ssize_t
        } else {
            retval
        }
    }
}

/// Sock_write - write to a socket (using send, with EINTR retry)
/// Signature: ssize_t Sock_write(int fd, const void *buf, size_t size, Sock_error_t perr)
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn Sock_write(
    fd: c_int,
    buf: *const c_void,
    sz: size_t,
    perr: *mut Sock_error_t,
) -> ssize_t {
    unsafe {
        let mut retval: ssize_t;
        loop {
            retval = send(fd, buf, sz, 0);
            if R_socket_error_eintr(retval as c_int) == 0 {
                break;
            }
        }
        if R_socket_error(retval as c_int) != 0 {
            Sock_error(perr, get_errno(), 0) as ssize_t
        } else {
            retval
        }
    }
}
