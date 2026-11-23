// Port of R's modules/internet/internet.c (737 lines)
// Internet connection interface - download.file(), url(), socket connections
// Unix implementation with real file:// download and raw socket HTTP GET.
// Windows-specific functions (wininet) remain as stubs.

use crate::mainutils::rfile::open_file;
use core::ffi::{c_char, c_double, c_int, c_void};
use std::ffi::{CStr, CString};
use std::io::{ErrorKind, Read, Write as IoWrite};
use std::net::TcpStream;
#[cfg(not(target_arch = "wasm32"))]
use std::os::unix::io::IntoRawFd;
use std::path::Path;
use std::ptr;

use crate::main::errors::{Rf_error, Rf_error_cstr};
use crate::sexp::accessors::{CAR, CDR, CHAR, INTEGER, LENGTH, STRING_ELT, TYPEOF};
use crate::sexp::constructors::Rf_ScalarInteger;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::*;

// DLsize_t type alias (matches C: typedef int_fast64_t DLsize_t)
type DLsize_t = i64;

// Constants matching R source
const CPBUFSIZE: usize = 65536;
const IBUFSIZE: usize = 4096;

/// Per-session internet module state.
pub(crate) struct InternetRuntimeState {
    pub(crate) quiet: c_int,
    pub(crate) sock_inited: c_int,
}

impl Default for InternetRuntimeState {
    fn default() -> Self {
        Self {
            quiet: 1,
            sock_inited: 0,
        }
    }
}

/// Single seam for session-owned internet module state: the one justified
/// instance-field deref lives here instead of per call site.
pub(crate) fn with_internet_state<R>(f: impl FnOnce(&mut InternetRuntimeState) -> R) -> R {
    with_required_current_instance(|instance| f(unsafe { &mut (*instance).internet_state }))
}

fn internet_quiet() -> c_int {
    with_internet_state(|state| state.quiet)
}

fn set_internet_quiet(value: c_int) {
    with_internet_state(|state| state.quiet = value);
}

// SEXP type constants
const NA_LOGICAL: c_int = -2147483648; // NA_INTEGER value

// Helper: check if SEXP is a string vector with at least 1 element
#[inline]
fn is_single_string(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::STRSXP && LENGTH(x) >= 1
}

// Helper: check if SEXP is a string vector of exactly length 1
#[inline]
fn is_string_len1(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::STRSXP && LENGTH(x) == 1
}

// Helper: convert a logical SEXP to c_int (Rboolean-like)
// Returns 0 for FALSE, 1 for TRUE, NA_LOGICAL for NA
#[inline]
fn asRbool(x: SEXP) -> c_int {
    unsafe {
        if x == R_NilValue() {
            return NA_LOGICAL;
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::LGLSXP {
            return NA_LOGICAL;
        }
        let v = INTEGER(x);
        if v.is_null() {
            return NA_LOGICAL;
        }
        *v
    }
}

// Helper: convert a logical SEXP to asLogical-style result
#[inline]
fn asLogical_val(x: SEXP) -> c_int {
    asRbool(x)
}

// Helper: safe CStr to Rust &str
#[inline]
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn cstr_to_str<'a>(s: *const c_char) -> &'a str {
    unsafe {
        if s.is_null() {
            return "";
        }
        CStr::from_ptr(s).to_str().unwrap_or("")
    }
}

// =========================================================================
// Progress reporting functions
// =========================================================================

/// putdots - print download progress dots to stderr
/// One dot per KB downloaded, newline every 50, space every 10
fn putdots(old_value: &mut DLsize_t, new_val: DLsize_t) {
    let old = *old_value;
    *old_value = new_val;
    let mut i = old;
    while i < new_val {
        eprint!(".");
        let pos = i + 1;
        if pos % 50 == 0 {
            eprintln!();
        } else if pos % 10 == 0 {
            eprint!(" ");
        }
        i += 1;
    }
}

/// putdashes - print download progress dashes to stderr
/// Dashes represent progress bar (up to 50 chars)
fn putdashes(old_value: &mut c_int, new_val: c_int) {
    let old = *old_value;
    *old_value = new_val;
    let mut i = old;
    while i < new_val {
        eprint!("=");
        i += 1;
    }
}

// =========================================================================
// Unix implementation
// =========================================================================

// =========================================================================
// HTTP download via raw sockets (Unix implementation)
// =========================================================================

/// inetconn - context for an internet connection (matches C struct)
/// Used internally to track download state.
#[repr(C)]
struct inetconn {
    /// Content length (-1 if unknown)
    length: DLsize_t,
    /// Content type string (allocated, must be freed)
    content_type: *mut c_char,
    /// Socket file descriptor (Unix) or handle (Windows)
    fd: c_int,
}

/// http_parse_url - parse a URL into host, port, and path components.
/// Returns (host, port, path) as CString/String tuples.
/// Returns None if the URL cannot be parsed.
fn http_parse_url(url: &str) -> Option<(CString, u16, String)> {
    let url = url
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    // Split off the path
    let (hostport, path) = match url.find('/') {
        Some(idx) => (&url[..idx], &url[idx..]),
        None => (url, "/"),
    };

    // Split host and port
    let (host, port) = if hostport.starts_with('[') {
        // IPv6 literal [::1]:port
        let end_bracket = hostport.find(']')?;
        let host_part = &hostport[1..end_bracket];
        let rest = &hostport[end_bracket + 1..];
        let port_num = if let Some(stripped) = rest.strip_prefix(':') {
            stripped.parse::<u16>().ok()?
        } else {
            80
        };
        (host_part, port_num)
    } else {
        match hostport.rfind(':') {
            Some(idx) => {
                let port_str = &hostport[idx + 1..];
                if port_str.parse::<u16>().is_ok() {
                    (
                        &hostport[..idx],
                        port_str.parse::<u16>().unwrap_or_default(),
                    )
                } else {
                    (hostport, 80)
                }
            }
            None => (hostport, 80),
        }
    };

    Some((CString::new(host).ok()?, port, path.to_string()))
}

/// http_open - open an HTTP connection to a URL using raw sockets.
/// Sends an HTTP GET request and reads the response headers.
/// Returns a boxed inetconn on success, null on failure.
///
/// This is the Unix equivalent of the Windows wininet-based in_R_HTTPOpen2.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn http_open(
    url: *const c_char,
    agent: *const c_char,
    headers: *const c_char,
    _cacheOK: c_int,
) -> *mut c_void {
    // SAFETY: raw pointers are forwarded unchanged under this function's own `# Safety` contract.
    let url_str = match unsafe { cstr_to_str(url) } {
        "" => return ptr::null_mut(),
        s => s,
    };

    // Only handle http:// (not https:// — that requires TLS)
    if !url_str.starts_with("http://") {
        return ptr::null_mut();
    }

    let (host_cstr, port, path) = match http_parse_url(url_str) {
        Some(parts) => parts,
        None => return ptr::null_mut(),
    };

    let agent_str = if agent.is_null() {
        "RMath-Rust/1.0"
    } else {
        // SAFETY: raw pointers are forwarded unchanged under this function's own `# Safety` contract.
        unsafe { cstr_to_str(agent) }
    };

    // Open TCP connection
    let addr = format!("{}:{}", host_cstr.to_str().unwrap_or(""), port);
    let mut stream = match TcpStream::connect(&*addr) {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    // Set read timeout to avoid hanging forever
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));

    // Build HTTP GET request
    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nConnection: close\r\n",
        path,
        host_cstr.to_str().unwrap_or(""),
        agent_str
    );

    // Add custom headers if provided
    if !headers.is_null() {
        // SAFETY: raw pointers are forwarded unchanged under this function's own `# Safety` contract.
        let headers_str = unsafe { cstr_to_str(headers) };
        if !headers_str.is_empty() {
            // Split on newlines and add each header
            for line in headers_str.split('\n') {
                let line = line.trim();
                if !line.is_empty() {
                    request.push_str(line);
                    request.push_str("\r\n");
                }
            }
        }
    }

    request.push_str("\r\n");

    // Send request
    if stream.write_all(request.as_bytes()).is_err() {
        return ptr::null_mut();
    }

    // Read response - we need to parse headers to find content length
    let mut response_buf = Vec::with_capacity(IBUFSIZE * 4);
    let mut temp_buf = [0u8; IBUFSIZE];

    // Read until we find the end of headers (\r\n\r\n)
    let mut header_end = None;
    let mut total_read = 0usize;
    let max_header_size = 64 * 1024; // 64KB max header size

    while total_read < max_header_size {
        match stream.read(&mut temp_buf) {
            Ok(0) => break, // EOF
            Ok(n) => {
                response_buf.extend_from_slice(&temp_buf[..n]);
                total_read += n;

                // Check for end of headers
                if let Some(pos) = find_header_end(&response_buf) {
                    header_end = Some(pos);
                    break;
                }
            }
            Err(_) => return ptr::null_mut(),
        }
    }

    let header_end_pos = match header_end {
        Some(pos) => pos,
        None => return ptr::null_mut(),
    };

    // Parse status code
    let header_str = String::from_utf8_lossy(&response_buf[..header_end_pos]);
    let status_code = parse_status_code(&header_str);
    if status_code.is_none()
        || status_code.unwrap_or_default() < 200
        || status_code.unwrap_or_default() >= 300
    {
        // Non-2xx status
        return ptr::null_mut();
    }

    // Parse content length
    let content_length = parse_content_length(&header_str);

    // Parse content type
    let content_type_cstr =
        CString::new(parse_content_type(&header_str).unwrap_or_else(|| "unknown".to_string()))
            .unwrap_or_default();

    // Report content info if not quiet
    if internet_quiet() == 0 {
        eprint!(
            "Content type '{}'",
            content_type_cstr.to_str().unwrap_or("unknown")
        );
        if let Some(len) = content_length {
            if len > 1024 * 1024 {
                eprintln!(
                    " length {} bytes ({:.1} MB)",
                    len,
                    len as f64 / 1024.0 / 1024.0
                );
            } else if len > 10240 {
                eprintln!(" length {} bytes ({} KB)", len, len / 1024);
            } else {
                eprintln!(" length {} bytes", len);
            }
        } else {
            eprintln!(" length unknown");
        }
    }

    // Convert the TcpStream into a raw file descriptor that we own.
    // wasm32 has no file descriptors (no sockets at all); http_open is
    // unreachable there because every connect fails beforehand.
    #[cfg(target_arch = "wasm32")]
    let raw_fd: c_int = -1;
    #[cfg(not(target_arch = "wasm32"))]
    let raw_fd = stream.into_raw_fd();

    // Allocate the inetconn context
    let ctx = Box::new(inetconn {
        length: content_length.unwrap_or(-1),
        content_type: content_type_cstr.into_raw(),
        fd: raw_fd,
    });

    Box::into_raw(ctx) as *mut c_void
}

/// http_read - read data from an open HTTP connection.
/// Returns number of bytes read, 0 on EOF, -1 on error.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn http_read(ctx: *mut c_void, dest: *mut u8, len: usize) -> isize {
    unsafe {
        let conn = ctx as *mut inetconn;
        if conn.is_null() || (*conn).fd < 0 {
            return -1;
        }

        let fd = (*conn).fd;
        let nread = libc::read(fd, dest as *mut c_void, len);
        nread
    }
}

/// http_close - close an open HTTP connection and free resources.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn http_close(ctx: *mut c_void) {
    unsafe {
        let conn = ctx as *mut inetconn;
        if conn.is_null() {
            return;
        }

        // Close socket
        if (*conn).fd >= 0 {
            let _ = libc::close((*conn).fd);
        }

        // Free content type string
        if !(*conn).content_type.is_null() {
            let _ = CString::from_raw((*conn).content_type);
        }

        // Free the context
        drop(Box::from_raw(conn));
    }
}

/// find_header_end - find the position of \r\n\r\n in a byte buffer
fn find_header_end(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    (0..buf.len() - 3).find(|&i| {
        buf[i] == b'\r' && buf[i + 1] == b'\n' && buf[i + 2] == b'\r' && buf[i + 3] == b'\n'
    })
}

/// parse_status_code - extract HTTP status code from response headers
fn parse_status_code(headers: &str) -> Option<i32> {
    let first_line = headers.lines().next()?;
    // "HTTP/1.1 200 OK" -> extract 200
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() >= 2 {
        parts[1].parse::<i32>().ok()
    } else {
        None
    }
}

/// parse_content_length - extract Content-Length from response headers
fn parse_content_length(headers: &str) -> Option<i64> {
    for line in headers.lines() {
        let line = line.trim();
        if line.to_lowercase().starts_with("content-length:") {
            let val_str = line[15..].trim();
            return val_str.parse::<i64>().ok();
        }
    }
    None
}

/// parse_content_type - extract Content-Type from response headers
fn parse_content_type(headers: &str) -> Option<String> {
    for line in headers.lines() {
        let line = line.trim();
        if line.to_lowercase().starts_with("content-type:") {
            let val = line[13..].trim().to_string();
            // Strip trailing parameters like "; charset=utf-8"
            if let Some(idx) = val.find(';') {
                return Some(val[..idx].trim().to_string());
            }
            return Some(val);
        }
    }
    None
}

// =========================================================================
// file:// download implementation
// =========================================================================

fn download_error(message: String) -> ! {
    let message = CString::new(message).unwrap_or_default();
    Rf_error_cstr(&message);
    unreachable!("Rf_error_cstr always unwinds")
}

fn file_download(url: &str, file: &str, mode: &[u8]) -> c_int {
    let path = url.strip_prefix("file://").unwrap_or(url);
    // All file streams are byte mode in this port.
    let mut src = open_file(Path::new(path), b"r").unwrap_or_else(|error| {
        download_error(format!("cannot open URL '{}', reason '{}'", url, error))
    });
    let mut dst = open_file(Path::new(file), mode).unwrap_or_else(|error| {
        download_error(format!("cannot open destfile '{}', reason '{}'", file, error))
    });
    let mut buf = vec![0; CPBUFSIZE];
    loop {
        match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if dst.write_all(&buf[..n]).is_err() {
                    download_error("write failed".to_owned());
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break, // preserve legacy read failure behavior
        }
    }
    0
}

// =========================================================================
// HTTP download via raw sockets
// =========================================================================

/// http_download - download a file from an HTTP URL using raw sockets.
/// This is the Unix implementation of the http:// branch in in_do_download.
///
/// Returns 0 on success, 1 on failure.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn http_download(
    url: *const c_char,
    file: *const c_char,
    mode: *const c_char,
    quiet: c_int,
    headers: SEXP,
) -> c_int {
    unsafe {
        let url_str = cstr_to_str(url);
        let file_str = cstr_to_str(file);
        let mode_str = cstr_to_str(mode);

        // Open dest file
        let mode_bytes = if mode.is_null() {
            &[][..]
        } else {
            CStr::from_ptr(mode).to_bytes()
        };
        let mut dst_file = open_file(Path::new(file_str), mode_bytes).unwrap_or_else(|error| {
            download_error(format!(
                "cannot open destfile '{}', reason '{}'",
                file_str, error
            ))
        });

        // Report URL being tried
        if quiet == 0 {
            eprintln!("trying URL '{}'", url_str);
        }

        // Build headers string from SEXP if provided
        let headers_cstr: Option<CString> = if headers != R_NilValue()
            && TYPEOF(headers) == SEXPTYPE::STRSXP
            && LENGTH(headers) > 0
        {
            let h = CHAR(STRING_ELT(headers, 0));
            if !h.is_null() {
                Some(CString::from(CStr::from_ptr(h)))
            } else {
                None
            }
        } else {
            None
        };

        let headers_ptr = match &headers_cstr {
            Some(s) => s.as_ptr(),
            None => ptr::null(),
        };

        // Open HTTP connection
        let ctxt = http_open(url, ptr::null(), headers_ptr, 1);
        if ctxt.is_null() {
            drop(dst_file);
            // Check if mode contains 'w' to clean up partial file
            if mode_str.contains('w') {
                let _ = std::fs::remove_file(file_str);
            }
            download_error(format!("cannot open URL '{}'", url_str));
        }

        // Get content length from context
        let conn = ctxt as *const inetconn;
        let total = (*conn).length;
        let mut guess = if total > 0 { total } else { 100 * 1024 };

        let mut nbytes: DLsize_t = 0;
        let mut ndots: DLsize_t = 0;
        let mut ndashes: c_int = 0;
        let mut buf = [0u8; IBUFSIZE];

        // Read loop
        loop {
            let nread = http_read(ctxt, buf.as_mut_ptr(), IBUFSIZE);
            if nread <= 0 {
                break;
            }

            if dst_file.write_all(&buf[..nread as usize]).is_err() {
                http_close(ctxt);
                download_error("write failed".to_owned());
            }

            nbytes += nread as DLsize_t;

            // Progress reporting
            if quiet == 0 {
                if guess <= 0 {
                    putdots(&mut ndots, nbytes / 1024);
                } else {
                    // Progress bar: 50 chars wide
                    let dashes = (50 * nbytes / guess) as c_int;
                    putdashes(&mut ndashes, dashes);
                }
            }
        }

        http_close(ctxt);

        // Print completion summary
        if quiet == 0 {
            if guess > 0 {
                eprintln!(); // newline after progress bar
            }
            if nbytes > 1024 * 1024 {
                eprintln!("downloaded {:.1} MB\n", nbytes as f64 / 1024.0 / 1024.0);
            } else if nbytes > 10240 {
                eprintln!("downloaded {} KB\n", (nbytes / 1024) as i64);
            } else {
                eprintln!("downloaded {} bytes\n", nbytes);
            }
        }

        drop(dst_file);

        // Warn if downloaded length doesn't match reported length
        if total > 0 && total != nbytes {
            let msg = format!(
                "downloaded length {:.0} != reported length {:.0}",
                nbytes as f64, total as f64
            );
            let c_msg = CString::new(msg).unwrap_or_default();
            crate::main::errors::Rf_warning_cstr(&c_msg);
        }

        0
    }
}

// =========================================================================
// Unix implementation
// =========================================================================

/// in_do_download - download.file() internal implementation.
/// Signature: SEXP in_do_download(SEXP args)
/// Expects args: url, destfile, quiet, mode, headers, cacheOK [, method]
///
/// Supports:
///   file:// URLs — direct file copy (matches R behavior)
///   http:// URLs — raw socket HTTP GET (Unix implementation, replaces defunct "internal" method)
///   https:// URLs — error (requires TLS, use libcurl method instead)
///   ftp:// URLs — error (defunct in R 4.2+)
///
/// Returns: ScalarInteger with status code (0 = success, 1 = failure)
pub(crate) fn in_do_download(args: SEXP) -> SEXP {
    unsafe {
        let mut args = args;

        // url
        let scmd = CAR(args);
        args = CDR(args);
        if !is_single_string(scmd) {
            let msg = c"invalid 'url' argument";
            Rf_error_cstr(&msg);
        }
        let url = CHAR(STRING_ELT(scmd, 0));

        // destfile
        let sfile = CAR(args);
        args = CDR(args);
        if !is_single_string(sfile) {
            let msg = c"invalid 'destfile' argument";
            Rf_error_cstr(&msg);
        }
        // Use translateChar for destfile (may have encoded paths)
        let file = crate::sexp::accessors::translateChar(STRING_ELT(sfile, 0));

        // quiet
        let squiet = CAR(args);
        args = CDR(args);
        let quiet = asRbool(squiet);
        if quiet == NA_LOGICAL {
            let msg = c"invalid 'quiet' argument";
            Rf_error_cstr(&msg);
        }
        set_internet_quiet(quiet);

        // mode
        let smode = CAR(args);
        args = CDR(args);
        if !is_string_len1(smode) {
            let msg = c"invalid 'mode' argument";
            Rf_error_cstr(&msg);
        }
        let mode = CHAR(STRING_ELT(smode, 0));

        // cacheOK
        let scacheOK = CAR(args);
        args = CDR(args);
        let cacheOK = asLogical_val(scacheOK);
        if cacheOK == NA_LOGICAL {
            let msg = c"invalid 'cacheOK' argument";
            Rf_error_cstr(&msg);
        }
        let _ = cacheOK; // Used only by Windows wininet code path

        // Check if file:// URL
        let url_rust = cstr_to_str(url);
        let file_url = url_rust.starts_with("file://");

        // headers (remaining arg before optional method)
        let sheaders = CAR(args);

        // method (optional, only present on Windows in R's code; we check for it)
        // In R's C code, meth = asLogical(CADR(args)) is only under #ifdef Win32
        // On Unix, there is no method argument in the args list.

        let mut status: c_int = 0;

        if file_url {
            // ---- file:// download ----
            let mode_bytes = if mode.is_null() {
                &[][..]
            } else {
                CStr::from_ptr(mode).to_bytes()
            };
            status = file_download(url_rust, cstr_to_str(file), mode_bytes);
        } else if url_rust.starts_with("http://") {
            // ---- http:// download via raw sockets ----
            // R 4.2+ made the "internal" method defunct for HTTP, but since we
            // provide a real socket implementation, we use it here as the
            // Unix-native HTTP download path.
            status = http_download(url, file, mode, quiet, sheaders);
        } else if url_rust.starts_with("https://") {
            // HTTPS requires TLS — not supported by raw sockets
            let msg = CString::new(format!(
                "scheme not supported in URL '{}' (use method=\"libcurl\" for https://)",
                url_rust
            ))
            .unwrap_or_default();
            Rf_error_cstr(&msg);
        } else if url_rust.starts_with("ftp://") {
            // FTP is defunct in R 4.2+
            let msg = c"the 'internal' method for ftp:// URLs is defunct";
            Rf_error_cstr(&msg);
        } else {
            let msg = CString::new(format!("scheme not supported in URL '{}'", url_rust))
                .unwrap_or_default();
            Rf_error_cstr(&msg);
        }

        Rf_ScalarInteger(status)
    }
}

// =========================================================================
// Module initialization
// =========================================================================

/// Compatibility initializer. Rust internet entry points do not use GNU R's
/// `R_InternetRoutines` registration table.
pub(crate) fn R_init_internet(_info: *mut c_void) {}

#[cfg(test)]
mod tests {
    use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};

    use super::*;

    #[test]
    fn quiet_flag_is_session_local() {
        let mut first = RInstance::new();
        unsafe {
            set_current_instance(&mut first);
        }
        assert_eq!(internet_quiet(), 1);
        set_internet_quiet(0);
        assert_eq!(internet_quiet(), 0);

        let mut second = RInstance::new();
        unsafe {
            set_current_instance(&mut second);
        }
        assert_eq!(internet_quiet(), 1);
        set_internet_quiet(2);
        assert_eq!(internet_quiet(), 2);

        unsafe {
            set_current_instance(&mut first);
        }
        assert_eq!(internet_quiet(), 0);

        clear_current_instance();
    }

    #[test]
    fn file_download_copies_binary_bytes_and_appends() {
        let dir = std::env::temp_dir().join(format!(
            "rport-file-download-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.bin");
        let dest = dir.join("dest.bin");
        // CPBUFSIZE + 3 exercises the chunked copy; embedded NULs and
        // non-UTF8 bytes keep this off any text path.
        let data: Vec<u8> = (0..CPBUFSIZE + 3).map(|i| (i % 251) as u8).collect();
        std::fs::write(&source, &data).unwrap();

        file_download(
            &format!("file://{}", source.display()),
            dest.to_str().unwrap(),
            b"wb",
        );
        assert_eq!(std::fs::read(&dest).unwrap(), data);

        file_download(
            &format!("file://{}", source.display()),
            dest.to_str().unwrap(),
            b"ab",
        );
        let mut expected = data.clone();
        expected.extend_from_slice(&data);
        assert_eq!(std::fs::read(&dest).unwrap(), expected);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
