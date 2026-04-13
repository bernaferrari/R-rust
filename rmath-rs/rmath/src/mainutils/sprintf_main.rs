#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/sprintf.c
//!
//! Implements R's `sprintf()` / `fmt` builtins.
//!
//! Key functions:
//!   - `sprintf_findspec()` -- locate the conversion specifier character in a format
//!   - `sprintf_checkfmt()` -- validate that a format string matches an allowed set
//!   - `do_sprintf()`       -- full sprintf implementation with format strings,
//!     type coercion, NA handling, star-width support,
//!     positional (%n$) arguments, and recycling
//!
//! Note: `findspec` and `checkfmt` are `static` in the original C source;
//! they are exported here with `sprintf_` prefixes so they can be reused
//! by other ports (e.g. formatC in util.c) and by tests.

use std::cell::Cell;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::sexp::accessors::{
    CAR, CDR, INTEGER, LENGTH, LOGICAL, REAL, SET_STRING_ELT, STRING_ELT, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::{Rf_allocVector, Rf_isString, Rf_mkChar};
use crate::sexp::ffi::{ISNAN, NA_INTEGER, NA_LOGICAL, R_FINITE, R_IsNA, R_xlen_t, SEXP};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum line / element size (from R_ext/Print.h MAXELTSIZE).
const MAXLINE: usize = 8192;

/// Maximum number of arguments sprintf will accept.
const MAXNARGS: usize = 100;

/// R encoding constants.
const CE_NATIVE: c_int = 0;
const CE_UTF8: c_int = 1;

/// R_PosInf constant.
const R_PosInf: c_double = f64::INFINITY;

/// R_NegInf constant.
const R_NegInf: c_double = f64::NEG_INFINITY;

/// SEXPTYPE values needed for matching.
const LGLSXP: c_int = 10;
const INTSXP: c_int = 13;
const REALSXP: c_int = 14;
const STRSXP: c_int = 16;
const LANGSXP: c_int = 6;
const SYMSXP: c_int = 1;

// ---------------------------------------------------------------------------
// Local helpers for R runtime features
//
// These are plain unsafe fn (NOT #[unsafe(no_mangle)]) to avoid duplicate symbol
// conflicts with other modules that define the same extern "C" stubs.
// ---------------------------------------------------------------------------

unsafe fn translateChar(s: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(s) }
}

unsafe fn translateCharUTF8(s: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateCharUTF8(s) }
}

unsafe fn getCharCE(s: SEXP) -> c_int {
    unsafe { crate::sexp::accessors::getCharCE(s) }
}

unsafe fn coerceVector(s: SEXP, t: c_int) -> SEXP {
    unsafe { crate::mainutils::coerce::coerceVector(s, t) }
}

unsafe fn mkCharCE(s: *const c_char, _enc: c_int) -> SEXP {
    unsafe { Rf_mkChar(s) }
}

unsafe fn error(fmt: *const c_char) {
    unsafe {
        crate::mainutils::errors::errorcall(crate::sexp::globals::R_NilValue(), fmt);
    }
}

unsafe fn warning(fmt: *const c_char, _a1: usize, _a2: usize) {
    unsafe {
        crate::mainutils::errors::warningcall(crate::sexp::globals::R_NilValue(), fmt);
    }
}

unsafe fn isNA_STRING(s: SEXP) -> bool {
    if s.is_null() {
        return false;
    }
    unsafe { s == crate::mainutils::relop::NA_STRING() }
}

// ---------------------------------------------------------------------------
// R_StringBuffer (Rust equivalent)
// ---------------------------------------------------------------------------

struct RStringBuffer {
    buf: Vec<u8>,
}

impl RStringBuffer {
    fn new() -> Self {
        RStringBuffer { buf: Vec::new() }
    }

    fn ensure_capacity(&mut self, len: usize) -> *mut c_char {
        if len > self.buf.len() {
            self.buf.resize(len, 0);
        }
        self.buf.as_mut_ptr() as *mut c_char
    }
}

thread_local! { static OUTBUFF: Cell<*mut RStringBuffer> = Cell::new(ptr::null_mut()); }

#[repr(transparent)]
struct MutPtr<T>(*mut T);

impl<T> std::ops::Deref for MutPtr<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.0 }
    }
}

impl<T> std::ops::DerefMut for MutPtr<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.0 }
    }
}

unsafe fn get_outbuff() -> MutPtr<RStringBuffer> {
    MutPtr(OUTBUFF.with(|v| {
        if v.get().is_null() {
            let buf = Box::new(RStringBuffer::new());
            v.set(Box::into_raw(buf));
        }
        v.get()
    }))
}

unsafe fn R_AllocStringBuffer(buflen: i64, buf: &mut RStringBuffer) -> *mut c_char {
    let len = if buflen < 0 { 0 } else { buflen as usize + 1 };
    buf.ensure_capacity(len)
}

unsafe fn R_FreeStringBufferL(_buf: &mut RStringBuffer) {}

// ---------------------------------------------------------------------------
// Helper: C strlen
// ---------------------------------------------------------------------------

unsafe fn c_strlen(s: *const c_char) -> usize {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let mut len: usize = 0;
        let mut p = s;
        while *p != 0 {
            len += 1;
            p = p.add(1);
        }
        len
    }
}

// ---------------------------------------------------------------------------
// Helper: C strchr (find first occurrence of char in string)
// ---------------------------------------------------------------------------

unsafe fn c_strchr(s: *const c_char, c: c_int) -> *const c_char {
    unsafe {
        if s.is_null() {
            return ptr::null();
        }
        let mut p = s;
        let target = c as u8;
        loop {
            if *p == 0 {
                return ptr::null();
            }
            if *p as u8 == target {
                return p;
            }
            p = p.add(1);
        }
    }
}

/// strcspn: length of initial segment of s that does NOT contain any chars from reject.
unsafe fn c_strcspn(s: *const c_char, reject: *const c_char) -> usize {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let reject_bytes = if reject.is_null() {
            &[] as &[u8]
        } else {
            std::ffi::CStr::from_ptr(reject).to_bytes()
        };
        let mut len: usize = 0;
        let mut p = s;
        loop {
            if *p == 0 {
                break;
            }
            let ch = *p as u8;
            if reject_bytes.contains(&ch) {
                break;
            }
            len += 1;
            p = p.add(1);
        }
        len
    }
}

// ---------------------------------------------------------------------------
// findspec  (static in C, exported here as sprintf_findspec)
// ---------------------------------------------------------------------------

pub unsafe fn sprintf_findspec(str: *const c_char) -> *const c_char {
    unsafe {
        if str.is_null() {
            return str;
        }
        if *str != b'%' as c_char {
            return str;
        }

        let mut p = str.add(1);
        loop {
            let ch = *p as u8;
            if ch == b'-' || ch == b'+' || ch == b' ' || ch == b'#' || ch == b'.' {
                p = p.add(1);
                continue;
            }
            if ch == b'*' || (ch >= b'0' && ch <= b'9') {
                p = p.add(1);
                continue;
            }
            break;
        }
        p
    }
}

// ---------------------------------------------------------------------------
// checkfmt  (static in C, exported here as sprintf_checkfmt)
// ---------------------------------------------------------------------------

pub unsafe fn sprintf_checkfmt(fmt: *const c_char, pattern: *const c_char) -> bool {
    unsafe {
        if fmt.is_null() || pattern.is_null() {
            return true;
        }
        if *fmt != b'%' as c_char {
            return true;
        }

        let p = sprintf_findspec(fmt);

        let p_cstr = std::ffi::CStr::from_ptr(p);
        let pat_cstr = std::ffi::CStr::from_ptr(pattern);
        let p_bytes = p_cstr.to_bytes();
        let pat_bytes = pat_cstr.to_bytes();

        let mut allowed = [false; 256];
        for &b in pat_bytes {
            allowed[b as usize] = true;
        }

        if p_bytes.is_empty() {
            return true;
        }
        let spec = p_bytes[0] as usize;
        !allowed[spec]
    }
}

// ---------------------------------------------------------------------------
// Helper: c_strcpy
// ---------------------------------------------------------------------------

unsafe fn c_strcpy(dest: *mut c_char, src: *const c_char) {
    unsafe {
        if dest.is_null() || src.is_null() {
            return;
        }
        let mut d = dest;
        let mut s = src;
        loop {
            let c = *s;
            *d = c;
            if c == 0 {
                break;
            }
            d = d.add(1);
            s = s.add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: c_strcat
// ---------------------------------------------------------------------------

unsafe fn c_strcat(dest: *mut c_char, src: *const c_char) {
    unsafe {
        if dest.is_null() || src.is_null() {
            return;
        }
        let mut d = dest;
        while *d != 0 {
            d = d.add(1);
        }
        let mut s = src;
        loop {
            let c = *s;
            *d = c;
            if c == 0 {
                break;
            }
            d = d.add(1);
            s = s.add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// do_sprintf
// ---------------------------------------------------------------------------

pub unsafe fn do_sprintf(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let mut nargs: c_int = 0;
        let mut nfmt: c_int = 0;
        let nprotect: c_int = 0;

        let mut fmt: [c_char; MAXLINE + 1] = [0; MAXLINE + 1];
        let mut fmt2: [c_char; MAXLINE + 10] = [0; MAXLINE + 10];
        let mut bit: [c_char; MAXLINE + 1] = [0; MAXLINE + 1];

        let format = CAR(args);
        if Rf_isString(format) == 0 {
            error(b"'fmt' is not a character vector\0".as_ptr() as *const c_char);
        }
        nfmt = LENGTH(format);
        if nfmt == 0 {
            return Rf_allocVector(STRSXP, 0);
        }
        let args_rest = CDR(args);
        nargs = LENGTH(args_rest);
        if nargs as usize >= MAXNARGS {
            error(b"only 100 arguments are allowed\0".as_ptr() as *const c_char);
        }

        let mut a: [SEXP; MAXNARGS] = [ptr::null_mut(); MAXNARGS];
        let mut used: [bool; MAXNARGS] = [false; MAXNARGS];
        let mut lens: [c_int; MAXNARGS] = [0; MAXNARGS];

        let mut tmp_args = args_rest;
        for i in 0..nargs as usize {
            let t_ai = TYPEOF(CAR(tmp_args));
            a[i] = CAR(tmp_args);
            used[i] = false;
            if t_ai == LANGSXP || t_ai == SYMSXP {
                error(b"invalid type of argument\0".as_ptr() as *const c_char);
            }
            lens[i] = LENGTH(a[i]);
            if lens[i] == 0 {
                return Rf_allocVector(STRSXP, 0);
            }
            tmp_args = CDR(tmp_args);
        }

        let mut maxlen: c_int = nfmt;
        for i in 0..nargs as usize {
            if maxlen < lens[i] {
                maxlen = lens[i];
            }
        }
        if maxlen != 0 && nfmt != 0 && maxlen % nfmt != 0 {
            error(b"arguments cannot be recycled to the same length\0".as_ptr() as *const c_char);
        }
        for i in 0..nargs as usize {
            if lens[i] != 0 && maxlen % lens[i] != 0 {
                error(
                    b"arguments cannot be recycled to the same length\0".as_ptr() as *const c_char,
                );
            }
        }

        let mut outbuff = get_outbuff();

        let mut ans: SEXP = ptr::null_mut();

        for ns in 0..maxlen as R_xlen_t {
            let outputString = R_AllocStringBuffer(0, &mut outbuff);
            *outputString = 0;

            let use_UTF8 = getCharCE(STRING_ELT(format, ns % nfmt as R_xlen_t)) == CE_UTF8;

            let formatString = if use_UTF8 {
                translateCharUTF8(STRING_ELT(format, ns % nfmt as R_xlen_t))
            } else {
                translateChar(STRING_ELT(format, ns % nfmt as R_xlen_t))
            };
            let n = c_strlen(formatString);
            if n > MAXLINE {
                error(
                    b"'fmt' length exceeds maximal format length 8192\0".as_ptr() as *const c_char,
                );
            }

            let mut cur: usize = 0;
            let mut cnt: c_int = 0;

            while cur < n {
                let curFormat = formatString.add(cur);
                let mut ss: *const c_char = ptr::null();
                let chunk: usize;

                if *curFormat == b'%' as c_char {
                    if cur < n - 1 && *curFormat.add(1) == b'%' as c_char {
                        chunk = 2;
                        bit[0] = b'%' as c_char;
                        bit[1] = 0;
                    } else {
                        let spec_chars = b"diosfeEgGxXaA";
                        let mut skip: usize = 0;
                        {
                            let mut p = curFormat.add(1);
                            let mut found = false;
                            while *p != 0 {
                                let ch = *p as u8;
                                if spec_chars.contains(&ch) {
                                    found = true;
                                    break;
                                }
                                skip += 1;
                                p = p.add(1);
                            }
                            if !found {
                                skip = n - cur - 1;
                            }
                        }
                        chunk = skip + 2;
                        if cur + chunk > n {
                            error(b"unrecognised format specification\0".as_ptr() as *const c_char);
                        }

                        for j in 0..chunk {
                            fmt[j] = *curFormat.add(j);
                        }
                        fmt[chunk] = 0;

                        let mut nthis: c_int = -1;

                        let fmt_len = c_strlen(fmt.as_ptr());
                        if fmt_len > 3 {
                            let c1 = fmt[1] as u8;
                            if c1 >= b'1' && c1 <= b'9' {
                                let mut v = (c1 - b'0') as c_int;
                                if fmt[2] == b'$' as c_char {
                                    if v > nargs as c_int {
                                        error(b"reference to non-existent argument\0".as_ptr()
                                            as *const c_char);
                                    }
                                    nthis = v - 1;
                                    let move_len = fmt_len - 2;
                                    let mut j = 1usize;
                                    while j <= move_len {
                                        fmt[j] = fmt[j + 2];
                                        j += 1;
                                    }
                                    fmt[j] = 0;
                                } else {
                                    let c2 = fmt[2] as u8;
                                    if c2 >= b'0'
                                        && c2 <= b'9'
                                        && fmt_len > 3
                                        && fmt[3] == b'$' as c_char
                                    {
                                        v = 10 * v + (c2 - b'0') as c_int;
                                        if v > nargs as c_int {
                                            error(b"reference to non-existent argument\0".as_ptr()
                                                as *const c_char);
                                        }
                                        nthis = v - 1;
                                        let move_len = fmt_len - 3;
                                        let mut j = 1usize;
                                        while j <= move_len {
                                            fmt[j] = fmt[j + 3];
                                            j += 1;
                                        }
                                        fmt[j] = 0;
                                    }
                                }
                            }
                        }

                        let mut has_star = false;
                        let mut star_arg: c_int = 0;

                        let mut star_idx: Option<usize> = None;
                        for (idx, &ch) in fmt.iter().enumerate() {
                            if ch == 0 {
                                break;
                            }
                            if ch == b'*' as c_char {
                                star_idx = Some(idx);
                                break;
                            }
                        }

                        if let Some(si) = star_idx {
                            let mut nstar: c_int = -1;
                            let star_len = c_strlen(fmt.as_ptr().add(si));
                            if star_len > 3 {
                                let c1 = fmt[si + 1] as u8;
                                if c1 >= b'1' && c1 <= b'9' {
                                    let mut v = (c1 - b'0') as c_int;
                                    if fmt[si + 2] == b'$' as c_char {
                                        if v > nargs as c_int {
                                            error(b"reference to non-existent argument\0".as_ptr()
                                                as *const c_char);
                                        }
                                        nstar = v - 1;
                                        let move_len = star_len - 2;
                                        let mut j = 1usize;
                                        while j <= move_len {
                                            fmt[si + j] = fmt[si + j + 2];
                                            j += 1;
                                        }
                                        fmt[si + j] = 0;
                                    } else {
                                        let c2 = fmt[si + 2] as u8;
                                        if c2 >= b'0'
                                            && c2 <= b'9'
                                            && star_len > 3
                                            && fmt[si + 3] == b'$' as c_char
                                        {
                                            v = 10 * v + (c2 - b'0') as c_int;
                                            if v > nargs as c_int {
                                                error(
                                                    b"reference to non-existent argument\0".as_ptr()
                                                        as *const c_char,
                                                );
                                            }
                                            nstar = v - 1;
                                            let move_len = star_len - 3;
                                            let mut j = 1usize;
                                            while j <= move_len {
                                                fmt[si + j] = fmt[si + j + 3];
                                                j += 1;
                                            }
                                            fmt[si + j] = 0;
                                        }
                                    }
                                }
                            }

                            if nstar < 0 {
                                if cnt >= nargs {
                                    error(b"too few arguments\0".as_ptr() as *const c_char);
                                }
                                nstar = cnt;
                                cnt += 1;
                            }

                            {
                                let mut found_second = false;
                                for j in (si + 1)..fmt.len() {
                                    if fmt[j] == 0 {
                                        break;
                                    }
                                    if fmt[j] == b'*' as c_char {
                                        found_second = true;
                                        break;
                                    }
                                }
                                if found_second {
                                    error(b"at most one asterisk '*' is supported in each conversion specification\0".as_ptr() as *const c_char);
                                }
                            }

                            let _this = a[nstar as usize];
                            used[nstar as usize] = true;

                            if ns == 0 && TYPEOF(_this) == REALSXP {
                                a[nstar as usize] = coerceVector(_this, INTSXP);
                            }

                            let this_type = TYPEOF(a[nstar as usize]);
                            let this_len = LENGTH(a[nstar as usize]);
                            if this_type != INTSXP
                                || this_len < 1
                                || *INTEGER(a[nstar as usize])
                                    .add((ns % this_len as R_xlen_t) as usize)
                                    == NA_INTEGER
                            {
                                error(
                                    b"argument for '*' conversion specification must be a number\0"
                                        .as_ptr()
                                        as *const c_char,
                                );
                            }
                            star_arg = *INTEGER(a[nstar as usize])
                                .add((ns % this_len as R_xlen_t) as usize);
                            has_star = true;
                        }

                        let fmt_len_now = c_strlen(fmt.as_ptr());
                        if fmt_len_now > 0 && fmt[fmt_len_now - 1] == b'%' as c_char {
                            if has_star {
                                let nc = libc::snprintf(
                                    bit.as_mut_ptr(),
                                    MAXLINE + 1,
                                    fmt.as_ptr(),
                                    star_arg,
                                );
                                if nc > MAXLINE as c_int {
                                    error(
                                        b"required resulting string length exceeds maximal 8192\0"
                                            .as_ptr()
                                            as *const c_char,
                                    );
                                }
                            } else {
                                c_strcpy(bit.as_mut_ptr(), fmt.as_ptr());
                            }
                        } else {
                            let did_this = false;

                            if nthis < 0 {
                                if cnt >= nargs {
                                    error(b"too few arguments\0".as_ptr() as *const c_char);
                                }
                                nthis = cnt;
                                cnt += 1;
                            }

                            let mut _this = a[nthis as usize];
                            used[nthis as usize] = true;

                            let fmtp: *const c_char;
                            if has_star {
                                let mut q = fmt2.as_mut_ptr();
                                let mut p = fmt.as_ptr();
                                while *p != 0 {
                                    if *p == b'*' as c_char {
                                        let star_str = format!("{}", star_arg);
                                        let star_bytes = star_str.as_bytes();
                                        for &b in star_bytes.iter() {
                                            *q = b as c_char;
                                            q = q.add(1);
                                        }
                                    } else {
                                        *q = *p;
                                        q = q.add(1);
                                    }
                                    p = p.add(1);
                                }
                                *q = 0;
                                let nf = c_strlen(fmt2.as_ptr());
                                if nf > MAXLINE {
                                    error(
                                        b"'fmt' length exceeds maximal format length 8192\0"
                                            .as_ptr()
                                            as *const c_char,
                                    );
                                }
                                fmtp = fmt2.as_ptr();
                            } else {
                                fmtp = fmt.as_ptr();
                            }

                            let thislen = LENGTH(_this);
                            if thislen == 0 {
                                error(b"coercion has changed vector length to 0\0".as_ptr()
                                    as *const c_char);
                            }

                            if ns == 0 {
                                let spec = *sprintf_findspec(fmtp);
                                match spec as u8 {
                                    b'd' | b'i' | b'o' | b'x' | b'X' => {
                                        if TYPEOF(_this) == REALSXP {
                                            let mut exactly_integer = true;
                                            let n_vals = XLENGTH(_this);
                                            for ii in 0..n_vals {
                                                let r = *REAL(_this).add(ii as usize);
                                                if R_IsNA(r) {
                                                    continue;
                                                }
                                                if !R_FINITE(r) || (r as c_int as c_double) != r {
                                                    exactly_integer = false;
                                                    break;
                                                }
                                            }
                                            if exactly_integer {
                                                _this = coerceVector(_this, INTSXP);
                                                a[nthis as usize] = _this;
                                            }
                                        }
                                    }
                                    b'a' | b'A' | b'e' | b'f' | b'g' | b'E' | b'G' => {
                                        if TYPEOF(_this) != REALSXP && TYPEOF(_this) != STRSXP {
                                            _this = coerceVector(_this, REALSXP);
                                            a[nthis as usize] = _this;
                                            let new_len = LENGTH(_this);
                                            if new_len == 0 {
                                                error(
                                                    b"coercion has changed vector length to 0\0"
                                                        .as_ptr()
                                                        as *const c_char,
                                                );
                                            }
                                            lens[nthis as usize] = new_len;
                                        }
                                    }
                                    b's' => {
                                        if TYPEOF(_this) != STRSXP {
                                            _this = coerceVector(_this, STRSXP);
                                            a[nthis as usize] = _this;
                                            let new_len = LENGTH(_this);
                                            if new_len == 0 {
                                                error(
                                                    b"coercion has changed vector length to 0\0"
                                                        .as_ptr()
                                                        as *const c_char,
                                                );
                                            }
                                            lens[nthis as usize] = new_len;
                                        }
                                    }
                                    _ => {}
                                }
                            }

                            let thislen = LENGTH(_this);

                            match TYPEOF(_this) {
                                LGLSXP => {
                                    let x =
                                        *LOGICAL(_this).add((ns % thislen as R_xlen_t) as usize);
                                    if sprintf_checkfmt(fmtp, b"di\0".as_ptr() as *const c_char) {
                                        error(b"invalid format '%s'; use format %d or %i for logical objects\0".as_ptr() as *const c_char);
                                    }
                                    if x == NA_LOGICAL {
                                        let fmtp_len = c_strlen(fmtp);
                                        *fmt.as_mut_ptr().add(fmtp_len - 1) = b's' as c_char;
                                        *fmt.as_mut_ptr().add(fmtp_len) = 0;
                                        let nc = libc::snprintf(
                                            bit.as_mut_ptr(),
                                            MAXLINE + 1,
                                            fmt.as_ptr(),
                                            b"NA\0".as_ptr(),
                                        );
                                        if nc > MAXLINE as c_int {
                                            error(b"required resulting string length exceeds maximal 8192\0".as_ptr() as *const c_char);
                                        }
                                    } else {
                                        let nc =
                                            libc::snprintf(bit.as_mut_ptr(), MAXLINE + 1, fmtp, x);
                                        if nc > MAXLINE as c_int {
                                            error(b"required resulting string length exceeds maximal 8192\0".as_ptr() as *const c_char);
                                        }
                                    }
                                }
                                INTSXP => {
                                    let x =
                                        *INTEGER(_this).add((ns % thislen as R_xlen_t) as usize);
                                    if sprintf_checkfmt(fmtp, b"dioxX\0".as_ptr() as *const c_char)
                                    {
                                        error(b"invalid format '%s'; use format %d, %i, %o, %x or %X for integer objects\0".as_ptr() as *const c_char);
                                    }
                                    if x == NA_INTEGER {
                                        let fmtp_len = c_strlen(fmtp);
                                        *fmt.as_mut_ptr().add(fmtp_len - 1) = b's' as c_char;
                                        *fmt.as_mut_ptr().add(fmtp_len) = 0;
                                        let nc = libc::snprintf(
                                            bit.as_mut_ptr(),
                                            MAXLINE + 1,
                                            fmt.as_ptr(),
                                            b"NA\0".as_ptr(),
                                        );
                                        if nc > MAXLINE as c_int {
                                            error(b"required resulting string length exceeds maximal 8192\0".as_ptr() as *const c_char);
                                        }
                                    } else {
                                        let nc =
                                            libc::snprintf(bit.as_mut_ptr(), MAXLINE + 1, fmtp, x);
                                        if nc > MAXLINE as c_int {
                                            error(b"required resulting string length exceeds maximal 8192\0".as_ptr() as *const c_char);
                                        }
                                    }
                                }
                                REALSXP => {
                                    let x = *REAL(_this).add((ns % thislen as R_xlen_t) as usize);
                                    if sprintf_checkfmt(
                                        fmtp,
                                        b"aAfeEgG\0".as_ptr() as *const c_char,
                                    ) {
                                        error(b"invalid format '%s'; use format %f, %e, %g or %a for numeric objects\0".as_ptr() as *const c_char);
                                    }
                                    if R_FINITE(x) {
                                        let nc =
                                            libc::snprintf(bit.as_mut_ptr(), MAXLINE + 1, fmtp, x);
                                        if nc > MAXLINE as c_int {
                                            error(b"required resulting string length exceeds maximal 8192\0".as_ptr() as *const c_char);
                                        }
                                    } else {
                                        let dot = c_strchr(fmtp, b'.' as c_int);
                                        let fmtp_buf = fmt.as_mut_ptr();
                                        let fmtp_len = c_strlen(fmtp);
                                        if !dot.is_null() {
                                            let dot_off = dot.offset_from(fmtp) as usize;
                                            *fmtp_buf.add(dot_off) = b's' as c_char;
                                            *fmtp_buf.add(dot_off + 1) = 0;
                                        } else {
                                            *fmtp_buf.add(fmtp_len - 1) = b's' as c_char;
                                            *fmtp_buf.add(fmtp_len) = 0;
                                        }

                                        let na_str: *const c_char = if R_IsNA(x) {
                                            if c_strcspn(fmtp_buf, b" \0".as_ptr() as *const c_char)
                                                < c_strlen(fmtp_buf)
                                            {
                                                b" NA\0".as_ptr() as *const c_char
                                            } else {
                                                b"NA\0".as_ptr() as *const c_char
                                            }
                                        } else if ISNAN(x) {
                                            if c_strcspn(fmtp_buf, b" \0".as_ptr() as *const c_char)
                                                < c_strlen(fmtp_buf)
                                            {
                                                b" NaN\0".as_ptr() as *const c_char
                                            } else {
                                                b"NaN\0".as_ptr() as *const c_char
                                            }
                                        } else if x == R_PosInf {
                                            if c_strcspn(fmtp_buf, b"+\0".as_ptr() as *const c_char)
                                                < c_strlen(fmtp_buf)
                                            {
                                                b"+Inf\0".as_ptr() as *const c_char
                                            } else if c_strcspn(
                                                fmtp_buf,
                                                b" \0".as_ptr() as *const c_char,
                                            ) < c_strlen(fmtp_buf)
                                            {
                                                b" Inf\0".as_ptr() as *const c_char
                                            } else {
                                                b"Inf\0".as_ptr() as *const c_char
                                            }
                                        } else {
                                            b"-Inf\0".as_ptr() as *const c_char
                                        };

                                        let nc = libc::snprintf(
                                            bit.as_mut_ptr(),
                                            MAXLINE + 1,
                                            fmtp_buf,
                                            na_str,
                                        );
                                        if nc > MAXLINE as c_int {
                                            error(b"required resulting string length exceeds maximal 8192\0".as_ptr() as *const c_char);
                                        }
                                    }
                                }
                                STRSXP => {
                                    if sprintf_checkfmt(fmtp, b"s\0".as_ptr() as *const c_char) {
                                        error(b"invalid format '%s'; use format %s for character objects\0".as_ptr() as *const c_char);
                                    }

                                    ss = if use_UTF8 {
                                        translateCharUTF8(STRING_ELT(
                                            _this,
                                            ns % thislen as R_xlen_t,
                                        ))
                                    } else {
                                        translateChar(STRING_ELT(_this, ns % thislen as R_xlen_t))
                                    };
                                    if *fmtp.add(1) != b's' as c_char {
                                        if c_strlen(ss) > MAXLINE {
                                            warning(
                                            b"likely truncation of character string to %d characters\0"
                                                .as_ptr() as *const c_char,
                                            MAXLINE,
                                            0,
                                        );
                                        }
                                        let nc =
                                            libc::snprintf(bit.as_mut_ptr(), MAXLINE + 1, fmtp, ss);
                                        if nc > MAXLINE as c_int {
                                            error(b"required resulting string length exceeds maximal 8192\0".as_ptr() as *const c_char);
                                        }
                                        bit[MAXLINE] = 0;
                                        ss = ptr::null();
                                    }
                                }
                                _ => {
                                    error(b"unsupported type\0".as_ptr() as *const c_char);
                                }
                            }
                        }
                    }
                } else {
                    let ch = c_strchr(curFormat, b'%' as c_int);
                    chunk = if !ch.is_null() {
                        (ch as usize) - (curFormat as usize)
                    } else {
                        c_strlen(curFormat)
                    };
                    for j in 0..chunk {
                        bit[j] = *curFormat.add(j);
                    }
                    bit[chunk] = 0;
                }

                let append_str = if !ss.is_null() { ss } else { bit.as_ptr() };
                let outputString = R_AllocStringBuffer(
                    (c_strlen(outputString) + c_strlen(append_str)) as i64,
                    &mut outbuff,
                );
                c_strcat(outputString, append_str);

                cur += chunk;
            }

            if ns == 0 {
                ans = Rf_allocVector(STRSXP, maxlen);
            }
            let ienc = if use_UTF8 { CE_UTF8 } else { CE_NATIVE };
            SET_STRING_ELT(ans, ns, mkCharCE(outputString, ienc));
        }

        let mut nunused: c_int = 0;
        for i in 0..nargs as usize {
            if !used[i] {
                nunused += 1;
            }
        }
        if nunused > 0 {
            if nfmt == 1 {
                let f = translateChar(STRING_ELT(format, 0));
                if nunused == 1 {
                    warning(
                        b"one argument not used by format '%s'\0".as_ptr() as *const c_char,
                        0,
                        0,
                    );
                } else {
                    warning(
                        b"%d arguments not used by format '%s'\0".as_ptr() as *const c_char,
                        nunused as usize,
                        0,
                    );
                }
                let _ = f;
            } else {
                if nunused == 1 {
                    warning(
                        b"one argument not used by format\0".as_ptr() as *const c_char,
                        0,
                        0,
                    );
                } else {
                    warning(
                        b"%d arguments not used by format\0".as_ptr() as *const c_char,
                        nunused as usize,
                        0,
                    );
                }
            }
        }

        R_FreeStringBufferL(&mut outbuff);
        ans
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn test_ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("test setup failed: {err}"),
        }
    }

    #[test]
    fn test_findspec_percent_d() {
        unsafe {
            let fmt = test_ok(CString::new("%d"));
            let spec = sprintf_findspec(fmt.as_ptr());
            assert_eq!(*spec, b'd' as c_char);
        }
    }

    #[test]
    fn test_findspec_percent_02f() {
        unsafe {
            let fmt = test_ok(CString::new("%.2f"));
            let spec = sprintf_findspec(fmt.as_ptr());
            assert_eq!(*spec, b'f' as c_char);
        }
    }

    #[test]
    fn test_findspec_percent_10s() {
        unsafe {
            let fmt = test_ok(CString::new("%10s"));
            let spec = sprintf_findspec(fmt.as_ptr());
            assert_eq!(*spec, b's' as c_char);
        }
    }

    #[test]
    fn test_findspec_percent_plus_d() {
        unsafe {
            let fmt = test_ok(CString::new("%+d"));
            let spec = sprintf_findspec(fmt.as_ptr());
            assert_eq!(*spec, b'd' as c_char);
        }
    }

    #[test]
    fn test_findspec_percent_minus_10_dot_2f() {
        unsafe {
            let fmt = test_ok(CString::new("%-10.2f"));
            let spec = sprintf_findspec(fmt.as_ptr());
            assert_eq!(*spec, b'f' as c_char);
        }
    }

    #[test]
    fn test_findspec_percent_hash_x() {
        unsafe {
            let fmt = test_ok(CString::new("%#x"));
            let spec = sprintf_findspec(fmt.as_ptr());
            assert_eq!(*spec, b'x' as c_char);
        }
    }

    #[test]
    fn test_findspec_percent_star_d() {
        unsafe {
            let fmt = test_ok(CString::new("%*d"));
            let spec = sprintf_findspec(fmt.as_ptr());
            assert_eq!(*spec, b'd' as c_char);
        }
    }

    #[test]
    fn test_findspec_percent_zero_10_dot_3_e() {
        unsafe {
            let fmt = test_ok(CString::new("%010.3e"));
            let spec = sprintf_findspec(fmt.as_ptr());
            assert_eq!(*spec, b'e' as c_char);
        }
    }

    #[test]
    fn test_findspec_not_percent() {
        unsafe {
            let fmt = test_ok(CString::new("hello"));
            let spec = sprintf_findspec(fmt.as_ptr());
            assert_eq!(*spec, b'h' as c_char);
        }
    }

    #[test]
    fn test_findspec_null() {
        unsafe {
            let spec = sprintf_findspec(ptr::null());
            assert!(spec.is_null());
        }
    }

    #[test]
    fn test_checkfmt_valid_d() {
        unsafe {
            let fmt = test_ok(CString::new("%d"));
            let pat = test_ok(CString::new("di"));
            assert_eq!(sprintf_checkfmt(fmt.as_ptr(), pat.as_ptr()), false);
        }
    }

    #[test]
    fn test_checkfmt_invalid_d_for_string() {
        unsafe {
            let fmt = test_ok(CString::new("%d"));
            let pat = test_ok(CString::new("s"));
            assert_eq!(sprintf_checkfmt(fmt.as_ptr(), pat.as_ptr()), true);
        }
    }

    #[test]
    fn test_checkfmt_valid_s() {
        unsafe {
            let fmt = test_ok(CString::new("%s"));
            let pat = test_ok(CString::new("s"));
            assert_eq!(sprintf_checkfmt(fmt.as_ptr(), pat.as_ptr()), false);
        }
    }

    #[test]
    fn test_checkfmt_valid_f() {
        unsafe {
            let fmt = test_ok(CString::new("%.2f"));
            let pat = test_ok(CString::new("aAfeEgG"));
            assert_eq!(sprintf_checkfmt(fmt.as_ptr(), pat.as_ptr()), false);
        }
    }

    #[test]
    fn test_checkfmt_invalid_f_for_int() {
        unsafe {
            let fmt = test_ok(CString::new("%f"));
            let pat = test_ok(CString::new("dioxX"));
            assert_eq!(sprintf_checkfmt(fmt.as_ptr(), pat.as_ptr()), true);
        }
    }

    #[test]
    fn test_checkfmt_null_fmt() {
        unsafe {
            let pat = test_ok(CString::new("s"));
            assert_eq!(sprintf_checkfmt(ptr::null(), pat.as_ptr()), true);
        }
    }

    #[test]
    fn test_checkfmt_null_pattern() {
        unsafe {
            let fmt = test_ok(CString::new("%s"));
            assert_eq!(sprintf_checkfmt(fmt.as_ptr(), ptr::null()), true);
        }
    }

    #[test]
    fn test_checkfmt_not_format() {
        unsafe {
            let fmt = test_ok(CString::new("hello"));
            let pat = test_ok(CString::new("s"));
            assert_eq!(sprintf_checkfmt(fmt.as_ptr(), pat.as_ptr()), true);
        }
    }

    #[test]
    fn test_c_strlen() {
        unsafe {
            let s = test_ok(CString::new("hello"));
            assert_eq!(c_strlen(s.as_ptr()), 5);
            let empty = test_ok(CString::new(""));
            assert_eq!(c_strlen(empty.as_ptr()), 0);
        }
    }

    #[test]
    fn test_c_strchr_found() {
        unsafe {
            let s = test_ok(CString::new("hello"));
            let result = c_strchr(s.as_ptr(), b'l' as c_int);
            assert_eq!(*result, b'l' as c_char);
            assert_eq!(result.offset_from(s.as_ptr()), 2);
        }
    }

    #[test]
    fn test_c_strchr_not_found() {
        unsafe {
            let s = test_ok(CString::new("hello"));
            let result = c_strchr(s.as_ptr(), b'z' as c_int);
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_c_strcspn() {
        unsafe {
            let s = test_ok(CString::new("hello world"));
            let reject = test_ok(CString::new(" w"));
            assert_eq!(c_strcspn(s.as_ptr(), reject.as_ptr()), 5);
        }
    }

    #[test]
    fn test_c_strcspn_no_match() {
        unsafe {
            let s = test_ok(CString::new("hello"));
            let reject = test_ok(CString::new("xyz"));
            assert_eq!(c_strcspn(s.as_ptr(), reject.as_ptr()), 5);
        }
    }

    #[test]
    fn test_maxline() {
        assert_eq!(MAXLINE, 8192);
    }

    #[test]
    fn test_maxnargs() {
        assert_eq!(MAXNARGS, 100);
    }

    #[test]
    fn test_rstring_buffer() {
        let mut buf = RStringBuffer::new();
        assert_eq!(buf.buf.len(), 0);
        buf.ensure_capacity(100);
        assert!(buf.buf.len() >= 100);
        buf.ensure_capacity(200);
        assert!(buf.buf.len() >= 200);
    }
}
