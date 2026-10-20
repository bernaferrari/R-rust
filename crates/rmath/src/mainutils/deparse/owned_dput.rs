//! Original .Internal dput boundary, with owned arguments across deparse and I/O.

use crate::sexp::{
    context::r_error,
    ffi::{SEXP, SEXPTYPE},
    object::SexpResult,
    owner::OwnerToken,
};

fn required<T>(value: SexpResult<T>) -> T {
    value.unwrap_or_else(|error| r_error(error.to_string()))
}

pub(super) unsafe fn do_dput(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let owner = required(OwnerToken::current());
        let pin = required(owner.pin());
        let factory = owner.node_factory();
        let arguments = required(factory.wrap(args));
        crate::mainutils::relop::checkArity(op, arguments.as_raw());
        let value = required(arguments.try_car());
        let rest = required(arguments.try_cdr());
        let file = required(rest.try_car());
        let rest = required(rest.try_cdr());
        let control = required(rest.try_car());
        let options = if control.is_nil() {
            super::SHOWATTRIBUTES
        } else {
            super::gnu_deparse_opts_to_port(crate::mainutils::coerce::asInteger(control.as_raw()))
        };
        let printed = if value.typeof_() == SEXPTYPE::CLOSXP {
            let copy =
                required(factory.wrap(crate::mainutils::duplicate::duplicate(value.as_raw())));
            required(owner.require_active());
            crate::sexp::accessors::SET_CLOENV(copy.as_raw(), crate::sexp::globals::R_GlobalEnv());
            copy
        } else {
            value.clone()
        };
        let lines = required(factory.wrap(super::deparse1(printed.as_raw(), false, options)));
        required(owner.require_active());
        if crate::mainutils::objects::inherits2(file.as_raw(), c"connection".as_ptr()) == 0 {
            r_error("'file' must be a character string or connection");
        }
        let separator = required(factory.strings(&["\n"]));
        let bytes = required(factory.wrap(crate::sexp::globals::R_False()));
        let nil = factory.nil();
        let mut output_args = nil.clone();
        for argument in [&bytes, &separator, &file, &lines] {
            output_args = required(factory.pairlist_cell(argument, &output_args, &nil));
        }
        // The existing connection writer owns temporary-open cleanup, follows
        // sinks for stdout, and supports portable files and text connections.
        crate::mainutils::connections::do_writeLines(call, op, output_args.as_raw(), nil.as_raw());
        if let Some(pin) = &pin {
            required(pin.require_live());
        }
        required(owner.require_active());
        value.as_raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_dput_retains_arguments_and_output_across_full_collection() {
        let session = crate::sexp::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let nil = factory.nil();
        unsafe {
            let value = factory
                .wrap(crate::sexp::constructors::Rf_ScalarInteger(42))
                .unwrap();
            let file = factory
                .wrap(crate::mainutils::connections::do_stdout(
                    nil.as_raw(),
                    nil.as_raw(),
                    nil.as_raw(),
                    nil.as_raw(),
                ))
                .unwrap();
            let index = crate::eval::primitive::fun_tab_index_by_name("dput").unwrap();
            let op = factory
                .wrap(crate::mainutils::dstruct::mkPRIMSXP(index, 1))
                .unwrap();
            let mut args = nil.clone();
            for argument in [&nil, &file, &value] {
                args = factory.pairlist_cell(argument, &args, &nil).unwrap();
            }
            owner.full_gc().unwrap();
            let capture = crate::sexp::output::OutputCaptureGuard::start();
            let result = do_dput(nil.as_raw(), op.as_raw(), args.as_raw(), nil.as_raw());
            let result = factory.wrap(result).unwrap();
            assert_eq!(capture.finish().stdout, "42\n");
            owner.full_gc().unwrap();
            assert_eq!(result, value);
            assert_eq!(result.integer_elt(0), Some(42));
        }
    }
}
