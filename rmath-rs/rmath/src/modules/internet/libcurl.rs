// Port of R's modules/internet/libcurl.c (1420 lines)
// libcurl FFI wrapper for HTTP/FTP downloads and URL connections

use crate::attrib_core::setAttrib;
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::*;
use core::ffi::{c_char, c_double, c_int, c_long, c_uint, c_void};
use libc::{FILE, size_t, ssize_t};
use std::cell::{Cell, RefCell};

// ============================================================
// libcurl FFI types and constants
// ============================================================

/// Opaque CURL easy handle
type CURL = c_void;

/// Opaque CURL multi handle
type CURLM = c_void;

/// Opaque CURL slist handle
type curl_slist = c_void;

/// libcurl error codes
const CURLE_OK: c_int = 0;
const CURLE_OPERATION_TIMEDOUT: c_int = 28;
const CURLE_ABORTED_BY_CALLBACK: c_int = 42;

/// CURLINFO option constants
const CURLINFO_EFFECTIVE_URL: c_int = 0x100001;
const CURLINFO_RESPONSE_CODE: c_int = 0x200002;
const CURLINFO_SIZE_DOWNLOAD: c_int = 0x300001;
const CURLINFO_SIZE_DOWNLOAD_T: c_int = 0x300305;
const CURLINFO_CONTENT_LENGTH_DOWNLOAD: c_int = 0x300001 + 5;
const CURLINFO_CONTENT_LENGTH_DOWNLOAD_T: c_int = 0x300300 + 5;
const CURLINFO_CONTENT_TYPE: c_int = 0x100001 + 18;
const CURLINFO_PRIVATE: c_int = 0x10000E + 4;
const CURLINFO_TLS_SSL_PTR: c_int = 0x400000 + 32;

/// CURLOPT option constants
const CURLOPT_URL: c_int = 10000 + 2;
const CURLOPT_NOPROGRESS: c_int = 0 + 43;
const CURLOPT_NOBODY: c_int = 0 + 44;
const CURLOPT_FAILONERROR: c_int = 0 + 45;
const CURLOPT_WRITEFUNCTION: c_int = 0 + 11;
const CURLOPT_WRITEDATA: c_int = 0 + 1;
const CURLOPT_HEADERFUNCTION: c_int = 0 + 79;
const CURLOPT_WRITEHEADER: c_int = 0 + 29;
const CURLOPT_SSL_VERIFYHOST: c_int = 0 + 81;
const CURLOPT_SSL_VERIFYPEER: c_int = 0 + 64;
const CURLOPT_SSLVERSION: c_int = 0 + 84;
const CURLOPT_USERAGENT: c_int = 10000 + 18;
const CURLOPT_CONNECTTIMEOUT_MS: c_int = 0 + 155;
const CURLOPT_TIMEOUT_MS: c_int = 0 + 155;
const CURLOPT_TIMEOUT: c_int = 0 + 13;
const CURLOPT_FOLLOWLOCATION: c_int = 0 + 52;
const CURLOPT_MAXREDIRS: c_int = 0 + 68;
const CURLOPT_VERBOSE: c_int = 0 + 41;
const CURLOPT_COOKIEFILE: c_int = 10000 + 31;
const CURLOPT_NETRC: c_int = 0 + 51;
const CURLOPT_NETRC_FILE: c_int = 10000 + 118;
const CURLOPT_HTTPHEADER: c_int = 0 + 23;
const CURLOPT_CAINFO: c_int = 10000 + 65;
const CURLOPT_ERRORBUFFER: c_int = 10000 + 34;
const CURLOPT_PRIVATE: c_int = 0 + 10100 + 10;
const CURLOPT_PIPEWAIT: c_int = 0 + 247;
const CURLOPT_TCP_KEEPALIVE: c_int = 0 + 213;
const CURLOPT_PROGRESSFUNCTION: c_int = 0 + 56;
const CURLOPT_PROGRESSDATA: c_int = 0 + 10000 + 57;
const CURLOPT_XFERINFOFUNCTION: c_int = 0 + 57;
const CURLOPT_XFERINFODATA: c_int = 0 + 10000 + 57;
const CURLOPT_LOW_SPEED_TIME: c_int = 0 + 20;
const CURLOPT_LOW_SPEED_LIMIT: c_int = 0 + 19;
const CURLOPT_PREREQFUNCTION: c_int = 0 + 272;
const CURLOPT_PREREQDATA: c_int = 0 + 10000 + 273;
const CURLOPT_SSL_OPTIONS: c_int = 0 + 216;

/// CURL_SSLVERSION constants
const CURL_SSLVERSION_TLSv1_0: c_long = 1;
const CURL_SSLVERSION_TLSv1_1: c_long = 2;
const CURL_SSLVERSION_TLSv1_2: c_long = 3;
const CURL_SSLVERSION_TLSv1_3: c_long = 4;

/// CURL_NETRC constants
const CURL_NETRC_OPTIONAL: c_long = 1;

/// CURL_SSL_OPTIONS constants
const CURLSSLOPT_REVOKE_BEST_EFFORT: c_long = 1 << 0;

/// CURLMOPT option constants
const CURLMOPT_MAX_HOST_CONNECTIONS: c_int = 6 + 20000;

/// CURLversion constant
const CURLVERSION_NOW: c_int = 0;

/// CURL_MAX_WRITE_SIZE (from libcurl)
const CURL_MAX_WRITE_SIZE: usize = 16384;

/// CURL_ERROR_SIZE (from libcurl)
const CURL_ERROR_SIZE: usize = 256;

/// CURL_PREREQFUNC_OK
const CURL_PREREQFUNC_OK: c_int = 0;

/// CURLSSLBACKEND_SCHANNEL
const CURLSSLBACKEND_SCHANNEL: c_int = 5;

/// curl_version_info_data struct matching libcurl's definition
#[repr(C)]
struct curl_version_info_data {
    age: c_int,
    version: *const c_char,
    version_num: c_uint,
    host: *const c_char,
    features: c_int,
    ssl_version: *const c_char,
    ssl_version_num: c_long,
    libz_version: *const c_char,
    protocols: *const *const c_char,
    ares: *const c_char,
    ares_num: c_int,
    libidn: *const c_char,
    iconv_ver_num: c_int,
    libssh_version: *const c_char,
    brotli_version_num: c_uint,
    brotli_version: *const c_char,
    nghttp2_version_num: c_uint,
    nghttp2_version: *const c_char,
    quic_version_num: c_uint,
    quic_version: *const c_char,
    cainfo: *const c_char,
    capath: *const c_char,
    zstd_version_num: c_uint,
    zstd_version: *const c_char,
    hyper_version: *const c_char,
    gsasl_version: *const c_char,
    feature_names: *const *const c_char,
}

/// curl_tlssessioninfo struct (simplified)
#[repr(C)]
struct curl_tlssessioninfo {
    backend: c_int,
    intern: *mut c_void,
}

/// CURLMsg struct (simplified for multi info read)
#[repr(C)]
struct CURLMsg {
    msg: c_int,
    easy_handle: *mut CURL,
    data: CURLMsgData,
}

#[repr(C)]
union CURLMsgData {
    whatever: *mut c_void,
    result: c_int,
}

/// CURLMcode return type
type CURLMcode = c_int;
const CURLM_OK: c_int = 0;

/// CURLcode return type
type CURLcode = c_int;

/// curl_off_t
type curl_off_t = i64;

// ============================================================
// libcurl FFI function declarations (linked via system libcurl)
// ============================================================

unsafe extern "C" {
    // curl_easy functions
    fn curl_easy_init() -> *mut CURL;
    fn curl_easy_setopt(handle: *mut CURL, option: c_int, ...) -> c_int;
    fn curl_easy_perform(handle: *mut CURL) -> c_int;
    fn curl_easy_cleanup(handle: *mut CURL);
    fn curl_easy_getinfo(handle: *mut CURL, info: c_int, ...) -> c_int;
    fn curl_easy_strerror(code: c_int) -> *const c_char;

    // curl_multi functions
    fn curl_multi_init() -> *mut CURLM;
    fn curl_multi_setopt(handle: *mut CURLM, option: c_int, ...) -> c_int;
    fn curl_multi_add_handle(multi: *mut CURLM, easy: *mut CURL) -> c_int;
    fn curl_multi_remove_handle(multi: *mut CURLM, easy: *mut CURL) -> c_int;
    fn curl_multi_perform(multi: *mut CURLM, running_handles: *mut c_int) -> c_int;
    fn curl_multi_cleanup(multi: *mut CURLM);
    fn curl_multi_info_read(multi: *mut CURLM, msgs_in_queue: *mut c_int) -> *mut CURLMsg;
    fn curl_multi_wait(
        multi: *mut CURLM,
        extra_fd: *mut c_void,
        extra_nfds: c_uint,
        timeout_ms: c_int,
        ret: *mut c_int,
    ) -> c_int;

    // curl_slist functions
    fn curl_slist_append(list: *mut curl_slist, string: *const c_char) -> *mut curl_slist;
    fn curl_slist_free_all(list: *mut curl_slist);

    // curl_version functions
    fn curl_version_info(version: c_int) -> *const curl_version_info_data;
}

// ============================================================
// Module-level state (statics matching C code)
// ============================================================

thread_local! { static current_timeout: Cell<c_int> = Cell::new(0); }
thread_local! { static current_time: Cell<c_double> = Cell::new(0.0); }

// ============================================================
// Internal helper functions
// ============================================================

/// http_errstr - convert HTTP status code to error string
fn http_errstr(status: c_long) -> *const c_char {
    match status {
        400 => b"Bad Request\0".as_ptr() as *const c_char,
        401 => b"Unauthorized\0".as_ptr() as *const c_char,
        402 => b"Payment Required\0".as_ptr() as *const c_char,
        403 => b"Forbidden\0".as_ptr() as *const c_char,
        404 => b"Not Found\0".as_ptr() as *const c_char,
        405 => b"Method Not Allowed\0".as_ptr() as *const c_char,
        406 => b"Not Acceptable\0".as_ptr() as *const c_char,
        407 => b"Proxy Authentication Required\0".as_ptr() as *const c_char,
        408 => b"Request Timeout\0".as_ptr() as *const c_char,
        409 => b"Conflict\0".as_ptr() as *const c_char,
        410 => b"Gone\0".as_ptr() as *const c_char,
        411 => b"Length Required\0".as_ptr() as *const c_char,
        412 => b"Precondition Failed\0".as_ptr() as *const c_char,
        413 => b"Request Entity Too Large\0".as_ptr() as *const c_char,
        414 => b"Request-URI Too Long\0".as_ptr() as *const c_char,
        415 => b"Unsupported Media Type\0".as_ptr() as *const c_char,
        416 => b"Requested Range Not Satisfiable\0".as_ptr() as *const c_char,
        417 => b"Expectation Failed\0".as_ptr() as *const c_char,
        500 => b"Internal Server Error\0".as_ptr() as *const c_char,
        501 => b"Not Implemented\0".as_ptr() as *const c_char,
        502 => b"Bad Gateway\0".as_ptr() as *const c_char,
        503 => b"Service Unavailable\0".as_ptr() as *const c_char,
        504 => b"Gateway Timeout\0".as_ptr() as *const c_char,
        _ => b"Unknown Error\0".as_ptr() as *const c_char,
    }
}

/// ftp_errstr - convert FTP status code to error string
fn ftp_errstr(status: c_long) -> *const c_char {
    match status {
        421 => b"Service not available, closing control connection\0".as_ptr() as *const c_char,
        425 => b"Cannot open data connection\0".as_ptr() as *const c_char,
        426 => b"Connection closed; transfer aborted\0".as_ptr() as *const c_char,
        430 => b"Invalid username or password\0".as_ptr() as *const c_char,
        434 => b"Requested host unavailable\0".as_ptr() as *const c_char,
        450 => b"Requested file action not taken\0".as_ptr() as *const c_char,
        451 => b"Requested action aborted; local error in processing\0".as_ptr() as *const c_char,
        452 => b"Requested action not taken; insufficient storage space in system\0".as_ptr()
            as *const c_char,
        501 => b"Syntax error in parameters or arguments\0".as_ptr() as *const c_char,
        502 => b"Command not implemented\0".as_ptr() as *const c_char,
        503 => b"Bad sequence of commands\0".as_ptr() as *const c_char,
        504 => b"Command not implemented for that parameter\0".as_ptr() as *const c_char,
        530 => b"Not logged in\0".as_ptr() as *const c_char,
        532 => b"Need account for storing files\0".as_ptr() as *const c_char,
        550 => b"Requested action not taken; file unavailable\0".as_ptr() as *const c_char,
        551 => b"Requested action aborted; page type unknown\0".as_ptr() as *const c_char,
        552 => b"Requested file action aborted; exceeded storage allocation\0".as_ptr()
            as *const c_char,
        553 => b"Requested action not taken; file name not allowed\0".as_ptr() as *const c_char,
        _ => b"Unknown Error\0".as_ptr() as *const c_char,
    }
}

/// streql - compare two C strings for equality
unsafe fn streql(a: *const c_char, b: *const c_char) -> bool {
    unsafe {
        if a.is_null() || b.is_null() {
            return false;
        }
        libc::strcmp(a, b) == 0
    }
}

/// R_MIN macro
fn r_min<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { a } else { b }
}

// ============================================================
// Headers storage for curlGetHeaders
// ============================================================

const MAX_HEADERS: usize = 500;
const MAX_HEADER_LEN: usize = 2049;

thread_local! { static HEADERS: RefCell<[[c_char; MAX_HEADER_LEN]; MAX_HEADERS]> = RefCell::new([[0; MAX_HEADER_LEN]; MAX_HEADERS]); }
thread_local! { static headers_used: Cell<c_int> = Cell::new(0); }

/// rcvHeaders - callback for receiving HTTP headers (used by curlGetHeaders)
unsafe fn rcvHeaders(
    buffer: *mut c_void,
    size: size_t,
    nmemb: size_t,
    _userp: *mut c_void,
) -> size_t {
    unsafe {
        let d = buffer as *mut c_char;
        let result = size * nmemb;
        let res = if result > 2048 { 2048 } else { result };
        if (headers_used.with(|v| v.get()) as usize) >= MAX_HEADERS {
            return result;
        }
        HEADERS.with(|headers| {
            libc::strncpy(
                headers.borrow_mut()[headers_used.with(|v| v.get()) as usize].as_mut_ptr(),
                d,
                res,
            )
        });
        // Header line is NOT zero terminated
        HEADERS.with(|headers| {
            *headers.borrow_mut()[headers_used.with(|v| v.get()) as usize]
                .as_mut_ptr()
                .add(res) = 0
        });
        headers_used.with(|v| v.set(v.get() + 1));
        result
    }
}

/// rcvBody - callback for receiving response body (discard spurious FTP body)
unsafe fn rcvBody(buffer: *mut c_void, size: size_t, nmemb: size_t, _userp: *mut c_void) -> size_t {
    size * nmemb
}

// ============================================================
// handle_cleanup - cleanup handler for curl easy handles
// ============================================================

unsafe fn handle_cleanup(data: *mut c_void) {
    unsafe {
        let hnd = data as *mut CURL;
        if !hnd.is_null() {
            curl_easy_cleanup(hnd);
        }
    }
}

// ============================================================
// download_cleanup_info struct and helpers
// ============================================================

/// download_cleanup_info - holds state for multi-URL download cleanup
struct download_cleanup_info {
    headers: *mut curl_slist,
    mhnd: *mut CURLM,
    nurls: c_int,
    hnd: *mut *mut CURL,
    out: *mut *mut FILE,
    tstart: *mut c_double,
    sfile: SEXP,
    errs: *mut c_int,
}

/// download_cleanup_url - clean up a single URL at given index
unsafe fn download_cleanup_url(i: c_int, c: *mut download_cleanup_info) {
    unsafe {
        let c_ref = &mut *c;
        if !c_ref.out.is_null() && !(*c_ref.out.add(i as usize)).is_null() {
            libc::fclose(*c_ref.out.add(i as usize));
            *c_ref.out.add(i as usize) = std::ptr::null_mut();

            let mut dl: c_double = 0.0;
            if !c_ref.hnd.is_null() && !(*c_ref.hnd.add(i as usize)).is_null() {
                curl_easy_getinfo(
                    *c_ref.hnd.add(i as usize),
                    CURLINFO_SIZE_DOWNLOAD,
                    &mut dl as *mut c_double as *mut c_void,
                );
            }

            if !Rf_isNull(c_ref.sfile) != 0 {
                let mut status: c_long = 0;
                if !c_ref.hnd.is_null() && !(*c_ref.hnd.add(i as usize)).is_null() {
                    curl_easy_getinfo(
                        *c_ref.hnd.add(i as usize),
                        CURLINFO_RESPONSE_CODE,
                        &mut status as *mut c_long as *mut c_void,
                    );
                }
                // Delete file if status != 200 and no data downloaded
                if status != 200 && dl == 0.0 {
                    let fname = translateChar(STRING_ELT(c_ref.sfile, i as R_xlen_t));
                    libc::unlink(fname);
                }
            }

            if !c_ref.mhnd.is_null()
                && !c_ref.hnd.is_null()
                && !(*c_ref.hnd.add(i as usize)).is_null()
            {
                curl_multi_remove_handle(c_ref.mhnd, *c_ref.hnd.add(i as usize));
            }
        }

        if !c_ref.hnd.is_null() && !(*c_ref.hnd.add(i as usize)).is_null() {
            curl_easy_cleanup(*c_ref.hnd.add(i as usize));
            *c_ref.hnd.add(i as usize) = std::ptr::null_mut();
        }
    }
}

/// download_cleanup - cleanup all resources for a batch download
unsafe fn download_cleanup(data: *mut c_void) {
    unsafe {
        let c = data as *mut download_cleanup_info;
        if c.is_null() {
            return;
        }
        let c_ref = &mut *c;
        for i in 0..c_ref.nurls {
            download_cleanup_url(i, c);
        }
        if !c_ref.mhnd.is_null() {
            curl_multi_cleanup(c_ref.mhnd);
            c_ref.mhnd = std::ptr::null_mut();
        }
        if !c_ref.headers.is_null() {
            curl_slist_free_all(c_ref.headers);
            c_ref.headers = std::ptr::null_mut();
        }
    }
}

/// download_report_url_error - report a download error based on libcurl message
unsafe fn download_report_url_error(msg: *mut CURLMsg) {
    unsafe {
        let mut url: *const c_char = std::ptr::null();
        let mut status: c_long = 0;
        let mut url_errs: *mut c_int = std::ptr::null_mut();

        if msg.is_null() {
            return;
        }
        curl_easy_getinfo(
            (*msg).easy_handle,
            CURLINFO_EFFECTIVE_URL,
            &mut url as *mut *const c_char as *mut c_void,
        );
        curl_easy_getinfo(
            (*msg).easy_handle,
            CURLINFO_RESPONSE_CODE,
            &mut status as *mut c_long as *mut c_void,
        );
        if curl_easy_getinfo(
            (*msg).easy_handle,
            CURLINFO_PRIVATE,
            &mut url_errs as *mut *mut c_int as *mut c_void,
        ) == CURLE_OK
            && !url_errs.is_null()
        {
            *url_errs += 1;
        }

        if status >= 400 {
            if !url.is_null() && *url == 'h' as c_char {
                let strerr = http_errstr(status);
                Rf_warning1(
                    b"cannot open URL '%s': HTTP status was '%ld %s'\0".as_ptr() as *const c_char
                );
                let _ = strerr;
            } else {
                let strerr = ftp_errstr(status);
                Rf_warning1(
                    b"cannot open URL '%s': FTP status was '%ld %s'\0".as_ptr() as *const c_char
                );
                let _ = strerr;
            }
        } else {
            let result_code = (*msg).data.result;
            let strerr = curl_easy_strerror(result_code);
            let timedout = result_code == CURLE_OPERATION_TIMEDOUT
                || result_code == CURLE_ABORTED_BY_CALLBACK
                || (!strerr.is_null()
                    && streql(strerr, b"Timeout was reached\0".as_ptr() as *const c_char));

            if timedout {
                Rf_warning1(b"URL '%s': Timeout was reached\0".as_ptr() as *const c_char);
                let _ = current_timeout.with(|v| v.get());
            } else {
                Rf_warning1(b"URL '%s': status was unknown\0".as_ptr() as *const c_char);
                let _ = strerr;
            }
        }
    }
}

/// curlMultiCheckerrs - check curl_multi_info_read for errors, return count
unsafe fn curlMultiCheckerrs(mhnd: *mut CURLM) -> c_int {
    unsafe {
        let mut retval: c_int = 0;
        let mut n: c_int = 1;
        while n > 0 {
            let msg = curl_multi_info_read(mhnd, &mut n);
            if !msg.is_null() && (*msg).data.result != CURLE_OK {
                download_report_url_error(msg);
                retval += 1;
            }
        }
        retval
    }
}

// ============================================================
// curlCommon - common curl handle setup
// ============================================================

unsafe fn curlCommon(hnd: *mut CURL, redirect: c_int, verify: c_int) {
    unsafe {
        if verify != 0 {
            let capath = libc::getenv(b"CURL_CA_BUNDLE\0".as_ptr() as *const c_char);
            if !capath.is_null() && *capath != 0 {
                curl_easy_setopt(hnd, CURLOPT_CAINFO, capath);
            }
        } else {
            curl_easy_setopt(hnd, CURLOPT_SSL_VERIFYHOST, 0);
            curl_easy_setopt(hnd, CURLOPT_SSL_VERIFYPEER, 0);
        }

        // User agent: use HTTPUserAgent option or default to libcurl version
        let mut default_agent: c_int = 1;
        let sua = GetOption1(install(b"HTTPUserAgent\0".as_ptr() as *const c_char));
        if TYPEOF(sua) == SEXPTYPE::STRSXP && LENGTH(sua) == 1 {
            let p = translateChar(STRING_ELT(sua, 0));
            if !p.is_null()
                && *p != 0
                && *p.add(1) != 0
                && *p.add(2) != 0
                && *p == 'R' as c_char
                && *p.add(1) == ' ' as c_char
                && *p.add(2) == '(' as c_char
            {
                // Default R user agent, don't override
            } else {
                default_agent = 0;
                curl_easy_setopt(hnd, CURLOPT_USERAGENT, p);
            }
        }
        if default_agent != 0 {
            let mut buf: [c_char; 20] = [0; 20];
            let d = curl_version_info(CURLVERSION_NOW);
            if !d.is_null() && !(*d).version.is_null() {
                libc::snprintf(
                    buf.as_mut_ptr(),
                    20,
                    b"libcurl/%s\0".as_ptr() as *const c_char,
                    (*d).version,
                );
                curl_easy_setopt(hnd, CURLOPT_USERAGENT, buf.as_ptr());
            }
        }

        // Timeout from option
        let timeout0 = asInteger(GetOption1(install(b"timeout\0".as_ptr() as *const c_char)));
        let timeout: c_long = if timeout0 == NA_INTEGER {
            0
        } else {
            1000 * timeout0 as c_long
        };
        current_timeout.with(|v| v.set(if timeout0 == NA_INTEGER { 0 } else { timeout0 }));
        curl_easy_setopt(hnd, CURLOPT_CONNECTTIMEOUT_MS, timeout);
        curl_easy_setopt(hnd, CURLOPT_TIMEOUT_MS, timeout);

        if redirect != 0 {
            curl_easy_setopt(hnd, CURLOPT_FOLLOWLOCATION, 1);
            curl_easy_setopt(hnd, CURLOPT_MAXREDIRS, 20);
        }

        let verbosity = asInteger(GetOption1(install(
            b"internet.info\0".as_ptr() as *const c_char
        )));
        if verbosity < 2 {
            curl_easy_setopt(hnd, CURLOPT_VERBOSE, 1);
        }

        // Enable cookie engine (cookies in memory)
        curl_easy_setopt(hnd, CURLOPT_COOKIEFILE, b"\0".as_ptr());

        // netrc file
        let snetrc = GetOption1(install(b"netrc\0".as_ptr() as *const c_char));
        if TYPEOF(snetrc) == SEXPTYPE::STRSXP && LENGTH(snetrc) == 1 {
            let p = translateCharFP(STRING_ELT(snetrc, 0));
            curl_easy_setopt(hnd, CURLOPT_NETRC, CURL_NETRC_OPTIONAL);
            curl_easy_setopt(hnd, CURLOPT_NETRC_FILE, p);
        }
    }
}

// ============================================================
// Progress callbacks for downloads
// ============================================================

thread_local! { static total: Cell<c_double> = Cell::new(0.0); }
thread_local! { static ndashes: Cell<c_int> = Cell::new(0); }

/// putdashes - print download progress dashes (Unix)
#[cfg(unix)]
unsafe fn putdashes(pold: *mut c_int, new: c_int) {
    unsafe {
        let old_val = *pold;
        for _i in old_val..new {
            eprint!("=");
        }
        use std::io::Write;
        let _ = std::io::stderr().flush();
        *pold = new;
    }
}

#[cfg(not(unix))]
unsafe fn putdashes(_pold: *mut c_int, _new: c_int) {}

/// progress - download progress callback (single URL)
unsafe fn progress(
    clientp: *mut c_void,
    dltotal: c_double,
    dlnow: c_double,
    _ultotal: c_double,
    _ulnow: c_double,
) -> c_int {
    unsafe {
        let hnd = clientp as *mut CURL;
        let mut status: c_long = 0;
        curl_easy_getinfo(
            hnd,
            CURLINFO_RESPONSE_CODE,
            &mut status as *mut c_long as *mut c_void,
        );

        // We only use downloads. dltotal may be zero.
        if status < 300 && dltotal > 0.0 {
            if total.with(|v| v.get()) == 0.0 {
                total.with(|v| v.set(dltotal));
                let mut content_type: *mut c_char = std::ptr::null_mut();
                curl_easy_getinfo(
                    hnd,
                    CURLINFO_CONTENT_TYPE,
                    &mut content_type as *mut *mut c_char as *mut c_void,
                );
                if content_type.is_null() {
                    eprintln!("Content type 'unknown'");
                } else {
                    eprintln!(
                        "Content type '{}'",
                        std::ffi::CStr::from_ptr(content_type).to_string_lossy()
                    );
                }
                let total_val = total.with(|v| v.get());
                if total_val > 1024.0 * 1024.0 {
                    eprintln!(
                        " length {:.0} bytes ({:.1} MB)",
                        total_val,
                        total_val / 1024.0 / 1024.0
                    );
                } else if total_val > 10240.0 {
                    eprintln!(
                        " length {} bytes ({} KB)",
                        total_val as c_int,
                        (total_val / 1024.0) as c_int
                    );
                } else {
                    eprintln!(" length {} bytes", total_val as c_int);
                }
            }
            let mut ndashes_ref = ndashes.with(|v| v.get());
            putdashes(
                &mut ndashes_ref,
                (50.0 * dlnow / total.with(|v| v.get())) as c_int,
            );
            ndashes.with(|v| v.set(ndashes_ref));
        }
        0
    }
}

/// progress_multi - download progress callback (multi URL) - implements absolute-time timeout
unsafe fn progress_multi(
    clientp: *mut c_void,
    dltotal: c_double,
    dlnow: c_double,
    _ultotal: c_double,
    _ulnow: c_double,
) -> c_int {
    unsafe {
        let tstart = clientp as *mut c_double;
        if !tstart.is_null() {
            if *tstart == 0.0 && (dlnow > 0.0 || dltotal > 0.0) {
                *tstart = current_time.with(|v| v.get());
            } else if *tstart > 0.0
                && (current_time.with(|v| v.get()) - *tstart)
                    > (current_timeout.with(|v| v.get()) as c_double)
            {
                return 1; // abort transfer
            }
        }
        0
    }
}

/// prereq_multi - pre-request callback for multi downloads
unsafe fn prereq_multi(
    clientp: *mut c_void,
    _conn_primary_ip: *mut c_char,
    _conn_local_ip: *mut c_char,
    _conn_primary_port: c_int,
    _conn_local_port: c_int,
) -> c_int {
    unsafe {
        let tstart = clientp as *mut c_double;
        if !tstart.is_null() {
            *tstart = current_time.with(|v| v.get());
        }
        CURL_PREREQFUNC_OK
    }
}

// ============================================================
// download_add_url - add a URL to the download queue
// ============================================================

/// download_add_url - open file, create easy handle, add to multi-handle.
/// Returns 0 on success. Reports errors only when mustWork != 0.
unsafe fn download_add_url(
    i: c_int,
    scmd: SEXP,
    mode: *const c_char,
    quiet: c_int,
    single: c_int,
    mustwork: c_int,
    c: *mut download_cleanup_info,
) -> c_int {
    unsafe {
        let c_ref = &mut *c;
        let url = translateChar(STRING_ELT(scmd, i as R_xlen_t));

        c_ref.hnd = c_ref.hnd; // already set
        let hnd_ptr = c_ref.hnd.add(i as usize);
        *hnd_ptr = curl_easy_init();
        if hnd_ptr.is_null() || (*hnd_ptr).is_null() {
            if mustwork != 0 {
                *c_ref.errs.add(i as usize) += 1;
                Rf_warning1(b"could not create curl handle\0".as_ptr() as *const c_char);
            }
            return 1;
        }

        let hnd = *hnd_ptr;
        curl_easy_setopt(hnd, CURLOPT_URL, url);
        curl_easy_setopt(hnd, CURLOPT_FAILONERROR, 1);
        curl_easy_setopt(hnd, CURLOPT_PIPEWAIT, 1);
        curlCommon(hnd, 1, 1);
        curl_easy_setopt(hnd, CURLOPT_TCP_KEEPALIVE, 1);
        curl_easy_setopt(hnd, CURLOPT_HTTPHEADER, c_ref.headers);

        // Check that destfile can be written
        let file = translateChar(STRING_ELT(c_ref.sfile, i as R_xlen_t));
        let expanded = R_ExpandFileName(file);
        let out_ptr = c_ref.out.add(i as usize);
        *out_ptr = libc::fopen(expanded, mode);
        if out_ptr.is_null() || (*out_ptr).is_null() {
            if mustwork != 0 {
                *c_ref.errs.add(i as usize) += 1;
                Rf_warning1(b"URL: cannot open destfile\0".as_ptr() as *const c_char);
            }
            return 1;
        }

        // Use internal CURLOPT_WRITEFUNCTION (writes to FILE*)
        curl_easy_setopt(hnd, CURLOPT_WRITEDATA, *out_ptr);
        curl_multi_add_handle(c_ref.mhnd, hnd);
        curl_easy_setopt(
            hnd,
            CURLOPT_PRIVATE,
            c_ref.errs.add(i as usize) as *mut c_void,
        );

        total.with(|v| v.set(0.0));
        if quiet == 0 && single != 0 {
            curl_easy_setopt(hnd, CURLOPT_NOPROGRESS, 0);
            ndashes.with(|v| v.set(0));
            curl_easy_setopt(hnd, CURLOPT_XFERINFOFUNCTION, progress as *const c_void);
            curl_easy_setopt(hnd, CURLOPT_XFERINFODATA, hnd);
        } else if quiet != 0 && single != 0 {
            curl_easy_setopt(hnd, CURLOPT_NOPROGRESS, 1);
        } else {
            curl_easy_setopt(hnd, CURLOPT_NOPROGRESS, 0);
            // Implement absolute-time timeout for simultaneous download
            curl_easy_setopt(hnd, CURLOPT_TIMEOUT, 0);
            let tstart_ptr = c_ref.tstart.add(i as usize);
            *tstart_ptr = 0.0;
            curl_easy_setopt(
                hnd,
                CURLOPT_XFERINFOFUNCTION,
                progress_multi as *const c_void,
            );
            curl_easy_setopt(hnd, CURLOPT_XFERINFODATA, tstart_ptr as *mut c_void);
            curl_easy_setopt(hnd, CURLOPT_PREREQFUNCTION, prereq_multi as *const c_void);
            curl_easy_setopt(hnd, CURLOPT_PREREQDATA, tstart_ptr as *mut c_void);
            curl_easy_setopt(
                hnd,
                CURLOPT_LOW_SPEED_TIME,
                current_timeout.with(|v| v.get()) as c_long,
            );
            curl_easy_setopt(hnd, CURLOPT_LOW_SPEED_LIMIT, 1);
        }

        if quiet == 0 {
            eprintln!(
                "trying URL '{}'",
                std::ffi::CStr::from_ptr(url).to_string_lossy()
            );
        }
        0
    }
}

/// download_add_one_url - add one URL to the multi-handle, possibly trying multiple URLs
unsafe fn download_add_one_url(
    i_ptr: *mut c_int,
    scmd: SEXP,
    mode: *const c_char,
    quiet: c_int,
    single: c_int,
    c: *mut download_cleanup_info,
) -> c_int {
    unsafe {
        let c_ref = &mut *c;
        while *i_ptr < c_ref.nurls {
            if download_add_url(*i_ptr, scmd, mode, quiet, single, 1, c) == 0 {
                *i_ptr += 1;
                return 0; // success
            }
            *i_ptr += 1;
        }
        1 // failure
    }
}

/// download_try_add_urls - try adding up to n URLs to the multi-handle
unsafe fn download_try_add_urls(
    i_ptr: *mut c_int,
    n: c_int,
    scmd: SEXP,
    mode: *const c_char,
    quiet: c_int,
    single: c_int,
    c: *mut download_cleanup_info,
) -> c_int {
    unsafe {
        let mut added: c_int = 0;
        let c_ref = &mut *c;
        while added < n && *i_ptr < c_ref.nurls {
            if download_add_url(*i_ptr, scmd, mode, quiet, single, 0, c) == 0 {
                *i_ptr += 1;
                added += 1;
            } else {
                break;
            }
        }
        added
    }
}

/// download_close_finished - clean up finished downloads from multi handle
unsafe fn download_close_finished(c: *mut download_cleanup_info) {
    unsafe {
        let c_ref = &mut *c;
        let mut n: c_int = 1;
        while n > 0 {
            let msg = curl_multi_info_read(c_ref.mhnd, &mut n);
            if msg.is_null() {
                break;
            }

            // Compute URL index from private data
            let mut url_errs: *mut c_int = std::ptr::null_mut();
            curl_easy_getinfo(
                (*msg).easy_handle,
                CURLINFO_PRIVATE,
                &mut url_errs as *mut *mut c_int as *mut c_void,
            );
            let idx = if !url_errs.is_null() && !c_ref.errs.is_null() {
                ((url_errs as usize) - (c_ref.errs as usize)) / std::mem::size_of::<c_int>()
            } else {
                0
            };

            if (*msg).data.result != CURLE_OK {
                download_report_url_error(msg);
            }
            download_cleanup_url(idx as c_int, c);
        }
    }
}

// ============================================================
// Exported R interface functions
// ============================================================

/// in_do_curlVersion - .Internal(curlVersion())
/// Returns a character vector with libcurl version info and attributes.
pub(crate) unsafe fn in_do_curlVersion(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, args, rho);
        checkArity(op, args);

        let ans = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        let _ans_guard = protect(ans);
        let d = curl_version_info(CURLVERSION_NOW);

        if !d.is_null() && !(*d).version.is_null() {
            SET_STRING_ELT(ans, 0, Rf_mkChar((*d).version));
        } else {
            SET_STRING_ELT(ans, 0, Rf_mkChar(b"\0".as_ptr() as *const c_char));
        }

        // ssl_version attribute
        if !d.is_null() && !(*d).ssl_version.is_null() {
            let sSSLVersion = install(b"ssl_version\0".as_ptr() as *const c_char);
            let value = Rf_mkString((*d).ssl_version);
            let _value_guard = protect(value);
            setAttrib(ans, sSSLVersion, value);
        } else if !d.is_null() {
            let sSSLVersion = install(b"ssl_version\0".as_ptr() as *const c_char);
            let value = Rf_mkString(b"none\0".as_ptr() as *const c_char);
            let _value_guard = protect(value);
            setAttrib(ans, sSSLVersion, value);
        }

        // libssh_version attribute
        if !d.is_null() && (*d).age >= 3 && !(*d).libssh_version.is_null() {
            let sLibSSHVersion = install(b"libssh_version\0".as_ptr() as *const c_char);
            let value = Rf_mkString((*d).libssh_version);
            let _value_guard = protect(value);
            setAttrib(ans, sLibSSHVersion, value);
        } else {
            let sLibSSHVersion = install(b"libssh_version\0".as_ptr() as *const c_char);
            let value = Rf_mkString(b"\0".as_ptr() as *const c_char);
            let _value_guard = protect(value);
            setAttrib(ans, sLibSSHVersion, value);
        }

        // protocols attribute
        if !d.is_null() && !(*d).protocols.is_null() {
            let mut n: c_int = 0;
            let mut p = (*d).protocols;
            while !p.is_null() && !(*p).is_null() {
                n += 1;
                p = p.add(1);
            }
            let protocols = Rf_allocVector(SEXPTYPE::STRSXP, n);
            let _protocols_guard = protect(protocols);
            p = (*d).protocols;
            for i in 0..n {
                SET_STRING_ELT(protocols, i as R_xlen_t, Rf_mkChar(*p));
                p = p.add(1);
            }
            let sProtocols = install(b"protocols\0".as_ptr() as *const c_char);
            setAttrib(ans, sProtocols, protocols);
        }

        ans
    }
}

/// in_do_curlGetHeaders - .Internal(curlGetHeaders(url, redirect, verify, timeout, TLS))
pub(crate) unsafe fn in_do_curlGetHeaders(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, rho);
        checkArity(op, args);

        // url
        let scmd = CAR(args);
        if TYPEOF(scmd) != SEXPTYPE::STRSXP || LENGTH(scmd) != 1 {
            Rf_error(b"invalid 'url' argument\0".as_ptr() as *const c_char);
        }
        let url = translateChar(STRING_ELT(scmd, 0));

        headers_used.with(|v| v.set(0));

        // redirect
        let redirect = asLogical(CADR(args));
        if redirect == NA_LOGICAL {
            Rf_error(b"invalid 'redirect' argument\0".as_ptr() as *const c_char);
        }

        // verify
        let verify = asLogical(CADDR(args));
        if verify == NA_LOGICAL {
            Rf_error(b"invalid 'verify' argument\0".as_ptr() as *const c_char);
        }

        // timeout
        let timeout = asInteger(CADDDR(args));
        if timeout == NA_INTEGER {
            Rf_error(b"invalid 'timeout' argument\0".as_ptr() as *const c_char);
        }

        // TLS (CAD4R)
        let sTLS = unsafe { *(args as *const SEXP).add(4) };
        let mut tls: *const c_char = b"\0".as_ptr() as *const c_char;
        if TYPEOF(sTLS) == SEXPTYPE::STRSXP && LENGTH(sTLS) == 1 {
            tls = translateChar(STRING_ELT(sTLS, 0));
        } else {
            Rf_error(b"invalid 'TLS' argument\0".as_ptr() as *const c_char);
        }

        let hnd = curl_easy_init();
        if hnd.is_null() {
            Rf_error(b"could not create curl handle\0".as_ptr() as *const c_char);
        }

        // Set up cleanup context
        curl_easy_setopt(hnd, CURLOPT_URL, url);
        curl_easy_setopt(hnd, CURLOPT_NOPROGRESS, 1);
        curl_easy_setopt(hnd, CURLOPT_NOBODY, 1);
        curl_easy_setopt(hnd, CURLOPT_HEADERFUNCTION, rcvHeaders as *const c_void);
        curl_easy_setopt(hnd, CURLOPT_WRITEHEADER, std::ptr::null::<c_void>());
        // Discard spurious FTP body
        curl_easy_setopt(hnd, CURLOPT_WRITEFUNCTION, rcvBody as *const c_void);
        curlCommon(hnd, redirect, verify);

        if timeout > 0 {
            curl_easy_setopt(hnd, CURLOPT_TIMEOUT, timeout as c_long);
            current_timeout.with(|v| v.set(timeout));
        }

        // TLS version
        if !tls.is_null() && *tls != 0 {
            let mut tls_ver: c_long = CURL_SSLVERSION_TLSv1_0;
            if streql(tls, b"1.0\0".as_ptr() as *const c_char) {
                tls_ver = CURL_SSLVERSION_TLSv1_0;
            } else if streql(tls, b"1.1\0".as_ptr() as *const c_char) {
                tls_ver = CURL_SSLVERSION_TLSv1_1;
            } else if streql(tls, b"1.2\0".as_ptr() as *const c_char) {
                tls_ver = CURL_SSLVERSION_TLSv1_2;
            } else if streql(tls, b"1.3\0".as_ptr() as *const c_char) {
                tls_ver = CURL_SSLVERSION_TLSv1_3;
            } else {
                curl_easy_cleanup(hnd);
                Rf_error(b"invalid 'TLS' argument\0".as_ptr() as *const c_char);
            }
            curl_easy_setopt(hnd, CURLOPT_SSLVERSION, tls_ver);
        }

        let mut errbuf: [c_char; CURL_ERROR_SIZE] = [0; CURL_ERROR_SIZE];
        curl_easy_setopt(hnd, CURLOPT_ERRORBUFFER, errbuf.as_mut_ptr());
        errbuf[0] = 0;

        let ret = curl_easy_perform(hnd);
        if ret != CURLE_OK {
            if errbuf[0] != 0 {
                curl_easy_cleanup(hnd);
                Rf_error(b"libcurl error code %d\0".as_ptr() as *const c_char);
            } else if ret == 77 {
                curl_easy_cleanup(hnd);
                Rf_error(
                    b"libcurl error code 77: unable to access SSL/TLS CA certificates\0".as_ptr()
                        as *const c_char,
                );
            } else {
                curl_easy_cleanup(hnd);
                Rf_error(b"libcurl error code\0".as_ptr() as *const c_char);
            }
        }

        let mut http_code: c_long = 0;
        curl_easy_getinfo(
            hnd,
            CURLINFO_RESPONSE_CODE,
            &mut http_code as *mut c_long as *mut c_void,
        );
        curl_easy_cleanup(hnd);

        let ans = Rf_allocVector(SEXPTYPE::STRSXP, headers_used.with(|v| v.get()));
        let _ans_guard = protect(ans);
        for i in 0..headers_used.with(|v| v.get()) {
            HEADERS.with(|headers| {
                SET_STRING_ELT(
                    ans,
                    i as R_xlen_t,
                    Rf_mkChar(headers.borrow()[i as usize].as_ptr()),
                )
            });
        }

        let sStatus = install(b"status\0".as_ptr() as *const c_char);
        let status = Rf_ScalarInteger(http_code as c_int);
        let _status_guard = protect(status);
        setAttrib(ans, sStatus, status);

        ans
    }
}

/// in_do_curlDownload - .Internal(curlDownload(urls, destfiles, quiet, mode, headers, cacheOK))
pub(crate) unsafe fn in_do_curlDownload(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, rho);
        checkArity(op, args);

        // url
        let mut args_iter = args;
        let scmd = CAR(args_iter);
        args_iter = CDR(args_iter);
        if TYPEOF(scmd) != SEXPTYPE::STRSXP || LENGTH(scmd) < 1 {
            Rf_error(b"invalid 'url' argument\0".as_ptr() as *const c_char);
        }
        let nurls = LENGTH(scmd);
        let single = if nurls == 1 { 1 } else { 0 };
        let max_concurrent_urls: c_int = 15;

        // destfile
        let sfile = CAR(args_iter);
        args_iter = CDR(args_iter);
        if TYPEOF(sfile) != SEXPTYPE::STRSXP || LENGTH(sfile) < 1 {
            Rf_error(b"invalid 'destfile' argument\0".as_ptr() as *const c_char);
        }
        if LENGTH(sfile) != LENGTH(scmd) {
            Rf_error(b"lengths of 'url' and 'destfile' must match\0".as_ptr() as *const c_char);
        }

        // quiet
        let quiet = asLogical(CAR(args_iter));
        args_iter = CDR(args_iter);
        if quiet == NA_LOGICAL {
            Rf_error(b"invalid 'quiet' argument\0".as_ptr() as *const c_char);
        }

        // mode
        let smode = CAR(args_iter);
        args_iter = CDR(args_iter);
        if TYPEOF(smode) != SEXPTYPE::STRSXP || LENGTH(smode) != 1 {
            Rf_error(b"invalid 'mode' argument\0".as_ptr() as *const c_char);
        }
        let mode = translateChar(STRING_ELT(smode, 0));

        // cacheOK
        let cacheOK = asLogical(CAR(args_iter));
        args_iter = CDR(args_iter);
        if cacheOK == NA_LOGICAL {
            Rf_error(b"invalid 'cacheOK' argument\0".as_ptr() as *const c_char);
        }

        // headers
        let sheaders = CAR(args_iter);
        if Rf_isNull(sheaders) == 0 && TYPEOF(sheaders) != SEXPTYPE::STRSXP {
            Rf_error(b"invalid 'headers' argument\0".as_ptr() as *const c_char);
        }

        // Build cleanup info
        let mut c_info = download_cleanup_info {
            headers: std::ptr::null_mut(),
            mhnd: std::ptr::null_mut(),
            nurls,
            hnd: std::ptr::null_mut(),
            out: std::ptr::null_mut(),
            tstart: std::ptr::null_mut(),
            sfile: R_NilValue(),
            errs: std::ptr::null_mut(),
        };

        // Set sfile based on mode
        if !mode.is_null() {
            let mode_str = std::ffi::CStr::from_ptr(mode).to_bytes();
            if mode_str.contains(&b'w') {
                c_info.sfile = sfile;
            }
        }

        // Build headers slist
        let mut headers: *mut curl_slist = std::ptr::null_mut();
        if Rf_isNull(sheaders) == 0 {
            for i in 0..LENGTH(sheaders) {
                let h = translateChar(STRING_ELT(sheaders, i as R_xlen_t));
                let tmp = curl_slist_append(headers, h);
                if tmp.is_null() {
                    if !headers.is_null() {
                        curl_slist_free_all(headers);
                    }
                    Rf_error(b"out of memory\0".as_ptr() as *const c_char);
                }
                headers = tmp;
                c_info.headers = headers;
            }
        }

        // Pragma: no-cache
        if cacheOK == 0 {
            let tmp = curl_slist_append(headers, b"Pragma: no-cache\0".as_ptr() as *const c_char);
            if tmp.is_null() {
                if !headers.is_null() {
                    curl_slist_free_all(headers);
                }
                Rf_error(b"out of memory\0".as_ptr() as *const c_char);
            }
            headers = tmp;
            c_info.headers = headers;
        }

        let mhnd = curl_multi_init();
        if mhnd.is_null() {
            if !headers.is_null() {
                curl_slist_free_all(headers);
            }
            Rf_error(b"could not create curl handle\0".as_ptr() as *const c_char);
        }
        c_info.mhnd = mhnd;

        // Allocate arrays
        let mut hnd_arr: Vec<*mut CURL> = vec![std::ptr::null_mut(); nurls as usize];
        let mut out_arr: Vec<*mut FILE> = vec![std::ptr::null_mut(); nurls as usize];
        let mut errs_arr: Vec<c_int> = vec![0; nurls as usize];
        let mut tstart_arr: Vec<c_double> = vec![0.0; nurls as usize];

        c_info.hnd = hnd_arr.as_mut_ptr();
        c_info.out = out_arr.as_mut_ptr();
        c_info.errs = errs_arr.as_mut_ptr();
        c_info.tstart = tstart_arr.as_mut_ptr();

        // Set max host connections
        curl_multi_setopt(mhnd, CURLMOPT_MAX_HOST_CONNECTIONS, 6);

        if single == 0 {
            current_time.with(|v| v.set(currentTime()));
        }

        let mut next_url: c_int = 0;

        // Add first URL (mandatory)
        if download_add_one_url(&mut next_url, scmd, mode, quiet, single, &mut c_info) != 0 {
            // No dest files could be opened
            download_cleanup(&mut c_info as *mut download_cleanup_info as *mut c_void);
            return Rf_ScalarInteger(1);
        }

        // Try adding more URLs up to max concurrent
        download_try_add_urls(
            &mut next_url,
            max_concurrent_urls - 1,
            scmd,
            mode,
            quiet,
            single,
            &mut c_info,
        );

        if single == 0 {
            current_time.with(|v| v.set(currentTime()));
        }

        let mut still_running: c_int = 0;
        curl_multi_perform(mhnd, &mut still_running);

        let mut repeats: c_int = 0;
        loop {
            if single == 0 {
                current_time.with(|v| v.set(currentTime()));
            }
            let mut numfds: c_int = 0;
            let mc = curl_multi_wait(mhnd, std::ptr::null_mut(), 0, 100, &mut numfds);
            if mc != CURLM_OK {
                break;
            }
            if numfds == 0 {
                if repeats > 0 {
                    // Sleep 100ms
                    libc::usleep(100000);
                }
                repeats += 1;
            } else {
                repeats = 0;
            }

            if single == 0 {
                current_time.with(|v| v.set(currentTime()));
            }
            curl_multi_perform(mhnd, &mut still_running);

            if single == 0 {
                // Release resources for finished downloads
                download_close_finished(&mut c_info);
            }

            if still_running == 0 {
                if download_add_one_url(&mut next_url, scmd, mode, quiet, single, &mut c_info) == 0
                {
                    still_running += 1;
                }
            }

            download_try_add_urls(
                &mut next_url,
                max_concurrent_urls - still_running,
                scmd,
                mode,
                quiet,
                single,
                &mut c_info,
            );

            if single == 0 {
                current_time.with(|v| v.set(currentTime()));
            }
            curl_multi_perform(mhnd, &mut still_running);

            if still_running == 0 && next_url >= nurls {
                break;
            }
        }

        // Final newline if progress was shown
        if total.with(|v| v.get()) > 0.0 {
            eprintln!();
        }

        // Report single URL download status
        if single != 0 && !hnd_arr[0].is_null() {
            let mut status: c_long = 0;
            curl_easy_getinfo(
                hnd_arr[0],
                CURLINFO_RESPONSE_CODE,
                &mut status as *mut c_long as *mut c_void,
            );

            let mut dl: c_double = 0.0;
            curl_easy_getinfo(
                hnd_arr[0],
                CURLINFO_SIZE_DOWNLOAD,
                &mut dl as *mut c_double as *mut c_void,
            );

            if quiet == 0 && status == 200 {
                if dl > 1024.0 * 1024.0 {
                    eprintln!("downloaded {:.1} MB", dl / 1024.0 / 1024.0);
                } else if dl > 10240.0 {
                    eprintln!("downloaded {} KB", (dl / 1024.0) as c_int);
                } else {
                    eprintln!("downloaded {} bytes", dl as c_int);
                }
            }

            let mut cl: c_double = 0.0;
            curl_easy_getinfo(
                hnd_arr[0],
                CURLINFO_CONTENT_LENGTH_DOWNLOAD,
                &mut cl as *mut c_double as *mut c_void,
            );
            if cl >= 0.0 && (dl - cl).abs() > f64::EPSILON {
                Rf_warning1(b"downloaded length != reported length\0".as_ptr() as *const c_char);
            }
        }

        // Record status before cleanup (easy handle gets cleaned up)
        let mut status: c_long = 0;
        if single != 0 && !hnd_arr[0].is_null() {
            curl_easy_getinfo(
                hnd_arr[0],
                CURLINFO_RESPONSE_CODE,
                &mut status as *mut c_long as *mut c_void,
            );
        }

        download_close_finished(&mut c_info);

        // Count errors
        let mut n_err: c_int = 0;
        for i in 0..nurls {
            if errs_arr[i as usize] != 0 {
                n_err += 1;
            }
        }

        if single == 0 {
            if n_err == nurls {
                Rf_error(b"cannot download any files\0".as_ptr() as *const c_char);
            } else if n_err != 0 {
                Rf_warning1(b"some files were not downloaded\0".as_ptr() as *const c_char);
            }
        } else if n_err != 0 {
            if status != 200 {
                let url = translateChar(STRING_ELT(scmd, 0));
                Rf_error(b"cannot open URL\0".as_ptr() as *const c_char);
                let _ = url;
            } else {
                Rf_error(b"download failed\0".as_ptr() as *const c_char);
            }
        }

        download_cleanup(&mut c_info as *mut download_cleanup_info as *mut c_void);

        let ans = Rf_ScalarInteger(0);
        if nurls > 1 {
            let _ans_guard = protect(ans);
            let sretvals = install(b"retvals\0".as_ptr() as *const c_char);
            let retval = Rf_allocVector(SEXPTYPE::INTSXP, nurls);
            let _retval_guard = protect(retval);
            for i in 0..nurls {
                *INTEGER(retval).add(i as usize) = if errs_arr[i as usize] != 0 { 1 } else { 0 };
            }
            setAttrib(ans, sretvals, retval);
        }
        ans
    }
}

/// in_newCurlUrl - create a new libcurl URL connection
/// Signature: Rconnection in_newCurlUrl(const char *description, const char *mode, SEXP headers, int type)
pub(crate) unsafe fn in_newCurlUrl(
    description: *const c_char,
    mode: *const c_char,
    headers: SEXP,
    r#type: c_int,
) -> *mut c_void {
    unsafe {
        let _ = (description, mode, headers, r#type);
        Rf_error(b"libcurl URL connections are not implemented\0".as_ptr() as *const c_char);
        std::ptr::null_mut()
    }
}

// ============================================================
// Helper function stubs (module-private, matching other modules' patterns)
// ============================================================

/// GetOption1 - get a single option value by symbol name
unsafe fn GetOption1(tag: SEXP) -> SEXP {
    unsafe { crate::main::options::GetOption1(tag) }
}

/// install - intern a symbol name
unsafe fn install(name: *const c_char) -> SEXP {
    unsafe { crate::sexp::symbol::Rf_install(name) }
}

/// asInteger - coerce SEXP to integer
unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe { crate::main::coerce::asInteger(x) }
}

/// asLogical - coerce SEXP to logical
unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe { crate::main::coerce::asLogical(x) }
}

unsafe fn translateChar(x: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(x) }
}

unsafe fn translateCharFP(x: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(x) }
}

/// R_ExpandFileName - expand ~ in file paths
unsafe fn R_ExpandFileName(path: *const c_char) -> *const c_char {
    unsafe { crate::unix::sys_unix::R_ExpandFileName(path) }
}

/// checkArity - check function call arity
unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe {
        crate::mainutils::relop::checkArity(op, args);
    }
}

/// currentTime - get current time in seconds
fn currentTime() -> f64 {
    crate::main::times::currentTime()
}

/// Rf_warning1 - issue a warning (single string)
unsafe fn Rf_warning1(msg: *const c_char) {
    unsafe {
        crate::mainutils::errors::Rf_warning1(msg);
    }
}

/// CAD4R - CDR(CDR(CDR(CDR(args))))
unsafe fn CAD4R(args: SEXP) -> SEXP {
    unsafe { CDR(CDR(CDR(CDR(args)))) }
}

/// isString - check if SEXP is a character vector
unsafe fn isString(x: SEXP) -> c_int {
    unsafe { if TYPEOF(x) == SEXPTYPE::STRSXP { 1 } else { 0 } }
}
