//! rbind/cbind: do_bind dispatch, dimnames helpers, cbind/rbind implementations (incl. data.frame row binding) — extracted verbatim from the former single-file module.
#![allow(unused_imports)]
use super::*;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::eval::attrib_core::{R_data_class, getAttrib, isObject, setAttrib};
use crate::eval::dispatch::DispatchOrEval;
use crate::eval::dispatch::promiseArgs;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rbyte, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// do_bind -- main dispatcher for cbind() / rbind()
// ---------------------------------------------------------------------------

/// R's `.Internal(cbind(...))` / `.Internal(rbind(...))`.
///
/// `PRIMVAL(op) == 1` selects `cbind`, otherwise `rbind`.
/// This is a special `.Internal`.
pub unsafe fn do_bind(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // The first argument is "deparse.level". Evaluate it.
        let deparse_level_val = crate::eval::eval::Rf_eval(CAR(args), env);
        let deparse_level: c_int = crate::mainutils::coerce::asInteger(deparse_level_val);
        let try_s4 = deparse_level >= 0;

        // Build promises for lazy evaluation and method dispatch.
        // This allows method implementations to use substitute() to get
        // the original expressions.
        let args = promiseArgs(args, env);
        let _args_guard = protect(args);

        // Determine the generic name from PRIMVAL(op).
        // PRIMVAL(op) == 1 for cbind, other for rbind.
        // Note: PRIMVAL is a stub that always returns 0 in this port.
        let generic_name = if !op.is_null() {
            let primval = crate::mainutils::relop::PRIMVAL(op);
            if primval == 1 { "cbind" } else { "rbind" }
        } else {
            "rbind"
        };

        let mut method: SEXP = R_NilValue();
        let mut any_s4 = false;
        let mut a = CDR(args);
        while !a.is_null() && a != R_NilValue() && method == R_NilValue() {
            let obj = crate::eval::eval::Rf_eval(CAR(a), env);
            let _obj_guard = protect(obj);
            if try_s4 && !any_s4 && crate::mainutils::objects::isS4(obj) != 0 {
                any_s4 = true;
            }
            if isObject(obj) != 0 {
                let classlist = R_data_class(obj);
                let _classlist_guard = protect(classlist);
                let classlen = Rf_length(classlist);
                for i in 0..classlen {
                    let class_str = translateChar(STRING_ELT(classlist, i as R_xlen_t));
                    let s = std::ffi::CStr::from_ptr(class_str).to_str().unwrap_or("");
                    let method_name = format!("{}.{}\0", generic_name, s);
                    let sym =
                        crate::sexp::symbol::Rf_install(method_name.as_ptr() as *const c_char);
                    let classmethod = crate::mainutils::objects::R_LookupMethod(
                        sym,
                        env,
                        env,
                        crate::sexp::globals::R_BaseEnv(),
                    );
                    if classmethod != crate::sexp::globals::R_UnboundValue() {
                        method = classmethod;
                        break;
                    }
                }
            }
            a = CDR(a);
        }

        if method != R_NilValue() {
            let _method_guard = protect(method);
            let dispatched_args = CDR(args);
            let ans = crate::eval::closure::applyClosure(
                call,
                method,
                dispatched_args,
                env,
                R_NilValue(),
                0,
            );
            return ans;
        }
        let args = CDR(args);
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };

        let mut t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = CAR(t);
            // PRVALUE: get the value of a promise, or the object itself
            let val = PRVALUE(u);
            let val = if val.is_null() || val == R_NilValue() {
                u
            } else {
                val
            };
            AnswerType(val, false, false, &mut data, call);
            t = CDR(t);
        }

        // zero-extent matrices shouldn't give NULL, but cbind(NULL) should:
        if data.ans_flags == 0 && data.ans_length == 0 {
            return R_NilValue();
        }

        let mode = ans_flags_to_mode(data.ans_flags);

        // Validate mode
        match mode.0 {
            NILSXP_I | LGLSXP_I | INTSXP_I | REALSXP_I | CPLXSXP_I | STRSXP_I | VECSXP_I
            | RAWSXP_I => {}
            _ => {
                let msg = std::ffi::CString::new(format!(
                    "cannot create a matrix from type '{}'",
                    std::ffi::CStr::from_ptr(type2char(mode.0))
                        .to_str()
                        .unwrap_or("unknown")
                ))
                .unwrap_or_default();
                std::panic::panic_any(crate::sexp::context::RError {
                    message: msg.into_string().unwrap_or_default(),
                });
            }
        }

        // Dispatch to cbind or rbind based on PRIMVAL(op)
        let primval = crate::mainutils::relop::PRIMVAL(op);
        let a = if primval == 1 {
            cbind(call, args, mode, env, deparse_level)
        } else {
            rbind(call, args, mode, env, deparse_level)
        };
        a
    }
}

// ---------------------------------------------------------------------------
// do_cbind -- convenience wrapper (public stub)
// ---------------------------------------------------------------------------

/// the `cbind` internal helper.
pub unsafe fn do_cbind(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { do_bind(call, op, args, env) }
}

// ---------------------------------------------------------------------------
// do_rbind -- convenience wrapper (public stub)
// ---------------------------------------------------------------------------

/// the `rbind` internal helper.
pub unsafe fn do_rbind(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe { do_bind(call, op, args, env) }
}

// ---------------------------------------------------------------------------
// SetRowNames -- set row names in a dimnames list
// ---------------------------------------------------------------------------

/// Assign `x` as the row names component of `dimnames`.
pub unsafe fn SetRowNames(dimnames: SEXP, x: SEXP) {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() {
            return;
        }
        let t = TYPEOF(dimnames);
        if t == VECSXP_I {
            SET_VECTOR_ELT(dimnames, 0, x);
        } else if t == LISTSXP_I {
            SETCAR(dimnames, x);
        }
    }
}

// ---------------------------------------------------------------------------
// SetColNames -- set column names in a dimnames list
// ---------------------------------------------------------------------------

/// Assign `x` as the column names component of `dimnames`.
pub unsafe fn SetColNames(dimnames: SEXP, x: SEXP) {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() {
            return;
        }
        let t = TYPEOF(dimnames);
        if t == VECSXP_I {
            SET_VECTOR_ELT(dimnames, 1, x);
        } else if t == LISTSXP_I {
            SETCADR(dimnames, x);
        }
    }
}

// ---------------------------------------------------------------------------
// GetRowNames / GetColNames -- local implementations
// ---------------------------------------------------------------------------

/// Retrieve row names from a dimnames attribute (vector-based list).
pub unsafe fn GetRowNames(dimnames: SEXP) -> SEXP {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(dimnames);
        if t == VECSXP_I {
            VECTOR_ELT(dimnames, 0)
        } else if t == LISTSXP_I {
            CAR(dimnames)
        } else {
            R_NilValue()
        }
    }
}

/// Retrieve column names from a dimnames attribute (vector-based list).
pub unsafe fn GetColNames(dimnames: SEXP) -> SEXP {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(dimnames);
        if t == VECSXP_I {
            VECTOR_ELT(dimnames, 1)
        } else if t == LISTSXP_I {
            CADR(dimnames)
        } else {
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// cbind -- column-binding implementation
// ---------------------------------------------------------------------------

/// Default `cbind` implementation.  Binds objects as columns, checking
/// conformability of matrix and vector arguments, and building dimnames.
pub unsafe fn cbind(
    call: SEXP,
    args: SEXP,
    mode: SEXPTYPE,
    rho: SEXP,
    deparse_level: c_int,
) -> SEXP {
    unsafe {
        let mut have_rnames: bool = false;
        let mut have_cnames: bool = false;
        let mut warned: bool = false;
        let mut nnames: c_int = 0;
        let mut mnames: c_int = 0;
        let mut rows: c_int = 0;
        let mut cols: c_int = 0;
        let mut mrows: c_int = -1;
        let mut lenmin: c_int = 0;

        let dim_sym = crate::eval::attrib_core::R_DimSymbol();
        let dimnames_sym = crate::eval::attrib_core::R_DimNamesSymbol();
        let names_sym = crate::eval::attrib_core::R_NamesSymbol();

        // check if we are in the zero-row case
        let mut t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = CAR(t);
            let u_val = PRVALUE(u);
            let u_val = if u_val.is_null() || u_val == R_NilValue() {
                u
            } else {
                u_val
            };
            let u_rows = if isMatrix(u_val) {
                nrows(u_val)
            } else {
                length(u_val)
            };
            if u_rows > 0 {
                lenmin = 1;
                break;
            }
            t = CDR(t);
        }

        // check conformability of matrix arguments
        let mut na: c_int = 0;
        t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = CAR(t);
            let u_val = PRVALUE(u);
            let u_val = if u_val.is_null() || u_val == R_NilValue() {
                u
            } else {
                u_val
            };
            let dims = getAttrib(u_val, dim_sym);
            if length(dims) == 2 {
                if mrows == -1 {
                    mrows = *INTEGER(dims);
                } else if mrows != *INTEGER(dims) {
                    let msg = std::ffi::CString::new(format!(
                        "number of rows of matrices must match (see arg {})",
                        na + 1
                    ))
                    .unwrap_or_default();
                    std::panic::panic_any(crate::sexp::context::RError {
                        message: msg.into_string().unwrap_or_default(),
                    });
                }
                cols += *INTEGER(dims).add(1);
            } else if length(u_val) >= lenmin {
                rows = imax2(rows, length(u_val));
                cols += 1;
            }
            na += 1;
            t = CDR(t);
        }
        if mrows != -1 {
            rows = mrows;
        }

        // Check conformability of vector arguments -- look for dimnames
        na = 0;
        t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = CAR(t);
            let u_val = PRVALUE(u);
            let u_val = if u_val.is_null() || u_val == R_NilValue() {
                u
            } else {
                u_val
            };
            let dims = getAttrib(u_val, dim_sym);
            if length(dims) == 2 {
                let dn = getAttrib(u_val, dimnames_sym);
                if length(dn) == 2 {
                    if !Rf_isNull(VECTOR_ELT(dn, 1)) != 0 {
                        have_cnames = true;
                    }
                    if !Rf_isNull(VECTOR_ELT(dn, 0)) != 0 {
                        mnames = mrows;
                    }
                }
            } else {
                let k = length(u_val);
                if !warned && k > 0 && (k > rows || rows % k != 0) {
                    warned = true;
                    // In R this is a warning, we just note it
                }
                let dn = getAttrib(u_val, names_sym);
                if k >= lenmin
                    && (!Rf_isNull(TAG(t)) != 0
                        || deparse_level == 2
                        || (deparse_level == 1 && isSymbol(CAR(t))))
                {
                    have_cnames = true;
                }
                nnames = imax2(nnames, length(dn));
            }
            na += 1;
            t = CDR(t);
        }
        if mnames != 0 || nnames == rows {
            have_rnames = true;
        }

        let result = allocMatrix(mode, rows, cols);
        let _result_guard = protect(result);
        let mut n: R_xlen_t = 0;

        // Fill the matrix values
        if mode == SEXPTYPE::STRSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::STRSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) { k } else { rows as R_xlen_t };
                    // Copy with recycling
                    for i in 0..idx {
                        let si = (i % k) as R_xlen_t;
                        SET_STRING_ELT(result, n + i, STRING_ELT(coerced, si));
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::VECSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                let umatrix = isMatrix(u);
                if umatrix || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::VECSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    if k > 0 {
                        let idx = if !umatrix { rows as R_xlen_t } else { k };
                        for i in 0..idx {
                            let si = (i % k) as R_xlen_t;
                            SET_VECTOR_ELT(result, n + i, lazy_duplicate(VECTOR_ELT(coerced, si)));
                        }
                    }
                    n += if !umatrix { rows as R_xlen_t } else { k };
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::CPLXSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::CPLXSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) { k } else { rows as R_xlen_t };
                    for i in 0..idx {
                        let si = (i % k) as R_xlen_t;
                        *COMPLEX(result).add((n + i) as usize) = *COMPLEX(coerced).add(si as usize);
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::RAWSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::RAWSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) { k } else { rows as R_xlen_t };
                    for i in 0..idx {
                        let si = (i % k) as R_xlen_t;
                        *RAW(result).add((n + i) as usize) = *RAW(coerced).add(si as usize);
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else {
            // NILSXP, REALSXP, INTSXP, LGLSXP
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let k = xlength(u);
                    let idx = if isMatrix(u) { k } else { rows as R_xlen_t };
                    let utype = TYPEOF(u);

                    if idx > 0 && utype <= INTSXP_I {
                        // NILSXP, LGLSXP, or INTSXP
                        if mode.0 <= INTSXP_I {
                            if k > 0 {
                                for i in 0..idx {
                                    let si = (i % k) as R_xlen_t;
                                    *INTEGER(result).add((n + i) as usize) =
                                        *INTEGER(u).add(si as usize);
                                }
                            }
                            n += idx;
                        } else {
                            // mode is REALSXP
                            if k > 0 {
                                for i in 0..idx {
                                    let si = (i % k) as R_xlen_t;
                                    let v = *INTEGER(u).add(si as usize);
                                    *REAL(result).add((n + i) as usize) = if v == NA_INTEGER {
                                        NA_REAL
                                    } else {
                                        v as c_double
                                    };
                                }
                            }
                            n += idx;
                        }
                    } else if utype == REALSXP_I {
                        for i in 0..idx {
                            let si = (i % k) as R_xlen_t;
                            *REAL(result).add((n + i) as usize) = *REAL(u).add(si as usize);
                        }
                        n += idx;
                    } else if utype == RAWSXP_I {
                        for i in 0..idx {
                            let si = (i % k) as R_xlen_t;
                            if mode == SEXPTYPE::LGLSXP {
                                *LOGICAL(result).add((n + i) as usize) =
                                    if *RAW(u).add(si as usize) != 0 {
                                        TRUE
                                    } else {
                                        FALSE
                                    };
                            } else if mode == SEXPTYPE::INTSXP {
                                *INTEGER(result).add((n + i) as usize) =
                                    *RAW(u).add(si as usize) as c_int;
                            } else if mode == SEXPTYPE::REALSXP {
                                *REAL(result).add((n + i) as usize) =
                                    *RAW(u).add(si as usize) as c_double;
                            }
                        }
                        n += idx;
                    }
                }
                t = CDR(t);
            }
        }

        // Adjustment of dimnames attributes
        if have_cnames || have_rnames {
            let dn = checked_allocVector(SEXPTYPE::VECSXP, 2);
            let _dn_guard = protect(dn);
            let nam: SEXP;
            if have_cnames {
                let nam_vec = checked_allocVector(SEXPTYPE::STRSXP, cols as R_xlen_t);
                SET_VECTOR_ELT(dn, 1, nam_vec);
                nam = nam_vec;
            } else {
                nam = R_NilValue();
            }
            let mut j: c_int = 0;

            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) {
                    let v = getAttrib(u, dimnames_sym);

                    if have_rnames
                        && GetRowNames(dn) == R_NilValue()
                        && GetRowNames(v) != R_NilValue()
                    {
                        SetRowNames(dn, lazy_duplicate(GetRowNames(v)));
                    }

                    let tnam = GetColNames(v);
                    if !Rf_isNull(tnam) != 0 {
                        for i in 0..length(tnam) {
                            SET_STRING_ELT(nam, j as R_xlen_t, STRING_ELT(tnam, i as R_xlen_t));
                            j += 1;
                        }
                    } else if have_cnames {
                        for _i in 0..ncols(u) {
                            SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                            j += 1;
                        }
                    }
                } else if length(u) >= lenmin {
                    let u_names = getAttrib(u, names_sym);

                    if have_rnames
                        && GetRowNames(dn) == R_NilValue()
                        && !Rf_isNull(u_names) != 0
                        && length(u_names) == rows
                    {
                        SetRowNames(dn, lazy_duplicate(u_names));
                    }

                    if !Rf_isNull(TAG(t)) != 0 {
                        SET_STRING_ELT(nam, j as R_xlen_t, PRINTNAME(TAG(t)));
                        j += 1;
                    } else if deparse_level == 1 && isSymbol(CAR(t)) {
                        SET_STRING_ELT(nam, j as R_xlen_t, PRINTNAME(CAR(t)));
                        j += 1;
                    } else if deparse_level == 2 {
                        // deparse1line not available; use blank
                        SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                        j += 1;
                    } else if have_cnames {
                        SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                        j += 1;
                    }
                }
                t = CDR(t);
            }

            setAttrib(result, dimnames_sym, dn);
        }

        result
    }
}

// ---------------------------------------------------------------------------
// rbind -- row-binding implementation
// ---------------------------------------------------------------------------

/// Default `rbind` implementation.  Binds objects as rows, checking
/// conformability of matrix and vector arguments, and building dimnames.
#[allow(clippy::if_same_then_else)]
pub unsafe fn rbind(
    call: SEXP,
    args: SEXP,
    mode: SEXPTYPE,
    rho: SEXP,
    deparse_level: c_int,
) -> SEXP {
    unsafe {
        let mut have_rnames: bool = false;
        let mut have_cnames: bool = false;
        let mut warned: bool = false;
        let mut nnames: c_int = 0;
        let mut mnames: c_int = 0;
        let mut rows: c_int = 0;
        let mut cols: c_int = 0;
        let mut mcols: c_int = -1;
        let mut lenmin: c_int = 0;

        let dim_sym = crate::eval::attrib_core::R_DimSymbol();
        let dimnames_sym = crate::eval::attrib_core::R_DimNamesSymbol();
        let names_sym = crate::eval::attrib_core::R_NamesSymbol();

        // check if we are in the zero-cols case
        let mut t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = resolve_promise(CAR(t));
            let u_cols = if isMatrix(u) { ncols(u) } else { length(u) };
            if u_cols > 0 {
                lenmin = 1;
                break;
            }
            t = CDR(t);
        }

        // check conformability of matrix arguments
        let mut na: c_int = 0;
        t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = resolve_promise(CAR(t));
            let dims = getAttrib(u, dim_sym);
            if length(dims) == 2 {
                if mcols == -1 {
                    mcols = *INTEGER(dims).add(1);
                } else if mcols != *INTEGER(dims).add(1) {
                    let msg = std::ffi::CString::new(format!(
                        "number of columns of matrices must match (see arg {})",
                        na + 1
                    ))
                    .unwrap_or_default();
                    std::panic::panic_any(crate::sexp::context::RError {
                        message: msg.into_string().unwrap_or_default(),
                    });
                }
                rows += *INTEGER(dims);
            } else if length(u) >= lenmin {
                cols = imax2(cols, length(u));
                rows += 1;
            }
            na += 1;
            t = CDR(t);
        }
        if mcols != -1 {
            cols = mcols;
        }

        // Check conformability of vector arguments -- look for dimnames
        na = 0;
        t = args;
        while !t.is_null() && t != R_NilValue() {
            let u = resolve_promise(CAR(t));
            let dims = getAttrib(u, dim_sym);
            if length(dims) == 2 {
                let dn = getAttrib(u, dimnames_sym);
                if length(dn) == 2 {
                    if !Rf_isNull(VECTOR_ELT(dn, 0)) != 0 {
                        have_rnames = true;
                    }
                    if !Rf_isNull(VECTOR_ELT(dn, 1)) != 0 {
                        mnames = mcols;
                    }
                }
            } else {
                let k = length(u);
                if !warned && k > 0 && (k > cols || cols % k != 0) {
                    warned = true;
                    // In R this is a warning
                }
                let _dn = getAttrib(u, names_sym);
                if k >= lenmin
                    && (!Rf_isNull(TAG(t)) != 0
                        || deparse_level == 2
                        || (deparse_level == 1 && isSymbol(CAR(t))))
                {
                    have_rnames = true;
                }
                nnames = imax2(nnames, length(_dn));
            }
            na += 1;
            t = CDR(t);
        }
        if mnames != 0 || nnames == cols {
            have_cnames = true;
        }

        let result = allocMatrix(mode, rows, cols);
        let _result_guard = protect(result);
        let mut n: R_xlen_t = 0;

        // Fill the matrix -- rbind fills row by row
        if mode == SEXPTYPE::STRSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::STRSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    // Fill matrix row by row with recycling
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            SET_STRING_ELT(result, dest_idx, STRING_ELT(coerced, si));
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::VECSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                let umatrix = isMatrix(u);
                let urows = if umatrix { nrows(u) } else { 1 };
                if umatrix || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::VECSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if umatrix {
                        urows as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            SET_VECTOR_ELT(
                                result,
                                dest_idx,
                                lazy_duplicate(VECTOR_ELT(coerced, si)),
                            );
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::RAWSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::RAWSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            *RAW(result).add(dest_idx as usize) = *RAW(coerced).add(si as usize);
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::CPLXSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::CPLXSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            *COMPLEX(result).add(dest_idx as usize) =
                                *COMPLEX(coerced).add(si as usize);
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::INTSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::INTSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            *INTEGER(result).add(dest_idx as usize) =
                                *INTEGER(coerced).add(si as usize);
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::LGLSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::LGLSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            *LOGICAL(result).add(dest_idx as usize) =
                                *LOGICAL(coerced).add(si as usize);
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else if mode == SEXPTYPE::REALSXP {
            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) || length(u) >= lenmin {
                    let coerced = coerceVector(u, SEXPTYPE::REALSXP);
                    let _coerced_guard = protect(coerced);
                    let k = xlength(coerced);
                    let idx = if isMatrix(u) {
                        nrows(u) as R_xlen_t
                    } else if k > 0 {
                        1
                    } else {
                        0
                    };
                    for r in 0..idx {
                        for c in 0..(cols as R_xlen_t) {
                            let si = ((r * cols as R_xlen_t + c) % k) as R_xlen_t;
                            let dest_idx = (n + r) * cols as R_xlen_t + c;
                            *REAL(result).add(dest_idx as usize) = *REAL(coerced).add(si as usize);
                        }
                    }
                    n += idx;
                }
                t = CDR(t);
            }
        } else {
            // NILSXP: do nothing
        }

        // Adjustment of dimnames attributes
        if have_rnames || have_cnames {
            let dn = checked_allocVector(SEXPTYPE::VECSXP, 2);
            let _dn_guard = protect(dn);
            let nam: SEXP;
            if have_rnames {
                let nam_vec = checked_allocVector(SEXPTYPE::STRSXP, rows as R_xlen_t);
                SET_VECTOR_ELT(dn, 0, nam_vec);
                nam = nam_vec;
            } else {
                nam = R_NilValue();
            }
            let mut j: c_int = 0;

            t = args;
            while !t.is_null() && t != R_NilValue() {
                let u = resolve_promise(CAR(t));
                if isMatrix(u) {
                    let v = getAttrib(u, dimnames_sym);

                    if have_cnames
                        && GetColNames(dn) == R_NilValue()
                        && GetColNames(v) != R_NilValue()
                    {
                        SetColNames(dn, lazy_duplicate(GetColNames(v)));
                    }

                    let tnam = GetRowNames(v);
                    if have_rnames {
                        if !Rf_isNull(tnam) != 0 {
                            for i in 0..length(tnam) {
                                SET_STRING_ELT(nam, j as R_xlen_t, STRING_ELT(tnam, i as R_xlen_t));
                                j += 1;
                            }
                        } else {
                            for _i in 0..nrows(u) {
                                SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                                j += 1;
                            }
                        }
                    }
                } else if length(u) >= lenmin {
                    let u_names = getAttrib(u, names_sym);

                    if have_cnames
                        && GetColNames(dn) == R_NilValue()
                        && !Rf_isNull(u_names) != 0
                        && length(u_names) == cols
                    {
                        SetColNames(dn, lazy_duplicate(u_names));
                    }

                    if !Rf_isNull(TAG(t)) != 0 {
                        SET_STRING_ELT(nam, j as R_xlen_t, PRINTNAME(TAG(t)));
                        j += 1;
                    } else if deparse_level == 1 && isSymbol(CAR(t)) {
                        SET_STRING_ELT(nam, j as R_xlen_t, PRINTNAME(CAR(t)));
                        j += 1;
                    } else if deparse_level == 2 {
                        SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                        j += 1;
                    } else if have_rnames {
                        SET_STRING_ELT(nam, j as R_xlen_t, R_BlankString());
                        j += 1;
                    }
                }
                t = CDR(t);
            }

            setAttrib(result, dimnames_sym, dn);
        }

        result
    }
}
