//! Utils package - I/O and utilities

mod hashtab;
mod init;
mod io;
mod size;
mod sock;
mod stubs;

pub(crate) unsafe fn lookup(name: &str) -> crate::unix::dynload::DL_FUNC {
    match name {
        "countfields" | "C_countfields" => Some(unsafe {
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP,
                _,
            >(c_countfields)
        }),
        "readtablehead" | "C_readtablehead" => Some(unsafe {
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP,
                _,
            >(c_readtablehead)
        }),
        "octsize" | "C_octsize" => Some(unsafe {
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP,
                _,
            >(c_octsize)
        }),
        "typeconvert" | "C_typeconvert" => Some(unsafe {
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(
                    crate::sexp::ffi::SEXP,
                    crate::sexp::ffi::SEXP,
                    crate::sexp::ffi::SEXP,
                    crate::sexp::ffi::SEXP,
                ) -> crate::sexp::ffi::SEXP,
                _,
            >(c_typeconvert)
        }),
        _ => None,
    }
}

unsafe extern "C-unwind" fn c_countfields(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { io::countfields(args) }
}

unsafe extern "C-unwind" fn c_readtablehead(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { io::readtablehead(args) }
}
unsafe extern "C-unwind" fn c_octsize(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { stubs::octsize(args) }
}

unsafe extern "C-unwind" fn c_typeconvert(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    env: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { io::typeconvert(call, op, args, env) }
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
        let parsed = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions(
                "format.bibentry <- function(x, macros = NULL, ...) {\n\
                 if (!length(x)) return(character())\n\
                 author <- as.character(x)[1L]\n\
                 if (length(macros)) {\n\
                   nm <- names(macros)\n\
                   for (i in seq_along(macros)) {\n\
                     if (!is.null(nm) && nzchar(nm[i]))\n\
                       author <- gsub(paste0(\"\\\\\", nm[i]), as.character(macros[[i]]), author, fixed = TRUE)\n\
                   }\n\
                 }\n\
                 author <- gsub(\"\\\\R\", \"R\", author, fixed = TRUE)\n\
                 paste0(author, \" (\", attr(x, \"year\"), \").\")\n\
                 }",
                arena,
            )
        });
        crate::eval::parser::flush_literal_warnings();
        if let Ok(exprs) = parsed {
            for expr in exprs {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    crate::eval::eval::Rf_eval(expr, env)
                }));
            }
        }
        let maker = crate::sexp::symbol::Rf_install(c"makeRweaveLatexCodeRunner".as_ptr());
        let bound = crate::sexp::envir::R_findVarInFrame(env, maker);
        if bound.is_null()
            || bound == crate::sexp::globals::R_UnboundValue()
            || bound == crate::sexp::globals::R_NilValue()
        {
            return;
        }
        let parsed = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions(
                "RweaveLatexRuncode <- makeRweaveLatexCodeRunner()",
                arena,
            )
        });
        crate::eval::parser::flush_literal_warnings();
        if let Ok(exprs) = parsed {
            for expr in exprs {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    crate::eval::eval::Rf_eval(expr, env)
                }));
            }
        }
    }
}
mod utils;
