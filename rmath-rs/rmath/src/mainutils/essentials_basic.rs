use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    CAR, CDR, CHAR, FRAME, INTEGER, LENGTH, LOGICAL, PRINTNAME, RAW, REAL, SET_ATTRIB,
    SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocList, Rf_allocVector3, Rf_cons,
    Rf_mkChar, Rf_mkString,
};
use crate::sexp::ffi::{
    FALSE, ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use crate::mainutils::essentials::elt_to_string;

// ---------------------------------------------------------------------------
// do_paste / do_paste0 — string concatenation
// ---------------------------------------------------------------------------

/// R's `paste(..., sep=" ")` — concatenates vectors element-wise with recycling.
pub unsafe fn do_paste(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_paste_impl(args, " ", false) }
}

/// R's `paste0(...)` — same as paste with sep="".
pub unsafe fn do_paste0(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_paste_impl(args, "", true) }
}

unsafe fn do_paste_impl(args: SEXP, default_sep: &str, paste0: bool) -> SEXP {
    unsafe {
        // Collect all args, find max length
        let mut arg_vecs: Vec<SEXP> = Vec::new();
        let mut max_len: R_xlen_t = 0;
        let mut sep = default_sep.to_string();
        let mut collapse: Option<String> = None;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            match arg_tag_name(current).as_deref() {
                Some("sep") if !paste0 => sep = elt_to_string(arg, 0),
                Some("collapse") => collapse = Some(elt_to_string(arg, 0)),
                _ => {
                    if !arg.is_null() && arg != R_NilValue() {
                        // Upstream paste() coerces every `...` argument with
                        // as.character before .Internal paste: a list arg
                        // contributes its elements ("list(\"a\")" columns),
                        // not its type code. Whisker assembles output with
                        // paste over an as.list()'d character vector.
                        let arg = if TYPEOF(arg) == SEXPTYPE::VECSXP
                            || TYPEOF(arg) == SEXPTYPE::EXPRSXP
                        {
                            crate::mainutils::coerce::coerceVector(arg, SEXPTYPE::STRSXP.as_c_int())
                        } else {
                            arg
                        };
                        let _arg_guard = protect(arg);
                        arg_vecs.push(arg);
                        let n = XLENGTH(arg);
                        if n > max_len {
                            max_len = n;
                        }
                    }
                }
            }
            current = CDR(current);
        }

        if arg_vecs.is_empty() || max_len == 0 {
            if collapse.is_some() {
                let s = c"";
                return Rf_mkString(s.as_ptr());
            }
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, max_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        for i in 0..max_len {
            let mut parts: Vec<String> = Vec::new();
            for &arg in &arg_vecs {
                let n = XLENGTH(arg);
                let idx = if n == 0 { 0 } else { i % n };
                let s = elt_to_string(arg, idx);
                parts.push(s);
            }
            let joined = parts.join(&sep);
            let cstr = CString::new(joined).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                SET_STRING_ELT(result, i, charsxp);
            }
        }

        if let Some(collapse) = collapse {
            let collapsed = (0..max_len)
                .map(|i| elt_to_string(result, i))
                .collect::<Vec<_>>()
                .join(&collapse);
            let cstr = CString::new(collapsed).unwrap_or_default();
            let out = Rf_mkString(cstr.as_ptr());
            return out;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_cat — print to stdout
// ---------------------------------------------------------------------------

/// R's `cat(..., file="", sep=" ", append=FALSE)` for stdout or file paths.
pub unsafe fn do_cat(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut sep = " ".to_string();
        let mut file: Option<String> = None;
        let mut append = false;
        let mut parts: Vec<String> = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            match arg_tag_name(current).as_deref() {
                Some("sep") => sep = elt_to_string(arg, 0),
                Some("file") => {
                    let path = elt_to_string(arg, 0);
                    if path.is_empty() {
                        file = None;
                    } else {
                        file = Some(path);
                    }
                }
                Some("append") => {
                    if !arg.is_null() && arg != R_NilValue() && XLENGTH(arg) > 0 {
                        let value =
                            if TYPEOF(arg) == SEXPTYPE::LGLSXP || TYPEOF(arg) == SEXPTYPE::INTSXP {
                                *INTEGER(arg)
                            } else {
                                FALSE
                            };
                        append = value != FALSE && value != NA_INTEGER;
                    }
                }
                _ => {
                    if !arg.is_null() && arg != R_NilValue() {
                        let n = XLENGTH(arg).max(1);
                        for i in 0..n {
                            parts.push(elt_to_string(arg, i));
                        }
                    }
                }
            }
            current = CDR(current);
        }
        let output = parts.join(&sep);
        if let Some(path) = file {
            if let Ok(mut handle) = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(append)
                .truncate(!append)
                .open(path)
            {
                use std::io::Write;
                let _ = handle.write_all(output.as_bytes());
            }
        } else if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&output);
        } else {
            print!("{}", output);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_print — basic print
// ---------------------------------------------------------------------------

/// R's `print(x)` — basic print with newline. Returns x invisibly.
pub unsafe fn do_print(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout("NULL\n");
            } else {
                println!("NULL");
            }
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return R_NilValue();
        }
        // Upstream `print` is `function(x, ...) UseMethod("print")`:
        // dispatch to a class method when one is registered as a closure
        // (loaded-package methods like R6's print.R6ClassGenerator); the
        // port's built-in print.<class> primitives keep the paths below.
        if let Some(result) =
            crate::mainutils::essentials::apply_s3_closure_method("print", _call, args, _rho)
        {
            return result;
        }
        if crate::mainutils::essentials::sexp_has_class(x, "data.frame") {
            return crate::mainutils::essentials::do_print_data_frame(_call, _op, args, _rho);
        }
        if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            crate::sexp::output::print_value(sexp);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

// ---------------------------------------------------------------------------
// do_typeof — type name
// ---------------------------------------------------------------------------

/// R's `typeof(x)` — returns the type name as STRSXP.
pub unsafe fn do_typeof(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            let s = c"NULL";
            return Rf_mkString(s.as_ptr());
        }
        let name = match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP => "logical",
            t if t == SEXPTYPE::INTSXP => "integer",
            t if t == SEXPTYPE::REALSXP => "double",
            t if t == SEXPTYPE::CPLXSXP => "complex",
            t if t == SEXPTYPE::STRSXP => "character",
            t if t == SEXPTYPE::RAWSXP => "raw",
            t if t == SEXPTYPE::VECSXP => "list",
            t if t == SEXPTYPE::EXPRSXP => "expression",
            t if t == SEXPTYPE::LISTSXP => "pairlist",
            t if t == SEXPTYPE::LANGSXP => "language",
            t if t == SEXPTYPE::SYMSXP => "symbol",
            t if t == SEXPTYPE::CLOSXP => "closure",
            t if t == SEXPTYPE::BUILTINSXP => "builtin",
            t if t == SEXPTYPE::SPECIALSXP => "special",
            t if t == SEXPTYPE::ENVSXP => "environment",
            t if t == SEXPTYPE::NILSXP => "NULL",
            t if t == SEXPTYPE::OBJSXP => {
                // R_typeToChar: distinguish S4 objects from bare OBJSXP
                // (e.g. S7 objects constructed via .OBJSXP()).
                if crate::mainutils::coerce::IS_S4_OBJECT(x) != 0 {
                    "S4"
                } else {
                    "object"
                }
            }
            t if t == SEXPTYPE::CHARSXP => "character",
            _ => "unknown",
        };
        let s = CString::new(name).unwrap_or_default();
        Rf_mkString(s.as_ptr())
    }
}

/// R's `mode(x)` — user-facing object mode.
pub unsafe fn do_mode(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let s = CString::new(mode_name(x)).unwrap_or_default();
        Rf_mkString(s.as_ptr())
    }
}

/// R's `storage.mode(x)` — underlying storage mode.
pub unsafe fn do_storage_mode_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let s = CString::new(storage_mode_name(x)).unwrap_or_default();
        Rf_mkString(s.as_ptr())
    }
}

/// R's `identity(x)` — return the object unchanged.
pub unsafe fn do_identity(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(args) }
}

fn mode_name(x: SEXP) -> &'static str {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return "NULL";
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP => "logical",
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP => "numeric",
            t if t == SEXPTYPE::CPLXSXP => "complex",
            t if t == SEXPTYPE::STRSXP || t == SEXPTYPE::CHARSXP => "character",
            t if t == SEXPTYPE::RAWSXP => "raw",
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::LISTSXP => "list",
            t if t == SEXPTYPE::EXPRSXP => "expression",
            t if t == SEXPTYPE::SYMSXP => "name",
            t if t == SEXPTYPE::LANGSXP => "call",
            t if t == SEXPTYPE::CLOSXP
                || t == SEXPTYPE::BUILTINSXP
                || t == SEXPTYPE::SPECIALSXP =>
            {
                "function"
            }
            t if t == SEXPTYPE::ENVSXP => "environment",
            _ => "unknown",
        }
    }
}

fn storage_mode_name(x: SEXP) -> &'static str {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return "NULL";
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP => "logical",
            t if t == SEXPTYPE::INTSXP => "integer",
            t if t == SEXPTYPE::REALSXP => "double",
            t if t == SEXPTYPE::CPLXSXP => "complex",
            t if t == SEXPTYPE::STRSXP || t == SEXPTYPE::CHARSXP => "character",
            t if t == SEXPTYPE::RAWSXP => "raw",
            t if t == SEXPTYPE::VECSXP => "list",
            t if t == SEXPTYPE::LISTSXP => "pairlist",
            t if t == SEXPTYPE::EXPRSXP => "expression",
            t if t == SEXPTYPE::SYMSXP => "symbol",
            t if t == SEXPTYPE::LANGSXP => "language",
            t if t == SEXPTYPE::CLOSXP
                || t == SEXPTYPE::BUILTINSXP
                || t == SEXPTYPE::SPECIALSXP =>
            {
                "function"
            }
            t if t == SEXPTYPE::ENVSXP => "environment",
            _ => "unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// do_is_na — check for NA
// ---------------------------------------------------------------------------

/// R's `is.na(x)` — returns LGLSXP with TRUE for NA elements.
pub unsafe fn do_is_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = LOGICAL(result);

        for i in 0..n {
            let is_na = if t == SEXPTYPE::REALSXP {
                let v = *REAL(x).add(i as usize);
                v.is_nan()
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                *INTEGER(x).add(i as usize) == NA_INTEGER
            } else if t == SEXPTYPE::STRSXP {
                STRING_ELT(x, i) == crate::sexp::globals::R_NaString()
            } else {
                false
            };
            *dst.add(i as usize) = if is_na { TRUE } else { FALSE };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_names — get/set names attribute
// ---------------------------------------------------------------------------

/// R's `names(x)` — returns the names attribute.
pub unsafe fn do_names(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::ENVSXP {
            return names_from_environment(x);
        }
        if t == SEXPTYPE::LISTSXP {
            return names_from_pairlist(x);
        }
        // Get names attribute
        let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));
        if !names.is_null() && names != R_NilValue() {
            return names;
        }

        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) == 1
        {
            let dimnames = crate::sexp::attrib_core::getAttrib(
                x,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
            );
            if !dimnames.is_null()
                && dimnames != R_NilValue()
                && TYPEOF(dimnames) == SEXPTYPE::VECSXP
                && XLENGTH(dimnames) > 0
            {
                return VECTOR_ELT(dimnames, 0);
            }
        }

        R_NilValue()
    }
}

/// R's `unname(obj, force = FALSE)` — return a copy without names/dimnames.
pub unsafe fn do_unname(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let result = crate::mainutils::duplicate::duplicate(x);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            R_NilValue(),
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            R_NilValue(),
        );
        result
    }
}

/// R's `unclass(obj)` — return a copy with the class attribute removed.
pub unsafe fn do_unclass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let result = crate::mainutils::duplicate::duplicate(x);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            R_NilValue(),
        );
        result
    }
}

unsafe fn names_from_environment(env: SEXP) -> SEXP {
    unsafe {
        let mut names = Vec::new();
        let mut frame = FRAME(env);
        while !frame.is_null() && frame != R_NilValue() {
            if let Some(name) = tag_name(TAG(frame)) {
                names.push(name);
            }
            frame = CDR(frame);
        }
        names.sort();
        string_vector(&names)
    }
}

unsafe fn names_from_pairlist(list: SEXP) -> SEXP {
    unsafe {
        let mut names = Vec::new();
        let mut has_name = false;
        let mut cell = list;
        while !cell.is_null() && cell != R_NilValue() {
            if let Some(name) = tag_name(TAG(cell)) {
                has_name = true;
                names.push(name);
            } else {
                names.push(String::new());
            }
            cell = CDR(cell);
        }
        if has_name {
            string_vector(&names)
        } else {
            R_NilValue()
        }
    }
}

unsafe fn tag_name(tag: SEXP) -> Option<String> {
    unsafe {
        if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return None;
        }
        let print_name = PRINTNAME(tag);
        if print_name.is_null() || print_name == R_NilValue() {
            return None;
        }
        let chars = CHAR(print_name);
        if chars.is_null() {
            return None;
        }
        Some(CStr::from_ptr(chars).to_string_lossy().into_owned())
    }
}

unsafe fn string_vector(names: &[String]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, name) in names.iter().enumerate() {
            let c_name = CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(c_name.as_ptr()));
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_which — find TRUE indices
// ---------------------------------------------------------------------------

/// R's `which(x)` — returns indices of TRUE elements in a logical vector.
pub unsafe fn do_which(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::LGLSXP && t != SEXPTYPE::INTSXP {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }

        let n = XLENGTH(x);
        let mut indices: Vec<i32> = Vec::new();
        for i in 0..n {
            let v = *INTEGER(x).add(i as usize);
            if v != 0 && v != NA_INTEGER {
                indices.push((i + 1) as i32); // R is 1-indexed
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = INTEGER(result);
        for (i, &idx) in indices.iter().enumerate() {
            *dst.add(i) = idx;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_ifelse — vectorized conditional
// ---------------------------------------------------------------------------

/// R's `ifelse(test, yes, no)` — vectorized if/else with recycling.
pub unsafe fn do_ifelse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let test = CAR(args);
        let yes = CAR(CDR(args));
        let no = CAR(CDR(CDR(args)));

        if test.is_null() || yes.is_null() || no.is_null() {
            return R_NilValue();
        }

        let n = XLENGTH(test);

        let result_type = if n == 0 {
            SEXPTYPE::LGLSXP
        } else if TYPEOF(yes) == SEXPTYPE::STRSXP || TYPEOF(no) == SEXPTYPE::STRSXP {
            SEXPTYPE::STRSXP
        } else if TYPEOF(yes) == SEXPTYPE::REALSXP || TYPEOF(no) == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP
        } else if TYPEOF(yes) == SEXPTYPE::LGLSXP && TYPEOF(no) == SEXPTYPE::LGLSXP {
            SEXPTYPE::LGLSXP
        } else {
            SEXPTYPE::INTSXP
        };

        let result = Rf_allocVector3(result_type, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let yes_n = XLENGTH(yes);
        let no_n = XLENGTH(no);

        for i in 0..n {
            let test_value = if TYPEOF(test) == SEXPTYPE::LGLSXP {
                *LOGICAL(test).add(i as usize)
            } else if TYPEOF(test) == SEXPTYPE::INTSXP {
                *INTEGER(test).add(i as usize)
            } else if TYPEOF(test) == SEXPTYPE::REALSXP {
                let v = *REAL(test).add(i as usize);
                if v.is_nan() { NA_LOGICAL } else { v as c_int }
            } else {
                0
            };
            if test_value == NA_LOGICAL {
                set_ifelse_na(result, result_type, i);
                continue;
            }
            let cond = test_value != 0;

            let src = if cond { yes } else { no };
            let src_n = if cond { yes_n } else { no_n };
            if src_n == 0 {
                set_ifelse_na(result, result_type, i);
                continue;
            }
            let src_idx = i % src_n;

            set_ifelse_value(result, result_type, i, src, src_idx);
        }
        result
    }
}

unsafe fn set_ifelse_na(result: SEXP, result_type: SEXPTYPE, index: R_xlen_t) {
    unsafe {
        match result_type {
            SEXPTYPE::STRSXP => {
                SET_STRING_ELT(result, index, crate::sexp::globals::R_NaString());
            }
            SEXPTYPE::REALSXP => *REAL(result).add(index as usize) = NA_REAL,
            SEXPTYPE::LGLSXP => *LOGICAL(result).add(index as usize) = NA_LOGICAL,
            SEXPTYPE::INTSXP => *INTEGER(result).add(index as usize) = NA_INTEGER,
            _ => {}
        }
    }
}

unsafe fn set_ifelse_value(
    result: SEXP,
    result_type: SEXPTYPE,
    out_index: R_xlen_t,
    src: SEXP,
    src_index: R_xlen_t,
) {
    unsafe {
        match result_type {
            SEXPTYPE::STRSXP => {
                let value = if TYPEOF(src) == SEXPTYPE::STRSXP {
                    STRING_ELT(src, src_index)
                } else {
                    let text = elt_to_string(src, src_index);
                    if text == "NA" && source_element_is_na(src, src_index) {
                        crate::sexp::globals::R_NaString()
                    } else {
                        let c_text = CString::new(text).unwrap_or_default();
                        Rf_mkChar(c_text.as_ptr())
                    }
                };
                SET_STRING_ELT(result, out_index, value);
            }
            SEXPTYPE::REALSXP => {
                *REAL(result).add(out_index as usize) = source_element_as_real(src, src_index);
            }
            SEXPTYPE::LGLSXP => {
                *LOGICAL(result).add(out_index as usize) =
                    source_element_as_logical(src, src_index);
            }
            SEXPTYPE::INTSXP => {
                *INTEGER(result).add(out_index as usize) =
                    source_element_as_integer(src, src_index);
            }
            _ => {}
        }
    }
}

unsafe fn source_element_is_na(src: SEXP, index: R_xlen_t) -> bool {
    unsafe {
        match TYPEOF(src) {
            t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {
                *INTEGER(src).add(index as usize) == NA_INTEGER
            }
            t if t == SEXPTYPE::REALSXP => ISNAN(*REAL(src).add(index as usize)),
            t if t == SEXPTYPE::STRSXP => {
                STRING_ELT(src, index) == crate::sexp::globals::R_NaString()
            }
            _ => true,
        }
    }
}

unsafe fn source_element_as_real(src: SEXP, index: R_xlen_t) -> f64 {
    unsafe {
        match TYPEOF(src) {
            t if t == SEXPTYPE::REALSXP => *REAL(src).add(index as usize),
            t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {
                let value = *INTEGER(src).add(index as usize);
                if value == NA_INTEGER {
                    NA_REAL
                } else {
                    value as f64
                }
            }
            _ => NA_REAL,
        }
    }
}

unsafe fn source_element_as_integer(src: SEXP, index: R_xlen_t) -> i32 {
    unsafe {
        match TYPEOF(src) {
            t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {
                *INTEGER(src).add(index as usize)
            }
            t if t == SEXPTYPE::REALSXP => {
                let value = *REAL(src).add(index as usize);
                if ISNAN(value) {
                    NA_INTEGER
                } else {
                    value as i32
                }
            }
            _ => NA_INTEGER,
        }
    }
}

unsafe fn source_element_as_logical(src: SEXP, index: R_xlen_t) -> i32 {
    unsafe {
        match TYPEOF(src) {
            t if t == SEXPTYPE::LGLSXP => *LOGICAL(src).add(index as usize),
            t if t == SEXPTYPE::INTSXP => {
                let value = *INTEGER(src).add(index as usize);
                if value == NA_INTEGER {
                    NA_LOGICAL
                } else {
                    (value != 0) as i32
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let value = *REAL(src).add(index as usize);
                if ISNAN(value) {
                    NA_LOGICAL
                } else {
                    (value != 0.0) as i32
                }
            }
            _ => NA_LOGICAL,
        }
    }
}

fn arg_tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() {
            return None;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return None;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            None
        } else {
            Some(std::ffi::CStr::from_ptr(chars).to_str().ok()?.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// do_table — frequency table
// ---------------------------------------------------------------------------

/// R's `table(...)` — counts occurrences of each unique value.
pub unsafe fn do_table(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::INTSXP
            && t != SEXPTYPE::REALSXP
            && t != SEXPTYPE::LGLSXP
            && t != SEXPTYPE::STRSXP
        {
            return R_NilValue();
        }
        let use_na = table_use_na(args);

        let (labels, counts) = if let Some(levels) = factor_levels(x) {
            let mut counts = vec![0_i64; XLENGTH(levels) as usize];
            let mut na_count = 0_i64;
            for i in 0..XLENGTH(x) {
                let code = *INTEGER(x).add(i as usize);
                if code > 0 && (code as usize) <= counts.len() {
                    counts[(code - 1) as usize] += 1;
                } else if code == NA_INTEGER {
                    na_count += 1;
                }
            }
            let mut labels: Vec<String> = (0..XLENGTH(levels))
                .map(|i| crate::mainutils::essentials::elt_to_string(levels, i))
                .collect();
            if use_na.should_include(na_count) {
                labels.push("<NA>".to_string());
                counts.push(na_count);
            }
            (labels, counts)
        } else {
            let mut counts: BTreeMap<String, i64> = BTreeMap::new();
            for i in 0..XLENGTH(x) {
                let key = crate::mainutils::essentials::elt_to_string(x, i);
                *counts.entry(key).or_insert(0) += 1;
            }
            let (labels, counts): (Vec<String>, Vec<i64>) = counts.into_iter().unzip();
            (labels, counts)
        };

        let len = counts.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, len);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = INTEGER(result);
        for (i, &count) in counts.iter().enumerate() {
            *dst.add(i) = count.min(c_int::MAX as i64) as c_int;
        }

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, len);
        if !names.is_null() {
            let _names_p = protect(names);
            for (i, label) in labels.iter().enumerate() {
                let cstr = CString::new(label.as_str()).unwrap_or_default();
                SET_STRING_ELT(names, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }

        let class = Rf_mkString(c"table".as_ptr());
        if !class.is_null() {
            let _class_p = protect(class);
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
        if let Some(title) = table_title(call, args) {
            let cstr = CString::new(title).unwrap_or_default();
            let title_value = Rf_mkString(cstr.as_ptr());
            let _title_p = protect(title_value);
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(c"table.name".as_ptr()),
                title_value,
            );
        }
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
        if !dim.is_null() {
            let _dim_p = protect(dim);
            *INTEGER(dim) = len as c_int;
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimSymbol(),
                dim,
            );
        }
        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if !dimnames.is_null() {
            let _dimnames_p = protect(dimnames);
            let dim_labels = Rf_allocVector3(SEXPTYPE::STRSXP, len);
            if !dim_labels.is_null() {
                let _labels_p = protect(dim_labels);
                for (i, label) in labels.iter().enumerate() {
                    let cstr = CString::new(label.as_str()).unwrap_or_default();
                    SET_STRING_ELT(dim_labels, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                }
                SET_VECTOR_ELT(dimnames, 0, dim_labels);
            }
            if let Some(title) = table_title(call, args) {
                let dimnames_names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                if !dimnames_names.is_null() {
                    let _dimnames_names_p = protect(dimnames_names);
                    let cstr = CString::new(title).unwrap_or_default();
                    SET_STRING_ELT(dimnames_names, 0, Rf_mkChar(cstr.as_ptr()));
                    crate::sexp::attrib_core::setAttrib(
                        dimnames,
                        crate::sexp::attrib_core::R_NamesSymbol(),
                        dimnames_names,
                    );
                }
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dimnames,
            );
        }
        result
    }
}

fn table_title(call: SEXP, args: SEXP) -> Option<String> {
    unsafe {
        let tag = arg_tag_name(args);
        if tag.is_some() {
            return tag;
        }
        if call.is_null() || call == R_NilValue() || TYPEOF(call) != SEXPTYPE::LANGSXP {
            return None;
        }
        let first_arg = CAR(CDR(call));
        if first_arg.is_null() || first_arg == R_NilValue() || TYPEOF(first_arg) != SEXPTYPE::SYMSXP
        {
            return None;
        }
        let printname = PRINTNAME(first_arg);
        if printname.is_null() {
            return None;
        }
        let chars = CHAR(printname);
        if chars.is_null() {
            None
        } else {
            Some(std::ffi::CStr::from_ptr(chars).to_str().ok()?.to_string())
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TableUseNa {
    No,
    IfAny,
    Always,
}

impl TableUseNa {
    fn should_include(self, na_count: i64) -> bool {
        match self {
            Self::No => false,
            Self::IfAny => na_count > 0,
            Self::Always => true,
        }
    }
}

fn table_use_na(args: SEXP) -> TableUseNa {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if arg_tag_name(current).as_deref() == Some("useNA") {
                return match crate::mainutils::essentials::elt_to_string(CAR(current), 0).as_str() {
                    "ifany" => TableUseNa::IfAny,
                    "always" => TableUseNa::Always,
                    _ => TableUseNa::No,
                };
            }
            current = CDR(current);
        }
        TableUseNa::No
    }
}

fn factor_levels(x: SEXP) -> Option<SEXP> {
    unsafe {
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return None;
        }
        let is_factor = (0..XLENGTH(class))
            .any(|i| crate::mainutils::essentials::elt_to_string(class, i) == "factor");
        if !is_factor {
            return None;
        }
        let levels =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_LevelsSymbol());
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            None
        } else {
            Some(levels)
        }
    }
}

// ---------------------------------------------------------------------------
// do_as_* — type coercion
// ---------------------------------------------------------------------------

/// R's `as.integer(x)` — coerce to INTSXP.
pub unsafe fn do_as_integer(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { coerce_to_type(args, SEXPTYPE::INTSXP.as_c_int()) }
}

/// R's `as.double(x)` — coerce to REALSXP.
pub unsafe fn do_as_double(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { coerce_to_type(args, SEXPTYPE::REALSXP.as_c_int()) }
}

/// R's `as.character(x)` — coerce to STRSXP.
pub unsafe fn do_as_character(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if class_contains(x, "octmode") {
            let n = XLENGTH(x);
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            for i in 0..n {
                let value = *INTEGER(x).add(i as usize);
                let text = if value == NA_INTEGER {
                    None
                } else {
                    Some(format!("{:o}", value))
                };
                let charsxp = text
                    .and_then(|text| CString::new(text).ok())
                    .map(|text| Rf_mkChar(text.as_ptr()))
                    .unwrap_or_else(|| crate::sexp::globals::R_NaString());
                SET_STRING_ELT(result, i, charsxp);
            }
            return result;
        }
        if class_contains(x, "POSIXct") && TYPEOF(x) == SEXPTYPE::REALSXP {
            let n = XLENGTH(x);
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            for i in 0..n {
                let seconds = *REAL(x).add(i as usize);
                let charsxp = crate::mainutils::essentials::posix_seconds_to_iso(seconds, false)
                    .and_then(|text| CString::new(text).ok())
                    .map(|text| Rf_mkChar(text.as_ptr()))
                    .unwrap_or_else(|| crate::sexp::globals::R_NaString());
                SET_STRING_ELT(result, i, charsxp);
            }
            return result;
        }
        if class_contains(x, "Date") && TYPEOF(x) == SEXPTYPE::REALSXP {
            let n = XLENGTH(x);
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            for i in 0..n {
                let days = *REAL(x).add(i as usize);
                let charsxp = crate::mainutils::essentials::date_days_to_iso(days)
                    .and_then(|text| CString::new(text).ok())
                    .map(|text| Rf_mkChar(text.as_ptr()))
                    .unwrap_or_else(|| crate::sexp::globals::R_NaString());
                SET_STRING_ELT(result, i, charsxp);
            }
            return result;
        }
        if let Some(levels) = factor_levels(x) {
            let n = XLENGTH(x);
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            for i in 0..n {
                let code = *INTEGER(x).add(i as usize);
                let value = if code > 0 && (code as R_xlen_t) <= XLENGTH(levels) {
                    STRING_ELT(levels, (code - 1) as R_xlen_t)
                } else {
                    crate::sexp::globals::R_NaString()
                };
                SET_STRING_ELT(result, i, value);
            }
            return result;
        }
        if TYPEOF(x) == SEXPTYPE::SYMSXP {
            let name = PRINTNAME(x);
            if name.is_null() || name == R_NilValue() {
                return Rf_mkString(c"".as_ptr());
            }
            return Rf_mkString(CHAR(name));
        }
        coerce_to_type(args, SEXPTYPE::STRSXP.as_c_int())
    }
}

unsafe fn class_contains(x: SEXP, class_name: &str) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        for i in 0..XLENGTH(class) {
            let elt = STRING_ELT(class, i);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                continue;
            }
            let text = CStr::from_ptr(CHAR(elt)).to_str().unwrap_or("");
            if text == class_name {
                return true;
            }
        }
        false
    }
}

/// R's `as.logical(x)` — coerce to LGLSXP.
pub unsafe fn do_as_logical(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { coerce_to_type(args, SEXPTYPE::LGLSXP.as_c_int()) }
}

/// R's `as.pairlist(x)` — coerce to LISTSXP.
pub unsafe fn do_as_pairlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { coerce_to_type(args, SEXPTYPE::LISTSXP.as_c_int()) }
}

/// R's `pairlist(...)` — build a LISTSXP preserving argument tags.
pub unsafe fn do_pairlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = pairlist_len(args);
        let result = Rf_allocList(n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut src = args;
        let mut dst = result;
        while !src.is_null() && src != R_NilValue() && !dst.is_null() && dst != R_NilValue() {
            SETCAR(dst, CAR(src));
            SETTAG(dst, TAG(src));
            src = CDR(src);
            dst = CDR(dst);
        }
        result
    }
}

/// R's `as.vector(x)` — strips attributes, returns simple vector.
pub unsafe fn do_as_vector(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let mode = vector_mode_arg(args);
        match mode.as_deref() {
            None | Some("any") => duplicate_without_attributes(x),
            Some("numeric") | Some("double") => coerce_to_type(args, SEXPTYPE::REALSXP.as_c_int()),
            Some("integer") => coerce_to_type(args, SEXPTYPE::INTSXP.as_c_int()),
            Some("logical") => coerce_to_type(args, SEXPTYPE::LGLSXP.as_c_int()),
            Some("character") => coerce_to_type(args, SEXPTYPE::STRSXP.as_c_int()),
            Some("complex") => coerce_to_type(args, SEXPTYPE::CPLXSXP.as_c_int()),
            Some("raw") => coerce_to_type(args, SEXPTYPE::RAWSXP.as_c_int()),
            Some("list") => do_as_list(_call, _op, args, _rho),
            Some("pairlist") => do_as_pairlist(_call, _op, args, _rho),
            _ => duplicate_without_attributes(x),
        }
    }
}

unsafe fn duplicate_without_attributes(x: SEXP) -> SEXP {
    unsafe {
        let result = crate::mainutils::duplicate::duplicate(x);
        if !result.is_null() && result != R_NilValue() {
            SET_ATTRIB(result, R_NilValue());
        }
        result
    }
}

fn vector_mode_arg(args: SEXP) -> Option<String> {
    unsafe {
        let mode = CAR(CDR(args));
        if mode.is_null() || mode == R_NilValue() || XLENGTH(mode) == 0 {
            return None;
        }
        Some(crate::mainutils::essentials::elt_to_string(mode, 0))
    }
}

/// R's `as.list(x)` — converts to VECSXP (list).
pub unsafe fn do_as_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::VECSXP {
            return x;
        }
        if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP {
            return pairlist_as_list(x);
        }
        if t == SEXPTYPE::EXPRSXP {
            let n = XLENGTH(x);
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            for i in 0..n {
                crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, VECTOR_ELT(x, i));
            }
            let names =
                crate::eval::attrib_core::getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
            if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && XLENGTH(names) == n
            {
                let names = crate::mainutils::duplicate::duplicate(names);
                if !names.is_null() {
                    crate::eval::attrib_core::setAttrib(
                        result,
                        crate::eval::attrib_core::R_NamesSymbol(),
                        names,
                    );
                }
            }
            return result;
        }
        if t == SEXPTYPE::ENVSXP {
            return environment_as_list(x);
        }
        // Convert atomic vector to list
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for i in 0..n {
            // Create a length-1 vector for each element
            let elem = Rf_allocVector3(t, 1);
            if !elem.is_null() {
                if t == SEXPTYPE::REALSXP {
                    *REAL(elem) = *REAL(x).add(i as usize);
                } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    *INTEGER(elem) = *INTEGER(x).add(i as usize);
                }
            }
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, elem);
        }
        let names =
            crate::eval::attrib_core::getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
        if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) == n
        {
            let names = crate::mainutils::duplicate::duplicate(names);
            if !names.is_null() {
                crate::eval::attrib_core::setAttrib(
                    result,
                    crate::eval::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
        }
        result
    }
}

/// R's `as.list.environment(x, all.names = FALSE, sorted = FALSE, ...)` —
/// environment bindings as a named list. Values are read through
/// `R_findVarInFrame` so active bindings evaluate to their current value,
/// matching upstream env2list. Unsorted output preserves frame order.
pub unsafe fn do_as_list_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::ENVSXP {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let _x_guard = protect(x);

        let all_names = match CDR(args) {
            rest if !rest.is_null() && rest != R_NilValue() => {
                crate::mainutils::essentials::logical_arg(CAR(rest), false)
            }
            _ => false,
        };
        let sorted = match CDR(CDR(args)) {
            rest if !rest.is_null() && rest != R_NilValue() => {
                crate::mainutils::essentials::logical_arg(CAR(rest), false)
            }
            _ => false,
        };

        let mut entries: Vec<(String, SEXP)> = Vec::new();
        let mut frame = FRAME(x);
        while !frame.is_null() && frame != R_NilValue() {
            if let Some(name) = tag_name(TAG(frame)) {
                if all_names || !name.starts_with('.') {
                    let sym = Rf_install(CString::new(name.as_str()).unwrap_or_default().as_ptr());
                    let value = crate::sexp::envir::R_findVarInFrame(x, sym);
                    if !value.is_null() && value != crate::sexp::globals::R_UnboundValue() {
                        entries.push((name, value));
                    }
                }
            }
            frame = CDR(frame);
        }
        if sorted {
            entries.sort_by(|left, right| left.0.cmp(&right.0));
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, entries.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let names: Vec<String> = entries.iter().map(|(name, _)| name.clone()).collect();
        for (i, (_, value)) in entries.iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, *value);
        }
        let names_vec = string_vector(&names);
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names_vec);
        result
    }
}

unsafe fn pairlist_len(mut list: SEXP) -> c_int {
    unsafe {
        let mut n = 0;
        while !list.is_null() && list != R_NilValue() {
            n += 1;
            list = CDR(list);
        }
        n
    }
}

unsafe fn pairlist_as_list(list: SEXP) -> SEXP {
    unsafe {
        let n = pairlist_len(list) as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut current = list;
        let mut names = Vec::new();
        let mut has_names = false;
        for i in 0..n {
            SET_VECTOR_ELT(result, i, CAR(current));
            if let Some(name) = tag_name(TAG(current)) {
                has_names = true;
                names.push(name);
            } else {
                names.push(String::new());
            }
            current = CDR(current);
        }
        if has_names {
            let names_vec = string_vector(&names);
            crate::eval::attrib_core::setAttrib(
                result,
                crate::eval::attrib_core::R_NamesSymbol(),
                names_vec,
            );
        }
        result
    }
}

unsafe fn environment_as_list(env: SEXP) -> SEXP {
    unsafe {
        let mut entries = Vec::new();
        let mut frame = FRAME(env);
        while !frame.is_null() && frame != R_NilValue() {
            if let Some(name) = tag_name(TAG(frame)) {
                entries.push((name, CAR(frame)));
            }
            frame = CDR(frame);
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, entries.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut names = Vec::with_capacity(entries.len());
        for (i, (name, value)) in entries.iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, *value);
            names.push(name.clone());
        }
        let names_vec = string_vector(&names);
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names_vec);
        result
    }
}

unsafe fn coerce_to_type(args: SEXP, target: c_int) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        if TYPEOF(x) == target {
            return duplicate_without_attributes(x);
        }

        match SEXPTYPE(target) {
            SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::RAWSXP
            | SEXPTYPE::LISTSXP => {
                let result = crate::mainutils::coerce::coerceVector(x, target);
                if !result.is_null() && result != R_NilValue() {
                    if SEXPTYPE(target) != SEXPTYPE::LISTSXP {
                        SET_ATTRIB(result, R_NilValue());
                    }
                }
                result
            }
            _ => duplicate_without_attributes(x),
        }
    }
}
