//! Bind ported tools C routines so GNU `.Call(C_doTabExpand, ...)` resolves.

use std::ffi::{CString, c_char, c_double, c_int};

use crate::sexp::accessors::TYPEOF;
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::symbol::Rf_install;


use crate::unix::dynload::DL_FUNC;

use super::text::{delim_match, doTabExpand, nonASCII};

const TOOLS_CALL_NAMES: &[&str] = &[
    "C_doTabExpand",
    "doTabExpand",
    "C_nonASCII",
    "nonASCII",
    "C_delim_match",
    "delim_match",
    "C_Renctest",
    "C_parseRd",
    "parseRd",
    "C_parseRdText",
    "parseRdText",
];

unsafe extern "C-unwind" fn c_do_tab_expand(strings: SEXP, starts: SEXP) -> SEXP {
    unsafe { doTabExpand(strings, starts) }
}

unsafe extern "C-unwind" fn c_non_ascii(text: SEXP) -> SEXP {
    unsafe { nonASCII(text) }
}

unsafe extern "C-unwind" fn c_delim_match(x: SEXP, delims: SEXP) -> SEXP {
    unsafe { delim_match(x, delims) }
}

unsafe extern "C" fn c_renctest(x: *mut std::ffi::c_void) {
    unsafe {
        if x.is_null() {
            return;
        }
        let p = x as *mut *const c_char;
        let s = *p;
        if s.is_null() {
            return;
        }
        let bytes = std::ffi::CStr::from_ptr(s).to_bytes();
        let shown = String::from_utf8_lossy(bytes);
        let msg = format!("'{}', nbytes = {}\n", shown, bytes.len());
        if let Ok(c) = CString::new(msg) {
            crate::mainutils::printutils::Rprintf(c.as_ptr(), std::ptr::null_mut());
        }
    }
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
        "nonASCII" => as_dl(c_non_ascii as unsafe extern "C-unwind" fn(SEXP) -> SEXP),
        "delim_match" => as_dl(c_delim_match as unsafe extern "C-unwind" fn(SEXP, SEXP) -> SEXP),
        "parseRd" | "parseRdText" => as_dl(
            super::parse_rd::c_parse_rd
                as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP,
        ),
        _ => None,
}
}

pub fn lookup_c(name: &str) -> DL_FUNC {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "Renctest" => as_dl(c_renctest as unsafe extern "C" fn(*mut std::ffi::c_void)),
        "kmns" => as_dl(crate::library::stats::kmeans::c_kmns as unsafe extern "C" fn(
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void,
        )),
        "eureka" => as_dl(crate::library::stats::burg::c_eureka as unsafe extern "C" fn(
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
        )),
        "multi_yw" => as_dl(crate::library::stats::mar::c_multi_yw as unsafe extern "C" fn(
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void,
        )),
        "kmeans_Lloyd" => as_dl(crate::library::stats::kmeans::c_kmeans_lloyd as unsafe extern "C" fn(
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void,
        )),
        "kmeans_MacQueen" => as_dl(crate::library::stats::kmeans::c_kmeans_macqueen as unsafe extern "C" fn(
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void,
        )),
        "hclust" => as_dl(crate::library::stats::hclust_f::c_hclust as unsafe extern "C" fn(
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void,
        )),

        "rbart" => as_dl(crate::library::stats::sbart::c_rbart as unsafe extern "C" fn(
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
        )),
        "bvalus" => as_dl(crate::library::stats::sbart::c_bvalus as unsafe extern "C" fn(
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
        )),
        "hcass2" => as_dl(crate::library::stats::hclust_f::c_hcass2 as unsafe extern "C" fn(
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void,
        )),
        "loess_raw" => {
            let f: unsafe extern "C" fn(
                *mut c_double, *mut c_double, *mut c_double, *mut c_double,
                *mut c_int, *mut c_int, *mut c_double, *mut c_int,
                *mut c_int, *mut c_int, *mut c_int, *mut c_double,
                *mut *mut c_char, *mut c_double, *mut c_int, *mut c_int,
                *mut c_double, *mut c_double, *mut c_double, *mut c_double,
                *mut c_double, *mut c_double, *mut c_double, *mut c_int,
            ) = crate::library::stats::loessc::loess_raw;
            as_dl(f)
        }
        "loess_dfit" => {
            let f: unsafe extern "C" fn(
                *mut c_double, *mut c_double, *mut c_double, *mut c_double,
                *mut c_double, *mut c_int, *mut c_int, *mut c_int,
                *mut c_int, *mut c_int, *mut c_int, *mut c_int,
                *mut c_double,
            ) = crate::library::stats::loessc::loess_dfit;
            as_dl(f)
        }
        "loess_ifit" => {
            let f: unsafe extern "C" fn(
                *mut c_int, *mut c_int, *mut c_double, *mut c_double,
                *mut c_double, *mut c_int, *mut c_double, *mut c_double,
            ) = crate::library::stats::loessc::loess_ifit;
            as_dl(f)
        }
        "lowesw" => as_dl(crate::library::stats::loessc::c_lowesw as unsafe extern "C" fn(
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
        )),
        "lowesp" => as_dl(crate::library::stats::loessc::c_lowesp as unsafe extern "C" fn(
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
            *mut std::ffi::c_void, *mut std::ffi::c_void, *mut std::ffi::c_void,
        )),
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

unsafe fn eval_tools_source(env: SEXP, source: &str) {
    unsafe {
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
pub unsafe fn install_tools_assert_closures(env: SEXP) {
    unsafe {
        let already = crate::sexp::envir::R_findVarInFrame(
            env,
            Rf_install(c"assertError".as_ptr()),
        );
        if already == crate::sexp::globals::R_UnboundValue()
            || crate::sexp::accessors::TYPEOF(already) != crate::sexp::ffi::SEXPTYPE::CLOSXP
        {
            eval_tools_source(env, include_str!("gnu_assertCondition.R"));
        }
        eval_tools_source(
            env,
            "delimMatch <- function(x, delim = c(\"{\", \"}\"), syntax = \"Rd\") {\n\
             if (!is.character(x))\n\
                 stop(\"argument 'x' must be a character vector\")\n\
             if ((length(delim) != 2L) || any(nchar(delim) != 1L))\n\
                 stop(\"argument 'delim' must specify two characters\")\n\
             if (syntax != \"Rd\")\n\
                 stop(\"only Rd syntax is currently supported\")\n\
             .Call(C_delim_match, x, delim)\n\
             }\n",
        );
        eval_tools_source(
            env,
            "parse_Rd <- function(file, ...) {\n\
             text <- paste(c(readLines(file, warn = FALSE), \"\"), collapse = \"\\n\")\n\
             .External2(C_parseRdText, text)\n\
             }\n",
        );
    }
}


