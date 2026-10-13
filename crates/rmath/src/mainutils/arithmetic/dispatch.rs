use super::*;

// ---- do_arith helpers ----

/// Arithmetic operation codes matching R's Arith.h
pub(crate) const OP_PLUS: c_int = 1;
pub(crate) const OP_MINUS: c_int = 2;
pub(crate) const OP_TIMES: c_int = 3;
pub(crate) const OP_DIV: c_int = 4;
pub(crate) const OP_POW: c_int = 5;
pub(crate) const OP_MOD: c_int = 6;
pub(crate) const OP_INTDIV: c_int = 7;

/// Integer binary operation with checked inputs and a rooted result of the
/// operation's actual kind (real for division and exponentiation).
#[forbid(unsafe_code)]
pub(crate) fn integer_binary_arith<'s>(
    factory: &SessionNodeFactory<'s>,
    code: c_int,
    left: &Sexp<'s>,
    right: &Sexp<'s>,
) -> SexpResult<Sexp<'s>> {
    factory.require_active()?;
    factory.link(left)?;
    factory.link(right)?;
    let n1 = left.len();
    let n2 = right.len();
    let length = if n1 == 0 || n2 == 0 { 0 } else { n1.max(n2) };
    let real_result = matches!(code, OP_DIV | OP_POW);
    let kind = if real_result {
        SEXPTYPE::REALSXP
    } else {
        SEXPTYPE::INTSXP
    };
    let result = factory.allocate(|arena| Some(arena.alloc_vector(kind, length)))?;
    let mut result = SexpMut::try_from_checked(result)?;
    let mut naflag = false;
    for index in 0..length {
        // Checked element reads copy scalars before the next provider call.
        // Neither input nor result exposes a buffer loan across reentry.
        let x1 = left.try_integer_elt(index % n1)?;
        let x2 = right.try_integer_elt(index % n2)?;
        if real_result {
            let value = if code == OP_DIV {
                R_integer_divide(x1, x2)
            } else if x1 == 1 || x2 == 0 {
                1.0
            } else if x1 == NA_INTEGER || x2 == NA_INTEGER {
                NA_REAL
            } else {
                R_pow(x1 as f64, x2 as f64)
            };
            result.try_set_real_elt(index, value)?;
        } else {
            let value = match code {
                OP_PLUS => R_integer_plus(x1, x2, &mut naflag),
                OP_MINUS => R_integer_minus(x1, x2, &mut naflag),
                OP_TIMES => R_integer_times(x1, x2, &mut naflag),
                OP_MOD => {
                    if x1 == NA_INTEGER || x2 == NA_INTEGER || x2 == 0 {
                        NA_INTEGER
                    } else if x1 >= 0 && x2 > 0 {
                        x1 % x2
                    } else {
                        myfmod(x1 as f64, x2 as f64) as c_int
                    }
                }
                OP_INTDIV => {
                    if x1 == NA_INTEGER || x2 == NA_INTEGER || x2 == 0 {
                        NA_INTEGER
                    } else {
                        libm::floor(x1 as f64 / x2 as f64) as c_int
                    }
                }
                _ => continue,
            };
            result.try_set_integer_elt(index, value)?;
        }
    }
    Ok(result.freeze())
}

/// Real (or mixed int/real) binary operation.
/// s1 and s2 can be REALSXP or INTSXP.
pub(crate) unsafe fn real_binary_arith(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1);
        let n2 = XLENGTH(s2);

        if n1 == 0 || n2 == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }

        let n = if n1 > n2 { n1 } else { n2 };
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _ans_guard = protect(ans);

        let da = REAL(ans);
        let is_real1 = TYPEOF(s1) == SEXPTYPE::REALSXP;
        let is_real2 = TYPEOF(s2) == SEXPTYPE::REALSXP;

        match code {
            OP_PLUS => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = x1 + x2;
                }
            }
            OP_MINUS => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = x1 - x2;
                }
            }
            OP_TIMES => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = x1 * x2;
                }
            }
            OP_DIV => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = x1 / x2;
                }
            }
            OP_POW => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = R_pow(x1, x2);
                }
            }
            OP_MOD => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = myfmod(x1, x2);
                }
            }
            OP_INTDIV => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = myfloor(x1, x2);
                }
            }
            _ => {} // intentionally unhandled: unsupported SEXPTYPE for real modulo
        }

        ans
    }
}

/// Complex binary arithmetic.
pub(crate) unsafe fn complex_binary_arith(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1);
        let n2 = XLENGTH(s2);
        let n = if n1 == 0 || n2 == 0 {
            0
        } else {
            if n1 > n2 { n1 } else { n2 }
        };

        let ans = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        let _ans_guard = protect(ans);

        // Coerce both to complex
        let s1 = coerce_to_complex(s1);
        let _s1_guard = protect(s1);
        let s2 = coerce_to_complex(s2);
        let _s2_guard = protect(s2);

        let da = COMPLEX(ans);
        let px1 = COMPLEX(s1);
        let px2 = COMPLEX(s2);

        for i in 0..(n as usize) {
            let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
            let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
            let a = *px1.add(i1);
            let b = *px2.add(i2);

            *da.add(i) = match code {
                OP_PLUS => Rcomplex {
                    r: a.r + b.r,
                    i: a.i + b.i,
                },
                OP_MINUS => Rcomplex {
                    r: a.r - b.r,
                    i: a.i - b.i,
                },
                OP_TIMES => Rcomplex {
                    r: a.r * b.r - a.i * b.i,
                    i: a.r * b.i + a.i * b.r,
                },
                OP_DIV => {
                    // (a.r + a.i*i) / (b.r + b.i*i)
                    let denom = b.r * b.r + b.i * b.i;
                    if denom == 0.0 {
                        Rcomplex {
                            r: f64::NAN,
                            i: f64::NAN,
                        }
                    } else {
                        Rcomplex {
                            r: (a.r * b.r + a.i * b.i) / denom,
                            i: (a.i * b.r - a.r * b.i) / denom,
                        }
                    }
                }
                OP_POW => {
                    // Complex power via polar form
                    let r = (a.r * a.r + a.i * a.i).sqrt();
                    let theta = libm::atan2(a.i, a.r);
                    if r == 0.0 {
                        Rcomplex { r: 0.0, i: 0.0 }
                    } else {
                        let log_r = r.ln();
                        let new_r = (log_r * b.r - theta * b.i).exp();
                        let new_theta = log_r * b.i + theta * b.r;
                        Rcomplex {
                            r: new_r * libm::cos(new_theta),
                            i: new_r * libm::sin(new_theta),
                        }
                    }
                }
                OP_MOD | OP_INTDIV => Rcomplex {
                    r: f64::NAN,
                    i: f64::NAN,
                },
                _ => Rcomplex { r: 0.0, i: 0.0 },
            };
        }

        ans
    }
}

/// Coerce a numeric SEXP to CPLXSXP.
pub(crate) unsafe fn coerce_to_complex(x: SEXP) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return x;
        }
        let n = XLENGTH(x);
        let y = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        let dst = COMPLEX(y);
        if t == SEXPTYPE::REALSXP {
            let src = REAL(x);
            for i in 0..(n as usize) {
                *dst.add(i) = Rcomplex {
                    r: *src.add(i),
                    i: 0.0,
                };
            }
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let src = INTEGER(x);
            for i in 0..(n as usize) {
                let v = *src.add(i);
                *dst.add(i) = Rcomplex {
                    r: if v == NA_INTEGER { NA_REAL } else { v as f64 },
                    i: 0.0,
                };
            }
        }
        y
    }
}

/// Unary arithmetic: +x and -x.
pub(crate) unsafe fn unary_arith(code: c_int, s1: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(s1);
        match TYPEOF(s1) {
            t if t == SEXPTYPE::REALSXP => match code {
                OP_PLUS => s1,
                OP_MINUS => {
                    let ans = if no_references(s1) {
                        s1
                    } else {
                        Rf_allocVector3(SEXPTYPE::REALSXP, n)
                    };
                    let _ans_guard = protect(ans);
                    let pa = REAL(ans);
                    let px = REAL(s1);
                    for i in 0..(n as usize) {
                        *pa.add(i) = -*px.add(i);
                    }
                    ans
                }
                _ => std::ptr::null_mut(),
            },
            t if t == SEXPTYPE::INTSXP => match code {
                OP_PLUS => s1,
                OP_MINUS => {
                    let ans = if no_references(s1) {
                        s1
                    } else {
                        Rf_allocVector3(SEXPTYPE::INTSXP, n)
                    };
                    let _ans_guard = protect(ans);
                    let pa = INTEGER(ans);
                    let px = INTEGER(s1);
                    for i in 0..(n as usize) {
                        let x = *px.add(i);
                        *pa.add(i) = if x == NA_INTEGER { NA_INTEGER } else { -x };
                    }
                    ans
                }
                _ => std::ptr::null_mut(),
            },
            t if t == SEXPTYPE::LGLSXP => {
                // Coerce to INTSXP for unary minus on logicals
                match code {
                    OP_PLUS | OP_MINUS => {
                        let ans = Rf_allocVector3(SEXPTYPE::INTSXP, n);
                        let _ans_guard = protect(ans);
                        let pa = INTEGER(ans);
                        let px = LOGICAL(s1);
                        for i in 0..(n as usize) {
                            let x = *px.add(i);
                            *pa.add(i) = if code == OP_MINUS && x != NA_INTEGER && x != 0 {
                                -x
                            } else {
                                x
                            };
                        }
                        ans
                    }
                    _ => std::ptr::null_mut(),
                }
            }
            t if t == SEXPTYPE::CPLXSXP => match code {
                OP_PLUS => s1,
                OP_MINUS => {
                    let ans = if no_references(s1) {
                        s1
                    } else {
                        Rf_allocVector3(SEXPTYPE::CPLXSXP, n)
                    };
                    let _ans_guard = protect(ans);
                    let pa = COMPLEX(ans);
                    let px = COMPLEX(s1);
                    for i in 0..(n as usize) {
                        let z = *px.add(i);
                        *pa.add(i) = Rcomplex { r: -z.r, i: -z.i };
                    }
                    ans
                }
                _ => std::ptr::null_mut(),
            },
            _ => std::ptr::null_mut(),
        }
    }
}

// ---- do_arith ----

/// General arithmetic dispatch: +, -, *, /, ^, %%, %/%.
///
/// Operation codes (from R's PRIMVAL):
///   1: + (OP_ADD), 2: - (OP_SUB), 3: * (OP_MUL),
///   4: / (OP_DIV), 5: ^ (OP_POW),
///   6: %% (OP_MOD), 7: %/% (OP_INTDIV)
pub unsafe fn do_arith(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let code = primval(op);

        let argc = Rf_length(args);

        let arg1 = CAR(args);

        if argc == 2 {
            let arg2 = CADR(args);

            // Retain both originals before either provider can detach them,
            // and both conversions through the second provider and kernel.
            let owner = crate::sexp::owner::OwnerToken::current().unwrap_or_else(|error| {
                crate::sexp::context::r_error(format!("arithmetic owner: {error}"))
            });
            let factory = SessionNodeFactory::new(owner);
            let original_arg1 = factory.wrap(arg1).unwrap_or_else(|error| {
                crate::sexp::context::r_error(format!("arithmetic argument: {error}"))
            });
            let original_arg2 = factory.wrap(arg2).unwrap_or_else(|error| {
                crate::sexp::context::r_error(format!("arithmetic argument: {error}"))
            });

            // Real scalars copy both values before allocating the rooted
            // output. Lazy providers remain lazy and no payload loan escapes.
            if original_arg1.typeof_() == SEXPTYPE::REALSXP
                && original_arg1.len() == 1
                && original_arg2.typeof_() == SEXPTYPE::REALSXP
                && original_arg2.len() == 1
            {
                let x1 = original_arg1.try_real_elt(0).unwrap_or_else(|error| {
                    crate::sexp::context::r_error(format!("real arithmetic input: {error}"))
                });
                let x2 = original_arg2.try_real_elt(0).unwrap_or_else(|error| {
                    crate::sexp::context::r_error(format!("real arithmetic input: {error}"))
                });
                let value = match code {
                    OP_PLUS => x1 + x2,
                    OP_MINUS => x1 - x2,
                    OP_TIMES => x1 * x2,
                    OP_DIV => x1 / x2,
                    OP_POW => R_pow(x1, x2),
                    OP_MOD => myfmod(x1, x2),
                    OP_INTDIV => myfloor(x1, x2),
                    _ => f64::NAN,
                };
                let result = factory
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, 1)))
                    .and_then(|result| {
                        let mut result = SexpMut::try_from_checked(result)?;
                        result.try_set_real_elt(0, value)?;
                        Ok(result.freeze())
                    })
                    .unwrap_or_else(|error| {
                        crate::sexp::context::r_error(format!("real arithmetic result: {error}"))
                    });
                return result.as_raw();
            }

            // Integer scalars use the same checked kernel as longer vectors.
            let arg1_owner =
                coerce_logical_to_int(&factory, original_arg1).unwrap_or_else(|error| {
                    crate::sexp::context::r_error(format!("logical coercion: {error}"))
                });
            let arg2_owner =
                coerce_logical_to_int(&factory, original_arg2).unwrap_or_else(|error| {
                    crate::sexp::context::r_error(format!("logical coercion: {error}"))
                });
            let arg1 = arg1_owner.as_raw();
            let arg2 = arg2_owner.as_raw();

            let t1 = TYPEOF(arg1);
            let t2 = TYPEOF(arg2);

            if t1 == SEXPTYPE::CPLXSXP || t2 == SEXPTYPE::CPLXSXP {
                complex_binary_arith(code, arg1, arg2)
            } else if t1 == SEXPTYPE::REALSXP || t2 == SEXPTYPE::REALSXP {
                // Ensure both are at least INTSXP or REALSXP for real_binary_arith
                let s1 = if t1 != SEXPTYPE::INTSXP {
                    coerce_to_real(arg1)
                } else {
                    arg1
                };
                let s2 = if t2 != SEXPTYPE::INTSXP {
                    coerce_to_real(arg2)
                } else {
                    arg2
                };
                let _s1_guard = protect(s1);
                let _s2_guard = protect(s2);
                let result = real_binary_arith(code, s1, s2);
                result
            } else if t1 == SEXPTYPE::INTSXP && t2 == SEXPTYPE::INTSXP {
                let result = integer_binary_arith(&factory, code, &arg1_owner, &arg2_owner)
                    .unwrap_or_else(|error| {
                        crate::sexp::context::r_error(format!("integer arithmetic: {error}"))
                    });
                result.as_raw()
            } else {
                std::ptr::null_mut()
            }
        } else if argc == 1 {
            unary_arith(code, arg1)
        } else {
            std::ptr::null_mut()
        }
    }
}

/// Copy lazy or shared logical values through checked element reads. Only an
/// unshared dense i32 payload can keep its allocation while changing kind.
pub(crate) fn coerce_logical_to_int<'s>(
    factory: &SessionNodeFactory<'s>,
    input: Sexp<'s>,
) -> SexpResult<Sexp<'s>> {
    factory.require_active()?;
    factory.link(&input)?;
    if input.typeof_() != SEXPTYPE::LGLSXP {
        return Ok(input);
    }
    if !crate::sexp::altrep::is_altrep(&input) && unsafe { no_references(input.as_raw()) } {
        let allocation = input.allocation()?;
        allocation
            .heap_identity()
            .retype_node(allocation, SEXPTYPE::INTSXP)
            .ok_or(crate::sexp::object::SexpError::StaleAllocation)?;
        return Ok(input);
    }
    let length = input.len();
    let output = factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, length)))?;
    let mut output = SexpMut::try_from_checked(output)?;
    for index in 0..length {
        // Providers may collect, allocate, reenter, or fail. No input/output
        // buffer loan crosses them; both canonical values remain rooted.
        let value = input.try_logical_elt(index)?;
        output.try_set_integer_elt(index, value)?;
    }
    Ok(output.freeze())
}
