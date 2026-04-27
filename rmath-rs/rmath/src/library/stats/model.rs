use std::os::raw::c_int;
use std::ptr;

use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::main::errors::Rf_error;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::memory_ext::*;
use crate::sexp::protect::{protect, Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Helper: nrows (number of rows of a vector/matrix)
// ---------------------------------------------------------------------------

unsafe fn nrows(x: SEXP) -> isize {
    let dim = getAttrib(x, R_DimSymbol());
    if dim == R_NilValue() {
        Rf_length(x) as isize
    } else if Rf_length(dim) >= 1 {
        *INTEGER(dim).add(0) as isize
    } else {
        Rf_length(x) as isize
    }
}

// ---------------------------------------------------------------------------
// Helper: ncols (number of columns of a vector/matrix)
// ---------------------------------------------------------------------------

unsafe fn ncols(x: SEXP) -> isize {
    let dim = getAttrib(x, R_DimSymbol());
    if dim == R_NilValue() {
        1
    } else if Rf_length(dim) >= 2 {
        *INTEGER(dim).add(1) as isize
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// Helper: nlevels
// ---------------------------------------------------------------------------

unsafe fn nlevels(x: SEXP) -> isize {
    let levels = getAttrib(x, Rf_install("levels"));
    if levels == R_NilValue() {
        0
    } else {
        Rf_length(levels) as isize
    }
}

// ---------------------------------------------------------------------------
// Helper: isFactor, isOrdered, isLogical, isNumeric (simplified)
// ---------------------------------------------------------------------------

unsafe fn isFactor(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::INTSXP
}

unsafe fn isOrdered_int(x: SEXP) -> bool {
    // Simplified: check for "ordered" class
    TYPEOF(x) == SEXPTYPE::INTSXP
}

unsafe fn isUnordered_int(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::INTSXP
}

unsafe fn isLogical(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::LGLSXP
}

unsafe fn isNumeric(x: SEXP) -> bool {
    let t = TYPEOF(x);
    t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP
}

unsafe fn isComplex(x: SEXP) -> bool {
    crate::main::coerce::isComplex(x)
}

// ---------------------------------------------------------------------------
// Helper: isString
// ---------------------------------------------------------------------------

unsafe fn isString(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::STRSXP
}

// ---------------------------------------------------------------------------
// Helper: isNewList
// ---------------------------------------------------------------------------

unsafe fn isNewList(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::VECSXP
}

// ---------------------------------------------------------------------------
// Helper: isDataFrame
// ---------------------------------------------------------------------------

unsafe fn isDataFrame(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::VECSXP
    // In a full implementation we'd check inherits(x, "data.frame")
}

// ---------------------------------------------------------------------------
// Helper: isLanguage, isSymbol, isMatrix
// ---------------------------------------------------------------------------

unsafe fn isLanguage(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::LANGSXP
}

unsafe fn isSymbol(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::SYMSXP
}

unsafe fn isMatrix(x: SEXP) -> bool {
    let dim = getAttrib(x, R_DimSymbol());
    !dim.is_null() && Rf_length(dim) == 2
}

unsafe fn isNull(x: SEXP) -> bool {
    x == R_NilValue()
}

// ---------------------------------------------------------------------------
// Helper: translateChar (simplified)
// ---------------------------------------------------------------------------

unsafe fn translateChar(s: SEXP) -> &str {
    if s.is_null() {
        return "";
    }
    let p = CHAR(s);
    if p.is_null() {
        return "";
    }
    std::ffi::CStr::from_ptr(p as *const _)
        .to_str()
        .unwrap_or("")
}

// ---------------------------------------------------------------------------
// Helper: allocMatrix
// ---------------------------------------------------------------------------

unsafe fn allocMatrix(sexptype: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    let s = Rf_allocVector3(sexptype as i32, (nrow * ncol) as R_xlen_t);
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    *INTEGER(dim).add(0) = nrow;
    *INTEGER(dim).add(1) = ncol;
    setAttrib(s, R_DimSymbol(), dim);
    s
}

// ---------------------------------------------------------------------------
// Local SETCADR / SETCADDR helpers
// ---------------------------------------------------------------------------

unsafe fn SETCADR(x: SEXP, y: SEXP) {
    if !x.is_null() {
        let cdr = CDR(x);
        if !cdr.is_null() {
            SETCAR(cdr, y);
        }
    }
}

unsafe fn SETCADDR(x: SEXP, y: SEXP) {
    if !x.is_null() {
        let cddr = CDR(CDR(x));
        if !cddr.is_null() {
            SETCAR(cddr, y);
        }
    }
}

// ---------------------------------------------------------------------------
// model.frame
// ---------------------------------------------------------------------------

pub unsafe fn modelframe(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    args = CDR(args);
    let terms = CAR(args);
    args = CDR(args);
    let row_names = CAR(args);
    args = CDR(args);
    let variables = CAR(args);
    args = CDR(args);
    let varnames = CAR(args);
    args = CDR(args);
    let dots = CAR(args);
    args = CDR(args);
    let dotnames = CAR(args);
    args = CDR(args);
    let subset = CAR(args);
    args = CDR(args);
    let na_action = CAR(args);

    // Argument Sanity Checks
    if !isNewList(variables) {
        Rf_error(b"invalid variables\0".as_ptr() as *const _);
        return R_NilValue();
    }
    if !isString(varnames) {
        Rf_error(b"invalid variable names\0".as_ptr() as *const _);
        return R_NilValue();
    }
    let nvars = Rf_length(variables) as isize;
    if nvars != Rf_length(varnames) as isize {
        Rf_error(b"number of variables != number of variable names\0".as_ptr() as *const _);
        return R_NilValue();
    }
    if !isNewList(dots) {
        Rf_error(b"invalid extra variables\0".as_ptr() as *const _);
        return R_NilValue();
    }
    let ndots = Rf_length(dots) as isize;
    if ndots != Rf_length(dotnames) as isize {
        Rf_error(b"number of variables != number of variable names\0".as_ptr() as *const _);
        return R_NilValue();
    }
    if ndots > 0 && !isString(dotnames) {
        Rf_error(b"invalid extra variable names\0".as_ptr() as *const _);
        return R_NilValue();
    }

    // Check for NULL extra arguments
    let mut nactualdots: isize = 0;
    for i in 0..ndots {
        if VECTOR_ELT(dots, i as R_xlen_t) != R_NilValue() {
            nactualdots += 1;
        }
    }

    // Assemble the base data frame
    let mut guards = Vec::new();
    let mut data = Rf_allocVector3(
        SEXPTYPE::VECSXP.as_c_int(),
        (nvars + nactualdots) as R_xlen_t,
    );
    guards.push(protect(data));
    let names = Rf_allocVector3(
        SEXPTYPE::STRSXP.as_c_int(),
        (nvars + nactualdots) as R_xlen_t,
    );
    guards.push(protect(names));

    for i in 0..nvars {
        SET_VECTOR_ELT(data, i as R_xlen_t, VECTOR_ELT(variables, i as R_xlen_t));
        SET_STRING_ELT(names, i as R_xlen_t, STRING_ELT(varnames, i as R_xlen_t));
    }
    let mut j: isize = 0;
    for i in 0..ndots {
        if VECTOR_ELT(dots, i as R_xlen_t) == R_NilValue() {
            continue;
        }
        let ss = translateChar(STRING_ELT(dotnames, i as R_xlen_t));
        let mut buf = [0u8; 256];
        let buf_str = format!("({})", ss);
        let bytes = buf_str.as_bytes();
        let len = bytes.len().min(255);
        buf[..len].copy_from_slice(bytes);
        buf[len] = 0;
        SET_VECTOR_ELT(
            data,
            (nvars + j) as R_xlen_t,
            VECTOR_ELT(dots, i as R_xlen_t),
        );
        SET_STRING_ELT(
            names,
            (nvars + j) as R_xlen_t,
            Rf_mkChar(
                std::ffi::CStr::from_ptr(buf.as_ptr() as *const _)
                    .to_str()
                    .unwrap_or(""),
            ),
        );
        j += 1;
    }
    setAttrib(data, R_NamesSymbol(), names);

    // Sanity checks
    let nc = Rf_length(data) as isize;
    let mut nr: isize = 0;
    if nc > 0 {
        for i in 0..nc {
            let ans = VECTOR_ELT(data, i as R_xlen_t);
            let t = TYPEOF(ans);
            if t != SEXPTYPE::LGLSXP
                && t != SEXPTYPE::INTSXP
                && t != SEXPTYPE::REALSXP
                && t != SEXPTYPE::CPLXSXP
                && t != SEXPTYPE::STRSXP
                && t != SEXPTYPE::RAWSXP
            {
                Rf_error(b"invalid type for variable\0".as_ptr() as *const _);
                return R_NilValue();
            }
            nr = nrows(VECTOR_ELT(data, 0));
            if nrows(ans) != nr {
                Rf_error(b"variable lengths differ\0".as_ptr() as *const _);
                return R_NilValue();
            }
        }
    } else {
        nr = Rf_length(row_names) as isize;
    }

    let _subset_guard = protect(subset);

    // Turn into data.frame
    let tmp = Rf_mkString("data.frame");
    let _class_guard = protect(tmp);
    setAttrib(data, R_ClassSymbol(), tmp);

    if Rf_length(row_names) == nr && row_names != R_NilValue() {
        setAttrib(data, Rf_install("row.names"), row_names);
    } else {
        let row_names = Rf_allocVector3(
            SEXPTYPE::INTSXP.as_c_int(),
            if nr > 0 { 2 } else { 0 } as R_xlen_t,
        );
        let _row_names_guard = protect(row_names);
        if nr > 0 {
            *INTEGER(row_names).add(0) = NA_INTEGER;
            *INTEGER(row_names).add(1) = nr as c_int;
        }
        setAttrib(data, Rf_install("row.names"), row_names);
    }

    // Subsetting
    if subset != R_NilValue() {
        let bracket_sym = Rf_install("[.data.frame");
        let _bracket_guard = protect(bracket_sym);
        let drop_arg = Rf_ScalarLogical(0);
        let _drop_arg_guard = protect(drop_arg);
        let call = Rf_lang4(
            bracket_sym,
            data,
            subset,
            R_MissingArg(),
            drop_arg,
        );
        let _call_guard = protect(call);
        data = crate::eval::eval::Rf_eval(call, rho);
        guards.push(protect(data));
    }

    // na.action
    let ans: SEXP;
    if na_action != R_NilValue() {
        setAttrib(data, Rf_install("terms"), terms);

        let na_action_val = na_action; // simplified - skip installTrChar
        let na_call = Rf_lang2(na_action_val, data);
        let _na_call_guard = protect(na_call);
        ans = crate::eval::eval::Rf_eval(na_call, rho);
        // Simplified: skip MAYBE_REFERENCED and copyMostAttribNoTs
        let _ans_guard = protect(ans);
    } else {
        ans = data;
    }

    ans
}

// ---------------------------------------------------------------------------
// Model matrix helper functions
// ---------------------------------------------------------------------------

unsafe fn firstfactor(
    x: *mut f64,
    nrx: isize,
    ncx: isize,
    c: *const f64,
    nrc: isize,
    ncc: isize,
    v: *const c_int,
    adj: isize,
) {
    for j in 0..ncc {
        let xj = x.add(j * nrx);
        let cj = c.add(j * nrc);
        for i in 0..nrx {
            if *v.add(i) == NA_INTEGER {
                *xj.add(i) = NA_REAL;
            } else {
                *xj.add(i) = *cj.add(*v.add(i) as usize - 1 + adj as usize);
            }
        }
    }
}

unsafe fn addfactor(
    x: *mut f64,
    nrx: isize,
    ncx: isize,
    c: *const f64,
    nrc: isize,
    ncc: isize,
    v: *const c_int,
    adj: isize,
) {
    for k in (0..ncc).rev() {
        for j in 0..ncx {
            let xj = x.add(j * nrx);
            let yj = x.add((k * ncx + j) * nrx);
            let ck = c.add(k * nrc);
            for i in 0..nrx {
                if *v.add(i) == NA_INTEGER {
                    *yj.add(i) = NA_REAL;
                } else {
                    *yj.add(i) = *ck.add(*v.add(i) as usize - 1 + adj as usize) * *xj.add(i);
                }
            }
        }
    }
}

unsafe fn firstvar(x: *mut f64, nrx: isize, ncx: isize, c: *const f64, nrc: isize, ncc: isize) {
    for j in 0..ncc {
        let xj = x.add(j * nrx);
        let cj = c.add(j * nrc);
        for i in 0..nrx {
            *xj.add(i) = *cj.add(i);
        }
    }
}

unsafe fn addvar(x: *mut f64, nrx: isize, ncx: isize, c: *const f64, nrc: isize, ncc: isize) {
    for k in (0..ncc).rev() {
        for j in 0..ncx {
            let xj = x.add(j * nrx);
            let yj = x.add((k * ncx + j) * nrx);
            let ck = c.add(k * nrc);
            for i in 0..nrx {
                *yj.add(i) = *ck.add(i) * *xj.add(i);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ColumnNames helper
// ---------------------------------------------------------------------------

unsafe fn ColumnNames(x: SEXP) -> SEXP {
    let dn = getAttrib(x, R_DimSymbol());
    if dn == R_NilValue() || Rf_length(dn) < 2 {
        return R_NilValue();
    }
    VECTOR_ELT(dn, 1)
}

// ---------------------------------------------------------------------------
// modelmatrix
// ---------------------------------------------------------------------------

pub unsafe fn modelmatrix(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    args = CDR(args);

    let terms = CAR(args);
    let intrcept = asLogical(getAttrib(terms, Rf_install("intercept")));
    let mut intrcept = if intrcept == NA_INTEGER { 0 } else { intrcept };
    let risponse = asLogical(getAttrib(terms, Rf_install("response")));
    let mut risponse = if risponse == NA_INTEGER { 0 } else { risponse };

    let mut nVar: isize = 0;
    let mut nterms: isize = 0;

    let factors = Rf_protect(crate::sexp::memory_ext::duplicate(getAttrib(
        terms,
        Rf_install("factors"),
    )));
    if Rf_length(factors) == 0 {
        nVar = 1;
        nterms = 0;
    } else if TYPEOF(factors) == SEXPTYPE::INTSXP && isMatrix(factors) {
        nVar = nrows(factors);
        nterms = ncols(factors);
    } else {
        Rf_error(b"invalid 'terms' argument\0".as_ptr() as *const _);
        return R_NilValue();
    }

    // Get variable names
    let vnames_attr = getAttrib(factors, R_DimSymbol());
    let vnames = if !vnames_attr.is_null() && Rf_length(vnames_attr) >= 1 {
        VECTOR_ELT(vnames_attr, 0)
    } else {
        R_NilValue()
    };

    // Get variables
    let vars = CADR(args);
    if !isNewList(vars) || Rf_length(vars) < nVar as i32 {
        Rf_error(b"invalid model frame\0".as_ptr() as *const _);
        return R_NilValue();
    }
    if Rf_length(vars) == 0 {
        Rf_error(b"do not know how many cases\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let n = nrows(VECTOR_ELT(vars, 0));
    let rnames = Rf_protect(getAttrib(vars, Rf_install("row.names")));

    // Check variable types and set up variable info
    let variable = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, nVar as R_xlen_t));
    let nlevs = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP, nVar as R_xlen_t));
    let ordered = Rf_protect(Rf_allocVector3(SEXPTYPE::LGLSXP, nVar as R_xlen_t));
    let columns = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP, nVar as R_xlen_t));

    for i in 0..nVar {
        let mut var_i = VECTOR_ELT(vars, i as R_xlen_t);
        SET_VECTOR_ELT(variable, i as R_xlen_t, var_i);
        if nrows(var_i) != n {
            let msg = std::ffi::CString::new(format!(
                "variable lengths differ (found for variable {})",
                i + 1
            ))
            .unwrap_or_default();
            Rf_error(msg.as_ptr());
            return R_NilValue();
        }
        if isOrdered_int(var_i) {
            *LOGICAL(ordered).add(i as usize) = 1;
            let nl = nlevels(var_i);
            if nl < 1 {
                let msg = std::ffi::CString::new(format!("variable {} has no levels", i + 1))
                    .unwrap_or_default();
                Rf_error(msg.as_ptr());
                return R_NilValue();
            }
            *INTEGER(nlevs).add(i as usize) = nl as c_int;
            *INTEGER(columns).add(i as usize) = ncols(var_i) as c_int;
        } else if isUnordered_int(var_i) {
            *LOGICAL(ordered).add(i as usize) = 0;
            let nl = nlevels(var_i);
            if nl < 1 {
                let msg = std::ffi::CString::new(format!("variable {} has no levels", i + 1))
                    .unwrap_or_default();
                Rf_error(msg.as_ptr());
                return R_NilValue();
            }
            *INTEGER(nlevs).add(i as usize) = nl as c_int;
            *INTEGER(columns).add(i as usize) = ncols(var_i) as c_int;
        } else if isLogical(var_i) {
            *LOGICAL(ordered).add(i as usize) = 0;
            *INTEGER(nlevs).add(i as usize) = 2;
            *INTEGER(columns).add(i as usize) = ncols(var_i) as c_int;
        } else if isNumeric(var_i) {
            var_i = Rf_protect(coerceVector(var_i, SEXPTYPE::REALSXP.as_c_int()));
            SET_VECTOR_ELT(variable, i as R_xlen_t, var_i);
            *LOGICAL(ordered).add(i as usize) = 0;
            *INTEGER(nlevs).add(i as usize) = 0;
            *INTEGER(columns).add(i as usize) = ncols(var_i) as c_int;
        } else {
            *LOGICAL(ordered).add(i as usize) = 0;
            *INTEGER(nlevs).add(i as usize) = 0;
            *INTEGER(columns).add(i as usize) = ncols(var_i) as c_int;
        }
    }

    // No intercept adjustment (simplified - skip the factor pattern adjustment)

    // Compute contrasts
    let contr1 = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, nVar as R_xlen_t));
    let contr2 = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, nVar as R_xlen_t));

    let expr = Rf_protect(Rf_lang3(
        Rf_install("contrasts"),
        R_NilValue(),
        Rf_ScalarLogical(0),
    ));

    for i in 0..nVar {
        if *INTEGER(nlevs).add(i as usize) > 0 {
            let mut k: c_int = 0;
            for j in 0..nterms {
                let fik = *INTEGER(factors).add(i as usize + j as usize * nVar);
                if fik == 1 {
                    k |= 1;
                } else if fik == 2 {
                    k |= 2;
                }
            }
            SETCADR(expr, VECTOR_ELT(variable, i as R_xlen_t));
            if k & 1 != 0 {
                *LOGICAL(CADDR(expr)).add(0) = 1;
                let val = crate::eval::eval::Rf_eval(expr, rho);
                SET_VECTOR_ELT(contr1, i as R_xlen_t, val);
            }
            if k & 2 != 0 {
                *LOGICAL(CADDR(expr)).add(0) = 0;
                let val = crate::eval::eval::Rf_eval(expr, rho);
                SET_VECTOR_ELT(contr2, i as R_xlen_t, val);
            }
        }
    }

    // Compute column counts
    let count = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP, nterms as R_xlen_t));
    let mut dnc: f64 = if intrcept != 0 { 1.0 } else { 0.0 };

    for j in 0..nterms {
        let mut dk: f64 = 1.0;
        for i in 0..nVar {
            let fik = *INTEGER(factors).add(i as usize + j as usize * nVar);
            if fik != 0 {
                if *INTEGER(nlevs).add(i as usize) > 0 {
                    let contr = if fik == 1 { contr1 } else { contr2 };
                    let nc = ncols(VECTOR_ELT(contr, i as R_xlen_t));
                    dk *= nc as f64;
                } else {
                    dk *= *INTEGER(columns).add(i as usize) as f64;
                }
            }
        }
        *INTEGER(count).add(j as usize) = dk as c_int;
        dnc += dk;
    }

    let nc = dnc as isize;

    // Compute assign vector
    let assign = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP, nc as R_xlen_t));
    let mut k: isize = 0;
    if intrcept != 0 {
        *INTEGER(assign).add(k) = 0;
        k += 1;
    }
    for j in 0..nterms {
        for _i in 0..*INTEGER(count).add(j as usize) {
            *INTEGER(assign).add(k) = (j + 1) as c_int;
            k += 1;
        }
    }

    // Create column labels
    let xnames = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP, nc as R_xlen_t));
    k = 0;
    if intrcept != 0 {
        SET_STRING_ELT(xnames, k, Rf_mkChar("(Intercept)"));
        k += 1;
    }
    for j in 0..nterms {
        let count_j = *INTEGER(count).add(j as usize) as isize;
        for kk in 0..count_j {
            let mut first = true;
            let mut indx = kk as isize;
            let mut buf = String::new();
            for i in 0..nVar {
                let ll = *INTEGER(factors).add(i as usize + j as usize * nVar);
                if ll != 0 {
                    if !first {
                        buf.push(':');
                    }
                    first = false;
                    let var_i = VECTOR_ELT(variable, i as R_xlen_t);
                    if isFactor(var_i) || isLogical(var_i) {
                        let contr = if ll == 1 { contr1 } else { contr2 };
                        let nc_col = ncols(VECTOR_ELT(contr, i as R_xlen_t)) as isize;
                        if !vnames.is_null() {
                            let addp = translateChar(STRING_ELT(vnames, i as R_xlen_t));
                            buf.push_str(addp);
                        }
                        let x_col = ColumnNames(VECTOR_ELT(contr, i as R_xlen_t));
                        if x_col != R_NilValue() {
                            let addp =
                                translateChar(STRING_ELT(x_col, (indx % nc_col) as R_xlen_t));
                            buf.push_str(addp);
                        } else {
                            buf.push_str(&format!("{}", indx % nc_col + 1));
                        }
                        indx /= nc_col;
                    } else if isComplex(var_i) {
                        Rf_error(
                            b"complex variables are not currently allowed in model matrices\0"
                                .as_ptr() as *const _,
                        );
                        return R_NilValue();
                    } else {
                        let nc_col = ncols(var_i);
                        if !vnames.is_null() {
                            let addp = translateChar(STRING_ELT(vnames, i as R_xlen_t));
                            buf.push_str(addp);
                        }
                        if nc_col > 1 {
                            let x_col = ColumnNames(var_i);
                            if x_col != R_NilValue() {
                                let addp =
                                    translateChar(STRING_ELT(x_col, (indx % nc_col) as R_xlen_t));
                                buf.push_str(addp);
                            } else {
                                buf.push_str(&format!("{}", indx % nc_col + 1));
                            }
                        }
                        indx /= nc_col;
                    }
                }
            }
            SET_STRING_ELT(xnames, k, Rf_mkChar(&buf));
            k += 1;
        }
    }

    // Allocate and compute the design matrix
    let x = Rf_protect(allocMatrix(SEXPTYPE::REALSXP, n as c_int, nc as c_int));
    let rx = REAL(x);

    // Begin with intercept column
    let mut jnext: isize = if intrcept != 0 { 1 } else { 0 };
    let jstart = jnext;

    if jnext > 0 {
        for i in 0..n {
            *rx.add(i) = 1.0;
        }
    }

    // Loop over model terms
    for _k in 0..nterms {
        for i in 0..nVar {
            if *INTEGER(columns).add(i as usize) == 0 {
                continue;
            }
            let var_i = VECTOR_ELT(variable, i as R_xlen_t);
            let fik = *INTEGER(factors).add(i as usize + _k as usize * nVar);

            if fik != 0 {
                let contrast = if fik == 1 { contr1 } else { contr2 };
                let contrast = Rf_protect(coerceVector(
                    VECTOR_ELT(contrast, i as R_xlen_t),
                    SEXPTYPE::REALSXP.as_c_int(),
                ));

                if jnext == jstart {
                    if *INTEGER(nlevs).add(i as usize) > 0 {
                        let adj = if isLogical(var_i) { 1 } else { 0 };
                        firstfactor(
                            rx.add(jstart * n),
                            n,
                            jnext - jstart,
                            REAL(contrast),
                            nrows(VECTOR_ELT(contrast, i as R_xlen_t)),
                            ncols(VECTOR_ELT(contrast, i as R_xlen_t)),
                            INTEGER(var_i),
                            adj,
                        );
                        jnext += ncols(VECTOR_ELT(contrast, i as R_xlen_t));
                    } else {
                        firstvar(
                            rx.add(jstart * n),
                            n,
                            jnext - jstart,
                            REAL(var_i),
                            n,
                            ncols(var_i),
                        );
                        jnext += ncols(var_i);
                    }
                } else {
                    if *INTEGER(nlevs).add(i as usize) > 0 {
                        let adj = if isLogical(var_i) { 1 } else { 0 };
                        addfactor(
                            rx.add(jstart * n),
                            n,
                            jnext - jstart,
                            REAL(contrast),
                            nrows(VECTOR_ELT(contrast, i as R_xlen_t)),
                            ncols(VECTOR_ELT(contrast, i as R_xlen_t)),
                            INTEGER(var_i),
                            adj,
                        );
                        jnext +=
                            (jnext - jstart) * (ncols(VECTOR_ELT(contrast, i as R_xlen_t)) - 1);
                    } else {
                        addvar(
                            rx.add(jstart * n),
                            n,
                            jnext - jstart,
                            REAL(var_i),
                            n,
                            ncols(var_i),
                        );
                        jnext += (jnext - jstart) * (ncols(var_i) - 1);
                    }
                }
                Rf_unprotect(1);
            }
        }
        jstart = jnext;
    }

    // Set dimnames
    let tnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    SET_VECTOR_ELT(tnames, 0, rnames);
    SET_VECTOR_ELT(tnames, 1, xnames);
    setAttrib(x, R_DimNamesSymbol(), tnames);
    setAttrib(x, Rf_install("assign"), assign);

    Rf_unprotect(9);
    x
}

// ---------------------------------------------------------------------------
// updateform
// ---------------------------------------------------------------------------

pub unsafe fn updateform(old: SEXP, new_: SEXP) -> SEXP {
    let tildeSymbol = Rf_install("~");
    let plusSymbol = Rf_install("+");
    let minusSymbol = Rf_install("-");
    let timesSymbol = Rf_install("*");
    let slashSymbol = Rf_install("/");
    let colonSymbol = Rf_install(":");
    let powerSymbol = Rf_install("^");
    let dotSymbol = Rf_install(".");
    let parenSymbol = Rf_install("(");
    let inSymbol = Rf_install("%in%");

    let _new = Rf_protect(crate::sexp::memory_ext::duplicate(new_));

    if TYPEOF(old) != SEXPTYPE::LANGSXP
        || (TYPEOF(_new) != SEXPTYPE::LANGSXP && CAR(old) != tildeSymbol)
        || CAR(_new) != tildeSymbol
    {
        Rf_error(b"formula expected\0".as_ptr() as *const _);
        return R_NilValue();
    }

    if Rf_length(old) == 3 {
        let lhs = CADR(old);
        let rhs = CADDR(old);

        if Rf_length(_new) == 2 {
            SETCDR(_new, Rf_cons(lhs, CDR(_new)));
        }

        Rf_protect(rhs);
        SETCADR(_new, ExpandDots(CADR(_new), lhs));
        SETCADDR(_new, ExpandDots(CADDR(_new), rhs));
        Rf_unprotect(1);
    } else {
        let rhs = CADR(old);
        if Rf_length(_new) == 3 {
            SETCADDR(_new, ExpandDots(CADDR(_new), rhs));
        } else {
            SETCADR(_new, ExpandDots(CADR(_new), rhs));
        }
    }

    // Clear attributes
    setAttrib(_new, R_NilValue());
    SET_OBJECT(_new, 0);
    setAttrib(
        _new,
        Rf_install(".Environment"),
        getAttrib(old, Rf_install(".Environment")),
    );

    Rf_unprotect(1);
    _new
}

// ---------------------------------------------------------------------------
// ExpandDots - helper for updateform
// ---------------------------------------------------------------------------

unsafe fn ExpandDots(object: SEXP, value: SEXP) -> SEXP {
    if isSymbol(object) {
        if object == dotSymbol {
            return crate::sexp::memory_ext::duplicate(value);
        }
        return object;
    }

    if isLanguage(object) {
        let op = if TYPEOF(value) == SEXPTYPE::LANGSXP {
            CAR(value)
        } else {
            R_NilValue()
        };
        Rf_protect(object);

        if CAR(object) == plusSymbol {
            let len = Rf_length(object);
            if len == 2 {
                SETCADR(object, ExpandDots(CADR(object), value));
            } else if len == 3 {
                SETCADR(object, ExpandDots(CADR(object), value));
                SETCADDR(object, ExpandDots(CADDR(object), value));
            }
        } else if CAR(object) == minusSymbol {
            let len = Rf_length(object);
            if len == 2 {
                if CADR(object) == dotSymbol && (op == plusSymbol || op == minusSymbol) {
                    SETCADR(
                        object,
                        Rf_lang2(parenSymbol, ExpandDots(CADR(object), value)),
                    );
                } else {
                    SETCADR(object, ExpandDots(CADR(object), value));
                }
            } else if len == 3 {
                if CADR(object) == dotSymbol && (op == plusSymbol || op == minusSymbol) {
                    SETCADR(
                        object,
                        Rf_lang2(parenSymbol, ExpandDots(CADR(object), value)),
                    );
                } else {
                    SETCADR(object, ExpandDots(CADR(object), value));
                }
                if CADDR(object) == dotSymbol && (op == plusSymbol || op == minusSymbol) {
                    SETCADDR(
                        object,
                        Rf_lang2(parenSymbol, ExpandDots(CADDR(object), value)),
                    );
                } else {
                    SETCADDR(object, ExpandDots(CADDR(object), value));
                }
            }
        } else if CAR(object) == timesSymbol || CAR(object) == slashSymbol {
            if CADR(object) == dotSymbol && (op == plusSymbol || op == minusSymbol) {
                SETCADR(
                    object,
                    Rf_lang2(parenSymbol, ExpandDots(CADR(object), value)),
                );
            } else {
                SETCADR(object, ExpandDots(CADR(object), value));
            }
            if CADDR(object) == dotSymbol && (op == plusSymbol || op == minusSymbol) {
                SETCADDR(
                    object,
                    Rf_lang2(parenSymbol, ExpandDots(CADDR(object), value)),
                );
            } else {
                SETCADDR(object, ExpandDots(CADDR(object), value));
            }
        } else if CAR(object) == colonSymbol {
            if CADR(object) == dotSymbol
                && (op == plusSymbol || op == minusSymbol || op == timesSymbol || op == slashSymbol)
            {
                SETCADR(
                    object,
                    Rf_lang2(parenSymbol, ExpandDots(CADR(object), value)),
                );
            } else {
                SETCADR(object, ExpandDots(CADR(object), value));
            }
            if CADDR(object) == dotSymbol && (op == plusSymbol || op == minusSymbol) {
                SETCADDR(
                    object,
                    Rf_lang2(parenSymbol, ExpandDots(CADDR(object), value)),
                );
            } else {
                SETCADDR(object, ExpandDots(CADDR(object), value));
            }
        } else if CAR(object) == powerSymbol {
            if CADR(object) == dotSymbol
                && (op == plusSymbol
                    || op == minusSymbol
                    || op == timesSymbol
                    || op == slashSymbol
                    || op == colonSymbol)
            {
                SETCADR(
                    object,
                    Rf_lang2(parenSymbol, ExpandDots(CADR(object), value)),
                );
            } else {
                SETCADR(object, ExpandDots(CADR(object), value));
            }
            if CADDR(object) == dotSymbol && (op == plusSymbol || op == minusSymbol) {
                SETCADDR(
                    object,
                    Rf_lang2(parenSymbol, ExpandDots(CADDR(object), value)),
                );
            } else {
                SETCADDR(object, ExpandDots(CADDR(object), value));
            }
        } else {
            let mut op2 = object;
            while op2 != R_NilValue() {
                SETCAR(op2, ExpandDots(CAR(op2), value));
                op2 = CDR(op2);
            }
        }
        Rf_unprotect(1);
        return object;
    }

    object
}

// ---------------------------------------------------------------------------
// termsform - workhorse to turn model formula into terms object
// ---------------------------------------------------------------------------

// Global state variables for terms computation
thread_local! {
    static ref INTERCEPT: std::cell::Cell<bool> = std::cell::Cell::new(false);
    static ref PARITY: std::cell::Cell<bool> = std::cell::Cell::new(false);
    static ref RESPONSE: std::cell::Cell<bool> = std::cell::Cell::new(false);
    static ref NWORDS: std::cell::Cell<isize> = std::cell::Cell::new(0);
    static ref VARLIST: std::cell::Cell<*mut std::ffi::c_void> = std::cell::Cell::new(ptr::null_mut());
    static ref FRAMENAMES: std::cell::Cell<*mut std::ffi::c_void> = std::cell::Cell::new(ptr::null_mut());
    static ref HAVE_DOT: std::cell::Cell<bool> = std::cell::Cell::new(false);
}

// Helper: isZeroOne
unsafe fn isZeroOne(x: SEXP) -> bool {
    isNumeric(x) && (asReal(x) == 0.0 || asReal(x) == 1.0)
}

unsafe fn isZeroS(x: SEXP) -> bool {
    isNumeric(x) && asReal(x) == 0.0
}

unsafe fn isOneS(x: SEXP) -> bool {
    isNumeric(x) && asReal(x) == 1.0
}

// Helper: MatchVar
unsafe fn MatchVar(var1: SEXP, var2: SEXP) -> bool {
    if var1 == var2 {
        return true;
    }
    if isNull(var1) && isNull(var2) {
        return true;
    }
    if isNull(var1) || isNull(var2) {
        return false;
    }
    if (isLanguage(var1) || isNewList(var1)) && (isLanguage(var2) || isNewList(var2)) {
        return MatchVar(CAR(var1), CAR(var2))
            && MatchVar(CDR(var1), CDR(var2))
            && MatchVar(TAG(var1), TAG(var2));
    }
    if isSymbol(var1) && isSymbol(var2) {
        return var1 == var2;
    }
    if isNumeric(var1) && isNumeric(var2) {
        let t1 = asReal(var1);
        let t2 = asReal(var2);
        if ISNAN(t1) {
            return ISNAN(t2);
        }
        return t1 == t2;
    }
    if isString(var1) && isString(var2) {
        // Simplified string comparison
        return false;
    }
    false
}

// Helper: InstallVar
unsafe fn InstallVar(var: SEXP) -> isize {
    if !isSymbol(var) && !isLanguage(var) && !isZeroOne(var) {
        Rf_error(b"invalid term in model formula\0".as_ptr() as *const _);
        return 0;
    }

    let varlist = VARLIST.get();
    if varlist.is_null() {
        return 1;
    }
    let mut v = varlist as *mut std::ffi::c_void;
    let mut indx: isize = 0;
    loop {
        v = CDR(v as SEXP);
        if v == R_NilValue() as *mut std::ffi::c_void {
            break;
        }
        indx += 1;
        if MatchVar(var, CADR(v as SEXP)) {
            return indx;
        }
    }

    // Add to end of varlist
    SETCDR(v as SEXP, Rf_cons(var, R_NilValue()));
    indx + 1
}

// Helper: CheckRHS
unsafe fn CheckRHS(v: SEXP) {
    while (isLanguage(v) || isNewList(v)) && v != R_NilValue() {
        CheckRHS(CAR(v));
        v = CDR(v);
    }
    if isSymbol(v) {
        let framenames = FRAMENAMES.get();
        if framenames.is_null() {
            return;
        }
        let flen = Rf_length(framenames as SEXP) as isize;
        // Simplified: skip frame name check
        let _ = flen;
    }
}

// Helper: ExtractVars
unsafe fn ExtractVars(formula: SEXP) {
    if isNull(formula) || isZeroOne(formula) {
        return;
    }
    if isSymbol(formula) {
        if formula == Rf_install(".") {
            HAVE_DOT.set(true);
        }
        if HAVE_DOT.get() {
            let framenames = FRAMENAMES.get();
            if !framenames.is_null() {
                let flen = Rf_length(framenames as SEXP) as isize;
                for i in 0..flen {
                    let vname = STRING_ELT(framenames as SEXP, i as R_xlen_t);
                    let s = std::ffi::CStr::from_ptr(CHAR(vname) as *const _)
                        .to_str()
                        .unwrap_or("");
                    let sym = Rf_install(s);
                    if !MatchVar(sym, CADR(VARLIST.get() as SEXP)) {
                        InstallVar(sym);
                    }
                }
            } else {
                InstallVar(formula);
            }
        } else {
            InstallVar(formula);
        }
        return;
    }
    if isLanguage(formula) {
        let tildeSymbol = Rf_install("~");
        let plusSymbol = Rf_install("+");
        let colonSymbol = Rf_install(":");
        let powerSymbol = Rf_install("^");
        let timesSymbol = Rf_install("*");
        let inSymbol = Rf_install("%in%");
        let slashSymbol = Rf_install("/");
        let minusSymbol = Rf_install("-");
        let parenSymbol = Rf_install("(");

        if CAR(formula) == tildeSymbol {
            if RESPONSE.get() {
                Rf_error(b"invalid model formula\0".as_ptr() as *const _);
                return;
            }
            if CDDR(formula) == R_NilValue() {
                RESPONSE.set(false);
                ExtractVars(CADR(formula));
            } else {
                RESPONSE.set(true);
                InstallVar(CADR(formula));
                ExtractVars(CADDR(formula));
            }
            return;
        }
        if CAR(formula) == plusSymbol {
            let len = Rf_length(formula);
            if len > 1 {
                ExtractVars(CADR(formula));
            }
            if len > 2 {
                ExtractVars(CADDR(formula));
            }
            return;
        }
        if CAR(formula) == colonSymbol {
            ExtractVars(CADR(formula));
            ExtractVars(CADDR(formula));
            return;
        }
        if CAR(formula) == powerSymbol {
            ExtractVars(CADR(formula));
            return;
        }
        if CAR(formula) == timesSymbol {
            ExtractVars(CADR(formula));
            ExtractVars(CADDR(formula));
            return;
        }
        if CAR(formula) == inSymbol {
            ExtractVars(CADR(formula));
            ExtractVars(CADDR(formula));
            return;
        }
        if CAR(formula) == slashSymbol {
            ExtractVars(CADR(formula));
            ExtractVars(CADDR(formula));
            return;
        }
        if CAR(formula) == minusSymbol {
            let len = Rf_length(formula);
            if len == 2 {
                ExtractVars(CADR(formula));
            } else {
                ExtractVars(CADR(formula));
                ExtractVars(CADDR(formula));
            }
            return;
        }
        if CAR(formula) == parenSymbol {
            ExtractVars(CADR(formula));
            return;
        }
        // All other calls
        InstallVar(formula);
        return;
    }
    Rf_error(b"invalid model formula in ExtractVars\0".as_ptr() as *const _);
}

// Helper: AllocTerm
unsafe fn AllocTerm() -> SEXP {
    let nw = NWORDS.get();
    let term = Rf_allocVector3(SEXPTYPE::INTSXP, nw as R_xlen_t);
    for i in 0..nw {
        *INTEGER(term).add(i as usize) = 0;
    }
    term
}

// Helper: SetBit
unsafe fn SetBit(term: SEXP, whichBit: isize, value: c_int) {
    let word = (whichBit - 1) / (8 * std::mem::size_of::<c_int>());
    let offset = -(whichBit as isize) % (8 * std::mem::size_of::<c_int>());
    if value != 0 {
        let p = INTEGER(term).add(word);
        *p |= 1 << offset;
    } else {
        let p = INTEGER(term).add(word);
        *p &= !(1 << offset);
    }
}

// Helper: GetBit
unsafe fn GetBit(term: SEXP, whichBit: isize) -> c_int {
    let word = (whichBit - 1) / (8 * std::mem::size_of::<c_int>());
    let offset = -(whichBit as isize) % (8 * std::mem::size_of::<c_int>());
    ((*INTEGER(term).add(word) >> offset) & 1) as c_int
}

// Helper: OrBits
unsafe fn OrBits(term1: SEXP, term2: SEXP) -> SEXP {
    let term = AllocTerm();
    let nw = NWORDS.get();
    for i in 0..nw {
        *INTEGER(term).add(i as usize) =
            *INTEGER(term1).add(i as usize) | *INTEGER(term2).add(i as usize);
    }
    term
}

// Helper: BitCount
unsafe fn BitCount(term: SEXP, nvar: isize) -> c_int {
    let mut sum: c_int = 0;
    for i in 1..=nvar {
        sum += GetBit(term, i);
    }
    sum
}

// Helper: TermZero
unsafe fn TermZero(term: SEXP) -> bool {
    let nw = NWORDS.get();
    for i in 0..nw {
        if *INTEGER(term).add(i as usize) != 0 {
            return false;
        }
    }
    true
}

// Helper: TermEqual
unsafe fn TermEqual(term1: SEXP, term2: SEXP) -> bool {
    let nw = NWORDS.get();
    for i in 0..nw {
        if *INTEGER(term1).add(i as usize) != *INTEGER(term2).add(i as usize) {
            return false;
        }
    }
    true
}

// Helper: StripTerm
unsafe fn StripTerm(term: SEXP, mut list: SEXP) -> SEXP {
    if TermZero(term) {
        INTERCEPT.set(false);
    }
    let mut root: SEXP = R_NilValue();
    let mut prev: SEXP = R_NilValue();
    while list != R_NilValue() {
        if TermEqual(term, CAR(list)) {
            if prev != R_NilValue() {
                SETCDR(prev, CDR(list));
            }
        } else {
            if root == R_NilValue() {
                root = list;
            }
            prev = list;
        }
        list = CDR(list);
    }
    root
}

// Helper: TrimRepeats (simplified - remove duplicates)
unsafe fn TrimRepeats(list: SEXP) -> SEXP {
    // Drop zero terms at start
    let mut list = list;
    while list != R_NilValue() && TermZero(CAR(list)) {
        list = CDR(list);
    }
    if list == R_NilValue() || CDR(list) == R_NilValue() {
        return list;
    }
    // Simplified: skip full duplicate removal (would need duplicated())
    list
}

// Helper: AllocTermSetBit1
unsafe fn AllocTermSetBit1(var: SEXP) -> SEXP {
    let whichBit = InstallVar(var);
    let term = AllocTerm();
    SetBit(term, whichBit, 1);
    term
}

// Helper: TermCode
unsafe fn TermCode(termlist: SEXP, thisterm: SEXP, whichbit: isize, term: SEXP) -> c_int {
    let nw = NWORDS.get();
    for i in 0..nw {
        *INTEGER(term).add(i as usize) = *INTEGER(CAR(thisterm)).add(i as usize);
    }
    SetBit(term, whichbit, 0);

    let mut allzero = true;
    for i in 0..nw {
        if *INTEGER(term).add(i as usize) != 0 {
            allzero = false;
            break;
        }
    }
    if allzero {
        return 1;
    }

    let mut t = termlist;
    while t != thisterm {
        allzero = true;
        for i in 0..nw {
            let val = *INTEGER(term).add(i as usize);
            let ct = *INTEGER(CAR(t)).add(i as usize);
            if val & !ct != 0 {
                allzero = false;
                break;
            }
        }
        if allzero {
            return 1;
        }
        t = CDR(t);
    }
    2
}

// ---------------------------------------------------------------------------
// termsform - main entry point
// ---------------------------------------------------------------------------

pub unsafe fn termsform(args: SEXP) -> SEXP {
    let args = CDR(args);

    let tildeSymbol = Rf_install("~");
    let plusSymbol = Rf_install("+");
    let minusSymbol = Rf_install("-");
    let timesSymbol = Rf_install("*");
    let slashSymbol = Rf_install("/");
    let colonSymbol = Rf_install(":");
    let powerSymbol = Rf_install("^");
    let dotSymbol = Rf_install(".");
    let parenSymbol = Rf_install("(");
    let inSymbol = Rf_install("%in%");

    if !isLanguage(CAR(args))
        || CAR(CAR(args)) != tildeSymbol
        || (Rf_length(CAR(args)) != 2 && Rf_length(CAR(args)) != 3)
    {
        Rf_error(b"argument is not a valid model\0".as_ptr() as *const _);
        return R_NilValue();
    }

    HAVE_DOT.set(false);

    let ans = Rf_protect(crate::sexp::memory_ext::duplicate(CAR(args)));

    let specials = CADR(args);
    if Rf_length(specials) > 0 && !isString(specials) {
        Rf_error(b"'specials' must be NULL or a character vector\0".as_ptr() as *const _);
        return R_NilValue();
    }

    let mut a = CDDR(args);
    let data = CAR(a);
    a = CDR(a);

    let framenames_val: SEXP;
    if isNull(data) || TYPEOF(data) == SEXPTYPE::ENVSXP {
        FRAMENAMES.set(ptr::null_mut());
        framenames_val = R_NilValue();
    } else if isDataFrame(data) {
        let fn_val = getAttrib(data, R_NamesSymbol());
        FRAMENAMES.set(fn_val as *mut std::ffi::c_void);
        framenames_val = fn_val;
    } else {
        Rf_error(b"'data' argument is of the wrong type\0".as_ptr() as *const _);
        return R_NilValue();
    }
    let _ = framenames_val;

    let keepOrder = {
        let aLog = asLogical(CAR(a));
        if aLog == NA_INTEGER { 0 } else { aLog }
    };
    a = CDR(a);
    let allowDot = {
        let aLog = asLogical(CAR(a));
        if aLog == NA_INTEGER { 0 } else { aLog }
    };

    // Step 1: Extract variables
    INTERCEPT.set(true);
    PARITY.set(true);
    RESPONSE.set(false);

    let varlist = Rf_protect(Rf_cons(Rf_install("list"), R_NilValue()));
    VARLIST.set(varlist as *mut std::ffi::c_void);

    ExtractVars(CAR(args));

    let nvar = (Rf_length(VARLIST.get() as SEXP) - 1) as isize;
    NWORDS.set(nvar / (8 * std::mem::size_of::<c_int>()) + 1);

    // Step 2: Encode variables
    let formula = Rf_protect(EncodeVars(CAR(args)));

    // Step 2a: Compute variable names
    let varnames = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP, nvar as R_xlen_t));
    {
        let mut v = CDR(VARLIST.get() as SEXP);
        let mut idx: R_xlen_t = 0;
        while v != R_NilValue() {
            SET_STRING_ELT(
                varnames,
                idx,
                STRING_ELT(Rf_allocVector3(SEXPTYPE::EXPRSXP, 1), 0),
            );
            // Simplified: use the symbol name directly
            idx += 1;
            v = CDR(v);
        }
    }

    // Step 2b: Find and remove offsets
    let mut nterm = Rf_length(formula) as isize;
    // Skip offset removal for simplicity (would need deparse1line)

    // Step 3: Reorder terms
    let ord = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP, nterm as R_xlen_t));
    let pattern = Rf_protect(Rf_allocVector3(SEXPTYPE::VECSXP, nterm as R_xlen_t));

    let mut call = formula;
    let mut bitmax: c_int = 0;
    for idx in 0..nterm {
        SET_VECTOR_ELT(pattern, idx as R_xlen_t, CAR(call));
        let bc = BitCount(CAR(call), nvar);
        *INTEGER(ord).add(idx as usize) = bc;
        if bc > bitmax {
            bitmax = bc;
        }
        call = CDR(call);
    }

    // Step 4: Compute factor pattern
    let pat: SEXP;
    if nterm > 0 {
        pat = Rf_protect(allocMatrix(
            SEXPTYPE::INTSXP.as_c_int(),
            nvar as c_int,
            nterm as c_int,
        ));
        let patn = INTEGER(pat);
        for idx in 0..(nterm * nvar) {
            *patn.add(idx) = 0;
        }
        let term = Rf_protect(AllocTerm());
        let mut nn: isize = -1;
        call = formula;
        for _idx in 0..nterm {
            for i in 1..=nvar {
                if GetBit(CAR(call), i) != 0 {
                    *patn.add(i as usize + nn as usize) = TermCode(formula, call, i, term);
                }
            }
            nn += nvar;
        }
    } else {
        pat = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP, 0));
    }

    // Step 5: Compute term labels
    let termlabs = Rf_protect(Rf_allocVector3(SEXPTYPE::STRSXP, nterm as R_xlen_t));
    {
        let mut call = formula;
        let mut idx: R_xlen_t = 0;
        while call != R_NilValue() {
            let mut buf = String::new();
            let mut l: usize = 0;
            for i in 1..=nvar {
                if GetBit(CAR(call), i) != 0 {
                    if l > 0 {
                        buf.push(':');
                    }
                    let name = translateChar(STRING_ELT(varnames, (i - 1) as R_xlen_t));
                    buf.push_str(name);
                    l += 1;
                }
            }
            SET_STRING_ELT(termlabs, idx, Rf_mkChar(&buf));
            idx += 1;
            call = CDR(call);
        }
    }

    // Set dimnames on pattern
    if nterm > 0 {
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        SET_VECTOR_ELT(dn, 0, varnames);
        SET_VECTOR_ELT(dn, 1, termlabs);
        setAttrib(pat, R_DimNamesSymbol(), dn);
    }

    // Set remaining attributes
    // (simplified - skip specials, dot expansion, etc.)

    let order_vec = Rf_allocVector3(SEXPTYPE::INTSXP, nterm as R_xlen_t);
    call = formula;
    for idx in 0..nterm {
        *INTEGER(order_vec).add(idx as usize) = *INTEGER(ord).add(idx as usize);
        call = CDR(call);
    }

    // Build the result as a formula with attributes
    setAttrib(ans, R_NilValue(), R_NilValue());
    SET_OBJECT(ans, 0);

    Rf_unprotect(7);
    ans
}

// ---------------------------------------------------------------------------
// EncodeVars - encode formula into bit string representation
// ---------------------------------------------------------------------------

unsafe fn EncodeVars(formula: SEXP) -> SEXP {
    if isNull(formula) {
        return R_NilValue();
    }
    if isOneS(formula) {
        INTERCEPT.set(PARITY.get());
        return R_NilValue();
    }
    if isZeroS(formula) {
        INTERCEPT.set(!PARITY.get());
        return R_NilValue();
    }

    let dotSymbol = Rf_install(".");
    let tildeSymbol = Rf_install("~");
    let plusSymbol = Rf_install("+");
    let colonSymbol = Rf_install(":");
    let timesSymbol = Rf_install("*");
    let inSymbol = Rf_install("%in%");
    let slashSymbol = Rf_install("/");
    let powerSymbol = Rf_install("^");
    let minusSymbol = Rf_install("-");
    let parenSymbol = Rf_install("(");

    if isSymbol(formula) {
        if formula == dotSymbol {
            let framenames = FRAMENAMES.get();
            if !framenames.is_null() {
                let flen = Rf_length(framenames as SEXP) as isize;
                if flen == 0 {
                    return R_NilValue();
                }
                let mut r: SEXP = R_NilValue();
                for i in 0..flen {
                    let c = translateChar(STRING_ELT(framenames as SEXP, i as R_xlen_t));
                    let sym = Rf_install(c);
                    let term = AllocTermSetBit1(sym);
                    if i == 0 {
                        r = Rf_protect(Rf_cons(term, R_NilValue()));
                    } else {
                        SETCDR(r as SEXP, Rf_cons(term, R_NilValue()));
                        r = CDR(r as SEXP);
                    }
                }
                Rf_unprotect(1);
                r
            } else {
                let term = AllocTermSetBit1(formula);
                Rf_cons(term, R_NilValue())
            }
        }
    } else if isLanguage(formula) {
        if CAR(formula) == tildeSymbol {
            if CDDR(formula) == R_NilValue() {
                EncodeVars(CADR(formula))
            } else {
                EncodeVars(CADDR(formula))
            }
        } else if CAR(formula) == plusSymbol {
            let len = Rf_length(formula);
            if len == 2 {
                EncodeVars(CADR(formula))
            } else {
                PlusTerms(CADR(formula), CADDR(formula))
            }
        } else if CAR(formula) == colonSymbol {
            InteractTerms(CADR(formula), CADDR(formula))
        } else if CAR(formula) == timesSymbol {
            CrossTerms(CADR(formula), CADDR(formula))
        } else if CAR(formula) == inSymbol {
            InTerms(CADR(formula), CADDR(formula))
        } else if CAR(formula) == slashSymbol {
            NestTerms(CADR(formula), CADDR(formula))
        } else if CAR(formula) == powerSymbol {
            PowerTerms(CADR(formula), CADDR(formula))
        } else if CAR(formula) == minusSymbol {
            let len = Rf_length(formula);
            if len == 2 {
                DeleteTerms(R_NilValue(), CADR(formula))
            } else {
                DeleteTerms(CADR(formula), CADDR(formula))
            }
        } else if CAR(formula) == parenSymbol {
            EncodeVars(CADR(formula))
        } else {
            let term = AllocTermSetBit1(formula);
            Rf_cons(term, R_NilValue())
        }
    } else {
        Rf_error(b"invalid model formula in EncodeVars\0".as_ptr() as *const _);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Term manipulation helpers
// ---------------------------------------------------------------------------

unsafe fn PlusTerms(left: SEXP, right: SEXP) -> SEXP {
    let left = Rf_protect(EncodeVars(left));
    let right = EncodeVars(right);
    Rf_unprotect(1);
    TrimRepeats(listAppend(left, right))
}

unsafe fn InteractTerms(left: SEXP, right: SEXP) -> SEXP {
    let left = Rf_protect(EncodeVars(left));
    let right = Rf_protect(EncodeVars(right));
    let term = Rf_protect(allocList((Rf_length(left) * Rf_length(right)) as c_int));
    let mut t = term;
    let mut l = left;
    while l != R_NilValue() {
        let mut r = right;
        while r != R_NilValue() {
            SETCAR(t, OrBits(CAR(l), CAR(r)));
            t = CDR(t);
            r = CDR(r);
        }
        l = CDR(l);
    }
    Rf_unprotect(3);
    TrimRepeats(term)
}

unsafe fn CrossTerms(left: SEXP, right: SEXP) -> SEXP {
    let left = Rf_protect(EncodeVars(left));
    let right = Rf_protect(EncodeVars(right));
    let term = Rf_protect(allocList((Rf_length(left) * Rf_length(right)) as c_int));
    let mut t = term;
    let mut l = left;
    while l != R_NilValue() {
        let mut r = right;
        while r != R_NilValue() {
            SETCAR(t, OrBits(CAR(l), CAR(r)));
            t = CDR(t);
            r = CDR(r);
        }
        l = CDR(l);
    }
    listAppend(right, term);
    listAppend(left, right);
    Rf_unprotect(3);
    TrimRepeats(left)
}

unsafe fn PowerTerms(left: SEXP, right: SEXP) -> SEXP {
    let ip = asInteger(right);
    if ip == NA_INTEGER || ip <= 1 {
        Rf_error(b"invalid power in formula\0".as_ptr() as *const _);
        return R_NilValue();
    }
    let left = Rf_protect(EncodeVars(left));
    let mut right_val = left;
    let mut term: SEXP = R_NilValue();
    for _i in 1..ip {
        Rf_protect(right_val);
        term = Rf_protect(allocList((Rf_length(left) * Rf_length(right_val)) as c_int));
        let mut t = term;
        let mut l = left;
        while l != R_NilValue() {
            let mut r = right_val;
            while r != R_NilValue() {
                SETCAR(t, OrBits(CAR(l), CAR(r)));
                t = CDR(t);
                r = CDR(r);
            }
            l = CDR(l);
        }
        Rf_unprotect(2);
        right_val = TrimRepeats(term);
    }
    Rf_unprotect(1);
    term
}

unsafe fn InTerms(left: SEXP, right: SEXP) -> SEXP {
    let left = Rf_protect(EncodeVars(left));
    let right = Rf_protect(EncodeVars(right));
    let term = Rf_protect(AllocTerm());
    let nw = NWORDS.get();
    let term_p = INTEGER(term);
    // Bitwise or of all terms on right
    let mut r = right;
    while r != R_NilValue() {
        for i in 0..nw {
            term_p[i] |= INTEGER(CAR(r))[i as usize];
        }
        r = CDR(r);
    }
    // Bitwise or with each term on left
    let mut l = left;
    while l != R_NilValue() {
        for i in 0..nw {
            INTEGER(CAR(l))[i as usize] |= term_p[i];
        }
        l = CDR(l);
    }
    Rf_unprotect(3);
    TrimRepeats(left) // simplified
}

unsafe fn NestTerms(left: SEXP, right: SEXP) -> SEXP {
    let left = Rf_protect(EncodeVars(left));
    let right = Rf_protect(EncodeVars(right));
    let term = Rf_protect(AllocTerm());
    let nw = NWORDS.get();
    let term_p = INTEGER(term);
    // Bitwise or of all terms on left
    let mut l = left;
    while l != R_NilValue() {
        for i in 0..nw {
            term_p[i] |= INTEGER(CAR(l))[i as usize];
        }
        l = CDR(l);
    }
    // Bitwise or with each term on right
    let mut r = right;
    while r != R_NilValue() {
        for i in 0..nw {
            INTEGER(CAR(r))[i as usize] |= term_p[i];
        }
        r = CDR(r);
    }
    Rf_unprotect(3);
    TrimRepeats(left) // simplified
}

unsafe fn DeleteTerms(left: SEXP, right: SEXP) -> SEXP {
    let left = Rf_protect(EncodeVars(left));
    PARITY.set(!PARITY.get());
    let right = EncodeVars(right);
    PARITY.set(!PARITY.get());
    let mut r = right;
    while r != R_NilValue() {
        left = StripTerm(CAR(r), left);
        r = CDR(r);
    }
    Rf_unprotect(2);
    left
}

unsafe fn listAppend(list: SEXP, s: SEXP) -> SEXP {
    if list == R_NilValue() {
        return s;
    }
    if s == R_NilValue() {
        return list;
    }
    let mut tail = list;
    while CDR(tail) != R_NilValue() {
        tail = CDR(tail);
    }
    SETCDR(tail, s);
    list
}
