//! Utils package - I/O and utilities

mod char_class;
mod hashtab;
mod init;
mod io;
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
    // SAFETY: the native dispatcher invokes this adapter within the active
    // interpreter; the pin below retains that same owner for the entire call.
    let owner = unsafe { OwnerToken::current() }.unwrap_or_else(|e| r_error(e.to_string()));
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
            let raw = unsafe {
                crate::main::coerce::coerceVector(input.as_raw(), SEXPTYPE::INTSXP.as_c_int())
            };
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
}

unsafe extern "C-unwind" fn c_rprof(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
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
    }
}

unsafe extern "C-unwind" fn c_countfields(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { io::countfields(args) }
}

unsafe extern "C-unwind" fn c_flushconsole() -> crate::sexp::ffi::SEXP {
    unsafe { stubs::flushconsole() }
}

unsafe extern "C-unwind" fn c_tzcode_type() -> crate::sexp::ffi::SEXP {
    unsafe { stubs::tzcode_type() }
}

unsafe extern "C-unwind" fn c_readtablehead(
    args: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { io::readtablehead(args) }
}
unsafe extern "C-unwind" fn c_crc64(in_: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { stubs::crc64(in_) }
}
unsafe extern "C-unwind" fn c_octsize(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { stubs::octsize(args) }
}
unsafe extern "C-unwind" fn c_object_size(x: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { size::objectSize(x) }
}

unsafe extern "C-unwind" fn c_typeconvert(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    env: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { io::typeconvert(call, op, args, env) }
}
unsafe extern "C-unwind" fn c_writetable(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    env: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { io::writetable(call, op, args, env) }
}
unsafe extern "C-unwind" fn c_edit(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    env: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { crate::mainutils::edit::do_edit(call, op, args, env) }
}

pub unsafe fn install_utils_call_symbols(env: crate::sexp::ffi::SEXP) {
    unsafe {
        // The original utils wrapper calls .External(C_Rprof, ...). Keep its
        // registration authority in this namespace's original allocation.
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let _pin = owner
            .pin()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let factory = owner.node_factory();
        let namespace = factory
            .wrap(env)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let binding = factory
            .wrap(crate::sexp::symbol::Rf_install(c"C_Rprof".as_ptr()))
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let keys = crate::mainutils::native_symbols::AttributeKeys {
            names: factory
                .wrap(crate::sexp::symbol::Rf_install(c"names".as_ptr()))
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string())),
            class: factory
                .wrap(crate::sexp::symbol::Rf_install(c"class".as_ptr()))
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
            .wrap(crate::sexp::symbol::Rf_install(c"C_charClass".as_ptr()))
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let metadata =
            crate::mainutils::native_symbols::symbol_info(owner, "charClass", "utils", true, &keys)
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
            crate::sexp::symbol::Rf_install(flush.as_ptr()),
            crate::sexp::constructors::Rf_mkString(flush.as_ptr()),
            env,
        );
        let cname = std::ffi::CString::new("C_countfields").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(cname.as_ptr()),
            crate::sexp::constructors::Rf_mkString(cname.as_ptr()),
            env,
        );
        let head = std::ffi::CString::new("C_readtablehead").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(head.as_ptr()),
            crate::sexp::constructors::Rf_mkString(head.as_ptr()),
            env,
        );
        let convert = std::ffi::CString::new("C_typeconvert").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(convert.as_ptr()),
            crate::sexp::constructors::Rf_mkString(convert.as_ptr()),
            env,
        );
        let write = std::ffi::CString::new("C_writetable").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(write.as_ptr()),
            crate::sexp::constructors::Rf_mkString(write.as_ptr()),
            env,
        );
        let size = std::ffi::CString::new("C_objectSize").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(size.as_ptr()),
            crate::sexp::constructors::Rf_mkString(size.as_ptr()),
            env,
        );
        let edit = std::ffi::CString::new("C_edit").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(edit.as_ptr()),
            crate::sexp::constructors::Rf_mkString(edit.as_ptr()),
            env,
        );
        let maker = crate::sexp::symbol::Rf_install(c"makeRweaveLatexCodeRunner".as_ptr());
        let bound = crate::sexp::envir::R_findVarInFrame(env, maker);
        if bound.is_null()
            || bound == crate::sexp::globals::R_UnboundValue()
            || bound == crate::sexp::globals::R_NilValue()
        {
            return;
        }
        let parser_factory = crate::eval::parser::active_factory();
        let parsed = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions(
                "RweaveLatexRuncode <- makeRweaveLatexCodeRunner()",
                arena,
                parser_factory,
            )
        });
        crate::eval::parser::flush_literal_warnings();
        if let Ok(exprs) = parsed {
            for expr in &exprs {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    crate::eval::eval::Rf_eval(expr.clone().as_raw(), env)
                }));
            }
        }
    }
}
mod utils;
