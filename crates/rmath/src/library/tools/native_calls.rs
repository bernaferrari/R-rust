//! Bind ported tools C routines so GNU `.Call(C_doTabExpand, ...)` resolves.

use std::ffi::{CString, c_char};

use crate::sexp::accessors::TYPEOF;
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::symbol::Rf_install;


use crate::unix::dynload::DL_FUNC;

use super::text::doTabExpand;

const TOOLS_CALL_NAMES: &[&str] = &["C_doTabExpand", "doTabExpand"];

unsafe extern "C-unwind" fn c_do_tab_expand(strings: SEXP, starts: SEXP) -> SEXP {
    unsafe { doTabExpand(strings, starts) }
}

fn as_dl<T>(f: T) -> DL_FUNC {
    Some(unsafe { std::mem::transmute_copy(&f) })
}

pub fn lookup(name: &str) -> DL_FUNC {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "doTabExpand" => {
            as_dl(c_do_tab_expand as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP)
        }
        _ => None,
    }
}

pub unsafe fn install_tools_call_symbols(env: SEXP) {
    unsafe {
        for name in TOOLS_CALL_NAMES {
            let cname = CString::new(*name).unwrap_or_default();
            defineVar(
                Rf_install(cname.as_ptr()),
                Rf_mkString(cname.as_ptr() as *const c_char),
                env,
            );
        }
    }
}
pub unsafe fn install_tools_assert_closures(env: SEXP) {
    unsafe {
        let already = crate::sexp::envir::R_findVarInFrame(
            env,
            Rf_install(c"assertError".as_ptr()),
        );
        if already != crate::sexp::globals::R_UnboundValue()
            && crate::sexp::accessors::TYPEOF(already) == crate::sexp::ffi::SEXPTYPE::CLOSXP
        {
            return;
        }
        let source = include_str!("gnu_assertCondition.R");
        let parsed = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions(source, arena)
        });
        crate::eval::parser::flush_literal_warnings();
        let Ok(exprs) = parsed else {
            return;
        };
        for expr in exprs {
            let _ = crate::eval::eval::Rf_eval(expr, env);
        }
    }
}


