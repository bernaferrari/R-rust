#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's `mapply` builtin.
//!
//! `mapply` applies a function to multiple lists/vectors in parallel,
//! recycling shorter arguments to match the longest.
//!
//! Upstream wires `do_mapply` behind the R-level closure
//! `mapply(FUN, ..., MoreArgs=NULL, SIMPLIFY=TRUE, USE.NAMES=TRUE)`, which
//! match-args before calling `.Internal(mapply(FUN, dots, MoreArgs))`. This
//! engine registers the builtin directly, so `do_mapply` receives the user's
//! evaluated argument pairlist with tags intact and must do the closure's
//! matching itself:
//!
//! - exactly-named cells bind `FUN` / `MoreArgs` / `SIMPLIFY` / `USE.NAMES`
//!   (matching happens before positional filling, like `matchArgs`);
//! - remaining positional cells fill `FUN` first (when it was not named),
//!   then land in `...` (the varyings), because `...` precedes `MoreArgs`.
//!
//! With a named `FUN` — e.g. whisker's `mapply(values, renders, FUN=f)` —
//! the old implementation read `fun = CAR(args)` (the first varying) and let
//! the closure value fall into the varyings. The varying-length pass then
//! ran `XLENGTH` on the CLOSXP, whose union stores `{formals, body, env}`
//! where a vector stores `{length, truelength}`: the formals pointer was
//! read as the length (~1e14), and the application loop ran (and
//! allocated) essentially forever. Lengths here must therefore never read
//! the vector header of a non-vector node.

use std::ptr;

use crate::sexp::accessors::{
    CAR, CDR, SET_VECTOR_ELT, SETCDR, SETTAG, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::constructors::{Rf_allocVector3, Rf_cons};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{ProtectGuard, protect};

/// True for types whose payload really is `{length, truelength}` followed by
/// element data, so `XLENGTH` is meaningful.
fn is_vector_type(t: SEXPTYPE) -> bool {
    t == SEXPTYPE::VECSXP
        || t == SEXPTYPE::EXPRSXP
        || t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::RAWSXP
}

/// Length of a varying argument without ever reading the vector header of a
/// non-vector node.
///
/// Pairlist-family nodes are walked; vector types use `XLENGTH`; `NULL` is
/// length 0; any other object (closure, environment, symbol, ...) is a
/// scalar of length 1, matching R's `length()` contract for non-vectors.
unsafe fn varying_length(v: SEXP) -> R_xlen_t {
    unsafe {
        if v.is_null() || v == R_NilValue() {
            return 0;
        }
        let t = SEXPTYPE(TYPEOF(v));
        if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP || t == SEXPTYPE::DOTSXP {
            let mut len: R_xlen_t = 0;
            let mut p = v;
            while !p.is_null() && p != R_NilValue() {
                len += 1;
                p = CDR(p);
            }
            len
        } else if is_vector_type(t) {
            XLENGTH(v)
        } else {
            1
        }
    }
}

/// Element `idx` of a varying argument, recycled by the caller.
///
/// Vector and pairlist inputs reuse the shared extractor (which boxes atomic
/// elements); a non-vector object is the whole argument (scalar semantics).
unsafe fn varying_element(v: SEXP, idx: R_xlen_t) -> SEXP {
    unsafe {
        let t = SEXPTYPE(TYPEOF(v));
        if t == SEXPTYPE::DOTSXP {
            let mut p = v;
            let mut k: R_xlen_t = 0;
            while !p.is_null() && p != R_NilValue() && k < idx {
                k += 1;
                p = CDR(p);
            }
            if p.is_null() || p == R_NilValue() {
                R_NilValue()
            } else {
                CAR(p)
            }
        } else if t == SEXPTYPE::LISTSXP
            || t == SEXPTYPE::LANGSXP
            || t == SEXPTYPE::VECSXP
            || t == SEXPTYPE::EXPRSXP
            || is_vector_type(t)
        {
            crate::mainutils::essentials::extract_element(v, idx)
        } else {
            v
        }
    }
}

/// `mapply(FUN, ..., MoreArgs=NULL, SIMPLIFY=TRUE, USE.NAMES=TRUE)` — apply
/// a function to multiple lists/vectors, recycling the shorter ones.
///
/// Returns the unsimplified list of results (the engine's `mapply` contract;
/// the closure-level `SIMPLIFY`/`USE.NAMES` post-processing is not applied).
pub unsafe fn do_mapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // --- Pass 1: exactly-named formals bind by tag. ---
        let mut fun: SEXP = ptr::null_mut();
        let mut fun_named = false;
        let mut moreargs = R_NilValue();
        let mut ap = args;
        while !ap.is_null() && ap != R_NilValue() {
            if let Some(tag) = crate::mainutils::essentials::tag_name(ap) {
                match tag.as_str() {
                    "FUN" => {
                        fun = CAR(ap);
                        fun_named = true;
                    }
                    "MoreArgs" => moreargs = CAR(ap),
                    // Consumed by the R-level closure upstream; nothing to do
                    // for the unsimplified engine result.
                    "SIMPLIFY" | "USE.NAMES" => {}
                    _ => {}
                }
            }
            ap = CDR(ap);
        }

        // --- Pass 2: positionals fill FUN (if unnamed), then `...`. ---
        // Varyings keep their dots tag so the applied call can name them.
        let mut varyings: Vec<(SEXP, SEXP)> = Vec::new();
        let mut ap = args;
        while !ap.is_null() && ap != R_NilValue() {
            match crate::mainutils::essentials::tag_name(ap).as_deref() {
                None => {
                    if !fun_named {
                        fun = CAR(ap);
                        fun_named = true;
                    } else {
                        varyings.push((CAR(ap), TAG(ap)));
                    }
                }
                Some("FUN") | Some("MoreArgs") | Some("SIMPLIFY") | Some("USE.NAMES") => {}
                Some(_) => varyings.push((CAR(ap), TAG(ap))),
            }
            ap = CDR(ap);
        }

        if fun.is_null() || fun == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }

        // --- Lengths: any zero-length varying short-circuits to list(). ---
        let lengths: Vec<R_xlen_t> = varyings.iter().map(|&(v, _)| varying_length(v)).collect();
        let mut longest: R_xlen_t = 0;
        let mut zero = false;
        for &l in &lengths {
            if l == 0 {
                zero = true;
            }
            if l > longest {
                longest = l;
            }
        }
        if longest == 0 || zero {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }

        // --- MoreArgs: a vector list (names become tags) or a pairlist. ---
        let mut more_elts: Vec<(SEXP, SEXP)> = Vec::new();
        if !moreargs.is_null() && moreargs != R_NilValue() {
            let t = TYPEOF(moreargs);
            if t == SEXPTYPE::VECSXP {
                let names = crate::sexp::attrib_core::getAttrib(
                    moreargs,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                );
                for j in 0..XLENGTH(moreargs) {
                    let tag = if !names.is_null()
                        && names != R_NilValue()
                        && TYPEOF(names) == SEXPTYPE::STRSXP
                        && j < XLENGTH(names)
                    {
                        let charsxp = crate::sexp::accessors::STRING_ELT(names, j);
                        let chars = crate::sexp::accessors::CHAR(charsxp);
                        if !chars.is_null() && unsafe { *chars != 0 } {
                            crate::sexp::symbol::Rf_install(chars)
                        } else {
                            R_NilValue()
                        }
                    } else {
                        R_NilValue()
                    };
                    more_elts.push((VECTOR_ELT(moreargs, j), tag));
                }
            } else if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::DOTSXP {
                let mut p = moreargs;
                while !p.is_null() && p != R_NilValue() {
                    more_elts.push((CAR(p), TAG(p)));
                    p = CDR(p);
                }
            } else {
                crate::sexp::context::r_error(
                    "argument 'MoreArgs' of 'mapply' is not a list or pairlist",
                );
            }
        }

        // --- Everything live across Rf_eval must be a GC root. ---
        let _fun_guard = protect(fun);
        let _more_guard = protect(moreargs);
        let mut varying_guards: Vec<ProtectGuard> = Vec::new();
        for &(v, _) in &varyings {
            varying_guards.push(protect(v));
        }

        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, longest);
        let _ans_guard = protect(ans);

        for i in 0..longest {
            // Build the call FUN(v1[i], v2[i], ..., MoreArgs...) fresh, like
            // upstream rebuilds its dots[[j]][[counter]] call per iteration.
            let mut call_args = R_NilValue();
            let mut tail: SEXP = ptr::null_mut();
            let mut cell_guards: Vec<ProtectGuard> = Vec::new();

            let push_cell = |elt: SEXP,
                             tag: SEXP,
                             call_args: &mut SEXP,
                             tail: &mut SEXP,
                             guards: &mut Vec<ProtectGuard>| {
                let cell = Rf_cons(elt, R_NilValue());
                guards.push(protect(cell));
                if !tag.is_null() && tag != R_NilValue() {
                    SETTAG(cell, tag);
                }
                if *call_args == R_NilValue() {
                    *call_args = cell;
                } else {
                    SETCDR(*tail, cell);
                }
                *tail = cell;
            };

            for (k, &(v, vtag)) in varyings.iter().enumerate() {
                let idx = i % lengths[k];
                push_cell(
                    varying_element(v, idx),
                    vtag,
                    &mut call_args,
                    &mut tail,
                    &mut cell_guards,
                );
            }
            for &(val, mtag) in &more_elts {
                push_cell(val, mtag, &mut call_args, &mut tail, &mut cell_guards);
            }

            let call = Rf_cons(fun, call_args);
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            let _call_guard = protect(call);
            let val = crate::eval::eval::Rf_eval(call, rho);
            SET_VECTOR_ELT(ans, i, val);
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::REAL;
    use crate::sexp::constructors::Rf_mkString;
    use crate::sexp::globals::R_GlobalEnv;
    use std::os::raw::c_int;

    /// Evaluate an R expression string in the global environment.
    unsafe fn eval_r(code: &str) -> SEXP {
        unsafe {
            let ccode = std::ffi::CString::new(code).expect("nul-free code");
            let text = Rf_mkString(ccode.as_ptr());
            let _text_guard = protect(text);
            let mut status: c_int = 0;
            let srcfile = Rf_mkString(c"<text>".as_ptr());
            let _srcfile_guard = protect(srcfile);
            let parsed = crate::mainutils::gram_main::R_ParseVector(text, -1, &mut status, srcfile);
            assert_eq!(status, 1, "parse failed for {code:?}");
            let _parsed_guard = protect(parsed);
            crate::eval::eval::Rf_eval(VECTOR_ELT(parsed, 0), R_GlobalEnv())
        }
    }

    /// Collect the numeric elements of a VECSXP result of length-1 REALSXPs.
    unsafe fn real_elements(x: SEXP) -> Vec<f64> {
        unsafe {
            let n = XLENGTH(x);
            (0..n).map(|i| *REAL(VECTOR_ELT(x, i)) as f64).collect()
        }
    }

    #[test]
    fn test_do_mapply_null_args_returns_empty() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let empty_list = Rf_allocVector3(SEXPTYPE::LISTSXP, 0);
            let args = crate::sexp::memory_ext::allocList(3);
            crate::sexp::accessors::SETCAR(args, R_NilValue());
            crate::sexp::accessors::SETCAR(CDR(args), empty_list);
            crate::sexp::accessors::SETCAR(CDR(CDR(args)), R_NilValue());

            let result = do_mapply(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(XLENGTH(result), 0);
        }
    }

    #[test]
    fn test_named_fun_one_varying_recycles() {
        // mapply(c(1,2), FUN=function(a) a+10) -> list(11, 12)
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let fun = eval_r("function(a) a + 10");
            let _fun_guard = protect(fun);
            let vec = eval_r("c(1,2)");
            let _vec_guard = protect(vec);

            let args = crate::sexp::memory_ext::allocList(2);
            crate::sexp::accessors::SETCAR(args, vec);
            crate::sexp::accessors::SETTAG(args, R_NilValue());
            crate::sexp::accessors::SETCAR(CDR(args), fun);
            crate::sexp::accessors::SETTAG(
                CDR(args),
                crate::sexp::symbol::Rf_install(c"FUN".as_ptr()),
            );

            let result = do_mapply(ptr::null_mut(), ptr::null_mut(), args, R_GlobalEnv());
            let _result_guard = protect(result);
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(real_elements(result), vec![11.0, 12.0]);
        }
    }

    #[test]
    fn test_named_fun_two_varyings() {
        // whisker's shape: mapply(values, renders, FUN=function(value, render) ...)
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let fun = eval_r("function(value, render) render(value)");
            let _fun_guard = protect(fun);
            let values = eval_r("list(\"World\", \"!\")");
            let _values_guard = protect(values);
            let renders =
                eval_r("list(function(x) paste0(x, \"?\"), function(x) paste0(x, \"!\"))");
            let _renders_guard = protect(renders);

            let args = crate::sexp::memory_ext::allocList(3);
            crate::sexp::accessors::SETCAR(args, values);
            crate::sexp::accessors::SETTAG(args, R_NilValue());
            crate::sexp::accessors::SETCAR(CDR(args), renders);
            crate::sexp::accessors::SETTAG(CDR(args), R_NilValue());
            crate::sexp::accessors::SETCAR(CDR(CDR(args)), fun);
            crate::sexp::accessors::SETTAG(
                CDR(CDR(args)),
                crate::sexp::symbol::Rf_install(c"FUN".as_ptr()),
            );

            let result = do_mapply(ptr::null_mut(), ptr::null_mut(), args, R_GlobalEnv());
            let _result_guard = protect(result);
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            let s = |i: i64| {
                crate::sexp::accessors::CHAR(crate::sexp::accessors::STRING_ELT(
                    VECTOR_ELT(result, i),
                    0,
                ))
            };
            assert_eq!(std::ffi::CStr::from_ptr(s(0)).to_bytes(), b"World?");
            assert_eq!(std::ffi::CStr::from_ptr(s(1)).to_bytes(), b"!!");
        }
    }

    #[test]
    fn test_positional_fun_two_varyings() {
        // mapply(function(a,b) a+b, c(1,2), c(3,4)) -> list(4, 6)
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let fun = eval_r("function(a, b) a + b");
            let _fun_guard = protect(fun);
            let x = eval_r("c(1,2)");
            let _x_guard = protect(x);
            let y = eval_r("c(3,4)");
            let _y_guard = protect(y);

            let args = crate::sexp::memory_ext::allocList(3);
            crate::sexp::accessors::SETCAR(args, fun);
            crate::sexp::accessors::SETTAG(args, R_NilValue());
            crate::sexp::accessors::SETCAR(CDR(args), x);
            crate::sexp::accessors::SETTAG(CDR(args), R_NilValue());
            crate::sexp::accessors::SETCAR(CDR(CDR(args)), y);
            crate::sexp::accessors::SETTAG(CDR(CDR(args)), R_NilValue());

            let result = do_mapply(ptr::null_mut(), ptr::null_mut(), args, R_GlobalEnv());
            let _result_guard = protect(result);
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(real_elements(result), vec![4.0, 6.0]);
        }
    }

    #[test]
    fn test_named_fun_with_moreargs() {
        // mapply(c(1,2), FUN=function(a, b) a+b, MoreArgs=list(100)) -> list(101, 102)
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let fun = eval_r("function(a, b) a + b");
            let _fun_guard = protect(fun);
            let vec = eval_r("c(1,2)");
            let _vec_guard = protect(vec);
            let more = eval_r("list(100)");
            let _more_guard = protect(more);

            let args = crate::sexp::memory_ext::allocList(3);
            crate::sexp::accessors::SETCAR(args, vec);
            crate::sexp::accessors::SETTAG(args, R_NilValue());
            crate::sexp::accessors::SETCAR(CDR(args), fun);
            crate::sexp::accessors::SETTAG(
                CDR(args),
                crate::sexp::symbol::Rf_install(c"FUN".as_ptr()),
            );
            crate::sexp::accessors::SETCAR(CDR(CDR(args)), more);
            crate::sexp::accessors::SETTAG(
                CDR(CDR(args)),
                crate::sexp::symbol::Rf_install(c"MoreArgs".as_ptr()),
            );

            let result = do_mapply(ptr::null_mut(), ptr::null_mut(), args, R_GlobalEnv());
            let _result_guard = protect(result);
            assert_eq!(real_elements(result), vec![101.0, 102.0]);
        }
    }

    #[test]
    fn test_zero_length_varying_returns_empty_list() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let fun = eval_r("function(a) a");
            let _fun_guard = protect(fun);
            let vec = eval_r("numeric(0)");
            let _vec_guard = protect(vec);

            let args = crate::sexp::memory_ext::allocList(2);
            crate::sexp::accessors::SETCAR(args, vec);
            crate::sexp::accessors::SETTAG(args, R_NilValue());
            crate::sexp::accessors::SETCAR(CDR(args), fun);
            crate::sexp::accessors::SETTAG(
                CDR(args),
                crate::sexp::symbol::Rf_install(c"FUN".as_ptr()),
            );

            let result = do_mapply(ptr::null_mut(), ptr::null_mut(), args, R_GlobalEnv());
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(XLENGTH(result), 0);
        }
    }
}
