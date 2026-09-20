//! srcref attribution (keep.source = TRUE).
//!
//! Upstream parse() with keep.source attaches to every top-level
//! expression an `srcref` attribute (an 8-integer vector of class
//! "srcref": first line/byte, last line/byte, first/last column,
//! first/last parsed) and to the expression vector a `srcfile`
//! environment carrying the filename. When `show.error.locations` is
//! on, error headers render `(from <file>#<line>)` from the srcref of
//! the evaluating top-level expression (`(from #n)` when the srcref
//! has no filename — upstream GetSrcLoc on an unnamed srcfile).

use crate::sexp::accessors::*;
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::ffi::{SEXP, SEXPTYPE};

/// Byte offset -> (1-based line, 1-based column) in `src`.
fn line_col(src: &str, byte: usize) -> (i32, i32) {
    let bytes = src.as_bytes();
    let mut line = 1i32;
    let mut last_nl = -1i64;
    for (i, &b) in bytes.iter().enumerate().take(byte.min(bytes.len())) {
        if b == b'\n' {
            line += 1;
            last_nl = i as i64;
        }
    }
    let col = (byte as i64 - last_nl).max(1) as i32;
    (line, col)
}

/// GNU 8-integer lloc for a byte span. Safe to call while the parse arena
/// is held; allocation happens on the caller.
pub(crate) fn srcref_lloc(src: &str, start: usize, end: usize) -> [i32; 8] {
    let (fl, fc) = line_col(src, start);
    let (ll, lc) = line_col(src, end.saturating_sub(1));
    [fl, fc, ll, lc, fc, lc, fl, ll]
}


/// Build the srcfile environment. `copy` produces class `srcfilecopy`
/// with a `lines` binding so `as.character.srcref` can recover text
/// (GNU `srcfilecopy()`, used by `source(textConnection, keep.source)`).
unsafe fn make_srcfile(filename: &str, copy: bool, src: &str) -> SEXP {
    unsafe {
        let env = crate::sexp::memory_ext::NewEnvironment(
            std::ptr::null_mut(),
            crate::sexp::globals::R_EmptyEnv(),
            std::ptr::null_mut(),
        );
        let _g = crate::sexp::protect::protect(env);
        let fname = Rf_mkString(
            std::ffi::CString::new(filename)
                .unwrap_or_default()
                .as_ptr(),
        );
        let _fg = crate::sexp::protect::protect(fname);
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(c"filename".as_ptr()),
            fname,
            env,
        );
        if copy {
            let mut lines: Vec<&str> = src.split('\n').collect();
            if lines.last() == Some(&"") {
                lines.pop();
            }
            let line_vec = crate::sexp::constructors::Rf_allocVector3(
                SEXPTYPE::STRSXP,
                lines.len() as i64,
            );
            let _lg = crate::sexp::protect::protect(line_vec);
            for (i, line) in lines.iter().enumerate() {
                let c = std::ffi::CString::new(*line).unwrap_or_default();
                SET_STRING_ELT(
                    line_vec,
                    i as i64,
                    crate::sexp::constructors::Rf_mkChar(c.as_ptr()),
                );
            }
            crate::sexp::envir::defineVar(
                crate::sexp::symbol::Rf_install(c"lines".as_ptr()),
                line_vec,
                env,
            );
            let copy_class = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            let _ccg = crate::sexp::protect::protect(copy_class);
            SET_STRING_ELT(
                copy_class,
                0,
                crate::sexp::constructors::Rf_mkChar(c"srcfilecopy".as_ptr()),
            );
            SET_STRING_ELT(
                copy_class,
                1,
                crate::sexp::constructors::Rf_mkChar(c"srcfile".as_ptr()),
            );
            crate::sexp::attrib_core::setAttrib(
                env,
                crate::sexp::attrib_core::R_ClassSymbol(),
                copy_class,
            );
        } else {
            let class = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            let _cg = crate::sexp::protect::protect(class);
            SET_STRING_ELT(
                class,
                0,
                crate::sexp::constructors::Rf_mkChar(c"srcfile".as_ptr()),
            );
            crate::sexp::attrib_core::setAttrib(
                env,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
        env
    }
}

/// GNU 8-integer `srcref`: line/byte/column/parse. Bytes are 1-based
/// columns in the first/last line (`as.character.srcref` substring).
pub(crate) unsafe fn make_srcref(src: &str, start: usize, end: usize, srcfile: SEXP) -> SEXP {
    unsafe {
        let (fl, fc) = line_col(src, start);
        let last = end.saturating_sub(1);
        let (ll, lc) = line_col(src, last);
        let srcref = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::INTSXP, 8);
        let _g = crate::sexp::protect::protect(srcref);
        let p = INTEGER(srcref);
        *p.add(0) = fl;
        *p.add(1) = fc;
        *p.add(2) = ll;
        *p.add(3) = lc;
        *p.add(4) = fc;
        *p.add(5) = lc;
        *p.add(6) = fl;
        *p.add(7) = ll;
        let class = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _cg = crate::sexp::protect::protect(class);
        SET_STRING_ELT(
            class,
            0,
            crate::sexp::constructors::Rf_mkChar(c"srcref".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            srcref,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        if !srcfile.is_null() && srcfile != crate::sexp::globals::R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                srcref,
                crate::sexp::symbol::Rf_install(c"srcfile".as_ptr()),
                srcfile,
            );
        }
        srcref
    }
}


/// Attach srcrefs using explicit byte spans (parser's
/// parse_top_level_with_spans output).
pub(crate) unsafe fn attach_srcrefs_with_spans(
    spans: &[(SEXP, usize, usize)],
    src: &str,
    filename: &str,
    exprs_vector: SEXP,
) {
    unsafe {
        if spans.is_empty() || exprs_vector.is_null() {
            return;
        }
        let srcfile = make_srcfile(filename, true, src);
        let _sf_guard = crate::sexp::protect::protect(srcfile);

        let srcref_list =
            crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::VECSXP, spans.len() as i64);
        let _sl_guard = crate::sexp::protect::protect(srcref_list);

        for (i, &(expr, start, end)) in spans.iter().enumerate() {
            let srcref = make_srcref(src, start, end, srcfile);
            crate::sexp::accessors::SET_VECTOR_ELT(srcref_list, i as i64, srcref);
            attach_srcfile_to_function_srcrefs(expr, srcfile);
        }

        crate::sexp::attrib_core::setAttrib(
            exprs_vector,
            crate::sexp::symbol::Rf_install(c"srcref".as_ptr()),
            srcref_list,
        );
        crate::sexp::attrib_core::setAttrib(
            exprs_vector,
            crate::sexp::symbol::Rf_install(c"srcfile".as_ptr()),
            srcfile,
        );
    }
}

/// GNU parse attaches a `srcref` to each `function()` call. Bind the
/// enclosing `srcfilecopy` so `as.character.srcref` can recover text.
unsafe fn attach_srcfile_to_function_srcrefs(expr: SEXP, srcfile: SEXP) {
    unsafe {
        if expr.is_null() || expr == crate::sexp::globals::R_NilValue() {
            return;
        }
        let ty = TYPEOF(expr);
        if ty == SEXPTYPE::LANGSXP {
            let head = CAR(expr);
            if !head.is_null()
                && TYPEOF(head) == SEXPTYPE::SYMSXP
                && std::ffi::CStr::from_ptr(CHAR(PRINTNAME(head)))
                    .to_bytes()
                    == b"function"
            {
                let sr = CADDDR(expr);
                if !sr.is_null()
                    && sr != crate::sexp::globals::R_NilValue()
                    && TYPEOF(sr) == SEXPTYPE::INTSXP
                {
                    let class = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                    let _cg = crate::sexp::protect::protect(class);
                    SET_STRING_ELT(
                        class,
                        0,
                        crate::sexp::constructors::Rf_mkChar(c"srcref".as_ptr()),
                    );
                    crate::sexp::attrib_core::setAttrib(
                        sr,
                        crate::sexp::attrib_core::R_ClassSymbol(),
                        class,
                    );
                    crate::sexp::accessors::SET_OBJECT(sr, 1);
                    crate::sexp::attrib_core::setAttrib(
                        sr,
                        crate::sexp::symbol::Rf_install(c"srcfile".as_ptr()),
                        srcfile,
                    );
                }
            }
            let mut cell = CDR(expr);
            while !cell.is_null() && cell != crate::sexp::globals::R_NilValue() {
                attach_srcfile_to_function_srcrefs(CAR(cell), srcfile);
                cell = CDR(cell);
            }
        } else if ty == SEXPTYPE::LISTSXP {
            let mut cell = expr;
            while !cell.is_null() && cell != crate::sexp::globals::R_NilValue() {
                attach_srcfile_to_function_srcrefs(CAR(cell), srcfile);
                cell = CDR(cell);
            }
        } else if ty == SEXPTYPE::VECSXP || ty == SEXPTYPE::EXPRSXP {
            let n = XLENGTH(expr);
            for i in 0..n {
                attach_srcfile_to_function_srcrefs(VECTOR_ELT(expr, i), srcfile);
            }
        }
    }
}


/// Record the srcref location of the top-level expression about to be
/// evaluated (None clears it) for the error renderer.
pub(crate) fn set_current_srcref_location(expr: SEXP, vector: SEXP, index: usize) {
    unsafe {
        let _ = expr;
        let loc = if vector.is_null() || vector == crate::sexp::globals::R_NilValue() {
            None
        } else {
            // Upstream eval.c reads the expression vector's srcref list
            // by index (per-element attributes are NULL).
            let srcref_list = crate::sexp::attrib_core::getAttrib(
                vector,
                crate::sexp::symbol::Rf_install(c"srcref".as_ptr()),
            );
            let srcref = if !srcref_list.is_null()
                && srcref_list != crate::sexp::globals::R_NilValue()
                && TYPEOF(srcref_list) == SEXPTYPE::VECSXP
                && (index as i64) < XLENGTH(srcref_list)
            {
                crate::sexp::accessors::VECTOR_ELT(srcref_list, index as i64)
            } else {
                crate::sexp::globals::R_NilValue()
            };
            if srcref.is_null()
                || srcref == crate::sexp::globals::R_NilValue()
                || TYPEOF(srcref) != SEXPTYPE::INTSXP
                || XLENGTH(srcref) < 3
            {
                None
            } else {
                let line = INTEGER_ELT(srcref, 2);
                if line <= 0 {
                    None
                } else {
                    let srcfile = crate::sexp::attrib_core::getAttrib(
                        srcref,
                        crate::sexp::symbol::Rf_install(c"srcfile".as_ptr()),
                    );
                    let filename =
                        if !srcfile.is_null() && srcfile != crate::sexp::globals::R_NilValue() {
                            let fname = crate::sexp::envir::R_findVarInFrame(
                                srcfile,
                                crate::sexp::symbol::Rf_install(c"filename".as_ptr()),
                            );
                            if !fname.is_null()
                                && fname != crate::sexp::globals::R_NilValue()
                                && TYPEOF(fname) == SEXPTYPE::STRSXP
                                && XLENGTH(fname) > 0
                            {
                                let cs = STRING_ELT(fname, 0);
                                if !cs.is_null() {
                                    std::ffi::CStr::from_ptr(CHAR(cs))
                                        .to_string_lossy()
                                        .into_owned()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                    Some((filename, line))
                }
            }
        };
        crate::sexp::instance::with_required_current_instance(|inst| unsafe {
            (*inst).error_state.current_srcref_location = loc;
        });
    }
}

/// The `(from ...)` location for the error renderer: `(file, line)` of
/// the current top-level expression when parsed with srcrefs.
pub fn current_srcref_location() -> Option<(String, i32)> {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.current_srcref_location.clone()
    })
}

/// GNU `utils::removeSource(fn)` — drop srcref/srcfile attributes.
pub unsafe fn do_remove_source(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == crate::sexp::globals::R_NilValue() {
            crate::sexp::context::r_error(
                "argument is not a function or language object:NULL",
            );
        }
        let ty = TYPEOF(x);
        if ty == SEXPTYPE::CLOSXP {
            strip_function_source(x)
        } else if ty == SEXPTYPE::BUILTINSXP || ty == SEXPTYPE::SPECIALSXP {
            x
        } else if ty == SEXPTYPE::SYMSXP || ty == SEXPTYPE::LANGSXP || ty == SEXPTYPE::EXPRSXP {
            recurse_remove_source(x)
        } else {
            let kind = unsafe {
                std::ffi::CStr::from_ptr(crate::mainutils::util_main::type2char(TYPEOF(x)))
                    .to_string_lossy()
            };

            crate::sexp::context::r_error(&format!(
                "argument is not a function or language object:{kind}"
            ));
        }
    }
}


unsafe fn strip_function_source(fun: SEXP) -> SEXP {
    unsafe {
        clear_source_attrs(fun);
        let formals = FORMALS(fun);
        if !formals.is_null() && formals != crate::sexp::globals::R_NilValue() {
            SET_FORMALS(fun, recurse_remove_source(formals));
        }
        let body = BODY(fun);
        if !body.is_null() && body != crate::sexp::globals::R_NilValue() {
            clear_source_attrs(body);
            SET_BODY(fun, recurse_remove_source(body));
        }
        fun
    }
}

unsafe fn recurse_remove_source(part: SEXP) -> SEXP {
    unsafe {
        if part.is_null() || part == crate::sexp::globals::R_NilValue() {
            return part;
        }
        if TYPEOF(part) == SEXPTYPE::SYMSXP {
            return part;
        }
        if crate::mainutils::essentials::sexp_has_class(part, "srcref") {
            return crate::sexp::globals::R_NilValue();
        }
        clear_source_attrs(part);
        let ty = TYPEOF(part);
        if ty == SEXPTYPE::LISTSXP || ty == SEXPTYPE::LANGSXP || ty == SEXPTYPE::EXPRSXP {
            if ty == SEXPTYPE::EXPRSXP {
                for i in 0..XLENGTH(part) {
                    SET_VECTOR_ELT(part, i, recurse_remove_source(VECTOR_ELT(part, i)));
                }
            } else {
                let mut cell = part;
                while !cell.is_null() && cell != crate::sexp::globals::R_NilValue() {
                    SETCAR(cell, recurse_remove_source(CAR(cell)));
                    cell = CDR(cell);
                }
            }
        }
        part
    }
}

unsafe fn clear_source_attrs(x: SEXP) {
    unsafe {
        let nil = crate::sexp::globals::R_NilValue();
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"srcref".as_ptr()),
            nil,
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"wholeSrcref".as_ptr()),
            nil,
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"srcfile".as_ptr()),
            nil,
        );
    }
}

