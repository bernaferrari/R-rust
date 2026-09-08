//! `source`, `sys.source`, `demo`, `example`.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

use super::eval::parse_source_expression_vector;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete R runtime — source, sys.source, demo, example
// ---------------------------------------------------------------------------

/// R's `source(file, local, echo, ...)` — evaluate an R script file.
pub unsafe fn do_source(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("source: no file specified");
            return R_NilValue();
        }
        let file_path = elt_to_string(file_arg, 0);

        match std::fs::read_to_string(&file_path) {
            Ok(content) => eval_source_text_with_name(&content, rho, &file_path),
            Err(e) => {
                base_error(format!("cannot open file '{}': {}", file_path, e));
            }
        }
    }
}

/// R's `sys.source(file, envir, ...)` — source an R file into a specific environment.
pub unsafe fn do_sys_source(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let envir_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("sys.source: no file specified");
            return R_NilValue();
        }
        let file_path = elt_to_string(file_arg, 0);
        let target_env = if !envir_arg.is_null() && envir_arg != R_NilValue() {
            envir_arg
        } else {
            rho
        };

        match std::fs::read_to_string(&file_path) {
            Ok(content) => eval_source_text_with_name(&content, target_env, &file_path),
            Err(e) => {
                base_error(format!("cannot open file '{}': {}", file_path, e));
            }
        }
    }
}

unsafe fn eval_source_text_with_name(content: &str, env: SEXP, filename: &str) -> SEXP {
    unsafe {
        // keep.source = TRUE: parse with byte spans and attach srcrefs
        // (upstream source() keeps source refs when the option is on, so
        // show.error.locations renders `(from <file>#<line>)`).
        let keep_source = {
            let opt = crate::mainutils::options::GetOption1(crate::sexp::symbol::Rf_install(
                c"keep.source".as_ptr(),
            ));
            !opt.is_null() && crate::mainutils::coerce::asLogical(opt) == 1
        };
        if keep_source {
            let spans = crate::sexp::memory::with_arena(|arena| {
                let mut parser = crate::eval::parser::Parser::new(content, arena);
                parser
                    .parse_top_level_with_spans()
                    .map_err(|e| e.to_string())
            });
            let mut result = R_NilValue();
            if let Ok(spans) = spans {
                let exprs: Vec<SEXP> = spans.iter().map(|&(e, _, _)| e).collect();
                let vec_sexp = crate::sexp::constructors::Rf_allocVector3(
                    crate::sexp::ffi::SEXPTYPE::EXPRSXP,
                    exprs.len() as i64,
                );
                let _vg = crate::sexp::protect::protect(vec_sexp);
                for (i, &e) in exprs.iter().enumerate() {
                    crate::sexp::accessors::SET_VECTOR_ELT(vec_sexp, i as i64, e);
                }
                crate::mainutils::srcref::attach_srcrefs_with_spans(
                    &spans, content, filename, vec_sexp,
                );
                for (i, &(expr, _, _)) in spans.iter().enumerate() {
                    let _ = i;
                    if expr.is_null() || expr == R_NilValue() {
                        continue;
                    }
                    crate::mainutils::srcref::set_current_srcref_location(
                        expr, vec_sexp, i as usize,
                    );
                    result = crate::eval::eval::Rf_eval(expr, env);
                }
                crate::mainutils::srcref::set_current_srcref_location(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                );
            }
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return result;
        }
        let parsed = parse_source_expression_vector(content);
        // do_eval()-style element-wise evaluation: Rf_eval returns an
        // expression vector unchanged, so source() walks the statements
        // itself (eval.c eval expression loop).
        let result = if parsed.is_null() || parsed == R_NilValue() {
            R_NilValue()
        } else {
            let mut result = R_NilValue();
            let n = XLENGTH(parsed);
            for i in 0..n {
                let element = VECTOR_ELT(parsed, i);
                if element.is_null() || element == R_NilValue() {
                    continue;
                }
                result = crate::eval::eval::Rf_eval(element, env);
            }
            result
        };
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}

unsafe fn eval_source_text(content: &str, env: SEXP) -> SEXP {
    unsafe { eval_source_text_with_name(content, env, "") }
}

/// R's `demo(topic, ...)` — run a demo (simplified).
pub unsafe fn do_demo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let topic_arg = CAR(args);
        if topic_arg.is_null() || topic_arg == R_NilValue() {
            eprintln!("demo: no topic specified");
            return R_NilValue();
        }
        let topic = elt_to_string(topic_arg, 0);
        // Look for demo in common locations
        let demo_path = find_package_demo(&topic);
        if demo_path.is_empty() {
            eprintln!("No demo available for topic '{}'", topic);
            return R_NilValue();
        }
        match std::fs::read_to_string(&demo_path) {
            Ok(_content) => {
                eprintln!("Demo for topic: {}", topic);
                // In a full impl, parse and eval demo content
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                R_NilValue()
            }
            Err(e) => {
                eprintln!("Error reading demo '{}': {}", topic, e);
                R_NilValue()
            }
        }
    }
}

/// R's `example(topic, ...)` — run an example (simplified).
pub unsafe fn do_example(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let topic_arg = CAR(args);
        if topic_arg.is_null() || topic_arg == R_NilValue() {
            eprintln!("example: no topic specified");
            return R_NilValue();
        }
        let topic = elt_to_string(topic_arg, 0);
        // Look for examples in common locations
        let example_path = find_package_example(&topic);
        if example_path.is_empty() {
            eprintln!("No examples available for topic '{}'", topic);
            return R_NilValue();
        }
        match std::fs::read_to_string(&example_path) {
            Ok(_content) => {
                eprintln!("Examples for topic: {}", topic);
                // In a full impl, parse and eval example content
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                R_NilValue()
            }
            Err(e) => {
                eprintln!("Error reading example '{}': {}", topic, e);
                R_NilValue()
            }
        }
    }
}
