#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/printarray.c -- matrix and array printing.
//!
//! Provides printMatrix() and printArray() for displaying R matrices and arrays.
//! Also provides format*Matrix() functions for determining column widths.

use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::sexp::accessors::{
    COMPLEX, INTEGER, LENGTH, LOGICAL, RAW, REAL, STRING_ELT, TYPEOF, VECTOR_ELT,
};
use crate::sexp::ffi::R_xlen_t;
use crate::sexp::ffi::{R_IsNA, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Minimum label offset for row names.
const R_MIN_LBLOFF: c_int = 2;

/// Left adjustment constant.
const Rprt_adj_left: c_int = 0;

/// Right adjustment constant.
const Rprt_adj_right: c_int = 1;

/// LGLSXP type constant.
const LGLSXP: c_int = SEXPTYPE::LGLSXP.0;
/// INTSXP type constant.
const INTSXP: c_int = SEXPTYPE::INTSXP.0;
/// REALSXP type constant.
const REALSXP: c_int = SEXPTYPE::REALSXP.0;
/// CPLXSXP type constant.
const CPLXSXP: c_int = SEXPTYPE::CPLXSXP.0;
/// STRSXP type constant.
const STRSXP: c_int = SEXPTYPE::STRSXP.0;
/// RAWSXP type constant.
const RAWSXP: c_int = SEXPTYPE::RAWSXP.0;

/// OutDec: decimal separator character (always '.' in this port).
static OUT_DEC: c_char = b'.' as c_char;

/// Rbyte type alias.
type Rbyte = u8;

// ---------------------------------------------------------------------------
// R_print full parameters -- imported from printvector
// ---------------------------------------------------------------------------

use crate::main::printvector::R_PrintData;

/// Get the global R_print parameters.
unsafe fn get_R_print_full() -> &'static R_PrintData {
    unsafe { crate::main::printvector::get_R_PrintData() }
}

// ---------------------------------------------------------------------------
// Functions from printutils (non-SEXP taking ones)
// ---------------------------------------------------------------------------

use crate::main::printutils::IndexWidth_xlen as IndexWidth;
use crate::main::printutils::{
    EncodeComplex, EncodeInteger, EncodeLogical, EncodeRaw, EncodeReal0, Rprt_adj,
};

// ---------------------------------------------------------------------------
// Local wrappers for printutils functions that take SEXP
//
// printutils.rs defines its own SEXP type (*mut SEXPREC) which is different
// from sexp::ffi::SEXP (*mut SexprecCore). Since Rstrlen and EncodeString
// are stubs (returning 0 and "" respectively), we define local wrappers.
// ---------------------------------------------------------------------------

/// Local Rstrlen stub: returns display width of a CHARSXP.
unsafe fn local_Rstrlen(_s: SEXP, _quote: c_int) -> c_int {
    // Stub: return 0. The real implementation would compute escaped display width.
    0
}

/// Local EncodeString stub: returns encoded string for a CHARSXP.
static EMPTY_CSTR: [u8; 1] = [0];

unsafe fn local_EncodeString(
    _s: SEXP,
    _w: c_int,
    _quote: c_int,
    _justify: Rprt_adj,
) -> *const c_char {
    EMPTY_CSTR.as_ptr() as *const c_char
}

// ---------------------------------------------------------------------------
// Functions from format (extern "C" linkage)
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn formatLogical(x: *const c_int, n: R_xlen_t, fieldwidth: *mut c_int);
    fn formatInteger(x: *const c_int, n: R_xlen_t, fieldwidth: *mut c_int);
    fn formatReal(
        x: *const f64,
        n: R_xlen_t,
        w: *mut c_int,
        d: *mut c_int,
        e: *mut c_int,
        nsmall: c_int,
    );
    fn formatComplex(
        x: *const Rcomplex,
        n: R_xlen_t,
        wr: *mut c_int,
        dr: *mut c_int,
        er: *mut c_int,
        wi: *mut c_int,
        di: *mut c_int,
        ei: *mut c_int,
        nsmall: c_int,
    );
    fn formatRaw(x: *const c_void, n: R_xlen_t, fieldwidth: *mut c_int);
}

// ---------------------------------------------------------------------------
// Functions from other modules (extern "C" linkage)
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn printVector(x: SEXP, indx: c_int, quote: c_int);
    fn GetMatrixDimnames(
        x: SEXP,
        rl: *mut SEXP,
        cl: *mut SEXP,
        rn: *mut *const c_char,
        cn: *mut *const c_char,
    );
    fn getAttrib(x: SEXP, which: SEXP) -> SEXP;
}

// ---------------------------------------------------------------------------
// NA_STRING -- sentinel; R_NilValue is used since real NA_STRING CHARSXP
// is not yet available.
// ---------------------------------------------------------------------------

unsafe fn NA_STRING() -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// Helper: ceil_DIV(a, b) = ceil(a / b) in integer arithmetic
// ---------------------------------------------------------------------------

fn ceil_DIV(a: c_int, b: c_int) -> c_int {
    let d = a / b;
    let r = a % b;
    d + if r != 0 { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// Helper: strwidth -- display width of a C string (simplified)
// ---------------------------------------------------------------------------

unsafe fn strwidth(s: *const c_char) -> c_int {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let len = libc::strlen(s);
        if len == 0 {
            return 0;
        }
        // Simplified: use byte length for CE_NATIVE ASCII strings
        len as c_int
    }
}

// ---------------------------------------------------------------------------
// MatrixColumnLabel -- print column label
// ---------------------------------------------------------------------------

unsafe fn MatrixColumnLabel(cl: SEXP, j: c_int, w: c_int) {
    unsafe {
        let rp = get_R_print_full();
        if cl != R_NilValue() {
            let tmp = STRING_ELT(cl, j as R_xlen_t);
            let l = if tmp == NA_STRING() {
                rp.na_width_noquote
            } else {
                local_Rstrlen(tmp, 0)
            };
            let pad = w - l;
            if pad > 0 {
                eprint!("{:width$}", "", width = pad as usize);
            }
            let s = local_EncodeString(tmp, l, 0, Rprt_adj::left);
            if !s.is_null() {
                let cstr = std::ffi::CStr::from_ptr(s);
                if let Ok(s) = cstr.to_str() {
                    eprint!("{}", s);
                }
            }
        } else {
            let iw = IndexWidth((j + 1) as R_xlen_t);
            let pad = w - iw - 3;
            if pad > 0 {
                eprint!("{:width$}", "", width = pad as usize);
            }
            eprint!("[,{}]", j + 1);
        }
    }
}

// ---------------------------------------------------------------------------
// RightMatrixColumnLabel -- right-adjusted column label
// ---------------------------------------------------------------------------

unsafe fn RightMatrixColumnLabel(cl: SEXP, j: c_int, w: c_int) {
    unsafe {
        let rp = get_R_print_full();
        if cl != R_NilValue() {
            let tmp = STRING_ELT(cl, j as R_xlen_t);
            let l = if tmp == NA_STRING() {
                rp.na_width_noquote
            } else {
                local_Rstrlen(tmp, 0)
            };
            let pad = rp.gap + w - l;
            if pad > 0 {
                eprint!("{:width$}", "", width = pad as usize);
            }
            let s = local_EncodeString(tmp, l, 0, Rprt_adj::right);
            if !s.is_null() {
                let cstr = std::ffi::CStr::from_ptr(s);
                if let Ok(s) = cstr.to_str() {
                    eprint!("{}", s);
                }
            }
        } else {
            let iw = IndexWidth((j + 1) as R_xlen_t);
            let pad = rp.gap + w - iw - 3;
            if pad > 0 {
                eprint!("{:width$}", "", width = pad as usize);
            }
            eprint!("[,{}]", j + 1);
        }
    }
}

// ---------------------------------------------------------------------------
// LeftMatrixColumnLabel -- left-adjusted column label
// ---------------------------------------------------------------------------

unsafe fn LeftMatrixColumnLabel(cl: SEXP, j: c_int, w: c_int) {
    unsafe {
        let rp = get_R_print_full();
        if cl != R_NilValue() {
            let tmp = STRING_ELT(cl, j as R_xlen_t);
            let l = if tmp == NA_STRING() {
                rp.na_width_noquote
            } else {
                local_Rstrlen(tmp, 0)
            };
            eprint!("{:width$}", "", width = rp.gap as usize);
            let s = local_EncodeString(tmp, l, 0, Rprt_adj::left);
            if !s.is_null() {
                let cstr = std::ffi::CStr::from_ptr(s);
                if let Ok(s) = cstr.to_str() {
                    eprint!("{}", s);
                }
            }
            let pad = w - l;
            if pad > 0 {
                eprint!("{:width$}", "", width = pad as usize);
            }
        } else {
            let iw = IndexWidth((j + 1) as R_xlen_t);
            eprint!("{:width$}", "", width = rp.gap as usize);
            eprint!("[,{}]", j + 1);
            let pad = w - iw - 3;
            if pad > 0 {
                eprint!("{:width$}", "", width = pad as usize);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MatrixRowLabel -- print row label
// ---------------------------------------------------------------------------

unsafe fn MatrixRowLabel(rl: SEXP, i: c_int, rlabw: c_int, lbloff: c_int) {
    unsafe {
        let rp = get_R_print_full();
        if rl != R_NilValue() {
            let tmp = STRING_ELT(rl, i as R_xlen_t);
            let l = if tmp == NA_STRING() {
                rp.na_width_noquote
            } else {
                local_Rstrlen(tmp, 0)
            };
            eprint!("\n{:width$}", "", width = lbloff as usize);
            let s = local_EncodeString(tmp, l, 0, Rprt_adj::left);
            if !s.is_null() {
                let cstr = std::ffi::CStr::from_ptr(s);
                if let Ok(s) = cstr.to_str() {
                    eprint!("{}", s);
                }
            }
            let pad = rlabw - l - lbloff;
            if pad > 0 {
                eprint!("{:width$}", "", width = pad as usize);
            }
        } else {
            let iw = IndexWidth((i + 1) as R_xlen_t);
            let pad = rlabw - 3 - iw;
            eprint!("\n");
            if pad > 0 {
                eprint!("{:width$}", "", width = pad as usize);
            }
            eprint!("[{},]", i + 1);
        }
    }
}

// ---------------------------------------------------------------------------
// init_rl_rn -- common initialization for row labels and row name offset
// ---------------------------------------------------------------------------

struct RowLabelInfo {
    rlabw: c_int,
    lbloff: c_int,
}

unsafe fn init_rl_rn(rl: SEXP, rn: *const c_char, r: c_int) -> RowLabelInfo {
    unsafe {
        let rp = get_R_print_full();
        let mut rlabw: c_int;
        let mut lbloff: c_int = 0;

        if rl != R_NilValue() {
            rlabw = 0;
            for ii in 0..r as R_xlen_t {
                let tmp = STRING_ELT(rl, ii);
                let l = if tmp == NA_STRING() {
                    rp.na_width_noquote
                } else {
                    local_Rstrlen(tmp, 0)
                };
                if l > rlabw {
                    rlabw = l;
                }
            }
        } else {
            rlabw = IndexWidth((r + 1) as R_xlen_t) + 3;
        }

        if !rn.is_null() {
            let rnw = strwidth(rn);
            if rnw < rlabw + R_MIN_LBLOFF {
                lbloff = R_MIN_LBLOFF;
            } else {
                lbloff = rnw - rlabw;
            }
            rlabw += lbloff;
        }

        RowLabelInfo { rlabw, lbloff }
    }
}

// ---------------------------------------------------------------------------
// print_row_header -- print the row name / column name header line
// ---------------------------------------------------------------------------

unsafe fn print_row_header(cn: *const c_char, rlabw: c_int, rn: *const c_char) {
    unsafe {
        if !cn.is_null() {
            let cn_cstr = std::ffi::CStr::from_ptr(cn);
            let s = cn_cstr.to_str().unwrap_or("");
            eprint!("{:width$}\n", s, width = rlabw as usize);
        }
        if !rn.is_null() {
            let rn_cstr = std::ffi::CStr::from_ptr(rn);
            let s = rn_cstr.to_str().unwrap_or("");
            eprint!("{:<width$}", s, width = rlabw as usize);
        } else {
            eprint!("{:width$}", "", width = rlabw as usize);
        }
    }
}

// ---------------------------------------------------------------------------
// print_column_labels -- standard column labels
// ---------------------------------------------------------------------------

unsafe fn std_column_labels(cl: SEXP, jmin: usize, jmax: usize, w: &[c_int]) {
    unsafe {
        for j in jmin..jmax {
            MatrixColumnLabel(cl, j as c_int, w[j]);
        }
    }
}

// ---------------------------------------------------------------------------
// print_logical_matrix
// ---------------------------------------------------------------------------

unsafe fn print_logical_matrix(
    sx: SEXP,
    offset: c_int,
    r_pr: c_int,
    r: c_int,
    c: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
    print_ij: bool,
) {
    unsafe {
        let rp = get_R_print_full();
        let info = init_rl_rn(rl, rn, r);
        let rlabw = info.rlabw;
        let lbloff = info.lbloff;

        let mut w = vec![0i32; c as usize];
        let mut clabw: c_int = 0;

        let x = LOGICAL(sx).offset(offset as isize);

        // Compute w[j] for each column
        for j in 0..c as usize {
            if print_ij {
                let col_ptr = x.offset((j as c_int * r) as isize);
                let mut fw: c_int = 0;
                formatLogical(col_ptr, r as R_xlen_t, &mut fw);
                w[j] = fw;
            } else {
                w[j] = 0;
            }

            // Check column label width
            if cl != R_NilValue() {
                let col_sexp = STRING_ELT(cl, j as R_xlen_t);
                clabw = if col_sexp == NA_STRING() {
                    rp.na_width_noquote
                } else {
                    local_Rstrlen(col_sexp, 0)
                };
            } else {
                clabw = IndexWidth((j as c_int + 1) as R_xlen_t) + 3;
            }
            if w[j] < clabw {
                w[j] = clabw;
            }
            w[j] += rp.gap;
        }

        // Print matrix
        if c == 0 {
            print_row_header(cn, rlabw, rn);
            for i in 0..r {
                MatrixRowLabel(rl, i, rlabw, lbloff);
            }
            eprintln!();
        } else {
            let mut jmin: usize = 0;
            while jmin < c as usize {
                let mut width: c_int = rlabw;
                let mut jmax = jmin;
                loop {
                    width += w[jmax];
                    jmax += 1;
                    if jmax >= c as usize {
                        break;
                    }
                    if width + w[jmax] >= rp.width {
                        break;
                    }
                }

                print_row_header(cn, rlabw, rn);
                std_column_labels(cl, jmin, jmax, &w);

                for i in 0..r_pr as usize {
                    MatrixRowLabel(rl, i as c_int, rlabw, lbloff);
                    if print_ij {
                        for j in jmin..jmax {
                            let val = *x.offset((j as c_int * r + i as c_int) as isize);
                            let s = EncodeLogical(val, w[j]);
                            let cstr = std::ffi::CStr::from_ptr(s);
                            eprint!("{}", cstr.to_str().unwrap_or(""));
                        }
                    }
                }
                eprintln!();
                jmin = jmax;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// print_integer_matrix
// ---------------------------------------------------------------------------

unsafe fn print_integer_matrix(
    sx: SEXP,
    offset: c_int,
    r_pr: c_int,
    r: c_int,
    c: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
    print_ij: bool,
) {
    unsafe {
        let rp = get_R_print_full();
        let info = init_rl_rn(rl, rn, r);
        let rlabw = info.rlabw;
        let lbloff = info.lbloff;

        let mut w = vec![0i32; c as usize];
        let mut clabw: c_int = 0;

        let x = INTEGER(sx).offset(offset as isize);

        for j in 0..c as usize {
            if print_ij {
                let col_ptr = x.offset((j as c_int * r) as isize);
                let mut fw: c_int = 0;
                formatInteger(col_ptr, r as R_xlen_t, &mut fw);
                w[j] = fw;
            } else {
                w[j] = 0;
            }
            if cl != R_NilValue() {
                let col_sexp = STRING_ELT(cl, j as R_xlen_t);
                clabw = if col_sexp == NA_STRING() {
                    rp.na_width_noquote
                } else {
                    local_Rstrlen(col_sexp, 0)
                };
            } else {
                clabw = IndexWidth((j as c_int + 1) as R_xlen_t) + 3;
            }
            if w[j] < clabw {
                w[j] = clabw;
            }
            w[j] += rp.gap;
        }

        if c == 0 {
            print_row_header(cn, rlabw, rn);
            for i in 0..r {
                MatrixRowLabel(rl, i, rlabw, lbloff);
            }
            eprintln!();
        } else {
            let mut jmin: usize = 0;
            while jmin < c as usize {
                let mut width: c_int = rlabw;
                let mut jmax = jmin;
                loop {
                    width += w[jmax];
                    jmax += 1;
                    if jmax >= c as usize {
                        break;
                    }
                    if width + w[jmax] >= rp.width {
                        break;
                    }
                }

                print_row_header(cn, rlabw, rn);
                std_column_labels(cl, jmin, jmax, &w);

                for i in 0..r_pr as usize {
                    MatrixRowLabel(rl, i as c_int, rlabw, lbloff);
                    if print_ij {
                        for j in jmin..jmax {
                            let val = *x.offset((j as c_int * r + i as c_int) as isize);
                            let s = EncodeInteger(val, w[j]);
                            let cstr = std::ffi::CStr::from_ptr(s);
                            eprint!("{}", cstr.to_str().unwrap_or(""));
                        }
                    }
                }
                eprintln!();
                jmin = jmax;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// print_real_matrix
// ---------------------------------------------------------------------------

unsafe fn print_real_matrix(
    sx: SEXP,
    offset: c_int,
    r_pr: c_int,
    r: c_int,
    c: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
    print_ij: bool,
) {
    unsafe {
        let rp = get_R_print_full();
        let info = init_rl_rn(rl, rn, r);
        let rlabw = info.rlabw;
        let lbloff = info.lbloff;

        let mut w = vec![0i32; c as usize];
        let mut d = vec![0i32; c as usize];
        let mut e = vec![0i32; c as usize];
        let mut clabw: c_int = 0;

        let x = REAL(sx).offset(offset as isize);
        let dec_ptr = &OUT_DEC as *const c_char;

        for j in 0..c as usize {
            if print_ij {
                let col_ptr = x.offset((j as c_int * r) as isize);
                formatReal(col_ptr, r as R_xlen_t, &mut w[j], &mut d[j], &mut e[j], 0);
            } else {
                w[j] = 0;
            }
            if cl != R_NilValue() {
                let col_sexp = STRING_ELT(cl, j as R_xlen_t);
                clabw = if col_sexp == NA_STRING() {
                    rp.na_width_noquote
                } else {
                    local_Rstrlen(col_sexp, 0)
                };
            } else {
                clabw = IndexWidth((j as c_int + 1) as R_xlen_t) + 3;
            }
            if w[j] < clabw {
                w[j] = clabw;
            }
            w[j] += rp.gap;
        }

        if c == 0 {
            print_row_header(cn, rlabw, rn);
            for i in 0..r {
                MatrixRowLabel(rl, i, rlabw, lbloff);
            }
            eprintln!();
        } else {
            let mut jmin: usize = 0;
            while jmin < c as usize {
                let mut width: c_int = rlabw;
                let mut jmax = jmin;
                loop {
                    width += w[jmax];
                    jmax += 1;
                    if jmax >= c as usize {
                        break;
                    }
                    if width + w[jmax] >= rp.width {
                        break;
                    }
                }

                print_row_header(cn, rlabw, rn);
                std_column_labels(cl, jmin, jmax, &w);

                for i in 0..r_pr as usize {
                    MatrixRowLabel(rl, i as c_int, rlabw, lbloff);
                    if print_ij {
                        for j in jmin..jmax {
                            let val = *x.offset((j as c_int * r + i as c_int) as isize);
                            let s = EncodeReal0(val, w[j], d[j], e[j], dec_ptr);
                            let cstr = std::ffi::CStr::from_ptr(s);
                            eprint!("{}", cstr.to_str().unwrap_or(""));
                        }
                    }
                }
                eprintln!();
                jmin = jmax;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// print_complex_matrix
// ---------------------------------------------------------------------------

unsafe fn print_complex_matrix(
    sx: SEXP,
    offset: c_int,
    r_pr: c_int,
    r: c_int,
    c: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
    print_ij: bool,
) {
    unsafe {
        let rp = get_R_print_full();
        let info = init_rl_rn(rl, rn, r);
        let rlabw = info.rlabw;
        let lbloff = info.lbloff;

        let mut w = vec![0i32; c as usize];
        let mut wr = vec![0i32; c as usize];
        let mut dr = vec![0i32; c as usize];
        let mut er = vec![0i32; c as usize];
        let mut wi = vec![0i32; c as usize];
        let mut di = vec![0i32; c as usize];
        let mut ei = vec![0i32; c as usize];
        let mut clabw: c_int = 0;

        let x = COMPLEX(sx).offset(offset as isize);
        let dec_ptr = &OUT_DEC as *const c_char;

        for j in 0..c as usize {
            if print_ij {
                let col_ptr = x.offset((j as c_int * r) as isize);
                formatComplex(
                    col_ptr,
                    r as R_xlen_t,
                    &mut wr[j],
                    &mut dr[j],
                    &mut er[j],
                    &mut wi[j],
                    &mut di[j],
                    &mut ei[j],
                    0,
                );
                w[j] = wr[j] + wi[j] + 2;
            } else {
                w[j] = 0;
            }
            if cl != R_NilValue() {
                let col_sexp = STRING_ELT(cl, j as R_xlen_t);
                clabw = if col_sexp == NA_STRING() {
                    rp.na_width_noquote
                } else {
                    local_Rstrlen(col_sexp, 0)
                };
            } else {
                clabw = IndexWidth((j as c_int + 1) as R_xlen_t) + 3;
            }
            if w[j] < clabw {
                w[j] = clabw;
            }
            w[j] += rp.gap;
        }

        if c == 0 {
            print_row_header(cn, rlabw, rn);
            for i in 0..r {
                MatrixRowLabel(rl, i, rlabw, lbloff);
            }
            eprintln!();
        } else {
            let mut jmin: usize = 0;
            while jmin < c as usize {
                let mut width: c_int = rlabw;
                let mut jmax = jmin;
                loop {
                    width += w[jmax];
                    jmax += 1;
                    if jmax >= c as usize {
                        break;
                    }
                    if width + w[jmax] >= rp.width {
                        break;
                    }
                }

                print_row_header(cn, rlabw, rn);
                std_column_labels(cl, jmin, jmax, &w);

                for i in 0..r_pr as usize {
                    MatrixRowLabel(rl, i as c_int, rlabw, lbloff);
                    if print_ij {
                        for j in jmin..jmax {
                            let cx = *x.offset((j as c_int * r + i as c_int) as isize);
                            let s = if R_IsNA(cx.r) || R_IsNA(cx.i) {
                                EncodeReal0(std::f64::NAN, w[j], 0, 0, dec_ptr)
                            } else {
                                EncodeComplex(
                                    cx,
                                    w[j] - wi[j] - 2,
                                    dr[j],
                                    er[j],
                                    wi[j],
                                    di[j],
                                    ei[j],
                                    dec_ptr,
                                )
                            };
                            let cstr = std::ffi::CStr::from_ptr(s);
                            eprint!("{}", cstr.to_str().unwrap_or(""));
                        }
                    }
                }
                eprintln!();
                jmin = jmax;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// print_string_matrix
// ---------------------------------------------------------------------------

unsafe fn print_string_matrix(
    sx: SEXP,
    offset: c_int,
    r_pr: c_int,
    r: c_int,
    c: c_int,
    quote: c_int,
    right: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
    print_ij: bool,
) {
    unsafe {
        let rp = get_R_print_full();
        let info = init_rl_rn(rl, rn, r);
        let rlabw = info.rlabw;
        let lbloff = info.lbloff;

        let mut w = vec![0i32; c as usize];
        let mut clabw: c_int = 0;

        // Compute column widths (NO gap added for strings)
        for j in 0..c as usize {
            if print_ij {
                let mut max_w: c_int = 0;
                for i in 0..r as R_xlen_t {
                    let elem = STRING_ELT(
                        sx,
                        (offset as R_xlen_t) + (j as R_xlen_t) * (r as R_xlen_t) + i,
                    );
                    let l = if elem == NA_STRING() {
                        if quote != 0 {
                            rp.na_width
                        } else {
                            rp.na_width_noquote
                        }
                    } else {
                        local_Rstrlen(elem, quote) + if quote != 0 { 2 } else { 0 }
                    };
                    if l > max_w {
                        max_w = l;
                    }
                }
                w[j] = max_w;
            } else {
                w[j] = 0;
            }

            if cl != R_NilValue() {
                let col_sexp = STRING_ELT(cl, j as R_xlen_t);
                clabw = if col_sexp == NA_STRING() {
                    rp.na_width_noquote
                } else {
                    local_Rstrlen(col_sexp, 0)
                };
            } else {
                clabw = IndexWidth((j as c_int + 1) as R_xlen_t) + 3;
            }
            if w[j] < clabw {
                w[j] = clabw;
            }
            // Note: no gap added for strings; gap is added during printing
        }

        // Print matrix with gap as extra width
        if c == 0 {
            print_row_header(cn, rlabw, rn);
            for i in 0..r {
                MatrixRowLabel(rl, i, rlabw, lbloff);
            }
            eprintln!();
        } else {
            let mut jmin: usize = 0;
            while jmin < c as usize {
                let mut width: c_int = rlabw;
                let mut jmax = jmin;
                loop {
                    width += w[jmax] + rp.gap;
                    jmax += 1;
                    if jmax >= c as usize {
                        break;
                    }
                    if width + w[jmax] + rp.gap >= rp.width {
                        break;
                    }
                }

                print_row_header(cn, rlabw, rn);

                // Column labels: right or left adjusted
                if right != 0 {
                    for j in jmin..jmax {
                        RightMatrixColumnLabel(cl, j as c_int, w[j]);
                    }
                } else {
                    for j in jmin..jmax {
                        LeftMatrixColumnLabel(cl, j as c_int, w[j]);
                    }
                }

                for i in 0..r_pr as usize {
                    MatrixRowLabel(rl, i as c_int, rlabw, lbloff);
                    if print_ij {
                        for j in jmin..jmax {
                            let elem = STRING_ELT(
                                sx,
                                (offset as R_xlen_t)
                                    + (j as R_xlen_t) * (r as R_xlen_t)
                                    + (i as R_xlen_t),
                            );
                            eprint!("{:width$}", "", width = rp.gap as usize);
                            let adj = if right != 0 {
                                Rprt_adj::right
                            } else {
                                Rprt_adj::left
                            };
                            let s = local_EncodeString(elem, w[j], quote, adj);
                            if !s.is_null() {
                                let cstr = std::ffi::CStr::from_ptr(s);
                                if let Ok(s) = cstr.to_str() {
                                    eprint!("{}", s);
                                }
                            }
                        }
                    }
                }
                eprintln!();
                jmin = jmax;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// print_raw_matrix
// ---------------------------------------------------------------------------

unsafe fn print_raw_matrix(
    sx: SEXP,
    offset: c_int,
    r_pr: c_int,
    r: c_int,
    c: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
    print_ij: bool,
) {
    unsafe {
        let rp = get_R_print_full();
        let info = init_rl_rn(rl, rn, r);
        let rlabw = info.rlabw;
        let lbloff = info.lbloff;

        let mut w = vec![0i32; c as usize];
        let mut clabw: c_int = 0;

        let x = RAW(sx).offset(offset as isize);

        for j in 0..c as usize {
            if print_ij {
                let col_ptr = x.offset((j as c_int * r) as isize);
                let mut fw: c_int = 0;
                formatRaw(col_ptr as *const c_void, r as R_xlen_t, &mut fw);
                w[j] = fw;
            } else {
                w[j] = 0;
            }
            if cl != R_NilValue() {
                let col_sexp = STRING_ELT(cl, j as R_xlen_t);
                clabw = if col_sexp == NA_STRING() {
                    rp.na_width_noquote
                } else {
                    local_Rstrlen(col_sexp, 0)
                };
            } else {
                clabw = IndexWidth((j as c_int + 1) as R_xlen_t) + 3;
            }
            if w[j] < clabw {
                w[j] = clabw;
            }
            w[j] += rp.gap;
        }

        if c == 0 {
            print_row_header(cn, rlabw, rn);
            for i in 0..r {
                MatrixRowLabel(rl, i, rlabw, lbloff);
            }
            eprintln!();
        } else {
            let mut jmin: usize = 0;
            while jmin < c as usize {
                let mut width: c_int = rlabw;
                let mut jmax = jmin;
                loop {
                    width += w[jmax];
                    jmax += 1;
                    if jmax >= c as usize {
                        break;
                    }
                    if width + w[jmax] >= rp.width {
                        break;
                    }
                }

                print_row_header(cn, rlabw, rn);
                std_column_labels(cl, jmin, jmax, &w);

                for i in 0..r_pr as usize {
                    MatrixRowLabel(rl, i as c_int, rlabw, lbloff);
                    if print_ij {
                        for j in jmin..jmax {
                            let val = *x.offset((j as c_int * r + i as c_int) as isize);
                            let pad = w[j] - 2;
                            if pad > 0 {
                                eprint!("{:width$}", "", width = pad as usize);
                            }
                            let s = EncodeRaw(val, std::ptr::null());
                            let cstr = std::ffi::CStr::from_ptr(s);
                            eprint!("{}", cstr.to_str().unwrap_or(""));
                        }
                    }
                }
                eprintln!();
                jmin = jmax;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// printMatrix -- main matrix printer (#[unsafe(no_mangle)] export)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn printMatrix(
    x: SEXP,
    offset: c_int,
    dim: SEXP,
    quote: c_int,
    right: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
) {
    unsafe {
        if x.is_null() || dim.is_null() {
            return;
        }

        let rp = get_R_print_full();
        let pdim = INTEGER(dim);
        let r = *pdim;
        let c = *pdim.offset(1);

        // Check label lengths
        if rl != R_NilValue() && r > LENGTH(rl) {
            return;
        }
        if cl != R_NilValue() && c > LENGTH(cl) {
            return;
        }

        if r == 0 && c == 0 {
            eprintln!("<0 x 0 matrix>");
            return;
        }

        let mut r_pr = r;
        let c_pr = if c > rp.max { rp.max } else { c };

        // Avoid integer overflow
        if c > 0 && rp.max / c < r {
            r_pr = rp.max / c;
        }
        // Display at least one row in case of truncation
        if c > c_pr && r_pr < 1 && r > 0 {
            r_pr = 1;
        }

        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP.0 => {
                print_logical_matrix(x, offset, r_pr, r, c_pr, rl, cl, rn, cn, true);
            }
            t if t == SEXPTYPE::INTSXP.0 => {
                print_integer_matrix(x, offset, r_pr, r, c_pr, rl, cl, rn, cn, true);
            }
            t if t == SEXPTYPE::REALSXP.0 => {
                print_real_matrix(x, offset, r_pr, r, c_pr, rl, cl, rn, cn, true);
            }
            t if t == SEXPTYPE::CPLXSXP.0 => {
                print_complex_matrix(x, offset, r_pr, r, c_pr, rl, cl, rn, cn, true);
            }
            t if t == SEXPTYPE::STRSXP.0 => {
                let q = if quote != 0 { b'"' as c_int } else { 0 };
                print_string_matrix(x, offset, r_pr, r, c_pr, q, right, rl, cl, rn, cn, true);
            }
            t if t == SEXPTYPE::RAWSXP.0 => {
                print_raw_matrix(x, offset, r_pr, r, c_pr, rl, cl, rn, cn, true);
            }
            _ => {
                eprintln!(" {} x {} matrix of type {}", r, c, TYPEOF(x));
            }
        }

        if r_pr < r || c_pr < c {
            eprint!(" [ reached 'max' / getOption(\"max.print\") -- omitted");
            if r_pr < r {
                let omitted = r - r_pr;
                eprint!(" {} row{}", omitted, if omitted == 1 { "" } else { "s" });
            }
            if c_pr < c {
                if r_pr < r {
                    eprint!(" and");
                }
                let omitted = c - c_pr;
                eprint!(" {} column{}", omitted, if omitted == 1 { "" } else { "s" });
            }
            eprintln!(" ]");
        }
    }
}

// ---------------------------------------------------------------------------
// printArray -- print an n-dimensional array (#[unsafe(no_mangle)] export)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn printArray(
    x: SEXP,
    dim: SEXP,
    quote: c_int,
    right: c_int,
    dimnames: SEXP,
) {
    unsafe {
        if x.is_null() || dim.is_null() {
            return;
        }

        let ndim = LENGTH(dim);

        if ndim == 1 {
            printVector(x, 1, quote);
            return;
        }

        if ndim == 2 {
            let mut rl: SEXP = ptr::null_mut();
            let mut cl: SEXP = ptr::null_mut();
            let mut rn: *const c_char = ptr::null();
            let mut cn: *const c_char = ptr::null();
            GetMatrixDimnames(x, &mut rl, &mut cl, &mut rn, &mut cn);
            printMatrix(x, 0, dim, quote, 0, rl, cl, rn, cn);
            return;
        }

        // ndim >= 3
        let rp = get_R_print_full();
        let pdim = INTEGER(dim);

        let nr = *pdim;
        let nc = *pdim.add(1);
        let b = nr * nc; // elements per matrix slice

        let has_dimnames = dimnames != R_NilValue();
        let mut dn0: SEXP = R_NilValue();
        let mut dn1: SEXP = R_NilValue();
        let mut dnn: SEXP = R_NilValue();
        let mut has_dnn = false;
        let rn: *const c_char = ptr::null();
        let cn: *const c_char = ptr::null();

        if has_dimnames {
            dn0 = VECTOR_ELT(dimnames, 0);
            dn1 = VECTOR_ELT(dimnames, 1);
            let names_sym = crate::main::relop::R_NamesSymbol();
            dnn = getAttrib(dimnames, names_sym);
            has_dnn = dnn != R_NilValue();
            // translateChar not available; rn and cn remain null
        }

        // nb := number of matrix slices
        let mut nb: c_int = 1;
        for i in 2..ndim {
            nb *= *pdim.offset(i as isize);
        }

        let max_reached = b > 0 && rp.max / b < nb;
        let mut nb_pr: c_int;
        let ne_last: c_int;
        let mut nc_last: c_int;
        let mut nr_last: c_int;

        if max_reached {
            nb_pr = ceil_DIV(rp.max, b);
            ne_last = rp.max - b * (nb_pr - 1);
            nc_last = if ne_last < nc { ne_last } else { nc };
            nr_last = if ne_last < nc { 1 } else { ne_last / nc };
            if nr_last == 0 {
                nb_pr -= 1;
                nc_last = nc;
                nr_last = nr;
            }
        } else {
            nb_pr = if nb > 0 { nb } else { 1 };
            ne_last = b;
            nc_last = nc;
            nr_last = nr;
        }

        for ii in 0..nb_pr as usize {
            let do_ij = nb > 0;
            let i_last = ii == (nb_pr as usize) - 1;
            let use_nc = if i_last { nc_last } else { nc };
            let use_nr = if i_last { nr_last } else { nr };

            if do_ij {
                eprint!(", ");
                let mut k: c_int = 1;
                for j in 2..ndim {
                    let l = (ii as c_int / k) % *pdim.offset(j as isize) + 1;
                    if has_dimnames {
                        let dn = VECTOR_ELT(dimnames, j as R_xlen_t);
                        if dn != R_NilValue() {
                            eprint!(", {}", l);
                        } else {
                            eprint!(", {}", l);
                        }
                    } else {
                        eprint!(", {}", l);
                    }
                    k *= *pdim.offset(j as isize);
                }
                eprintln!();
            } else {
                // nb == 0, e.g. <2 x 3 x 0 array>
                for di in 0..ndim {
                    if di == 0 {
                        eprint!("<{}", *pdim.offset(di as isize));
                    } else {
                        eprint!(" x {}", *pdim.offset(di as isize));
                    }
                }
                eprintln!(" array of type {}>", TYPEOF(x));
            }

            let offset = (ii as c_int) * b;
            match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP.0 => {
                    print_logical_matrix(x, offset, use_nr, nr, use_nc, dn0, dn1, rn, cn, do_ij);
                }
                t if t == SEXPTYPE::INTSXP.0 => {
                    print_integer_matrix(x, offset, use_nr, nr, use_nc, dn0, dn1, rn, cn, do_ij);
                }
                t if t == SEXPTYPE::REALSXP.0 => {
                    print_real_matrix(x, offset, use_nr, nr, use_nc, dn0, dn1, rn, cn, do_ij);
                }
                t if t == SEXPTYPE::CPLXSXP.0 => {
                    print_complex_matrix(x, offset, use_nr, nr, use_nc, dn0, dn1, rn, cn, do_ij);
                }
                t if t == SEXPTYPE::STRSXP.0 => {
                    let q = if quote != 0 { b'"' as c_int } else { 0 };
                    print_string_matrix(
                        x, offset, use_nr, nr, use_nc, q, right, dn0, dn1, rn, cn, do_ij,
                    );
                }
                t if t == SEXPTYPE::RAWSXP.0 => {
                    print_raw_matrix(x, offset, use_nr, nr, use_nc, dn0, dn1, rn, cn, do_ij);
                }
                _ => {}
            }
            eprintln!();
        }

        if max_reached {
            eprint!(" [ reached 'max' / getOption(\"max.print\") -- omitted");
            if (nb_pr as c_int) < nb {
                let omitted = nb - nb_pr;
                eprint!(" {} slice{}", omitted, if omitted == 1 { "" } else { "s" });
            } else if (nb_pr as c_int) == nb {
                let nr_rem = nr - nr_last;
                if nr_rem > 0 {
                    eprint!(" {} row{}", nr_rem, if nr_rem == 1 { "" } else { "s" });
                }
                let nc_rem = nc - nc_last;
                if nc_rem > 0 {
                    eprint!(" {} column{}", nc_rem, if nc_rem == 1 { "" } else { "s" });
                }
            }
            eprintln!(" ]");
        }
    }
}

// ---------------------------------------------------------------------------
// formatLogicalMatrix -- compute column widths for logical matrix
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn formatLogicalMatrix(x: SEXP, n: R_xlen_t, w: *mut c_int) {
    unsafe {
        if x.is_null() || n <= 0 {
            return;
        }
        let len = LENGTH(x);
        let nc = if n > 0 { len as i64 / n } else { 0 };
        let r = n as c_int;

        let data = LOGICAL(x);
        let mut max_w: c_int = 0;

        for j in 0..nc as usize {
            let mut fw: c_int = 0;
            let col_ptr = data.offset((j as c_int * r) as isize);
            formatLogical(col_ptr, n, &mut fw);
            if fw > max_w {
                max_w = fw;
            }
        }

        if !w.is_null() {
            *w = max_w;
        }
    }
}

// ---------------------------------------------------------------------------
// formatIntegerMatrix -- compute column widths for integer matrix
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn formatIntegerMatrix(x: SEXP, n: R_xlen_t, w: *mut c_int) {
    unsafe {
        if x.is_null() || n <= 0 {
            return;
        }
        let len = LENGTH(x);
        let nc = if n > 0 { len as i64 / n } else { 0 };
        let r = n as c_int;

        let data = INTEGER(x);
        let mut max_w: c_int = 0;

        for j in 0..nc as usize {
            let mut fw: c_int = 0;
            let col_ptr = data.offset((j as c_int * r) as isize);
            formatInteger(col_ptr, n, &mut fw);
            if fw > max_w {
                max_w = fw;
            }
        }

        if !w.is_null() {
            *w = max_w;
        }
    }
}

// ---------------------------------------------------------------------------
// formatRealMatrix -- compute column widths for real matrix
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn formatRealMatrix(
    x: SEXP,
    n: R_xlen_t,
    w: *mut c_int,
    d: *mut c_int,
    e: *mut c_int,
) {
    unsafe {
        if x.is_null() || n <= 0 {
            return;
        }
        let len = LENGTH(x);
        let nc = if n > 0 { len as i64 / n } else { 0 };
        let r = n as c_int;

        let data = REAL(x);
        let mut max_w: c_int = 0;
        let mut best_d: c_int = 0;
        let mut best_e: c_int = 0;

        for j in 0..nc as usize {
            let mut fw: c_int = 0;
            let mut fd: c_int = 0;
            let mut fe: c_int = 0;
            let col_ptr = data.offset((j as c_int * r) as isize);
            formatReal(col_ptr, n, &mut fw, &mut fd, &mut fe, 0);
            if fw > max_w {
                max_w = fw;
                best_d = fd;
                best_e = fe;
            }
        }

        if !w.is_null() {
            *w = max_w;
        }
        if !d.is_null() {
            *d = best_d;
        }
        if !e.is_null() {
            *e = best_e;
        }
    }
}

// ---------------------------------------------------------------------------
// formatComplexMatrix -- compute column widths for complex matrix
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn formatComplexMatrix(
    x: SEXP,
    n: R_xlen_t,
    wr: *mut c_int,
    dr: *mut c_int,
    er: *mut c_int,
    wi: *mut c_int,
    di: *mut c_int,
    ei: *mut c_int,
) {
    unsafe {
        if x.is_null() || n <= 0 {
            return;
        }
        let len = LENGTH(x);
        let nc = if n > 0 { len as i64 / n } else { 0 };
        let r = n as c_int;

        let data = COMPLEX(x);
        let mut max_wr: c_int = 0;
        let mut best_dr: c_int = 0;
        let mut best_er: c_int = 0;
        let mut max_wi: c_int = 0;
        let mut best_di: c_int = 0;
        let mut best_ei: c_int = 0;

        for j in 0..nc as usize {
            let mut fwr: c_int = 0;
            let mut fdr: c_int = 0;
            let mut fer: c_int = 0;
            let mut fwi: c_int = 0;
            let mut fdi: c_int = 0;
            let mut fei: c_int = 0;
            let col_ptr = data.offset((j as c_int * r) as isize);
            formatComplex(
                col_ptr, n, &mut fwr, &mut fdr, &mut fer, &mut fwi, &mut fdi, &mut fei, 0,
            );
            if fwr > max_wr {
                max_wr = fwr;
                best_dr = fdr;
                best_er = fer;
            }
            if fwi > max_wi {
                max_wi = fwi;
                best_di = fdi;
                best_ei = fei;
            }
        }

        if !wr.is_null() {
            *wr = max_wr;
        }
        if !dr.is_null() {
            *dr = best_dr;
        }
        if !er.is_null() {
            *er = best_er;
        }
        if !wi.is_null() {
            *wi = max_wi;
        }
        if !di.is_null() {
            *di = best_di;
        }
        if !ei.is_null() {
            *ei = best_ei;
        }
    }
}

// ---------------------------------------------------------------------------
// formatStringMatrix -- compute column widths for string matrix
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn formatStringMatrix(x: SEXP, n: R_xlen_t, w: *mut c_int, quote: c_int) {
    unsafe {
        if x.is_null() || n <= 0 {
            return;
        }
        let len = LENGTH(x);
        let nc = if n > 0 { len as i64 / n } else { 0 };

        let rp = get_R_print_full();
        let mut max_w: c_int = 0;

        for j in 0..nc as usize {
            for i in 0..n {
                let elem = STRING_ELT(x, (j as R_xlen_t) * n + i);
                let l = if elem == NA_STRING() {
                    if quote != 0 {
                        rp.na_width
                    } else {
                        rp.na_width_noquote
                    }
                } else {
                    local_Rstrlen(elem, quote) + if quote != 0 { 2 } else { 0 }
                };
                if l > max_w {
                    max_w = l;
                }
            }
        }

        if !w.is_null() {
            *w = max_w;
        }
    }
}

// ---------------------------------------------------------------------------
// formatRawMatrix -- compute column widths for raw matrix
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn formatRawMatrix(x: SEXP, n: R_xlen_t, w: *mut c_int) {
    unsafe {
        if x.is_null() || n <= 0 {
            return;
        }
        // Raw format width is always 2 (hex representation "00".."ff")
        if !w.is_null() {
            *w = 2;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn test_print_matrix_null() {
        unsafe {
            printMatrix(
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
            );
        }
    }

    #[test]
    fn test_print_array_null() {
        unsafe {
            printArray(ptr::null_mut(), ptr::null_mut(), 0, 0, ptr::null_mut());
        }
    }

    #[test]
    fn test_ceil_div() {
        assert_eq!(ceil_DIV(10, 3), 4);
        assert_eq!(ceil_DIV(9, 3), 3);
        assert_eq!(ceil_DIV(1, 3), 1);
        assert_eq!(ceil_DIV(0, 3), 0);
        assert_eq!(ceil_DIV(7, 1), 7);
    }

    #[test]
    fn test_format_logical_matrix_null() {
        unsafe {
            let mut w: c_int = 0;
            formatLogicalMatrix(ptr::null_mut(), 0, &mut w);
            assert_eq!(w, 0);
        }
    }

    #[test]
    fn test_format_integer_matrix_null() {
        unsafe {
            let mut w: c_int = 0;
            formatIntegerMatrix(ptr::null_mut(), 0, &mut w);
            assert_eq!(w, 0);
        }
    }

    #[test]
    fn test_format_real_matrix_null() {
        unsafe {
            let mut w: c_int = 0;
            let mut d: c_int = 0;
            let mut e: c_int = 0;
            formatRealMatrix(ptr::null_mut(), 0, &mut w, &mut d, &mut e);
            assert_eq!(w, 0);
        }
    }

    #[test]
    fn test_format_complex_matrix_null() {
        unsafe {
            let mut wr: c_int = 0;
            let mut dr: c_int = 0;
            let mut er: c_int = 0;
            let mut wi: c_int = 0;
            let mut di: c_int = 0;
            let mut ei: c_int = 0;
            formatComplexMatrix(
                ptr::null_mut(),
                0,
                &mut wr,
                &mut dr,
                &mut er,
                &mut wi,
                &mut di,
                &mut ei,
            );
            assert_eq!(wr, 0);
            assert_eq!(wi, 0);
        }
    }

    #[test]
    fn test_format_string_matrix_null() {
        unsafe {
            let mut w: c_int = 0;
            formatStringMatrix(ptr::null_mut(), 0, &mut w, 0);
            assert_eq!(w, 0);
        }
    }

    #[test]
    fn test_format_raw_matrix_null() {
        unsafe {
            let mut w: c_int = 0;
            formatRawMatrix(ptr::null_mut(), 0, &mut w);
            assert_eq!(w, 0);
        }
    }
}
