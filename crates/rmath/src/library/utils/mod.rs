//! Utils package - I/O and utilities

mod hashtab;
mod init;
mod io;
mod size;
mod sock;
mod stubs;

pub(crate) fn lookup(name: &str) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    match name {
        "countfields" | "C_countfields" => Some(
            crate::mainutils::native_routines::NativeRoutine::External1(c_countfields),
        ),
        "readtablehead" | "C_readtablehead" => Some(
            crate::mainutils::native_routines::NativeRoutine::External1(c_readtablehead),
        ),
        "octsize" | "C_octsize" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(c_octsize),
        )),
        "objectSize" | "C_objectSize" => {
            Some(crate::mainutils::native_routines::NativeRoutine::Call(
                crate::mainutils::native_routines::CallRoutine::Args1(c_object_size),
            ))
        }
        "typeconvert" | "C_typeconvert" => Some(
            crate::mainutils::native_routines::NativeRoutine::External2(c_typeconvert),
        ),
        "writetable" | "C_writetable" => Some(
            crate::mainutils::native_routines::NativeRoutine::External2(c_writetable),
        ),
        "edit" | "C_edit" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_edit,
        )),
        "tzcode_type" | "C_tzcode_type" => {
            Some(crate::mainutils::native_routines::NativeRoutine::Call(
                crate::mainutils::native_routines::CallRoutine::Args0(c_tzcode_type),
            ))
        }
        _ => None,
    }
}

unsafe extern "C-unwind" fn c_countfields(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { io::countfields(args) }
}

unsafe extern "C-unwind" fn c_tzcode_type() -> crate::sexp::ffi::SEXP {
    unsafe { stubs::tzcode_type() }
}

unsafe extern "C-unwind" fn c_readtablehead(
    args: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { io::readtablehead(args) }
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
