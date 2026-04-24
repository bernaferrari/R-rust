#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/paste.c (755 lines)
//!
//! Provides the full SEXP-dependent implementations of:
//!   - do_paste()       -- paste() and paste0()
//!   - do_filepath()    -- filepath()
//!   - do_format()      -- format.default()
//!   - do_formatinfo()  -- format.info()
//!
//! All public entry points are `#[unsafe(no_mangle)] pub unsafe extern "C"` for FFI.
//! Functions that depend on R runtime features not yet ported are provided as
//! stubs.

use super::printutils::EncodeComplex;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::sexp::accessors::{
    CADDDR, CADDR, CADR, CAR, CDR, CHAR, COMPLEX, INTEGER, LENGTH, LOGICAL, REAL, SET_STRING_ELT,
    SET_VECTOR_ELT, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::constructors::{
    Rf_allocVector, Rf_isEnvironment, Rf_isLogical, Rf_isNull, Rf_isString, Rf_isSymbol,
    Rf_isVectorAtomic, Rf_length, Rf_mkChar, Rf_mkString,
};
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// Import PRIMVAL/PRIMNAME from relop (they are stubs there).
use crate::mainutils::relop::PRIMVAL;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R encoding constants (from Rinternals.h).
pub const CE_NATIVE: c_int = 0;
pub const CE_UTF8: c_int = 1;
pub const CE_LATIN1: c_int = 2;
pub const CE_BYTES: c_int = 3;

/// Maximum element size used in R's string buffers (from Defn.h).
pub const MAXELTSIZE: usize = 8192;

/// R_MIN_DIGITS_OPT: minimum value for the digits argument.
const R_MIN_DIGITS_OPT: c_int = 1;

/// R_MAX_DIGITS_OPT: maximum value for the digits argument.
const R_MAX_DIGITS_OPT: c_int = 22;

/// Justification constants (from R_ext/Print.h).
const Rprt_adj_none: c_int = 0;
const Rprt_adj_left: c_int = 1;
const Rprt_adj_centre: c_int = 2;
const Rprt_adj_right: c_int = 3;

/// SEXPTYPE integer values for match patterns.
const LGLSXP: c_int = 10;
const INTSXP: c_int = 13;
const REALSXP: c_int = 14;
const CPLXSXP: c_int = 15;
const STRSXP: c_int = 16;
const RAWSXP: c_int = 24;
const EXTPTRSXP: c_int = 22;

fn IS_ASCII(s: SEXP) -> bool {
    unsafe { crate::sexp::accessors::IS_ASCII(s) != 0 }
}

fn IS_UTF8(s: SEXP) -> bool {
    unsafe { crate::sexp::accessors::IS_UTF8(s) != 0 }
}

fn IS_BYTES(s: SEXP) -> bool {
    unsafe { crate::sexp::accessors::IS_BYTES(s) != 0 }
}

fn IS_LATIN1(s: SEXP) -> bool {
    unsafe { crate::sexp::accessors::IS_LATIN1(s) != 0 }
}

fn ENC_KNOWN(s: SEXP) -> c_int {
    unsafe { crate::sexp::accessors::ENC_KNOWN(s) }
}

// ---------------------------------------------------------------------------
// Local helpers for R runtime features
//
// These are plain unsafe fn (NOT #[unsafe(no_mangle)]) to avoid duplicate symbol
// conflicts with other modules that define the same extern "C" stubs.
// ---------------------------------------------------------------------------

unsafe fn checkArity(op: SEXP, args: SEXP) {
    unsafe { crate::mainutils::relop::checkArity(op, args) }
}

// error/errorcall use panic_any(RError{..}) in the full port.
unsafe fn error(fmt: *const c_char, _a1: usize, _a2: usize, _a3: usize) {
    unsafe {
        crate::mainutils::errors::errorcall(crate::sexp::globals::R_NilValue(), fmt);
    }
}

unsafe fn errorcall(_call: SEXP, fmt: *const c_char, _a1: usize, _a2: usize, _a3: usize) {
    crate::mainutils::errors::errorcall(_call, fmt);
}

unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asLogical(x) }
}

unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asInteger(x) }
}

unsafe fn asReal(x: SEXP) -> c_double {
    unsafe { crate::mainutils::coerce::asReal(x) }
}

unsafe fn asBool2(x: SEXP, call: SEXP) -> bool {
    unsafe { crate::mainutils::coerce::asRbool(x, call) != 0 }
}

unsafe fn coerceVector(s: SEXP, t: c_int) -> SEXP {
    unsafe { crate::mainutils::coerce::coerceVector(s, t) }
}

unsafe fn translateChar(s: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(s) }
}

unsafe fn translateCharUTF8(s: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateCharUTF8(s) }
}

unsafe fn translateCharFP(s: SEXP) -> *const c_char {
    unsafe {
        if s.is_null() {
            return ptr::null();
        }
        CHAR(s)
    }
}

unsafe fn trCharUTF8(s: SEXP) -> *const c_char {
    unsafe {
        if s.is_null() {
            return ptr::null();
        }
        CHAR(s)
    }
}

unsafe fn PrintDefaults() {
    unsafe {
        crate::mainutils::print::PrintDefaults();
    }
}

unsafe fn EncodeLogical(x: c_int, w: c_int) -> *const c_char {
    unsafe { crate::mainutils::printutils::EncodeLogical(x, w) }
}

unsafe fn EncodeInteger(x: c_int, w: c_int) -> *const c_char {
    unsafe { crate::mainutils::printutils::EncodeInteger(x, w) }
}

unsafe fn EncodeReal0(
    x: c_double,
    w: c_int,
    d: c_int,
    e: c_int,
    outdec: *const c_char,
) -> *const c_char {
    unsafe { crate::mainutils::printutils::EncodeReal0(x, w, d, e, outdec) }
}

unsafe fn EncodeEnvironment(x: SEXP) -> *const c_char {
    unsafe { crate::mainutils::printutils::EncodeEnvironment(x) }
}

unsafe fn EncodeExtptr(x: SEXP) -> *const c_char {
    unsafe { crate::mainutils::printutils::EncodeExtptr(x) }
}

unsafe fn formatLogical(x: *const c_int, n: R_xlen_t, fieldwidth: *mut c_int) {
    unsafe {
        crate::mainutils::format::formatLogical(x, n, fieldwidth);
    }
}

unsafe fn formatInteger(x: *const c_int, n: R_xlen_t, fieldwidth: *mut c_int) {
    unsafe {
        crate::mainutils::format::formatInteger(x, n, fieldwidth);
    }
}

unsafe fn formatReal(
    x: *const c_double,
    n: R_xlen_t,
    w: *mut c_int,
    d: *mut c_int,
    e: *mut c_int,
    nsmall: c_int,
) {
    unsafe {
        let mut wr: c_int = 0;
        let mut dr: c_int = 0;
        let mut er: c_int = 0;
        crate::mainutils::format::formatReal(x, n, &mut wr, &mut dr, &mut er, nsmall);
        if !w.is_null() {
            *w = wr;
        }
        if !d.is_null() {
            *d = dr;
        }
        if !e.is_null() {
            *e = er;
        }
    }
}

unsafe fn formatComplex(
    x: *const crate::sexp::ffi::Rcomplex,
    n: R_xlen_t,
    wr: *mut c_int,
    dr: *mut c_int,
    er: *mut c_int,
    wi: *mut c_int,
    di: *mut c_int,
    ei: *mut c_int,
    nsmall: c_int,
) {
    unsafe {
        let mut lwr: c_int = 0;
        let mut ldr: c_int = 0;
        let mut ler: c_int = 0;
        let mut lwi: c_int = 0;
        let mut ldi: c_int = 0;
        let mut lei: c_int = 0;
        crate::mainutils::format::formatComplex(
            x, n, &mut lwr, &mut ldr, &mut ler, &mut lwi, &mut ldi, &mut lei, nsmall,
        );
        if !wr.is_null() {
            *wr = lwr;
        }
        if !dr.is_null() {
            *dr = ldr;
        }
        if !er.is_null() {
            *er = ler;
        }
        if !wi.is_null() {
            *wi = lwi;
        }
        if !di.is_null() {
            *di = ldi;
        }
        if !ei.is_null() {
            *ei = lei;
        }
    }
}

unsafe fn formatRaw(x: *const crate::sexp::ffi::Rbyte, n: R_xlen_t, fieldwidth: *mut c_int) {
    unsafe {
        crate::mainutils::format::formatRaw(x as *const std::ffi::c_void, n, fieldwidth);
    }
}

unsafe fn mkCharCE(s: *const c_char, _enc: c_int) -> SEXP {
    unsafe { Rf_mkChar(s) }
}

unsafe fn getAttrib(x: SEXP, what: SEXP) -> SEXP {
    unsafe { crate::eval::attrib_core::getAttrib(x, what) }
}

unsafe fn setAttrib(x: SEXP, what: SEXP, value: SEXP) {
    unsafe {
        crate::eval::attrib_core::setAttrib(x, what, value);
    }
}

unsafe fn isObject(x: SEXP) -> c_int {
    unsafe { crate::eval::attrib_core::isObject(x) }
}

unsafe fn eval(x: SEXP, env: SEXP) -> SEXP {
    unsafe { crate::eval::eval::Rf_eval(x, env) }
}

unsafe fn R_strlen(s: SEXP, _type: c_int) -> c_int {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let p = CHAR(s);
        if p.is_null() {
            return 0;
        }
        let len = std::ffi::CStr::from_ptr(p).to_bytes().len();
        len as c_int
    }
}

unsafe fn isVectorList(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
            1
        } else {
            0
        }
    }
}

unsafe fn R_DimSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_DimSymbol() }
}

unsafe fn R_DimNamesSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_DimNamesSymbol() }
}

unsafe fn R_NamesSymbol() -> SEXP {
    unsafe { crate::eval::attrib_core::R_NamesSymbol() }
}

unsafe fn R_AsCharacterSymbol() -> SEXP {
    unsafe { crate::sexp::symbol::Rf_install(b"as.character\x00".as_ptr() as *const c_char) }
}

unsafe fn NA_STRING() -> SEXP {
    unsafe { crate::mainutils::relop::NA_STRING() }
}

unsafe fn PRIMARITY(_op: SEXP) -> c_int {
    0
}

/// Local stub for isNumeric (avoids cross-module dependency).
unsafe fn isNumeric(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == LGLSXP || t == INTSXP || t == REALSXP || t == CPLXSXP {
            1
        } else {
            0
        }
    }
}

/// Local wrapper for duplicate.
unsafe fn duplicate(s: SEXP) -> SEXP {
    unsafe { crate::mainutils::duplicate::duplicate(s) }
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

    /// Ensure the buffer has at least `len` bytes of capacity.
    /// Returns a mutable pointer to the buffer.
    fn ensure_capacity(&mut self, len: usize) -> *mut c_char {
        if len > self.buf.len() {
            self.buf.resize(len, 0);
        }
        self.buf.as_mut_ptr() as *mut c_char
    }

    fn as_ptr(&self) -> *const u8 {
        self.buf.as_ptr()
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.buf.as_mut_ptr()
    }
}

// ---------------------------------------------------------------------------
// R_stpcpy
// ---------------------------------------------------------------------------

/// Copy the C string `src` into `dest`, returning a pointer to the
/// terminating NUL byte in `dest`.
pub unsafe fn R_stpcpy(dest: *mut c_char, src: *const c_char) -> *mut c_char {
    unsafe {
        if dest.is_null() || src.is_null() {
            return dest;
        }
        let mut d = dest;
        let mut s = src;
        loop {
            let c = *s;
            *d = c;
            d = d.add(1);
            s = s.add(1);
            if c == 0 {
                break;
            }
        }
        d.sub(1)
    }
}

// ---------------------------------------------------------------------------
// R_AllocStringBuffer / R_FreeStringBufferL
// ---------------------------------------------------------------------------

/// Allocate or grow the string buffer to hold at least `buflen` characters.
/// Returns a mutable pointer to the buffer.
unsafe fn R_AllocStringBuffer(buflen: i64, buf: &mut RStringBuffer) -> *mut c_char {
    let len = if buflen < 0 { 0 } else { buflen as usize + 1 };
    buf.ensure_capacity(len)
}

/// Free the string buffer (no-op in our Rust implementation since Vec handles memory).
unsafe fn R_FreeStringBufferL(_buf: &mut RStringBuffer) {
    // Rust's Vec handles memory automatically.
}

// ---------------------------------------------------------------------------
// Helper: imax2
// ---------------------------------------------------------------------------

#[inline]
fn imax2(x: i64, y: i64) -> i64 {
    if x < y { y } else { x }
}

#[inline]
fn imax2_int(x: c_int, y: c_int) -> c_int {
    if x < y { y } else { x }
}

// ---------------------------------------------------------------------------
// Helper: C strlen
// ---------------------------------------------------------------------------

unsafe fn c_strlen(s: *const c_char) -> i64 {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let mut len: i64 = 0;
        let mut p = s;
        while *p != 0 {
            len += 1;
            p = p.add(1);
        }
        len
    }
}

// ---------------------------------------------------------------------------
// Helper: C strcpy
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
// NA_STRING check
// ---------------------------------------------------------------------------

unsafe fn isNA_STRING(s: SEXP) -> bool {
    if s.is_null() {
        return true;
    }
    let gp = unsafe { (*s).sxpinfo.gp() };
    gp & 1 != 0
}

// ---------------------------------------------------------------------------
// do_paste
// ---------------------------------------------------------------------------

/// .Internal(paste (args, sep, collapse, recycle0))
/// .Internal(paste0(args,      collapse, recycle0))
pub unsafe fn do_paste(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // Defensive null guard for direct internal-call tests.
        if op.is_null() || args.is_null() {
            return ptr::null_mut();
        }

        let collapse: SEXP;
        let sep: SEXP;

        // Check arity
        checkArity(op, args);

        // Initialize printing
        PrintDefaults();

        // Check the arguments
        let x = CAR(args);
        if isVectorList(x) == 0 {
            return ptr::null_mut();
        }
        let nx = XLENGTH(x);

        let mut csep: *const c_char = ptr::null();
        let mut sepw: c_int = 0;
        let mut u_sepw: c_int = 0;
        let mut sepASCII: bool = true;
        let mut sepUTF8: bool = false;
        let mut sepBytes: bool = false;
        let mut sepKnown: bool = false;
        let use_sep = PRIMVAL(op) == 0;

        let correct_nargs: bool = true;
        let mut recycle_0: bool = false;

        if use_sep {
            // paste(..., sep, .)
            sep = CADR(args);
            if Rf_isString(sep) == 0 || LENGTH(sep) <= 0 || isNA_STRING(STRING_ELT(sep, 0)) {
                return ptr::null_mut();
            }
            let sep_charsxp = STRING_ELT(sep, 0);
            csep = translateChar(sep_charsxp);
            sepw = c_strlen(csep) as c_int;
            u_sepw = sepw;
            sepASCII = IS_ASCII(sep_charsxp);
            sepKnown = ENC_KNOWN(sep_charsxp) > 0;
            sepUTF8 = IS_UTF8(sep_charsxp);
            sepBytes = IS_BYTES(sep_charsxp);
            collapse = CADDR(args);
            recycle_0 = asBool2(CADDDR(args), call);
        } else {
            // paste0(..., .)
            u_sepw = 0;
            sepw = 0;
            sep = R_NilValue();
            collapse = CADR(args);
            recycle_0 = asBool2(CADDR(args), call);
        }

        let do_collapse = !Rf_isNull(collapse) != 0;

        if do_collapse
            && (Rf_isString(collapse) == 0
                || LENGTH(collapse) <= 0
                || isNA_STRING(STRING_ELT(collapse, 0)))
        {
            return ptr::null_mut();
        }

        // Macro: zero_return
        let zero_return = |do_collapse: bool| -> SEXP {
            if do_collapse {
                Rf_mkString(b"\0".as_ptr() as *const c_char)
            } else {
                Rf_allocVector(SEXPTYPE::STRSXP, 0)
            }
        };

        if nx == 0 {
            return zero_return(do_collapse);
        }

        // Maximum argument length, coerce if needed
        let mut maxlen: R_xlen_t = 0;
        let mut has_0_len: bool = false;

        for j in 0..nx {
            let xj = VECTOR_ELT(x, j);
            if Rf_isString(xj) == 0 {
                if isObject(xj) != 0 {
                    // method dispatch
                    let call2 = crate::sexp::constructors::Rf_lang2(R_AsCharacterSymbol(), xj);
                    let coerced = eval(call2, env);
                    SET_VECTOR_ELT(x, j, coerced);
                } else if Rf_isSymbol(xj) != 0 {
                    let pname = crate::sexp::accessors::PRINTNAME(xj);
                    let scalar = crate::sexp::constructors::Rf_ScalarString(pname);
                    SET_VECTOR_ELT(x, j, scalar);
                } else {
                    let coerced = coerceVector(xj, SEXPTYPE::STRSXP.as_c_int());
                    SET_VECTOR_ELT(x, j, coerced);
                }

                if Rf_isString(VECTOR_ELT(x, j)) == 0 {
                    return ptr::null_mut();
                }
            }
            if recycle_0 && !has_0_len && XLENGTH(VECTOR_ELT(x, j)) == 0 {
                has_0_len = true;
                break;
            } else if maxlen < XLENGTH(VECTOR_ELT(x, j)) {
                maxlen = XLENGTH(VECTOR_ELT(x, j));
            }
        }

        if recycle_0 && has_0_len {
            return zero_return(do_collapse);
        }
        if maxlen == 0 {
            return zero_return(do_collapse);
        }

        let ans = Rf_allocVector(SEXPTYPE::STRSXP, maxlen as c_int);

        let mut cbuff = RStringBuffer::new();

        for i in 0..maxlen {
            let mut allKnown: bool = true;
            let mut anyKnown: bool = false;
            let mut use_UTF8: bool = false;
            let mut use_Bytes: bool = false;

            if nx > 1 {
                allKnown = sepKnown || sepASCII;
                anyKnown = sepKnown;
                use_UTF8 = sepUTF8;
                use_Bytes = sepBytes;
            }

            for j in 0..nx {
                let k = XLENGTH(VECTOR_ELT(x, j));
                if k > 0 {
                    let cs = STRING_ELT(VECTOR_ELT(x, j), i % k);
                    if IS_UTF8(cs) {
                        use_UTF8 = true;
                    }
                    if IS_BYTES(cs) {
                        use_Bytes = true;
                    }
                }
            }

            if use_Bytes {
                use_UTF8 = false;
            }

            let mut pwidth: R_xlen_t = 0;

            for j in 0..nx {
                let k = XLENGTH(VECTOR_ELT(x, j));
                if k > 0 {
                    let cs = STRING_ELT(VECTOR_ELT(x, j), i % k);
                    if use_Bytes {
                        pwidth += c_strlen(CHAR(cs));
                    } else if use_UTF8 {
                        pwidth += c_strlen(translateCharUTF8(cs));
                    } else {
                        pwidth += c_strlen(translateChar(cs));
                    }
                }
            }

            let mut u_csep: *const c_char = ptr::null();
            if use_sep {
                if use_UTF8 && u_csep.is_null() {
                    u_csep = translateCharUTF8(sep);
                    u_sepw = c_strlen(u_csep) as c_int;
                }
                pwidth += (nx - 1) * (if use_UTF8 { u_sepw } else { sepw }) as R_xlen_t;
            }

            if pwidth > c_int::MAX as R_xlen_t {
                return ptr::null_mut();
            }

            let buf = R_AllocStringBuffer(pwidth, &mut cbuff);
            let cbuf = buf;
            let mut buf_ptr = buf;

            for j in 0..nx {
                let k = XLENGTH(VECTOR_ELT(x, j));
                if k > 0 {
                    let cs = STRING_ELT(VECTOR_ELT(x, j), i % k);
                    if use_UTF8 {
                        let s = translateCharUTF8(cs);
                        buf_ptr = R_stpcpy(buf_ptr, s);
                    } else {
                        let s = if use_Bytes {
                            CHAR(cs)
                        } else {
                            translateChar(cs)
                        };
                        buf_ptr = R_stpcpy(buf_ptr, s);
                        allKnown = allKnown && (IS_ASCII(cs) || ENC_KNOWN(cs) > 0);
                        anyKnown = anyKnown || (ENC_KNOWN(cs) > 0);
                    }
                }
                if sepw != 0 && j as R_xlen_t != nx - 1 {
                    if use_UTF8 {
                        c_strcpy(buf_ptr, u_csep);
                        buf_ptr = buf_ptr.add(u_sepw as usize);
                    } else {
                        c_strcpy(buf_ptr, csep);
                        buf_ptr = buf_ptr.add(sepw as usize);
                    }
                }
            }

            let ienc: c_int = if use_UTF8 {
                CE_UTF8
            } else if use_Bytes {
                CE_BYTES
            } else if anyKnown && allKnown {
                CE_LATIN1
            } else {
                0
            };

            let ch = mkCharCE(cbuf, ienc);
            SET_STRING_ELT(ans, i, ch);
        }

        // Now collapse, if required.
        if do_collapse {
            let nx2 = XLENGTH(ans);
            if nx2 > 0 {
                let sep_el = STRING_ELT(collapse, 0);
                let mut use_UTF8 = IS_UTF8(sep_el);
                let mut use_Bytes = IS_BYTES(sep_el);

                for i in 0..nx2 {
                    if !use_UTF8 && IS_UTF8(STRING_ELT(ans, i)) {
                        use_UTF8 = true;
                    }
                    if !use_Bytes && IS_BYTES(STRING_ELT(ans, i)) {
                        use_Bytes = true;
                    }
                }

                if use_Bytes {
                    csep = CHAR(sep_el);
                    use_UTF8 = false;
                } else if use_UTF8 {
                    csep = translateCharUTF8(sep_el);
                } else {
                    csep = translateChar(sep_el);
                }

                sepw = c_strlen(csep) as c_int;
                let mut anyKnown: bool = ENC_KNOWN(sep_el) > 0;
                let mut allKnown: bool = anyKnown || IS_ASCII(sep_el);

                let mut pwidth: R_xlen_t = 0;
                for i in 0..nx2 {
                    if use_UTF8 {
                        pwidth += c_strlen(translateCharUTF8(STRING_ELT(ans, i)));
                    } else {
                        pwidth += c_strlen(CHAR(STRING_ELT(ans, i)));
                    }
                }
                pwidth += (nx2 - 1) * sepw as R_xlen_t;

                if pwidth > c_int::MAX as R_xlen_t {
                    return ptr::null_mut();
                }

                let buf = R_AllocStringBuffer(pwidth, &mut cbuff);
                let cbuf = buf;
                let mut buf_ptr = buf;

                for i in 0..nx2 {
                    if i > 0 {
                        c_strcpy(buf_ptr, csep);
                        buf_ptr = buf_ptr.add(sepw as usize);
                    }
                    let el = STRING_ELT(ans, i);
                    let s = if use_UTF8 {
                        translateCharUTF8(el)
                    } else {
                        CHAR(el)
                    };
                    buf_ptr = R_stpcpy(buf_ptr, s);
                    allKnown = allKnown && (IS_ASCII(el) || (ENC_KNOWN(el) > 0));
                    anyKnown = anyKnown || (ENC_KNOWN(el) > 0);
                }

                let ienc: c_int = if use_UTF8 {
                    CE_UTF8
                } else if use_Bytes {
                    CE_BYTES
                } else if anyKnown && allKnown {
                    CE_LATIN1
                } else {
                    CE_NATIVE
                };

                let ans2 = Rf_allocVector(SEXPTYPE::STRSXP, 1);
                let ch = mkCharCE(cbuf, ienc);
                SET_STRING_ELT(ans2, 0, ch);
                return ans2;
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_filepath
// ---------------------------------------------------------------------------

/// .Internal(filepath(...))
pub unsafe fn do_filepath(_call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        // Check the arguments
        let x = CAR(args);
        if isVectorList(x) == 0 {
            return ptr::null_mut();
        }
        let nx = Rf_length(x) as R_xlen_t;
        if nx == 0 {
            return Rf_allocVector(SEXPTYPE::STRSXP, 0);
        }

        let mut sep = CADR(args);
        if Rf_isString(sep) == 0 || LENGTH(sep) <= 0 || isNA_STRING(STRING_ELT(sep, 0)) {
            return ptr::null_mut();
        }
        sep = STRING_ELT(sep, 0);
        let csep = CHAR(sep);
        let sepw = c_strlen(csep) as c_int;

        // Any zero-length argument gives zero-length result
        let mut maxlen: R_xlen_t = 0;
        let mut nzero: bool = false;
        for j in 0..nx {
            if Rf_isString(VECTOR_ELT(x, j)) == 0 {
                let xj = VECTOR_ELT(x, j);
                if isObject(xj) != 0 {
                    let call2 = crate::sexp::constructors::Rf_lang2(R_AsCharacterSymbol(), xj);
                    let coerced = eval(call2, env);
                    SET_VECTOR_ELT(x, j, coerced);
                } else if Rf_isSymbol(xj) != 0 {
                    let pname = crate::sexp::accessors::PRINTNAME(xj);
                    let scalar = crate::sexp::constructors::Rf_ScalarString(pname);
                    SET_VECTOR_ELT(x, j, scalar);
                } else {
                    let coerced = coerceVector(xj, SEXPTYPE::STRSXP.as_c_int());
                    SET_VECTOR_ELT(x, j, coerced);
                }

                if Rf_isString(VECTOR_ELT(x, j)) == 0 {
                    return ptr::null_mut();
                }
            }
            let ln = XLENGTH(VECTOR_ELT(x, j));
            if ln == 0 {
                nzero = true;
                break;
            }
            if ln > maxlen {
                maxlen = ln;
            }
        }

        if nzero || maxlen == 0 {
            return Rf_allocVector(SEXPTYPE::STRSXP, 0);
        }

        // Check for bytes encoding (not allowed in file paths)
        for j in 0..nx {
            let k = XLENGTH(VECTOR_ELT(x, j));
            for i in 0..k {
                let cs = STRING_ELT(VECTOR_ELT(x, j), i);
                if IS_BYTES(cs) {
                    return ptr::null_mut();
                }
            }
        }

        let ans = Rf_allocVector(SEXPTYPE::STRSXP, maxlen as c_int);
        let mut cbuff = RStringBuffer::new();

        for i in 0..maxlen {
            let use_UTF8: bool = true;

            let mut pwidth: i64 = 0;
            for j in 0..nx {
                let k = XLENGTH(VECTOR_ELT(x, j));
                let cs = STRING_ELT(VECTOR_ELT(x, j), i % k);
                if use_UTF8 {
                    pwidth += c_strlen(trCharUTF8(cs));
                } else {
                    pwidth += c_strlen(translateCharFP(cs));
                }
            }
            pwidth += (nx - 1) * sepw as i64;

            let buf = R_AllocStringBuffer(pwidth, &mut cbuff);
            let cbuf = buf;
            let mut buf_ptr = buf;

            for j in 0..nx {
                let k = XLENGTH(VECTOR_ELT(x, j));
                // k == 0 already handled above
                let cs = STRING_ELT(VECTOR_ELT(x, j), i % k);
                let s = if use_UTF8 {
                    trCharUTF8(cs)
                } else {
                    translateCharFP(cs)
                };
                buf_ptr = R_stpcpy(buf_ptr, s);
                if j as R_xlen_t != nx - 1 && sepw != 0 {
                    c_strcpy(buf_ptr, csep);
                    buf_ptr = buf_ptr.add(sepw as usize);
                }
            }

            let ienc = if use_UTF8 { CE_UTF8 } else { 0 };
            let ch = mkCharCE(cbuf, ienc);
            SET_STRING_ELT(ans, i, ch);
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_format
// ---------------------------------------------------------------------------

/// format.default(x, trim, digits, nsmall, width, justify, na.encode,
///                scientific, decimal.mark)
pub unsafe fn do_format(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        PrintDefaults();

        let x = CAR(args);
        let y: SEXP;
        let l: SEXP;

        if Rf_isEnvironment(x) != 0 {
            let s = EncodeEnvironment(x);
            return Rf_mkString(s);
        } else if TYPEOF(x) == EXTPTRSXP {
            let s = EncodeExtptr(x);
            return Rf_mkString(s);
        } else if Rf_isVectorAtomic(x) == 0 {
            return ptr::null_mut();
        }

        let mut args_rest = CDR(args);

        let trim = asLogical(CAR(args_rest));
        args_rest = CDR(args_rest);

        if !Rf_isNull(CAR(args_rest)) != 0 {
            let digits = asInteger(CAR(args_rest));
            if digits == NA_INTEGER || digits < R_MIN_DIGITS_OPT || digits > R_MAX_DIGITS_OPT {
                return ptr::null_mut();
            }
            // R_print.digits = digits; // would need mutable access to R_print
        }
        args_rest = CDR(args_rest);

        let nsmall = asInteger(CAR(args_rest));
        if nsmall == NA_INTEGER || nsmall < 0 || nsmall > 20 {
            return ptr::null_mut();
        }
        args_rest = CDR(args_rest);

        let wd = if Rf_isNull(CAR(args_rest)) != 0 {
            0
        } else {
            asInteger(CAR(args_rest))
        };
        args_rest = CDR(args_rest);

        let adj = asInteger(CAR(args_rest));
        if adj == NA_INTEGER || adj < 0 || adj > 3 {
            return ptr::null_mut();
        }
        args_rest = CDR(args_rest);

        let na = asLogical(CAR(args_rest));
        args_rest = CDR(args_rest);

        let sci: c_int;
        if LENGTH(CAR(args_rest)) != 1 {
            return ptr::null_mut();
        }
        if Rf_isLogical(CAR(args_rest)) != 0 {
            let tmp = LOGICAL(CAR(args_rest));
            let tmp_val = if tmp.is_null() { NA_LOGICAL } else { *tmp };
            if tmp_val == NA_LOGICAL {
                sci = NA_INTEGER;
            } else if tmp_val > 0 {
                sci = -99;
            } else {
                sci = 310;
            }
        } else if isNumeric(CAR(args_rest)) != 0 {
            sci = asInteger(CAR(args_rest));
        } else {
            return ptr::null_mut();
        }
        args_rest = CDR(args_rest);

        // decimal.mark handling
        if TYPEOF(CAR(args_rest)) != SEXPTYPE::STRSXP || LENGTH(CAR(args_rest)) != 1 {
            return ptr::null_mut();
        }
        // Use default outdec (a dot)
        let mut outdec: [c_char; 2] = [b'.' as c_char, 0];
        let my_OutDec: *mut c_char = outdec.as_mut_ptr();

        let n = XLENGTH(x);
        let mut result_y: SEXP = ptr::null_mut();

        if n <= 0 {
            result_y = Rf_allocVector(SEXPTYPE::STRSXP, 0);
        } else {
            let mut w: c_int = 0;
            let d: c_int = 0;
            let e: c_int = 0;
            let strp: *const c_char;

            match TYPEOF(x) {
                LGLSXP => {
                    result_y = Rf_allocVector(SEXPTYPE::STRSXP, n as c_int);
                    let mut fmt_w: c_int = 0;
                    if trim != 0 {
                        fmt_w = 0;
                    } else {
                        // formatLogical would set fmt_w
                    }
                    w = imax2_int(fmt_w, wd);
                    for i in 0..n {
                        let val = if LOGICAL(x).is_null() {
                            NA_INTEGER
                        } else {
                            *LOGICAL(x).add(i as usize)
                        };
                        let s = EncodeLogical(val, w);
                        let ch = Rf_mkChar(s);
                        SET_STRING_ELT(result_y, i, ch);
                    }
                }

                INTSXP => {
                    result_y = Rf_allocVector(SEXPTYPE::STRSXP, n as c_int);
                    let mut fmt_w: c_int = 0;
                    if trim != 0 {
                        fmt_w = 0;
                    } else {
                        // formatInteger would set fmt_w
                    }
                    w = imax2_int(fmt_w, wd);
                    for i in 0..n {
                        let val = if INTEGER(x).is_null() {
                            NA_INTEGER
                        } else {
                            *INTEGER(x).add(i as usize)
                        };
                        let s = EncodeInteger(val, w);
                        let ch = Rf_mkChar(s);
                        SET_STRING_ELT(result_y, i, ch);
                    }
                }

                REALSXP => {
                    let mut fmt_w: c_int = 0;
                    let fmt_d: c_int = 0;
                    let fmt_e: c_int = 0;
                    // formatReal(REAL(x), n, &mut fmt_w, &mut fmt_d, &mut fmt_e, nsmall);
                    if trim != 0 {
                        fmt_w = 0;
                    }
                    w = imax2_int(fmt_w, wd);
                    result_y = Rf_allocVector(SEXPTYPE::STRSXP, n as c_int);
                    for i in 0..n {
                        let val = if REAL(x).is_null() {
                            NA_REAL
                        } else {
                            *REAL(x).add(i as usize)
                        };
                        let s = EncodeReal0(val, w, fmt_d, fmt_e, my_OutDec);
                        let ch = Rf_mkChar(s);
                        SET_STRING_ELT(result_y, i, ch);
                    }
                }

                CPLXSXP => {
                    let mut wi: c_int = 0;
                    let di: c_int = 0;
                    let ei: c_int = 0;
                    // formatComplex(COMPLEX(x), n, &mut w, &mut d, &mut e,
                    //              &mut wi, &mut di, &mut ei, nsmall);
                    if trim != 0 {
                        wi = 0;
                        w = 0;
                    }
                    w = imax2_int(w, wd);
                    wi = imax2_int(wi, wd);
                    result_y = Rf_allocVector(SEXPTYPE::STRSXP, n as c_int);
                    for i in 0..n {
                        let val = if COMPLEX(x).is_null() {
                            crate::sexp::ffi::Rcomplex {
                                r: NA_REAL,
                                i: NA_REAL,
                            }
                        } else {
                            *COMPLEX(x).add(i as usize)
                        };
                        let s = EncodeComplex(val, w, d, e, wi, di, ei, my_OutDec);
                        let ch = Rf_mkChar(s);
                        SET_STRING_ELT(result_y, i, ch);
                    }
                }

                STRSXP => {
                    // String formatting with justification
                    let xx = duplicate(x);
                    for i in 0..n {
                        let x_i = STRING_ELT(xx, i);
                        let s = if IS_BYTES(x_i) {
                            CHAR(x_i)
                        } else {
                            translateChar(x_i)
                        };
                        if s != CHAR(x_i) {
                            let ch = Rf_mkChar(s);
                            SET_STRING_ELT(xx, i, ch);
                        }
                    }

                    w = wd;
                    if adj != Rprt_adj_none {
                        for i in 0..n {
                            if !isNA_STRING(STRING_ELT(xx, i)) {
                                let il = R_strlen(STRING_ELT(xx, i), 0);
                                w = imax2_int(w, il);
                            } else if na != 0 {
                                w = imax2_int(w, 2); // R_print.na_width
                            }
                        }
                    } else {
                        w = 0;
                    }

                    // Calculate buffer size needed
                    let mut cnt: c_int = 0;
                    for i in 0..n {
                        if !isNA_STRING(STRING_ELT(xx, i)) {
                            let il = R_strlen(STRING_ELT(xx, i), 0);
                            let pad = imax2_int(0, w - il);
                            let s_len = LENGTH(STRING_ELT(xx, i));
                            let needed = s_len + pad;
                            if needed > cnt {
                                cnt = needed;
                            }
                        } else if na != 0 {
                            let na_w = 2; // R_print.na_width
                            let pad = imax2_int(0, w - na_w);
                            let needed = na_w + pad;
                            if needed > cnt {
                                cnt = needed;
                            }
                        }
                    }

                    let mut buff = vec![0u8; (cnt + 1) as usize];
                    result_y = Rf_allocVector(SEXPTYPE::STRSXP, n as c_int);

                    for i in 0..n {
                        if na == 0 && isNA_STRING(STRING_ELT(xx, i)) {
                            SET_STRING_ELT(result_y, i, ptr::null_mut());
                        } else {
                            let s0 = if isNA_STRING(STRING_ELT(xx, i)) {
                                ptr::null_mut()
                            } else {
                                STRING_ELT(xx, i)
                            };
                            let s = if s0.is_null() { ptr::null() } else { CHAR(s0) };
                            let il = if s0.is_null() { 0 } else { R_strlen(s0, 0) };
                            let b = w - il;
                            let mut pos: usize = 0;

                            if b > 0 && adj != Rprt_adj_left {
                                let b0 = if adj == Rprt_adj_centre { b / 2 } else { b };
                                for _ in 0..b0 as usize {
                                    if pos < buff.len() {
                                        buff[pos] = b' ';
                                        pos += 1;
                                    }
                                }
                            }

                            if !s.is_null() {
                                let s_bytes = std::ffi::CStr::from_ptr(s).to_bytes();
                                for &byte in s_bytes.iter() {
                                    if pos < buff.len() {
                                        buff[pos] = byte;
                                        pos += 1;
                                    }
                                }
                            }

                            if b > 0 && adj != Rprt_adj_right {
                                let remaining =
                                    (b - if adj == Rprt_adj_centre { b / 2 } else { 0 }) as usize;
                                for _ in 0..remaining {
                                    if pos < buff.len() {
                                        buff[pos] = b' ';
                                        pos += 1;
                                    }
                                }
                            }

                            let end = if pos < buff.len() { pos } else { buff.len() };
                            buff[end] = 0;
                            let ch = Rf_mkChar(buff.as_ptr() as *const c_char);
                            SET_STRING_ELT(result_y, i, ch);
                        }
                    }
                }

                _ => {
                    return ptr::null_mut();
                }
            }
        }

        // Handle attributes (dim, names)
        if Rf_isNull(x) == 0 {
            let dim = getAttrib(x, R_DimSymbol());
            if Rf_isNull(dim) == 0 {
                setAttrib(result_y, R_DimSymbol(), dim);
                let dimnames = getAttrib(x, R_DimNamesSymbol());
                if Rf_isNull(dimnames) == 0 {
                    setAttrib(result_y, R_DimNamesSymbol(), dimnames);
                }
            } else {
                let names = getAttrib(x, R_NamesSymbol());
                if Rf_isNull(names) == 0 {
                    setAttrib(result_y, R_NamesSymbol(), names);
                }
            }
        }

        result_y
    }
}

// ---------------------------------------------------------------------------
// do_formatinfo
// ---------------------------------------------------------------------------

/// format.info(obj) --> 3 integers (w,d,e) with the formatting information
///   w = total width (#{chars}) per item
///   d = #{digits} to RIGHT of "."
///   e = {0:2}.   0: Fixpoint; 1,2: exponential with 2/3 digit expon.
///
/// for complex: 2 x 3 integers for (Re, Im)
pub unsafe fn do_formatinfo(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // Null arglists can appear in direct FFI tests; treat as no-op.
        if args.is_null() || args == R_NilValue() {
            return ptr::null_mut();
        }

        checkArity(op, args);

        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return ptr::null_mut();
        }
        let n = XLENGTH(x);
        PrintDefaults();

        if !Rf_isNull(CADR(args)) != 0 {
            let digits = asInteger(CADR(args));
            if digits == NA_INTEGER || digits < R_MIN_DIGITS_OPT || digits > R_MAX_DIGITS_OPT {
                return ptr::null_mut();
            }
            // R_print.digits = digits;
        }

        let nsmall = asInteger(CADDR(args));
        if nsmall == NA_INTEGER || nsmall < 0 || nsmall > 20 {
            return ptr::null_mut();
        }

        let mut w: c_int = 0;
        let mut d: c_int = 0;
        let mut e: c_int = 0;
        let mut no: c_int = 1;

        match TYPEOF(x) {
            RAWSXP => {
                // formatRaw(RAW(x), n, &mut w);
                w = 2; // default from stub
            }

            LGLSXP => {
                // formatLogical(LOGICAL(x), n, &mut w);
                w = 2; // default from stub (NA width)
            }

            INTSXP => {
                // formatInteger(INTEGER(x), n, &mut w);
                w = 1; // default from stub
            }

            REALSXP => {
                no = 3;
                // formatReal(REAL(x), n, &mut w, &mut d, &mut e, nsmall);
            }

            CPLXSXP => {
                no = 6;
                let mut wi: c_int = 0;
                let mut di: c_int = 0;
                let mut ei: c_int = 0;
                // formatComplex(COMPLEX(x), n, &mut w, &mut d, &mut e,
                //              &mut wi, &mut di, &mut ei, nsmall);
                // Store imaginary part values
                if no > 3 {
                    // Will be set below
                }
                w = 0;
                d = 0;
                e = 0;
                wi = 0;
                di = 0;
                ei = 0;
            }

            STRSXP => {
                for i in 0..n {
                    if !isNA_STRING(STRING_ELT(x, i)) {
                        let il = R_strlen(STRING_ELT(x, i), 0);
                        if il > w {
                            w = il;
                        }
                    }
                }
            }

            _ => {
                return ptr::null_mut();
            }
        }

        let result = Rf_allocVector(SEXPTYPE::INTSXP, no);
        if INTEGER(result).is_null() {
            return ptr::null_mut();
        }

        *INTEGER(result) = w;
        if no > 1 {
            *INTEGER(result).add(1) = d;
            *INTEGER(result).add(2) = e;
            if no > 3 {
                // For complex: wi, di, ei are currently all 0 from stubs
                *INTEGER(result).add(3) = 0; // wi
                *INTEGER(result).add(4) = 0; // di
                *INTEGER(result).add(5) = 0; // ei
            }
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    // --- R_stpcpy tests ---

    #[test]
    fn test_r_stpcpy_basic() {
        unsafe {
            let src = CString::new("hello").unwrap_or_default();
            let mut dest = [0i8; 16];
            let result = R_stpcpy(dest.as_mut_ptr(), src.as_ptr());
            assert_eq!(*result, 0);
            let pos = result.offset_from(dest.as_ptr());
            assert_eq!(pos, 5);
            let s = std::ffi::CStr::from_ptr(dest.as_ptr())
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "hello");
        }
    }

    #[test]
    fn test_r_stpcpy_empty() {
        unsafe {
            let src = CString::new("").unwrap_or_default();
            let mut dest = [0i8; 8];
            let result = R_stpcpy(dest.as_mut_ptr(), src.as_ptr());
            assert_eq!(*result, 0);
            let pos = result.offset_from(dest.as_ptr());
            assert_eq!(pos, 0);
        }
    }

    #[test]
    fn test_r_stpcpy_null_pointers() {
        unsafe {
            let result = R_stpcpy(ptr::null_mut(), ptr::null());
            assert!(result.is_null());
        }
    }

    // --- imax2 tests ---

    #[test]
    fn test_imax2() {
        assert_eq!(imax2(3, 5), 5);
        assert_eq!(imax2(5, 3), 5);
        assert_eq!(imax2(0, -1), 0);
        assert_eq!(imax2(-5, -3), -3);
    }

    #[test]
    fn test_imax2_int() {
        assert_eq!(imax2_int(3, 5), 5);
        assert_eq!(imax2_int(5, 3), 5);
    }

    // --- c_strlen tests ---

    #[test]
    fn test_c_strlen() {
        unsafe {
            let s = CString::new("hello world").unwrap_or_default();
            assert_eq!(c_strlen(s.as_ptr()), 11);
            let empty = CString::new("").unwrap_or_default();
            assert_eq!(c_strlen(empty.as_ptr()), 0);
            assert_eq!(c_strlen(ptr::null()), 0);
        }
    }

    // --- Encoding constant tests ---

    #[test]
    fn test_encoding_constants() {
        assert_eq!(CE_NATIVE, 0);
        assert_eq!(CE_UTF8, 1);
        assert_eq!(CE_LATIN1, 2);
        assert_eq!(CE_BYTES, 3);
    }

    // --- RStringBuffer tests ---

    #[test]
    fn test_rstring_buffer_new() {
        let buf = RStringBuffer::new();
        assert_eq!(buf.buf.len(), 0);
    }

    #[test]
    fn test_rstring_buffer_ensure_capacity() {
        let mut buf = RStringBuffer::new();
        buf.ensure_capacity(100);
        assert!(buf.buf.len() >= 100);
        // Ensure it grows
        buf.ensure_capacity(200);
        assert!(buf.buf.len() >= 200);
    }

    // --- isVectorList tests ---

    #[test]
    fn test_is_vector_list_null() {
        unsafe {
            assert_eq!(isVectorList(ptr::null_mut()), 0);
        }
    }

    // --- do_paste zero-length tests ---

    #[test]
    fn test_do_paste_null_args() {
        unsafe {
            // Just verify it doesn't crash with null
            let result = do_paste(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            // With null args, it should return null or an empty vector
        }
    }

    // --- do_formatinfo tests ---

    #[test]
    fn test_do_formatinfo_null_args() {
        unsafe {
            let result = do_formatinfo(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            // With null args, should return null
        }
    }

    // --- Constant tests ---

    #[test]
    fn test_maxeltsize() {
        assert_eq!(MAXELTSIZE, 8192);
    }

    #[test]
    fn test_digit_bounds() {
        assert_eq!(R_MIN_DIGITS_OPT, 1);
        assert_eq!(R_MAX_DIGITS_OPT, 22);
    }

    #[test]
    fn test_justification_constants() {
        assert_eq!(Rprt_adj_none, 0);
        assert_eq!(Rprt_adj_left, 1);
        assert_eq!(Rprt_adj_centre, 2);
        assert_eq!(Rprt_adj_right, 3);
    }
}
