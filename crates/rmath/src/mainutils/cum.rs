//! Checked payload drivers for R's cumulative sum and product.

use crate::eval::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};
use crate::mainutils::coerce::coerceVector;
use crate::mainutils::errors::Rf_warning_cstr;
use crate::mainutils::essentials::{active_owner, owner_sexp, sexp_ok};
use crate::sexp::accessors::CAR;
use crate::sexp::constructors::Rf_allocVector3;
use crate::sexp::ffi::{NA_INTEGER, NA_REAL, R_NA_BIT_PATTERN, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::{Sexp, SexpMut};

#[derive(Clone, Copy)]
pub(crate) enum Operation {
    Sum,
    Product,
}



pub(crate) fn cumulative(input: SEXP, operation: Operation) -> SEXP {
    if input.is_null() || input == R_NilValue() {
        return R_NilValue();
    }
    let owner = active_owner();
    let source = owner_sexp(owner, input);
    let source_type = source.typeof_();
    let kind = match (operation, source_type) {
        (_, SEXPTYPE::CPLXSXP) => SEXPTYPE::CPLXSXP,
        (Operation::Sum, SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP) => SEXPTYPE::INTSXP,
        _ => SEXPTYPE::REALSXP,
    };
    let input = if source_type == kind {
        source.clone()
    } else {
        owner_sexp(owner, coerceVector(source.as_raw(), kind.as_c_int()))
    };
    sexp_ok(owner.require_active());
    let result = owner_sexp(owner, Rf_allocVector3(kind, input.len()));
    let mut output = sexp_ok(SexpMut::try_from_checked(result.clone()));
    sexp_ok(owner.require_active());
    let symbol = R_NamesSymbol();
    setAttrib(result.as_raw(), symbol, getAttrib(input.as_raw(), symbol));
    sexp_ok(owner.require_active());
    let overflow = match kind {
        SEXPTYPE::INTSXP => accumulate_integer(&input, &mut output),
        SEXPTYPE::REALSXP => {
            accumulate_real(&input, &mut output, operation);
            false
        }
        SEXPTYPE::CPLXSXP => {
            accumulate_complex(&input, &result, &mut output, operation);
            false
        }
        _ => unreachable!(),
    };
    sexp_ok(owner.require_active());
    if overflow {
        Rf_warning_cstr(c"integer overflow in 'cumsum'; use 'cumsum(as.numeric(.))'");
        sexp_ok(owner.require_active());
    }
    result.as_raw()
}

fn accumulate_integer(input: &Sexp<'_>, output: &mut SexpMut<'_>) -> bool {
    let mut sum = 0_i32;
    for i in 0..input.len() {
        let value = sexp_ok(input.try_integer_elt(i));
        let next = if value == NA_INTEGER {
            None
        } else {
            sum.checked_add(value).filter(|&value| value != NA_INTEGER)
        };
        let Some(next) = next else {
            for j in i..input.len() {
                sexp_ok(output.try_set_integer_elt(j, NA_INTEGER));
            }
            return value != NA_INTEGER;
        };
        sum = next;
        sexp_ok(output.try_set_integer_elt(i, sum));
    }
    false
}

fn is_na(value: f64) -> bool {
    value.to_bits() == R_NA_BIT_PATTERN
}

fn accumulate_real(input: &Sexp<'_>, output: &mut SexpMut<'_>, operation: Operation) {
    let mut acc = match operation {
        Operation::Sum => 0.0,
        Operation::Product => 1.0,
    };
    let mut seen_na = false;
    for i in 0..input.len() {
        let value = sexp_ok(input.try_real_elt(i));
        seen_na |= is_na(value);
        acc = match operation {
            Operation::Sum => acc + value,
            Operation::Product => acc * value,
        };
        let value = if seen_na {
            NA_REAL
        } else if acc.is_nan() {
            f64::NAN
        } else {
            acc
        };
        sexp_ok(output.try_set_real_elt(i, value));
    }
}

fn accumulate_complex(
    input: &Sexp<'_>,
    result: &Sexp<'_>,
    output: &mut SexpMut<'_>,
    operation: Operation,
) {
    let n = input.len();
    let mut acc = match operation {
        Operation::Sum => Rcomplex { r: 0.0, i: 0.0 },
        Operation::Product => Rcomplex { r: 1.0, i: 0.0 },
    };
    let mut first_nan = n;
    let mut first_na = n;
    for i in 0..n {
        let value = sexp_ok(input.try_complex_elt(i));
        if value.r.is_nan() || value.i.is_nan() {
            first_nan = first_nan.min(i);
            if is_na(value.r) || is_na(value.i) {
                first_na = first_na.min(i);
            }
        }
        acc = match operation {
            Operation::Sum => Rcomplex {
                r: acc.r + value.r,
                i: acc.i + value.i,
            },
            Operation::Product => Rcomplex {
                r: value.r * acc.r - value.i * acc.i,
                i: value.r * acc.i + value.i * acc.r,
            },
        };
        sexp_ok(output.try_set_complex_elt(i, acc));
    }
    let (real_nan, imag_nan) = (acc.r.is_nan(), acc.i.is_nan());
    if !real_nan && !imag_nan {
        return;
    }
    // The final NaN components determine which lanes to normalize, even if
    // one became NaN later than the other. Do not call lazy inputs twice.
    for i in first_nan..n {
        let value = if i >= first_na { NA_REAL } else { f64::NAN };
        let z = if real_nan && imag_nan {
            Rcomplex { r: value, i: value }
        } else {
            let z = sexp_ok(result.try_complex_elt(i));
            Rcomplex {
                r: if real_nan { value } else { z.r },
                i: if imag_nan { value } else { z.i },
            }
        };
        sexp_ok(output.try_set_complex_elt(i, z));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::altrep::{
        AltrepBuilder, AltrepClass, AltrepContext, AltrepElement, is_materialized,
    };
    use crate::sexp::{ffi::R_xlen_t, object::SexpResult, session::RSession};
    use std::{cell::Cell, rc::Rc};

    struct CollectingReal(Rc<Cell<usize>>);

    impl AltrepClass for CollectingReal {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::REALSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
            Ok(3)
        }
        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            i: R_xlen_t,
        ) -> SexpResult<AltrepElement<'s>> {
            self.0.set(self.0.get() + 1);
            context.gc()?;
            Ok(AltrepElement::Real([2.0, 3.0, 5.0][i as usize]))
        }
    }

    #[test]
    fn cumulative_functions_keep_collecting_altrep_input_lazy() {
        let session = RSession::new_for_gc_tests();
        let reads = Rc::new(Cell::new(0));
        let class = session
            .register_altrep_class(
                "collecting-cumulative-real",
                CollectingReal(reads.clone()),
            )
            .unwrap();
        let input = AltrepBuilder::new(class).build().unwrap();
        session.with_active(|| {
            let args = crate::sexp::constructors::Rf_cons(input.as_raw(), R_NilValue());
            let args = session.sexp(args).unwrap();
            for (f, expected) in [
                (do_cumsum as fn(SEXP, SEXP, SEXP, SEXP) -> SEXP, [2.0, 5.0, 10.0]),
                (do_cumprod, [2.0, 6.0, 30.0]),
            ] {
                let nil = R_NilValue();
                let raw = f(nil, nil, args.as_raw(), nil);
                let result = session.sexp(raw).unwrap();
                for (i, expected) in expected.into_iter().enumerate() {
                    assert_eq!(result.try_real_elt(i as R_xlen_t), Ok(expected));
                }
                assert!(!is_materialized(&input));
            }
        });
        assert_eq!(reads.get(), 6);
    }
}
