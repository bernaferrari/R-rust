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
        _ => None,
    }
}

unsafe extern "C-unwind" fn c_countfields(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { io::countfields(args) }
}

pub unsafe fn install_utils_call_symbols(env: crate::sexp::ffi::SEXP) {
    unsafe {
        let cname = std::ffi::CString::new("C_countfields").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(cname.as_ptr()),
            crate::sexp::constructors::Rf_mkString(cname.as_ptr()),
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
