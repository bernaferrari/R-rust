//! Minimal `libc` facade for `rmath` on `wasm32-unknown-unknown`.
//!
//! The `wasm32-unknown-unknown` target has no operating system, so the
//! crates.io `libc` crate exposes almost none of the C library surface that
//! rmath's C-ported sources reference (`c_int`, `snprintf`, `strlen`, `FILE`,
//! `malloc`, sockets, ...). This crate is wired in as a **target-specific
//! dependency override** in `rmath/Cargo.toml`
//! (`[target.'cfg(target_arch = "wasm32")'.dependencies] libc = { path = ... }`)
//! and implements, in pure safe-dependency-free Rust, exactly the surface the
//! rmath tree uses (grep-driven; see `scripts/wasm_toolchain_check.sh`).
//!
//! # Policy (documented, honest behavior)
//!
//! * **Pure computation** (string.h, ctype, atoi/strtol/strtod, byte order,
//!   calendar math): real implementations with C semantics.
//! * **Heap** (`malloc`/`calloc`/`realloc`/`free`/`strdup`): backed by Rust's
//!   global allocator with a 16-byte size header so `free`/`realloc` get the
//!   exact `Layout` back. No leaks, no bump arena.
//! * **Clock**: `SystemTime`/`Instant` under UTC (the only zone on wasm).
//! * **stdio** (`FILE*`): there is no filesystem, so `fopen`/`tmpfile` return
//!   `NULL` and every operation on a `FILE*` reports failure (`EOF`, short
//!   count, `-1`). Callers already treat those as clean errors ("cannot open
//!   file", ...). `FILE` is an opaque zero-sized type; no `FILE*` is ever
//!   dereferenced here.
//! * **Environment** (`getenv`/`setenv`/`unsetenv`): no environment exists;
//!   `getenv` returns `NULL`, mutators return `-1`.
//! * **Processes, signals, rlimits, sockets, fds**: no OS services exist.
//!   Queries return fixed deterministic values (documented per function);
//!   operations that would need the OS return `-1` failure. errno-style
//!   constants use the Linux/glibc numeric values as the port's convention.
//! * **`snprintf`**: C variadic definitions are not expressible in stable
//!   Rust, so this facade exports a typed variant [`snprintf_args`] taking
//!   [`CArg`] values; rmath's `rport_snprintf!` macro dispatches to it on
//!   wasm and to the real variadic `libc::snprintf` on every other target.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::undocumented_unsafe_blocks)]

pub use std::os::raw::{
    c_double, c_int, c_long, c_longlong, c_uchar, c_uint, c_ulong, c_ulonglong, c_void,
};

pub type c_char = std::os::raw::c_char;

pub type size_t = usize;
pub type ssize_t = isize;
pub type ptrdiff_t = isize;
pub type intptr_t = isize;
pub type uintptr_t = usize;
pub type off_t = i64;
pub type time_t = i64;
pub type clock_t = i64;
pub type clockid_t = c_int;
pub type pid_t = i32;
pub type uid_t = u32;
pub type gid_t = u32;
pub type id_t = u32;
pub type mode_t = u32;
pub type dev_t = u64;
pub type ino_t = u64;
pub type nlink_t = u64;
pub type rlim_t = u64;
pub type suseconds_t = c_long;
pub type socklen_t = u32;
pub type sa_family_t = u16;
pub type in_addr_t = u32;
pub type in_port_t = u16;
pub type sighandler_t = usize;
pub type BOOLEAN = c_uchar;

// ---------------------------------------------------------------------------
// errno (Linux/glibc values; wasm has no real errno — stubs set this cell)
// ---------------------------------------------------------------------------

pub const EPERM: c_int = 1;
pub const ENOENT: c_int = 2;
pub const ESRCH: c_int = 3;
pub const EINTR: c_int = 4;
pub const EIO: c_int = 5;
pub const EBADF: c_int = 9;
pub const EAGAIN: c_int = 11;
pub const ENOMEM: c_int = 12;
pub const EACCES: c_int = 13;
pub const EFAULT: c_int = 14;
pub const EBUSY: c_int = 16;
pub const EEXIST: c_int = 17;
pub const ENOTDIR: c_int = 20;
pub const EISDIR: c_int = 21;
pub const EINVAL: c_int = 22;
pub const EMFILE: c_int = 24;
pub const EPIPE: c_int = 32;
pub const ERANGE: c_int = 34;
pub const EPROTO: c_int = 71;
pub const EOPNOTSUPP: c_int = 95;
pub const EADDRINUSE: c_int = 98;
pub const EADDRNOTAVAIL: c_int = 99;
pub const ENETDOWN: c_int = 100;
pub const ENETUNREACH: c_int = 101;
pub const ECONNABORTED: c_int = 103;
pub const ECONNREFUSED: c_int = 111;
pub const EINPROGRESS: c_int = 115;
pub const EWOULDBLOCK: c_int = EAGAIN;
pub const EAI_NONAME: c_int = -2;
pub const EAI_FAIL: c_int = -4;

thread_local! {
    static ERRNO: std::cell::Cell<c_int> = const { std::cell::Cell::new(0) };
}

/// errno cell (macOS `__error`-shaped accessor; wasm stubs store the error of
/// the most recent failed OS-service call here).
pub fn __error() -> *mut c_int {
    ERRNO.with(std::cell::Cell::as_ptr)
}

// ---------------------------------------------------------------------------
// stdio — no filesystem: clean failure everywhere
// ---------------------------------------------------------------------------

/// Opaque stdio stream. No `FILE` is ever created on wasm; all `FILE*` APIs
/// report failure without dereferencing the pointer.
#[repr(C)]
pub struct FILE {
    _opaque: [u8; 0],
}

pub type fpos_t = i64;

pub const EOF: c_int = -1;
pub const SEEK_SET: c_int = 0;
pub const SEEK_CUR: c_int = 1;
pub const SEEK_END: c_int = 2;
pub const _IOFBF: c_int = 0;
pub const _IOLBF: c_int = 1;
pub const _IONBF: c_int = 2;

pub unsafe fn fopen(_path: *const c_char, _mode: *const c_char) -> *mut FILE {
    ERRNO.with(|e| e.set(ENOENT));
    std::ptr::null_mut()
}

pub unsafe fn tmpfile() -> *mut FILE {
    ERRNO.with(|e| e.set(EACCES));
    std::ptr::null_mut()
}

pub unsafe fn fclose(_f: *mut FILE) -> c_int {
    if _f.is_null() { EOF } else { 0 }
}

pub unsafe fn fread(_ptr: *mut c_void, _size: size_t, _nmemb: size_t, _f: *mut FILE) -> size_t {
    0
}

pub unsafe fn fwrite(_ptr: *const c_void, _size: size_t, _nmemb: size_t, _f: *mut FILE) -> size_t {
    0
}

pub unsafe fn fseek(_f: *mut FILE, _offset: c_long, _whence: c_int) -> c_int {
    -1
}

pub unsafe fn ftell(_f: *mut FILE) -> c_long {
    -1
}

pub unsafe fn rewind(_f: *mut FILE) {}

pub unsafe fn fflush(_f: *mut FILE) -> c_int {
    0
}

pub unsafe fn feof(_f: *mut FILE) -> c_int {
    1
}

pub unsafe fn ferror(_f: *mut FILE) -> c_int {
    1
}

pub unsafe fn fgetc(_f: *mut FILE) -> c_int {
    EOF
}

pub unsafe fn fgets(_buf: *mut c_char, _n: c_int, _f: *mut FILE) -> *mut c_char {
    std::ptr::null_mut()
}

pub unsafe fn fputc(_c: c_int, _f: *mut FILE) -> c_int {
    EOF
}

pub unsafe fn fputs(_s: *const c_char, _f: *mut FILE) -> c_int {
    EOF
}

pub unsafe fn ungetc(_c: c_int, _f: *mut FILE) -> c_int {
    EOF
}

pub unsafe fn remove(_path: *const c_char) -> c_int {
    ERRNO.with(|e| e.set(ENOENT));
    -1
}

pub unsafe fn unlink(_path: *const c_char) -> c_int {
    ERRNO.with(|e| e.set(ENOENT));
    -1
}

// ---------------------------------------------------------------------------
// string.h / ctype — real implementations
// ---------------------------------------------------------------------------

unsafe fn bytes_of(s: *const c_char) -> &'static [u8] {
    if s.is_null() {
        &[]
    } else {
        let mut len = 0usize;
        while unsafe { *s.add(len) } != 0 {
            len += 1;
        }
        unsafe { std::slice::from_raw_parts(s.cast::<u8>(), len) }
    }
}

pub unsafe fn strlen(s: *const c_char) -> size_t {
    unsafe { bytes_of(s).len() }
}

pub unsafe fn strcpy(dest: *mut c_char, src: *const c_char) -> *mut c_char {
    unsafe {
        if dest.is_null() {
            return dest;
        }
        let src_bytes = bytes_of(src);
        std::ptr::copy_nonoverlapping(src_bytes.as_ptr(), dest.cast::<u8>(), src_bytes.len());
        *dest.add(src_bytes.len()) = 0;
        dest
    }
}

pub unsafe fn strncpy(dest: *mut c_char, src: *const c_char, n: size_t) -> *mut c_char {
    unsafe {
        if dest.is_null() || n == 0 {
            return dest;
        }
        let src_bytes = bytes_of(src);
        let copy = src_bytes.len().min(n);
        std::ptr::copy_nonoverlapping(src_bytes.as_ptr(), dest.cast::<u8>(), copy);
        // C semantics: zero-fill the remainder of the buffer
        let fill = n - copy;
        std::ptr::write_bytes(dest.add(copy).cast::<u8>(), 0, fill);
        dest
    }
}

pub unsafe fn strcat(dest: *mut c_char, src: *const c_char) -> *mut c_char {
    unsafe {
        if dest.is_null() {
            return dest;
        }
        let dlen = bytes_of(dest).len();
        strcpy(dest.add(dlen), src);
        dest
    }
}

pub unsafe fn strncat(dest: *mut c_char, src: *const c_char, n: size_t) -> *mut c_char {
    unsafe {
        if dest.is_null() {
            return dest;
        }
        let dlen = bytes_of(dest).len();
        let src_bytes = bytes_of(src);
        let copy = src_bytes.len().min(n);
        std::ptr::copy_nonoverlapping(src_bytes.as_ptr(), dest.add(dlen).cast::<u8>(), copy);
        *dest.add(dlen + copy) = 0;
        dest
    }
}

pub unsafe fn strcmp(s1: *const c_char, s2: *const c_char) -> c_int {
    unsafe {
        let (a, b) = (bytes_of(s1), bytes_of(s2));
        match a.cmp(b) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

pub unsafe fn strncmp(s1: *const c_char, s2: *const c_char, n: size_t) -> c_int {
    unsafe {
        if n == 0 {
            return 0;
        }
        let (a, b) = (bytes_of(s1), bytes_of(s2));
        let (a, b) = (&a[..a.len().min(n)], &b[..b.len().min(n)]);
        match a.cmp(b) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

pub unsafe fn strcasecmp(s1: *const c_char, s2: *const c_char) -> c_int {
    unsafe {
        let (a, b) = (bytes_of(s1), bytes_of(s2));
        let n = a.len().min(b.len());
        for i in 0..n {
            let (x, y) = (a[i].to_ascii_lowercase(), b[i].to_ascii_lowercase());
            if x != y {
                return x as c_int - y as c_int;
            }
        }
        a.len().cmp(&b.len()) as c_int
    }
}

pub unsafe fn strncasecmp(s1: *const c_char, s2: *const c_char, n: size_t) -> c_int {
    unsafe {
        if n == 0 {
            return 0;
        }
        let (a, b) = (bytes_of(s1), bytes_of(s2));
        let m = a.len().min(b.len()).min(n);
        for i in 0..m {
            let (x, y) = (a[i].to_ascii_lowercase(), b[i].to_ascii_lowercase());
            if x != y {
                return x as c_int - y as c_int;
            }
        }
        if m == n || a.len() == b.len() {
            0
        } else {
            a.len().cmp(&b.len()) as c_int
        }
    }
}

pub unsafe fn strchr(s: *const c_char, c: c_int) -> *mut c_char {
    unsafe {
        let needle = c as u8;
        let bytes = bytes_of(s);
        // Searching for the terminator returns a pointer to the NUL.
        let found = if needle == 0 {
            Some(bytes.len())
        } else {
            bytes.iter().position(|&b| b == needle)
        };
        match found {
            Some(i) => s.add(i) as *mut c_char,
            None => std::ptr::null_mut(),
        }
    }
}

pub unsafe fn strrchr(s: *const c_char, c: c_int) -> *mut c_char {
    unsafe {
        let c = c as u8;
        let bytes = bytes_of(s);
        let found = if c == 0 {
            Some(bytes.len())
        } else {
            bytes.iter().rposition(|&b| b == c)
        };
        match found {
            Some(i) => s.add(i) as *mut c_char,
            None => std::ptr::null_mut(),
        }
    }
}

pub unsafe fn strstr(haystack: *const c_char, needle: *const c_char) -> *mut c_char {
    unsafe {
        let (hay, needle) = (bytes_of(haystack), bytes_of(needle));
        if needle.is_empty() {
            return haystack as *mut c_char;
        }
        if let Some(pos) = find_sub(hay, needle) {
            return haystack.add(pos) as *mut c_char;
        }
        std::ptr::null_mut()
    }
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

pub unsafe fn strerror(errnum: c_int) -> *mut c_char {
    let msg: &[u8] = match errnum {
        0 => b"Success\0",
        EPERM => b"Operation not permitted\0",
        ENOENT => b"No such file or directory\0",
        EINTR => b"Interrupted system call\0",
        EBADF => b"Bad file descriptor\0",
        EAGAIN => b"Resource temporarily unavailable\0",
        ENOMEM => b"Cannot allocate memory\0",
        EACCES => b"Permission denied\0",
        EEXIST => b"File exists\0",
        EINVAL => b"Invalid argument\0",
        EPIPE => b"Broken pipe\0",
        ERANGE => b"Numerical result out of range\0",
        EPROTO => b"Protocol error\0",
        EADDRINUSE => b"Address already in use\0",
        ECONNREFUSED => b"Connection refused\0",
        EINPROGRESS => b"Operation now in progress\0",
        _ => b"Unknown error\0",
    };
    msg.as_ptr() as *mut c_char
}

pub unsafe fn memcpy(dest: *mut c_void, src: *const c_void, n: size_t) -> *mut c_void {
    unsafe {
        std::ptr::copy_nonoverlapping(src.cast::<u8>(), dest.cast::<u8>(), n);
        dest
    }
}

pub unsafe fn memmove(dest: *mut c_void, src: *const c_void, n: size_t) -> *mut c_void {
    unsafe {
        std::ptr::copy(src.cast::<u8>(), dest.cast::<u8>(), n);
        dest
    }
}

pub unsafe fn memset(s: *mut c_void, c: c_int, n: size_t) -> *mut c_void {
    unsafe {
        std::ptr::write_bytes(s.cast::<u8>(), c as u8, n);
        s
    }
}

pub unsafe fn memcmp(s1: *const c_void, s2: *const c_void, n: size_t) -> c_int {
    unsafe {
        let a = std::slice::from_raw_parts(s1.cast::<u8>(), n);
        let b = std::slice::from_raw_parts(s2.cast::<u8>(), n);
        match a.cmp(b) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// ctype — ASCII, C locale
// ---------------------------------------------------------------------------

pub fn isalnum(c: c_int) -> c_int {
    ((c as u8).is_ascii_alphanumeric() && c >= 0 && c < 128) as c_int
}
pub fn isalpha(c: c_int) -> c_int {
    ((c as u8).is_ascii_alphabetic() && c >= 0 && c < 128) as c_int
}
pub fn iscntrl(c: c_int) -> c_int {
    ((c as u8).is_ascii_control() && c >= 0 && c < 128) as c_int
}
pub fn isdigit(c: c_int) -> c_int {
    ((c as u8).is_ascii_digit() && c >= 0 && c < 128) as c_int
}
pub fn islower(c: c_int) -> c_int {
    ((c as u8).is_ascii_lowercase() && c >= 0 && c < 128) as c_int
}
pub fn isprint(c: c_int) -> c_int {
    let printable = (c as u8).is_ascii_graphic() || c == b' ' as c_int;
    (printable && (0..128).contains(&c)) as c_int
}
pub fn ispunct(c: c_int) -> c_int {
    ((c as u8).is_ascii_punctuation() && c >= 0 && c < 128) as c_int
}
pub fn isspace(c: c_int) -> c_int {
    (c == b' ' as c_int
        || c == b'\t' as c_int
        || c == b'\n' as c_int
        || c == b'\r' as c_int
        || c == 0x0b
        || c == 0x0c) as c_int
}
pub fn isupper(c: c_int) -> c_int {
    ((c as u8).is_ascii_uppercase() && c >= 0 && c < 128) as c_int
}
pub fn isxdigit(c: c_int) -> c_int {
    ((c as u8).is_ascii_hexdigit() && c >= 0 && c < 128) as c_int
}
pub fn tolower(c: c_int) -> c_int {
    if c >= 0 && c < 128 {
        (c as u8).to_ascii_lowercase() as c_int
    } else {
        c
    }
}
pub fn toupper(c: c_int) -> c_int {
    if c >= 0 && c < 128 {
        (c as u8).to_ascii_uppercase() as c_int
    } else {
        c
    }
}

// ---------------------------------------------------------------------------
// stdlib — heap via the Rust global allocator (16-byte size header), env absent
// ---------------------------------------------------------------------------

const HEAP_ALIGN: usize = 16;
const HEADER: usize = HEAP_ALIGN;

unsafe fn heap_alloc(size: size_t) -> *mut c_void {
    unsafe {
        use std::alloc::{Layout, alloc};
        let total = size + HEADER;
        let layout = Layout::from_size_align_unchecked(total, HEAP_ALIGN);
        let base = alloc(layout);
        if base.is_null() {
            return std::ptr::null_mut();
        }
        (base as *mut usize).write(size);
        base.add(HEADER).cast::<c_void>()
    }
}

unsafe fn heap_size(ptr: *mut c_void) -> size_t {
    unsafe { *(ptr.sub(HEADER) as *const usize) }
}

pub unsafe fn malloc(size: size_t) -> *mut c_void {
    unsafe {
        if size == 0 {
            // C: malloc(0) may return NULL or a unique pointer; glibc returns
            // a unique freeable pointer, so mirror that.
            return heap_alloc(1);
        }
        heap_alloc(size)
    }
}

pub unsafe fn calloc(nmemb: size_t, size: size_t) -> *mut c_void {
    unsafe {
        let Some(total) = nmemb.checked_mul(size) else {
            return std::ptr::null_mut();
        };
        let ptr = malloc(total);
        if !ptr.is_null() {
            std::ptr::write_bytes(ptr.cast::<u8>(), 0, total);
        }
        ptr
    }
}

pub unsafe fn realloc(ptr: *mut c_void, size: size_t) -> *mut c_void {
    unsafe {
        if ptr.is_null() {
            return malloc(size);
        }
        if size == 0 {
            free(ptr);
            return std::ptr::null_mut();
        }
        let old = heap_size(ptr);
        if old >= size {
            // Keep the old block; shrink is a no-op (documented choice).
            return ptr;
        }
        let new = malloc(size);
        if new.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(ptr.cast::<u8>(), new.cast::<u8>(), old);
        free(ptr);
        new
    }
}

pub unsafe fn free(ptr: *mut c_void) {
    unsafe {
        if ptr.is_null() {
            return;
        }
        use std::alloc::{Layout, dealloc};
        let size = heap_size(ptr);
        let layout = Layout::from_size_align_unchecked(size + HEADER, HEAP_ALIGN);
        dealloc(ptr.sub(HEADER).cast::<u8>(), layout);
    }
}

pub unsafe fn strdup(s: *const c_char) -> *mut c_char {
    unsafe {
        let bytes = bytes_of(s);
        let len = bytes.len() + 1;
        let ptr = malloc(len) as *mut c_char;
        if ptr.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), len);
        ptr
    }
}

pub unsafe fn strndup(s: *const c_char, n: size_t) -> *mut c_char {
    unsafe {
        let bytes = bytes_of(s);
        let len = bytes.len().min(n) + 1;
        let ptr = malloc(len) as *mut c_char;
        if ptr.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), len - 1);
        *ptr.add(len - 1) = 0;
        ptr
    }
}

pub unsafe fn getenv(_name: *const c_char) -> *mut c_char {
    std::ptr::null_mut()
}

pub unsafe fn setenv(_name: *const c_char, _value: *const c_char, _overwrite: c_int) -> c_int {
    -1
}

pub unsafe fn unsetenv(_name: *const c_char) -> c_int {
    -1
}

pub unsafe fn abort() -> ! {
    std::process::abort()
}

pub unsafe fn exit(code: c_int) -> ! {
    std::process::exit(code)
}

pub fn _exit(code: c_int) -> ! {
    std::process::exit(code)
}

pub unsafe fn atoi(s: *const c_char) -> c_int {
    unsafe { strtol(s, std::ptr::null_mut(), 10) as c_int }
}

pub unsafe fn atol(s: *const c_char) -> c_long {
    unsafe { strtol(s, std::ptr::null_mut(), 10) }
}

pub unsafe fn atoll(s: *const c_char) -> c_longlong {
    unsafe { strtoll(s, std::ptr::null_mut(), 10) }
}

pub unsafe fn strtol(s: *const c_char, endptr: *mut *mut c_char, base: c_int) -> c_long {
    unsafe { strto_int(bytes_of(s), endptr, base) as c_long }
}

pub unsafe fn strtoll(s: *const c_char, endptr: *mut *mut c_char, base: c_int) -> c_longlong {
    unsafe { strto_int(bytes_of(s), endptr, base) as c_longlong }
}

pub unsafe fn strtoul(s: *const c_char, endptr: *mut *mut c_char, base: c_int) -> c_ulong {
    unsafe { strto_int(bytes_of(s), endptr, base) as c_ulong }
}

unsafe fn strto_int(b: &[u8], endptr: *mut *mut c_char, base: c_int) -> i128 {
    unsafe {
        let mut i = 0;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let start_after_ws = i;
        let mut neg = false;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            neg = b[i] == b'-';
            i += 1;
        }
        let mut base = base as u32;
        if (base == 16 || base == 0) && i + 1 < b.len() && b[i] == b'0' && (b[i + 1] | 0x20) == b'x'
        {
            i += 2;
            base = 16;
        } else if base == 0 {
            base = if i < b.len() && b[i] == b'0' { 8 } else { 10 };
        }
        let mut val: i128 = 0;
        let mut any = false;
        while i < b.len() {
            let Some(d) = (b[i] as char).to_digit(base) else {
                break;
            };
            val = val * base as i128 + d as i128;
            any = true;
            i += 1;
        }
        let end = if any {
            b.as_ptr().add(i)
        } else {
            b.as_ptr().add(start_after_ws)
        };
        if !endptr.is_null() {
            *endptr = end as *mut c_char;
        }
        if neg { -val } else { val }
    }
}

pub unsafe fn strtod(s: *const c_char, endptr: *mut *mut c_char) -> c_double {
    unsafe {
        let b = bytes_of(s);
        let mut i = 0;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let num_start = i;
        let mut seen_digit = false;
        let mut seen_dot = false;
        let mut seen_exp = false;
        while i < b.len() {
            let c = b[i];
            match c {
                b'0'..=b'9' => seen_digit = true,
                b'+' | b'-' if i == num_start || (seen_exp && matches!(b[i - 1], b'e' | b'E')) => {}
                b'.' if !seen_dot && !seen_exp => seen_dot = true,
                b'e' | b'E' if seen_digit && !seen_exp => seen_exp = true,
                _ => break,
            }
            i += 1;
        }
        // Rust's parser accepts "inf"/"nan" too; C does as well.
        let text = std::str::from_utf8(&b[num_start..i]).unwrap_or("");
        let val: c_double = text.parse().unwrap_or_else(|_| {
            // try inf/nan spellings that Rust parses
            let t = text.to_ascii_lowercase();
            if t.starts_with("inf") || t.starts_with("+inf") || t.starts_with("-inf") {
                if t.starts_with('-') {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                }
            } else if t.starts_with("nan") {
                f64::NAN
            } else {
                0.0
            }
        });
        if !endptr.is_null() {
            let end = if text.is_empty() {
                s as *const c_char
            } else {
                b.as_ptr().add(i) as *const c_char
            };
            *endptr = end as *mut c_char;
        }
        val
    }
}

// ---------------------------------------------------------------------------
// time — UTC-only calendar, SystemTime/Instant-backed
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct tm {
    pub tm_sec: c_int,
    pub tm_min: c_int,
    pub tm_hour: c_int,
    pub tm_mday: c_int,
    pub tm_mon: c_int,
    pub tm_year: c_int,
    pub tm_wday: c_int,
    pub tm_yday: c_int,
    pub tm_isdst: c_int,
    pub tm_gmtoff: c_long,
    pub tm_zone: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct timeval {
    pub tv_sec: time_t,
    pub tv_usec: suseconds_t,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct timespec {
    pub tv_sec: time_t,
    pub tv_nsec: c_long,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct timezone_ {
    pub tz_minuteswest: c_int,
    pub tz_dsttime: c_int,
}

pub const CLOCK_REALTIME: clockid_t = 0;
pub const CLOCK_MONOTONIC: clockid_t = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: clockid_t = 2;
pub const CLOCK_THREAD_CPUTIME_ID: clockid_t = 3;

pub unsafe fn time(t: *mut time_t) -> time_t {
    let now = system_seconds();
    unsafe {
        if !t.is_null() {
            *t = now;
        }
    }
    now
}

fn system_seconds() -> time_t {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as time_t)
        .unwrap_or(0)
}

pub unsafe fn clock_gettime(_clk_id: clockid_t, tp: *mut timespec) -> c_int {
    unsafe {
        if tp.is_null() {
            return -1;
        }
        // Both clocks report the wall clock: wasm has no per-process CPU
        // clocks, and Instant's base is opaque on this target.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        (*tp).tv_sec = now.as_secs() as time_t;
        (*tp).tv_nsec = now.subsec_nanos() as c_long;
        0
    }
}

pub unsafe fn gettimeofday(tv: *mut timeval, _tz: *mut c_void) -> c_int {
    unsafe {
        if !tv.is_null() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            (*tv).tv_sec = now.as_secs() as time_t;
            (*tv).tv_usec = now.subsec_micros() as suseconds_t;
        }
        0
    }
}

/// Days-from-civil (Howard Hinnant's algorithm), valid for the full range.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m as i64 + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn fill_tm(secs: i64, out: &mut tm) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (year, month, mday) = civil_from_days(days);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let cum = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let yday = cum[(month - 1) as usize] + mday as i64 + if leap && month > 2 { 1 } else { 0 } - 1;
    out.tm_sec = (rem % 60) as c_int;
    out.tm_min = ((rem / 60) % 60) as c_int;
    out.tm_hour = (rem / 3600) as c_int;
    out.tm_mday = mday as c_int;
    out.tm_mon = (month - 1) as c_int;
    out.tm_year = (year - 1900) as c_int;
    out.tm_wday = ((days + 4).rem_euclid(7)) as c_int; // 1970-01-01 = Thursday
    out.tm_yday = yday as c_int;
    out.tm_isdst = 0;
    out.tm_gmtoff = 0; // wasm is UTC-only
    out.tm_zone = UTC_NAME.as_ptr() as *const c_char;
}

const UTC_NAME: &[u8] = b"UTC\0";

pub unsafe fn gmtime_r(time_p: *const time_t, result: *mut tm) -> *mut tm {
    unsafe {
        if result.is_null() {
            return std::ptr::null_mut();
        }
        let secs = if time_p.is_null() { 0 } else { *time_p };
        fill_tm(secs, &mut *result);
        result
    }
}

/// wasm has no timezone database: local time is UTC.
pub unsafe fn localtime_r(time_p: *const time_t, result: *mut tm) -> *mut tm {
    unsafe { gmtime_r(time_p, result) }
}

pub unsafe fn mktime(tm_: *mut tm) -> time_t {
    unsafe {
        if tm_.is_null() {
            return -1;
        }
        let t = *tm_;
        let (year, mon, mday) = (
            t.tm_year as i64 + 1900,
            (t.tm_mon + 1).max(1).min(12) as u32,
            t.tm_mday.max(1) as u32,
        );
        days_from_civil(year, mon, mday) * 86400
            + t.tm_hour as i64 * 3600
            + t.tm_min as i64 * 60
            + t.tm_sec as i64
    }
}

const WDAY: [&[u8]; 7] = [
    b"Sun\0", b"Mon\0", b"Tue\0", b"Wed\0", b"Thu\0", b"Fri\0", b"Sat\0",
];
const WDAY_FULL: [&[u8]; 7] = [
    b"Sunday\0",
    b"Monday\0",
    b"Tuesday\0",
    b"Wednesday\0",
    b"Thursday\0",
    b"Friday\0",
    b"Saturday\0",
];
const MON: [&[u8]; 12] = [
    b"Jan\0", b"Feb\0", b"Mar\0", b"Apr\0", b"May\0", b"Jun\0", b"Jul\0", b"Aug\0", b"Sep\0",
    b"Oct\0", b"Nov\0", b"Dec\0",
];
const MON_FULL: [&[u8]; 12] = [
    b"January\0",
    b"February\0",
    b"March\0",
    b"April\0",
    b"May\0",
    b"June\0",
    b"July\0",
    b"August\0",
    b"September\0",
    b"October\0",
    b"November\0",
    b"December\0",
];

/// strftime with the C-locale conversions R's datetime code uses
/// (%a %A %b %B %c %C %d %D %e %F %g %G %H %I %j %m %M %n %p %r %R %S %t %T
/// %u %U %V %w %W %x %X %y %Y %z %Z %%).
pub unsafe fn strftime(
    s: *mut c_char,
    max: size_t,
    format: *const c_char,
    tm_: *const tm,
) -> size_t {
    unsafe {
        if s.is_null() || max == 0 || tm_.is_null() {
            return 0;
        }
        let fmt = bytes_of(format);
        let t = *tm_;
        let mut out: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < fmt.len() {
            if fmt[i] != b'%' {
                if out.len() + 1 >= max {
                    break;
                }
                out.push(fmt[i]);
                i += 1;
                continue;
            }
            i += 1;
            if i >= fmt.len() {
                break;
            }
            if fmt[i] == b'E' || fmt[i] == b'O' {
                i += 1; // locale modifiers: ignore in C locale
                if i >= fmt.len() {
                    break;
                }
            }
            let two = |v: i32| v.to_string().into_bytes();
            let pad2 = |v: i32| -> Vec<u8> {
                let neg = v < 0;
                let a = v.unsigned_abs().to_string();
                let mut s = String::with_capacity(4);
                if neg {
                    s.push('-');
                }
                if !neg && a.len() < 2 {
                    s.push('0');
                }
                s.push_str(&a);
                s.into_bytes()
            };
            let c = fmt[i];
            i += 1;
            let piece: Vec<u8> = match c {
                b'a' => WDAY[(t.tm_wday as usize).min(6)]
                    .split(|&b| b == 0)
                    .next()
                    .unwrap()
                    .to_vec(),
                b'A' => WDAY_FULL[(t.tm_wday as usize).min(6)]
                    .split(|&b| b == 0)
                    .next()
                    .unwrap()
                    .to_vec(),
                b'b' | b'h' => MON[(t.tm_mon as usize).min(11)]
                    .split(|&b| b == 0)
                    .next()
                    .unwrap()
                    .to_vec(),
                b'B' => MON_FULL[(t.tm_mon as usize).min(11)]
                    .split(|&b| b == 0)
                    .next()
                    .unwrap()
                    .to_vec(),
                b'C' => pad2((t.tm_year / 100 + 19) as i32),
                b'd' => pad2(t.tm_mday),
                b'D' => format!(
                    "{:02}/{:02}/{:02}",
                    t.tm_mon + 1,
                    t.tm_mday,
                    (t.tm_year % 100).rem_euclid(100)
                )
                .into_bytes(),
                b'e' => format!("{:2}", t.tm_mday).into_bytes(),
                b'F' => format!("{}-{:02}-{:02}", t.tm_year + 1900, t.tm_mon + 1, t.tm_mday)
                    .into_bytes(),
                b'H' => pad2(t.tm_hour),
                b'I' => {
                    let h = t.tm_hour % 12;
                    pad2(if h == 0 { 12 } else { h })
                }
                b'j' => format!("{:03}", t.tm_yday + 1).into_bytes(),
                b'm' => pad2(t.tm_mon + 1),
                b'M' => pad2(t.tm_min),
                b'n' => b"\n".to_vec(),
                b'p' => if t.tm_hour < 12 { b"AM" } else { b"PM" }.to_vec(),
                b'P' => if t.tm_hour < 12 { b"am" } else { b"pm" }.to_vec(),
                b'r' => format!(
                    "{:02}:{:02}:{:02} {}",
                    {
                        let h = t.tm_hour % 12;
                        if h == 0 { 12 } else { h }
                    },
                    t.tm_min,
                    t.tm_sec,
                    if t.tm_hour < 12 { "AM" } else { "PM" }
                )
                .into_bytes(),
                b'R' => format!("{:02}:{:02}", t.tm_hour, t.tm_min).into_bytes(),
                b'S' => pad2(t.tm_sec),
                b't' => b"\t".to_vec(),
                b'T' => format!("{:02}:{:02}:{:02}", t.tm_hour, t.tm_min, t.tm_sec).into_bytes(),
                b'u' => {
                    let d = if t.tm_wday == 0 { 7 } else { t.tm_wday };
                    two(d)
                }
                b'U' => format!("{:02}", (t.tm_yday + 7 - t.tm_wday) / 7).into_bytes(),
                b'V' | b'G' | b'g' => {
                    // ISO 8601 week-based values
                    let (iso_y, iso_w, _) = iso_week_year(&t);
                    match c {
                        b'V' => format!("{:02}", iso_w).into_bytes(),
                        b'G' => iso_y.to_string().into_bytes(),
                        _ => pad2((iso_y % 100).rem_euclid(100)),
                    }
                }
                b'w' => two(t.tm_wday),
                b'W' => format!("{:02}", (t.tm_yday + 7 - ((t.tm_wday + 6) % 7)) / 7).into_bytes(),
                b'x' => format!(
                    "{:02}/{:02}/{:02}",
                    t.tm_mon + 1,
                    t.tm_mday,
                    (t.tm_year % 100).rem_euclid(100)
                )
                .into_bytes(),
                b'X' => format!("{:02}:{:02}:{:02}", t.tm_hour, t.tm_min, t.tm_sec).into_bytes(),
                b'y' => pad2((t.tm_year % 100).rem_euclid(100)),
                b'Y' => (t.tm_year + 1900).to_string().into_bytes(),
                b'z' => b"+0000".to_vec(),
                b'Z' => b"UTC".to_vec(),
                b'%' => b"%".to_vec(),
                b'c' => format!(
                    "{} {} {:2} {:02}:{:02}:{:02} {}",
                    String::from_utf8_lossy(
                        WDAY[(t.tm_wday as usize).min(6)]
                            .split(|&b| b == 0)
                            .next()
                            .unwrap()
                    ),
                    String::from_utf8_lossy(
                        MON[(t.tm_mon as usize).min(11)]
                            .split(|&b| b == 0)
                            .next()
                            .unwrap()
                    ),
                    t.tm_mday,
                    t.tm_hour,
                    t.tm_min,
                    t.tm_sec,
                    t.tm_year + 1900
                )
                .into_bytes(),
                _ => {
                    out.push(b'%');
                    out.push(c);
                    continue;
                }
            };
            if out.len() + piece.len() + 1 > max {
                break;
            }
            out.extend_from_slice(&piece);
        }
        if out.len() + 1 > max {
            return 0;
        }
        std::ptr::copy_nonoverlapping(out.as_ptr(), s.cast::<u8>(), out.len());
        *s.add(out.len()) = 0;
        out.len()
    }
}

fn iso_week_year(t: &tm) -> (i32, u32, u32) {
    // Simplified ISO week computation from the ordinal date.
    let wday = if t.tm_wday == 0 { 7 } else { t.tm_wday }; // 1..7 Mon..Sun
    let yday = t.tm_yday + 1;
    let week: i64 = (yday as i64 - wday as i64 + 10) / 7;
    let year: i64 = t.tm_year as i64 + 1900;
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let ydays: i64 = if leap { 366 } else { 365 };
    let (iso_year, iso_week) = if week < 1 {
        let prev = year - 1;
        let pleap = (prev % 4 == 0 && prev % 100 != 0) || prev % 400 == 0;
        (prev, if pleap { 53 } else { 52 })
    } else if week > 52 {
        let rem = ydays - yday as i64;
        if rem >= 4 - wday as i64 {
            (year + 1, 1)
        } else {
            (year, week)
        }
    } else {
        (year, week)
    };
    (iso_year as i32, iso_week as u32, wday as u32)
}

// ---------------------------------------------------------------------------
// locale — permanently "C"
// ---------------------------------------------------------------------------

pub const LC_CTYPE: c_int = 0;
pub const LC_NUMERIC: c_int = 1;
pub const LC_TIME: c_int = 2;
pub const LC_COLLATE: c_int = 3;
pub const LC_MONETARY: c_int = 4;
pub const LC_MESSAGES: c_int = 5;
pub const LC_ALL: c_int = 6;

#[repr(C)]
pub struct lconv {
    pub decimal_point: *mut c_char,
    pub thousands_sep: *mut c_char,
    pub grouping: *mut c_char,
    pub int_curr_symbol: *mut c_char,
    pub currency_symbol: *mut c_char,
    pub mon_decimal_point: *mut c_char,
    pub mon_thousands_sep: *mut c_char,
    pub mon_grouping: *mut c_char,
    pub positive_sign: *mut c_char,
    pub negative_sign: *mut c_char,
    pub int_frac_digits: c_char,
    pub frac_digits: c_char,
    pub p_cs_precedes: c_char,
    pub p_sep_by_space: c_char,
    pub n_cs_precedes: c_char,
    pub n_sep_by_space: c_char,
    pub p_sign_posn: c_char,
    pub n_sign_posn: c_char,
    pub int_p_cs_precedes: c_char,
    pub int_p_sep_by_space: c_char,
    pub int_n_cs_precedes: c_char,
    pub int_n_sep_by_space: c_char,
    pub int_p_sign_posn: c_char,
    pub int_n_sign_posn: c_char,
}

const C_DECIMAL_POINT: &[u8] = b".\0";
const EMPTY_STR: &[u8] = b"\0";

pub unsafe fn localeconv() -> *mut lconv {
    static mut C_LOCALE: lconv = lconv {
        decimal_point: C_DECIMAL_POINT.as_ptr() as *mut c_char,
        thousands_sep: EMPTY_STR.as_ptr() as *mut c_char,
        grouping: EMPTY_STR.as_ptr() as *mut c_char,
        int_curr_symbol: EMPTY_STR.as_ptr() as *mut c_char,
        currency_symbol: EMPTY_STR.as_ptr() as *mut c_char,
        mon_decimal_point: EMPTY_STR.as_ptr() as *mut c_char,
        mon_thousands_sep: EMPTY_STR.as_ptr() as *mut c_char,
        mon_grouping: EMPTY_STR.as_ptr() as *mut c_char,
        positive_sign: EMPTY_STR.as_ptr() as *mut c_char,
        negative_sign: EMPTY_STR.as_ptr() as *mut c_char,
        int_frac_digits: 127,
        frac_digits: 127,
        p_cs_precedes: 127,
        p_sep_by_space: 127,
        n_cs_precedes: 127,
        n_sep_by_space: 127,
        p_sign_posn: 127,
        n_sign_posn: 127,
        int_p_cs_precedes: 127,
        int_p_sep_by_space: 127,
        int_n_cs_precedes: 127,
        int_n_sep_by_space: 127,
        int_p_sign_posn: 127,
        int_n_sign_posn: 127,
    };
    std::ptr::addr_of_mut!(C_LOCALE)
}

const C_LOCALE_NAME: &[u8] = b"C\0";

pub unsafe fn setlocale(_category: c_int, _locale: *const c_char) -> *mut c_char {
    C_LOCALE_NAME.as_ptr() as *mut c_char
}

// ---------------------------------------------------------------------------
// signals / processes / resources — no OS; fixed answers or -1
// ---------------------------------------------------------------------------

pub const SIGHUP: c_int = 1;
pub const SIGINT: c_int = 2;
pub const SIGQUIT: c_int = 3;
pub const SIGILL: c_int = 4;
pub const SIGTRAP: c_int = 5;
pub const SIGABRT: c_int = 6;
pub const SIGBUS: c_int = 7;
pub const SIGFPE: c_int = 8;
pub const SIGKILL: c_int = 9;
pub const SIGUSR1: c_int = 10;
pub const SIGSEGV: c_int = 11;
pub const SIGUSR2: c_int = 12;
pub const SIGPIPE: c_int = 13;
pub const SIGALRM: c_int = 14;
pub const SIGTERM: c_int = 15;
pub const SIGCHLD: c_int = 17;
pub const SIGCONT: c_int = 18;
pub const SIGSTOP: c_int = 19;
pub const SIGTSTP: c_int = 20;
pub const SIGTTIN: c_int = 21;
pub const SIGTTOU: c_int = 22;
pub const SIGPROF: c_int = 27;
pub const SIGWINCH: c_int = 28;

pub const SIG_DFL: sighandler_t = 0;
pub const SIG_IGN: sighandler_t = 1;
pub const SIG_ERR: sighandler_t = !0;
pub const SIG_BLOCK: c_int = 0;
pub const SIG_UNBLOCK: c_int = 1;
pub const SIG_SETMASK: c_int = 2;
pub const SA_NOCLDSTOP: c_int = 1;
pub const SA_NOCLDWAIT: c_int = 2;
pub const SA_SIGINFO: c_int = 4;
pub const SA_ONSTACK: c_int = 0x08000000;
pub const SA_RESTART: c_int = 0x10000000;
pub const SA_NODEFER: c_int = 0x40000000;
pub const SA_RESETHAND: c_int = -2147483648;
pub const SA_RESTORER: c_int = 0x04000000;

pub type sigset_t = [u64; 16]; // glibc 128-byte layout

#[repr(C)]
pub struct sigaction {
    pub sa_sigaction: sighandler_t,
    pub sa_mask: sigset_t,
    pub sa_flags: c_int,
    pub sa_restorer: *mut c_void,
}

pub unsafe fn sigaction(_signum: c_int, _act: *const sigaction, _oldact: *mut sigaction) -> c_int {
    ERRNO.with(|e| e.set(EINVAL));
    -1
}

pub unsafe fn sigemptyset(set: *mut sigset_t) -> c_int {
    unsafe {
        if set.is_null() {
            return -1;
        }
        std::ptr::write_bytes(set.cast::<u8>(), 0, std::mem::size_of::<sigset_t>());
        0
    }
}

pub unsafe fn sigfillset(set: *mut sigset_t) -> c_int {
    unsafe {
        if set.is_null() {
            return -1;
        }
        std::ptr::write_bytes(set.cast::<u8>(), 0xff, std::mem::size_of::<sigset_t>());
        0
    }
}

pub unsafe fn sigaddset(set: *mut sigset_t, signum: c_int) -> c_int {
    unsafe {
        if set.is_null() || signum <= 0 || signum as usize > 64 * 8 {
            return -1;
        }
        let bit = (signum - 1) as usize;
        (*set)[bit / 64] |= 1u64 << (bit % 64);
        0
    }
}

pub unsafe fn sigdelset(set: *mut sigset_t, signum: c_int) -> c_int {
    unsafe {
        if set.is_null() || signum <= 0 || signum as usize > 64 * 8 {
            return -1;
        }
        let bit = (signum - 1) as usize;
        (*set)[bit / 64] &= !(1u64 << (bit % 64));
        0
    }
}

pub unsafe fn sigismember(set: *const sigset_t, signum: c_int) -> c_int {
    unsafe {
        if set.is_null() || signum <= 0 || signum as usize > 64 * 8 {
            return -1;
        }
        let bit = (signum - 1) as usize;
        ((*set)[bit / 64] >> (bit % 64) & 1) as c_int
    }
}

pub unsafe fn sigprocmask(_how: c_int, _set: *const sigset_t, _oldset: *mut sigset_t) -> c_int {
    -1
}

pub unsafe fn signal(_signum: c_int, _handler: sighandler_t) -> sighandler_t {
    SIG_ERR
}

pub unsafe fn kill(_pid: pid_t, _sig: c_int) -> c_int {
    ERRNO.with(|e| e.set(EPERM));
    -1
}

pub unsafe fn getpid() -> pid_t {
    1 // single "process" sandbox
}

pub unsafe fn getppid() -> pid_t {
    0
}

pub unsafe fn getuid() -> uid_t {
    0
}

pub unsafe fn geteuid() -> uid_t {
    0
}

pub unsafe fn getgid() -> gid_t {
    0
}

pub unsafe fn getegid() -> gid_t {
    0
}

pub unsafe fn getlogin() -> *mut c_char {
    std::ptr::null_mut()
}

pub unsafe fn gethostname(name: *mut c_char, len: size_t) -> c_int {
    unsafe {
        const HOST: &[u8] = b"localhost\0";
        if name.is_null() || len < HOST.len() {
            return -1;
        }
        std::ptr::copy_nonoverlapping(HOST.as_ptr(), name.cast::<u8>(), HOST.len());
        0
    }
}

pub fn sleep(_secs: c_uint) -> c_uint {
    0
}

pub fn usleep(_usecs: c_uint) -> c_int {
    0
}

pub const PRIO_PROCESS: c_int = 0;
pub const PRIO_PGRP: c_int = 1;
pub const PRIO_USER: c_int = 2;

pub unsafe fn getpriority(_which: c_int, _who: id_t) -> c_int {
    0
}

pub unsafe fn setpriority(_which: c_int, _who: id_t, _prio: c_int) -> c_int {
    -1
}

pub const RUSAGE_SELF: c_int = 0;
pub const RUSAGE_CHILDREN: c_int = -1;
pub const RUSAGE_THREAD: c_int = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct rusage {
    pub ru_utime: timeval,
    pub ru_stime: timeval,
    pub ru_maxrss: c_long,
    pub ru_ixrss: c_long,
    pub ru_idrss: c_long,
    pub ru_minrss: c_long,
    pub ru_majflt: c_long,
    pub ru_nswap: c_long,
    pub ru_inblock: c_long,
    pub ru_oublock: c_long,
    pub ru_msgsnd: c_long,
    pub ru_nsignals: c_long,
    pub ru_nvcsw: c_long,
    pub ru_nivcsw: c_long,
}

pub unsafe fn getrusage(_who: c_int, usage: *mut rusage) -> c_int {
    unsafe {
        if usage.is_null() {
            return -1;
        }
        std::ptr::write_bytes(usage.cast::<u8>(), 0, std::mem::size_of::<rusage>());
        // Report the wall clock as user time: the only clock wasm has.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        (*usage).ru_utime = timeval {
            tv_sec: now.as_secs() as time_t,
            tv_usec: now.subsec_micros() as suseconds_t,
        };
        (*usage).ru_maxrss = 0;
        0
    }
}

pub const RLIMIT_CPU: c_int = 0;
pub const RLIMIT_FSIZE: c_int = 1;
pub const RLIMIT_DATA: c_int = 2;
pub const RLIMIT_STACK: c_int = 3;
pub const RLIMIT_CORE: c_int = 4;
pub const RLIMIT_RSS: c_int = 5;
pub const RLIMIT_NPROC: c_int = 6;
pub const RLIMIT_NOFILE: c_int = 7;
pub const RLIMIT_AS: c_int = 9;
pub const RLIM_INFINITY: rlim_t = rlim_t::MAX;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct rlimit {
    pub rlim_cur: rlim_t,
    pub rlim_max: rlim_t,
}

pub unsafe fn getrlimit(_resource: c_int, rlim: *mut rlimit) -> c_int {
    unsafe {
        if rlim.is_null() {
            return -1;
        }
        (*rlim).rlim_cur = RLIM_INFINITY;
        (*rlim).rlim_max = RLIM_INFINITY;
        0
    }
}

pub unsafe fn setrlimit(_resource: c_int, _rlim: *const rlimit) -> c_int {
    -1
}

pub const ITIMER_REAL: c_int = 0;
pub const ITIMER_VIRTUAL: c_int = 1;
pub const ITIMER_PROF: c_int = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct itimerval {
    pub it_interval: timeval,
    pub it_value: timeval,
}

pub unsafe fn getitimer(_which: c_int, curr_value: *mut itimerval) -> c_int {
    unsafe {
        if curr_value.is_null() {
            return -1;
        }
        std::ptr::write_bytes(curr_value.cast::<u8>(), 0, std::mem::size_of::<itimerval>());
        0
    }
}

pub unsafe fn setitimer(
    _which: c_int,
    _new_value: *const itimerval,
    old_value: *mut itimerval,
) -> c_int {
    unsafe {
        if !old_value.is_null() {
            std::ptr::write_bytes(old_value.cast::<u8>(), 0, std::mem::size_of::<itimerval>());
        }
        -1 // R's profiler treats this as "cannot set timer"
    }
}

pub const _SC_CLK_TCK: c_int = 2;
pub const _SC_PAGE_SIZE: c_int = 30;
pub const _SC_PAGESIZE: c_int = 30;
pub const _SC_PHYS_PAGES: c_int = 85;
pub const _SC_NPROCESSORS_CONF: c_int = 83;
pub const _SC_NPROCESSORS_ONLN: c_int = 84;

pub fn sysconf(name: c_int) -> c_long {
    match name {
        _SC_CLK_TCK => 100,
        _SC_PAGE_SIZE => 65_536,
        _SC_NPROCESSORS_CONF | _SC_NPROCESSORS_ONLN => std::thread::available_parallelism()
            .map(|n| n.get() as c_long)
            .unwrap_or(1),
        _ => 0,
    }
}

pub const CTL_KERN: c_int = 1;

pub unsafe fn sysctl(
    _name: *mut c_int,
    _namelen: c_uint,
    _oldp: *mut c_void,
    _oldlenp: *mut size_t,
    _newp: *mut c_void,
    _newlen: size_t,
) -> c_int {
    ERRNO.with(|e| e.set(ENOSYS_VAL));
    -1
}

const ENOSYS_VAL: c_int = 38;

#[repr(C)]
pub struct utsname {
    pub sysname: [c_char; 65],
    pub nodename: [c_char; 65],
    pub release: [c_char; 65],
    pub version: [c_char; 65],
    pub machine: [c_char; 65],
    pub domainname: [c_char; 65],
}

pub unsafe fn uname(buf: *mut utsname) -> c_int {
    unsafe {
        if buf.is_null() {
            return -1;
        }
        fn fill(dst: *mut c_char, val: &[u8]) {
            unsafe {
                let n = val.len().min(64);
                std::ptr::copy_nonoverlapping(val.as_ptr(), dst.cast::<u8>(), n);
                *dst.add(n) = 0;
            }
        }
        fill((*buf).sysname.as_mut_ptr(), b"wasm32");
        fill((*buf).nodename.as_mut_ptr(), b"localhost");
        fill((*buf).release.as_mut_ptr(), b"unknown");
        fill((*buf).version.as_mut_ptr(), b"wasm32-unknown-unknown");
        fill((*buf).machine.as_mut_ptr(), b"wasm32");
        fill((*buf).domainname.as_mut_ptr(), b"");
        0
    }
}

#[repr(C)]
pub struct passwd {
    pub pw_name: *mut c_char,
    pub pw_passwd: *mut c_char,
    pub pw_uid: uid_t,
    pub pw_gid: gid_t,
    pub pw_gecos: *mut c_char,
    pub pw_dir: *mut c_char,
    pub pw_shell: *mut c_char,
}

pub unsafe fn getpwnam(_name: *const c_char) -> *mut passwd {
    std::ptr::null_mut()
}

pub unsafe fn getpwuid(_uid: uid_t) -> *mut passwd {
    std::ptr::null_mut()
}

pub unsafe fn waitpid(_pid: pid_t, _status: *mut c_int, _options: c_int) -> pid_t {
    -1
}

// ---------------------------------------------------------------------------
// fds / files — no filesystem descriptors
// ---------------------------------------------------------------------------

pub const STDIN_FILENO: c_int = 0;
pub const STDOUT_FILENO: c_int = 1;
pub const STDERR_FILENO: c_int = 2;

pub const O_RDONLY: c_int = 0;
pub const O_WRONLY: c_int = 1;
pub const O_RDWR: c_int = 2;
pub const O_CREAT: c_int = 0o100;
pub const O_EXCL: c_int = 0o200;
pub const O_TRUNC: c_int = 0o1000;
pub const O_APPEND: c_int = 0o2000;
pub const O_NONBLOCK: c_int = 0o4000;

pub const F_GETFD: c_int = 1;
pub const F_SETFD: c_int = 2;
pub const F_GETFL: c_int = 3;
pub const F_SETFL: c_int = 4;
pub const FD_CLOEXEC: c_int = 1;

pub const S_IRUSR: mode_t = 0o400;
pub const S_IWUSR: mode_t = 0o200;
pub const S_IXUSR: mode_t = 0o100;
pub const S_IRGRP: mode_t = 0o040;
pub const S_IWGRP: mode_t = 0o020;
pub const S_IXGRP: mode_t = 0o010;
pub const S_IROTH: mode_t = 0o004;
pub const S_IWOTH: mode_t = 0o002;
pub const S_IXOTH: mode_t = 0o001;

pub unsafe fn open(_path: *const c_char, _oflag: c_int) -> c_int {
    ERRNO.with(|e| e.set(ENOENT));
    -1
}

pub unsafe fn close(_fd: c_int) -> c_int {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn read(_fd: c_int, _buf: *mut c_void, _count: size_t) -> ssize_t {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn write(_fd: c_int, _buf: *const c_void, _count: size_t) -> ssize_t {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn fcntl(_fd: c_int, _cmd: c_int, _arg: c_int) -> c_int {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn isatty(_fd: c_int) -> c_int {
    0
}

pub unsafe fn chdir(_path: *const c_char) -> c_int {
    -1
}

pub unsafe fn getcwd(buf: *mut c_char, size: size_t) -> *mut c_char {
    unsafe {
        const CWD: &[u8] = b"/\0";
        if buf.is_null() || size < CWD.len() as size_t {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(CWD.as_ptr(), buf.cast::<u8>(), CWD.len());
        buf
    }
}

// ---------------------------------------------------------------------------
// sockets — no network: constants real, operations fail
// ---------------------------------------------------------------------------

pub const AF_UNSPEC: c_int = 0;
pub const AF_INET: c_int = 2;
pub const AF_INET6: c_int = 10;
pub const PF_INET: c_int = AF_INET;
pub const PF_UNSPEC: c_int = AF_UNSPEC;
pub const SOCK_STREAM: c_int = 1;
pub const SOCK_DGRAM: c_int = 2;
pub const SOCK_RAW: c_int = 3;
pub const SOL_SOCKET: c_int = 1;
pub const SO_DEBUG: c_int = 1;
pub const SO_REUSEADDR: c_int = 2;
pub const SO_TYPE: c_int = 3;
pub const SO_ERROR: c_int = 4;
pub const SO_DONTROUTE: c_int = 5;
pub const SO_BROADCAST: c_int = 6;
pub const SO_SNDBUF: c_int = 7;
pub const SO_RCVBUF: c_int = 8;
pub const SO_KEEPALIVE: c_int = 9;
pub const SO_OOBINLINE: c_int = 10;
pub const SO_LINGER: c_int = 13;
pub const SO_RCVTIMEO: c_int = 20;
pub const SO_SNDTIMEO: c_int = 21;
pub const IPPROTO_IP: c_int = 0;
pub const IPPROTO_TCP: c_int = 6;
pub const IPPROTO_UDP: c_int = 17;
pub const TCP_NODELAY: c_int = 1;
pub const SOMAXCONN: c_int = 128;
pub const INADDR_ANY: in_addr_t = 0;
pub const INADDR_LOOPBACK: in_addr_t = 0x7f000001;
pub const INADDR_BROADCAST: in_addr_t = 0xffffffff;
pub const INADDR_NONE: in_addr_t = 0xffffffff;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct in_addr {
    pub s_addr: in_addr_t,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct sockaddr {
    pub sa_family: sa_family_t,
    pub sa_data: [u8; 14],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct sockaddr_in {
    pub sin_family: sa_family_t,
    pub sin_port: in_port_t,
    pub sin_addr: in_addr,
    pub sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct addrinfo {
    pub ai_flags: c_int,
    pub ai_family: c_int,
    pub ai_socktype: c_int,
    pub ai_protocol: c_int,
    pub ai_addrlen: socklen_t,
    pub ai_canonname: *mut c_char,
    pub ai_addr: *mut sockaddr,
    pub ai_next: *mut addrinfo,
}

pub const AI_PASSIVE: c_int = 0x0001;
pub const AI_CANONNAME: c_int = 0x0002;
pub const AI_NUMERICHOST: c_int = 0x0004;
pub const AI_NUMERICSERV: c_int = 0x0008;
pub const NI_NUMERICHOST: c_int = 1;
pub const NI_NUMERICSERV: c_int = 2;
pub const NI_NAMEREQD: c_int = 8;

#[repr(C)]
pub struct hostent {
    pub h_name: *mut c_char,
    pub h_aliases: *mut *mut c_char,
    pub h_addrtype: c_int,
    pub h_length: c_int,
    pub h_addr_list: *mut *mut c_char,
}

pub const FD_SETSIZE: c_int = 1024;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct fd_set {
    pub fds_bits: [u32; 32],
}

impl Default for fd_set {
    fn default() -> Self {
        fd_set { fds_bits: [0; 32] }
    }
}

pub unsafe fn FD_ZERO(set: *mut fd_set) {
    unsafe {
        if !set.is_null() {
            (*set).fds_bits = [0; 32];
        }
    }
}

pub unsafe fn FD_SET(fd: c_int, set: *mut fd_set) {
    unsafe {
        if set.is_null() || fd < 0 || fd >= FD_SETSIZE {
            return;
        }
        (*set).fds_bits[fd as usize / 32] |= 1u32 << (fd as usize % 32);
    }
}

pub unsafe fn FD_CLR(fd: c_int, set: *mut fd_set) {
    unsafe {
        if set.is_null() || fd < 0 || fd >= FD_SETSIZE {
            return;
        }
        (*set).fds_bits[fd as usize / 32] &= !(1u32 << (fd as usize % 32));
    }
}

pub unsafe fn FD_ISSET(fd: c_int, set: *const fd_set) -> bool {
    unsafe {
        if set.is_null() || fd < 0 || fd >= FD_SETSIZE {
            return false;
        }
        (((*set).fds_bits[fd as usize / 32] >> (fd as usize % 32)) & 1) != 0
    }
}

pub fn htons(host: u16) -> u16 {
    host.swap_bytes()
}

pub fn htonl(host: u32) -> u32 {
    host.swap_bytes()
}

pub fn ntohs(host: u16) -> u16 {
    host.swap_bytes()
}

pub fn ntohl(host: u32) -> u32 {
    host.swap_bytes()
}

pub unsafe fn socket(_domain: c_int, _ty: c_int, _protocol: c_int) -> c_int {
    ERRNO.with(|e| e.set(EPROTONOSUPPORT_VAL));
    -1
}

const EPROTONOSUPPORT_VAL: c_int = 93;

pub unsafe fn bind(_sockfd: c_int, _addr: *const sockaddr, _addrlen: socklen_t) -> c_int {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn listen(_sockfd: c_int, _backlog: c_int) -> c_int {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn accept(_sockfd: c_int, _addr: *mut sockaddr, _addrlen: *mut socklen_t) -> c_int {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn connect(_sockfd: c_int, _addr: *const sockaddr, _addrlen: socklen_t) -> c_int {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn shutdown(_sockfd: c_int, _how: c_int) -> c_int {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn recv(_sockfd: c_int, _buf: *mut c_void, _len: size_t, _flags: c_int) -> ssize_t {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn send(_sockfd: c_int, _buf: *const c_void, _len: size_t, _flags: c_int) -> ssize_t {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn setsockopt(
    _sockfd: c_int,
    _level: c_int,
    _optname: c_int,
    _optval: *const c_void,
    _optlen: socklen_t,
) -> c_int {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn getsockopt(
    _sockfd: c_int,
    _level: c_int,
    _optname: c_int,
    _optval: *mut c_void,
    _optlen: *mut socklen_t,
) -> c_int {
    ERRNO.with(|e| e.set(EBADF));
    -1
}

pub unsafe fn select(
    _nfds: c_int,
    _readfds: *mut fd_set,
    _writefds: *mut fd_set,
    _exceptfds: *mut fd_set,
    _timeout: *mut timeval,
) -> c_int {
    ERRNO.with(|e| e.set(EINVAL));
    -1
}

pub unsafe fn getaddrinfo(
    _node: *const c_char,
    _service: *const c_char,
    _hints: *const addrinfo,
    _res: *mut *mut addrinfo,
) -> c_int {
    EAI_NONAME // no resolver on wasm
}

pub unsafe fn freeaddrinfo(_ai: *mut addrinfo) {}

pub unsafe fn getnameinfo(
    _sa: *const sockaddr,
    _salen: socklen_t,
    _host: *mut c_char,
    _hostlen: socklen_t,
    _serv: *mut c_char,
    _servlen: socklen_t,
    _flags: c_int,
) -> c_int {
    EAI_NONAME
}

pub unsafe fn gethostbyname(_name: *const c_char) -> *mut hostent {
    std::ptr::null_mut()
}

pub unsafe fn inet_addr(_cp: *const c_char) -> in_addr_t {
    INADDR_NONE
}

// ---------------------------------------------------------------------------
// printf engine (typed args; the rport_snprintf! macro feeds it on wasm)
// ---------------------------------------------------------------------------

pub mod printf;
pub use printf::{CArg, snprintf_args};

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(fmt: &str, args: &[CArg]) -> String {
        let mut buf = vec![0i8; 256];
        let n = unsafe {
            snprintf_args(
                buf.as_mut_ptr(),
                buf.len(),
                format!("{fmt}\0").as_ptr() as *const c_char,
                args,
            )
        };
        let end = buf.iter().position(|&c| c == 0).unwrap_or(n as usize);
        String::from_utf8_lossy(unsafe {
            std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), end)
        })
        .into_owned()
    }

    #[test]
    fn strings_and_ints() {
        assert_eq!(fmt("[[%%d]] %d", &[CArg::Int(3)]), "[[%d]] 3");
        assert_eq!(
            fmt(
                "%s=%05d",
                &[CArg::Str(b"n\0".as_ptr() as *const c_char), CArg::Int(42)]
            ),
            "n=00042"
        );
        assert_eq!(
            fmt(
                "%x %X %o %u",
                &[
                    CArg::UInt(255),
                    CArg::UInt(255),
                    CArg::UInt(8),
                    CArg::UInt(7)
                ]
            ),
            "ff FF 10 7"
        );
        assert_eq!(fmt("%c%c", &[CArg::Char(b'a'), CArg::Char(b'b')]), "ab");
    }

    #[test]
    fn floats() {
        assert_eq!(fmt("%f", &[CArg::Double(1.5)]), "1.500000");
        assert_eq!(fmt("%.2f", &[CArg::Double(3.14159)]), "3.14");
        assert_eq!(fmt("%e", &[CArg::Double(12345.0)]), "1.234500e+04");
        assert_eq!(fmt("%g", &[CArg::Double(0.0001)]), "0.0001");
        assert_eq!(fmt("%g", &[CArg::Double(1234567.0)]), "1.23457e+06");
        assert_eq!(fmt("%.3g", &[CArg::Double(0.5)]), "0.5");
    }

    #[test]
    fn longs_and_widths() {
        assert_eq!(fmt("%ld", &[CArg::Long(-5)]), "-5");
        assert_eq!(fmt("%lu", &[CArg::ULong(5_000_000_000)]), "5000000000");
        assert_eq!(
            fmt(
                "%8.3s|",
                &[CArg::Str(b"abcdef\0".as_ptr() as *const c_char)]
            ),
            "     abc|"
        );
        assert_eq!(fmt("%-8d|", &[CArg::Int(3)]), "3       |");
        assert_eq!(fmt("%+d", &[CArg::Int(3)]), "+3");
    }

    #[test]
    fn truncation_and_ret() {
        let mut buf = [0i8; 5];
        let n = unsafe {
            snprintf_args(
                buf.as_mut_ptr(),
                5,
                b"abcdefgh\0".as_ptr() as *const c_char,
                &[],
            )
        };
        assert_eq!(n, 8); // return is the untruncated length, like C
        let end = buf.iter().position(|&c| c == 0).unwrap();
        assert_eq!(
            &buf[..end],
            b"abcd".iter().copied().map(|b| b as i8).collect::<Vec<_>>()
        );
    }
}
