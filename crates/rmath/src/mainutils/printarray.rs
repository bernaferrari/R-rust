#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/printarray.c -- matrix and array printing.
//!
//! Provides printMatrix() and printArray() for displaying R matrices and arrays.
//! Also provides format*Matrix() functions for determining column widths.

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::mainutils::printvector::GetMatrixDimnames;
use crate::sexp::accessors::{LENGTH, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH};
use crate::sexp::altseq::{CompactSeq, unexpanded_int, unexpanded_real};
use crate::sexp::ffi::R_xlen_t;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, NA_REAL, R_IsNA, Rcomplex, SEXP};
use crate::sexp::globals::R_NilValue;

// `eprint!` during `cargo test` never reaches fd 2. These shadows keep the
// call sites unchanged and let a test install [`printutils::console_emit`].
macro_rules! eprint {
    ($($arg:tt)*) => {
        crate::mainutils::printutils::console_emit(format_args!($($arg)*))
    };
}
macro_rules! eprintln {
    () => {
        crate::mainutils::printutils::console_emit(format_args!("\n"))
    };
    ($($arg:tt)*) => {{
        crate::mainutils::printutils::console_emit(format_args!($($arg)*));
        crate::mainutils::printutils::console_emit(format_args!("\n"));
    }};
}

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
const LGLSXP_VAL: i32 = 10;
/// INTSXP type constant.
const INTSXP_VAL: i32 = 13;
/// REALSXP type constant.
const REALSXP_VAL: i32 = 14;
/// CPLXSXP type constant.
const CPLXSXP_VAL: i32 = 15;
/// STRSXP type constant.
const STRSXP_VAL: i32 = 16;
/// RAWSXP type constant.
const RAWSXP_VAL: i32 = 24;

/// OutDec: decimal separator character (always '.' in this port).
static OUT_DEC: [c_char; 2] = [b'.' as c_char, 0];

/// Rbyte type alias.
type Rbyte = u8;

// ---------------------------------------------------------------------------
// R_print full parameters -- imported from printvector
// ---------------------------------------------------------------------------

use crate::mainutils::printvector::{MutPtr, R_PrintData};

/// Get the global R_print parameters.
fn get_R_print_full() -> MutPtr<R_PrintData> {
    crate::mainutils::printvector::get_R_PrintData()
}

// ---------------------------------------------------------------------------
// Checked SEXP boundary helpers
// ---------------------------------------------------------------------------

/// Checked node factory bound to the active runtime session.
fn node_factory() -> crate::sexp::object::SessionNodeFactory<'static> {
    crate::sexp::object::SessionNodeFactory::new(
        // SAFETY: matrix printing only runs inside evaluated R calls.
        unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
    )
}

fn wrapped(x: SEXP) -> crate::sexp::object::Sexp<'static> {
    node_factory()
        .wrap(x)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

fn checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// NUL-terminated pointer text (null maps to "").
fn cstr_text<'a>(s: *const c_char) -> &'a str {
    if s.is_null() {
        return "";
    }
    // SAFETY: callers pass NUL-terminated static buffers or null.
    unsafe { std::ffi::CStr::from_ptr(s) }
        .to_str()
        .unwrap_or("")
}

/// Encode a string element to owned text (null encodes as "").
fn encode_label(s: SEXP, w: c_int, quote: c_int, justify: Rprt_adj) -> String {
    crate::mainutils::printutils::encode_string_bytes(s, w, quote, justify)
        .map(text_string)
        .unwrap_or_default()
}

/// Encode a logical value to owned text.
fn encode_logical(x: c_int, w: c_int) -> String {
    text_string(crate::mainutils::printutils::encode_logical_text(x, w))
}

/// Encode an integer value to owned text.
fn encode_integer(x: c_int, w: c_int) -> String {
    text_string(crate::mainutils::printutils::encode_integer_text(x, w))
}

/// Encode a real value to owned text (decimal mark `.`).
fn encode_real(x: c_double, w: c_int, d: c_int, e: c_int) -> String {
    text_string(crate::mainutils::printutils::encode_real0_text(
        x, w, d, e, b".",
    ))
}

/// Encode a complex value to owned text.
fn encode_complex(
    z: Rcomplex,
    wr: c_int,
    dr: c_int,
    er: c_int,
    wi: c_int,
    di: c_int,
    ei: c_int,
) -> String {
    text_string(crate::mainutils::printutils::encode_complex_text(
        z, wr, dr, er, wi, di, ei,
    ))
}

/// Encode a raw byte to owned text (no prefix).
fn encode_raw(x: u8) -> String {
    text_string(crate::mainutils::printutils::encode_raw_text(x, b""))
}

/// Encoder bytes as text; invalid UTF-8 reads as empty, like `CStr::to_str`.
fn text_string(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Functions from printutils (non-SEXP taking ones)
// ---------------------------------------------------------------------------

use crate::mainutils::format::{
    RealField, compact_int_width, compact_real_field, integer_field_width, real_field,
};
use crate::mainutils::printutils::IndexWidth_xlen as IndexWidth;
use crate::mainutils::printutils::Rprt_adj;
use crate::mainutils::printvector::printVector;

// ---------------------------------------------------------------------------
// Local wrappers for printutils functions that take SEXP
//
// printutils.rs defines its own SEXP type (*mut SEXPREC) which is different
// from sexp::ffi::SEXP (*mut SexprecCore). Since Rstrlen and EncodeString
// are stubs (returning 0 and "" respectively), we define local wrappers.
// ---------------------------------------------------------------------------

fn local_Rstrlen(s: SEXP, quote: c_int) -> c_int {
    crate::mainutils::printutils::Rstrlen(s, quote)
}

static EMPTY_CSTR: [u8; 1] = [0];

use crate::sexp::attrib_core::getAttrib;

// ---------------------------------------------------------------------------
// NA_STRING -- sentinel; R_NilValue is used since real NA_STRING CHARSXP
// is not yet available.
// ---------------------------------------------------------------------------

fn NA_STRING() -> SEXP {
    crate::mainutils::relop::NA_STRING()
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

fn strwidth(s: *const c_char) -> c_int {
    {
        if s.is_null() {
            return 0;
        }
        // SAFETY: callers pass NUL-terminated strings or null.
        let len = unsafe { std::ffi::CStr::from_ptr(s) }.to_bytes().len();
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

fn MatrixColumnLabel(cl: SEXP, j: c_int, w: c_int) {
    {
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
            eprint!("{}", encode_label(tmp, l, 0, Rprt_adj::left));
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

fn RightMatrixColumnLabel(cl: SEXP, j: c_int, w: c_int) {
    {
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
            eprint!("{}", encode_label(tmp, l, 0, Rprt_adj::right));
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

fn LeftMatrixColumnLabel(cl: SEXP, j: c_int, w: c_int) {
    {
        let rp = get_R_print_full();
        if cl != R_NilValue() {
            let tmp = STRING_ELT(cl, j as R_xlen_t);
            let l = if tmp == NA_STRING() {
                rp.na_width_noquote
            } else {
                local_Rstrlen(tmp, 0)
            };
            eprint!("{:width$}", "", width = rp.gap as usize);
            eprint!("{}", encode_label(tmp, l, 0, Rprt_adj::left));
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

fn MatrixRowLabel(rl: SEXP, i: c_int, rlabw: c_int, lbloff: c_int) {
    {
        let rp = get_R_print_full();
        if rl != R_NilValue() {
            let tmp = STRING_ELT(rl, i as R_xlen_t);
            let l = if tmp == NA_STRING() {
                rp.na_width_noquote
            } else {
                local_Rstrlen(tmp, 0)
            };
            eprint!("\n{:width$}", "", width = lbloff as usize);
            eprint!("{}", encode_label(tmp, l, 0, Rprt_adj::left));
            let pad = rlabw - l - lbloff;
            if pad > 0 {
                eprint!("{:width$}", "", width = pad as usize);
            }
        } else {
            let iw = IndexWidth((i + 1) as R_xlen_t);
            let pad = rlabw - 3 - iw;
            eprintln!();
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

fn init_rl_rn(rl: SEXP, rn: *const c_char, r: c_int) -> RowLabelInfo {
    {
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

fn print_row_header(cn: *const c_char, rlabw: c_int, rn: *const c_char) {
    {
        // GNU `_PRINT_ROW_LAB`: Rprintf("%*s%s\n", rlabw, "", cn)
        // then left-justified rn (or rlabw spaces).
        if !cn.is_null() {
            eprint!("{:width$}", "", width = rlabw.max(0) as usize);
            eprintln!("{}", cstr_text(cn));
        }
        if !rn.is_null() {
            eprint!("{:<width$}", cstr_text(rn), width = rlabw.max(0) as usize);
        } else {
            eprint!("{:width$}", "", width = rlabw.max(0) as usize);
        }
    }
}

// ---------------------------------------------------------------------------
// print_column_labels -- standard column labels
// ---------------------------------------------------------------------------

fn std_column_labels(cl: SEXP, jmin: usize, jmax: usize, w: &[c_int]) {
    {
        for j in jmin..jmax {
            MatrixColumnLabel(cl, j as c_int, w[j]);
        }
    }
}

// ---------------------------------------------------------------------------
// print_logical_matrix
// ---------------------------------------------------------------------------

fn print_logical_matrix(
    sx: SEXP,
    offset: R_xlen_t,
    r_pr: c_int,
    r: c_int,
    c: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
    print_ij: bool,
) {
    {
        let rp = get_R_print_full();
        let info = init_rl_rn(rl, rn, r);
        let rlabw = info.rlabw;
        let lbloff = info.lbloff;

        let mut w = vec![0i32; c as usize];
        let mut clabw: c_int = 0;

        let base = Cells::new(sx);
        let len = XLENGTH(sx);
        let rows = r as R_xlen_t;

        // Compute w[j] for each column
        for j in 0..c as usize {
            if print_ij {
                let start = column_start(offset, j as R_xlen_t, rows);
                w[j] = if base.column_in_range(start, rows) && rows > 0 {
                    crate::mainutils::format::logical_field_width(
                        (0..rows).map(|i| base.logical(start + i)),
                    )
                } else {
                    1
                };
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
                            let val = logical_at(
                                &base,
                                len,
                                element_index(offset, j as R_xlen_t, i as R_xlen_t, rows),
                            );
                            eprint!("{}", encode_logical(val, w[j]));
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
// Compact sequences have no element buffer. The formula is an owned copy.
// A null data pointer is never offset. Its column width matches
// formatInteger/formatReal/formatLogical on a null pointer. Elements still
// print as NA.
// ---------------------------------------------------------------------------

/// Column origin in column-major order.
///
/// `col * nrow` is formed in `R_xlen_t`. The same product in `c_int` wraps
/// once a matrix is longer than `c_int::MAX`, and a wrapped origin is either
/// the wrong column or an offset off the buffer.
fn column_count(len: R_xlen_t, nrow: R_xlen_t) -> usize {
    if nrow <= 0 || len <= 0 {
        0
    } else {
        usize::try_from(len / nrow).unwrap_or(0)
    }
}

fn column_start(offset: R_xlen_t, col: R_xlen_t, nrow: R_xlen_t) -> R_xlen_t {
    col.checked_mul(nrow)
        .and_then(|scaled| scaled.checked_add(offset))
        .unwrap_or(R_xlen_t::MAX)
}

fn element_index(offset: R_xlen_t, col: R_xlen_t, row: R_xlen_t, nrow: R_xlen_t) -> R_xlen_t {
    column_start(offset, col, nrow)
        .checked_add(row)
        .unwrap_or(R_xlen_t::MAX)
}

/// Checked element source for one matrix payload. Out-of-range or absent
/// cells read as the type's missing value, matching the former bounded
/// buffer loads.
struct Cells {
    src: Option<crate::sexp::object::Sexp<'static>>,
    len: R_xlen_t,
}

impl Cells {
    fn new(x: SEXP) -> Self {
        Cells {
            src: Some(wrapped(x)),
            len: XLENGTH(x),
        }
    }

    /// A payload with no readable buffer (compact sequences): every cell is missing.
    fn absent(x: SEXP) -> Self {
        Cells {
            src: None,
            len: XLENGTH(x),
        }
    }

    /// Whether `n` cells from `start` form a readable in-range column.
    fn column_in_range(&self, start: R_xlen_t, n: R_xlen_t) -> bool {
        self.src.is_some()
            && start >= 0
            && n >= 0
            && start <= self.len
            && n <= self.len.saturating_sub(start)
    }

    fn read<T>(
        &self,
        index: R_xlen_t,
        missing: T,
        elt: impl FnOnce(
            &crate::sexp::object::Sexp<'static>,
            R_xlen_t,
        ) -> crate::sexp::object::SexpResult<T>,
    ) -> T {
        match &self.src {
            Some(src) if index >= 0 && index < self.len => checked(elt(src, index)),
            _ => missing,
        }
    }

    fn logical(&self, index: R_xlen_t) -> c_int {
        self.read(index, NA_LOGICAL, |src, i| src.try_logical_elt(i))
    }

    fn integer(&self, index: R_xlen_t) -> c_int {
        self.read(index, NA_INTEGER, |src, i| src.try_integer_elt(i))
    }

    fn real(&self, index: R_xlen_t) -> c_double {
        self.read(index, NA_REAL, |src, i| src.try_real_elt(i))
    }

    fn complex(&self, index: R_xlen_t) -> Rcomplex {
        let missing = Rcomplex {
            r: NA_REAL,
            i: NA_REAL,
        };
        self.read(index, missing, |src, i| src.try_complex_elt(i))
    }

    fn raw(&self, index: R_xlen_t) -> Rbyte {
        self.read(index, 0, |src, i| src.try_raw_elt(i))
    }
}

fn logical_at(data: &Cells, _len: R_xlen_t, index: R_xlen_t) -> c_int {
    data.logical(index)
}

fn complex_at(data: &Cells, _len: R_xlen_t, index: R_xlen_t) -> Rcomplex {
    data.complex(index)
}

fn raw_at(data: &Cells, _len: R_xlen_t, index: R_xlen_t) -> Rbyte {
    data.raw(index)
}

fn int_width_at(
    seq: Option<CompactSeq>,
    data: &Cells,
    len: R_xlen_t,
    start: R_xlen_t,
    n: R_xlen_t,
) -> c_int {
    if let Some(seq) = seq {
        return compact_int_width(seq, start, n);
    }
    if data.src.is_none() || (data.column_in_range(start, n) && n <= 0) {
        // GNU formatInteger on a null pointer or empty span.
        return 1;
    }
    integer_field_width(n, |i| {
        data.integer(start.checked_add(i).unwrap_or(R_xlen_t::MAX))
    })
}

fn int_at(seq: Option<CompactSeq>, data: &Cells, _len: R_xlen_t, index: R_xlen_t) -> c_int {
    if let Some(seq) = seq {
        return seq.int_or_na(index);
    }
    data.integer(index)
}

fn real_format_at(
    seq: Option<CompactSeq>,
    data: &Cells,
    len: R_xlen_t,
    start: R_xlen_t,
    n: R_xlen_t,
) -> RealField {
    if let Some(seq) = seq {
        return compact_real_field(seq, start, n, 0);
    }
    if data.src.is_none() || n <= 0 {
        // GNU formatReal on a null pointer or empty span.
        return RealField { w: 0, d: 0, e: 0 };
    }
    real_field(n, 0, |i| {
        data.real(start.checked_add(i).unwrap_or(R_xlen_t::MAX))
    })
}

fn real_at(seq: Option<CompactSeq>, data: &Cells, _len: R_xlen_t, index: R_xlen_t) -> c_double {
    if let Some(seq) = seq {
        return seq.real_or_na(index);
    }
    data.real(index)
}

// ---------------------------------------------------------------------------
// print_integer_matrix
// ---------------------------------------------------------------------------

fn print_integer_matrix(
    sx: SEXP,
    offset: R_xlen_t,
    r_pr: c_int,
    r: c_int,
    c: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
    print_ij: bool,
) {
    {
        let rp = get_R_print_full();
        let info = init_rl_rn(rl, rn, r);
        let rlabw = info.rlabw;
        let lbloff = info.lbloff;

        let mut w = vec![0i32; c as usize];
        let mut clabw: c_int = 0;

        let seq = unexpanded_int(sx);
        let data = if seq.is_some() {
            Cells::absent(sx)
        } else {
            Cells::new(sx)
        };
        let len = XLENGTH(sx);
        let rows = r as R_xlen_t;

        for j in 0..c as usize {
            if print_ij {
                let start = column_start(offset, j as R_xlen_t, rows);
                let fw = int_width_at(seq, &data, len, start, rows);
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
                            let val = int_at(
                                seq,
                                &data,
                                len,
                                element_index(offset, j as R_xlen_t, i as R_xlen_t, rows),
                            );
                            eprint!("{}", encode_integer(val, w[j]));
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

fn print_real_matrix(
    sx: SEXP,
    offset: R_xlen_t,
    r_pr: c_int,
    r: c_int,
    c: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
    print_ij: bool,
) {
    {
        let rp = get_R_print_full();
        let info = init_rl_rn(rl, rn, r);
        let rlabw = info.rlabw;
        let lbloff = info.lbloff;

        let mut w = vec![0i32; c as usize];
        let mut d = vec![0i32; c as usize];
        let mut e = vec![0i32; c as usize];
        let mut clabw: c_int = 0;

        let seq = unexpanded_real(sx);
        let data = if seq.is_some() {
            Cells::absent(sx)
        } else {
            Cells::new(sx)
        };
        let len = XLENGTH(sx);
        let rows = r as R_xlen_t;

        for j in 0..c as usize {
            if print_ij {
                let start = column_start(offset, j as R_xlen_t, rows);
                let fmt = real_format_at(seq, &data, len, start, rows);
                w[j] = fmt.w;
                d[j] = fmt.d;
                e[j] = fmt.e;
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
                            let val = real_at(
                                seq,
                                &data,
                                len,
                                element_index(offset, j as R_xlen_t, i as R_xlen_t, rows),
                            );
                            eprint!("{}", encode_real(val, w[j], d[j], e[j]));
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

fn print_complex_matrix(
    sx: SEXP,
    offset: R_xlen_t,
    r_pr: c_int,
    r: c_int,
    c: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
    print_ij: bool,
) {
    {
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

        let base = Cells::new(sx);
        let len = XLENGTH(sx);
        let rows = r as R_xlen_t;

        for j in 0..c as usize {
            if print_ij {
                let start = column_start(offset, j as R_xlen_t, rows);
                let f = if base.column_in_range(start, rows) && rows > 0 {
                    let column: Vec<Rcomplex> =
                        (0..rows).map(|i| base.complex(start + i)).collect();
                    crate::mainutils::format::complex_field(&column, 0)
                } else {
                    crate::mainutils::format::ComplexField::default()
                };
                (wr[j], dr[j], er[j], wi[j], di[j], ei[j]) = (f.wr, f.dr, f.er, f.wi, f.di, f.ei);
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
                            let cx = complex_at(
                                &base,
                                len,
                                element_index(offset, j as R_xlen_t, i as R_xlen_t, rows),
                            );
                            let s = if R_IsNA(cx.r) || R_IsNA(cx.i) {
                                encode_real(NA_REAL, w[j], 0, 0)
                            } else {
                                encode_complex(
                                    cx,
                                    w[j] - wi[j] - 2,
                                    dr[j],
                                    er[j],
                                    wi[j],
                                    di[j],
                                    ei[j],
                                )
                            };
                            eprint!("{}", s);
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

fn print_string_matrix(
    sx: SEXP,
    offset: R_xlen_t,
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
    {
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
                    let elem =
                        STRING_ELT(sx, element_index(offset, j as R_xlen_t, i, r as R_xlen_t));
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
                                element_index(offset, j as R_xlen_t, i as R_xlen_t, r as R_xlen_t),
                            );
                            eprint!("{:width$}", "", width = rp.gap as usize);
                            let adj = if right != 0 {
                                Rprt_adj::right
                            } else {
                                Rprt_adj::left
                            };
                            eprint!("{}", encode_label(elem, w[j], quote, adj));
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

fn print_raw_matrix(
    sx: SEXP,
    offset: R_xlen_t,
    r_pr: c_int,
    r: c_int,
    c: c_int,
    rl: SEXP,
    cl: SEXP,
    rn: *const c_char,
    cn: *const c_char,
    print_ij: bool,
) {
    {
        let rp = get_R_print_full();
        let info = init_rl_rn(rl, rn, r);
        let rlabw = info.rlabw;
        let lbloff = info.lbloff;

        let mut w = vec![0i32; c as usize];
        let mut clabw: c_int = 0;

        let base = Cells::new(sx);
        let len = XLENGTH(sx);
        let rows = r as R_xlen_t;

        for j in 0..c as usize {
            if print_ij {
                let start = column_start(offset, j as R_xlen_t, rows);
                // GNU formatRaw: always two hex digits.
                w[j] = 2;
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
                            let val = raw_at(
                                &base,
                                len,
                                element_index(offset, j as R_xlen_t, i as R_xlen_t, rows),
                            );
                            let pad = w[j] - 2;
                            if pad > 0 {
                                eprint!("{:width$}", "", width = pad as usize);
                            }
                            eprint!("{}", encode_raw(val));
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

pub unsafe fn printMatrix(
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
    {
        if x.is_null() || dim.is_null() {
            return;
        }

        let rp = get_R_print_full();
        // An empty dim has no payload to read (formerly a null INTEGER()).
        if XLENGTH(dim) == 0 {
            return;
        }
        let dims = wrapped(dim);
        let r = checked(dims.try_integer_elt(0));
        let c = checked(dims.try_integer_elt(1));
        if r < 0 || c < 0 {
            return;
        }
        let origin = offset as R_xlen_t;

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
            LGLSXP_VAL => {
                print_logical_matrix(x, origin, r_pr, r, c_pr, rl, cl, rn, cn, true);
            }
            INTSXP_VAL => {
                print_integer_matrix(x, origin, r_pr, r, c_pr, rl, cl, rn, cn, true);
            }
            REALSXP_VAL => {
                print_real_matrix(x, origin, r_pr, r, c_pr, rl, cl, rn, cn, true);
            }
            CPLXSXP_VAL => {
                print_complex_matrix(x, origin, r_pr, r, c_pr, rl, cl, rn, cn, true);
            }
            STRSXP_VAL => {
                let q = if quote != 0 { b'"' as c_int } else { 0 };
                print_string_matrix(x, origin, r_pr, r, c_pr, q, right, rl, cl, rn, cn, true);
            }
            RAWSXP_VAL => {
                print_raw_matrix(x, origin, r_pr, r, c_pr, rl, cl, rn, cn, true);
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

#[allow(clippy::if_same_then_else)]
pub fn printArray(x: SEXP, dim: SEXP, quote: c_int, right: c_int, dimnames: SEXP) {
    {
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
            // SAFETY: all four out pointers reference live locals; `rn`/`cn`
            // may be null or NUL-terminated names returned by GetMatrixDimnames
            // for the duration of the printMatrix call.
            unsafe {
                GetMatrixDimnames(x, &mut rl, &mut cl, &mut rn, &mut cn);
                printMatrix(x, 0, dim, quote, 0, rl, cl, rn, cn);
            }
            return;
        }

        // ndim >= 3
        let rp = get_R_print_full();
        // An empty dim has no payload to read (formerly a null INTEGER()).
        if XLENGTH(dim) == 0 {
            return;
        }

        let dims = wrapped(dim);
        let nr = checked(dims.try_integer_elt(0));
        let nc = checked(dims.try_integer_elt(1));
        if nr < 0 || nc < 0 {
            return;
        }
        // Slice length can exceed `c_int`. The printers take that origin as
        // `R_xlen_t`; the `max.print` cap still speaks `c_int`.
        let b_wide = (nr as R_xlen_t).saturating_mul(nc as R_xlen_t);
        let b = c_int::try_from(b_wide).unwrap_or(c_int::MAX);

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
            let names_sym = crate::sexp::attrib_core::R_NamesSymbol();
            dnn = getAttrib(dimnames, names_sym);
            has_dnn = dnn != R_NilValue();
            // translateChar not available; rn and cn remain null
        }

        // nb := number of matrix slices
        let mut nb: c_int = 1;
        for i in 2..ndim {
            nb *= checked(dims.try_integer_elt(i as R_xlen_t));
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
                    let dj = checked(dims.try_integer_elt(j as R_xlen_t));
                    let l = (ii as c_int / k) % dj + 1;
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
                    k *= dj;
                }
                eprintln!();
            } else {
                // nb == 0, e.g. <2 x 3 x 0 array>
                for di in 0..ndim {
                    let dd = checked(dims.try_integer_elt(di as R_xlen_t));
                    if di == 0 {
                        eprint!("<{}", dd);
                    } else {
                        eprint!(" x {}", dd);
                    }
                }
                eprintln!(" array of type {}>", TYPEOF(x));
            }

            let offset = (ii as R_xlen_t).saturating_mul(b_wide);
            match TYPEOF(x) {
                LGLSXP_VAL => {
                    print_logical_matrix(x, offset, use_nr, nr, use_nc, dn0, dn1, rn, cn, do_ij);
                }
                INTSXP_VAL => {
                    print_integer_matrix(x, offset, use_nr, nr, use_nc, dn0, dn1, rn, cn, do_ij);
                }
                REALSXP_VAL => {
                    print_real_matrix(x, offset, use_nr, nr, use_nc, dn0, dn1, rn, cn, do_ij);
                }
                CPLXSXP_VAL => {
                    print_complex_matrix(x, offset, use_nr, nr, use_nc, dn0, dn1, rn, cn, do_ij);
                }
                STRSXP_VAL => {
                    let q = if quote != 0 { b'"' as c_int } else { 0 };
                    print_string_matrix(
                        x, offset, use_nr, nr, use_nc, q, right, dn0, dn1, rn, cn, do_ij,
                    );
                }
                RAWSXP_VAL => {
                    print_raw_matrix(x, offset, use_nr, nr, use_nc, dn0, dn1, rn, cn, do_ij);
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for matrix printing
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

pub unsafe fn formatLogicalMatrix(x: SEXP, n: R_xlen_t, w: *mut c_int) {
    unsafe {
        if x.is_null() || n <= 0 {
            return;
        }
        let len = XLENGTH(x);
        let columns = column_count(len, n);

        let data = Cells::new(x);
        let mut max_w: c_int = 0;

        for j in 0..columns {
            let start = column_start(0, j as R_xlen_t, n);
            let fw = if data.column_in_range(start, n) {
                crate::mainutils::format::logical_field_width(
                    (0..n).map(|i| data.logical(start + i)),
                )
            } else {
                1
            };
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

pub unsafe fn formatIntegerMatrix(x: SEXP, n: R_xlen_t, w: *mut c_int) {
    unsafe {
        if x.is_null() || n <= 0 {
            return;
        }
        let len = XLENGTH(x);
        let columns = column_count(len, n);

        let seq = unexpanded_int(x);
        let data = if seq.is_some() {
            Cells::absent(x)
        } else {
            Cells::new(x)
        };
        let mut max_w: c_int = 0;

        for j in 0..columns {
            let start = column_start(0, j as R_xlen_t, n);
            let fw = int_width_at(seq, &data, len, start, n);
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

pub unsafe fn formatRealMatrix(x: SEXP, n: R_xlen_t, w: *mut c_int, d: *mut c_int, e: *mut c_int) {
    unsafe {
        if x.is_null() || n <= 0 {
            return;
        }
        let len = XLENGTH(x);
        let columns = column_count(len, n);

        let seq = unexpanded_real(x);
        let data = if seq.is_some() {
            Cells::absent(x)
        } else {
            Cells::new(x)
        };
        let mut max_w: c_int = 0;
        let mut best_d: c_int = 0;
        let mut best_e: c_int = 0;

        for j in 0..columns {
            let start = column_start(0, j as R_xlen_t, n);
            let fmt = real_format_at(seq, &data, len, start, n);
            if fmt.w > max_w {
                max_w = fmt.w;
                best_d = fmt.d;
                best_e = fmt.e;
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

pub unsafe fn formatComplexMatrix(
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
        let len = XLENGTH(x);
        let columns = column_count(len, n);

        let data = Cells::new(x);
        let mut max_wr: c_int = 0;
        let mut best_dr: c_int = 0;
        let mut best_er: c_int = 0;
        let mut max_wi: c_int = 0;
        let mut best_di: c_int = 0;
        let mut best_ei: c_int = 0;

        for j in 0..columns {
            let start = column_start(0, j as R_xlen_t, n);
            let f = if data.column_in_range(start, n) {
                let column: Vec<Rcomplex> = (0..n).map(|i| data.complex(start + i)).collect();
                crate::mainutils::format::complex_field(&column, 0)
            } else {
                crate::mainutils::format::ComplexField::default()
            };
            let (fwr, fdr, fer, fwi, fdi, fei) = (f.wr, f.dr, f.er, f.wi, f.di, f.ei);
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

pub unsafe fn formatStringMatrix(x: SEXP, n: R_xlen_t, w: *mut c_int, quote: c_int) {
    unsafe {
        if x.is_null() || n <= 0 {
            return;
        }
        let len = XLENGTH(x);
        let columns = column_count(len, n);

        let rp = get_R_print_full();
        let mut max_w: c_int = 0;

        for j in 0..columns {
            for i in 0..n {
                let elem = STRING_ELT(x, element_index(0, j as R_xlen_t, i, n));
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

pub unsafe fn formatRawMatrix(x: SEXP, n: R_xlen_t, w: *mut c_int) {
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
    fn decimal_separator_is_terminated_for_real_encoding() {
        let dec_ptr = OUT_DEC.as_ptr();
        // Numeric encoders consume this argument as a NUL-terminated C string.
        let separator = unsafe { std::ffi::CStr::from_ptr(dec_ptr) };
        assert_eq!(separator.to_bytes(), b".");
    }

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
