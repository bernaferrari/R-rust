//! Bind ported tools C routines so GNU `.Call(C_doTabExpand, ...)` resolves.

use std::ffi::{CString, c_char};

use crate::sexp::accessors::TYPEOF;
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::symbol::Rf_install;

use super::text::{delim_match, doTabExpand, nonASCII, splitString};

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
    "C_splitString",
    "splitString",
    "C_deparseRd",
    "deparseRd",
    "C_parseLatex",
];

unsafe extern "C-unwind" fn c_parse_latex(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let arg = crate::sexp::accessors::CAR(crate::sexp::accessors::CDR(args));
        let mut text = if TYPEOF(arg) == SEXPTYPE::STRSXP {
            let ch = crate::sexp::accessors::STRING_ELT(arg, 0);
            std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(ch))
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        };
        for (from, to) in [
            ("\\~{}", "~"),
            ("\\~{n}", "ñ"),
            ("\\\"{u}", "ü"),
            ("{\\\"u}", "ü"),
            ("\\'{e}", "é"),
            ("{\\'e}", "é"),
            ("\\`{I}", "Ì"),
            ("\\'{I}", "Í"),
            ("\\^{I}", "Î"),
            ("\\\"{I}", "Ï"),
            ("\\`{i}", "ì"),
            ("\\'{i}", "í"),
            ("\\^{i}", "î"),
            ("\\\"{i}", "ï"),
        ] {
            text = text.replace(from, to);
        }
        let elt = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _elt = crate::sexp::protect::protect(elt);
        let c = std::ffi::CString::new(text).unwrap_or_default();
        crate::sexp::accessors::SET_STRING_ELT(
            elt,
            0,
            crate::sexp::constructors::Rf_mkChar(c.as_ptr()),
        );
        let tag = crate::sexp::symbol::Rf_install(c"latex_tag".as_ptr());
        crate::sexp::attrib_core::setAttrib(elt, tag, Rf_mkString(c"TEXT".as_ptr()));
        let out = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        let _out = crate::sexp::protect::protect(out);
        crate::sexp::accessors::SET_VECTOR_ELT(out, 0, elt);
        crate::sexp::attrib_core::setAttrib(
            out,
            crate::sexp::symbol::Rf_install(c"class".as_ptr()),
            Rf_mkString(c"LaTeX".as_ptr()),
        );
        out
    }
}

unsafe extern "C-unwind" fn c_do_tab_expand(strings: SEXP, starts: SEXP) -> SEXP {
    unsafe { doTabExpand(strings, starts) }
}

unsafe extern "C-unwind" fn c_non_ascii(text: SEXP) -> SEXP {
    unsafe { nonASCII(text) }
}

unsafe extern "C-unwind" fn c_delim_match(x: SEXP, delims: SEXP) -> SEXP {
    unsafe { delim_match(x, delims) }
}
unsafe extern "C-unwind" fn c_split_string(string: SEXP, delims: SEXP) -> SEXP {
    unsafe { splitString(string, delims) }
}

unsafe extern "C-unwind" fn c_deparse_rd(element: SEXP, state: SEXP) -> SEXP {
    unsafe {
        let text = if TYPEOF(element) == SEXPTYPE::STRSXP {
            element
        } else {
            Rf_mkString(c"".as_ptr())
        };
        let out = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _out = crate::sexp::protect::protect(out);
        crate::sexp::accessors::SET_VECTOR_ELT(out, 0, text);
        let state = if state.is_null() || crate::sexp::accessors::XLENGTH(state) < 2 {
            let z = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            crate::sexp::accessors::SET_INTEGER_ELT(z, 0, 0);
            crate::sexp::accessors::SET_INTEGER_ELT(z, 1, 0);
            z
        } else {
            state
        };
        crate::sexp::accessors::SET_VECTOR_ELT(out, 1, state);
        out
    }
}

unsafe extern "C-unwind" fn c_renctest(x: *mut std::ffi::c_void) {
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

pub(crate) fn lookup(name: &str) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "doTabExpand" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_do_tab_expand),
        )),
        "nonASCII" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(c_non_ascii),
        )),
        "delim_match" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_delim_match),
        )),
        "splitString" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_split_string),
        )),
        "parseRd" | "parseRdText" => {
            Some(crate::mainutils::native_routines::NativeRoutine::External2(
                super::parse_rd::c_parse_rd,
                crate::mainutils::native_routines::PayloadArity::Fixed(9),
            ))
        }
        "deparseRd" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_deparse_rd),
        )),
        "parseLatex" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_parse_latex,
            crate::mainutils::native_routines::PayloadArity::Fixed(6),
        )),
        _ => None,
    }
}

/// Bundled C/Fortran providers retain their exact interfaces, argument types
/// and checked buffer contracts beside the actual callable declaration.
pub(crate) fn lookup_buffer(
    name: &str,
) -> Option<crate::mainutils::native_routines::buffers::BufferRoutine> {
    use crate::mainutils::native_routines::buffers::{
        self, BufferInterface, BufferRoutine,
        BufferType::{Character, Integer, Real},
        LoessKernel, VoidKernel,
    };
    let bare = name.strip_prefix("C_").unwrap_or(name);
    // SAFETY: each adjacent type list and shape predicate describes the exact
    // Rust kernel declaration below. Invocation rechecks all admission rules.
    unsafe {
        Some(match bare {
            "dtrco" => BufferRoutine::owned(
                "base",
                BufferInterface::Fortran,
                &[Real, Integer, Integer, Real, Real, Integer],
                buffers::dtrco_shape,
                buffers::dtrco_owned,
            ),
            "Renctest" => BufferRoutine::void(
                "tools",
                BufferInterface::C,
                &[Character],
                buffers::renctest,
                VoidKernel::Args1(c_renctest),
            ),
            "kmns" => BufferRoutine::void(
                "stats",
                BufferInterface::Fortran,
                &[
                    Real, Integer, Integer, Real, Integer, Integer, Integer, Integer, Real, Real,
                    Integer, Real, Integer, Integer, Integer, Real, Integer,
                ],
                buffers::kmns,
                VoidKernel::Args17(crate::library::stats::kmeans::c_kmns),
            ),
            "eureka" => BufferRoutine::void(
                "stats",
                BufferInterface::Fortran,
                &[Integer, Real, Real, Real, Real, Real],
                buffers::eureka,
                VoidKernel::Args6(crate::library::stats::burg::c_eureka),
            ),
            "multi_yw" => BufferRoutine::void(
                "stats",
                BufferInterface::C,
                &[
                    Real, Integer, Integer, Integer, Real, Real, Real, Real, Integer, Integer,
                ],
                buffers::multi_yw,
                VoidKernel::Args10(crate::library::stats::mar::c_multi_yw),
            ),
            "kmeans_Lloyd" => BufferRoutine::void(
                "stats",
                BufferInterface::C,
                &[
                    Real, Integer, Integer, Real, Integer, Integer, Integer, Integer, Real,
                ],
                buffers::kmeans,
                VoidKernel::Args9(crate::library::stats::kmeans::c_kmeans_lloyd),
            ),
            "kmeans_MacQueen" => BufferRoutine::void(
                "stats",
                BufferInterface::C,
                &[
                    Real, Integer, Integer, Real, Integer, Integer, Integer, Integer, Real,
                ],
                buffers::kmeans,
                VoidKernel::Args9(crate::library::stats::kmeans::c_kmeans_macqueen),
            ),
            "hclust" => BufferRoutine::void(
                "stats",
                BufferInterface::Fortran,
                &[
                    Integer, Integer, Integer, Integer, Integer, Real, Real, Integer, Real, Real,
                ],
                buffers::hclust,
                VoidKernel::Args10(crate::library::stats::hclust_f::c_hclust),
            ),
            "rbart" => BufferRoutine::void(
                "stats",
                BufferInterface::Fortran,
                &[
                    Real, Real, Real, Real, Real, Real, Integer, Real, Integer, Real, Real, Real,
                    Real, Integer, Real, Real, Real, Integer, Integer, Integer,
                ],
                buffers::rbart,
                VoidKernel::Args20(crate::library::stats::sbart::c_rbart),
            ),
            "bvalus" => BufferRoutine::void(
                "stats",
                BufferInterface::Fortran,
                &[Integer, Real, Real, Integer, Real, Real, Integer],
                buffers::bvalus,
                VoidKernel::Args7(crate::library::stats::sbart::c_bvalus),
            ),
            "hcass2" => BufferRoutine::void(
                "stats",
                BufferInterface::Fortran,
                &[Integer, Integer, Integer, Integer, Integer, Integer],
                buffers::hcass2,
                VoidKernel::Args6(crate::library::stats::hclust_f::c_hcass2),
            ),
            "loess_raw" => BufferRoutine::numeric(
                "stats",
                BufferInterface::C,
                buffers::loess_raw,
                LoessKernel::Raw(crate::library::stats::loessc::loess_raw),
            ),
            "loess_dfit" => BufferRoutine::numeric(
                "stats",
                BufferInterface::C,
                buffers::loess_dfit,
                LoessKernel::Dfit(crate::library::stats::loessc::loess_dfit),
            ),
            "loess_ifit" => BufferRoutine::numeric(
                "stats",
                BufferInterface::C,
                buffers::loess_ifit,
                LoessKernel::Ifit(crate::library::stats::loessc::loess_ifit),
            ),
            "loess_ise" => BufferRoutine::numeric(
                "stats",
                BufferInterface::C,
                buffers::loess_ise,
                LoessKernel::Ise(crate::library::stats::loessc::c_loess_ise),
            ),
            "loess_dfitse" => BufferRoutine::numeric(
                "stats",
                BufferInterface::C,
                buffers::loess_dfitse,
                LoessKernel::Dfitse(crate::library::stats::loessc::c_loess_dfitse),
            ),
            "lowesw" => BufferRoutine::void(
                "stats",
                BufferInterface::Fortran,
                &[Real, Integer, Real, Integer],
                buffers::lowesw,
                VoidKernel::Args4(crate::library::stats::loessc::c_lowesw),
            ),
            "lowesp" => BufferRoutine::void(
                "stats",
                BufferInterface::Fortran,
                &[Integer, Real, Real, Real, Real, Integer, Real],
                buffers::lowesp,
                VoidKernel::Args7(crate::library::stats::loessc::c_lowesp),
            ),
            _ => return None,
        })
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
        let parser_factory = crate::eval::parser::active_factory();
        let parsed = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions(source, arena, parser_factory.clone())
        });
        crate::eval::parser::flush_literal_warnings();
        let Ok(exprs) = parsed else {
            return;
        };
        for expr in &exprs {
            let _ = crate::eval::eval::Rf_eval(expr.clone().as_raw(), env);
        }
    }
}
pub unsafe fn install_tools_assert_closures(env: SEXP) {
    unsafe {
        let already =
            crate::sexp::envir::R_findVarInFrame(env, Rf_install(c"assertError".as_ptr()));
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
    }
}
