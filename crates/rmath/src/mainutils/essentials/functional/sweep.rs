//! sweep().

use super::*;

/// R's `sweep(x, MARGIN, STATS, FUN="-")` — sweep out statistics from array.
///
/// For each row/column, applies FUN(x, STATS) element-wise.
pub unsafe fn do_sweep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let margin_arg = CAR(CDR(args));
        let stats = CAR(CDR(CDR(args)));
        let fun_arg = CAR(CDR(CDR(CDR(args))));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        // Determine operation
        let op_str = if fun_arg.is_null() || fun_arg == R_NilValue() {
            "-".to_string()
        } else if TYPEOF(fun_arg) == SEXPTYPE::STRSXP {
            elt_to_string(fun_arg, 0)
        } else if TYPEOF(fun_arg) == SEXPTYPE::SYMSXP {
            let pname = crate::sexp::accessors::PRINTNAME(fun_arg);
            if !pname.is_null() {
                let s = crate::sexp::accessors::CHAR(pname);
                if !s.is_null() {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .unwrap_or("-")
                        .to_string()
                } else {
                    "-".to_string()
                }
            } else {
                "-".to_string()
            }
        } else {
            String::new()
        };

        let margin = if margin_arg.is_null() || margin_arg == R_NilValue() {
            1
        } else {
            real_or_default(margin_arg, 1.0) as i64
        };

        let t = TYPEOF(x);
        let n = XLENGTH(x);

        // Get dimensions
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));
        let (nrow, ncol) =
            if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2
            {
                (
                    *INTEGER(dim_attr) as R_xlen_t,
                    *INTEGER(dim_attr).add(1) as R_xlen_t,
                )
            } else {
                (n, 1)
            };

        // Division always yields doubles in GNU (`1L/4L` is 0.25), so the
        // swept vector is REALSXP for `/` even when `x` is integer.
        let result_t = if op_str == "/" {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE(t)
        };

        let result = Rf_allocVector3(result_t, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        // Fast path for common ops
        let apply_binary = |src_val: f64, stat_val: f64| -> f64 {
            match op_str.as_str() {
                "-" => src_val - stat_val,
                "+" => src_val + stat_val,
                "*" => src_val * stat_val,
                "/" => {
                    if stat_val != 0.0 {
                        src_val / stat_val
                    } else {
                        NA_REAL
                    }
                }
                _ => src_val - stat_val,
            }
        };

        if margin == 1 {
            // GNU sweep MARGIN=1: one STATS value per row.
            let stats_len = if stats.is_null() || stats == R_NilValue() {
                0
            } else {
                XLENGTH(stats)
            };
            for i in 0..nrow {
                for j in 0..ncol {
                    let src_idx = (j * nrow + i) as usize;
                    let stat_idx = if stats_len == 0 {
                        0
                    } else {
                        (i as usize) % (stats_len as usize)
                    };
                    let src_val = if t == SEXPTYPE::REALSXP {
                        *REAL(x).add(src_idx)
                    } else if t == SEXPTYPE::INTSXP {
                        let v = *INTEGER(x).add(src_idx);
                        if v == NA_INTEGER { NA_REAL } else { v as f64 }
                    } else {
                        NA_REAL
                    };
                    let stat_val = if stats.is_null() || stats == R_NilValue() {
                        0.0
                    } else {
                        elt_real_safe(stats, stat_idx as i64)
                    };
                    let res = apply_binary(src_val, stat_val);
                    if result_t == SEXPTYPE::REALSXP {
                        *REAL(result).add(src_idx) = res;
                    } else if result_t == SEXPTYPE::INTSXP {
                        *INTEGER(result).add(src_idx) = if res.is_nan() || res == NA_REAL {
                            NA_INTEGER
                        } else {
                            res as c_int
                        };
                    }
                }
            }
        } else if margin == 2 {
            // GNU sweep MARGIN=2: one STATS value per column.
            let stats_len = if stats.is_null() || stats == R_NilValue() {
                0
            } else {
                XLENGTH(stats)
            };
            for j in 0..ncol {
                for i in 0..nrow {
                    let src_idx = (j * nrow + i) as usize;
                    let stat_idx = if stats_len == 0 {
                        0
                    } else {
                        (j as usize) % (stats_len as usize)
                    };
                    let src_val = if t == SEXPTYPE::REALSXP {
                        *REAL(x).add(src_idx)
                    } else if t == SEXPTYPE::INTSXP {
                        let v = *INTEGER(x).add(src_idx);
                        if v == NA_INTEGER { NA_REAL } else { v as f64 }
                    } else {
                        NA_REAL
                    };
                    let stat_val = if stats.is_null() || stats == R_NilValue() {
                        0.0
                    } else {
                        elt_real_safe(stats, stat_idx as i64)
                    };
                    let res = apply_binary(src_val, stat_val);
                    if result_t == SEXPTYPE::REALSXP {
                        *REAL(result).add(src_idx) = res;
                    } else if result_t == SEXPTYPE::INTSXP {
                        *INTEGER(result).add(src_idx) = if res.is_nan() || res == NA_REAL {
                            NA_INTEGER
                        } else {
                            res as c_int
                        };
                    }
                }
            }
        }

        // Copy dim attribute if present
        if !dim_attr.is_null() {
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"dim".as_ptr()), dim_attr);
        }

        result
    }
}
