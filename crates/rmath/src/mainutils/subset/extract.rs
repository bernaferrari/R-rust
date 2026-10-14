use super::*;

// ---------------------------------------------------------------------------
// ExtractSubset -- extract elements from x according to integer/real subscripts
// ---------------------------------------------------------------------------

/// Extract subset elements from `x` according to subscripts in `indx`.
///
/// This allocates the result and transfers elements from `x` to `result`
/// according to integer or real subscripts in `indx`.
#[allow(clippy::absurd_extreme_comparisons)]
pub unsafe fn ExtractSubset(x: SEXP, indx: SEXP, call: SEXP) -> SEXP {
    unsafe {
        if isNull(x) {
            return x;
        }

        // ALTREP fast path -- skip for now (no ALTREP in Rust port)
        let n = xlength(indx);
        let nx = xlength(x);
        let mode = TYPEOF(x);

        /* protect allocation in case _ELT operations need to allocate */
        let result = Rf_allocVector3(mode, n);
        let _result_guard = protect(result);

        if TYPEOF(indx) == SEXPTYPE::INTSXP {
            let pindx = INTEGER(indx);
            for i in 0..n {
                let ii = *pindx.add(i as usize);
                if ii > 0 && (ii as R_xlen_t) <= nx {
                    let ii_0 = (ii - 1) as usize;
                    match mode {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(i as usize) = LOGICAL_ELT(x, (ii - 1) as c_int);
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(i as usize) = INTEGER_ELT(x, (ii - 1) as c_int);
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(i as usize) = REAL_ELT(x, (ii - 1) as c_int);
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            let c = COMPLEX_ELT(x, (ii - 1) as c_int);
                            *COMPLEX(result).add(i as usize) = c;
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(result, i, STRING_ELT(x, (ii - 1) as R_xlen_t));
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, i, VECTOR_ELT_FIX_NAMED(x, ii_0 as R_xlen_t));
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(i as usize) = RAW_ELT(x, (ii - 1) as c_int);
                        }
                        _ => {
                            let _ = ii_0; // suppress warning
                            errorcallNotSubsettable(x, call);
                        }
                    }
                } else {
                    // Out of bounds or NA
                    match mode {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(i as usize) = NA_LOGICAL;
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(i as usize) = NA_INTEGER;
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(i as usize) = NA_REAL;
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            *COMPLEX(result).add(i as usize) = crate::sexp::ffi::Rcomplex {
                                r: NA_REAL,
                                i: NA_REAL,
                            };
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, i, R_NilValue());
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(i as usize) = 0;
                        }
                        _ => {} // intentionally unhandled: unsupported SEXPTYPE for NA fill in subset
                    }
                }
            }
        } else {
            // REAL subscript
            let pindx = REAL(indx);
            for i in 0..n {
                let di = *pindx.add(i as usize);
                let ii = (di - 1.0) as R_xlen_t;
                if R_FINITE(di) && ii >= 0 && ii < nx {
                    match mode {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(i as usize) = LOGICAL_ELT(x, ii as c_int);
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(i as usize) = INTEGER_ELT(x, ii as c_int);
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(i as usize) = REAL_ELT(x, ii as c_int);
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            let c = COMPLEX_ELT(x, ii as c_int);
                            *COMPLEX(result).add(i as usize) = c;
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(result, i, STRING_ELT(x, ii as R_xlen_t));
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, i, VECTOR_ELT_FIX_NAMED(x, ii));
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(i as usize) = RAW_ELT(x, ii as c_int);
                        }
                        _ => {
                            errorcallNotSubsettable(x, call);
                        }
                    }
                } else {
                    // Out of bounds or NA
                    match mode {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(i as usize) = NA_LOGICAL;
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(i as usize) = NA_INTEGER;
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(i as usize) = NA_REAL;
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            *COMPLEX(result).add(i as usize) = crate::sexp::ffi::Rcomplex {
                                r: NA_REAL,
                                i: NA_REAL,
                            };
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, i, R_NilValue());
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(i as usize) = 0;
                        }
                        _ => {} // intentionally unhandled: unsupported SEXPTYPE for NA fill in subset
                    }
                }
            }
        }

        result
    }
}

// ---------------------------------------------------------------------------
// VectorSubset -- single-index subsetting (including 1D arrays)
// ---------------------------------------------------------------------------

/// Subset a vector by a single index. Handles special matrix subscripting
/// when the index has the same number of columns as the dimension of x.
pub(crate) unsafe fn VectorSubset(x: SEXP, s: SEXP, call: SEXP) -> SEXP {
    unsafe {
        // If s is R_MissingArg, duplicate x
        // R_MissingArg has mark bit set; we check via a special approach
        // If s looks like a symbol with empty name, treat as missing.
        // The simpler approach: check if the symbol's printname is empty.
        let is_missing = s == R_MissingArg()
            || if isSymbol(s) {
                let pn = PRINTNAME(s);
                if isNull(pn) {
                    false
                } else {
                    let c = CHAR(pn);
                    !c.is_null() && *c == 0
                }
            } else {
                false
            };

        if is_missing {
            return crate::mainutils::duplicate::duplicate(x);
        }

        /* Protect s */
        let mut s = s;
        let mut s_guard = protect(s);

        /* Check for special matrix subscripting. */
        if !isNull(ATTRIB(s)) {
            let dim = getAttrib(x, sym_Dim());
            if isMatrix(s) && isArray(x) && ncols(s) == length_int(dim) {
                if isString(s) {
                    let dnames = GetArrayDimnames(x);
                    let _dnames_guard = protect(dnames);
                    s = strmat2intmat(s, dnames, call, x);
                    drop(s_guard);
                    s_guard = protect(s);
                }
                if TYPEOF(s) == SEXPTYPE::INTSXP || TYPEOF(s) == SEXPTYPE::REALSXP {
                    s = mat2indsub(dim, s, call, x);
                    drop(s_guard);
                    s_guard = protect(s);
                }
            }
        }

        /* Convert to a vector of integer subscripts in the range 1:length(x). */
        let mut stretch: R_xlen_t = 1;
        let indx = makeSubscript(x, s, &mut stretch, call);
        let _indx_guard = protect(indx);

        /* Allocate the result. */
        let mode = TYPEOF(x);
        let result = ExtractSubset(x, indx, call);
        let _result_guard = protect(result);
        if mode == SEXPTYPE::VECSXP || mode == SEXPTYPE::EXPRSXP {
            /* we do not duplicate the values when extracting the subset,
            so to be conservative mark the result as NAMED = NAMEDMAX */
            ENSURE_NAMEDMAX(result);
        }

        if !isNull(result) {
            /* Handle names attribute */
            let mut attrib = getAttrib(x, sym_Names());
            let mut has_names = !isNull(attrib);

            /* Here we might have an array. Use row names if 1D */
            if !has_names && isArray(x) {
                let dimnames = getAttrib(x, sym_DimNames());
                if !isNull(dimnames) && length_int(dimnames) == 1 {
                    attrib = GetRowNames(dimnames);
                    has_names = !isNull(attrib);
                }
            }

            if has_names {
                let _attrib_guard = protect(attrib);
                let nattrib = ExtractSubset(attrib, indx, call);
                let _nattrib_guard = protect(nattrib);
                setAttrib(result, sym_Names(), nattrib);
            }

            /* Handle srcref attribute */
            let srcref = getAttrib(x, sym_Srcref());
            if !isNull(srcref) && TYPEOF(srcref) == SEXPTYPE::VECSXP {
                let nattrib = ExtractSubset(srcref, indx, call);
                let _nattrib_guard = protect(nattrib);
                setAttrib(result, sym_Srcref(), nattrib);
            }
        }

        result
    }
}

// ---------------------------------------------------------------------------
// MatrixSubset -- 2D subsetting
// ---------------------------------------------------------------------------

/// Subset a matrix by row and column indices.
#[allow(clippy::absurd_extreme_comparisons)]
pub(crate) unsafe fn MatrixSubset(x: SEXP, s: SEXP, call: SEXP, drop: c_int) -> SEXP {
    unsafe {
        let nr = nrows(x);
        let nc = ncols(x);

        /* s is protected on entry */
        let dim = getAttrib(x, sym_Dim());
        let _dim_guard = protect(dim);

        /* Convert row and column subscripts to integer form */
        let sr = int_arraySubscript(0, CAR(s), dim, x, call);
        let _sr_guard = protect(sr);
        let sc = int_arraySubscript(1, CADR(s), dim, x, call);
        let _sc_guard = protect(sc);
        let nrs = LENGTH(sr);
        let ncs = LENGTH(sc);

        /* Overflow check */
        if (nrs as R_xlen_t) * (ncs as R_xlen_t) > R_XLEN_T_MAX {
            errorcall(call, "dimensions would exceed maximum size of array");
        }

        let psr = INTEGER(sr);
        let psc = INTEGER(sc);
        let mut result = Rf_allocVector3(TYPEOF(x), (nrs as R_xlen_t) * (ncs as R_xlen_t));
        let _result_guard = protect(result);

        let mut i: R_xlen_t;
        let mut j: R_xlen_t;
        let mut ii: R_xlen_t;
        let mut jj: R_xlen_t;
        let mut ij: R_xlen_t;
        let mut iijj: R_xlen_t;

        /* Matrix subset loop -- column-major order */
        for j in 0..(ncs as R_xlen_t) {
            jj = *psc.add(j as usize) as R_xlen_t;
            if jj != NA_INTEGER as R_xlen_t {
                if jj < 1 || jj > nc as R_xlen_t {
                    errorcallOutOfBounds(x, 1, jj, call);
                }
                jj -= 1;
            }
            for i_idx in 0..(nrs as R_xlen_t) {
                ii = *psr.add(i_idx as usize) as R_xlen_t;
                if ii != NA_INTEGER as R_xlen_t {
                    if ii < 1 || ii > nr as R_xlen_t {
                        errorcallOutOfBounds(x, 0, ii, call);
                    }
                    ii -= 1;
                }
                ij = i_idx + j * (nrs as R_xlen_t);
                if ii == NA_INTEGER as R_xlen_t || jj == NA_INTEGER as R_xlen_t {
                    /* NA code */
                    match TYPEOF(x) {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(ij as usize) = NA_LOGICAL;
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(ij as usize) = NA_INTEGER;
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(ij as usize) = NA_REAL;
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            *COMPLEX(result).add(ij as usize) = crate::sexp::ffi::Rcomplex {
                                r: NA_REAL,
                                i: NA_REAL,
                            };
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            // GNU ExtractSubset: NA subscripts yield NA_STRING,
                            // not NULL (`m[c(1,NA), ]` keeps is.na() TRUE).
                            SET_STRING_ELT(result, ij, crate::sexp::globals::R_NaString());
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, ij, R_NilValue());
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(ij as usize) = 0;
                        }
                        _ => {} // intentionally unhandled: unsupported SEXPTYPE for NA fill in subset
                    }
                } else {
                    iijj = ii + jj * (nr as R_xlen_t);
                    /* Standard code */
                    match TYPEOF(x) {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(result).add(ij as usize) = LOGICAL_ELT(x, iijj as c_int);
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(result).add(ij as usize) = INTEGER_ELT(x, iijj as c_int);
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(result).add(ij as usize) = REAL_ELT(x, iijj as c_int);
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            *COMPLEX(result).add(ij as usize) = COMPLEX_ELT(x, iijj as c_int);
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(result, ij, STRING_ELT(x, iijj as R_xlen_t));
                        }
                        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                            SET_VECTOR_ELT(result, ij, VECTOR_ELT_FIX_NAMED(x, iijj));
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(result).add(ij as usize) = RAW_ELT(x, iijj as c_int);
                        }
                        _ => {
                            errorcall(call, "matrix subscripting not handled for this type");
                        }
                    }
                }
            }
        }

        /* Set dim attribute */
        if nrs >= 0 && ncs >= 0 {
            let attr = Rf_allocVector(SEXPTYPE::INTSXP, 2);
            let _attr_guard = protect(attr);
            *INTEGER(attr).add(0) = nrs;
            *INTEGER(attr).add(1) = ncs;
            if !isNull(getAttrib(dim, sym_Names())) {
                setAttrib(attr, sym_Names(), getAttrib(dim, sym_Names()));
            }
            setAttrib(result, sym_Dim(), attr);
        }

        /* Transfer dimnames */
        if nrs >= 0 && ncs >= 0 {
            let dimnames = getAttrib(x, sym_DimNames());
            let dimnamesnames = getAttrib(dimnames, sym_Names());
            let _dimnamesnames_guard = protect(dimnamesnames);
            if !isNull(dimnames) {
                let newdimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
                let _newdimnames_guard = protect(newdimnames);
                if TYPEOF(dimnames) == SEXPTYPE::VECSXP {
                    SET_VECTOR_ELT(
                        newdimnames,
                        0,
                        if nrs == 0 {
                            R_NilValue()
                        } else {
                            ExtractSubset(VECTOR_ELT(dimnames, 0), sr, call)
                        },
                    );
                    SET_VECTOR_ELT(
                        newdimnames,
                        1,
                        if ncs == 0 {
                            R_NilValue()
                        } else {
                            ExtractSubset(VECTOR_ELT(dimnames, 1), sc, call)
                        },
                    );
                } else {
                    SET_VECTOR_ELT(
                        newdimnames,
                        0,
                        if nrs == 0 {
                            R_NilValue()
                        } else {
                            ExtractSubset(CAR(dimnames), sr, call)
                        },
                    );
                    SET_VECTOR_ELT(
                        newdimnames,
                        1,
                        if ncs == 0 {
                            R_NilValue()
                        } else {
                            ExtractSubset(CADR(dimnames), sc, call)
                        },
                    );
                }
                setAttrib(newdimnames, sym_Names(), dimnamesnames);
                setAttrib(result, sym_DimNames(), newdimnames);
            }
        }

        if drop != 0 {
            result = DropDims(result);
        }

        result
    }
}

// ---------------------------------------------------------------------------
// findASubIndex -- compute linear index for array subscripting
// ---------------------------------------------------------------------------

/// Compute a linear index for array subscripting given per-dimension sub-indices.
pub(crate) unsafe fn findASubIndex(
    k: R_xlen_t,
    subs: *const *const c_int,
    indx: *const c_int,
    _pxdims: *const c_int,
    offset: *const R_xlen_t,
    _call: SEXP,
) -> R_xlen_t {
    unsafe {
        let mut ii: R_xlen_t = 0;
        for j in 0..(k as usize) {
            let jj = *(*subs.add(j)).add(*indx.add(j) as usize);
            if jj == NA_INTEGER {
                return NA_INTEGER as R_xlen_t;
            }
            ii += (jj as R_xlen_t - 1) * *offset.add(j);
        }
        ii
    }
}

// ---------------------------------------------------------------------------
// ArraySubset -- N-D subsetting
// ---------------------------------------------------------------------------

/// Subset an array (N dimensions) by per-dimension indices.
pub(crate) unsafe fn ArraySubset(x: SEXP, s: SEXP, call: SEXP, drop: c_int) -> SEXP {
    unsafe {
        let mode = TYPEOF(x);
        let xdims = getAttrib(x, sym_Dim());
        let _xdims_guard = protect(xdims);
        let k = length_int(xdims);
        let pxdims = INTEGER(xdims);

        /* Use Vec for temporary storage (equivalent to R_alloc in C) */
        let mut subs: Vec<*const c_int> = Vec::with_capacity(k as usize);
        let mut indx: Vec<c_int> = vec![0; k as usize];
        let mut bound: Vec<c_int> = Vec::with_capacity(k as usize);
        let mut offset_arr: Vec<R_xlen_t> = vec![0; k as usize];

        /* Construct subscripts and compute bounds */
        let mut sub_guards = Vec::with_capacity(k as usize);
        let mut n: R_xlen_t = 1;
        let mut r = s;
        for i in 0..(k as usize) {
            let sub = int_arraySubscript(i as c_int, CAR(r), xdims, x, call);
            sub_guards.push(protect(sub));
            SETCAR(r, sub);
            bound.push(LENGTH(sub));
            subs.push(INTEGER(sub));
            n *= bound[i] as R_xlen_t;
            r = CDR(r);
        }

        /* Initialize index array */
        for i in 0..(k as usize) {
            indx[i] = 0;
        }

        /* Compute offsets (column-major) */
        offset_arr[0] = 1;
        for i in 1..(k as usize) {
            offset_arr[i] = offset_arr[i - 1] * (*pxdims.add(i - 1)) as R_xlen_t;
        }

        /* Range check on indices */
        for i in 0..(k as usize) {
            for j in 0..(bound[i] as usize) {
                let jj = *subs[i].add(j);
                if jj > *pxdims.add(i) || (jj < 1 && jj != NA_INTEGER) {
                    errorcallOutOfBounds(x, i as c_int, jj as R_xlen_t, call);
                }
            }
        }

        /* Transfer subset elements */
        let mut result = Rf_allocVector3(mode, n);
        let _result_guard = protect(result);

        for i in 0..n {
            let ii = findASubIndex(
                k as R_xlen_t,
                subs.as_ptr(),
                indx.as_ptr(),
                pxdims,
                offset_arr.as_ptr(),
                call,
            );

            if ii != NA_INTEGER as R_xlen_t {
                match mode {
                    t if t == SEXPTYPE::LGLSXP => {
                        *LOGICAL(result).add(i as usize) = LOGICAL_ELT(x, ii as c_int);
                    }
                    t if t == SEXPTYPE::INTSXP => {
                        *INTEGER(result).add(i as usize) = INTEGER_ELT(x, ii as c_int);
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        *REAL(result).add(i as usize) = REAL_ELT(x, ii as c_int);
                    }
                    t if t == SEXPTYPE::CPLXSXP => {
                        *COMPLEX(result).add(i as usize) = COMPLEX_ELT(x, ii as c_int);
                    }
                    t if t == SEXPTYPE::STRSXP => {
                        SET_STRING_ELT(result, i, STRING_ELT(x, ii as R_xlen_t));
                    }
                    t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                        SET_VECTOR_ELT(result, i, VECTOR_ELT_FIX_NAMED(x, ii));
                    }
                    t if t == SEXPTYPE::RAWSXP => {
                        *RAW(result).add(i as usize) = RAW_ELT(x, ii as c_int);
                    }
                    _ => {
                        errorcall(call, "array subscripting not handled for this type");
                    }
                }
            } else {
                match mode {
                    t if t == SEXPTYPE::LGLSXP => {
                        *LOGICAL(result).add(i as usize) = NA_LOGICAL;
                    }
                    t if t == SEXPTYPE::INTSXP => {
                        *INTEGER(result).add(i as usize) = NA_INTEGER;
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        *REAL(result).add(i as usize) = NA_REAL;
                    }
                    t if t == SEXPTYPE::CPLXSXP => {
                        *COMPLEX(result).add(i as usize) = crate::sexp::ffi::Rcomplex {
                            r: NA_REAL,
                            i: NA_REAL,
                        };
                    }
                    t if t == SEXPTYPE::STRSXP => {
                        SET_STRING_ELT(result, i, R_NilValue());
                    }
                    t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                        SET_VECTOR_ELT(result, i, R_NilValue());
                    }
                    t if t == SEXPTYPE::RAWSXP => {
                        *RAW(result).add(i as usize) = 0;
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for NA fill in subset
                }
            }

            /* Increment multi-dimensional index */
            if n > 1 {
                let mut j = 0;
                let kk = k as usize;
                loop {
                    indx[j] += 1;
                    if indx[j] < bound[j] {
                        break;
                    }
                    indx[j] = 0;
                    j = (j + 1) % kk;
                    if j == 0 {
                        break;
                    }
                }
            }
        }

        /* Set dim attribute */
        let new_dim = Rf_allocVector(SEXPTYPE::INTSXP, k);
        let _new_dim_guard = protect(new_dim);
        for i in 0..(k as usize) {
            *INTEGER(new_dim).add(i) = bound[i];
        }
        if !isNull(getAttrib(xdims, sym_Names())) {
            setAttrib(new_dim, sym_Names(), getAttrib(xdims, sym_Names()));
        }
        setAttrib(result, sym_Dim(), new_dim);

        /* Transfer dimnames */
        let dimnames = getAttrib(x, sym_DimNames());
        let dimnamesnames = getAttrib(dimnames, sym_Names());
        let _dimnamesnames_guard = protect(dimnamesnames);
        if !isNull(dimnames) {
            let new_xdims = Rf_allocVector3(SEXPTYPE::VECSXP, k as R_xlen_t);
            let _new_xdims_guard = protect(new_xdims);
            let mut jj = 0;
            if TYPEOF(dimnames) == SEXPTYPE::VECSXP {
                let mut rr = s;
                for i in 0..(k as usize) {
                    if bound[i] > 0 {
                        SET_VECTOR_ELT(
                            new_xdims,
                            jj as R_xlen_t,
                            ExtractSubset(VECTOR_ELT(dimnames, i as R_xlen_t), CAR(rr), call),
                        );
                    } else {
                        SET_VECTOR_ELT(new_xdims, jj as R_xlen_t, R_NilValue());
                    }
                    jj += 1;
                    rr = CDR(rr);
                }
            } else {
                let mut p = dimnames;
                let mut q = new_xdims;
                let mut rr = s;
                for i in 0..(k as usize) {
                    SETCAR(q, ExtractSubset(CAR(p), CAR(rr), call));
                    p = CDR(p);
                    q = CDR(q);
                    rr = CDR(rr);
                }
            }
            setAttrib(new_xdims, sym_Names(), dimnamesnames);
            setAttrib(result, sym_DimNames(), new_xdims);
        }

        if drop != 0 {
            result = DropDims(result);
        }
        let class = getAttrib(x, Rf_install(c"class".as_ptr()));
        if !isNull(class) && TYPEOF(class) == SEXPTYPE::STRSXP {
            let mut is_table = false;
            for i in 0..XLENGTH(class) {
                let elt = STRING_ELT(class, i);
                if !elt.is_null() {
                    let bytes = std::ffi::CStr::from_ptr(CHAR(elt)).to_bytes();
                    if bytes == b"table" {
                        is_table = true;
                        break;
                    }
                }
            }
            if is_table {
                setAttrib(result, Rf_install(c"class".as_ptr()), class);
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// scalarIndex -- fast path for scalar integer/real index
// ---------------------------------------------------------------------------

/// Fast-path extraction of a scalar integer or real index from SEXP.
/// Returns the 1-based index, or -1 for NA / non-scalar / attributed values.
pub(crate) unsafe fn scalarIndex(s: SEXP) -> R_xlen_t {
    unsafe {
        if isNull(s) {
            return -1;
        }
        if !isNull(ATTRIB(s)) {
            return -1;
        }

        let t = TYPEOF(s);
        if t == SEXPTYPE::INTSXP && IS_SCALAR(s, SEXPTYPE::INTSXP.into()) != 0 {
            let ival = SCALAR_IVAL(s);
            if ival != NA_INTEGER {
                ival as R_xlen_t
            } else {
                -1
            }
        } else if t == SEXPTYPE::REALSXP && IS_SCALAR(s, SEXPTYPE::REALSXP.into()) != 0 {
            let rval = SCALAR_DVAL(s);
            if R_FINITE(rval) { rval as R_xlen_t } else { -1 }
        } else {
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// ExtractArg -- find and remove a named argument from an argument list
// ---------------------------------------------------------------------------

/// Search for `arg_sym` in the argument list `args`. If found, remove it
/// from the list and return its value. Otherwise return R_NilValue.
pub(crate) unsafe fn ExtractArg(mut args: SEXP, arg_sym: SEXP) -> SEXP {
    unsafe {
        let mut prev_arg = args;
        let mut arg = args;

        while !isNull(arg) {
            if TAG(arg) == arg_sym {
                let val = CAR(arg);
                if arg == prev_arg {
                    /* found at head of args */
                    args = CDR(args);
                } else {
                    SETCDR(prev_arg, CDR(arg));
                }
                return val;
            }
            prev_arg = arg;
            arg = CDR(arg);
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// ExtractDropArg -- extract the `drop` argument from an argument list
// ---------------------------------------------------------------------------

/// Extract the `drop` argument (if present) from the argument list.
pub(crate) unsafe fn ExtractDropArg(el: SEXP, drop: *mut c_int) {
    unsafe {
        let val = asLogical(ExtractArg(el, sym_Drop()));
        if val == NA_LOGICAL {
            *drop = 1;
        } else {
            *drop = val;
        }
    }
}

// ---------------------------------------------------------------------------
// ExtractExactArg -- extract the `exact` argument from an argument list
// ---------------------------------------------------------------------------

/// Extract the `exact` argument. Returns 0 (not exact), 1 (exact), or -1 (NA).
pub(crate) unsafe fn ExtractExactArg(args: SEXP) -> c_int {
    unsafe {
        let argval = ExtractArg(args, sym_Exact());
        if isNull(argval) {
            return 1;
        } /* Default is true as from R 2.7.0 */
        let exact = asLogical(argval);
        if exact == NA_LOGICAL { -1 } else { exact }
    }
}

// ---------------------------------------------------------------------------
// R_DispatchOrEvalSP -- fast-path dispatch or eval for [ / [[ / $
// ---------------------------------------------------------------------------

/// Version of DispatchOrEval for `[`, `[[`, and `$` that speeds up simple cases.
/// Returns TRUE if dispatch succeeded (answer in *ans), FALSE if fall-through
/// with evaluated args in *ans. Port of R's `R_DispatchOrEvalSP` in subset.c.
pub(crate) unsafe fn R_DispatchOrEvalSP(
    call: SEXP,
    op: SEXP,
    generic: *const c_char,
    args: SEXP,
    rho: SEXP,
    ans: *mut SEXP,
) -> c_int {
    unsafe {
        use crate::eval::dispatch::{DispatchOrEval, evalListKeepMissing};
        use crate::eval::eval::Rf_eval;
        use crate::sexp::memory_ext::R_mkEVPROMISE;
        use crate::sexp::object::SessionNodeFactory;
        use crate::sexp::symbol::R_DotsSymbol;

        let factory = SessionNodeFactory::new(
            crate::sexp::owner::OwnerToken::current()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
        );
        let args_owner = if args.is_null() {
            factory.nil()
        } else {
            factory
                .wrap(args)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        };
        let rho_owner = if rho.is_null() {
            factory.nil()
        } else {
            factory
                .wrap(rho)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        };
        let mut args_work = args_owner.clone();

        if !args_owner.is_nil() && CAR(args) != R_DotsSymbol() {
            let expression = args_owner
                .try_car()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let tail = args_owner
                .try_cdr()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let x = factory
                .wrap(Rf_eval(expression.as_raw(), rho_owner.as_raw()))
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if !isObject(x.as_raw()) {
                let rest = evalListKeepMissing(tail, rho_owner.clone());
                if !ans.is_null() {
                    let nil = factory.nil();
                    let evaluated = factory
                        .allocate(|arena| Some(arena.cons(x.as_raw(), rest.as_raw(), nil.as_raw())))
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    *ans = evaluated.as_raw();
                }
                return 0;
            }
            let promise = factory
                .wrap(R_mkEVPROMISE(expression.as_raw(), x.as_raw()))
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let nil = factory.nil();
            args_work = factory
                .allocate(|arena| Some(arena.cons(promise.as_raw(), tail.as_raw(), nil.as_raw())))
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }

        // The owned prefix retains its promise and evaluated value throughout
        // method lookup, fallback argument evaluation and method application.
        DispatchOrEval(
            call,
            op,
            generic,
            args_work.as_raw(),
            rho_owner.as_raw(),
            ans,
            0,
            0,
        )
    }
}
