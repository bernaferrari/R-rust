// Port of R's modules/internet/sockconn.c (309 lines)
// Socket connections: Rconnection creation for socket and server socket connections.
// Implements sock_open, sock_close, servsock_close, sock_read_helper,
// sock_fgetc_internal, sock_read, sock_write, listencleanup, in_R_newsock, in_R_newservsock.
//
// Since the full Rconnection struct is not ported, we replicate the C layout
// here to access the fields we need. The private data (sockconn/servsockconn
// structs) is stored in con->private.

use crate::sexp::*;
use core::alloc::{Layout, alloc, dealloc};
use core::ffi::{c_char, c_int, c_void};
use libc::{FD_SETSIZE, size_t, snprintf, ssize_t, strcpy, strlen};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Socket connection options (matches C: RSC_SET_TCP_NODELAY)
const RSC_SET_TCP_NODELAY: c_int = 1;

/// R_EOF value (matches C: #define R_EOF -1)
const R_EOF: c_int = -1;

/// NA_INTEGER sentinel
const NA_INTEGER: c_int = std::i32::MIN; // R's NA_INTEGER = INT_MIN

/// CE_NATIVE encoding constant
const CE_NATIVE: c_int = 0;

/// TRUE/FALSE as c_int
const R_TRUE: c_int = 1;
const R_FALSE: c_int = 0;

// ---------------------------------------------------------------------------
// Rconn struct layout (matches C struct Rconn from R_ext/Connections.h)
// ---------------------------------------------------------------------------

/// Replicates the C `Rconn` struct layout from R_ext/Connections.h.
/// We only define the fields that sockconn.c accesses.
/// Rconnection = *mut Rconn.
#[repr(C)]
pub struct Rconn {
    pub class: *mut c_char,
    pub description: *mut c_char,
    pub enc: c_int,
    pub mode: [c_char; 5],
    pub text: c_int,       // Rboolean
    pub isopen: c_int,     // Rboolean
    pub incomplete: c_int, // Rboolean
    pub canread: c_int,    // Rboolean
    pub canwrite: c_int,   // Rboolean
    pub canseek: c_int,    // Rboolean
    pub blocking: c_int,   // Rboolean
    pub isGzcon: c_int,    // Rboolean
    // Function pointers follow; we use raw function pointer types
    pub open: Option<unsafe extern "C" fn(*mut Rconn) -> c_int>,
    pub close: Option<unsafe extern "C" fn(*mut Rconn)>,
    pub destroy: Option<unsafe extern "C" fn(*mut Rconn)>,
    pub vfprintf: Option<unsafe extern "C" fn(*mut Rconn, *const c_char, *mut c_void) -> c_int>,
    pub fgetc: Option<unsafe extern "C" fn(*mut Rconn) -> c_int>,
    pub fgetc_internal: Option<unsafe extern "C" fn(*mut Rconn) -> c_int>,
    pub seek: Option<unsafe extern "C" fn(*mut Rconn, f64, c_int, c_int) -> f64>,
    pub truncate_fn: Option<unsafe extern "C" fn(*mut Rconn)>,
    pub fflush_fn: Option<unsafe extern "C" fn(*mut Rconn) -> c_int>,
    pub read_fn: Option<unsafe extern "C" fn(*mut c_void, size_t, size_t, *mut Rconn) -> size_t>,
    pub write_fn: Option<unsafe extern "C" fn(*const c_void, size_t, size_t, *mut Rconn) -> size_t>,
    pub nPushBack: c_int,
    pub posPushBack: c_int,
    pub PushBack: *mut *mut c_char,
    pub save: c_int,
    pub save2: c_int,
    pub encname: [c_char; 101],
    pub inconv: *mut c_void,
    pub outconv: *mut c_void,
    pub iconvbuff: [c_char; 25],
    pub oconvbuff: [c_char; 50],
    pub next: *mut c_char,
    pub init_out: [c_char; 25],
    pub navail: i16,
    pub inavail: i16,
    pub EOF_signalled: c_int, // Rboolean
    pub UTF8out: c_int,       // Rboolean
    pub id: *mut c_void,
    pub ex_ptr: *mut c_void,
    pub private: *mut c_void,
    pub status: c_int,
    pub buff: *mut u8,
    pub buff_len: size_t,
    pub buff_stored_len: size_t,
    pub buff_pos: size_t,
}

/// Opaque Rconnection type
type Rconnection = *mut Rconn;

// ---------------------------------------------------------------------------
// sockconn private data struct (matches C struct sockconn from Rconnections.h)
// ---------------------------------------------------------------------------

/// Private data for a socket connection.
/// Matches C: struct sockconn { int port; int server; int fd; int timeout;
/// char *host; char inbuf[4096], *pstart, *pend; int serverfd; int options; };
#[repr(C)]
pub struct sockconn {
    pub port: c_int,
    pub server: c_int,
    pub fd: c_int,
    pub timeout: c_int,
    pub host: *mut c_char,
    pub inbuf: [c_char; 4096],
    pub pstart: *mut c_char,
    pub pend: *mut c_char,
    pub serverfd: c_int,
    pub options: c_int,
}

/// Pointer to sockconn private data
type Rsockconn = *mut sockconn;

// ---------------------------------------------------------------------------
// servsockconn private data struct (matches C struct servsockconn)
// ---------------------------------------------------------------------------

/// Private data for a server socket connection.
/// Matches C: struct servsockconn { int fd; int port; };
#[repr(C)]
pub struct servsockconn {
    pub fd: c_int,
    pub port: c_int,
}

/// Pointer to servsockconn private data
type Rservsockconn = *mut servsockconn;

// ---------------------------------------------------------------------------
// External function declarations (from rsock.rs and sock.rs)
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn R_SockOpen(port: c_int) -> c_int;
    fn R_SockListen(sockp: c_int, buf: *mut c_char, len: c_int, timeout: c_int) -> c_int;
    fn R_SockConnect(port: c_int, host: *mut c_char, timeout: c_int) -> c_int;
    fn R_SockClose(sockp: c_int) -> c_int;
    fn R_SockRead(
        sockp: c_int,
        buf: *mut c_void,
        maxlen: size_t,
        blocking: c_int,
        timeout: c_int,
    ) -> ssize_t;
    fn R_SockWrite(sockp: c_int, buf: *const c_void, len: size_t, timeout: c_int) -> ssize_t;
    fn R_set_nodelay(s: c_int) -> c_int;
    fn REprintf(format: *const i8);
    fn R_alloc(size: usize, nelem: usize) -> *mut c_void;
}

// ---------------------------------------------------------------------------
// Internal allocation helpers (using std::alloc instead of libc::malloc/free)
// ---------------------------------------------------------------------------

unsafe fn alloc_c_string(len: usize) -> *mut c_char {
    if len == 0 {
        return std::ptr::null_mut();
    }
    let layout = Layout::from_size_align_unchecked(len, 1);
    alloc(layout) as *mut c_char
}

unsafe fn free_c_string(p: *mut c_char) {
    if p.is_null() {
        return;
    }
    let len = strlen(p) + 1;
    let layout = Layout::from_size_align_unchecked(len, 1);
    dealloc(p as *mut u8, layout);
}

unsafe fn alloc_boxed<T>() -> *mut T {
    let layout = Layout::new::<T>();
    let ptr = alloc(layout) as *mut T;
    if !ptr.is_null() {
        core::ptr::write_bytes(ptr as *mut u8, 0, layout.size());
    }
    ptr
}

unsafe fn free_boxed<T>(p: *mut T) {
    if p.is_null() {
        return;
    }
    let layout = Layout::new::<T>();
    dealloc(p as *mut u8, layout);
}

// ---------------------------------------------------------------------------
// Internal helper: init_con
// ---------------------------------------------------------------------------

/// init_con - initialize connection fields that don't depend on the connection type.
/// Simplified version of Rf_init_con from connections.c.
/// Sets mode, isopen=false, text=true, canread/canwrite/canseek, blocking=true, etc.
unsafe fn init_con(new: Rconnection, description: *const c_char, enc: c_int, mode: *const c_char) {
    (*new).enc = enc;
    // Copy mode string (max 4 chars + null)
    if !mode.is_null() {
        let mut i = 0;
        while i < 4 {
            let c = *mode.add(i);
            (*new).mode[i] = c;
            if c == 0 {
                break;
            }
            i += 1;
        }
        // Ensure null termination
        (*new).mode[4] = 0;
    } else {
        (*new).mode[0] = 0;
    }
    (*new).isopen = R_FALSE;
    (*new).text = R_TRUE;
    (*new).incomplete = R_FALSE;
    // Determine canread/canwrite/canseek from mode
    let mode_str = core::ffi::CStr::from_ptr(mode);
    let mode_bytes = mode_str.to_bytes();
    let has_r = mode_bytes.iter().any(|&b| b == b'r');
    let has_w = mode_bytes.iter().any(|&b| b == b'w' || b == b'a');
    let has_plus = mode_bytes.iter().any(|&b| b == b'+');
    (*new).canread = if has_r || has_plus { R_TRUE } else { R_FALSE };
    (*new).canwrite = if has_w || has_plus { R_TRUE } else { R_FALSE };
    (*new).canseek = R_FALSE; // sockets are not seekable
    (*new).blocking = R_TRUE;
    (*new).isGzcon = R_FALSE;
    (*new).nPushBack = 0;
    (*new).posPushBack = 0;
    (*new).PushBack = core::ptr::null_mut();
    (*new).save = 0;
    (*new).save2 = 0;
    (*new).inconv = core::ptr::null_mut();
    (*new).outconv = core::ptr::null_mut();
    (*new).next = core::ptr::null_mut();
    (*new).navail = 0;
    (*new).inavail = 0;
    (*new).EOF_signalled = R_FALSE;
    (*new).UTF8out = R_FALSE;
    (*new).id = core::ptr::null_mut();
    (*new).ex_ptr = core::ptr::null_mut();
    (*new).status = 0;
    (*new).buff = core::ptr::null_mut();
    (*new).buff_len = 0;
    (*new).buff_stored_len = 0;
    (*new).buff_pos = 0;
    (*new).encname[0] = 0;
}

// ---------------------------------------------------------------------------
// Stub function pointers: dummy_vfprintf, dummy_fgetc
// ---------------------------------------------------------------------------

/// dummy_vfprintf - stub vfprintf for connections that don't support it.
/// Matches C: int dummy_vfprintf(Rconnection con, const char *format, va_list ap)
unsafe fn dummy_vfprintf(
    _con: Rconnection,
    _format: *const c_char,
    _ap: *mut c_void,
) -> c_int {
    -1
}

/// dummy_fgetc - stub fgetc for connections that use fgetc_internal instead.
/// Matches C: int dummy_fgetc(Rconnection con)
unsafe fn dummy_fgetc(_con: Rconnection) -> c_int {
    R_EOF
}

// ---------------------------------------------------------------------------
// listencleanup - cleanup handler for listening socket context
// ---------------------------------------------------------------------------

/// listencleanup - close the listening socket on error/jump.
/// Matches C: static void listencleanup(void *data)
unsafe fn listencleanup(data: *mut c_void) {
    let psock = data as *mut c_int;
    if !psock.is_null() {
        R_SockClose(*psock);
    }
}

// ---------------------------------------------------------------------------
// sock_open - open a socket connection
// ---------------------------------------------------------------------------

/// sock_open - open a socket connection (server or client).
/// Matches C: static Rboolean sock_open(Rconnection con)
unsafe fn sock_open(con: Rconnection) -> c_int {
    if con.is_null() {
        return R_FALSE;
    }

    let this = (*con).private as Rsockconn;
    if this.is_null() {
        return R_FALSE;
    }

    let mut sock: c_int;
    let mut sock1: c_int;
    let mlen: c_int;
    let mut timeout = (*this).timeout;

    let mut buf: [c_char; 256] = [0; 256];

    if timeout == NA_INTEGER || timeout <= 0 {
        timeout = 60;
    }

    // Reset the input buffer pointers
    (*this).pstart = (*this).inbuf.as_mut_ptr();
    (*this).pend = (*this).inbuf.as_mut_ptr();

    if (*this).server != 0 {
        // Server mode: open a listening socket, then accept
        if (*this).serverfd == -1 {
            // No pre-existing server fd: create one
            sock1 = R_SockOpen((*this).port);
            if sock1 < 0 {
                REprintf(b"port %d cannot be opened\n\0".as_ptr() as *const i8);
                // Note: C uses warning() with format args, we use REprintf with a simplified message
                return R_FALSE;
            }

            // Check FD_SETSIZE
            if sock1 as usize >= FD_SETSIZE as usize {
                R_SockClose(sock1);
                REprintf(b"file descriptor is too large for select()\n\0".as_ptr() as *const i8);
                return R_FALSE;
            }

            // Set up cleanup context (simplified: no R error handling context)
            // In full R, this uses begincontext/endcontext to close sock1 on longjmp.
            // Here we just call R_SockListen directly.
            sock = R_SockListen(sock1, buf.as_mut_ptr(), 256, timeout);
            R_SockClose(sock1);

            if sock < 0 {
                REprintf(b"problem in listening on this socket\n\0".as_ptr() as *const i8);
                return R_FALSE;
            }
        } else {
            // Use pre-existing server fd: just accept
            sock = R_SockListen((*this).serverfd, buf.as_mut_ptr(), 256, timeout);
            if sock < 0 {
                REprintf(
                    b"problem in accepting connections on this socket\n\0".as_ptr() as *const i8,
                );
                return R_FALSE;
            }
        }

        // Check FD_SETSIZE for the accepted socket
        if sock as usize >= FD_SETSIZE as usize && ((*con).canwrite != 0 || (*con).blocking != 0) {
            R_SockClose(sock);
            REprintf(b"file descriptor is too large for select()\n\0".as_ptr() as *const i8);
            return R_FALSE;
        }

        // Update description: "<-hostname:port"
        if !(*con).description.is_null() {
            free_c_string((*con).description);
        }
        let buf_len = strlen(buf.as_ptr());
        let sz = buf_len + 10;
        (*con).description = alloc_c_string(sz);
        if (*con).description.is_null() {
            REprintf(b"allocation of socket connection failed\n\0".as_ptr() as *const i8);
            return R_FALSE;
        }
        snprintf(
            (*con).description,
            sz,
            b"<-%s:%d\0".as_ptr() as *const c_char,
            buf.as_ptr(),
            (*this).port,
        );
    } else {
        // Client mode: connect to a remote host
        sock = R_SockConnect((*this).port, (*con).description, timeout);
        if sock < 0 {
            REprintf(b"cannot be opened\n\0".as_ptr() as *const i8);
            // Note: C uses warning("%s:%d cannot be opened", con->description, this->port)
            return R_FALSE;
        }

        // Update description: "->host:port"
        snprintf(
            buf.as_mut_ptr(),
            256,
            b"->%s:%d\0".as_ptr() as *const c_char,
            (*con).description,
            (*this).port,
        );
        strcpy((*con).description, buf.as_ptr());
    }

    (*this).fd = sock;

    // Set TCP_NODELAY if requested
    if (*this).options & RSC_SET_TCP_NODELAY != 0 {
        R_set_nodelay(sock);
    }

    mlen = strlen((*con).mode.as_ptr()) as c_int;
    (*con).isopen = R_TRUE;
    if mlen >= 2 && (*con).mode[(mlen - 1) as usize] == b'b' as c_char {
        (*con).text = R_FALSE;
    } else {
        (*con).text = R_TRUE;
    }

    // set_iconv: stub - no iconv support in this port
    // set_iconv(con);

    (*con).save = -1000;
    R_TRUE
}

// ---------------------------------------------------------------------------
// sock_close - close a socket connection
// ---------------------------------------------------------------------------

/// sock_close - close a socket connection.
/// Matches C: static void sock_close(Rconnection con)
unsafe fn sock_close(con: Rconnection) {
    if con.is_null() {
        return;
    }
    let this = (*con).private as Rsockconn;
    if !this.is_null() {
        R_SockClose((*this).fd);
    }
    (*con).isopen = R_FALSE;
}

// ---------------------------------------------------------------------------
// servsock_close - close a server socket connection
// ---------------------------------------------------------------------------

/// servsock_close - close a server socket connection.
/// Matches C: static void servsock_close(Rconnection con)
unsafe fn servsock_close(con: Rconnection) {
    if con.is_null() {
        return;
    }
    let this = (*con).private as Rservsockconn;
    if !this.is_null() {
        R_SockClose((*this).fd);
    }
    (*con).isopen = R_FALSE;
}

// ---------------------------------------------------------------------------
// sock_read_helper - read data from socket into buffer
// ---------------------------------------------------------------------------

/// sock_read_helper - read `size` bytes from the socket connection into `ptr`.
/// Uses an internal 4096-byte buffer for efficiency.
/// Matches C: static ssize_t sock_read_helper(Rconnection con, void *ptr, size_t size)
unsafe fn sock_read_helper(con: Rconnection, mut ptr: *mut c_void, mut size: size_t) -> ssize_t {
    if con.is_null() {
        return -1;
    }
    let this = (*con).private as Rsockconn;
    if this.is_null() {
        return -1;
    }

    let mut res: ssize_t;
    let mut nread: size_t = 0;
    let mut n: size_t;

    (*con).incomplete = R_FALSE;

    loop {
        // Read data into the buffer if it's empty and size > 0
        if size > 0 && (*this).pstart == (*this).pend {
            (*this).pstart = (*this).inbuf.as_mut_ptr();
            (*this).pend = (*this).inbuf.as_mut_ptr();

            loop {
                res = R_SockRead(
                    (*this).fd,
                    (*this).inbuf.as_mut_ptr() as *mut c_void,
                    4096,
                    (*con).blocking,
                    (*this).timeout,
                );
                // Retry on EINTR
                if -res != libc::EINTR as isize {
                    break;
                }
            }

            // Check for non-blocking EAGAIN/EWOULDBLOCK
            if (*con).blocking == 0
                && (-res == libc::EAGAIN as isize || -res == libc::EWOULDBLOCK as isize)
            {
                (*con).incomplete = R_TRUE;
                return nread as ssize_t;
            } else if res == 0 {
                // EOF
                return nread as ssize_t;
            } else if res < 0 {
                return res;
            } else {
                (*this).pend = (*this).inbuf.as_mut_ptr().add(res as usize);
            }
        }

        // Copy data from buffer to ptr
        let pstart_offset = (*this).pstart as usize;
        let pend_offset = (*this).pend as usize;
        if pstart_offset + size <= pend_offset {
            n = size;
        } else {
            n = pend_offset - pstart_offset;
        }
        if n > 0 {
            core::ptr::copy_nonoverlapping((*this).pstart, ptr as *mut c_char, n);
        }
        ptr = (ptr as *mut c_char).add(n) as *mut c_void;
        (*this).pstart = (*this).pstart.add(n);
        size -= n;
        nread += n;

        if size == 0 {
            break;
        }
    }

    nread as ssize_t
}

// ---------------------------------------------------------------------------
// sock_fgetc_internal - read a single character from socket
// ---------------------------------------------------------------------------

/// sock_fgetc_internal - read a single character from a socket connection.
/// Matches C: static int sock_fgetc_internal(Rconnection con)
unsafe fn sock_fgetc_internal(con: Rconnection) -> c_int {
    let mut c: u8 = 0;
    let n = sock_read_helper(con, &mut c as *mut u8 as *mut c_void, 1);
    if n == 1 { c as c_int } else { R_EOF }
}

// ---------------------------------------------------------------------------
// sock_read - fread-like read from socket connection
// ---------------------------------------------------------------------------

/// sock_read - read `nitems` objects of `size` bytes each from socket.
/// Matches C: static size_t sock_read(void *ptr, size_t size, size_t nitems, Rconnection con)
unsafe fn sock_read(
    ptr: *mut c_void,
    size: size_t,
    nitems: size_t,
    con: Rconnection,
) -> size_t {
    if size == 0 {
        return 0;
    }
    let n = sock_read_helper(con, ptr, size * nitems) / (size as ssize_t);
    if n > 0 { n as size_t } else { 0 }
}

// ---------------------------------------------------------------------------
// sock_write - fwrite-like write to socket connection
// ---------------------------------------------------------------------------

/// sock_write - write `nitems` objects of `size` bytes each to socket.
/// Matches C: static size_t sock_write(const void *ptr, size_t size, size_t nitems, Rconnection con)
unsafe fn sock_write(
    ptr: *const c_void,
    size: size_t,
    nitems: size_t,
    con: Rconnection,
) -> size_t {
    if con.is_null() {
        return 0;
    }
    let this = (*con).private as Rsockconn;
    if this.is_null() {
        return 0;
    }
    if size == 0 {
        return 0;
    }

    let n = R_SockWrite((*this).fd, ptr, size * nitems, (*this).timeout) / (size as ssize_t);
    if n > 0 { n as size_t } else { 0 }
}

// ---------------------------------------------------------------------------
// Trampoline functions: extern "C" wrappers for the function pointer fields
// ---------------------------------------------------------------------------

/// Trampoline for sock_open to match the Rconn.open function pointer signature.
unsafe fn sock_open_trampoline(con: *mut Rconn) -> c_int {
    sock_open(con)
}

/// Trampoline for sock_close to match the Rconn.close function pointer signature.
unsafe fn sock_close_trampoline(con: *mut Rconn) {
    sock_close(con);
}

/// Trampoline for servsock_close to match the Rconn.close function pointer signature.
unsafe fn servsock_close_trampoline(con: *mut Rconn) {
    servsock_close(con);
}

/// Trampoline for sock_fgetc_internal to match the Rconn.fgetc_internal function pointer signature.
unsafe fn sock_fgetc_internal_trampoline(con: *mut Rconn) -> c_int {
    sock_fgetc_internal(con)
}

// ---------------------------------------------------------------------------
// in_R_newsock - create a new socket connection
// ---------------------------------------------------------------------------

/// in_R_newsock - create a new socket connection (Rconnection).
/// Allocates and initializes the Rconn struct with socket-specific fields.
/// Matches C: Rconnection in_R_newsock(const char *host, int port, int server,
///              int serverfd, const char * const mode, int timeout, int options)
pub(crate) unsafe fn in_R_newsock(
    host: *const c_char,
    port: c_int,
    server: c_int,
    serverfd: c_int,
    mode: *const c_char,
    timeout: c_int,
    options: c_int,
) -> Rconnection {
    // Allocate the Rconn struct
    let new = alloc_boxed::<Rconn>();
    if new.is_null() {
        REprintf(b"allocation of socket connection failed\n\0".as_ptr() as *const i8);
        return core::ptr::null_mut();
    }

    // Allocate class name
    (*new).class = alloc_c_string(10);
    if (*new).class.is_null() {
        free_boxed(new);
        REprintf(b"allocation of socket connection failed\n\0".as_ptr() as *const i8);
        return core::ptr::null_mut();
    }
    strcpy((*new).class, b"sockconn\0".as_ptr() as *const c_char);

    // Allocate description
    let host_len = if host.is_null() { 0 } else { strlen(host) };
    let desc_size = host_len + 10;
    (*new).description = alloc_c_string(desc_size);
    if (*new).description.is_null() {
        free_c_string((*new).class);
        free_boxed(new);
        REprintf(b"allocation of socket connection failed\n\0".as_ptr() as *const i8);
        return core::ptr::null_mut();
    }
    if !host.is_null() {
        strcpy((*new).description, host);
    } else {
        (*new).description.write(0);
    }

    // Initialize common connection fields
    init_con(new, host, CE_NATIVE, mode);

    // Set function pointers
    (*new).open = Some(sock_open_trampoline);
    (*new).close = Some(sock_close_trampoline);
    (*new).vfprintf = Some(dummy_vfprintf);
    (*new).fgetc_internal = Some(sock_fgetc_internal_trampoline);
    (*new).fgetc = Some(dummy_fgetc);
    (*new).read_fn = Some(sock_read);
    (*new).write_fn = Some(sock_write);

    // Allocate private data (sockconn struct)
    let priv_data = alloc_boxed::<sockconn>();
    if priv_data.is_null() {
        free_c_string((*new).description);
        free_c_string((*new).class);
        free_boxed(new);
        REprintf(b"allocation of socket connection failed\n\0".as_ptr() as *const i8);
        return core::ptr::null_mut();
    }

    (*priv_data).port = port;
    (*priv_data).server = server;
    (*priv_data).timeout = timeout;
    (*priv_data).serverfd = serverfd;
    (*priv_data).options = options;
    (*priv_data).fd = -1;
    (*priv_data).host = core::ptr::null_mut();

    (*new).private = priv_data as *mut c_void;

    new
}

// ---------------------------------------------------------------------------
// in_R_newservsock - create a new server socket connection
// ---------------------------------------------------------------------------

/// in_R_newservsock - create a new server socket connection (Rconnection).
/// Opens a listening socket on the given port.
/// Matches C: Rconnection in_R_newservsock(int port)
pub(crate) unsafe fn in_R_newservsock(port: c_int) -> Rconnection {
    // Allocate the Rconn struct
    let new = alloc_boxed::<Rconn>();
    if new.is_null() {
        REprintf(b"allocation of server socket connection failed\n\0".as_ptr() as *const i8);
        return core::ptr::null_mut();
    }

    // Allocate class name
    (*new).class = alloc_c_string(14);
    if (*new).class.is_null() {
        free_boxed(new);
        REprintf(b"allocation of server socket connection failed\n\0".as_ptr() as *const i8);
        return core::ptr::null_mut();
    }
    strcpy((*new).class, b"servsockconn\0".as_ptr() as *const c_char);

    // Allocate description
    (*new).description = alloc_c_string(16);
    if (*new).description.is_null() {
        free_c_string((*new).class);
        free_boxed(new);
        REprintf(b"allocation of server socket connection failed\n\0".as_ptr() as *const i8);
        return core::ptr::null_mut();
    }
    strcpy((*new).description, b"localhost\0".as_ptr() as *const c_char);

    // Initialize common connection fields
    init_con(
        new,
        b"localhost\0".as_ptr() as *const c_char,
        CE_NATIVE,
        b"a+\0".as_ptr() as *const c_char,
    );

    // Set function pointers
    (*new).close = Some(servsock_close_trampoline);

    // Allocate private data (servsockconn struct)
    let priv_data = alloc_boxed::<servsockconn>();
    if priv_data.is_null() {
        free_c_string((*new).description);
        free_c_string((*new).class);
        free_boxed(new);
        REprintf(b"allocation of server socket connection failed\n\0".as_ptr() as *const i8);
        return core::ptr::null_mut();
    }

    (*priv_data).port = port;

    // socket(), bind(), listen()
    let sock = R_SockOpen(port);
    if sock < 0 {
        free_boxed(priv_data);
        free_c_string((*new).description);
        free_c_string((*new).class);
        free_boxed(new);
        REprintf(
            b"creation of server socket failed: port cannot be opened\n\0".as_ptr() as *const i8,
        );
        return core::ptr::null_mut();
    }

    // Check FD_SETSIZE
    if sock as usize >= FD_SETSIZE as usize {
        R_SockClose(sock);
        free_boxed(priv_data);
        free_c_string((*new).description);
        free_c_string((*new).class);
        free_boxed(new);
        REprintf(b"file descriptor is too large for select()\n\0".as_ptr() as *const i8);
        return core::ptr::null_mut();
    }

    (*priv_data).fd = sock;
    (*new).private = priv_data as *mut c_void;
    (*new).isopen = R_TRUE;

    new
}
