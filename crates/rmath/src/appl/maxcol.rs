// Ported from R's appl/maxcol.c
//
// Find maximum column: designed for probabilities.
// Uses reservoir sampling to break ties at random.
//
// Original (by permission) from MASS/MASS.c by W. N. Venables and B. D. Ripley

use crate::rng::unif_rand;
use crate::utils::*;
use libm::*;

const RELTOL: f64 = 1e-5;

/// Find the column with the maximum value in each row of a matrix.
///
/// Uses reservoir sampling to break ties randomly when ties_meth == 1.
///
/// # Arguments
/// * `matrix` - matrix stored column-major (Fortran style), nr rows x nc columns
/// * `nr` - number of rows
/// * `nc` - number of columns
/// * `maxes` - output array of length nr (1-based column indices)
/// * `ties_meth` - tie-breaking method: 1=random, 2=first, 3=last
pub unsafe fn R_max_col(
    matrix: *const f64,
    nr: std::os::raw::c_int,
    nc: std::os::raw::c_int,
    maxes: *mut std::os::raw::c_int,
    ties_meth: std::os::raw::c_int,
) {
    unsafe {
        let nr = nr as usize;
        let nc = nc as usize;
        let matrix = std::slice::from_raw_parts(matrix, nr * nc);
        let maxes = std::slice::from_raw_parts_mut(maxes, nr);
        let do_rand = ties_meth == 1;
        let mut used_random = false;

        for r in 0..nr {
            // first check row for any NAs and find the largest abs(entry)
            let mut large = 0.0_f64;
            let mut isna = true;
            let mut c = 0;
            while c < nc {
                let a = matrix[r + c * nr];
                if a.is_nan() {
                    isna = true;
                    break;
                } else if isna {
                    isna = false;
                }
                if !a.is_finite() {
                    c += 1;
                    continue;
                }
                if do_rand {
                    large = fmax2(large, fabs(a));
                }
                c += 1;
            }
            if isna {
                maxes[r] = i32::MIN; // NA_INTEGER
                continue;
            }

            let mut m: i32 = 0;
            let mut a = matrix[r];
            if do_rand {
                let tol = RELTOL * large;
                let mut ntie: i32 = 1;
                for c in 1..nc {
                    let b = matrix[r + c * nr];
                    if b > a + tol {
                        a = b;
                        m = c as i32;
                        ntie = 1;
                    } else if b >= a - tol {
                        ntie += 1;
                        if !used_random {
                            used_random = true;
                        }
                        if ntie as f64 * unif_rand() < 1.0 {
                            m = c as i32;
                        }
                    }
                }
            } else if ties_meth == 2 {
                // return the first max if there are ties
                for c in 1..nc {
                    let b = matrix[r + c * nr];
                    if a < b {
                        a = b;
                        m = c as i32;
                    }
                }
            } else if ties_meth == 3 {
                // return the last max
                for c in 1..nc {
                    let b = matrix[r + c * nr];
                    if a <= b {
                        a = b;
                        m = c as i32;
                    }
                }
            } else {
                eprintln!("invalid 'ties_meth' {{should not happen}}");
            }
            maxes[r] = m + 1;
        }
    }
}
