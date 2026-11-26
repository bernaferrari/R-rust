#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Workspace save/load entry points.
//!
//! User-facing `save()`/`load()` and the internal snapshot paths. Payloads
//! are RBG1 graphs inside the workspace container; the GNU wire formats
//! (ascii/XDR/binary) and the pre-RBG1 legacy layout are gone.

use crate::sexp::accessors::{
    CAD5R, CADDR, CADR, CAR, CDR, CHAR, COMPLEX, INTEGER, LOGICAL, PRINTNAME, RAW, REAL,
    SET_ATTRIB, SET_OBJECT, SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETTAG, STRING_ELT, TAG,
    TYPEOF, XLENGTH,
};
use crate::sexp::attrib_core::{R_ClassSymbol, getAttrib};
use crate::sexp::constructors::{Rf_allocList, Rf_allocVector3};
use crate::sexp::envir::R_findVar;
use crate::sexp::ffi::{R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NaString, R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;

use std::io::{self, BufRead, Cursor, Read, Write};
use std::os::raw::c_int;

mod workspace_file;
#[cfg(test)]
mod workspace_tests;

fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_INTEGER sentinel.
const NA_INTEGER: c_int = c_int::MIN;

pub fn defaultSaveVersion() -> c_int {
    match std::env::var("R_DEFAULT_SAVE_VERSION") {
        Ok(val) => match val.trim().parse::<c_int>() {
            Ok(2 | 3) => val.trim().parse::<c_int>().unwrap_or(3),
            _ => 3,
        },
        Err(_) => 3,
    }
}

unsafe fn tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(tag);
        if printname.is_null() {
            return None;
        }
        let ptr = CHAR(printname);
        if ptr.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

unsafe fn load_arguments(args: SEXP) -> [Option<SEXP>; 3] {
    const NAMES: [&str; 3] = ["file", "envir", "verbose"];
    let mut selected = [None; 3];
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if let Some(name) = tag_name(current) {
                let index = NAMES
                    .iter()
                    .position(|candidate| *candidate == name)
                    .unwrap_or_else(|| error(&format!("unused argument ({name} = ...)")));
                if selected[index].replace(CAR(current)).is_some() {
                    error(&format!(
                        "formal argument \"{name}\" matched by multiple actual arguments"
                    ));
                }
            }
            current = CDR(current);
        }
        let mut current = args;
        let mut available = selected.iter_mut().filter(|slot| slot.is_none());
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                *available
                    .next()
                    .unwrap_or_else(|| error("unused load() argument")) = Some(CAR(current));
            }
            current = CDR(current);
        }
    }
    selected
}

unsafe fn save_arguments(args: SEXP) -> ([Option<SEXP>; 9], Vec<String>) {
    const NAMES: [&str; 9] = [
        "list",
        "file",
        "ascii",
        "version",
        "envir",
        "compress",
        "compression_level",
        "eval.promises",
        "precheck",
    ];
    let mut options = [None; 9];
    let mut names = Vec::new();
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let name = tag_name(current);
            let expr = CAR(current);
            if let Some(index) = name
                .as_deref()
                .and_then(|name| NAMES.iter().position(|option| *option == name))
            {
                if options[index].replace(expr).is_some() {
                    error(&format!(
                        "formal argument \"{}\" matched by multiple actual arguments",
                        NAMES[index]
                    ));
                }
            } else {
                if expr.is_null() || TYPEOF(expr) != SEXPTYPE::SYMSXP || expr == R_MissingArg() {
                    error("save() requires object names");
                }
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(expr)));
                names.push(name.to_string_lossy().into_owned());
            }
            current = CDR(current);
        }
    }
    (options, names)
}

fn eval_save_argument(arg: Option<SEXP>, rho: SEXP) -> SEXP {
    arg.map_or_else(R_NilValue, |expr| crate::eval::eval::Rf_eval(expr, rho))
}

fn names_from_save_list(list: SEXP) -> Vec<String> {
    if list == R_NilValue() {
        return Vec::new();
    }
    if TYPEOF(list) != SEXPTYPE::STRSXP {
        error("'list' must be a character vector");
    }
    let mut names = Vec::new();
    for i in 0..XLENGTH(list) {
        let charsxp = STRING_ELT(list, i);
        if charsxp.is_null() || charsxp == R_NaString() {
            error("'list' contains missing object names");
        }
        names.push(
            String::from_utf8_lossy(&crate::sexp::accessors::char_cstr_bytes(charsxp)).into_owned(),
        );
    }
    names
}

fn workspace_version(version: SEXP) -> c_int {
    if version.is_null() || version == R_NilValue() {
        return defaultSaveVersion();
    }
    let version = crate::mainutils::coerce::asInteger(version);
    if !matches!(version, 2 | 3) {
        error(&format!("version {version} not supported"));
    }
    version
}

fn workspace_flag(value: SEXP, default: bool) -> bool {
    if value.is_null() || value == R_NilValue() {
        return default;
    }
    match crate::mainutils::coerce::asLogical(value) {
        0 => false,
        1 => true,
        _ => error("NA in coercion to boolean"),
    }
}

unsafe fn workspace_codec(value: SEXP, ascii: bool) -> Option<&'static str> {
    if value.is_null() {
        return (!ascii).then_some("gzip");
    }
    match SEXPTYPE::from(TYPEOF(value)) {
        SEXPTYPE::LGLSXP if XLENGTH(value) == 1 => workspace_flag(value, !ascii).then_some("gzip"),
        SEXPTYPE::STRSXP if XLENGTH(value) == 1 => {
            let name = STRING_ELT(value, 0);
            if name == R_NaString() {
                error("missing value where TRUE/FALSE needed");
            }
            match &crate::sexp::accessors::char_cstr_bytes(name)[..] {
                b"gzip" => Some("gzip"),
                b"bzip2" => Some("bzip2"),
                b"xz" => Some("xz"),
                b"no compression" => None,
                codec => error(&format!(
                    "'compress = \"{}\"' is invalid",
                    String::from_utf8_lossy(codec)
                )),
            }
        }
        _ => error("'compress' must be logical or character"),
    }
}

fn workspace_owner() -> crate::sexp::owner::OwnerToken<'static> {
    crate::sexp::owner::OwnerToken::current()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// The evaluated .Internal(save()) path writes an uncompressed workspace.
pub unsafe fn do_save(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    crate::mainutils::relop::checkArity(op, args);
    let owner = workspace_owner();
    let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
    let _args = protect(args);
    let names = names_from_save_list(CAR(args));
    let opts = workspace_file::SaveOptions {
        ascii: workspace_flag(CADDR(args), false),
        version: workspace_version(CAR(CDR(CDR(CDR(args))))),
        codec: None,
        eval_promises: workspace_flag(CAR(CDR(CDR(CDR(CDR(CDR(args)))))), true),
    };
    workspace_file::save(owner, &names, CADR(args), CAD5R(args), opts);
    R_NilValue()
}

/// User-facing save() resolves every requested object before publishing bytes.
pub unsafe fn do_save_user(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner = workspace_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let _args = protect(args);
        let _rho = protect(rho);
        let (
            [
                list,
                file,
                ascii,
                version,
                envir,
                compress,
                compression_level,
                eval_promises,
                _precheck,
            ],
            dots,
        ) = save_arguments(args);
        if list.is_none() && dots.is_empty() {
            error("nothing specified to be save()d");
        }
        let mut names = if list.is_some() {
            names_from_save_list(eval_save_argument(list, rho))
        } else {
            Vec::new()
        };
        names.extend(dots);
        let file_expr = file.unwrap_or_else(|| error("'file' must be specified"));
        let file = eval_save_argument(Some(file_expr), rho);
        let _file = protect(file);
        let envir = if envir.is_some() {
            eval_save_argument(envir, rho)
        } else {
            rho
        };
        let _envir = protect(envir);
        let version = workspace_version(eval_save_argument(version, rho));
        let ascii = workspace_flag(eval_save_argument(ascii, rho), false);
        let codec = if compress.is_some() {
            workspace_codec(eval_save_argument(compress, rho), ascii)
        } else {
            (!ascii).then_some("gzip")
        };
        if codec.is_some() && compression_level.is_some() {
            error("custom workspace compression_level is not supported");
        }
        let eval_promises = workspace_flag(eval_save_argument(eval_promises, rho), true);
        let opts = workspace_file::SaveOptions {
            ascii,
            version,
            codec,
            eval_promises,
        };
        // Always resolve the complete snapshot before replacing a workspace.
        workspace_file::save(owner, &names, file, envir, opts);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

pub unsafe fn do_load(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::relop::checkArity(op, args);
        let owner = workspace_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let _args = protect(args);
        workspace_file::load(owner, CAR(args), CADR(args))
    }
}

/// User-facing load() decodes and validates the snapshot before binding it.
pub unsafe fn do_load_user(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner = workspace_owner();
        let _pin = owner.pin().unwrap_or_else(|e| error(&e.to_string()));
        let _args = protect(args);
        let [file, envir, verbose] = load_arguments(args);
        let verbose = workspace_flag(verbose.unwrap_or(R_NilValue()), false);
        let names = workspace_file::load(owner, file.unwrap_or(R_NilValue()), envir.unwrap_or(rho));
        if verbose {
            let _names = protect(names);
            let mut text = String::from("Loading objects:\n");
            for index in 0..XLENGTH(names) {
                let name = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, index)));
                text.push_str("  ");
                text.push_str(&name.to_string_lossy());
                text.push('\n');
            }
            crate::sexp::output::capture_stdout(&text);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        names
    }
}
