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

thread_local! {
    /// (filename, first-line) of the top-level expression currently being
    /// evaluated, when parsed with srcrefs. Cleared per statement.
    static CURRENT_SRCREF_LOCATION: std::cell::RefCell<Option<(String, i32)>> =
        const { std::cell::RefCell::new(None) };
}

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

/// Build the srcfile environment for a parsed source (class "srcfile",
/// `filename` binding; `lines` left empty — the renderer only reads the
/// filename).
unsafe fn make_srcfile(filename: &str, copy: bool) -> SEXP {
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
        let class = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _cg = crate::sexp::protect::protect(class);
        if copy {
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
        let srcfile = make_srcfile(filename, true);
        let _sf_guard = crate::sexp::protect::protect(srcfile);

        // Upstream layout: the expression VECTOR carries a LIST-valued
        // `srcref` attribute (one entry per expression; per-ELEMENT
        // srcref attributes stay NULL) and the `srcfile` attribute; each
        // srcref also references the srcfile. Eval loops (eval.c) read
        // the vector's list by index.
        let srcref_list =
            crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::VECSXP, spans.len() as i64);
        let _sl_guard = crate::sexp::protect::protect(srcref_list);

        for (i, &(expr, start, end)) in spans.iter().enumerate() {
            let _ = expr;
            let (fl, fc) = line_col(src, start);
            let (ll, lc) = line_col(src, end.saturating_sub(1));
            let srcref = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::INTSXP, 8);
            let _g = crate::sexp::protect::protect(srcref);
            let p = INTEGER(srcref);
            *p.add(0) = fl;
            *p.add(1) = start as i32; // first byte (0-based, upstream)
            *p.add(2) = ll;
            *p.add(3) = end as i32; // last byte
            *p.add(4) = fc;
            *p.add(5) = lc;
            *p.add(6) = 1; // first parsed expression index
            *p.add(7) = 1; // last parsed
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
            crate::sexp::attrib_core::setAttrib(
                srcref,
                crate::sexp::symbol::Rf_install(c"srcfile".as_ptr()),
                srcfile,
            );
            crate::sexp::accessors::SET_VECTOR_ELT(srcref_list, i as i64, srcref);
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
        CURRENT_SRCREF_LOCATION.with(|c| *c.borrow_mut() = loc);
    }
}

/// The `(from ...)` location for the error renderer: `(file, line)` of
/// the current top-level expression when parsed with srcrefs.
pub fn current_srcref_location() -> Option<(String, i32)> {
    CURRENT_SRCREF_LOCATION.with(|c| c.borrow().clone())
}
