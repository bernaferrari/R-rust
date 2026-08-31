#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(unused_imports)]

//! `ArrayAssign` — `x[i, j, k, ...] <- y` for arrays.

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

use super::*;

// ---------------------------------------------------------------------------
// ArrayAssign
// ---------------------------------------------------------------------------

/// Port of `ArrayAssign()` -- handles `x[i,j,k,...] <- y` for arrays.
pub(crate) unsafe fn ArrayAssign(call: SEXP, rho: SEXP, x: SEXP, s: SEXP, y: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_DimSymbol;

        let mut k = 0i32;
        let dims = getAttrib(x, R_DimSymbol());
        let _dims_guard = protect(dims);
        if isNull(dims) || {
            k = LENGTH(dims);
            k != Rf_length(s)
        } {
            // Error: incorrect number of subscripts
            return x;
        }

        let ny = XLENGTH(y);
        let kk = k as usize;

        // Allocate stack arrays for subscripts, indices, bounds, offsets
        let mut subs: Vec<*const c_int> = Vec::with_capacity(kk);
        let mut indx: Vec<c_int> = vec![0; kk];
        let mut bound: Vec<c_int> = vec![0; kk];
        let mut offset: Vec<R_xlen_t> = vec![0; kk];

        // Expand the list of subscripts.
        let mut tmp = s;
        for i in 0..kk {
            SETCAR(tmp, int_arraySubscript(i as c_int, CAR(tmp), dims, x, call));
            tmp = CDR(tmp);
        }

        let mut n: R_xlen_t = 1;
        tmp = s;
        for i in 0..kk {
            indx[i] = 0;
            subs.push(INTEGER(CAR(tmp)));
            bound[i] = LENGTH(CAR(tmp));
            n *= bound[i] as R_xlen_t;
            tmp = CDR(tmp);
        }

        if n > 0 && ny == 0 {
            // Error: replacement has length zero
            return x;
        }

        offset[0] = 1;
        let pdims = INTEGER(dims);
        for i in 1..kk {
            offset[i] = offset[i - 1] * (*pdims.add(i - 1)) as R_xlen_t;
        }

        let mut x = x;
        let mut y = y;
        let which = SubassignTypeFix(&mut x, &mut y, 0, 1, call, rho);

        if n == 0 {
            return x;
        }

        let _x_guard = protect(x);
        let _y_guard = if x == y {
            y = shallow_duplicate(y);
            protect(y)
        } else {
            protect(y)
        };

        // Array assignment loop
        let mut iny: R_xlen_t = 0;
        for idx in 0..n {
            let mut ii: R_xlen_t = 0;
            let mut is_na = false;
            for j in 0..kk {
                let jj = *subs[j].add(indx[j] as usize);
                if jj == NA_INTEGER {
                    is_na = true;
                    break;
                } else {
                    ii += ((jj - 1) as R_xlen_t) * offset[j];
                }
            }

            if !is_na {
                match which {
                    1010 | 1310 | 1313 => {
                        *INTEGER(x).add(ii as usize) = INTEGER_ELT(y, iny as c_int);
                    }
                    1410 | 1413 => {
                        let iy = INTEGER_ELT(y, iny as c_int);
                        if iy == NA_INTEGER {
                            *REAL(x).add(ii as usize) = NA_REAL;
                        } else {
                            *REAL(x).add(ii as usize) = iy as c_double;
                        }
                    }
                    1414 => {
                        *REAL(x).add(ii as usize) = REAL_ELT(y, iny as c_int);
                    }
                    1510 | 1513 => {
                        let iy = INTEGER_ELT(y, iny as c_int);
                        if iy == NA_INTEGER {
                            (*COMPLEX(x).add(ii as usize)).r = NA_REAL;
                            (*COMPLEX(x).add(ii as usize)).i = 0.0;
                        } else {
                            (*COMPLEX(x).add(ii as usize)).r = iy as c_double;
                            (*COMPLEX(x).add(ii as usize)).i = 0.0;
                        }
                    }
                    1514 => {
                        let ry = REAL_ELT(y, iny as c_int);
                        if ISNA(ry) {
                            (*COMPLEX(x).add(ii as usize)).r = NA_REAL;
                            (*COMPLEX(x).add(ii as usize)).i = 0.0;
                        } else {
                            (*COMPLEX(x).add(ii as usize)).r = ry;
                            (*COMPLEX(x).add(ii as usize)).i = 0.0;
                        }
                    }
                    1515 => {
                        *COMPLEX(x).add(ii as usize) = COMPLEX_ELT(y, iny as c_int);
                    }
                    1610 | 1613 | 1614 | 1615 | 1616 => {
                        SET_STRING_ELT(x, ii, STRING_ELT(y, iny));
                    }
                    1919 => {
                        if (idx as R_xlen_t) >= ny {
                            ENSURE_NAMEDMAX(VECTOR_ELT(y, iny));
                        }
                        SET_VECTOR_ELT(x, ii, VECTOR_ELT_FIX_NAMED(y, iny));
                    }
                    2424 => {
                        *RAW(x).add(ii as usize) = RAW_ELT(y, iny as c_int);
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for subassignment
                }
            }

            iny += 1;
            if iny >= ny {
                iny = 0;
            }

            // Increment multi-dimensional index
            if n > 1 {
                let mut j = 0usize;
                loop {
                    indx[j] += 1;
                    if indx[j] < bound[j] {
                        break;
                    }
                    indx[j] = 0;
                    j += 1;
                    if j == kk {
                        j = 0;
                    }
                }
            }
        }

        x
    }
}
