#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(unused_imports)]

//! `[<-` and `[[<-` entry points — do_subassign, do_subassign_dflt,
//! do_subassign2, do_subassign2_dflt.

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::mainutils::subscript::{
    OneIndex, get1index, int_arraySubscript, makeSubscript, mat2indsub, strmat2intmat, vectorIndex,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::{allocList, allocSExp};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::support::{SET_S4_OBJECT, SET_TRUELENGTH, UNSET_S4_OBJECT};
use super::*;
use crate::mainutils::essentials::{elt_real_safe, elt_to_string};

// ---------------------------------------------------------------------------
fn atomic_rank(t: i32) -> i32 {
    match t {
        10 => 1,
        13 => 2,
        14 => 3,
        15 => 4,
        16 => 5,
        _ => 0,
    }
}
// Exported functions
unsafe fn data_frame_assign_cells(frame: SEXP, subs: SEXP, value: SEXP) -> Option<SEXP> {
    unsafe {
        let ncol = XLENGTH(frame);
        if ncol == 0 || value.is_null() || value == R_NilValue() {
            return None;
        }
        let nrows = {
            let col0 = VECTOR_ELT(frame, 0);
            if col0.is_null() { 0 } else { XLENGTH(col0) }
        };
        if nrows == 0 {
            return None;
        }
        let rows = subscript_positions(CAR(subs), nrows)?;
        let cols = column_positions(frame, CADR(subs))?;
        if cols.is_empty() || rows.is_empty() {
            return Some(frame);
        }
        let ylen = XLENGTH(value).max(1);
        for (cpos, &col_i) in cols.iter().enumerate() {
            let col = VECTOR_ELT(frame, col_i);
            if col.is_null() {
                continue;
            }
            let src = if TYPEOF(value) == SEXPTYPE::VECSXP && XLENGTH(value) == cols.len() as i64 {
                VECTOR_ELT(value, cpos as i64)
            } else {
                value
            };
            if rows.len() as i64 == nrows
                && rows.iter().enumerate().all(|(i, r)| *r == i as i64)
                && XLENGTH(src) == nrows
                && TYPEOF(col) == SEXPTYPE::REALSXP
                && TYPEOF(src) == SEXPTYPE::INTSXP
            {
                SET_VECTOR_ELT(frame, col_i, crate::mainutils::duplicate::Rf_duplicate(src));
                continue;
            }
            let updated = assign_column_rows(col, &rows, src, XLENGTH(src).max(1));
            SET_VECTOR_ELT(frame, col_i, updated);
        }
        Some(frame)
    }
}
unsafe fn data_frame_assign_matrix(frame: SEXP, index: SEXP, value: SEXP) -> Option<SEXP> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(index, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() || XLENGTH(dim) != 2 || *INTEGER(dim).add(1) != 2 {
            return None;
        }
        let n = XLENGTH(index) / 2;
        for k in 0..n {
            let row = crate::mainutils::essentials::elt_real_safe(index, k) as i64;
            let col = crate::mainutils::essentials::elt_real_safe(index, k + n) as i64;
            if row < 1 || col < 1 || col > XLENGTH(frame) {
                continue;
            }
            let column = VECTOR_ELT(frame, col - 1);
            let src = if XLENGTH(value) <= 1 { value } else { value };
            let updated = assign_column_rows(column, &[row - 1], src, XLENGTH(src).max(1));
            SET_VECTOR_ELT(frame, col - 1, updated);
        }
        Some(frame)
    }
}

unsafe fn subscript_positions(index: SEXP, n: i64) -> Option<Vec<i64>> {
    unsafe {
        if index.is_null() || index == R_NilValue() || index == crate::sexp::globals::R_MissingArg() {
            return Some((0..n).collect());
        }
        let mut out = Vec::new();
        if TYPEOF(index) == SEXPTYPE::INTSXP || TYPEOF(index) == SEXPTYPE::REALSXP {
            for i in 0..XLENGTH(index) {
                let v = crate::mainutils::essentials::elt_real_safe(index, i);
                if v.is_nan() {
                    continue;
                }
                let pos = v as i64;
                if pos >= 1 && pos <= n {
                    out.push(pos - 1);
                }
            }
            Some(out)
        } else {
            None
        }
    }
}

unsafe fn column_positions(frame: SEXP, index: SEXP) -> Option<Vec<i64>> {
    unsafe {
        if index.is_null() || index == R_NilValue() || index == crate::sexp::globals::R_MissingArg() {
            return Some((0..XLENGTH(frame)).collect());
        }
        if TYPEOF(index) == SEXPTYPE::STRSXP {
            let names = crate::sexp::attrib_core::getAttrib(
                frame,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            let mut out = Vec::new();
            for i in 0..XLENGTH(index) {
                let want = elt_to_string(index, i);
                let mut found = None;
                if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP {
                    for j in 0..XLENGTH(names) {
                        if elt_to_string(names, j) == want {
                            found = Some(j);
                            break;
                        }
                    }
                }
                found?;
                out.push(found.unwrap());
            }
            return Some(out);
        }
        subscript_positions(index, XLENGTH(frame))
    }
}

unsafe fn assign_column_rows(col: SEXP, rows: &[i64], value: SEXP, value_len: i64) -> SEXP {
    unsafe {
        let col = crate::mainutils::duplicate::shallow_duplicate(col);
        let levels = crate::sexp::attrib_core::getAttrib(
            col,
            crate::sexp::attrib_core::R_LevelsSymbol(),
        );
        let factor = TYPEOF(col) == SEXPTYPE::INTSXP
            && !levels.is_null()
            && TYPEOF(levels) == SEXPTYPE::STRSXP;
        for (k, &row) in rows.iter().enumerate() {
            if row < 0 || row >= XLENGTH(col) {
                continue;
            }
            let src_i = (k as i64) % value_len.max(1);
            if factor && TYPEOF(value) == SEXPTYPE::STRSXP {
                let text = elt_to_string(value, src_i);
                let mut code = NA_INTEGER;
                for j in 0..XLENGTH(levels) {
                    if elt_to_string(levels, j) == text {
                        code = (j + 1) as c_int;
                        break;
                    }
                }
                *INTEGER(col).add(row as usize) = code;
            } else if TYPEOF(col) == SEXPTYPE::INTSXP && TYPEOF(value) == SEXPTYPE::INTSXP {
                *INTEGER(col).add(row as usize) = *INTEGER(value).add(src_i as usize);
            } else if TYPEOF(col) == SEXPTYPE::REALSXP {
                *REAL(col).add(row as usize) = elt_real_safe(value, src_i);
            } else if TYPEOF(col) == SEXPTYPE::STRSXP && TYPEOF(value) == SEXPTYPE::STRSXP {
                SET_STRING_ELT(col, row, STRING_ELT(value, src_i));
            } else if TYPEOF(col) == SEXPTYPE::INTSXP {
                let v = elt_real_safe(value, src_i);
                *INTEGER(col).add(row as usize) = if v.is_nan() { NA_INTEGER } else { v as c_int };
            }
        }
        col
    }
}

// ---------------------------------------------------------------------------

/// Port of `do_subassign()` -- the `[<-` operator.
pub(crate) unsafe fn do_subassign(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[<-\0".as_ptr() as *const c_char,
            args,
            rho,
            &mut ans,
        ) != 0
        {
            return ans;
        }
        do_subassign_dflt(call, op, ans, rho)
    }
}

/// Port of `do_subassign_dflt()` -- default `[<-` implementation.
pub unsafe fn do_subassign_dflt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = op;
        let _args_guard = protect(args);

        let mut subs: SEXP = ptr::null_mut();
        let mut y: SEXP = ptr::null_mut();
        let mut x: SEXP = ptr::null_mut();
        let nsubs = SubAssignArgs(args, &mut x, &mut subs, &mut y);
        let _y_guard = protect(y);

        // Make sure LHS is duplicated if it matches one of the indices
        let mut s_iter = subs;
        while !isNull(s_iter) {
            let idx = CAR(s_iter);
            if x == idx {
                MARK_NOT_MUTABLE(x);
            }
            s_iter = CDR(s_iter);
        }

        // Duplicate if shared
        if MAYBE_SHARED(CAR(args)) {
            let dup = shallow_duplicate(CAR(args));
            SETCAR(args, dup);
            x = CAR(args);
        }

        let s4 = IS_S4_OBJECT(x);
        let mut oldtype = 0;

        if TYPEOF(x) == LISTSXP || TYPEOF(x) == LANGSXP {
            oldtype = TYPEOF(x);
            x = PairToVectorList(x);
        } else if XLENGTH(x) == 0 {
            if XLENGTH(y) == 0
                && (isNull(x)
                    || TYPEOF(x) == TYPEOF(y)
                    || TYPEOF(y) == VECSXP
                    || TYPEOF(y) == EXPRSXP)
            {
                return x;
            } else {
                let xr = atomic_rank(TYPEOF(x));
                let yr = atomic_rank(TYPEOF(y));
                if isNull(x) {
                    x = Rf_allocVector(TYPEOF(y), 0);
                } else if xr > 0 && yr > 0 && yr <= xr {
                    y = coerceVector(y, TYPEOF(x));
                } else {
                    x = coerceVector(x, TYPEOF(y));
                }
            }
        }
        let _x_guard = protect(x);
        if nsubs == 2
            && TYPEOF(x) == SEXPTYPE::VECSXP
            && crate::mainutils::essentials::sexp_has_class(x, "data.frame")
        {
            if let Some(updated) = data_frame_assign_cells(x, subs, y) {
                return updated;
            }
        }
        if nsubs == 1
            && TYPEOF(x) == SEXPTYPE::VECSXP
            && crate::mainutils::essentials::sexp_has_class(x, "data.frame")
        {
            if let Some(updated) = data_frame_assign_matrix(x, CAR(subs), y) {
                return updated;
            }
        }

        match TYPEOF(x) {
            LGLSXP | INTSXP | REALSXP | CPLXSXP | STRSXP | EXPRSXP | VECSXP | RAWSXP => {
                x = match nsubs {
                    0 => VectorAssign(
                        call,
                        rho,
                        x,
                        {
                            use crate::sexp::globals::R_MissingArg;
                            R_MissingArg()
                        },
                        y,
                    ),
                    1 => VectorAssign(call, rho, x, CAR(subs), y),
                    2 => MatrixAssign(call, rho, x, subs, y),
                    _ => ArrayAssign(call, rho, x, subs, y),
                };
            }
            _ => {
                errorNotSubsettable(x);
            }
        }

        if oldtype == LANGSXP && Rf_length(x) > 0 {
            x = VectorToPairList(x);
            SET_TYPEOF(x, LANGSXP);
        }

        SETTER_CLEAR_NAMED(x);
        if s4 != 0 {
            SET_S4_OBJECT(x);
        }
        x
    }
}

/// Port of `do_subassign2()` -- the `[[<-` operator.
pub(crate) unsafe fn do_subassign2(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[[<-\0".as_ptr() as *const c_char,
            args,
            rho,
            &mut ans,
        ) != 0
        {
            return ans;
        }
        do_subassign2_dflt(call, op, ans, rho)
    }
}

/// Port of `do_subassign2_dflt()` -- default `[[<-` implementation.
pub unsafe fn do_subassign2_dflt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = op;
        use crate::eval::attrib_core::R_DimNamesSymbol;
        use crate::eval::attrib_core::R_DimSymbol;
        use crate::sexp::globals::R_MissingArg;

        let _args_guard = protect(args);
        let mut dynamic_guards: Vec<crate::sexp::protect::ProtectGuard> = Vec::new();

        let mut subs: SEXP = ptr::null_mut();
        let mut y: SEXP = ptr::null_mut();
        let mut x: SEXP = ptr::null_mut();
        let nsubs = SubAssignArgs(args, &mut x, &mut subs, &mut y);
        let _initial_y_guard = protect(y);

        // Handle NULL left-hand sides
        if isNull(x) {
            if isNull(y) {
                return x;
            }
            x = Rf_allocVector3(VECSXP, 0);
        }

        // Ensure LHS is local
        if MAYBE_SHARED(x) {
            let dup = shallow_duplicate(x);
            SETCAR(args, dup);
            x = dup;
        }

        let s4 = IS_S4_OBJECT(x);
        let xOrig = if s4 != 0 && TYPEOF(x) == OBJSXP {
            let orig = x;
            x = R_getS4DataSlot(x, ANYSXP);
            orig
        } else {
            ptr::null_mut()
        };

        let _initial_x_guard = protect(x);
        let xtop = x;
        let mut xup = x;

        let dims = getAttrib(x, R_DimSymbol());
        let ndims = Rf_length(dims);

        let pdims: *const c_int = if ndims > 0 {
            if TYPEOF(dims) == INTSXP {
                INTEGER(dims)
            } else {
                ptr::null()
            }
        } else {
            ptr::null()
        };
        if nsubs == 2
            && crate::mainutils::subset::is_data_frame(x)
            && TYPEOF(x) == SEXPTYPE::VECSXP
        {
            let ncols = XLENGTH(x);
            let names = getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            let j = crate::mainutils::subscript::get1index(CADR(subs), names, ncols, 0, 0, call);
            if j < 0 || j >= ncols {
                return x;
            }
            let col = VECTOR_ELT(x, j);
            let nrows = XLENGTH(col);
            let i = crate::mainutils::subscript::get1index(CAR(subs), R_NilValue(), nrows, 0, 0, call);
            if i < 0 || i >= nrows {
                return x;
            }
            let v = if TYPEOF(y) == SEXPTYPE::REALSXP {
                *REAL(y)
            } else if TYPEOF(y) == SEXPTYPE::INTSXP {
                *INTEGER(y) as f64
            } else {
                return x;
            };
            match TYPEOF(col) {
                t if t == SEXPTYPE::REALSXP => *REAL(col).add(i as usize) = v,
                t if t == SEXPTYPE::INTSXP => *INTEGER(col).add(i as usize) = v as i32,
                _ => {}
            }
            return x;
        }

        // ENVSXP special case
        if TYPEOF(x) == ENVSXP {
            if nsubs != 1 || !isString(CAR(subs)) || Rf_length(CAR(subs)) != 1 {
                // Error: wrong args
                return x;
            }
            defineVar(installTrChar(STRING_ELT(CAR(subs), 0 as R_xlen_t)), y, x);
            if s4 != 0 && !isNull(xOrig) {
                return xOrig;
            }
            return x;
        }

        // Recursive indexing case
        let mut recursed = false;
        let mut thesub: SEXP = R_NilValue();
        let mut len = 0;
        let mut off: R_xlen_t = -1;
        let mut newname: SEXP = R_NilValue();

        if nsubs == 1 {
            thesub = CAR(subs);
            len = Rf_length(thesub);
            if len > 1 {
                xup = vectorIndex(x, thesub, 0, len - 2, TRUE, call, TRUE);
                dynamic_guards.push(protect(xup));
                off = OneIndex(
                    xup,
                    thesub,
                    XLENGTH(xup),
                    0,
                    &mut newname,
                    len - 2,
                    R_NilValue(),
                );
                x = vectorIndex(xup, thesub, len - 2, len - 1, TRUE, call, TRUE);
                dynamic_guards.push(protect(x));
                recursed = true;
            }
        }
        let _xup_guard = protect(xup);

        let mut stretch: R_xlen_t = 0;
        let mut offset: R_xlen_t = 0;

        if isVector(x) {
            if !isVectorList(x) && Rf_length(y) == 0 {
                // Error: replacement has length zero
                return xtop;
            }
            if !isVectorList(x) && Rf_length(y) > 1 {
                // Error: more elements supplied
                return xtop;
            }
            if nsubs == 0 || CAR(subs) == R_MissingArg() {
                errorMissingSubscript(x);
            }
            if nsubs == 1 {
                offset = OneIndex(
                    x,
                    thesub,
                    XLENGTH(x),
                    0,
                    &mut newname,
                    if recursed { len - 1 } else { -1 },
                    R_NilValue(),
                );
                if isVectorList(x) && isNull(y) {
                    let old_x = x;
                    x = DeleteOneVectorListItem(x, offset);
                    if recursed {
                        if isVectorList(xup) {
                            SET_VECTOR_ELT(xup, off, x);
                        } else {
                            let _x_guard = protect(x);
                            xup = SimpleListAssign(call, xup, subs, x, len - 2, false);
                        }
                    } else {
                        // xtop = x handled below
                    }
                    if s4 != 0 && !isNull(xOrig) {
                        SET_S4_OBJECT(xOrig);
                    }
                    return if recursed { xup } else { x };
                }
                if offset < 0 {
                    errorOutOfBoundsSEXP(x, -1, thesub);
                }
                if offset >= XLENGTH(x) {
                    stretch = offset + 1;
                }
            } else {
                if ndims != nsubs {
                    // Error: improper number of subscripts
                    return xtop;
                }
                let indx = Rf_allocVector3(INTSXP, ndims as R_xlen_t);
                let _indx_guard = protect(indx);
                let pindx = INTEGER(indx);
                let names = getAttrib(x, R_DimNamesSymbol());
                let mut subs_tmp = subs;
                for i in 0..ndims {
                    let sub_i = CAR(subs_tmp);
                    *pindx.add(i as usize) = get1index(
                        sub_i,
                        if isNull(names) {
                            R_NilValue()
                        } else {
                            VECTOR_ELT(names, i as R_xlen_t)
                        },
                        if pdims.is_null() {
                            0
                        } else {
                            *pdims.add(i as usize) as R_xlen_t
                        },
                        FALSE,
                        -1,
                        call,
                    ) as c_int;
                    subs_tmp = CDR(subs_tmp);
                    if *pindx.add(i as usize) < 0
                        || (pdims.is_null() || *pindx.add(i as usize) >= *pdims.add(i as usize))
                    {
                        errorOutOfBoundsSEXP(x, i, sub_i);
                    }
                }
                offset = 0;
                for i in (1..ndims).rev() {
                    offset = (offset + (*pindx.add(i as usize) as R_xlen_t))
                        * (if pdims.is_null() {
                            1
                        } else {
                            *pdims.add((i - 1) as usize) as R_xlen_t
                        });
                }
                offset += *pindx.add(0) as R_xlen_t;
            }
            // NAs are not allowed in subscripted assignments (upstream raises
            // this from OneIndex processing, before any typed assignment arm).
            if offset == NA_INTEGER as R_xlen_t {
                crate::mainutils::errors::Rf_error(
                    b"NAs are not allowed in subscripted assignments\0".as_ptr()
                        as *const core::ffi::c_char,
                );
            }
            let which = SubassignTypeFix(&mut x, &mut y, stretch, 2, call, rho);
            dynamic_guards.push(protect(x));
            dynamic_guards.push(protect(y));

            match which {
                1010 | 1310 | 1313 => {
                    *INTEGER(x).add(offset as usize) = INTEGER_ELT(y, 0);
                }
                1410 | 1413 => {
                    if INTEGER_ELT(y, 0) == NA_INTEGER {
                        *REAL(x).add(offset as usize) = NA_REAL;
                    } else {
                        *REAL(x).add(offset as usize) = INTEGER_ELT(y, 0) as c_double;
                    }
                }
                1414 => {
                    *REAL(x).add(offset as usize) = REAL(y).read();
                }
                1510 | 1513 => {
                    if INTEGER_ELT(y, 0) == NA_INTEGER {
                        (*COMPLEX(x).add(offset as usize)).r = NA_REAL;
                        (*COMPLEX(x).add(offset as usize)).i = 0.0;
                    } else {
                        (*COMPLEX(x).add(offset as usize)).r = INTEGER_ELT(y, 0) as c_double;
                        (*COMPLEX(x).add(offset as usize)).i = 0.0;
                    }
                }
                1514 => {
                    let ry = REAL_ELT(y, 0);
                    if ISNA(ry) {
                        (*COMPLEX(x).add(offset as usize)).r = NA_REAL;
                        (*COMPLEX(x).add(offset as usize)).i = 0.0;
                    } else {
                        (*COMPLEX(x).add(offset as usize)).r = ry;
                        (*COMPLEX(x).add(offset as usize)).i = 0.0;
                    }
                }
                1515 => {
                    *COMPLEX(x).add(offset as usize) = COMPLEX_ELT(y, 0);
                }
                1610 | 1613 | 1614 | 1615 | 1616 => {
                    SET_STRING_ELT(x, offset, STRING_ELT(y, 0));
                }
                1019 | 1319 | 1419 | 1519 | 1619 | 1901 | 1902 | 1904 | 1905 | 1906 | 1910
                | 1913 | 1914 | 1915 | 1916 | 1920 | 1921 | 1922 | 1923 | 1924 | 1925 | 1903
                | 1907 | 1908 | 1999 | 2001 | 2002 | 2006 | 2010 | 2013 | 2014 | 2015 | 2016
                | 2024 | 2025 | 1919 | 2020 => {
                    if crate::mainutils::subset::is_data_frame(xtop)
                        && crate::mainutils::objects::inherits2(y, c"AsIs".as_ptr()) == 0
                    {
                        y = crate::mainutils::duplicate::duplicate(y);
                        dynamic_guards.push(protect(y));
                        setAttrib(y, crate::eval::attrib_core::R_NamesSymbol(), R_NilValue());
                    }
                    if MAYBE_REFERENCED(y) && VECTOR_ELT(x, offset) != y {
                        y = R_FixupRHS(x, y);
                    }
                    SET_VECTOR_ELT(x, offset, y);
                }
                2424 => {
                    *RAW(x).add(offset as usize) = RAW_ELT(y, 0);
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for scalar subassignment
            }

            // If stretched, handle new name
            if stretch > 0 && !isNull(newname) {
                let names = getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
                if isNull(names) {
                    let names_new = Rf_allocVector3(STRSXP, Rf_length(x) as R_xlen_t);
                    let _names_new_guard = protect(names_new);
                    SET_STRING_ELT(names_new, offset, newname);
                    setAttrib(x, crate::eval::attrib_core::R_NamesSymbol(), names_new);
                } else {
                    SET_STRING_ELT(names, offset, newname);
                }
            }

            dynamic_guards.push(protect(x));
            dynamic_guards.push(protect(xup));
        } else if isPairList(x) {
            dynamic_guards.push(protect(y));
            if nsubs == 1 {
                if isNull(y) {
                    x = listRemove(x, CAR(subs), len - 1);
                } else {
                    x = SimpleListAssign(call, x, subs, y, len - 1, true);
                }
            } else {
                if ndims != nsubs {
                    // Error
                    return xtop;
                }
                let indx = Rf_allocVector3(INTSXP, ndims as R_xlen_t);
                let _indx_guard = protect(indx);
                let pindx = INTEGER(indx);
                let names = getAttrib(x, R_DimNamesSymbol());
                let mut subs_tmp = subs;
                for i in 0..ndims {
                    let sub_i = CAR(subs_tmp);
                    *pindx.add(i as usize) = get1index(
                        sub_i,
                        VECTOR_ELT(names, i as R_xlen_t),
                        if pdims.is_null() {
                            0
                        } else {
                            *pdims.add(i as usize) as R_xlen_t
                        },
                        FALSE,
                        -1,
                        call,
                    ) as c_int;
                    subs_tmp = CDR(subs_tmp);
                    if *pindx.add(i as usize) < 0
                        || (pdims.is_null() || *pindx.add(i as usize) >= *pdims.add(i as usize))
                    {
                        errorOutOfBoundsSEXP(x, i, sub_i);
                    }
                }
                offset = 0;
                for i in (1..ndims).rev() {
                    offset = (offset + (*pindx.add(i as usize) as R_xlen_t))
                        * (if pdims.is_null() {
                            1
                        } else {
                            *pdims.add((i - 1) as usize) as R_xlen_t
                        });
                }
                offset += *pindx.add(0) as R_xlen_t;
                let slot = nthcdr(x, offset as c_int);
                SETCAR(slot, y);
            }
            dynamic_guards.push(protect(x));
            dynamic_guards.push(protect(xup));
        } else {
            errorNotSubsettable(x);
        }

        let mut xtop = xtop;
        if recursed {
            if isVectorList(xup) {
                SET_VECTOR_ELT(xup, off, x);
            } else {
                xup = SimpleListAssign(call, xup, subs, x, len - 2, false);
            }
            if len == 2 {
                xtop = xup;
            }
        } else {
            xtop = x;
        }

        SETTER_CLEAR_NAMED(xtop);
        if s4 != 0 {
            SET_S4_OBJECT(xtop);
        }
        xtop
    }
}
