
//! B-spline basis evaluation and spline value computation.
//!
//! Port of r-source/src/library/splines/src/splines.c
//!
//! Routines for manipulating B-splines, based on the pseudo-code in
//! Schumacher (Wiley, 1981) and the CMLIB library DBSPLINES.

use std::os::raw::{c_char, c_double, c_int, c_void};
use std::ptr;

use crate::attrib_core::setAttrib;
use crate::main::errors::Rf_error;
use crate::main::util_main::R_NaN;
use crate::sexp::accessors::{INTEGER, LENGTH, REAL};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{NA_INTEGER, SEXP, SEXPTYPE};
use crate::sexp::memory_ext::R_alloc;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// spl_struct — internal B-spline state
// ---------------------------------------------------------------------------

/// Internal B-spline evaluation state.
///
/// In C this was heap-allocated via R_alloc. Here we use a plain Rust struct.
struct SplStruct {
    order: c_int,           /* order of the spline */
    ordm1: c_int,           /* order - 1 (3 for cubic splines) */
    nknots: c_int,          /* number of knots */
    curs: c_int,            /* current position in knots vector */
    boundary: c_int,        /* must have knots[curs] <= x < knots[curs+1] except boundary */
    ldel: *mut c_double,    /* differences from knots on the left */
    rdel: *mut c_double,    /* differences from knots on the right */
    knots: *const c_double, /* knot vector */
    coeff: *const c_double, /* coefficients */
    a: *mut c_double,       /* scratch array */
}

// ---------------------------------------------------------------------------
// set_cursor
// ---------------------------------------------------------------------------

/// Set sp.curs to the index of the first knot position > x.
/// Special handling for x == sp.knots[sp.nknots - sp.order + 1].
unsafe fn set_cursor(sp: &mut SplStruct, x: c_double) -> c_int {
    sp.curs = -1; /* Wall */
    sp.boundary = 0;
    let knots = sp.knots;
    let nk = sp.nknots;
    let mut i: c_int = 0;
    while i < nk {
        if *knots.offset(i as isize) >= x {
            sp.curs = i;
        }
        if *knots.offset(i as isize) > x {
            break;
        }
        i += 1;
    }
    if sp.curs > sp.nknots - sp.order {
        let last_legit = sp.nknots - sp.order;
        if x == *knots.offset(last_legit as isize) {
            sp.boundary = 1;
            sp.curs = last_legit;
        }
    }
    sp.curs
}

// ---------------------------------------------------------------------------
// diff_table
// ---------------------------------------------------------------------------

/// Compute left and right difference tables.
unsafe fn diff_table(sp: &mut SplStruct, x: c_double, ndiff: c_int) {
    let curs = sp.curs as isize;
    let knots = sp.knots;
    let mut i: c_int = 0;
    while i < ndiff {
        *sp.rdel.offset(i as isize) = *knots.offset(curs + i as isize) - x;
        *sp.ldel.offset(i as isize) = x - *knots.offset(curs - (i as isize + 1));
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// basis_funcs — fast evaluation of basis functions
// ---------------------------------------------------------------------------

/// Fast evaluation of B-spline basis functions (no derivatives).
unsafe fn basis_funcs(sp: &mut SplStruct, x: c_double, b: *mut c_double) {
    diff_table(sp, x, sp.ordm1);
    *b = 1.0;
    let mut j: c_int = 1;
    while j <= sp.ordm1 {
        let mut saved: c_double = 0.0;
        let mut r: c_int = 0;
        while r < j {
            let den = *sp.rdel.offset(r as isize) + *sp.ldel.offset((j - 1 - r) as isize);
            if den != 0.0 {
                let term = *b.offset(r as isize) / den;
                *b.offset(r as isize) = saved + *sp.rdel.offset(r as isize) * term;
                saved = *sp.ldel.offset((j - 1 - r) as isize) * term;
            } else {
                if r != 0 || *sp.rdel.offset(r as isize) != 0.0 {
                    *b.offset(r as isize) = saved;
                }
                saved = 0.0;
            }
            r += 1;
        }
        *b.offset(j as isize) = saved;
        j += 1;
    }
}

// ---------------------------------------------------------------------------
// evaluate — "slow" evaluation of (derivative of) basis functions
// ---------------------------------------------------------------------------

/// Slow evaluation of (derivative of) basis functions.
unsafe fn evaluate(sp: &mut SplStruct, x: c_double, nder: c_int) -> c_double {
    let mut outer = sp.ordm1;

    if sp.boundary != 0 && nder == sp.ordm1 {
        /* value is arbitrary */
        return 0.0;
    }

    let mut nder = nder;
    while nder > 0 {
        let mut inner = outer;
        let mut apt = sp.a;
        let mut lpt = sp.knots.offset(sp.curs as isize - outer as isize);
        while inner > 0 {
            *apt =
                outer as c_double * (*apt.offset(1) - *apt) / (*lpt.offset(outer as isize) - *lpt);
            apt = apt.offset(1);
            lpt = lpt.offset(1);
            inner -= 1;
        }
        outer -= 1;
        nder -= 1;
    }

    diff_table(sp, x, outer);

    while outer > 0 {
        let mut apt = sp.a;
        let mut lpt = sp.ldel.offset(outer as isize - 1);
        let mut rpt = sp.rdel;
        let mut inner = outer + 1;
        while inner > 0 {
            *apt = (*apt.offset(1) * *lpt + *apt * *rpt) / (*rpt + *lpt);
            apt = apt.offset(1);
            lpt = lpt.offset(-1);
            rpt = rpt.offset(1);
            inner -= 1;
        }
        outer -= 1;
    }

    *sp.a
}

// ---------------------------------------------------------------------------
// spline_value — evaluate spline with given coefficients
// ---------------------------------------------------------------------------

/// Evaluate a spline with given coefficients at specified points.
///
/// Called from `predict.bSpline()` and `predict.pbSpline()`.
///
/// # Safety
/// All SEXP arguments must be valid R objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spline_value(
    knots: SEXP,
    coeff: SEXP,
    order: SEXP,
    x: SEXP,
    deriv: SEXP,
) -> SEXP {
    use crate::main::coerce::coerceVector;

    let nk = LENGTH(knots);
    let n = LENGTH(x);
    let ord = crate::main::coerce::asInteger(order);
    let der = crate::main::coerce::asInteger(deriv);

    if ord == NA_INTEGER || ord <= 0 {
        let msg = std::ffi::CString::new("'ord' must be a positive integer").expect("CString::new failed: contains null byte");
        Rf_error(msg.as_ptr());
    }

    // Allocate scratch arrays via R_alloc (transient memory)
    let ordm1 = ord - 1;
    let ldel = R_alloc(std::mem::size_of::<c_double>(), ordm1 as usize) as *mut c_double;
    let rdel = R_alloc(std::mem::size_of::<c_double>(), ordm1 as usize) as *mut c_double;
    let a = R_alloc(std::mem::size_of::<c_double>(), ord as usize) as *mut c_double;

    let kk = REAL(knots);
    let xx = REAL(x);
    let coeff_ptr = REAL(coeff);

    let val = Rf_allocVector(SEXPTYPE::REALSXP.0, n);
    Rf_protect(val);
    let rval = REAL(val);

    let mut i: c_int = 0;
    while i < n {
        let mut sp = SplStruct {
            order: ord,
            ordm1,
            nknots: nk,
            curs: 0,
            boundary: 0,
            ldel,
            rdel,
            knots: kk,
            coeff: coeff_ptr,
            a,
        };
        set_cursor(&mut sp, *xx.offset(i as isize));
        if sp.curs < sp.order || sp.curs > (nk - sp.order) {
            *rval.offset(i as isize) = R_NaN;
        } else {
            // Memcpy: copy coeff[curs - order .. curs] into a
            let src = coeff_ptr.offset((sp.curs - sp.order) as isize);
            ptr::copy_nonoverlapping(src, a, ord as usize);
            *rval.offset(i as isize) = evaluate(&mut sp, *xx.offset(i as isize), der);
        }
        i += 1;
    }

    Rf_unprotect(1);
    val
}

// ---------------------------------------------------------------------------
// spline_basis — evaluate B-spline basis functions
// ---------------------------------------------------------------------------

/// Evaluate the non-zero B-spline basis functions (or their derivatives) at xvals.
///
/// Called from `splineDesign()`.
///
/// # Safety
/// All SEXP arguments must be valid R objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spline_basis(knots: SEXP, order: SEXP, xvals: SEXP, derivs: SEXP) -> SEXP {
    use crate::main::array::allocMatrix;
    use crate::main::coerce::coerceVector;

    let nk = LENGTH(knots);
    let ord = crate::main::coerce::asInteger(order);
    let nx = LENGTH(xvals);
    let nd = LENGTH(derivs);

    let kk = REAL(knots);
    let xx = REAL(xvals);
    let ders = INTEGER(derivs);

    let ordm1 = ord - 1;

    // Allocate scratch arrays via R_alloc (transient memory)
    let ldel = R_alloc(std::mem::size_of::<c_double>(), ordm1 as usize) as *mut c_double;
    let rdel = R_alloc(std::mem::size_of::<c_double>(), ordm1 as usize) as *mut c_double;
    let a = R_alloc(std::mem::size_of::<c_double>(), ord as usize) as *mut c_double;

    let val = allocMatrix(SEXPTYPE::REALSXP.0, ord, nx);
    Rf_protect(val);
    let valM = REAL(val);

    let offsets = Rf_allocVector(SEXPTYPE::INTSXP.0, nx);
    Rf_protect(offsets);
    let ioff = INTEGER(offsets);

    let mut i: c_int = 0;
    while i < nx {
        let mut sp = SplStruct {
            order: ord,
            ordm1,
            nknots: nk,
            curs: 0,
            boundary: 0,
            ldel,
            rdel,
            knots: kk,
            coeff: ptr::null(),
            a,
        };
        set_cursor(&mut sp, *xx.offset(i as isize));
        // io is the knot-interval "number"
        let io = sp.curs - ord;
        *ioff.offset(i as isize) = io;

        let der_i = *ders.offset((i % nd) as isize);

        if io < 0 || io > nk {
            let mut j: c_int = 0;
            while j < ord {
                *valM.offset((i * ord + j) as isize) = R_NaN;
                j += 1;
            }
        } else if der_i > 0 {
            /* slow method for derivatives */
            if der_i >= ord {
                let msg = if nd == 1 {
                    std::ffi::CString::new(format!(
                        "derivs = {} >= ord = {}, but should be in {{0,..,ord-1}}",
                        der_i, ord
                    ))
                    .expect("unwrap on None/Err")
                } else {
                    std::ffi::CString::new(format!(
                        "derivs[{}] = {} >= ord = {}, but should be in {{0,..,ord-1}}",
                        i + 1,
                        der_i,
                        ord
                    ))
                    .expect("unwrap on None/Err")
                };
                Rf_error(msg.as_ptr());
            }
            let mut ii: c_int = 0;
            while ii < ord {
                // Zero the a array
                let mut j: c_int = 0;
                while j < ord {
                    *a.offset(j as isize) = 0.0;
                    j += 1;
                }
                *a.offset(ii as isize) = 1.0;
                *valM.offset((i * ord + ii) as isize) =
                    evaluate(&mut sp, *xx.offset(i as isize), der_i);
                ii += 1;
            }
        } else {
            /* fast method for value */
            basis_funcs(
                &mut sp,
                *xx.offset(i as isize),
                valM.offset((i * ord) as isize),
            );
        }
        i += 1;
    }

    // Set the "Offsets" attribute on the result
    let offsets_sym = Rf_install(b"Offsets\0".as_ptr() as *const c_char);
    setAttrib(val, offsets_sym, offsets);

    Rf_unprotect(2);
    val
}

// ---------------------------------------------------------------------------
// R_init_splines — package registration
// ---------------------------------------------------------------------------

/// Package initialization and routine registration for the splines package.
///
/// # Safety
/// dll must be a valid DllInfo pointer (or can be null in this Rust port).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_init_splines(_dll: *mut c_void) {
    // In the C code, this calls:
    //   R_registerRoutines(dll, NULL, R_CallDef, NULL, NULL);
    //   R_useDynamicSymbols(dll, FALSE);
    //   R_forceSymbols(dll, TRUE);
    // Since our functions are #[unsafe(no_mangle)] and directly linked, no registration needed.
}
