//! Original base `.bincode` admission and the existing binary-binning kernel.

use crate::mainutils::essentials::base_error;
use crate::sexp::{
    accessors::{CADDDR, CADDR, CADR, CAR},
    constructors::Rf_allocVector3,
    ffi::{NA_INTEGER, SEXP, SEXPTYPE},
    object::{Sexp, SexpMut},
    owner::OwnerToken,
    protect::protect,
};

fn numeric_values(value: &Sexp<'_>) -> Vec<f64> {
    (0..value.len())
        .map(|index| value.try_real_elt(index))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| base_error(error.to_string()))
}

/// # Safety
/// Arguments are initialized projections of the caller's active original
/// runtime. The internal dispatcher owns the evaluated argument list.
pub(crate) unsafe fn do_bincode(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::relop::checkArity(op, args);
        let owner = OwnerToken::current().unwrap_or_else(|error| base_error(error.to_string()));
        let x = crate::mainutils::coerce::coerceVector(CAR(args), SEXPTYPE::REALSXP.into());
        let _x = protect(x);
        let breaks = crate::mainutils::coerce::coerceVector(CADR(args), SEXPTYPE::REALSXP.into());
        let _breaks = protect(breaks);
        let breaks = owner
            .sexp(breaks)
            .unwrap_or_else(|error| base_error(error.to_string()));
        let nb = i32::try_from(breaks.len())
            .ok()
            .filter(|&length| length != 0)
            .unwrap_or_else(|| base_error("long vector 'breaks' is not supported"));
        if nb < 2 {
            base_error("invalid 'breaks' argument");
        }
        let right = crate::mainutils::coerce::asLogical(CADDR(args));
        if right == NA_INTEGER {
            base_error("invalid 'right' argument");
        }
        let include = crate::mainutils::coerce::asLogical(CADDDR(args));
        if include == NA_INTEGER {
            base_error("invalid 'include.lowest' argument");
        }
        // Conversion and ALTREP reads may run callbacks. Retain both original
        // converted vectors throughout; the kernel only sees copied Rust data.
        let x = owner
            .sexp(x)
            .unwrap_or_else(|error| base_error(error.to_string()));
        let xs = numeric_values(&x);
        let bounds = numeric_values(&breaks);
        let mut codes = vec![NA_INTEGER; xs.len()];
        super::util_main::bincode_impl(
            xs.as_ptr(),
            xs.len(),
            bounds.as_ptr(),
            nb,
            codes.as_mut_ptr(),
            right,
            include,
        );
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, codes.len() as i64);
        let _result = protect(result);
        let result = owner
            .sexp(result)
            .unwrap_or_else(|error| base_error(error.to_string()));
        let mut result =
            SexpMut::try_from_checked(result).unwrap_or_else(|error| base_error(error.to_string()));
        for (index, code) in codes.into_iter().enumerate() {
            result
                .try_set_integer_elt(index as i64, code)
                .unwrap_or_else(|error| base_error(error.to_string()));
        }
        result.freeze().as_raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{object::SessionNodeFactory, session::RSession};

    #[test]
    fn binning_keeps_both_coercions_live_through_torture_and_result_collection() {
        let mut session = RSession::new_for_gc_tests();
        session
            .eval_code_with_output_capture("gctorture(TRUE)")
            .0
            .unwrap();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let integer = |values: &[i32]| {
            let value = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, values.len() as i64)))
                .unwrap();
            let mut value = SexpMut::try_from_checked(value).unwrap();
            for (index, &element) in values.iter().enumerate() {
                value.try_set_integer_elt(index as i64, element).unwrap();
            }
            value.freeze()
        };
        let mut args = factory.nil();
        for values in [
            &[1][..],
            &[1][..],
            &[0, 1, 2][..],
            &[-1, 0, 1, 2, NA_INTEGER][..],
        ] {
            let value = integer(values);
            args = factory
                .pairlist_cell(&value, &args, &factory.nil())
                .unwrap();
        }
        // Only the original argument graph owns either input before conversion.
        let result = session.with_active(|| unsafe {
            let raw = do_bincode(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args.as_raw(),
                std::ptr::null_mut(),
            );
            factory.wrap(raw).unwrap().into_owned().unwrap()
        });
        drop(args);
        session.owner_token().unwrap().full_gc().unwrap();
        let actual = (0..result.len())
            .map(|index| result.try_integer_elt(index))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(actual, [NA_INTEGER, 1, 1, 2, NA_INTEGER]);
        session
            .eval_code_with_output_capture("gctorture(FALSE)")
            .0
            .unwrap();
    }
}
