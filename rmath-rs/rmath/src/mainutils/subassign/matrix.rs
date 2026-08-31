#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(unused_imports)]

//! `MatrixAssign` — `x[i, j] <- y` for matrices.

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
// MatrixAssign
// ---------------------------------------------------------------------------

/// Port of `MatrixAssign()` -- handles `x[i,j] <- y` for matrices.
pub(crate) unsafe fn MatrixAssign(call: SEXP, rho: SEXP, x: SEXP, s: SEXP, y: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_DimSymbol;

        if !isMatrix(x) {
            // Error: incorrect number of subscripts
            return x;
        }

        let nr = nrows(x);
        let ny = XLENGTH(y) as R_xlen_t;

        let dim = getAttrib(x, R_DimSymbol());
        SETCAR(s, int_arraySubscript(0, CAR(s), dim, x, call));
        SETCADR(s, int_arraySubscript(1, CADR(s), dim, x, call));
        let sr = CAR(s);
        let sc = CADR(s);
        let nrs = Rf_length(sr);
        let ncs = Rf_length(sc);

        let psc = INTEGER(sc);
        let psr = INTEGER(sr);

        let mut anyIdxNA = false;
        for i in 0..nrs {
            if *psr.add(i as usize) == NA_INTEGER {
                anyIdxNA = true;
                break;
            }
        }
        for i in 0..ncs {
            if *psc.add(i as usize) == NA_INTEGER {
                anyIdxNA = true;
                break;
            }
        }

        let n = (nrs as R_xlen_t) * (ncs as R_xlen_t);

        if n > 0 && ny == 0 {
            // Error: replacement has length zero
            return x;
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

        let NR = nr as R_xlen_t;
        let mut k: R_xlen_t = 0;

        if anyIdxNA {
            for j in 0..ncs {
                let jj = *psc.add(j as usize);
                if jj != NA_INTEGER {
                    let jj = (jj - 1) as R_xlen_t;
                    let offset = jj * NR;
                    for i in 0..nrs {
                        let ii = *psr.add(i as usize);
                        if ii != NA_INTEGER {
                            let ij = (ii as R_xlen_t - 1) + offset;
                            // Perform assignment based on type
                            match which {
                                1010 | 1310 | 1313 => {
                                    *INTEGER(x).add(ij as usize) = INTEGER_ELT(y, k as c_int);
                                }
                                1410 | 1413 => {
                                    let iy = INTEGER_ELT(y, k as c_int);
                                    if iy == NA_INTEGER {
                                        *REAL(x).add(ij as usize) = NA_REAL;
                                    } else {
                                        *REAL(x).add(ij as usize) = iy as c_double;
                                    }
                                }
                                1414 => {
                                    *REAL(x).add(ij as usize) = REAL_ELT(y, k as c_int);
                                }
                                1510 | 1513 => {
                                    let iy = INTEGER_ELT(y, k as c_int);
                                    if iy == NA_INTEGER {
                                        (*COMPLEX(x).add(ij as usize)).r = NA_REAL;
                                        (*COMPLEX(x).add(ij as usize)).i = 0.0;
                                    } else {
                                        (*COMPLEX(x).add(ij as usize)).r = iy as c_double;
                                        (*COMPLEX(x).add(ij as usize)).i = 0.0;
                                    }
                                }
                                1514 => {
                                    let ry = REAL_ELT(y, k as c_int);
                                    if ISNA(ry) {
                                        (*COMPLEX(x).add(ij as usize)).r = NA_REAL;
                                        (*COMPLEX(x).add(ij as usize)).i = 0.0;
                                    } else {
                                        (*COMPLEX(x).add(ij as usize)).r = ry;
                                        (*COMPLEX(x).add(ij as usize)).i = 0.0;
                                    }
                                }
                                1515 => {
                                    *COMPLEX(x).add(ij as usize) = COMPLEX_ELT(y, k as c_int);
                                }
                                1610 | 1613 | 1614 | 1615 | 1616 => {
                                    SET_STRING_ELT(x, ij, STRING_ELT(y, k));
                                }
                                1919 => {
                                    SET_VECTOR_ELT(x, ij, VECTOR_ELT_FIX_NAMED(y, k as R_xlen_t));
                                }
                                2424 => {
                                    *RAW(x).add(ij as usize) = RAW_ELT(y, k as c_int);
                                }
                                _ => {} // intentionally unhandled: unsupported SEXPTYPE for matrix subassignment
                            }
                            k += 1;
                            if k == ny {
                                k = 0;
                            }
                        }
                    }
                }
            }
        } else {
            for j in 0..ncs {
                let jj = (*psc.add(j as usize) - 1) as R_xlen_t;
                let offset = jj * NR;
                for i in 0..nrs {
                    let ii = *psr.add(i as usize);
                    let ij = (ii as R_xlen_t - 1) + offset;
                    match which {
                        1010 | 1310 | 1313 => {
                            *INTEGER(x).add(ij as usize) = INTEGER_ELT(y, k as c_int);
                        }
                        1410 | 1413 => {
                            let iy = INTEGER_ELT(y, k as c_int);
                            if iy == NA_INTEGER {
                                *REAL(x).add(ij as usize) = NA_REAL;
                            } else {
                                *REAL(x).add(ij as usize) = iy as c_double;
                            }
                        }
                        1414 => {
                            *REAL(x).add(ij as usize) = REAL_ELT(y, k as c_int);
                        }
                        1510 | 1513 => {
                            let iy = INTEGER_ELT(y, k as c_int);
                            if iy == NA_INTEGER {
                                (*COMPLEX(x).add(ij as usize)).r = NA_REAL;
                                (*COMPLEX(x).add(ij as usize)).i = 0.0;
                            } else {
                                (*COMPLEX(x).add(ij as usize)).r = iy as c_double;
                                (*COMPLEX(x).add(ij as usize)).i = 0.0;
                            }
                        }
                        1514 => {
                            let ry = REAL_ELT(y, k as c_int);
                            if ISNA(ry) {
                                (*COMPLEX(x).add(ij as usize)).r = NA_REAL;
                                (*COMPLEX(x).add(ij as usize)).i = 0.0;
                            } else {
                                (*COMPLEX(x).add(ij as usize)).r = ry;
                                (*COMPLEX(x).add(ij as usize)).i = 0.0;
                            }
                        }
                        1515 => {
                            *COMPLEX(x).add(ij as usize) = COMPLEX_ELT(y, k as c_int);
                        }
                        1610 | 1613 | 1614 | 1615 | 1616 => {
                            SET_STRING_ELT(x, ij, STRING_ELT(y, k));
                        }
                        1919 => {
                            if ny < (ncs as R_xlen_t) * (nrs as R_xlen_t) {
                                for ii in 0..ny {
                                    ENSURE_NAMEDMAX(VECTOR_ELT(y, ii));
                                }
                            }
                            SET_VECTOR_ELT(x, ij, VECTOR_ELT_FIX_NAMED(y, k as R_xlen_t));
                        }
                        2424 => {
                            *RAW(x).add(ij as usize) = RAW_ELT(y, k as c_int);
                        }
                        _ => {} // intentionally unhandled: unsupported SEXPTYPE for matrix subassignment
                    }
                    k += 1;
                    if k == ny {
                        k = 0;
                    }
                }
            }
        }

        x
    }
}
