//! Matrix construction: lower/upper.tri, matrix(), array(), diag(), element access/coercion helpers, as.matrix — extracted verbatim from the former single-file module.
use super::*;
use crate::sexp::object::SexpMut;

// ---------------------------------------------------------------------------
// Matrix: lower.tri, upper.tri
// ---------------------------------------------------------------------------

/// R's `lower.tri(x, diag=FALSE)` — TRUE for lower triangle of matrix.
pub fn do_lower_tri(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let diag_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let include_diag = !diag_arg.is_null()
        && diag_arg != R_NilValue()
        && real_or_default(diag_arg, 0.0) != 0.0;
    let dim_attr = crate::sexp::attrib_core::getAttrib(x, install_str("dim"));
    let owner = active_owner();
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2
        {
            let dim_attr = owner_sexp(owner, dim_attr);
            (
                dim_attr
                    .try_integer_elt(0)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    as R_xlen_t,
                dim_attr
                    .try_integer_elt(1)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    as R_xlen_t,
            )
        } else {
            let n = XLENGTH(x);
            (n, 1)
        };
    let total = nrow * ncol;
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    let mut dst = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for j in 0..ncol {
        for i in 0..nrow {
            let idx = j * nrow + i;
            let is_lower = if include_diag { i >= j } else { i > j };
            dst.try_set_logical_elt(idx, if is_lower { TRUE } else { FALSE })
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
    result
}

/// R's `upper.tri(x, diag=FALSE)` — TRUE for upper triangle of matrix.
pub fn do_upper_tri(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let diag_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let include_diag = !diag_arg.is_null()
        && diag_arg != R_NilValue()
        && real_or_default(diag_arg, 0.0) != 0.0;
    let dim_attr = crate::sexp::attrib_core::getAttrib(x, install_str("dim"));
    let owner = active_owner();
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2
        {
            let dim_attr = owner_sexp(owner, dim_attr);
            (
                dim_attr
                    .try_integer_elt(0)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    as R_xlen_t,
                dim_attr
                    .try_integer_elt(1)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    as R_xlen_t,
            )
        } else {
            let n = XLENGTH(x);
            (n, 1)
        };
    let total = nrow * ncol;
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    let mut dst = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for j in 0..ncol {
        for i in 0..nrow {
            let idx = j * nrow + i;
            let is_upper = if include_diag { i <= j } else { i < j };
            dst.try_set_logical_elt(idx, if is_upper { TRUE } else { FALSE })
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Matrix operations: matrix(), t(), nrow(), ncol(), dim(), diag()
// ---------------------------------------------------------------------------

pub fn supported_matrix_type(t: c_int) -> bool {
    t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::RAWSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::VECSXP
        || t == SEXPTYPE::EXPRSXP
}

pub fn set_matrix_na_or_zero(x: SEXP, i: R_xlen_t) {
    let owner = active_owner();
    let Ok(mut x) = owner.sexp(x).and_then(SexpMut::try_from_checked) else {
        return;
    };
    match x.typeof_() {
        SEXPTYPE::REALSXP => {
            let _ = x.try_set_real_elt(i, NA_REAL);
        }
        SEXPTYPE::INTSXP => {
            let _ = x.try_set_integer_elt(i, NA_INTEGER);
        }
        SEXPTYPE::LGLSXP => {
            let _ = x.try_set_logical_elt(i, NA_INTEGER);
        }
        SEXPTYPE::CPLXSXP => {
            let _ = x.try_set_complex_elt(
                i,
                Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                },
            );
        }
        SEXPTYPE::RAWSXP => {
            let _ = x.try_set_raw_elt(i, 0 as Rbyte);
        }
        SEXPTYPE::STRSXP => {
            if let Ok(na) = owner.sexp(crate::sexp::globals::R_NaString()) {
                let _ = x.try_set_string_elt(i, na);
            }
        }
        SEXPTYPE::VECSXP => {
            if let Ok(nil) = owner.sexp(R_NilValue()) {
                let _ = x.try_set_vector_elt(i, nil);
            }
        }
        _ => {}
    }
}

/// Read element `i` of `x` as a CHARSXP, coercing non-character atomics via
/// their string form. Returns R_NaString when the source element is NA or the
/// index is out of range.
pub(crate) fn str_elt_or_na(x: SEXP, i: R_xlen_t) -> SEXP {
    if x.is_null() || x == R_NilValue() {
        return crate::sexp::globals::R_NaString();
    }
    let n = XLENGTH(x);
    if n == 0 {
        return crate::sexp::globals::R_NaString();
    }
    if TYPEOF(x) == SEXPTYPE::STRSXP {
        let owner = active_owner();
        let charsxp = owner
            .sexp(x)
            .and_then(|x| x.try_string_elt(i % n))
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            .as_raw();
        return if charsxp.is_null() {
            crate::sexp::globals::R_NaString()
        } else {
            charsxp
        };
    }
    if as_character_element_is_na(x, i % n) {
        return crate::sexp::globals::R_NaString();
    }
    let s = elt_to_string(x, i % n);
    let cstr = CString::new(s).unwrap_or_default();
    // SAFETY: cstr is a live NUL-terminated buffer for the call duration.
    let charsxp = mk_char_cstr(&cstr);
    if charsxp.is_null() {
        crate::sexp::globals::R_NaString()
    } else {
        charsxp
    }
}

/// Read element `i` of `x` as an integer, coercing logical/raw; NA otherwise.
pub(crate) fn int_elt_or_na(x: SEXP, i: R_xlen_t) -> c_int {
    if x.is_null() || x == R_NilValue() {
        return NA_INTEGER;
    }
    let n = XLENGTH(x);
    if n == 0 {
        return NA_INTEGER;
    }
    let idx = i % n;
    match TYPEOF(x) {
        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
            let owner = active_owner();
            integer_like_elt(&owner_sexp(owner, x), idx)
        }
        t if t == SEXPTYPE::RAWSXP => {
            let owner = active_owner();
            let x = owner_sexp(owner, x);
            x.try_raw_elt(idx)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                as c_int
        }
        _ => NA_INTEGER,
    }
}

/// Read element `i` of a raw vector as a byte (0 when out of range).
pub(crate) fn raw_elt_or_zero(x: SEXP, i: R_xlen_t) -> Rbyte {
    if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::RAWSXP || XLENGTH(x) == 0 {
        return 0;
    }
    let idx = i % XLENGTH(x);
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    x.try_raw_elt(idx)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Read element `i` of `x` as a complex value; NA complex otherwise.
pub(crate) fn cplx_elt_or_na(x: SEXP, i: R_xlen_t) -> Rcomplex {
    if !x.is_null() && x != R_NilValue() && TYPEOF(x) == SEXPTYPE::CPLXSXP && XLENGTH(x) != 0 {
        let idx = i % XLENGTH(x);
        let owner = active_owner();
        let x = owner_sexp(owner, x);
        return x
            .try_complex_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    Rcomplex {
        r: NA_REAL,
        i: NA_REAL,
    }
}
pub fn set_matrix_zero(x: SEXP, i: R_xlen_t) {
    let owner = active_owner();
    let Ok(mut x) = owner.sexp(x).and_then(SexpMut::try_from_checked) else {
        return;
    };
    match x.typeof_() {
        SEXPTYPE::REALSXP => {
            let _ = x.try_set_real_elt(i, 0.0);
        }
        SEXPTYPE::INTSXP => {
            let _ = x.try_set_integer_elt(i, 0);
        }
        SEXPTYPE::LGLSXP => {
            let _ = x.try_set_logical_elt(i, FALSE);
        }
        SEXPTYPE::CPLXSXP => {
            let _ = x.try_set_complex_elt(i, Rcomplex { r: 0.0, i: 0.0 });
        }
        SEXPTYPE::RAWSXP => {
            let _ = x.try_set_raw_elt(i, 0 as Rbyte);
        }
        SEXPTYPE::STRSXP => {
            // SAFETY: the empty string literal outlives this call.
            let empty = mk_char_cstr(c"");
            if let Ok(empty) = owner.sexp(empty) {
                let _ = x.try_set_string_elt(i, empty);
            }
        }
        SEXPTYPE::VECSXP => {
            if let Ok(nil) = owner.sexp(R_NilValue()) {
                let _ = x.try_set_vector_elt(i, nil);
            }
        }
        _ => {}
    }
}

pub(crate) fn copy_matrix_element(dst: SEXP, dst_i: R_xlen_t, src: SEXP, src_i: R_xlen_t) {
    copy_vector_elt(dst, dst_i, src, src_i);
}

pub(crate) fn set_two_dim_attr(x: SEXP, nrow: R_xlen_t, ncol: R_xlen_t) {
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if dim.is_null() {
        return;
    }
    let _dim_guard = protect(dim);
    let owner = active_owner();
    let mut dim_mut = owner
        .sexp(dim)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    dim_mut
        .try_set_integer_elt(0, nrow as c_int)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    dim_mut
        .try_set_integer_elt(1, ncol as c_int)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_DimSymbol(), dim);
}

pub fn set_diagonal_identity_value(x: SEXP, i: R_xlen_t) {
    let owner = active_owner();
    let Ok(mut x) = owner.sexp(x).and_then(SexpMut::try_from_checked) else {
        return;
    };
    match x.typeof_() {
        SEXPTYPE::REALSXP => {
            let _ = x.try_set_real_elt(i, 1.0);
        }
        SEXPTYPE::INTSXP => {
            let _ = x.try_set_integer_elt(i, 1);
        }
        SEXPTYPE::LGLSXP => {
            let _ = x.try_set_logical_elt(i, TRUE);
        }
        SEXPTYPE::CPLXSXP => {
            let _ = x.try_set_complex_elt(i, Rcomplex { r: 1.0, i: 0.0 });
        }
        SEXPTYPE::RAWSXP => {
            let _ = x.try_set_raw_elt(i, 1 as Rbyte);
        }
        _ => {}
    }
}

pub fn string_vector_contains_value(x: SEXP, needle: &str) -> bool {
    if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP {
        return false;
    }
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    for i in 0..XLENGTH(x.as_raw()) {
        if let Ok(elt) = x.try_string_elt(i)
            && elt.try_char_cstr_eq(needle.as_bytes()).unwrap_or(false)
        {
            return true;
        }
    }
    false
}

pub fn is_data_frame_object(x: SEXP) -> bool {
    // SAFETY: x is a live SEXP retained by the caller's argument graph.
    let class = crate::sexp::attrib_core::getAttrib(x, install_str("class"));
    string_vector_contains_value(class, "data.frame")
}

pub(crate) fn data_frame_row_count(x: SEXP) -> R_xlen_t {
    let row_names = crate::sexp::attrib_core::getAttrib(x, install_str("row.names"));
    if !row_names.is_null() && row_names != R_NilValue() {
        let t = TYPEOF(row_names);
        let len = XLENGTH(row_names);
        let compact = t == SEXPTYPE::INTSXP && len == 2 && {
            let owner = active_owner();
            owner_sexp(owner, row_names)
                .try_integer_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                == NA_INTEGER
        };
        if compact {
            let owner = active_owner();
            let second = owner_sexp(owner, row_names)
                .try_integer_elt(1)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if second != NA_INTEGER && second != 0 {
                return (second as R_xlen_t).unsigned_abs() as R_xlen_t;
            }
        } else if (t == SEXPTYPE::STRSXP || t == SEXPTYPE::INTSXP) && len > 0 {
            return len;
        }
    }

    let mut rows = 0;
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    for i in 0..XLENGTH(x) {
        let col = x_owned
            .try_vector_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if !col.as_raw().is_null() {
            rows = rows.max(column_row_count(col.as_raw()));
        }
    }
    rows
}

pub(crate) fn column_row_count(col: SEXP) -> R_xlen_t {
    if col.is_null() || col == R_NilValue() {
        return 0;
    }
    let dim = crate::sexp::attrib_core::getAttrib(col, install_str("dim"));
    if !dim.is_null() && TYPEOF(dim) == SEXPTYPE::INTSXP && LENGTH(dim) >= 1 {
        let owner = active_owner();
        let n = owner_sexp(owner, dim)
            .try_integer_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if n > 0 {
            return n as R_xlen_t;
        }
    }
    XLENGTH(col)
}

/// R's `matrix(data, nrow, ncol, byrow, dimnames)` — create a matrix.
pub fn do_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let data = arg_by_name_or_position(args, &["data"], 0);
    let nrow_arg = arg_by_name_or_position(args, &["nrow"], 1);
    let ncol_arg = arg_by_name_or_position(args, &["ncol"], 2);
    let byrow_arg = arg_by_name_or_position(args, &["byrow"], 3);
    let dimnames = arg_by_name_or_position(args, &["dimnames"], 4);
    // GNU base/matrix.R passes `missing(nrow)` / `missing(ncol)` as
    // R-level formal default and must be treated as not supplied.
    let flag_true = |value: SEXP| -> bool {
        !value.is_null() && value != R_NilValue() && TYPEOF(value) == SEXPTYPE::LGLSXP && {
            let owner = active_owner();
            owner_sexp(owner, value)
                .try_logical_elt(0)
                .unwrap_or(NA_INTEGER)
                == 1
        }
    };
    let nrow_missing = flag_true(arg_by_name_or_position(args, &[], 5));
    let ncol_missing = flag_true(arg_by_name_or_position(args, &[], 6));

    if data.is_null() || data == R_NilValue() {
        return R_NilValue();
    }

    let data_len = XLENGTH(data);
    let dimension = |value: SEXP, name: &str| -> Option<R_xlen_t> {
        if value.is_null() || value == R_NilValue() {
            return None;
        }
        let number = real_or_default(value, f64::NAN).trunc();
        if !number.is_finite() || number < 0. || number > i32::MAX as f64 {
            base_error(format!("invalid '{name}' value (too large or NA)"));
        }
        Some(number as R_xlen_t)
    };
    let rows = if nrow_missing {
        None
    } else {
        dimension(nrow_arg, "nrow")
    };
    let cols = if ncol_missing {
        None
    } else {
        dimension(ncol_arg, "ncol")
    };
    let infer = |known: R_xlen_t| {
        if known == 0 {
            if data_len > 0 {
                base_error("non-empty data for zero-extent matrix");
            }
            0
        } else {
            data_len / known + R_xlen_t::from(data_len % known != 0)
        }
    };
    let (nrow, ncol) = match (rows, cols) {
        (None, None) => (data_len, 1),
        (Some(rows), None) => (rows, infer(rows)),
        (None, Some(cols)) => (infer(cols), cols),
        (Some(rows), Some(cols)) => (rows, cols),
    };
    if nrow > i32::MAX as i64 || ncol > i32::MAX as i64 {
        base_error("matrix dimensions exceed integer limits");
    }
    let length = nrow
        .checked_mul(ncol)
        .unwrap_or_else(|| base_error("matrix dimensions overflow"));
    let byrow = if byrow_arg.is_null() || byrow_arg == R_NilValue() {
        false
    } else {
        TYPEOF(byrow_arg) == SEXPTYPE::LGLSXP && {
            let owner = active_owner();
            owner_sexp(owner, byrow_arg).try_logical_elt(0).unwrap_or(0) != 0
        }
    };

    let t = TYPEOF(data);
    if !supported_matrix_type(t) || nrow < 0 || ncol < 0 {
        return R_NilValue();
    }
    let result = Rf_allocVector3(t, length);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);

    // R stores matrices in column-major order. If data has length zero, keep
    // the requested shape and fill with the type-appropriate missing value.
    for i in 0..length {
        if data_len == 0 {
            set_matrix_na_or_zero(result, i);
        } else {
            let src_idx = if byrow && ncol > 0 {
                let row = i % nrow;
                let col = i / nrow;
                row * ncol + col
            } else {
                i
            } % data_len;
            copy_matrix_element(result, i, data, src_idx);
        }
    }

    // Set dim attribute
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !dim.is_null() {
        let owner = active_owner();
        if let Ok(mut dim_mut) = owner.sexp(dim).and_then(SexpMut::try_from_checked) {
            let _ = dim_mut.try_set_integer_elt(0, nrow as c_int);
            let _ = dim_mut.try_set_integer_elt(1, ncol as c_int);
        }
        crate::sexp::attrib_core::setAttrib(result, install_str("dim"), dim);
    }

    if !dimnames.is_null() && dimnames != R_NilValue() {
        super::attrs::set_array_dimnames(result, dimnames);
    }

    result
}

/// R's `array(data, dim, dimnames)` — create an array with recycled data.
pub fn do_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let data = arg_by_name_or_position(args, &["data"], 0);
        let dim_arg = arg_by_name_or_position(args, &["dim"], 1);
        let dimnames = arg_by_name_or_position(args, &["dimnames"], 2);

        let data_missing = data.is_null() || data == R_NilValue();
        if data_missing {
            let mut cell = args;
            while !cell.is_null() && cell != R_NilValue() {
                if tag_name(cell).is_none() || tag_name(cell).as_deref() == Some("data") {
                    crate::mainutils::errors::errorcall_str(
                        crate::mainutils::errors::condition_caller_call(),
                        "'data' must be of a vector type, was 'NULL'",
                    );
                }
                cell = CDR(cell);
            }
        }
        let data_type = if data_missing {
            SEXPTYPE::LGLSXP.as_c_int()
        } else {
            TYPEOF(data)
        };
        if !supported_matrix_type(data_type) {
            std::panic::panic_any(RError {
                message: "'data' must be a vector type".to_string(),
            });
        }

        let data_len = if data_missing { 1 } else { XLENGTH(data) };
        let dim = match array_dimension_attribute(dim_arg, data_len) {
            Ok(dim) => dim,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };
        let _dim_guard = protect(dim);
        // dim2total(dims, &err) with all checks (trunk array.c): errors on
        // length-0, NA, and negative dims; "too many elements specified"
        // once the product passes R_XLEN_T_MAX.
        let total_len = match crate::mainutils::array::dim2total(dim) {
            Ok(total) => total,
            Err(()) => std::panic::panic_any(RError {
                message: "too many elements specified".to_string(),
            }),
        };

        let result = Rf_allocVector3(data_type, total_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..total_len {
            if data_missing || data_len == 0 {
                set_matrix_na_or_zero(result, i);
            } else {
                copy_matrix_element(result, i, data, i % data_len);
            }
        }

        crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_DimSymbol(), dim);
        if !dimnames.is_null() && dimnames != R_NilValue() {
            super::attrs::set_array_dimnames(result, dimnames);
        }

        result
    }
}

/// Coerce the `dim=` argument of `array()` to an integer vector, as
/// upstream's `coerceVector(dims, INTSXP)`. The dim2total() checks
/// (length-0 / NA / negative / overflow) run in do_array on the coerced
/// vector, exactly as in trunk array.c.
pub fn array_dimension_attribute(value: SEXP, data_len: R_xlen_t) -> Result<SEXP, String> {
    if value.is_null() || value == R_NilValue() {
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
        if dim.is_null() {
            return Ok(R_NilValue());
        }
        let owner = active_owner();
        if let Ok(mut dim_mut) = owner.sexp(dim).and_then(SexpMut::try_from_checked) {
            let _ = dim_mut.try_set_integer_elt(0, data_len as c_int);
        }
        return Ok(dim);
    }

    if !is_atomic_vector_type(TYPEOF(value)) {
        return Err("'dim' must be a numeric vector".to_string());
    }

    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, XLENGTH(value));
    if dim.is_null() {
        return Ok(R_NilValue());
    }
    let _dim_guard = protect(dim);
    let owner = active_owner();
    let mut dim_mut = owner
        .sexp(dim)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for i in 0..XLENGTH(value) {
        let component = dimension_component(value, i);
        dim_mut
            .try_set_integer_elt(i, component)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    let names =
        crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_NamesSymbol());
    if !names.is_null()
        && names != R_NilValue()
        && TYPEOF(names) == SEXPTYPE::STRSXP
        && XLENGTH(names) == XLENGTH(dim)
    {
        crate::sexp::attrib_core::setAttrib(dim, crate::sexp::attrib_core::R_NamesSymbol(), names);
    }
    Ok(dim)
}

pub fn valid_array_dimnames(dimnames: SEXP, dim: SEXP) -> bool {
    if dimnames.is_null() || dimnames == R_NilValue() {
        return true;
    }
    if TYPEOF(dimnames) != SEXPTYPE::VECSXP {
        return false;
    }
    let dim_count = XLENGTH(dim);
    if XLENGTH(dimnames) > dim_count {
        return false;
    }
    let owner = active_owner();
    let dimnames = owner_sexp(owner, dimnames);
    let dim = owner_sexp(owner, dim);
    for i in 0..XLENGTH(dimnames.as_raw()) {
        let Ok(names) = dimnames.try_vector_elt(i) else {
            continue;
        };
        if names.as_raw().is_null() || names.as_raw() == R_NilValue() {
            continue;
        }
        let extent = match dim.try_integer_elt(i) {
            Ok(v) => v as R_xlen_t,
            Err(error) => crate::sexp::context::r_error(error.to_string()),
        };
        if XLENGTH(names.as_raw()) != extent {
            return false;
        }
    }
    true
}

pub fn retained_dimname(dimnames: SEXP, axis: R_xlen_t) -> SEXP {
    if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    if axis >= XLENGTH(dimnames) {
        return R_NilValue();
    }
    let owner = active_owner();
    let dimnames = owner_sexp(owner, dimnames);
    match dimnames.try_vector_elt(axis) {
        Ok(elt) => elt.as_raw(),
        Err(error) => crate::sexp::context::r_error(error.to_string()),
    }
}

pub fn retained_dimnames(dimnames: SEXP, axes: &[R_xlen_t]) -> SEXP {
    if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, axes.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    let owner = active_owner();
    let mut has_names = false;
    for (out_i, axis) in axes.iter().enumerate() {
        let names = if *axis < XLENGTH(dimnames) {
            let raw = owner
                .sexp(dimnames)
                .and_then(|d| d.try_vector_elt(*axis))
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            raw.as_raw()
        } else {
            R_NilValue()
        };
        if !names.is_null() && names != R_NilValue() {
            has_names = true;
        }
        let value = owner
            .sexp(names)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut result_mut = owner
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        result_mut
            .try_set_vector_elt(out_i as R_xlen_t, value)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    let src_names = crate::sexp::attrib_core::getAttrib(
        dimnames,
        crate::sexp::attrib_core::R_NamesSymbol(),
    );
    if !src_names.is_null()
        && src_names != R_NilValue()
        && TYPEOF(src_names) == SEXPTYPE::STRSXP
    {
        let kept = Rf_allocVector3(SEXPTYPE::STRSXP, axes.len() as R_xlen_t);
        if !kept.is_null() {
            let _kept_guard = protect(kept);
            let mut kept_mut = owner
                .sexp(kept)
                .and_then(SexpMut::try_from_checked)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            for (out_i, axis) in axes.iter().enumerate() {
                let raw = if *axis < XLENGTH(src_names) {
                    owner
                        .sexp(src_names)
                        .and_then(|v| v.try_string_elt(*axis))
                        .unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        })
                        .as_raw()
                } else {
                    R_NilValue()
                };
                let elt = if raw.is_null() || raw == R_NilValue() {
                    mk_char_cstr(c"")
                } else {
                    raw
                };
                let elt = owner
                    .sexp(elt)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                kept_mut
                    .try_set_string_elt(out_i as R_xlen_t, elt)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                kept,
            );
        }
    }
    if has_names { result } else { R_NilValue() }
}

pub fn dimension_component(value: SEXP, i: R_xlen_t) -> c_int {
    let kind = TYPEOF(value);
    if kind != SEXPTYPE::INTSXP.as_c_int()
        && kind != SEXPTYPE::LGLSXP.as_c_int()
        && kind != SEXPTYPE::REALSXP.as_c_int()
        && kind != SEXPTYPE::CPLXSXP.as_c_int()
        && kind != SEXPTYPE::STRSXP.as_c_int()
        && kind != SEXPTYPE::RAWSXP.as_c_int()
    {
        return NA_INTEGER;
    }
    let owner = active_owner();
    let value = owner_sexp(owner, value);
    if kind == SEXPTYPE::INTSXP.as_c_int() || kind == SEXPTYPE::LGLSXP.as_c_int() {
        integer_like_elt(&value, i)
    } else if kind == SEXPTYPE::REALSXP.as_c_int() {
        let v = value
            .try_real_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        real_to_dimension(v)
    } else if kind == SEXPTYPE::CPLXSXP.as_c_int() {
        let v = value
            .try_complex_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        real_to_dimension(v.r)
    } else if kind == SEXPTYPE::STRSXP.as_c_int() {
        let Ok(elt) = value.try_string_elt(i) else {
            return NA_INTEGER;
        };
        if elt.is_na_string() {
            NA_INTEGER
        } else {
            let Some(text) = charsxp_utf8_lossy(&elt) else {
                return NA_INTEGER;
            };
            text.trim()
                .parse::<f64>()
                .map(real_to_dimension)
                .unwrap_or(NA_INTEGER)
        }
    } else if kind == SEXPTYPE::RAWSXP.as_c_int() {
        value
            .try_raw_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            as c_int
    } else {
        NA_INTEGER
    }
}

pub fn real_to_dimension(value: f64) -> c_int {
    if !value.is_finite() || value < c_int::MIN as f64 || value > c_int::MAX as f64 {
        return NA_INTEGER;
    }
    value.trunc() as c_int
}

pub fn is_atomic_vector_type(kind: c_int) -> bool {
    kind == SEXPTYPE::LGLSXP.as_c_int()
        || kind == SEXPTYPE::INTSXP.as_c_int()
        || kind == SEXPTYPE::REALSXP.as_c_int()
        || kind == SEXPTYPE::CPLXSXP.as_c_int()
        || kind == SEXPTYPE::STRSXP.as_c_int()
        || kind == SEXPTYPE::RAWSXP.as_c_int()
}

/// R's `diag(x)` — extract diagonal or create diagonal matrix.
pub fn do_diag(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    // Check if x is a matrix
    let dim_attr = crate::sexp::attrib_core::getAttrib(x, install_str("dim"));
    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
        // Extract diagonal
        let owner = active_owner();
        let dim_attr_owned = owner_sexp(owner, dim_attr);
        let nrow = dim_attr_owned
            .try_integer_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            as usize;
        let ncol = dim_attr_owned
            .try_integer_elt(1)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            as usize;
        let n = nrow.min(ncol);
        if !supported_matrix_type(TYPEOF(x)) {
            return R_NilValue();
        }
        let result = Rf_allocVector3(TYPEOF(x), n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let src = i + i * nrow;
            copy_matrix_element(result, i as R_xlen_t, x, src as R_xlen_t);
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        if !dimnames.is_null()
            && dimnames != R_NilValue()
            && TYPEOF(dimnames) == SEXPTYPE::VECSXP
            && XLENGTH(dimnames) >= 2
        {
            let dimnames_owned = owner_sexp(owner, dimnames);
            let rn = dimnames_owned
                .try_vector_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                .as_raw();
            let cn = dimnames_owned
                .try_vector_elt(1)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                .as_raw();
            let same = !rn.is_null()
                && rn != R_NilValue()
                && !cn.is_null()
                && cn != R_NilValue()
                && TYPEOF(rn) == SEXPTYPE::STRSXP
                && TYPEOF(cn) == SEXPTYPE::STRSXP
                && XLENGTH(rn) >= n as i64
                && XLENGTH(cn) >= n as i64
                && (0..n).all(|i| {
                    let rn_owned = owner_sexp(owner, rn);
                    let cn_owned = owner_sexp(owner, cn);
                    let a = rn_owned
                        .try_string_elt(i as R_xlen_t)
                        .unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        });
                    let b = cn_owned
                        .try_string_elt(i as R_xlen_t)
                        .unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        });
                    !a.as_raw().is_null()
                        && !b.as_raw().is_null()
                        && (a.as_raw() == b.as_raw()
                            || match (charsxp_utf8_lossy(&a), charsxp_utf8_lossy(&b)) {
                                (Some(ta), Some(tb)) => ta == tb,
                                _ => false,
                            })
                });
            if same {
                let names = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
                let _names_guard = protect(names);
                let owner = active_owner();
                let rn_owned = owner_sexp(owner, rn);
                if let Ok(mut names_mut) = owner.sexp(names).and_then(SexpMut::try_from_checked)
                {
                    for i in 0..n {
                        if let Ok(elt) = rn_owned.try_string_elt(i as R_xlen_t) {
                            let _ = names_mut.try_set_string_elt(i as R_xlen_t, elt);
                        }
                    }
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
        }
        result
    } else {
        let nrow_arg = CAR(CDR(args));
        let nrow_given = !nrow_arg.is_null()
            && nrow_arg != R_NilValue()
            && nrow_arg != crate::sexp::globals::R_MissingArg();
        if XLENGTH(x) == 1 && !nrow_given {
            let n = real_or_default(x, 0.0).max(0.0) as usize;
            let t = TYPEOF(x);
            if !supported_matrix_type(t) {
                return R_NilValue();
            }
            let result = Rf_allocVector3(t, (n * n) as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for i in 0..n * n {
                set_matrix_zero(result, i as R_xlen_t);
            }
            for i in 0..n {
                let dst = i + i * n;
                set_diagonal_identity_value(result, dst as R_xlen_t);
            }
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if !dim.is_null() {
                let owner = active_owner();
                if let Ok(mut dim_mut) = owner.sexp(dim).and_then(SexpMut::try_from_checked) {
                    let _ = dim_mut.try_set_integer_elt(0, n as c_int);
                    let _ = dim_mut.try_set_integer_elt(1, n as c_int);
                }
                crate::sexp::attrib_core::setAttrib(result, install_str("dim"), dim);
            }
            return result;
        }

        let n = XLENGTH(x) as usize;
        let t = TYPEOF(x);
        if !supported_matrix_type(t) {
            return R_NilValue();
        }
        let ncol_arg = CAR(CDR(CDR(args)));
        let ncol_given = !ncol_arg.is_null()
            && ncol_arg != R_NilValue()
            && ncol_arg != crate::sexp::globals::R_MissingArg();
        let nrow = if nrow_given {
            real_or_default(nrow_arg, 0.0).max(0.0) as usize
        } else {
            n
        };
        let ncol = if ncol_given {
            real_or_default(ncol_arg, 0.0).max(0.0) as usize
        } else {
            nrow
        };
        let ndiag = nrow.min(ncol);
        let cells = nrow.saturating_mul(ncol);
        let result = Rf_allocVector3(t, cells as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..cells {
            set_matrix_zero(result, i as R_xlen_t);
        }
        if n > 0 {
            for i in 0..ndiag {
                let dst = i + i * nrow;
                copy_matrix_element(result, dst as R_xlen_t, x, (i % n) as R_xlen_t);
            }
        }
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            let owner = active_owner();
            if let Ok(mut dim_mut) = owner.sexp(dim).and_then(SexpMut::try_from_checked) {
                let _ = dim_mut.try_set_integer_elt(0, nrow as c_int);
                let _ = dim_mut.try_set_integer_elt(1, ncol as c_int);
            }
            crate::sexp::attrib_core::setAttrib(result, install_str("dim"), dim);
        }
        result
    }
}
// ---------------------------------------------------------------------------
// Complete S3 generics — as.matrix, as.numeric
// ---------------------------------------------------------------------------

/// R's `as.matrix(x)` — convert to matrix (simplified).
/// For vectors, wraps as a single-column matrix.
/// For lists/data.frames, wraps as a matrix.
pub fn do_as_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if !class.is_null() && class != R_NilValue() && TYPEOF(class) == SEXPTYPE::STRSXP {
            let owner = active_owner();
            let class_owned = owner_sexp(owner, class);
            for i in 0..XLENGTH(class) {
                let is_dist = class_owned
                    .try_string_elt(i)
                    .map(|elt| elt.try_char_cstr_eq(b"dist").unwrap_or(false))
                    .unwrap_or(false);
                if !is_dist {
                    continue;
                }
                let size = crate::sexp::attrib_core::getAttrib(
                    x,
                    install_str("Size"),
                );
                let n = if TYPEOF(size) == SEXPTYPE::INTSXP && XLENGTH(size) > 0 {
                    owner_sexp(owner, size)
                        .try_integer_elt(0)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                        as usize
                } else {
                    0
                };
                if n < 1 {
                    let mat =
                        alloc_matrix(SEXPTYPE::REALSXP.as_c_int(), 0, 0);
                    let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
                    if let Ok(mut dims_mut) = owner.sexp(dims).and_then(SexpMut::try_from_checked) {
                        let _ = dims_mut.try_set_integer_elt(0, 0);
                        let _ = dims_mut.try_set_integer_elt(1, 0);
                    }
                    crate::sexp::attrib_core::setAttrib(
                        mat,
                        crate::sexp::attrib_core::R_DimSymbol(),
                        dims,
                    );
                    return mat;
                }
                let mat = alloc_matrix(
                    SEXPTYPE::REALSXP.as_c_int(),
                    n as i32,
                    n as i32,
                );
                let _m = protect(mat);
                let mut mat_mut = owner
                    .sexp(mat)
                    .and_then(SexpMut::try_from_checked)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                for i in 0..(n * n) {
                    mat_mut
                        .try_set_real_elt(i as R_xlen_t, 0.0)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                }
                let x_owned = owner_sexp(owner, x);
                let mut k = 0usize;
                for i in 0..n {
                    for j in (i + 1)..n {
                        let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                            x_owned.try_real_elt(k as R_xlen_t).unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            })
                        } else {
                            0.0
                        };
                        mat_mut
                            .try_set_real_elt((j + i * n) as R_xlen_t, v)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                        mat_mut
                            .try_set_real_elt((i + j * n) as R_xlen_t, v)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                        k += 1;
                    }
                }
                return mat;
            }
        }
        let existing_dim =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if !existing_dim.is_null()
            && existing_dim != R_NilValue()
            && TYPEOF(existing_dim) == SEXPTYPE::INTSXP
            && XLENGTH(existing_dim) == 2
        {
            return x;
        }
        if is_data_frame_object(x) {
            // GNU's as.matrix.data.frame closure (exact base dataframe.R
            // semantics: automatic row names collapse to NULL, matrix and
            // data-frame columns expand, classed columns format).
            let method = find_var_in_frame(
                crate::sexp::globals::R_BaseEnv(),
                install_str("as.matrix.data.frame"),
            );
            let quoted = crate::sexp::constructors::Rf_lang2(install_str("quote"), x);
            let _quoted = protect(quoted);
            let call = if !CDR(args).is_null() && CDR(args) != R_NilValue() {
                let forced = crate::sexp::constructors::Rf_lang3(method, quoted, CAR(CDR(args)));
                forced
            } else {
                crate::sexp::constructors::Rf_lang2(method, quoted)
            };
            let _call = protect(call);
            return crate::eval::eval::Rf_eval(call, _rho);
        }
        // Run the original default workflow so all vector kinds, names and
        // higher-dimensional arrays share array()'s admission and allocation.
        let default = find_var_in_frame(
            crate::sexp::globals::R_BaseEnv(),
            install_str("as.matrix.default"),
        );
        let quoted = crate::sexp::constructors::Rf_lang2(install_str("quote"), x);
        let _quoted = protect(quoted);
        let call = crate::sexp::constructors::Rf_lang2(default, quoted);
        let _call = protect(call);
        crate::eval::eval::Rf_eval(call, _rho)
    }
}

/// GNU `isSymmetric(object)` — square matrix equals its transpose.
pub fn do_is_symmetric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() || TYPEOF(dim) != SEXPTYPE::INTSXP || XLENGTH(dim) < 2 {
        return Rf_ScalarLogical(FALSE);
    }
    let owner = active_owner();
    let dim_owned = owner_sexp(owner, dim);
    let n = dim_owned
        .try_integer_elt(0)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        as usize;
    let nc = dim_owned
        .try_integer_elt(1)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        as usize;
    if n != nc {
        return Rf_ScalarLogical(FALSE);
    }
    let x_owned = owner_sexp(owner, x);
    for i in 0..n {
        for j in (i + 1)..n {
            let a = if TYPEOF(x) == SEXPTYPE::REALSXP {
                x_owned
                    .try_real_elt((i + j * n) as R_xlen_t)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                x_owned
                    .try_integer_elt((i + j * n) as R_xlen_t)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    as f64
            } else {
                return Rf_ScalarLogical(FALSE);
            };
            let b = if TYPEOF(x) == SEXPTYPE::REALSXP {
                x_owned
                    .try_real_elt((j + i * n) as R_xlen_t)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            } else {
                x_owned
                    .try_integer_elt((j + i * n) as R_xlen_t)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    as f64
            };
            if (a - b).abs() > 1e-10 {
                return Rf_ScalarLogical(FALSE);
            }
        }
    }
    Rf_ScalarLogical(TRUE)
}

fn data_matrix_row_names(frame: SEXP, rownames_force: SEXP) -> SEXP {
    let force = if rownames_force.is_null() || rownames_force == R_NilValue() {
        NA_INTEGER
    } else if TYPEOF(rownames_force) == SEXPTYPE::LGLSXP
        || TYPEOF(rownames_force) == SEXPTYPE::INTSXP
    {
        let owner = active_owner();
        integer_like_elt(&owner_sexp(owner, rownames_force), 0)
    } else {
        NA_INTEGER
    };
    if force == FALSE {
        return R_NilValue();
    }

    // getAttrib expands compact row names and loses their automatic flag.
    // Inspect the stored attribute before choosing matrix row labels.
    let row_names_symbol = crate::sexp::attrib_core::R_RowNamesSymbol();
    let mut row_names = R_NilValue();
    let mut attribute = ATTRIB(frame);
    while !attribute.is_null() && attribute != R_NilValue() {
        if TAG(attribute) == row_names_symbol {
            row_names = CAR(attribute);
            break;
        }
        attribute = CDR(attribute);
    }
    let automatic = !row_names.is_null()
        && TYPEOF(row_names) == SEXPTYPE::INTSXP
        && XLENGTH(row_names) == 2
        && {
            let owner = active_owner();
            let row_names = owner_sexp(owner, row_names);
            let first = row_names
                .try_integer_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let second = row_names
                .try_integer_elt(1)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            first == NA_INTEGER && second < 0
        };
    if force == NA_INTEGER && automatic {
        return R_NilValue();
    }
    string_vector(&data_frame_row_names(frame))
}

fn character_column_codes(column: SEXP, result: SEXP, offset: R_xlen_t) {
    let owner = active_owner();
    let column = owner_sexp(owner, column);
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut codes: std::collections::BTreeMap<Option<String>, c_int> =
        std::collections::BTreeMap::new();
    let mut next_code = 1;
    for row in 0..XLENGTH(column.as_raw()) {
        let value = column
            .try_string_elt(row)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        // match(x, unique(x)), used by base data.matrix(), assigns an
        // ordinary code to NA_character_ because missing values match
        // one another. Keep that sentinel distinct from real strings.
        let key =
            if value.as_raw().is_null() || value.as_raw() == crate::sexp::globals::R_NaString() {
                None
            } else {
                charsxp_utf8_lossy(&value)
            };
        if key.is_none() {
            // NA stays NA in the numeric result — stock data.matrix
            // does not assign a level code to missing strings.
            if TYPEOF(result) == SEXPTYPE::INTSXP {
                result_mut
                    .try_set_integer_elt(offset + row, crate::sexp::ffi::NA_INTEGER)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            } else {
                result_mut
                    .try_set_real_elt(offset + row, crate::sexp::ffi::NA_REAL)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            continue;
        }
        let code = *codes.entry(key).or_insert_with(|| {
            let code = next_code;
            next_code += 1;
            code
        });
        if TYPEOF(result) == SEXPTYPE::INTSXP {
            result_mut
                .try_set_integer_elt(offset + row, code)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        } else {
            result_mut
                .try_set_real_elt(offset + row, code as f64)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
}

/// GNU `as.matrix.data.frame`: character columns stay character, factors
/// become their level labels, and a mixed frame becomes character.
pub fn data_frame_as_matrix(frame: SEXP) -> SEXP {
    let mut leaves: Vec<SEXP> = Vec::new();
    // Retain allocated matrix slices until they are installed into
    // `flat`; Rust's Vec<SEXP> is invisible to R's GC.
    let mut leaf_guards = Vec::new();
    // GNU as.matrix.data.frame in a single-byte locale treats names as
    // opaque bytes; keep them as raw bytes so native (non-UTF-8) column
    // names survive the flatten/relabel round-trip.
    let mut leaf_names: Vec<Vec<u8>> = Vec::new();
    let frame_names =
        crate::sexp::attrib_core::getAttrib(frame, crate::sexp::attrib_core::R_NamesSymbol());
    let mut changed = false;
    let owner = active_owner();
    let frame_owned = owner_sexp(owner, frame);
    let frame_names_owned = if !frame_names.is_null()
        && frame_names != R_NilValue()
        && TYPEOF(frame_names) == SEXPTYPE::STRSXP
    {
        Some(owner_sexp(owner, frame_names))
    } else {
        None
    };
    for j in 0..XLENGTH(frame) {
        let column = frame_owned
            .try_vector_elt(j)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            .as_raw();
        let outer = if let Some(names) = &frame_names_owned
            && j < XLENGTH(frame_names)
        {
            let ch = names
                .try_string_elt(j)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if ch.as_raw().is_null() || ch.as_raw() == crate::sexp::globals::R_NaString() {
                Vec::new()
            } else {
                ch.header().char_bytes().unwrap_or_default()
            }
        } else {
            Vec::new()
        };
        let dim = crate::sexp::attrib_core::getAttrib(
            column,
            crate::sexp::attrib_core::R_DimSymbol(),
        );
        let dim_owned = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 2
        {
            Some(owner_sexp(owner, dim))
        } else {
            None
        };
        let matrix_cols = dim_owned.as_ref().map(|d| {
            d.try_integer_elt(1)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        });
        if let Some(mc) = matrix_cols {
            changed = true;
            if mc == 0 {
                continue;
            }
            let mr = dim_owned
                .as_ref()
                .unwrap()
                .try_integer_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                as i64;
            let dn = crate::sexp::attrib_core::getAttrib(
                column,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
            );
            let cnames = if !dn.is_null()
                && dn != R_NilValue()
                && TYPEOF(dn) == SEXPTYPE::VECSXP
                && XLENGTH(dn) >= 2
            {
                owner_sexp(owner, dn)
                    .try_vector_elt(1)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    .as_raw()
            } else {
                R_NilValue()
            };
            let cnames_owned = if TYPEOF(cnames) == SEXPTYPE::STRSXP {
                Some(owner_sexp(owner, cnames))
            } else {
                None
            };
            for k in 0..mc as i64 {
                let piece = Rf_allocVector3(TYPEOF(column), mr);
                leaf_guards.push(protect(piece));
                let column_owned = owner_sexp(owner, column);
                let mut piece_mut = owner
                    .sexp(piece)
                    .and_then(SexpMut::try_from_checked)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                for r in 0..mr {
                    let src = (k * mr + r) as R_xlen_t;
                    if TYPEOF(column) == SEXPTYPE::REALSXP {
                        let v = column_owned.try_real_elt(src).unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        });
                        piece_mut.try_set_real_elt(r, v).unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        });
                    } else if TYPEOF(column) == SEXPTYPE::INTSXP
                        || TYPEOF(column) == SEXPTYPE::LGLSXP
                    {
                        let v = integer_like_elt(&column_owned, src);
                        let written = if TYPEOF(column) == SEXPTYPE::INTSXP {
                            piece_mut.try_set_integer_elt(r, v)
                        } else {
                            piece_mut.try_set_logical_elt(r, v)
                        };
                        written.unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        });
                    }
                }
                leaves.push(piece);
                let inner = if let Some(cn) = &cnames_owned
                    && k < XLENGTH(cnames)
                {
                    cn.try_string_elt(k)
                        .map(|ch| ch.header().char_bytes().unwrap_or_default())
                        .unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        })
                } else {
                    format!("{}", k + 1).into_bytes()
                };
                let labeled = TYPEOF(cnames) == SEXPTYPE::STRSXP;
                leaf_names.push(if mc == 1 {
                    if outer.is_empty() {
                        inner
                    } else {
                        outer.clone()
                    }
                } else if outer.is_empty() {
                    inner
                } else {
                    [outer.as_slice(), b".", inner.as_slice()].concat()
                });
            }
            continue;
        }
        if crate::mainutils::objects::inherits2(column, c"data.frame") != 0 {
            changed = true;
            let inner_names = crate::sexp::attrib_core::getAttrib(
                column,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            let inner_names_owned =
                if !inner_names.is_null() && TYPEOF(inner_names) == SEXPTYPE::STRSXP {
                    Some(owner_sexp(owner, inner_names))
                } else {
                    None
                };
            let column_owned = owner_sexp(owner, column);
            for k in 0..XLENGTH(column) {
                let leaf = column_owned
                    .try_vector_elt(k)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    .as_raw();
                leaves.push(leaf);
                let inner = if let Some(names) = &inner_names_owned
                    && k < XLENGTH(inner_names)
                {
                    names
                        .try_string_elt(k)
                        .ok()
                        .and_then(|ch| ch.header().char_bytes())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                leaf_names.push(if inner.is_empty() {
                    outer.clone()
                } else if outer.is_empty() {
                    inner
                } else {
                    [outer.as_slice(), b".", inner.as_slice()].concat()
                });
            }
        } else {
            leaves.push(column);
            leaf_names.push(outer);
        }
    }
    if changed {
        let flat = Rf_allocVector3(SEXPTYPE::VECSXP, leaves.len() as i64);
        let _flat = protect(flat);
        let nm = Rf_allocVector3(SEXPTYPE::STRSXP, leaves.len() as i64);
        let _nm = protect(nm);
        let mut flat_mut = owner
            .sexp(flat)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut nm_mut = owner
            .sexp(nm)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        for (i, (col, name)) in leaves.iter().zip(leaf_names.iter()).enumerate() {
            let value = owner
                .sexp(*col)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            flat_mut
                .try_set_vector_elt(i as i64, value)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let cs = std::ffi::CString::new(name.as_slice()).unwrap_or_default();
            let charsxp = mk_char_cstr(&cs);
            let charsxp = owner
                .sexp(charsxp)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            nm_mut
                .try_set_string_elt(i as i64, charsxp)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        crate::sexp::attrib_core::setAttrib(
            flat,
            crate::sexp::attrib_core::R_NamesSymbol(),
            nm,
        );
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _class = protect(class);
        let class_charsxp = mk_char_cstr(c"data.frame");
        if let (Ok(charsxp), Ok(mut class_mut)) = (
            owner.sexp(class_charsxp),
            owner.sexp(class).and_then(SexpMut::try_from_checked),
        ) {
            let _ = class_mut.try_set_string_elt(0, charsxp);
        }
        crate::sexp::attrib_core::setAttrib(
            flat,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        let rn = crate::sexp::attrib_core::getAttrib(
            frame,
            crate::sexp::attrib_core::R_RowNamesSymbol(),
        );
        crate::sexp::attrib_core::setAttrib(
            flat,
            crate::sexp::attrib_core::R_RowNamesSymbol(),
            rn,
        );
        return data_frame_as_matrix(flat);
    }
    let nrow = data_frame_row_count(frame);
    let ncol = XLENGTH(frame);
    let mut any_char = false;
    let mut any_real = false;
    let mut any_factor = false;
    let mut all_logical = ncol > 0;
    for j in 0..ncol {
        let column = frame_owned
            .try_vector_elt(j)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            .as_raw();
        let factor = crate::mainutils::objects::inherits2(column, c"factor") != 0;
        if factor {
            any_factor = true;
        }
        match TYPEOF(column) {
            t if t == SEXPTYPE::STRSXP && !factor => {
                any_char = true;
                all_logical = false;
            }
            t if t == SEXPTYPE::REALSXP => {
                any_real = true;
                all_logical = false;
            }
            t if t == SEXPTYPE::LGLSXP => {}
            _ => all_logical = false,
        }
        if factor {
            all_logical = false;
        }
    }
    let result_type = if any_char || any_factor {
        SEXPTYPE::STRSXP
    } else if all_logical || ncol == 0 {
        SEXPTYPE::LGLSXP
    } else if any_real {
        SEXPTYPE::REALSXP
    } else {
        SEXPTYPE::INTSXP
    };
    let result = Rf_allocVector3(result_type, nrow.saturating_mul(ncol));
    let _g = protect(result);
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for j in 0..ncol {
        let column = frame_owned
            .try_vector_elt(j)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            .as_raw();
        let offset = j * nrow;
        let factor = crate::mainutils::objects::inherits2(column, c"factor") != 0;
        let column_owned = owner_sexp(owner, column);
        for row in 0..nrow {
            let src = if XLENGTH(column) == 0 {
                0
            } else {
                row % XLENGTH(column)
            };
            let at = offset + row;
            if result_type == SEXPTYPE::STRSXP {
                let text = if XLENGTH(column) == 0 {
                    crate::sexp::globals::R_NaString()
                } else if TYPEOF(column) == SEXPTYPE::STRSXP && !factor {
                    column_owned
                        .try_string_elt(src)
                        .unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        })
                        .as_raw()
                } else if factor {
                    let v = column_owned.try_integer_elt(src).unwrap_or_else(|error| {
                        crate::sexp::context::r_error(error.to_string())
                    });
                    let levels = crate::sexp::attrib_core::getAttrib(
                        column,
                        crate::sexp::attrib_core::R_LevelsSymbol(),
                    );
                    if v == NA_INTEGER
                        || levels.is_null()
                        || TYPEOF(levels) != SEXPTYPE::STRSXP
                        || v < 1
                        || v as i64 > XLENGTH(levels)
                    {
                        crate::sexp::globals::R_NaString()
                    } else {
                        owner_sexp(owner, levels)
                            .try_string_elt((v - 1) as i64)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            })
                            .as_raw()
                    }
                } else if TYPEOF(column) == SEXPTYPE::LGLSXP {
                    let v = integer_like_elt(&column_owned, src);
                    if v == NA_INTEGER {
                        crate::sexp::globals::R_NaString()
                    } else if v == 0 {
                        mk_char_cstr(c"FALSE")
                    } else {
                        mk_char_cstr(c"TRUE")
                    }
                } else if TYPEOF(column) == SEXPTYPE::INTSXP {
                    let v = column_owned.try_integer_elt(src).unwrap_or_else(|error| {
                        crate::sexp::context::r_error(error.to_string())
                    });
                    if v == NA_INTEGER {
                        crate::sexp::globals::R_NaString()
                    } else {
                        let s = std::ffi::CString::new(v.to_string()).unwrap_or_default();
                        mk_char_cstr(&s)
                    }
                } else {
                    let v = column_owned.try_real_elt(src).unwrap_or_else(|error| {
                        crate::sexp::context::r_error(error.to_string())
                    });
                    if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        crate::sexp::globals::R_NaString()
                    } else {
                        let s = std::ffi::CString::new(v.to_string()).unwrap_or_default();
                        mk_char_cstr(&s)
                    }
                };
                let text = owner
                    .sexp(text)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                result_mut
                    .try_set_string_elt(at, text)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            } else if result_type == SEXPTYPE::INTSXP || result_type == SEXPTYPE::LGLSXP {
                let v = if XLENGTH(column) == 0 {
                    NA_INTEGER
                } else {
                    integer_like_elt(&column_owned, src)
                };
                let written = if result_type == SEXPTYPE::INTSXP {
                    result_mut.try_set_integer_elt(at, v)
                } else {
                    result_mut.try_set_logical_elt(at, v)
                };
                written
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            } else {
                let v = if XLENGTH(column) == 0 {
                    NA_REAL
                } else if TYPEOF(column) == SEXPTYPE::REALSXP {
                    column_owned.try_real_elt(src).unwrap_or_else(|error| {
                        crate::sexp::context::r_error(error.to_string())
                    })
                } else {
                    let v = integer_like_elt(&column_owned, src);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                };
                result_mut
                    .try_set_real_elt(at, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
        }
    }
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    let _d = protect(dim);
    if let Ok(mut dim_mut) = owner.sexp(dim).and_then(SexpMut::try_from_checked) {
        let _ = dim_mut.try_set_integer_elt(0, nrow as c_int);
        let _ = dim_mut.try_set_integer_elt(1, ncol as c_int);
    }
    crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_DimSymbol(), dim);
    let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    let _n = protect(dimnames);
    let row_names = data_matrix_row_names(frame, R_NilValue());
    let _row_names = protect(row_names);
    let mut dimnames_mut = owner
        .sexp(dimnames)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let row_names_owned = owner
        .sexp(row_names)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    dimnames_mut
        .try_set_vector_elt(0, row_names_owned)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let col_names =
        crate::sexp::attrib_core::getAttrib(frame, crate::sexp::attrib_core::R_NamesSymbol());
    let col_names_owned = owner
        .sexp(col_names)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    dimnames_mut
        .try_set_vector_elt(1, col_names_owned)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_DimNamesSymbol(),
        dimnames,
    );
    result
}

/// R's `data.matrix(frame, rownames.force = NA)`.
/// first-occurrence integer codes, and logical/real columns select double
/// storage just as base R does after its per-column conversion pass.
pub fn do_data_matrix(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    if args.is_null() || args == R_NilValue() {
        base_error("argument 'frame' is missing, with no default");
    }
    let frame = CAR(args);
    if frame.is_null() || frame == R_NilValue() {
        return R_NilValue();
    }
    if !is_data_frame_object(frame) {
        let dim =
            crate::sexp::attrib_core::getAttrib(frame, crate::sexp::attrib_core::R_DimSymbol());
        if !dim.is_null() && dim != R_NilValue() && XLENGTH(dim) == 2 {
            return frame;
        }
        return do_as_matrix(_call, _op, args, rho);
    }

    let nrow = data_frame_row_count(frame);
    let ncol = XLENGTH(frame);
    let owner = active_owner();
    let frame_owned = owner_sexp(owner, frame);
    let integer_result = ncol > 0
        && (0..ncol).all(|column_index| {
            let column = frame_owned
                .try_vector_elt(column_index)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                .as_raw();
            TYPEOF(column) == SEXPTYPE::INTSXP || TYPEOF(column) == SEXPTYPE::STRSXP
        });
    let result_type = if ncol == 0 {
        SEXPTYPE::LGLSXP
    } else if integer_result {
        SEXPTYPE::INTSXP
    } else {
        SEXPTYPE::REALSXP
    };
    let result = Rf_allocVector3(result_type, nrow.saturating_mul(ncol));
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));

    for column_index in 0..ncol {
        let column = frame_owned
            .try_vector_elt(column_index)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            .as_raw();
        let offset = column_index * nrow;
        if TYPEOF(column) == SEXPTYPE::STRSXP {
            character_column_codes(column, result, offset);
            continue;
        }
        let column_owned = owner_sexp(owner, column);
        for row in 0..nrow {
            let source_row = if XLENGTH(column) == 0 {
                0
            } else {
                row % XLENGTH(column)
            };
            if result_type == SEXPTYPE::INTSXP {
                let v = if XLENGTH(column) == 0 {
                    NA_INTEGER
                } else {
                    integer_like_elt(&column_owned, source_row)
                };
                result_mut
                    .try_set_integer_elt(offset + row, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            } else {
                let v = if XLENGTH(column) == 0 {
                    NA_REAL
                } else {
                    match TYPEOF(column) {
                        t if t == SEXPTYPE::REALSXP => column_owned
                            .try_real_elt(source_row)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            }),
                        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                            let value = integer_like_elt(&column_owned, source_row);
                            if value == NA_INTEGER {
                                NA_REAL
                            } else {
                                value as f64
                            }
                        }
                        _ => NA_REAL,
                    }
                };
                result_mut
                    .try_set_real_elt(offset + row, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
        }
    }

    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    let _dim_guard = protect(dim);
    if let Ok(mut dim_mut) = owner.sexp(dim).and_then(SexpMut::try_from_checked) {
        let _ = dim_mut.try_set_integer_elt(0, nrow as c_int);
        let _ = dim_mut.try_set_integer_elt(1, ncol as c_int);
    }
    crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_DimSymbol(), dim);

    let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    let _dimnames_guard = protect(dimnames);
    let rownames_force = arg_by_name_or_position(args, &["rownames.force"], 1);
    let row_names = data_matrix_row_names(frame, rownames_force);
    let _row_names_guard = protect(row_names);
    let mut dimnames_mut = owner
        .sexp(dimnames)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let row_names_owned = owner
        .sexp(row_names)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    dimnames_mut
        .try_set_vector_elt(0, row_names_owned)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let col_names =
        crate::sexp::attrib_core::getAttrib(frame, crate::sexp::attrib_core::R_NamesSymbol());
    let col_names_owned = owner
        .sexp(col_names)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    dimnames_mut
        .try_set_vector_elt(1, col_names_owned)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_DimNamesSymbol(),
        dimnames,
    );
    result
}

/// R's `as.numeric(x)` — GNU alias of `as.double`. S3 dispatch uses
/// PRIMNAME(op) (`as.numeric.*`); S4 methods on `as.numeric` go through
/// `R_possible_dispatch(op)` inside DispatchOrEval.
pub fn do_as_numeric(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    do_as_double(call, op, args, rho)
}

/// GNU `embed(x, dimension)`.
pub fn do_embed(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let d_arg = CAR(CDR(args));
    let owner = active_owner();
    let d_owned = owner_sexp(owner, d_arg);
    let dimension = if TYPEOF(d_arg) == SEXPTYPE::INTSXP {
        d_owned
            .try_integer_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            as isize
    } else {
        d_owned
            .try_real_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            as isize
    };
    let n = XLENGTH(x) as isize;
    if dimension < 1 || dimension > n {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "wrong embedding dimension",
        );
    }
    let nrow = n - dimension + 1;
    let ans = alloc_matrix(TYPEOF(x), nrow as i32, dimension as i32);
    let _a = protect(ans);
    let x_owned = owner_sexp(owner, x);
    let mut ans_mut = owner
        .sexp(ans)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for col in 0..dimension {
        let src0 = dimension - 1 - col;
        for row in 0..nrow {
            let src = (src0 + row) as R_xlen_t;
            let dst = (row + col * nrow) as R_xlen_t;
            if TYPEOF(x) == SEXPTYPE::INTSXP {
                let v = x_owned
                    .try_integer_elt(src)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                ans_mut
                    .try_set_integer_elt(dst, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            } else {
                let v = x_owned
                    .try_real_elt(src)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                ans_mut
                    .try_set_real_elt(dst, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
        }
    }
    ans
}

/// GNU `toeplitz(x, r = NULL, symmetric = is.null(r))`.
pub fn do_toeplitz(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m =
            crate::mainutils::match_mod::match_formal_slots(call, args, &["x", "r", "symmetric"]);
        let x = m[0];
        if x.is_null() || x == R_NilValue() {
            crate::mainutils::errors::errorcall_str(call, "'x' is not a vector");
        }
        let n = XLENGTH(x) as isize;
        let r = m[1];
        let r_absent =
            r.is_null() || r == R_NilValue() || r == crate::sexp::globals::R_MissingArg();
        let symmetric = if m[2].is_null()
            || m[2] == R_NilValue()
            || m[2] == crate::sexp::globals::R_MissingArg()
        {
            r_absent
        } else {
            real_or_default(m[2], 0.0) != 0.0
        };
        if n < 1 && symmetric {
            crate::mainutils::errors::errorcall_str(call, "'x' is not a vector of adequate length");
        }
        if symmetric {
            let ans = alloc_matrix(TYPEOF(x), n as i32, n as i32);
            let _a = protect(ans);
            for col in 0..n {
                for row in 0..n {
                    let k = (row - col).unsigned_abs() as isize;
                    copy_atomic_elt(x, k, ans, row + col * n);
                }
            }
            return ans;
        }
        if r_absent {
            crate::mainutils::errors::errorcall_str(call, "'r' is not a vector");
        }
        let nc = XLENGTH(r) as isize;
        if n > 0 && nc > 0 && !atomic_elt_equal(x, 0, r, 0) {
            let msg =
                std::ffi::CString::new("x[1] != r[1]; using x[1] for diagonal").unwrap_or_default();
            crate::mainutils::errors::warningcall(call, msg.as_ptr());
        }
        let ans = alloc_matrix(TYPEOF(x), n as i32, nc as i32);
        let _a = protect(ans);
        for col in 0..nc {
            for row in 0..n {
                let dst = row + col * n;
                if row >= col {
                    copy_atomic_elt(x, row - col, ans, dst);
                } else {
                    copy_atomic_elt(r, col - row, ans, dst);
                }
            }
        }
        ans
    }
}

fn copy_atomic_elt(from: SEXP, fi: isize, to: SEXP, ti: isize) {
    let owner = active_owner();
    let from = owner_sexp(owner, from);
    let mut to = owner
        .sexp(to)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    match from.typeof_() {
        SEXPTYPE::INTSXP => {
            let v = from
                .try_integer_elt(fi as R_xlen_t)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            to.try_set_integer_elt(ti as R_xlen_t, v)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        SEXPTYPE::LGLSXP => {
            let v = from
                .try_logical_elt(fi as R_xlen_t)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            to.try_set_logical_elt(ti as R_xlen_t, v)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        SEXPTYPE::STRSXP => {
            let v = from
                .try_string_elt(fi as R_xlen_t)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            to.try_set_string_elt(ti as R_xlen_t, v)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        _ => {
            // GNU's fallthrough arm reads the pair as REALSXP.
            let v = from
                .try_real_elt(fi as R_xlen_t)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            to.try_set_real_elt(ti as R_xlen_t, v)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
}

fn atomic_elt_equal(a: SEXP, ai: isize, b: SEXP, bi: isize) -> bool {
    if TYPEOF(a) == SEXPTYPE::REALSXP && TYPEOF(b) == SEXPTYPE::REALSXP {
        let owner = active_owner();
        let a = owner_sexp(owner, a);
        let b = owner_sexp(owner, b);
        let av = a
            .try_real_elt(ai as R_xlen_t)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let bv = b
            .try_real_elt(bi as R_xlen_t)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        av == bv
    } else if TYPEOF(a) == SEXPTYPE::INTSXP && TYPEOF(b) == SEXPTYPE::INTSXP {
        let owner = active_owner();
        let a = owner_sexp(owner, a);
        let b = owner_sexp(owner, b);
        let av = a
            .try_integer_elt(ai as R_xlen_t)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let bv = b
            .try_integer_elt(bi as R_xlen_t)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        av == bv
    } else {
        true
    }
}

/// GNU `toeplitz2(x, nrow)` — banded Toeplitz, ncol = length(x)+1-nrow.
pub fn do_toeplitz2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let nx = XLENGTH(x) as isize;
    let nrow_cell = CDR(args);
    let nrow = if nrow_cell.is_null()
        || nrow_cell == R_NilValue()
        || CAR(nrow_cell) == crate::sexp::globals::R_MissingArg()
    {
        nx
    } else {
        let nr = CAR(nrow_cell);
        let owner = active_owner();
        let nr_owned = owner_sexp(owner, nr);
        if TYPEOF(nr) == SEXPTYPE::INTSXP {
            nr_owned
                .try_integer_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                as isize
        } else {
            nr_owned
                .try_real_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                as isize
        }
    };
    let ncol = nx + 1 - nrow;
    if nrow < 1 || ncol < 1 || nrow + ncol - 1 > nx {
        return R_NilValue();
    }
    let ans = alloc_matrix(TYPEOF(x), nrow as i32, ncol as i32);
    let _a = protect(ans);
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    let mut ans_mut = owner
        .sexp(ans)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for col in 0..ncol {
        for row in 0..nrow {
            let k = (ncol - col + row - 1) as R_xlen_t;
            let dst = (row + col * nrow) as R_xlen_t;
            if TYPEOF(x) == SEXPTYPE::INTSXP {
                let v = x_owned
                    .try_integer_elt(k)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                ans_mut
                    .try_set_integer_elt(dst, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            } else {
                let v = x_owned
                    .try_real_elt(k)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                ans_mut
                    .try_set_real_elt(dst, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
        }
    }
    ans
}

/// GNU `diffinv(x, xi=0)`.
pub fn do_diffinv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let n = XLENGTH(x);
    let mut xi = 0.0;
    let rest = CDR(args);
    let owner = active_owner();
    if !rest.is_null() && rest != R_NilValue() {
        let xi_s = CAR(rest);
        let xi_owned = owner_sexp(owner, xi_s);
        if TYPEOF(xi_s) == SEXPTYPE::INTSXP {
            xi = xi_owned
                .try_integer_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                as f64;
        } else if TYPEOF(xi_s) == SEXPTYPE::REALSXP {
            xi = xi_owned
                .try_real_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
    let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n + 1);
    let _a = protect(ans);
    let mut ans_mut = owner
        .sexp(ans)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    ans_mut
        .try_set_real_elt(0, xi)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut acc = xi;
    let x_owned = owner_sexp(owner, x);
    for i in 0..n {
        let dx = if TYPEOF(x) == SEXPTYPE::INTSXP {
            x_owned
                .try_integer_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                as f64
        } else {
            x_owned
                .try_real_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        };
        acc += dx;
        ans_mut
            .try_set_real_elt(i + 1, acc)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    ans
}

#[cfg(test)]
mod data_matrix_tests {
    use crate::sexp::ffi::TRUE;
    use crate::sexp::session::RSession;

    fn assert_r_true(code: &str) {
        let mut session = RSession::new();
        let (result, output, visible) = session.eval_code_with_output_capture(code);
        let result = result.expect("data.matrix expression should evaluate");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(output.stdout.is_empty());
        assert!(visible);
    }

    #[test]
    fn data_matrix_preserves_empty_data_frame_dimensions() {
        assert_r_true(
            "x <- data.frame(a = 1:3); \
             x30 <- x[, -1]; x01 <- x[-(1:3), , drop = FALSE]; x00 <- x01[, -1]; \
             identical(dim(data.matrix(x30)), c(3L, 0L)) && \
             identical(dim(data.matrix(x00)), c(0L, 0L))",
        );
    }

    #[test]
    fn data_matrix_converts_factors_characters_and_logicals() {
        assert_r_true(
            "m <- data.matrix(data.frame(\
                 f = factor(c('b', 'a')), s = c(NA, 'z'), l = c(TRUE, FALSE))); \
             identical(dim(m), c(2L, 3L)) && \
             all(m[, 1] == c(2, 1)) && \
             all(is.na(m[, 2]) == c(TRUE, FALSE)) && \
             all(m[, 3] == c(1, 0)) && \
             identical(colnames(m), c('f', 's', 'l'))",
        );
    }

    #[test]
    fn data_matrix_leaves_existing_matrices_unchanged() {
        assert_r_true("m <- matrix(1:4, 2, 2); identical(data.matrix(m), m)");
    }
}
