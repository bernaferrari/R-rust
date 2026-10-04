//! Grid package - grid graphics

mod clippath;
mod gpar;
mod grid;
mod just;
mod layout;
mod mask;
mod matrix;
mod path;
mod register;
pub mod state;
pub mod types;
mod typeset;
mod unit;
pub mod util;
mod viewport;

use crate::sexp::ffi::SEXP;

unsafe extern "C-unwind" fn c_pretty2(scale: SEXP, n: SEXP) -> SEXP {
    unsafe { grid::L_pretty2(scale, n) }
}

pub(crate) fn lookup(name: &str) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "pretty2" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_pretty2),
        )),
        _ => None,
    }
}

pub unsafe fn install_call_symbols(env: SEXP) {
    unsafe {
        let cname = std::ffi::CString::new("C_pretty2").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(cname.as_ptr()),
            crate::sexp::constructors::Rf_mkString(cname.as_ptr()),
            env,
        );
    }
}
