//! Utils package - I/O and utilities

mod char_class;
mod hashtab;
mod init;
mod io;
#[cfg(test)]
pub(crate) use self::io::writetable;
mod size;
mod sock;
mod stubs;

pub(crate) fn lookup(name: &str) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    match name {
        "charClass" | "C_charClass" => {
            Some(crate::mainutils::native_routines::NativeRoutine::Call(
                crate::mainutils::native_routines::CallRoutine::Args2(c_char_class),
            ))
        }
        "Rprof" | "C_Rprof" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_rprof,
            crate::mainutils::native_routines::PayloadArity::Fixed(10),
        )),
        "flushconsole" | "C_flushconsole" => {
            Some(crate::mainutils::native_routines::NativeRoutine::Call(
                crate::mainutils::native_routines::CallRoutine::Args0(c_flushconsole),
            ))
        }
        "countfields" | "C_countfields" => {
            Some(crate::mainutils::native_routines::NativeRoutine::External1(
                c_countfields,
                crate::mainutils::native_routines::PayloadArity::Fixed(6),
            ))
        }
        "readtablehead" | "C_readtablehead" => {
            Some(crate::mainutils::native_routines::NativeRoutine::External1(
                c_readtablehead,
                crate::mainutils::native_routines::PayloadArity::Fixed(7),
            ))
        }
        "crc64" | "C_crc64" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(c_crc64),
        )),
        "octsize" | "C_octsize" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(c_octsize),
        )),
        "objectSize" | "C_objectSize" => {
            Some(crate::mainutils::native_routines::NativeRoutine::Call(
                crate::mainutils::native_routines::CallRoutine::Args1(c_object_size),
            ))
        }
        "typeconvert" | "C_typeconvert" => {
            Some(crate::mainutils::native_routines::NativeRoutine::External2(
                c_typeconvert,
                crate::mainutils::native_routines::PayloadArity::Fixed(6),
            ))
        }
        "writetable" | "C_writetable" => {
            Some(crate::mainutils::native_routines::NativeRoutine::External2(
                c_writetable,
                crate::mainutils::native_routines::PayloadArity::Fixed(11),
            ))
        }
        "edit" | "C_edit" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_edit,
            crate::mainutils::native_routines::PayloadArity::Fixed(4),
        )),
        "tzcode_type" | "C_tzcode_type" => {
            Some(crate::mainutils::native_routines::NativeRoutine::Call(
                crate::mainutils::native_routines::CallRoutine::Args0(c_tzcode_type),
            ))
        }
        _ => None,
    }
}

unsafe extern "C-unwind" fn c_char_class(
    x: crate::sexp::ffi::SEXP,
    class: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    use crate::sexp::{
        context::r_error,
        ffi::SEXPTYPE,
        owner::{OwnerToken, StoredOwner},
    };
    OwnerToken::with_current(|owner| {
        let _pin = owner.pin().unwrap_or_else(|e| r_error(e.to_string()));
        let factory = owner.node_factory();
        let input = factory.wrap(x).unwrap_or_else(|e| r_error(e.to_string()));
        let class = factory
            .wrap(class)
            .unwrap_or_else(|e| r_error(e.to_string()));
        let stored = StoredOwner::from_token(owner);
        let outcome = char_class::prepare(stored.clone(), input, class);
        let result = match outcome {
            Ok(char_class::Classification::Complete(value)) => value,
            Ok(char_class::Classification::NeedsIntegerCoercion { input, descriptor }) => {
                // SAFETY: input is rooted in the pinned active owner throughout
                // coercion, and the resulting pointer is wrapped after rechecking it.
                let raw =
                    crate::main::coerce::coerceVector(input.as_raw(), SEXPTYPE::INTSXP.as_c_int());
                owner
                    .require_active()
                    .unwrap_or_else(|e| r_error(e.to_string()));
                let value = factory.wrap(raw).unwrap_or_else(|e| r_error(e.to_string()));
                char_class::classify_integers(stored, value, descriptor)
                    .unwrap_or_else(|e| r_error(e.to_string()))
            }
            Err(char_class::ClassError::Access(error)) => r_error(error.to_string()),
            Err(char_class::ClassError::InvalidClassArgument) => {
                r_error("argument 'class' must be a character string")
            }
            Err(char_class::ClassError::InvalidCharacterArgument) => {
                r_error("argument 'x' must be a length-1 character vector")
            }
            Err(char_class::ClassError::InvalidClass(name)) => {
                let message = char_class::invalid_class_message(stored, name)
                    .unwrap_or_else(|e| r_error(e.to_string()));
                r_error(message.as_str())
            }
        };
        owner
            .require_active()
            .unwrap_or_else(|e| r_error(e.to_string()));
        result.as_raw()
    })
    .unwrap_or_else(|e| r_error(e.to_string()))
}

unsafe extern "C-unwind" fn c_rprof(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    {
        crate::sexp::owner::OwnerToken::with_current(|owner| {
            let _pin = owner
                .pin()
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            let arguments = owner
                .node_factory()
                .wrap(args)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            let nil = crate::sexp::globals::R_NilValue();
            let result = crate::mainutils::essentials::do_Rprof(
                nil,
                nil,
                crate::sexp::accessors::CDR(arguments.as_raw()),
                nil,
            );
            owner
                .require_active()
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            result
        })
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
    }
}

unsafe extern "C-unwind" fn c_countfields(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    io::countfields(args)
}

unsafe extern "C-unwind" fn c_flushconsole() -> crate::sexp::ffi::SEXP {
    stubs::flushconsole()
}

unsafe extern "C-unwind" fn c_tzcode_type() -> crate::sexp::ffi::SEXP {
    stubs::tzcode_type()
}

unsafe extern "C-unwind" fn c_readtablehead(
    args: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    io::readtablehead(args)
}
unsafe extern "C-unwind" fn c_crc64(in_: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    stubs::crc64(in_)
}
unsafe extern "C-unwind" fn c_octsize(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    stubs::octsize(args)
}
unsafe extern "C-unwind" fn c_object_size(x: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    size::objectSize(x)
}

unsafe extern "C-unwind" fn c_typeconvert(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    env: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    io::typeconvert(call, op, args, env)
}
unsafe extern "C-unwind" fn c_writetable(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    env: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    io::writetable(call, op, args, env)
}
unsafe extern "C-unwind" fn c_edit(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    env: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { crate::mainutils::edit::do_edit(call, op, args, env) }
}

pub fn install_utils_call_symbols(env: crate::sexp::ffi::SEXP) {
    // The original utils wrapper calls .External(C_Rprof, ...). Keep its
    // registration authority in this namespace's original allocation.
    crate::sexp::owner::OwnerToken::with_current(|owner| {
        let _pin = owner
            .pin()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let factory = owner.node_factory();
        let namespace = factory
            .wrap(env)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let binding = factory
            .wrap(crate::sexp::symbol::install_str("C_Rprof"))
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let keys = crate::mainutils::native_symbols::AttributeKeys {
            names: factory
                .wrap(crate::sexp::symbol::install_str("names"))
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string())),
            class: factory
                .wrap(crate::sexp::symbol::install_str("class"))
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string())),
        };
        let metadata =
            crate::mainutils::native_symbols::symbol_info(owner, "Rprof", "utils", true, &keys)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
                .expect("bundled Rprof registration is present");
        let address = metadata
            .try_vector_elt(1)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        crate::sexp::envir::defineVar(binding.as_raw(), address.as_raw(), namespace.as_raw());
        drop(metadata);
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let binding = factory
            .wrap(crate::sexp::symbol::install_str("C_charClass"))
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let metadata = crate::mainutils::native_symbols::symbol_info(
            owner,
            "charClass",
            "utils",
            true,
            &keys,
        )
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        .expect("bundled charClass registration is present");
        let address = metadata
            .try_vector_elt(1)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        crate::sexp::envir::defineVar(binding.as_raw(), address.as_raw(), namespace.as_raw());
        drop(metadata);
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let flush = c"C_flushconsole";
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::install_cstr(&flush),
            crate::sexp::constructors::mk_string_bytes(flush.to_bytes()),
            env,
        );
        let cname = std::ffi::CString::new("C_countfields").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::install_cstr(&cname),
            crate::sexp::constructors::mk_string_bytes(cname.to_bytes()),
            env,
        );
        let head = std::ffi::CString::new("C_readtablehead").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::install_cstr(&head),
            crate::sexp::constructors::mk_string_bytes(head.to_bytes()),
            env,
        );
        let convert = std::ffi::CString::new("C_typeconvert").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::install_cstr(&convert),
            crate::sexp::constructors::mk_string_bytes(convert.to_bytes()),
            env,
        );
        let write = std::ffi::CString::new("C_writetable").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::install_cstr(&write),
            crate::sexp::constructors::mk_string_bytes(write.to_bytes()),
            env,
        );
        let size = std::ffi::CString::new("C_objectSize").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::install_cstr(&size),
            crate::sexp::constructors::mk_string_bytes(size.to_bytes()),
            env,
        );
        let edit = std::ffi::CString::new("C_edit").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::install_cstr(&edit),
            crate::sexp::constructors::mk_string_bytes(edit.to_bytes()),
            env,
        );
        let maker = crate::sexp::symbol::install_str("makeRweaveLatexCodeRunner");
        let bound = crate::sexp::envir::R_findVarInFrame(env, maker);
        if bound.is_null()
            || bound == crate::sexp::globals::R_UnboundValue()
            || bound == crate::sexp::globals::R_NilValue()
        {
            return;
        }
        let parser_factory = owner.node_factory();
        let parsed = owner
            .with_arena(|arena| {
                crate::eval::parser::parse_expressions(
                    "RweaveLatexRuncode <- makeRweaveLatexCodeRunner()",
                    arena,
                    parser_factory,
                )
            })
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        crate::eval::parser::flush_literal_warnings();
        if let Ok(exprs) = parsed {
            for expr in &exprs {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    crate::eval::eval::Rf_eval(expr.clone().as_raw(), env)
                }));
            }
        }
    })
    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
}
mod utils;
