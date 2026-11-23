// Port of R's modules/internet/internet.c (737 lines)
// Internet connection interface - download.file(), url(), socket connections
// Unix implementation with real file:// download and raw socket HTTP GET.
// Windows-specific functions (wininet) remain as stubs.

use crate::mainutils::rfile::open_file;
use core::ffi::{c_int, c_void};
use std::ffi::CString;
use std::io::{ErrorKind, Read, Write as IoWrite};
use std::net::TcpStream;
use std::path::Path;

use crate::main::errors::Rf_error_cstr;
use crate::sexp::accessors::{
    CAR, CDR, LENGTH, LOGICAL_ELT, STRING_ELT, TYPEOF, char_cstr_bytes_opt,
};
use crate::sexp::constructors::Rf_ScalarInteger;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::{owner::OwnerToken, protect::protect};

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
    if TYPEOF(x) != SEXPTYPE::LGLSXP || LENGTH(x) == 0 {
        NA_LOGICAL
    } else {
        LOGICAL_ELT(x, 0)
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

/// HttpResponse - an opened HTTP response stream.
///
/// `body` yields the response body: bytes already buffered while parsing the
/// headers first, then the remainder of the socket stream.
struct HttpResponse<R> {
    body: std::io::Chain<std::io::Cursor<Vec<u8>>, R>,
    length: Option<DLsize_t>,
    content_type: String,
}

/// http_parse_url - parse a URL into host, port, and path components.
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
/// Returns the response stream on success, None on failure.
///
/// This is the Unix equivalent of the Windows wininet-based in_R_HTTPOpen2.
fn http_open(url: &str, headers: &str) -> Option<HttpResponse<TcpStream>> {
    // Only handle http:// (not https:// — that requires TLS)
    if !url.starts_with("http://") {
        return None;
    }
    let (host, port, path) = http_parse_url(url)?;
    let host = host.to_str().ok()?;

    // Open TCP connection
    let addr = format!("{}:{}", host, port);
    let mut stream = TcpStream::connect(&*addr).ok()?;

    // Set read timeout to avoid hanging forever
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));

    // Build HTTP GET request
    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nConnection: close\r\n",
        path, host, "RMath-Rust/1.0"
    );
    for line in headers.lines().map(str::trim).filter(|line| !line.is_empty()) {
        request.push_str(line);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");

    // Send request
    stream.write_all(request.as_bytes()).ok()?;

    read_http_response(stream)
}

/// read_http_response - read response headers and return the body stream.
///
/// Bytes already fetched past the header terminator are kept at the front of
/// `body` so no part of the response is lost. Returns None when the headers
/// are malformed, oversized, or report a non-2xx status.
fn read_http_response<R: Read>(mut stream: R) -> Option<HttpResponse<R>> {
    const MAX_HEADER_SIZE: usize = 64 * 1024;
    let mut response = Vec::with_capacity(IBUFSIZE * 4);
    let mut buf = [0; IBUFSIZE];
    let end = loop {
        if response.len() >= MAX_HEADER_SIZE {
            return None;
        }
        // Rescan a few bytes back so a terminator split across reads is found.
        let start = response.len().saturating_sub(3);
        match stream.read(&mut buf) {
            Ok(0) => return None,
            Ok(n) => response.extend_from_slice(&buf[..n]),
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
        if let Some(end) = find_header_end(&response[start..]) {
            let end = start + end;
            if end + 4 > MAX_HEADER_SIZE {
                return None;
            }
            break end;
        }
    };
    let header = String::from_utf8_lossy(&response[..end]);
    if !(200..300).contains(&parse_status_code(&header)?) {
        return None;
    }
    let length = parse_content_length(&header);
    let content_type = parse_content_type(&header).unwrap_or_else(|| "unknown".to_owned());
    let buffered_body = response.split_off(end + 4);
    Some(HttpResponse {
        body: std::io::Cursor::new(buffered_body).chain(stream),
        length,
        content_type,
    })
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
/// Returns 0 on success; raises an R error on open or write failure.
fn http_download(url: &str, file: &str, mode: &[u8], quiet: c_int, headers: &str) -> c_int {
    // Open dest file
    let mut dst_file = open_file(Path::new(file), mode).unwrap_or_else(|error| {
        download_error(format!("cannot open destfile '{}', reason '{}'", file, error))
    });

    // Report URL being tried
    if quiet == 0 {
        eprintln!("trying URL '{}'", url);
    }

    let Some(mut response) = http_open(url, headers) else {
        drop(dst_file);
        // Check if mode contains 'w' to clean up partial file
        if std::str::from_utf8(mode).is_ok_and(|mode| mode.contains('w')) {
            let _ = std::fs::remove_file(file);
        }
        download_error(format!("cannot open URL '{}'", url));
    };

    // Report content info if not quiet
    if quiet == 0 {
        eprint!("Content type '{}'", response.content_type);
        if let Some(len) = response.length {
            if len > 1024 * 1024 {
                eprintln!(" length {} bytes ({:.1} MB)", len, len as f64 / 1024.0 / 1024.0);
            } else if len > 10240 {
                eprintln!(" length {} bytes ({} KB)", len, len / 1024);
            } else {
                eprintln!(" length {} bytes", len);
            }
        } else {
            eprintln!(" length unknown");
        }
    }

    let total = response.length.unwrap_or(-1);
    let guess = if total > 0 { total } else { 100 * 1024 };

    let mut nbytes: DLsize_t = 0;
    let mut ndots: DLsize_t = 0;
    let mut ndashes: c_int = 0;
    let mut buf = [0u8; IBUFSIZE];

    // Read loop
    loop {
        let nread = match response.body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        };

        if dst_file.write_all(&buf[..nread]).is_err() {
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

    drop(response);

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

// =========================================================================
// Unix implementation
// =========================================================================

fn require_download_owner(owner: OwnerToken<'_>) {
    owner
        .require_active()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
}

fn string_bytes(owner: OwnerToken<'_>, vector: SEXP) -> Vec<u8> {
    let value = STRING_ELT(vector, 0);
    require_download_owner(owner);
    char_cstr_bytes_opt(value).unwrap_or_default()
}

/// in_do_download - download.file() internal implementation.
/// Signature: SEXP in_do_download(SEXP args)
/// Expects args: url, destfile, quiet, mode, cacheOK, headers [, method]
///
/// Supports:
///   file:// URLs — direct file copy (matches R behavior)
///   http:// URLs — raw socket HTTP GET (Unix implementation, replaces defunct "internal" method)
///   https:// URLs — error (requires TLS, use libcurl method instead)
///   ftp:// URLs — error (defunct in R 4.2+)
///
/// Returns: ScalarInteger with status code (0 = success, 1 = failure)
pub(crate) fn in_do_download(args: SEXP) -> SEXP {
    let owner = OwnerToken::current()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let _root = protect(args);
    let mut args = args;

    // url
    let scmd = CAR(args);
    args = CDR(args);
    if !is_single_string(scmd) {
        let msg = c"invalid 'url' argument";
        Rf_error_cstr(&msg);
    }
    let url = String::from_utf8(string_bytes(owner, scmd)).unwrap_or_default();

    // destfile
    let sfile = CAR(args);
    args = CDR(args);
    if !is_single_string(sfile) {
        let msg = c"invalid 'destfile' argument";
        Rf_error_cstr(&msg);
    }
    // GNU R uses translateChar here; its first-NUL bytes are identical.
    let file = String::from_utf8(string_bytes(owner, sfile)).unwrap_or_default();

    // quiet
    let squiet = CAR(args);
    args = CDR(args);
    let quiet = asRbool(squiet);
    require_download_owner(owner);
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
    let mode = string_bytes(owner, smode);

    // cacheOK
    let scacheOK = CAR(args);
    args = CDR(args);
    let cacheOK = asRbool(scacheOK);
    require_download_owner(owner);
    if cacheOK == NA_LOGICAL {
        let msg = c"invalid 'cacheOK' argument";
        Rf_error_cstr(&msg);
    }
    let _ = cacheOK; // Used only by Windows wininet code path

    // Check if file:// URL
    let file_url = url.starts_with("file://");

    // headers (remaining arg before optional method)
    let sheaders = CAR(args);

    // method (optional, only present on Windows in R's code; we check for it)
    // In R's C code, meth = asLogical(CADR(args)) is only under #ifdef Win32
    // On Unix, there is no method argument in the args list.

    let mut status: c_int = 0;

    if file_url {
        // ---- file:// download ----
        status = file_download(&url, &file, &mode);
    } else if url.starts_with("http://") {
        // ---- http:// download via raw sockets ----
        // R 4.2+ made the "internal" method defunct for HTTP, but since we
        // provide a real socket implementation, we use it here as the
        // Unix-native HTTP download path.
        let headers = if is_single_string(sheaders) {
            String::from_utf8(string_bytes(owner, sheaders)).unwrap_or_default()
        } else {
            String::new()
        };
        status = http_download(&url, &file, &mode, quiet, &headers);
    } else if url.starts_with("https://") {
        // HTTPS requires TLS — not supported by raw sockets
        let msg = CString::new(format!(
            "scheme not supported in URL '{}' (use method=\"libcurl\" for https://)",
            url
        ))
        .unwrap_or_default();
        Rf_error_cstr(&msg);
    } else if url.starts_with("ftp://") {
        // FTP is defunct in R 4.2+
        let msg = c"the 'internal' method for ftp:// URLs is defunct";
        Rf_error_cstr(&msg);
    } else {
        let msg = CString::new(format!("scheme not supported in URL '{}'", url))
            .unwrap_or_default();
        Rf_error_cstr(&msg);
    }

    Rf_ScalarInteger(status)
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

    #[test]
    fn http_response_keeps_body_already_read_with_headers() {
        let bytes =
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Type: application/octet-stream\r\n\r\n\x00\xffz";
        let mut response =
            read_http_response(std::io::Cursor::new(bytes.to_vec())).unwrap();
        assert_eq!(response.length, Some(3));
        assert_eq!(response.content_type, "application/octet-stream");
        let mut body = Vec::new();
        response.body.read_to_end(&mut body).unwrap();
        assert_eq!(body, vec![0, 255, b'z']);
    }

    #[test]
    fn http_response_handles_split_header_terminator() {
        let stream = std::io::Cursor::new(b"HTTP/1.1 200 OK\r\n\r")
            .chain(std::io::Cursor::new(b"\npayload"));
        let mut response = read_http_response(stream).unwrap();
        let mut body = Vec::new();
        response.body.read_to_end(&mut body).unwrap();
        assert_eq!(body, b"payload");
    }

    #[test]
    fn http_response_rejects_oversized_headers() {
        let mut bytes = b"HTTP/1.1 200 OK\r\nX-Long: ".to_vec();
        bytes.resize(64 * 1024 - 1, b'x');
        bytes.extend_from_slice(b"\r\n\r\n");
        assert!(read_http_response(bytes.as_slice()).is_none());
    }
}
