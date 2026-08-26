//! Essentials domain module `s3` — extracted verbatim from essentials.rs.

use super::*;
use std::ffi::CString;
use std::os::raw::c_int;

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
use crate::sexp::context::RError;
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// S3: setOldClass, methods
// ---------------------------------------------------------------------------

/// R's `setOldClass(Class)` — register old-style S3 class. Simplified: returns Class.
pub unsafe fn do_setOldClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class_arg = CAR(args);
        if class_arg.is_null() || class_arg == R_NilValue() {
            return R_NilValue();
        }
        class_arg
    }
}

/// R's `methods(generic)` — list methods known to the Rust runtime.
pub unsafe fn do_methods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let generic_arg = CAR(args);
        if generic_arg.is_null() || generic_arg == R_NilValue() {
            return string_vector(&all_runtime_method_names());
        }
        let generic = elt_to_string(generic_arg, 0);
        if generic.is_empty() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let prefix = format!("{generic}.");
        let methods = all_runtime_method_names()
            .into_iter()
            .filter(|name| name.starts_with(&prefix))
            .collect::<Vec<_>>();
        string_vector(&methods)
    }
}

fn all_runtime_method_names() -> Vec<String> {
    let mut methods = crate::eval::builtin::builtin_handler_names()
        .filter(|name| name.contains('.'))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    methods.sort();
    methods.dedup();
    methods
}

// ---------------------------------------------------------------------------
// S3 dispatch, environment functions, I/O extensions
// ---------------------------------------------------------------------------

/// R's `UseMethod(generic, obj)` — delegate to the translated object-system
/// dispatch implementation.
pub unsafe fn do_usemethod(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::objects::do_usemethod(call, op, args, rho) }
}

/// R's `missing(x)` — check if argument was missing in call.
pub unsafe fn do_missing(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        Rf_ScalarLogical(FALSE) // Simplified
    }
}

/// R's `parent.frame(n)` — get enclosing environment.
pub unsafe fn do_parent_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = context_index_arg(args, 1);
        if n == NA_INTEGER || n < 1 {
            base_error("invalid 'n' value");
        }

        let mut remaining = n;
        let mut context = crate::sexp::context::R_GlobalContext();
        while !context.is_null() {
            if (*context).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION != 0 {
                remaining -= 1;
                if remaining == 0 {
                    return (*context).sysparent;
                }
            }
            context = (*context).nextcontext;
        }
        crate::sexp::globals::R_GlobalEnv()
    }
}

/// R's `sys.call(which)` — get the call that's currently being evaluated.
pub unsafe fn do_sys_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let which = context_index_arg(args, 0);
        let top = crate::sexp::context::R_GlobalContext();
        if top.is_null() {
            R_NilValue()
        } else {
            crate::eval::context::R_syscall(which, top)
        }
    }
}

/// R's `sys.frame(which)` — get frame at specified level.
pub unsafe fn do_sys_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let which = context_index_arg(args, 0);
        let top = crate::sexp::context::R_GlobalContext();
        if top.is_null() {
            crate::sexp::globals::R_GlobalEnv()
        } else {
            crate::eval::context::R_sysframe(which, top)
        }
    }
}

pub(crate) unsafe fn current_function_context() -> Option<*mut crate::sexp::context::RCNTXT> {
    unsafe {
        let mut context = crate::sexp::context::R_GlobalContext();
        while !context.is_null() {
            if (*context).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION != 0 {
                return Some(context);
            }
            context = (*context).nextcontext;
        }
        None
    }
}

pub(crate) unsafe fn pairlist_len(mut list: SEXP) -> c_int {
    unsafe {
        let mut len = 0;
        while !list.is_null() && list != R_NilValue() {
            len += 1;
            list = CDR(list);
        }
        len
    }
}

pub(crate) unsafe fn context_index_arg(args: SEXP, default: c_int) -> c_int {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            default
        } else {
            real_or_default(CAR(args), default as f64) as c_int
        }
    }
}

/// R's `getwd()` — get working directory.
pub unsafe fn do_getwd(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        match std::env::current_dir() {
            Ok(path) => {
                let s = path.to_string_lossy();
                let cstr = CString::new(s.as_ref()).unwrap_or_default();
                Rf_mkString(cstr.as_ptr())
            }
            Err(_) => R_NilValue(),
        }
    }
}

/// R's `setwd(dir)` — set working directory.
pub unsafe fn do_setwd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let dir_arg = CAR(args);
        if dir_arg.is_null() {
            return R_NilValue();
        }
        let path = elt_to_string(dir_arg, 0);
        match std::env::set_current_dir(&path) {
            Ok(()) => {
                crate::sexp::globals::set_R_Visible(FALSE);
                let cstr = CString::new(path).unwrap_or_default();
                Rf_mkString(cstr.as_ptr())
            }
            Err(_) => {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: format!("cannot change working directory to '{}'", path),
                });
            }
        }
    }
}

/// R's `basename(path)` — final path component, vectorized over character input.
pub unsafe fn do_basename(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { map_path_strings(CAR(args), r_basename) }
}

/// R's `dirname(path)` — parent path component, vectorized over character input.
pub unsafe fn do_dirname(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { map_path_strings(CAR(args), r_dirname) }
}

/// R's `file.path(...)` — join path components element-wise with recycling.
pub unsafe fn do_file_path(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut parts = Vec::new();
        let mut max_len = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() != Some("fsep") {
                let value = CAR(current);
                if !value.is_null() && value != R_NilValue() {
                    max_len = max_len.max(XLENGTH(value));
                    parts.push(value);
                }
            }
            current = CDR(current);
        }
        if parts.is_empty() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, max_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..max_len {
            let joined = parts
                .iter()
                .filter_map(|part| {
                    let value = elt_to_string(*part, i);
                    (!value.is_empty()).then_some(value)
                })
                .collect::<Vec<_>>()
                .join("/");
            SET_STRING_ELT(
                result,
                i,
                Rf_mkChar(CString::new(joined).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

/// R's `dir.exists(paths)` — check if directories exist.
pub unsafe fn do_dir_exists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let path = elt_to_string(x, i);
            *dst.add(i as usize) = if std::path::Path::new(&path).is_dir() {
                TRUE
            } else {
                FALSE
            };
        }
        result
    }
}

/// R's `file.create(...)` — create empty files.
pub unsafe fn do_file_create(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let path = elt_to_string(x, i);
            *dst.add(i as usize) =
                match crate::mainutils::platform::create_file_with_session_umask(&path) {
                    Ok(_) => TRUE,
                    Err(_) => FALSE,
                };
        }
        result
    }
}

/// R's `unlink(x, recursive)` — delete files or directories.
pub unsafe fn do_unlink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        let n = XLENGTH(x);
        let mut count = 0;
        for i in 0..n {
            let path = elt_to_string(x, i);
            let p = std::path::Path::new(&path);
            let result = if p.is_dir() {
                std::fs::remove_dir_all(p)
            } else {
                std::fs::remove_file(p)
            };
            if result.is_ok() {
                count += 1;
            }
        }
        let result = Rf_ScalarInteger(count);
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

/// R's `nzchar(x)` — check if strings are non-empty.
pub unsafe fn do_nzchar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            *dst.add(i as usize) = if s.is_empty() { FALSE } else { TRUE };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// S3 generics
// ---------------------------------------------------------------------------

/// R's `as.data.frame(x)` — convert to data.frame.
/// Simplified: wraps x in a list with data.frame class.
pub unsafe fn do_as_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // If already a data.frame, return as-is
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            let cls_name = elt_to_string(class, 0);
            if cls_name == "data.frame" {
                return x;
            }
        }
        // Wrap in a single-element list and set class
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        SET_VECTOR_ELT(result, 0, x);

        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class_vec.is_null() {
            let _class_guard = protect(class_vec);
            let cstr = CString::new("data.frame").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*class_vec).gengc_next_node as *mut SEXP;
                *data.add(0) = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class_vec,
            );
        }

        // Set row.names
        let nrow = XLENGTH(x);
        let rn = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !rn.is_null() {
            let _row_names_guard = protect(rn);
            *INTEGER(rn) = NA_INTEGER;
            *INTEGER(rn).add(1) = -(nrow as i32);
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
                rn,
            );
        }

        // Set column name to "x"
        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !names_vec.is_null() {
            let _names_guard = protect(names_vec);
            let cstr = CString::new("x").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*names_vec).gengc_next_node as *mut SEXP;
                *data.add(0) = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names_vec,
            );
        }

        result
    }
}

/// R's `as.list(x)` — generic list conversion.
/// Delegates to do_as_list but available as a separate entry point.
pub unsafe fn do_as_list_generic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_as_list(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// Complete data operations
// ---------------------------------------------------------------------------

/// R's `reshape(x, direction, varying, v.names, timevar, idvar, times)` — reshape data.
///
/// Not ported: this runtime does not implement `reshape()`. Fail loudly
/// instead of silently returning the input unchanged.
pub unsafe fn do_reshape(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    package_error(
        "reshape() is not ported in this pure-R Android runtime; use aggregate(), melt-style helpers, or explicit subsetting/assignment instead",
    )
}

/// R's `complete_cases(...)` — returns logical vector: TRUE where all args are non-NA.
pub unsafe fn do_complete_cases(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Collect all argument vectors
        let mut arg_vecs: Vec<SEXP> = Vec::new();
        let mut max_len: R_xlen_t = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                arg_vecs.push(arg);
                let n = XLENGTH(arg);
                if n > max_len {
                    max_len = n;
                }
            }
            current = CDR(current);
        }
        if arg_vecs.is_empty() || max_len == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, max_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = LOGICAL(result);
        for i in 0..max_len {
            let mut complete = TRUE;
            for &arg in &arg_vecs {
                let n = XLENGTH(arg);
                let idx = if n == 0 { 0 } else { i % n };
                if atomic_value_is_missing(arg, idx) {
                    complete = FALSE;
                    break;
                }
            }
            *dst.add(i as usize) = complete;
        }
        result
    }
}

/// R's `na.omit(x)` — returns x with rows containing any NA removed (simplified: works on vectors).
pub unsafe fn do_na_omit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { na_omit_atomic(args, "omit") }
}

/// R's `na.exclude(x)` — like na.omit with "exclude" na.action metadata.
pub unsafe fn do_na_exclude(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { na_omit_atomic(args, "exclude") }
}

unsafe fn na_omit_atomic(args: SEXP, action_class: &str) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let sexptype = SEXPTYPE(t);
        if !matches!(
            sexptype,
            SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::STRSXP
        ) {
            return x;
        }

        let n = XLENGTH(x);
        let mut keep: Vec<R_xlen_t> = Vec::new();
        let mut dropped: Vec<R_xlen_t> = Vec::new();
        for i in 0..n {
            if atomic_value_is_missing(x, i) {
                dropped.push(i);
            } else {
                keep.push(i);
            }
        }

        let result = Rf_allocVector3(t, keep.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (out, &src) in keep.iter().enumerate() {
            copy_vector_element(result, out as R_xlen_t, x, src, sexptype);
        }
        set_selected_names_attribute(x, result, &keep);
        if !dropped.is_empty() {
            set_na_action_attribute(x, result, &dropped, action_class);
        }
        result
    }
}

unsafe fn set_selected_names_attribute(x: SEXP, result: SEXP, indices: &[R_xlen_t]) {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return;
        }
        let selected = Rf_allocVector3(SEXPTYPE::STRSXP, indices.len() as R_xlen_t);
        if selected.is_null() {
            return;
        }
        let _selected_guard = protect(selected);
        for (out, &src) in indices.iter().enumerate() {
            SET_STRING_ELT(selected, out as R_xlen_t, STRING_ELT(names, src));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            selected,
        );
    }
}

unsafe fn set_na_action_attribute(
    source: SEXP,
    result: SEXP,
    dropped: &[R_xlen_t],
    action_class: &str,
) {
    unsafe {
        let action = Rf_allocVector3(SEXPTYPE::INTSXP, dropped.len() as R_xlen_t);
        if action.is_null() {
            return;
        }
        let _action_guard = protect(action);
        for (out, &src) in dropped.iter().enumerate() {
            *INTEGER(action).add(out) = (src + 1) as c_int;
        }

        let names =
            crate::sexp::attrib_core::getAttrib(source, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            let action_names = Rf_allocVector3(SEXPTYPE::STRSXP, dropped.len() as R_xlen_t);
            if !action_names.is_null() {
                let _names_guard = protect(action_names);
                for (out, &src) in dropped.iter().enumerate() {
                    SET_STRING_ELT(action_names, out as R_xlen_t, STRING_ELT(names, src));
                }
                crate::sexp::attrib_core::setAttrib(
                    action,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    action_names,
                );
            }
        }

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class.is_null() {
            let _class_guard = protect(class);
            SET_STRING_ELT(
                class,
                0,
                Rf_mkChar(CString::new(action_class).unwrap_or_default().as_ptr()),
            );
            crate::sexp::attrib_core::setAttrib(
                action,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"na.action".as_ptr()), action);
    }
}

/// R's `is_complete(x)` — logical vector of complete cases for a single vector.
pub unsafe fn do_is_complete(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let na = atomic_value_is_missing(x, i);
            *dst.add(i as usize) = if na { FALSE } else { TRUE };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete S3
// ---------------------------------------------------------------------------

/// R-like `rownames_to_column(x, var)` — convert row names to a leading column.
pub unsafe fn do_rownames_to_column(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if !is_data_frame_like(x) {
            data_frame_error("rownames_to_column() requires a data frame");
        }
        let var_arg = arg_by_name_or_position(args, &["var"], 1);
        let var = if var_arg.is_null() || var_arg == R_NilValue() || XLENGTH(var_arg) == 0 {
            "rowname".to_string()
        } else {
            elt_to_string(var_arg, 0)
        };

        let mut names = data_frame_column_names(x);
        let mut columns = data_frame_columns(x);
        names.insert(0, var);
        columns.insert(0, string_vector(&data_frame_row_names(x)));
        build_data_frame(columns, names, data_frame_row_names_attr(x))
    }
}

/// R-like `column_to_rownames(x, var)` — convert a column to row names.
pub unsafe fn do_column_to_rownames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if !is_data_frame_like(x) {
            data_frame_error("column_to_rownames() requires a data frame");
        }
        let var_arg = arg_by_name_or_position(args, &["var"], 1);
        let var = if var_arg.is_null() || var_arg == R_NilValue() || XLENGTH(var_arg) == 0 {
            "rowname".to_string()
        } else {
            elt_to_string(var_arg, 0)
        };
        let names = data_frame_column_names(x);
        let Some(row_col) = names.iter().position(|name| name == &var) else {
            data_frame_error(format!("column '{}' not found", var));
        };

        let mut out_names = Vec::new();
        let mut out_columns = Vec::new();
        for (i, name) in names.into_iter().enumerate() {
            if i != row_col {
                out_names.push(name);
                out_columns.push(VECTOR_ELT(x, i as R_xlen_t));
            }
        }
        build_data_frame(
            out_columns,
            out_names,
            string_vector(&vector_to_string_values(VECTOR_ELT(x, row_col as R_xlen_t))),
        )
    }
}

/// R-like `relocate(x, cols, .before, .after)` — reorder data-frame columns.
pub unsafe fn do_relocate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if !is_data_frame_like(x) {
            data_frame_error("relocate() requires a data frame");
        }
        let cols_arg = arg_by_name_or_position(args, &["cols", ".cols"], 1);
        let before_arg = arg_by_name_or_position(args, &[".before", "before"], usize::MAX);
        let after_arg = arg_by_name_or_position(args, &[".after", "after"], usize::MAX);
        let names = data_frame_column_names(x);
        let requested = string_arg_values(cols_arg);
        let moving: Vec<String> = requested
            .into_iter()
            .filter(|name| names.iter().any(|column| column == name))
            .collect();
        if moving.is_empty() {
            return x;
        }

        let mut rest: Vec<String> = names
            .iter()
            .filter(|name| !moving.iter().any(|moving_name| moving_name == *name))
            .cloned()
            .collect();
        let insert_at = if !before_arg.is_null() && before_arg != R_NilValue() {
            let before = elt_to_string(before_arg, 0);
            rest.iter()
                .position(|name| name == &before)
                .unwrap_or(rest.len())
        } else if !after_arg.is_null() && after_arg != R_NilValue() {
            let after = elt_to_string(after_arg, 0);
            rest.iter()
                .position(|name| name == &after)
                .map(|idx| idx + 1)
                .unwrap_or(rest.len())
        } else {
            0
        };
        for (offset, name) in moving.into_iter().enumerate() {
            rest.insert(insert_at + offset, name);
        }

        let mut out_columns = Vec::new();
        for name in &rest {
            if let Some(idx) = names.iter().position(|column| column == name) {
                out_columns.push(VECTOR_ELT(x, idx as R_xlen_t));
            }
        }
        build_data_frame(out_columns, rest, data_frame_row_names_attr(x))
    }
}

pub(crate) fn is_data_frame_like(x: SEXP) -> bool {
    unsafe {
        !x.is_null()
            && x != R_NilValue()
            && TYPEOF(x) == SEXPTYPE::VECSXP
            && is_data_frame_object(x)
    }
}

fn data_frame_column_names(x: SEXP) -> Vec<String> {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return (0..XLENGTH(x)).map(|i| format!("V{}", i + 1)).collect();
        }
        (0..XLENGTH(x)).map(|i| elt_to_string(names, i)).collect()
    }
}

fn data_frame_columns(x: SEXP) -> Vec<SEXP> {
    unsafe { (0..XLENGTH(x)).map(|i| VECTOR_ELT(x, i)).collect() }
}

fn data_frame_row_names_attr(x: SEXP) -> SEXP {
    unsafe {
        crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
        )
    }
}

pub(crate) fn data_frame_row_names(x: SEXP) -> Vec<String> {
    unsafe {
        let attr = data_frame_row_names_attr(x);
        if attr.is_null() || attr == R_NilValue() {
            return (1..=data_frame_row_count(x))
                .map(|i| i.to_string())
                .collect();
        }
        if TYPEOF(attr) == SEXPTYPE::STRSXP {
            return (0..XLENGTH(attr)).map(|i| elt_to_string(attr, i)).collect();
        }
        if TYPEOF(attr) == SEXPTYPE::INTSXP && LENGTH(attr) == 2 {
            let first = *INTEGER(attr);
            let second = *INTEGER(attr).add(1);
            if first == NA_INTEGER && second < 0 {
                return (1..=(-second as R_xlen_t)).map(|i| i.to_string()).collect();
            }
        }
        vector_to_string_values(attr)
    }
}

fn vector_to_string_values(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        (0..XLENGTH(x)).map(|i| elt_to_string(x, i)).collect()
    }
}

fn string_arg_values(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        (0..XLENGTH(x)).map(|i| elt_to_string(x, i)).collect()
    }
}

pub(crate) unsafe fn condition_message_text(args: SEXP, option_names: &[&str]) -> String {
    unsafe {
        let mut parts = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            let is_option = tag_name(current)
                .as_deref()
                .is_some_and(|name| option_names.contains(&name));
            if !is_option && !arg.is_null() && arg != R_NilValue() {
                for i in 0..XLENGTH(arg) {
                    parts.push(elt_to_string(arg, i));
                }
            }
            current = CDR(current);
        }
        parts.join("")
    }
}

fn build_data_frame(columns: Vec<SEXP>, names: Vec<String>, row_names: SEXP) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, columns.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, column) in columns.into_iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, column);
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            string_vector(&names),
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            Rf_mkString(CString::new("data.frame").unwrap_or_default().as_ptr()),
        );
        if !row_names.is_null() && row_names != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
                row_names,
            );
        }
        result
    }
}

fn data_frame_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    })
}

// ---------------------------------------------------------------------------
// Internal helpers for new functions
// ---------------------------------------------------------------------------

/// Extract numeric data from a SEXP into a Vec<f64>.
pub(crate) fn get_numeric_data(x: SEXP) -> Vec<f64> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let mut data = Vec::with_capacity(n as usize);
        if t == SEXPTYPE::REALSXP {
            for i in 0..n {
                data.push(*REAL(x).add(i as usize));
            }
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            for i in 0..n {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER {
                    data.push(NA_REAL);
                } else {
                    data.push(v as f64);
                }
            }
        }
        data
    }
}

/// Extract a single element from a vector as a SEXP (for use with real_or_default).
pub(crate) fn elt_to_sexp(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };

        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(idx as usize);
            Rf_ScalarReal(v)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            Rf_ScalarInteger(*INTEGER(x).add(idx as usize))
        } else {
            R_NilValue()
        }
    }
}

unsafe fn constructor_length(value: SEXP) -> R_xlen_t {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return 0;
        }
        if XLENGTH(value) == 0 {
            return 0;
        }

        let raw_len = match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => *INTEGER(value),
            t if t == SEXPTYPE::REALSXP => {
                let value = *REAL(value);
                if value.is_nan() || value < 0.0 {
                    base_error("invalid 'length' argument");
                }
                value.trunc() as i32
            }
            t if t == SEXPTYPE::STRSXP => elt_to_string(value, 0)
                .parse::<i32>()
                .unwrap_or_else(|_| base_error("invalid 'length' argument")),
            _ => base_error("invalid 'length' argument"),
        };

        if raw_len == NA_INTEGER || raw_len < 0 {
            base_error("invalid 'length' argument");
        }
        raw_len as R_xlen_t
    }
}

unsafe fn first_constructor_arg(args: SEXP, name: &str, position: usize) -> SEXP {
    unsafe {
        let mut current = args;
        let mut positional = 0;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match tag_name(current).as_deref() {
                Some(tag) if tag == name => return value,
                Some(_) => {}
                None => {
                    if positional == position {
                        return value;
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }
        R_NilValue()
    }
}

unsafe fn allocate_initialized_vector(sexptype: SEXPTYPE, length: R_xlen_t) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(sexptype, length);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        match sexptype {
            t if t == SEXPTYPE::STRSXP => {
                let empty = Rf_mkChar(c"".as_ptr());
                for i in 0..length {
                    SET_STRING_ELT(result, i, empty);
                }
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                for i in 0..length {
                    SET_VECTOR_ELT(result, i, R_NilValue());
                }
            }
            _ => {}
        }
        result
    }
}

unsafe fn do_typed_vector_constructor(args: SEXP, sexptype: SEXPTYPE) -> SEXP {
    unsafe {
        let length_arg = first_constructor_arg(args, "length", 0);
        let length = constructor_length(length_arg);
        allocate_initialized_vector(sexptype, length)
    }
}

/// R's `logical(length = 0)` constructor.
pub unsafe fn do_logical_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::LGLSXP) }
}

/// R's `integer(length = 0)` constructor.
pub unsafe fn do_integer_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::INTSXP) }
}

/// R's `numeric(length = 0)` / `double(length = 0)` constructor.
pub unsafe fn do_numeric_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::REALSXP) }
}

/// R's legacy `single(length = 0)` constructor.
pub unsafe fn do_single_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let result = do_typed_vector_constructor(args, SEXPTYPE::REALSXP);
        if result.is_null() || result == R_NilValue() {
            return result;
        }

        let _result_guard = protect(result);
        let marker = Rf_ScalarLogical(TRUE);
        let _marker_guard = protect(marker);
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"Csingle".as_ptr()), marker);
        result
    }
}

/// R's `complex(length = 0)` constructor.
pub unsafe fn do_complex_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::CPLXSXP) }
}

/// R's `character(length = 0)` constructor.
pub unsafe fn do_character_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::STRSXP) }
}

/// R's `raw(length = 0)` constructor.
pub unsafe fn do_raw_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::RAWSXP) }
}

/// R's `vector(mode = "logical", length = 0)` constructor.
pub unsafe fn do_vector_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mode_arg = first_constructor_arg(args, "mode", 0);
        let length_arg = first_constructor_arg(args, "length", 1);
        let mode = if mode_arg.is_null() || mode_arg == R_NilValue() {
            "logical".to_string()
        } else {
            elt_to_string(mode_arg, 0)
        };
        let sexptype = match mode.as_str() {
            "logical" => SEXPTYPE::LGLSXP,
            "integer" => SEXPTYPE::INTSXP,
            "numeric" | "double" => SEXPTYPE::REALSXP,
            "complex" => SEXPTYPE::CPLXSXP,
            "character" => SEXPTYPE::STRSXP,
            "raw" => SEXPTYPE::RAWSXP,
            "list" => SEXPTYPE::VECSXP,
            "expression" => SEXPTYPE::EXPRSXP,
            _ => base_error(format!("vector: cannot make a vector of mode '{mode}'")),
        };
        let length = constructor_length(length_arg);
        allocate_initialized_vector(sexptype, length)
    }
}

// ---------------------------------------------------------------------------
// Complete S3 coercion — as.complex, as.raw, as
// ---------------------------------------------------------------------------

/// R's `as.complex(x)` — coerce to CPLXSXP through the shared vector coercer.
pub unsafe fn do_as_complex(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::mainutils::coerce::coerceVector(x, SEXPTYPE::CPLXSXP.as_c_int())
    }
}

/// R's `as.raw(x)` — coerce to RAWSXP.
pub unsafe fn do_as_raw(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::RAWSXP, 0);
        }
        crate::mainutils::coerce::coerceVector(x, SEXPTYPE::RAWSXP.as_c_int())
    }
}

/// R's `as(x, Class)` — S4-style coercion (simplified: delegates to appropriate as.* function).
pub unsafe fn do_as(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || class_arg.is_null() || class_arg == R_NilValue() {
            return x;
        }
        let class_name = elt_to_string(class_arg, 0);
        match class_name.as_str() {
            "numeric" | "double" => do_as_double(_call, _op, args, _rho),
            "integer" => do_as_integer(_call, _op, args, _rho),
            "logical" => do_as_logical(_call, _op, args, _rho),
            "character" => do_as_character(_call, _op, args, _rho),
            "complex" => do_as_complex(_call, _op, args, _rho),
            "raw" => do_as_raw(_call, _op, args, _rho),
            "list" => do_as_list(_call, _op, args, _rho),
            _ => x, // unknown class, return as-is
        }
    }
}

// ---------------------------------------------------------------------------
// Complete data operations — subset
// ---------------------------------------------------------------------------

/// R's `subset(x, subset, select, drop)` — subset data.frame (simplified).
/// Already defined as do_subset above — this is an alias with named args.
pub unsafe fn do_subset_named(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Delegate to existing do_subset
        do_subset(_call, _op, args, _rho)
    }
}

// ---------------------------------------------------------------------------
// Complete S3 — method dispatch
// ---------------------------------------------------------------------------

/// R's `getS3method(generic, class)` — get S3 method function.
pub unsafe fn do_getS3method(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let generic = elt_to_string(CAR(args), 0);
        let class = elt_to_string(CAR(CDR(args)), 0);
        let Some(method_sym) = crate::mainutils::objects::s3_method_symbol(&generic, &class) else {
            return R_NilValue();
        };
        let method = crate::mainutils::objects::lookup_s3_method_symbol(
            method_sym,
            rho,
            rho,
            effective_s3_defrho(rho),
        );
        if is_function_value(method) {
            method
        } else {
            R_NilValue()
        }
    }
}

/// R's `hasS3method(generic, class)` — check if S3 method exists.
pub unsafe fn do_hasS3method(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let generic = elt_to_string(CAR(args), 0);
        let class = elt_to_string(CAR(CDR(args)), 0);
        let Some(method_sym) = crate::mainutils::objects::s3_method_symbol(&generic, &class) else {
            return Rf_ScalarLogical(FALSE);
        };
        let method = crate::mainutils::objects::lookup_s3_method_symbol(
            method_sym,
            rho,
            rho,
            effective_s3_defrho(rho),
        );
        Rf_ScalarLogical(if is_function_value(method) {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `registerS3method(generic, class, method)` — register S3 method.
pub unsafe fn do_registerS3method(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let generic = elt_to_string(CAR(args), 0);
        let class = elt_to_string(CAR(CDR(args)), 0);
        let method = CAR(CDR(CDR(args)));
        let env_arg = CDR(CDR(CDR(args)));
        let target_env = if !env_arg.is_null() && env_arg != R_NilValue() {
            CAR(env_arg)
        } else {
            rho
        };

        if let Err(message) = define_s3_method(target_env, &generic, &class, method) {
            package_error(message);
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

pub(crate) unsafe fn s3_methods_table_symbol() -> SEXP {
    unsafe { crate::mainutils::objects::S3MethodsTable_symbol() }
}

unsafe fn ensure_s3_methods_table(env: SEXP) -> Result<SEXP, String> {
    unsafe {
        if env.is_null() || env == R_NilValue() || TYPEOF(env) != SEXPTYPE::ENVSXP {
            return Err("S3 method registration requires an environment".to_string());
        }

        let table_sym = s3_methods_table_symbol();
        let existing = crate::sexp::envir::R_findVarInFrame(env, table_sym);
        if !existing.is_null()
            && existing != crate::sexp::globals::R_UnboundValue()
            && TYPEOF(existing) == SEXPTYPE::ENVSXP
        {
            return Ok(existing);
        }

        let table = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        if table.is_null() {
            return Err("could not create S3 methods table".to_string());
        }
        let _table_guard = crate::sexp::protect::protect(table);
        crate::sexp::envir::defineVar(table_sym, table, env);
        Ok(table)
    }
}

pub(crate) unsafe fn define_s3_method(
    env: SEXP,
    generic: &str,
    class: &str,
    method: SEXP,
) -> Result<(), String> {
    unsafe {
        if !is_function_value(method) {
            return Err(format!(
                "S3 method '{}.{}' must be a function",
                generic, class
            ));
        }
        let Some(method_sym) = crate::mainutils::objects::s3_method_symbol(generic, class) else {
            return Err(format!(
                "invalid S3 method signature '{}.{}'",
                generic, class
            ));
        };
        let table = ensure_s3_methods_table(env)?;
        crate::sexp::envir::defineVar(method_sym, method, table);
        Ok(())
    }
}

unsafe fn effective_s3_defrho(rho: SEXP) -> SEXP {
    unsafe {
        if rho.is_null() || rho == R_NilValue() || TYPEOF(rho) != SEXPTYPE::ENVSXP {
            crate::sexp::globals::R_GlobalEnv()
        } else {
            let namespace_env = crate::sexp::envir::R_findVarInFrame(rho, namespace_env_symbol());
            if !namespace_env.is_null()
                && namespace_env != crate::sexp::globals::R_UnboundValue()
                && TYPEOF(namespace_env) == SEXPTYPE::ENVSXP
            {
                namespace_env
            } else {
                rho
            }
        }
    }
}

pub(crate) unsafe fn is_function_value(value: SEXP) -> bool {
    unsafe {
        !value.is_null()
            && value != R_NilValue()
            && value != crate::sexp::globals::R_UnboundValue()
            && {
                let value_type = TYPEOF(value);
                value_type == SEXPTYPE::CLOSXP
                    || value_type == SEXPTYPE::BUILTINSXP
                    || value_type == SEXPTYPE::SPECIALSXP
            }
    }
}

unsafe fn initialize_generic_dispatch_tables(generic: SEXP) {
    unsafe {
        if generic.is_null() || generic == R_NilValue() || TYPEOF(generic) != SEXPTYPE::CLOSXP {
            return;
        }

        let f_env = crate::sexp::accessors::CLOENV(generic);
        if f_env.is_null() || TYPEOF(f_env) != SEXPTYPE::ENVSXP {
            return;
        }

        let all_mtable_sym = Rf_install(c".AllMTable".as_ptr());
        let existing_mtable = crate::sexp::envir::R_findVarInFrame(f_env, all_mtable_sym);
        if existing_mtable == R_UnboundValue() || TYPEOF(existing_mtable) != SEXPTYPE::ENVSXP {
            let mtable = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), R_NilValue(), f_env);
            if !mtable.is_null() {
                crate::sexp::envir::defineVar(all_mtable_sym, mtable, f_env);
            }
        }

        let sig_args_sym = Rf_install(c".SigArgs".as_ptr());
        let sig_length_sym = Rf_install(c".SigLength".as_ptr());
        let existing_sig_args = crate::sexp::envir::R_findVarInFrame(f_env, sig_args_sym);
        if existing_sig_args != R_UnboundValue() {
            return;
        }

        let mut formals = FORMALS(generic);
        let mut arg_syms = Vec::new();
        while !formals.is_null() && formals != R_NilValue() {
            let tag = TAG(formals);
            if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                arg_syms.push(tag);
            }
            formals = CDR(formals);
        }

        let sigargs = Rf_allocVector3(SEXPTYPE::VECSXP, arg_syms.len() as R_xlen_t);
        if sigargs.is_null() {
            return;
        }
        for (index, sym) in arg_syms.iter().enumerate() {
            SET_VECTOR_ELT(sigargs, index as R_xlen_t, *sym);
        }
        crate::sexp::envir::defineVar(sig_args_sym, sigargs, f_env);
        crate::sexp::envir::defineVar(
            sig_length_sym,
            Rf_ScalarInteger(arg_syms.len() as c_int),
            f_env,
        );
    }
}

/// R's `setGeneric(f, fdef, ...)` — set generic function.
pub unsafe fn do_setGeneric(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let f_arg = CAR(args);
        let fdef_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        let generic = if !fdef_arg.is_null() && fdef_arg != R_NilValue() {
            fdef_arg
        } else {
            f_arg
        };

        initialize_generic_dispatch_tables(generic);

        // Upstream setGeneric rebinds the name in the defining environment to
        // the generic closure and marks it as an object carrying the "generic"
        // attribute, so standardGeneric can find it via get_this_generic.
        if !generic.is_null() && generic != R_NilValue() && TYPEOF(generic) == SEXPTYPE::CLOSXP {
            let _generic_guard = protect(generic);

            // Resolve the generic's name: explicit first argument, else the
            // symbol this closure is bound to in the defining environment.
            let mut name_sexp: Option<String> = None;
            if !f_arg.is_null()
                && f_arg != R_NilValue()
                && (TYPEOF(f_arg) == SEXPTYPE::STRSXP || TYPEOF(f_arg) == SEXPTYPE::SYMSXP)
            {
                name_sexp = Some(elt_to_string(f_arg, 0));
            } else {
                let target_env =
                    if !rho.is_null() && rho != R_NilValue() && TYPEOF(rho) == SEXPTYPE::ENVSXP {
                        rho
                    } else {
                        crate::sexp::globals::R_GlobalEnv()
                    };
                let name_sym = find_binding_name_for_value(target_env, generic);
                if let Some(nm) = name_sym {
                    let pname = PRINTNAME(nm);
                    if !pname.is_null() && pname != R_NilValue() {
                        name_sexp = Some(elt_to_string(pname, 0));
                    }
                }
            }

            if let Some(name) = name_sexp.filter(|n| !n.is_empty()) {
                let cname = CString::new(name.clone()).unwrap_or_default();
                let name_sym = Rf_install(cname.as_ptr());
                let name_str = Rf_mkString(cname.as_ptr());

                // Mark the closure as a generic function object: attribute
                // "generic" = the generic's name, plus the OBJECT bit.
                let generic_sym = Rf_install(c"generic".as_ptr());
                crate::eval::attrib_core::setAttrib(generic, generic_sym, name_str);
                crate::sexp::accessors::SET_OBJECT(generic, 1);

                // Rebind the name to the generic closure in the calling frame.
                let target_env =
                    if !rho.is_null() && rho != R_NilValue() && TYPEOF(rho) == SEXPTYPE::ENVSXP {
                        rho
                    } else {
                        crate::sexp::globals::R_GlobalEnv()
                    };
                let existing = crate::sexp::envir::R_findVarInFrame(target_env, name_sym);
                let already_generic = !existing.is_null()
                    && existing != R_UnboundValue()
                    && is_function_type(existing)
                    && crate::eval::attrib_core::isObject(existing) != FALSE;
                if !already_generic {
                    crate::sexp::envir::defineVar(name_sym, generic, target_env);
                }
            }
        }

        generic
    }
}

unsafe fn is_function_type(value: SEXP) -> bool {
    unsafe {
        !value.is_null()
            && value != R_NilValue()
            && value != R_UnboundValue()
            && matches!(TYPEOF(value), t if SEXPTYPE::CLOSXP == t
                || SEXPTYPE::BUILTINSXP == t
                || SEXPTYPE::SPECIALSXP == t)
    }
}

/// Search an environment's frame (and hash chain) for a symbol whose binding
/// holds exactly `value`; returns the first matching symbol.
unsafe fn find_binding_name_for_value(env: SEXP, value: SEXP) -> Option<SEXP> {
    unsafe {
        if env.is_null() || TYPEOF(env) != SEXPTYPE::ENVSXP {
            return None;
        }
        let frame = FRAME(env);
        let mut cell = frame;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            if !tag.is_null() && tag != R_NilValue() && CAR(cell) == value {
                return Some(tag);
            }
            cell = CDR(cell);
        }

        let hashtab = HASHTAB(env);
        if !hashtab.is_null() && hashtab != R_NilValue() {
            let n = XLENGTH(hashtab);
            for i in 0..n {
                let bucket = VECTOR_ELT(hashtab, i);
                let mut entry = bucket;
                while !entry.is_null() && entry != R_NilValue() {
                    let tag = TAG(entry);
                    if !tag.is_null() && tag != R_NilValue() && CAR(entry) == value {
                        return Some(tag);
                    }
                    entry = CDR(entry);
                }
            }
        }
        None
    }
}

/// R's `setMethod(f, signature, definition, ...)` — set S4 method.
pub unsafe fn do_setMethod(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let f_arg = CAR(args);
        let signature_arg = CAR(CDR(args));
        let definition = CAR(CDR(CDR(args)));

        if definition.is_null() || definition == R_NilValue() {
            return R_NilValue();
        }

        let generic = if TYPEOF(f_arg) == SEXPTYPE::CLOSXP {
            f_arg
        } else if TYPEOF(f_arg) == SEXPTYPE::STRSXP || TYPEOF(f_arg) == SEXPTYPE::SYMSXP {
            crate::library::methods::methods_list_dispatch::R_getGenericByName(
                f_arg,
                Rf_ScalarLogical(TRUE),
                rho,
                R_NilValue(),
            )
        } else {
            return definition;
        };

        if generic.is_null() || generic == R_NilValue() || TYPEOF(generic) != SEXPTYPE::CLOSXP {
            return definition;
        }

        let f_env = crate::sexp::accessors::CLOENV(generic);
        if f_env.is_null() || TYPEOF(f_env) != SEXPTYPE::ENVSXP {
            return definition;
        }

        let all_mtable_sym = Rf_install(c".AllMTable".as_ptr());
        let existing = crate::sexp::envir::R_findVarInFrame(f_env, all_mtable_sym);
        let mtable = if !existing.is_null()
            && existing != R_UnboundValue()
            && TYPEOF(existing) == SEXPTYPE::ENVSXP
        {
            existing
        } else {
            let table = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), R_NilValue(), f_env);
            if table.is_null() {
                return definition;
            }
            let _table_guard = protect(table);
            crate::sexp::envir::defineVar(all_mtable_sym, table, f_env);
            table
        };

        let label = if signature_arg.is_null() || signature_arg == R_NilValue() {
            String::new()
        } else if TYPEOF(signature_arg) == SEXPTYPE::STRSXP {
            let n = XLENGTH(signature_arg);
            (0..n)
                .map(|i| elt_to_string(signature_arg, i))
                .collect::<Vec<_>>()
                .join("#")
        } else {
            elt_to_string(signature_arg, 0)
        };

        let method_sym = Rf_install(CString::new(label).unwrap_or_default().as_ptr());
        crate::sexp::envir::defineVar(method_sym, definition, mtable);

        definition
    }
}
