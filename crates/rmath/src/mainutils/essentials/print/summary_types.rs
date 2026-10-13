use super::*;

// ---------------------------------------------------------------------------
// S3 summary dispatch — type-specific summary methods
// ---------------------------------------------------------------------------

/// R's `summary.numeric(x)` — summary for numeric (double) vector.
pub unsafe fn do_summary_numeric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let mut vals: Vec<f64> = Vec::new();
        for i in 0..n {
            let v = *REAL(x).add(i as usize);
            if v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && !v.is_nan() {
                vals.push(v);
            }
        }
        let na_count = n as usize - vals.len();
        if vals.is_empty() {
            println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
            println!(
                "     NA      NA      NA      NA      NA      NA       {}",
                n
            );
        } else {
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let min_v = vals[0];
            let max_v = vals[vals.len() - 1];
            let mean_v: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
            let median_idx = vals.len() / 2;
            let median_v = if vals.len() % 2 == 1 {
                vals[median_idx]
            } else {
                (vals[median_idx - 1] + vals[median_idx]) / 2.0
            };
            let q1_idx = vals.len() / 4;
            let q3_idx = 3 * vals.len() / 4;
            println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
            println!(
                "{:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8}",
                min_v,
                vals[q1_idx],
                median_v,
                mean_v,
                vals[q3_idx],
                max_v,
                if na_count > 0 {
                    na_count.to_string()
                } else {
                    String::new()
                }
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `summary.integer(x)` — summary for integer vector.
pub unsafe fn do_summary_integer(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let mut vals: Vec<f64> = Vec::new();
        for i in 0..n {
            let v = *INTEGER(x).add(i as usize);
            if v != NA_INTEGER {
                vals.push(v as f64);
            }
        }
        let na_count = n as usize - vals.len();
        if vals.is_empty() {
            println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
            println!(
                "     NA      NA      NA      NA      NA      NA       {}",
                n
            );
        } else {
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let min_v = vals[0];
            let max_v = vals[vals.len() - 1];
            let mean_v: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
            let median_idx = vals.len() / 2;
            let median_v = if vals.len() % 2 == 1 {
                vals[median_idx]
            } else {
                (vals[median_idx - 1] + vals[median_idx]) / 2.0
            };
            let q1_idx = vals.len() / 4;
            let q3_idx = 3 * vals.len() / 4;
            println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
            println!(
                "{:>8.0} {:>8.0} {:>8.0} {:>8.2} {:>8.0} {:>8.0} {:>8}",
                min_v,
                vals[q1_idx],
                median_v,
                mean_v,
                vals[q3_idx],
                max_v,
                if na_count > 0 {
                    na_count.to_string()
                } else {
                    String::new()
                }
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `summary.logical(x)` — summary for logical vector (TRUE/FALSE/NA counts).
pub unsafe fn do_summary_logical(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let mut true_count = 0;
        let mut false_count = 0;
        let mut na_count = 0;
        for i in 0..n {
            let v = *LOGICAL(x).add(i as usize);
            if v == NA_INTEGER {
                na_count += 1;
            } else if v == TRUE {
                true_count += 1;
            } else {
                false_count += 1;
            }
        }
        println!("   Mode   FALSE    TRUE    NA's");
        println!(
            "logical {:>7} {:>7} {:>7}",
            false_count, true_count, na_count
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `summary.character(x)` — summary for character vector (class/length/NA).
pub unsafe fn do_summary_character(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let mut na_count = 0;
        for i in 0..n {
            let charsxp = STRING_ELT(x, i);
            if charsxp.is_null() {
                na_count += 1;
            } else {
                let s = CHAR(charsxp);
                if s.is_null() {
                    na_count += 1;
                }
            }
        }
        println!("   Length     Class      Mode");
        println!("{:>9} character character", n);
        if na_count > 0 {
            println!("   NA's: {}", na_count);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}
