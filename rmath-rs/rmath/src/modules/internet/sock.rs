// Port of R's modules/internet/sock.c (446 lines)
// Low-level socket operations: Sock_init/open/listen/connect/close/read/write
// and platform wrappers: R_close_socket, R_invalid_socket, R_socket_error, etc.
// Unix implementation using libc system calls.

use core::ffi::{c_char, c_int, c_void};
use libc::{size_t, socklen_t, ssize_t};

use libc::{
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
    addrinfo,
    bind,
    close,
    connect,
    fcntl,
    freeaddrinfo,
    getaddrinfo,
    getnameinfo,
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

// --- Internal helper: get errno (platform-specific) ---

#[inline]
unsafe fn get_errno() -> c_int {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn __error() -> *mut c_int;
        }
        *__error()
    }
    #[cfg(not(target_os = "macos"))]
    {
        unsafe extern "C" {
            fn __errno_location() -> *mut c_int;
        }
        *__errno_location()
    }
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
pub(crate) unsafe fn R_close_socket(s: c_int) -> c_int {
    unsafe { close(s) }
}

/// R_socket_errno - get last socket error number
/// Signature: int R_socket_errno(void)
pub(crate) unsafe fn R_socket_errno() -> c_int {
    unsafe { get_errno() }
}

/// R_invalid_socket - check if a socket descriptor is invalid
/// Signature: int R_invalid_socket(SOCKET s)
pub(crate) unsafe fn R_invalid_socket(s: c_int) -> c_int {
    if s < 0 { 1 } else { 0 }
}

/// R_socket_error - check if a socket call returned an error
/// Signature: int R_socket_error(int s)
pub(crate) unsafe fn R_socket_error(s: c_int) -> c_int {
    if s < 0 { 1 } else { 0 }
}

/// R_invalid_socket_eintr - check if socket is invalid due to EINTR
/// Signature: int R_invalid_socket_eintr(SOCKET s)
pub(crate) unsafe fn R_invalid_socket_eintr(s: c_int) -> c_int {
    if s == -1 && get_errno() == EINTR {
        1
    } else {
        0
    }
}

/// R_socket_error_eintr - check if socket error is EINTR
/// Signature: int R_socket_error_eintr(int s)
pub(crate) unsafe fn R_socket_error_eintr(s: c_int) -> c_int {
    if s == -1 && get_errno() == EINTR {
        1
    } else {
        0
    }
}

/// R_socket_strerror - convert socket error number to string
/// Signature: char *R_socket_strerror(int errnum)
pub(crate) unsafe fn R_socket_strerror(errnum: c_int) -> *mut c_char {
    unsafe { libc::strerror(errnum) as *mut c_char }
}

/// R_set_nonblocking - set a socket to non-blocking mode
/// Signature: int R_set_nonblocking(SOCKET s)
pub(crate) unsafe fn R_set_nonblocking(s: c_int) -> c_int {
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
pub(crate) unsafe fn R_set_nodelay(s: c_int) -> c_int {
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
pub(crate) unsafe fn Sock_init() -> c_int {
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
pub(crate) unsafe fn Sock_open(
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
pub(crate) unsafe fn Sock_listen(
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
            let mut name_buf: Vec<c_char> = vec![0; buflen as usize];
            let ret = getnameinfo(
                &net_client as *const sockaddr_in as *const sockaddr,
                core::mem::size_of::<sockaddr_in>() as socklen_t,
                name_buf.as_mut_ptr(),
                buflen as libc::socklen_t,
                core::ptr::null_mut(),
                0,
                0,
            );
            if ret != 0 {
                // Fallback to "unknown" on failure
                let unknown = b"unknown\0";
                let copy_len = std::cmp::min(unknown.len() - 1, (buflen - 1) as usize);
                core::ptr::copy_nonoverlapping(unknown.as_ptr() as *const c_char, cname, copy_len);
                *cname.add(copy_len) = 0;
            } else {
                // getnameinfo null-terminates; copy up to buflen-1
                let nlen = libc::strlen(name_buf.as_ptr() as *const c_char);
                let max_len = std::cmp::min(nlen, (buflen - 1) as usize);
                core::ptr::copy_nonoverlapping(name_buf.as_ptr(), cname, max_len);
                *cname.add(max_len) = 0;
            }
        }

        retval
    }
}

/// Sock_connect - open and connect to a socket on a remote host
/// Signature: int Sock_connect(Sock_port_t port, char *sname, Sock_error_t perr)
pub(crate) unsafe fn Sock_connect(
    port: Sock_port_t,
    sname: *mut c_char,
    perr: *mut Sock_error_t,
) -> c_int {
    unsafe {
        // Use getaddrinfo (thread-safe, Android-friendly) instead of deprecated gethostbyname
        let mut hints: addrinfo = core::mem::zeroed();
        hints.ai_family = AF_INET;
        hints.ai_socktype = SOCK_STREAM;
        let mut res: *mut addrinfo = core::ptr::null_mut();
        let gai_err = getaddrinfo(sname, core::ptr::null(), &hints, &mut res);
        if gai_err != 0 {
            return Sock_error(perr, get_errno(), gai_err);
        }
        if res.is_null() {
            freeaddrinfo(res);
            return Sock_error(perr, get_errno(), 0);
        }

        // Find first IPv4 result
        let mut ai = res;
        let mut found = false;
        while !ai.is_null() {
            if (*ai).ai_family == AF_INET {
                found = true;
                break;
            }
            ai = (*ai).ai_next;
        }
        if !found {
            freeaddrinfo(res);
            return Sock_error(perr, get_errno(), 0);
        }

        let sock = socket(AF_INET, SOCK_STREAM, 0);
        if R_invalid_socket(sock) != 0 {
            freeaddrinfo(res);
            return Sock_error(perr, get_errno(), 0);
        }

        let mut server: sockaddr_in = core::mem::zeroed();
        core::ptr::copy_nonoverlapping((*ai).ai_addr as *const sockaddr_in, &mut server, 1);
        server.sin_port = htons(port as u16);
        server.sin_family = AF_INET as u8;
        freeaddrinfo(res);

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
pub(crate) unsafe fn Sock_close(fd: c_int, perr: *mut Sock_error_t) -> c_int {
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
pub(crate) unsafe fn Sock_read(
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
pub(crate) unsafe fn Sock_write(
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
