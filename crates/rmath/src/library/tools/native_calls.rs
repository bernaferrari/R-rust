//! Bind ported tools C routines so GNU `.Call(C_doTabExpand, ...)` resolves.

use std::ffi::{CStr, CString, c_char};

use crate::mainutils::native_routines::buffers::{Args, BufferError, NativeBuffer};
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
    "C_package_dependencies_scan",
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

unsafe fn c_parse_latex(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let arg = crate::sexp::accessors::CAR(crate::sexp::accessors::CDR(args));
    let mut text = if TYPEOF(arg) == SEXPTYPE::STRSXP {
        let ch = crate::sexp::accessors::STRING_ELT(arg, 0);
        String::from_utf8_lossy(&crate::sexp::accessors::char_cstr_bytes(ch)).into_owned()
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
        crate::sexp::constructors::mk_char_bytes(c.to_bytes()),
    );
    let tag = crate::sexp::symbol::install_str("latex_tag");
    crate::sexp::attrib_core::setAttrib(elt, tag, crate::sexp::constructors::mk_string_str("TEXT"));
    let out = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::VECSXP, 1);
    let _out = crate::sexp::protect::protect(out);
    crate::sexp::accessors::SET_VECTOR_ELT(out, 0, elt);
    crate::sexp::attrib_core::setAttrib(
        out,
        crate::sexp::symbol::install_str("class"),
        crate::sexp::constructors::mk_string_str("LaTeX"),
    );
    out
}

unsafe fn c_do_tab_expand(strings: SEXP, starts: SEXP) -> SEXP {
    doTabExpand(strings, starts)
}

unsafe fn c_non_ascii(text: SEXP) -> SEXP {
    nonASCII(text)
}

unsafe fn c_delim_match(x: SEXP, delims: SEXP) -> SEXP {
    delim_match(x, delims)
}
unsafe fn c_split_string(string: SEXP, delims: SEXP) -> SEXP {
    splitString(string, delims)
}

unsafe fn c_deparse_rd(element: SEXP, state: SEXP) -> SEXP {
    let text = if TYPEOF(element) == SEXPTYPE::STRSXP {
        element
    } else {
        crate::sexp::constructors::mk_string_str("")
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

fn renctest_owned(buffers: &mut [NativeBuffer]) -> Result<(), BufferError> {
    let mut args = Args::new(buffers);
    let bytes = CStr::from_bytes_with_nul(&args.chars()[0])
        .map_err(|_| BufferError::new("invalid native character buffer"))?
        .to_bytes();
    // GNU prints the native bytes verbatim; they need not be UTF-8.
    let mut msg = Vec::with_capacity(bytes.len() + 24);
    msg.push(b'\'');
    msg.extend_from_slice(bytes);
    msg.extend_from_slice(format!("', nbytes = {}\n", bytes.len()).as_bytes());
    crate::mainutils::printutils::rprintf_bytes(&msg);
    Ok(())
}

fn setsmu_owned(buffers: &mut [NativeBuffer]) -> Result<(), BufferError> {
    crate::library::stats::smart::setsmu(Args::new(buffers).int()[0]);
    Ok(())
}

pub(crate) fn lookup(name: &str) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "doTabExpand" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_do_tab_expand),
        )),
        "package_dependencies_scan" => {
            Some(crate::mainutils::native_routines::NativeRoutine::Call(
                crate::mainutils::native_routines::CallRoutine::Args1(
                    super::pdscan::package_dependencies_scan,
                ),
            ))
        }
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
    };
    let bare = name.strip_prefix("C_").unwrap_or(name);
    Some(match bare {
        "HoltWinters" => buffers::holtwinters::ROUTINE,
        "multi_burg" => buffers::multi_burg::ROUTINE,
        "stl" => buffers::stl::ROUTINE,
        "supsmu" => buffers::supsmu::ROUTINE,
        "setsmu" => BufferRoutine::owned(
            "stats",
            BufferInterface::Fortran,
            &[Integer],
            buffers::setsmu,
            setsmu_owned,
        ),
        "pppred" => buffers::ppr_predict::ROUTINE,
        "setppr" => buffers::smart::SETPPR_ROUTINE,
        "smart" => buffers::smart::SMART_ROUTINE,
        "dqrdc2" => BufferRoutine::owned(
            "base",
            BufferInterface::Fortran,
            &[
                Real, Integer, Integer, Integer, Real, Integer, Real, Integer, Real,
            ],
            buffers::dqrutl::dqrdc2_shape,
            buffers::dqrutl::dqrdc2,
        ),
        "dqrcf" => BufferRoutine::owned(
            "base",
            BufferInterface::Fortran,
            &[Real, Integer, Integer, Real, Real, Integer, Real, Integer],
            buffers::dqrutl::dqrcf_shape,
            buffers::dqrutl::dqrcf,
        ),
        "dqrqy" => BufferRoutine::owned(
            "base",
            BufferInterface::Fortran,
            &[Real, Integer, Integer, Real, Real, Integer, Real],
            buffers::dqrutl::dqrqy_shape,
            buffers::dqrutl::dqrqy,
        ),
        "dqrqty" => BufferRoutine::owned(
            "base",
            BufferInterface::Fortran,
            &[Real, Integer, Integer, Real, Real, Integer, Real],
            buffers::dqrutl::dqrqty_shape,
            buffers::dqrutl::dqrqty,
        ),
        "dqrrsd" => BufferRoutine::owned(
            "base",
            BufferInterface::Fortran,
            &[Real, Integer, Integer, Real, Real, Integer, Real],
            buffers::dqrutl::dqrrsd_shape,
            buffers::dqrutl::dqrrsd,
        ),
        "dqrxb" => BufferRoutine::owned(
            "base",
            BufferInterface::Fortran,
            &[Real, Integer, Integer, Real, Real, Integer, Real],
            buffers::dqrutl::dqrxb_shape,
            buffers::dqrutl::dqrxb,
        ),
        "dtrco" => BufferRoutine::owned(
            "base",
            BufferInterface::Fortran,
            &[Real, Integer, Integer, Real, Real, Integer],
            buffers::dtrco_shape,
            buffers::dtrco_owned,
        ),
        "Renctest" => BufferRoutine::owned(
            "tools",
            BufferInterface::C,
            &[Character],
            buffers::renctest,
            renctest_owned,
        ),
        "kmns" => BufferRoutine::owned(
            "stats",
            BufferInterface::Fortran,
            &[
                Real, Integer, Integer, Real, Integer, Integer, Integer, Integer, Real, Real,
                Integer, Real, Integer, Integer, Integer, Real, Integer,
            ],
            buffers::kmns,
            crate::library::stats::kmeans::kmns_owned,
        ),
        "eureka" => BufferRoutine::owned(
            "stats",
            BufferInterface::Fortran,
            &[Integer, Real, Real, Real, Real, Real],
            buffers::eureka,
            crate::library::stats::burg::eureka_owned,
        ),
        "multi_yw" => BufferRoutine::owned(
            "stats",
            BufferInterface::C,
            &[
                Real, Integer, Integer, Integer, Real, Real, Real, Real, Integer, Integer,
            ],
            buffers::multi_yw,
            crate::library::stats::mar::multi_yw_owned,
        ),
        "kmeans_Lloyd" => BufferRoutine::owned(
            "stats",
            BufferInterface::C,
            &[
                Real, Integer, Integer, Real, Integer, Integer, Integer, Integer, Real,
            ],
            buffers::kmeans,
            crate::library::stats::kmeans::kmeans_lloyd_owned,
        ),
        "kmeans_MacQueen" => BufferRoutine::owned(
            "stats",
            BufferInterface::C,
            &[
                Real, Integer, Integer, Real, Integer, Integer, Integer, Integer, Real,
            ],
            buffers::kmeans,
            crate::library::stats::kmeans::kmeans_macqueen_owned,
        ),
        "hclust" => BufferRoutine::owned(
            "stats",
            BufferInterface::Fortran,
            &[
                Integer, Integer, Integer, Integer, Integer, Real, Real, Integer, Real, Real,
            ],
            buffers::hclust,
            crate::library::stats::hclust_f::hclust_owned,
        ),
        "rbart" => BufferRoutine::owned(
            "stats",
            BufferInterface::Fortran,
            &[
                Real, Real, Real, Real, Real, Real, Integer, Real, Integer, Real, Real, Real,
                Real, Integer, Real, Real, Real, Integer, Integer, Integer,
            ],
            buffers::rbart,
            crate::library::stats::sbart::rbart_owned,
        ),
        "bvalus" => BufferRoutine::owned(
            "stats",
            BufferInterface::Fortran,
            &[Integer, Real, Real, Integer, Real, Real, Integer],
            buffers::bvalus,
            crate::library::stats::sbart::bvalus_owned,
        ),
        "hcass2" => BufferRoutine::owned(
            "stats",
            BufferInterface::Fortran,
            &[Integer, Integer, Integer, Integer, Integer, Integer],
            buffers::hcass2,
            crate::library::stats::hclust_f::hcass2_owned,
        ),
        "loess_raw" => BufferRoutine::owned(
            "stats",
            BufferInterface::C,
            &[Real, Real, Real, Real, Integer, Integer, Real, Integer, Integer, Integer, Integer, Real, Character, Real, Integer, Integer, Real, Real, Real, Real, Real, Real, Real, Integer],
            buffers::loess_raw,
            crate::library::stats::loessc::loess_raw,
        ),
        "loess_dfit" => BufferRoutine::owned(
            "stats",
            BufferInterface::C,
            &[Real, Real, Real, Real, Real, Integer, Integer, Integer, Integer, Integer, Integer, Integer, Real],
            buffers::loess_dfit,
            crate::library::stats::loessc::loess_dfit,
        ),
        "loess_ifit" => buffers::loess_ifit::ROUTINE,
        "loess_ise" => BufferRoutine::owned(
            "stats",
            BufferInterface::C,
            &[Real, Real, Real, Real, Real, Integer, Integer, Integer, Integer, Real, Integer, Integer, Integer, Real, Real],
            buffers::loess_ise,
            crate::library::stats::loessc::loess_ise,
        ),
        "loess_dfitse" => BufferRoutine::owned(
            "stats",
            BufferInterface::C,
            &[Real, Real, Real, Real, Real, Integer, Real, Integer, Integer, Integer, Integer, Integer, Integer, Integer, Real, Real],
            buffers::loess_dfitse,
            crate::library::stats::loessc::loess_dfitse,
        ),
        "lowesw" => BufferRoutine::owned(
            "stats",
            BufferInterface::Fortran,
            &[Real, Integer, Real, Integer],
            buffers::lowesw,
            crate::library::stats::loessc::lowesw,
        ),
        "lowesp" => BufferRoutine::owned(
            "stats",
            BufferInterface::Fortran,
            &[Integer, Real, Real, Real, Real, Integer, Real],
            buffers::lowesp,
            crate::library::stats::loessc::lowesp,
        ),
        _ => return None,
    })
}

pub fn install_tools_call_symbols(env: SEXP) {
    unsafe {
        for name in TOOLS_CALL_NAMES {
            let cname = CString::new(*name).unwrap_or_default();
            defineVar(
                crate::sexp::symbol::install_cstr(&cname),
                Rf_mkString(cname.as_ptr() as *const c_char),
                env,
            );
        }
    }
}

fn eval_tools_source(env: SEXP, source: &str) {
    let parsed = crate::eval::parser::parse_active_expressions(source);
    crate::eval::parser::flush_literal_warnings();
    let Ok(exprs) = parsed else {
        return;
    };
    for expr in &exprs {
        let _ = crate::eval::eval::Rf_eval(expr.clone().as_raw(), env);
    }
}
pub fn install_tools_assert_closures(env: SEXP) {
    let already =
        crate::sexp::envir::R_findVarInFrame(env, crate::sexp::symbol::install_str("assertError"));
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
