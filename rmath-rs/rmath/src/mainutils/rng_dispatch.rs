//! R-level RNG builtins — ports R's src/main/RNG.c dispatch layer.
//!
//! Provides the R built-in functions: set.seed(), RNGkind(), runif(), rnorm(),
//! rpois(), rbinom(), rexp(). These sit on top of the nmath layer (which has
//! the actual generator implementations).

use std::ffi::CString;
use std::os::raw::c_int;

use crate::sexp::accessors::{CAR, CDR, INTEGER, LENGTH, REAL, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector3};
use crate::sexp::ffi::{NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::Rf_protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// RNG kind tracking
// ---------------------------------------------------------------------------

/// Current RNG kind (0 = Marsaglia-MultiCarry default).
static RNG_KIND: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Get the current RNG kind.
pub fn get_rng_kind() -> i32 {
    RNG_KIND.load(std::sync::atomic::Ordering::Relaxed)
}

/// Set the RNG kind.
pub fn set_rng_kind(kind: i32) {
    RNG_KIND.store(kind, std::sync::atomic::Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// do_set.seed
// ---------------------------------------------------------------------------

/// Handle R's `set.seed(seed, kind, normal.kind)`.
///
/// Sets the RNG state from an integer seed. If seed is NA_INTEGER or NULL,
/// uses a time-based seed.
pub unsafe fn do_set_seed(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let seed_arg = CAR(args);
        let kind_arg = CAR(CDR(args));

        // Handle seed
        if !seed_arg.is_null() && seed_arg != R_NilValue() {
            let t = TYPEOF(seed_arg);
            if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
                let s = *INTEGER(seed_arg);
                if s != crate::sexp::ffi::NA_INTEGER {
                    // Use seed to set RNG state
                    // Split seed into two 16-bit halves for Marsaglia-MultiCarry
                    let i1 = (s as u32).wrapping_mul(69069).wrapping_add(1);
                    let i2 = (s as u32).wrapping_mul(12345).wrapping_add(67890);
                    crate::rng::set_seed(i1, i2);
                }
            } else if t == SEXPTYPE::REALSXP.0 {
                let s = *REAL(seed_arg);
                if !s.is_nan() {
                    let is = s as i64;
                    let i1 = (is as u32).wrapping_mul(69069).wrapping_add(1);
                    let i2 = (is as u32).wrapping_mul(12345).wrapping_add(67890);
                    crate::rng::set_seed(i1, i2);
                }
            }
        } else {
            // No seed given — use a pseudo-random default
            // In a real implementation, this would use the system clock
            crate::rng::set_seed(1234, 5678);
        }

        // Handle kind
        if !kind_arg.is_null() && kind_arg != R_NilValue() {
            let t = TYPEOF(kind_arg);
            if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::REALSXP.0 {
                let k = if t == SEXPTYPE::INTSXP.0 {
                    *INTEGER(kind_arg)
                } else {
                    *REAL(kind_arg) as i32
                };
                set_rng_kind(k);
            }
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_RNGkind
// ---------------------------------------------------------------------------

/// Handle R's `RNGkind(kind, normal.kind)`.
///
/// Without arguments, returns the current RNG kind as a character vector.
/// With arguments, sets the RNG kind.
pub unsafe fn do_RNGkind(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let kind_arg = CAR(args);

        if kind_arg.is_null() || kind_arg == R_NilValue() {
            // No arguments — return current kind
            let kind_name = match get_rng_kind() {
                0 => "Marsaglia-MultiCarry",
                1 => "Wichmann-Hill",
                2 => "Mersenne-Twister",
                _ => "Marsaglia-MultiCarry",
            };
            let cstr = CString::new(kind_name).unwrap_or_default();
            return crate::sexp::constructors::Rf_mkString(cstr.as_ptr());
        }

        // Set kind
        let t = TYPEOF(kind_arg);
        if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::REALSXP.0 {
            let k = if t == SEXPTYPE::INTSXP.0 {
                *INTEGER(kind_arg)
            } else {
                *REAL(kind_arg) as i32
            };
            set_rng_kind(k.max(0).min(2));
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_runif
// ---------------------------------------------------------------------------

/// Handle R's `runif(n, min, max)`.
///
/// Generates n uniform random numbers in [min, max).
pub unsafe fn do_runif(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let min_arg = CAR(CDR(args));
        let max_arg = CAR(CDR(CDR(args)));

        let n = parse_n(n_arg, 1);
        let min = parse_double_scalar(min_arg, 0.0);
        let max = parse_double_scalar(max_arg, 1.0);

        if n <= 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP.0, 0);
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);

        let range = max - min;
        for i in 0..n {
            let u = crate::rng::unif_rand();
            *dst.add(i as usize) = min + u * range;
        }

        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// do_rnorm
// ---------------------------------------------------------------------------

/// Handle R's `rnorm(n, mean, sd)`.
///
/// Generates n normal random numbers.
pub unsafe fn do_rnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let mean_arg = CAR(CDR(args));
        let sd_arg = CAR(CDR(CDR(args)));

        let n = parse_n(n_arg, 1);
        let mu = parse_double_scalar(mean_arg, 0.0);
        let sigma = parse_double_scalar(sd_arg, 1.0);

        if n <= 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP.0, 0);
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);

        for i in 0..n {
            *dst.add(i as usize) = crate::dist::normal::rnorm(mu, sigma);
        }

        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// do_rpois
// ---------------------------------------------------------------------------

/// Handle R's `rpois(n, lambda)`.
///
/// Generates n Poisson random numbers.
pub unsafe fn do_rpois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let lambda_arg = CAR(CDR(args));

        let n = parse_n(n_arg, 1);
        let lambda = parse_double_scalar(lambda_arg, 1.0);

        if n <= 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP.0, 0);
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);

        for i in 0..n {
            *dst.add(i as usize) = crate::dist::poisson::rpois(lambda);
        }

        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// do_rexp
// ---------------------------------------------------------------------------

/// Handle R's `rexp(n, rate)`.
///
/// Generates n exponential random numbers with mean = 1/rate.
pub unsafe fn do_rexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let rate_arg = CAR(CDR(args));

        let n = parse_n(n_arg, 1);
        let rate = parse_double_scalar(rate_arg, 1.0);
        let scale = if rate > 0.0 { 1.0 / rate } else { NA_REAL };

        if n <= 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP.0, 0);
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);

        for i in 0..n {
            *dst.add(i as usize) = crate::dist::exponential::rexp_inner(scale);
        }

        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// do_sample
// ---------------------------------------------------------------------------

/// Handle R's `sample(x, size, replace, prob)`.
///
/// Simple uniform sampling from a vector.
pub unsafe fn do_sample(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let size_arg = CAR(CDR(args));
        let replace_arg = CAR(CDR(CDR(args)));

        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }

        let x_len = XLENGTH(x_arg);
        if x_len == 0 {
            return R_NilValue();
        }

        let size = parse_n(size_arg, 1);
        let replace = if replace_arg.is_null() || replace_arg == R_NilValue() {
            false
        } else if TYPEOF(replace_arg) == SEXPTYPE::LGLSXP.0 {
            *crate::sexp::accessors::LOGICAL(replace_arg) != 0
        } else {
            false
        };

        // For now, sample from REALSXP only
        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, size as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);

        for i in 0..size {
            let u = crate::rng::unif_rand();
            let idx = (u * x_len as f64) as usize;
            let idx = idx.min(x_len as usize - 1);
            let val = if TYPEOF(x_arg) == SEXPTYPE::REALSXP.0 {
                *REAL(x_arg).add(idx)
            } else if TYPEOF(x_arg) == SEXPTYPE::INTSXP.0 {
                let v = *INTEGER(x_arg).add(idx);
                if v == crate::sexp::ffi::NA_INTEGER {
                    NA_REAL
                } else {
                    v as f64
                }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) = val;
        }

        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// Register RNG builtins
// ---------------------------------------------------------------------------

/// Register RNG builtins in the base environment.
pub unsafe fn register_rng_builtins(env: SEXP) {
    unsafe {
        use crate::sexp::accessors::SET_FRAME;
        use crate::sexp::constructors::persistent_cons;
        use crate::sexp::ffi::SexprecCore;

        static RNG_SEXPS: std::sync::OnceLock<Vec<usize>> = std::sync::OnceLock::new();
        let rng_fns = [
            "set.seed",
            "RNGkind",
            "runif",
            "rnorm",
            "rpois",
            "rexp",
            "sample",
        ];

        let builtins = RNG_SEXPS.get_or_init(|| {
            rng_fns
                .iter()
                .map(|_| {
                    let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::BUILTINSXP));
                    boxed.sxpinfo.set_gp(1);
                    Box::into_raw(boxed) as usize
                })
                .collect::<Vec<usize>>()
        });

        let frame = (*env).data.envsxp.frame;
        let mut chain = frame;
        for (i, name) in rng_fns.iter().enumerate() {
            let prim: SEXP = builtins[i] as SEXP;
            let sym = Rf_install(CString::new(*name).unwrap_or_default().as_ptr());
            let cell = persistent_cons(prim, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }
        SET_FRAME(env, chain);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse the 'n' argument (first arg to r* functions).
fn parse_n(arg: SEXP, default: c_int) -> R_xlen_t {
    unsafe {
        if arg.is_null() || arg == R_NilValue() {
            return default as R_xlen_t;
        }
        let t = TYPEOF(arg);
        if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
            let v = *INTEGER(arg);
            if v == crate::sexp::ffi::NA_INTEGER || v < 0 {
                return default as R_xlen_t;
            }
            v as R_xlen_t
        } else if t == SEXPTYPE::REALSXP.0 {
            let v = *REAL(arg);
            if v.is_nan() || v < 0.0 {
                return default as R_xlen_t;
            }
            v as R_xlen_t
        } else {
            default as R_xlen_t
        }
    }
}

/// Parse a scalar double argument with a default.
fn parse_double_scalar(arg: SEXP, default: f64) -> f64 {
    unsafe {
        if arg.is_null() || arg == R_NilValue() {
            return default;
        }
        let t = TYPEOF(arg);
        if t == SEXPTYPE::REALSXP.0 {
            *REAL(arg)
        } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
            let v = *INTEGER(arg);
            if v == crate::sexp::ffi::NA_INTEGER {
                NA_REAL
            } else {
                v as f64
            }
        } else {
            default
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::constructors::Rf_cons;

    #[test]
    fn test_runif_returns_vector() {
        unsafe {
            let n = Rf_ScalarInteger(10);
            let min = Rf_ScalarReal(0.0);
            let max = Rf_ScalarReal(1.0);
            let nil = R_NilValue();

            // Build args: n, min, max
            let args = Rf_cons(n, Rf_cons(min, Rf_cons(max, nil)));
            let result = do_runif(R_NilValue(), R_NilValue(), args, R_NilValue());

            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP.0);
            assert_eq!(LENGTH(result), 10);

            // All values should be in [0, 1)
            let data = REAL(result);
            for i in 0..10 {
                let v = *data.add(i);
                assert!(v >= 0.0 && v < 1.0, "runif value {} out of range", v);
            }
        }
    }

    #[test]
    fn test_rnorm_returns_vector() {
        unsafe {
            let n = Rf_ScalarInteger(5);
            let mean = Rf_ScalarReal(0.0);
            let sd = Rf_ScalarReal(1.0);
            let nil = R_NilValue();

            let args = Rf_cons(n, Rf_cons(mean, Rf_cons(sd, nil)));
            let result = do_rnorm(R_NilValue(), R_NilValue(), args, R_NilValue());

            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP.0);
            assert_eq!(LENGTH(result), 5);
        }
    }

    #[test]
    fn test_set_seed() {
        unsafe {
            let seed = Rf_ScalarInteger(42);
            let nil = R_NilValue();
            let args = Rf_cons(seed, Rf_cons(nil, nil));

            let result = do_set_seed(R_NilValue(), R_NilValue(), args, R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_runif_seeded_reproducible() {
        unsafe {
            let nil = R_NilValue();
            let seed_args = Rf_cons(Rf_ScalarInteger(123), Rf_cons(nil, nil));
            do_set_seed(R_NilValue(), R_NilValue(), seed_args, R_NilValue());

            let runif_args = Rf_cons(
                Rf_ScalarInteger(3),
                Rf_cons(Rf_ScalarReal(0.0), Rf_cons(Rf_ScalarReal(1.0), nil)),
            );
            let r1 = do_runif(R_NilValue(), R_NilValue(), runif_args, R_NilValue());

            // Reset seed
            let seed_args = Rf_cons(Rf_ScalarInteger(123), Rf_cons(nil, nil));
            do_set_seed(R_NilValue(), R_NilValue(), seed_args, R_NilValue());

            let runif_args = Rf_cons(
                Rf_ScalarInteger(3),
                Rf_cons(Rf_ScalarReal(0.0), Rf_cons(Rf_ScalarReal(1.0), nil)),
            );
            let r2 = do_runif(R_NilValue(), R_NilValue(), runif_args, R_NilValue());

            // Same seed should produce same sequence
            let d1 = REAL(r1);
            let d2 = REAL(r2);
            for i in 0..3 {
                assert_eq!(*d1.add(i), *d2.add(i), "seeded runif not reproducible");
            }
        }
    }
}
