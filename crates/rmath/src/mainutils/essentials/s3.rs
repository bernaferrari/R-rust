//! Essentials domain module `s3` — extracted verbatim from essentials.rs.

use super::*;
use crate::sexp::object::SexpMut;
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
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// S3: setOldClass, methods
// ---------------------------------------------------------------------------

/// R's `setOldClass(Class)` — register old-style S3 class. Simplified: returns Class.
pub fn do_setOldClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let class_arg = CAR(args);
        if class_arg.is_null() || class_arg == R_NilValue() {
            return R_NilValue();
        }
        class_arg
    }
}

/// Dispatch a generic builtin call to a class-specific S3 method.
///
/// Closures (package `.__S3MethodsTable__.` entries) go through
/// `applyClosure`. Builtin/special methods (`format.POSIXct`, `print.Date`,
/// …) are invoked through their PRIMFUN, matching `eval::apply`.
pub(crate) fn apply_s3_closure_method(
    generic: &str,
    call: SEXP,
    args: SEXP,
    rho: SEXP,
) -> Option<SEXP> {
    unsafe {
        let first = CAR(args);
        if first.is_null() || first == R_NilValue() {
            return None;
        }
        let klass =
            crate::sexp::attrib_core::getAttrib(first, crate::sexp::attrib_core::R_ClassSymbol());
        if klass.is_null() || klass == R_NilValue() || TYPEOF(klass) != SEXPTYPE::STRSXP {
            return None;
        }
        let _klass_guard = protect(klass);
        let method = crate::mainutils::objects::lookup_s3_method_for_classes(
            generic, klass, rho, rho, rho, false,
        )?;
        let _method_guard = protect(method.method);
        let ty = TYPEOF(method.method);
        if ty == SEXPTYPE::CLOSXP {
            return Some(crate::eval::closure::applyClosure(
                call,
                method.method,
                args,
                rho,
                R_NilValue(),
                TRUE,
            ));
        }
        if ty == SEXPTYPE::BUILTINSXP || ty == SEXPTYPE::SPECIALSXP {
            if let Some(primfun) = crate::eval::builtin::PRIMFUN(method.method) {
                return Some(primfun(call, method.method, args, rho));
            }
            // Portable (Rust-only) primitives carry a session-local negative
            // PRIMOFFSET, so PRIMFUN cannot resolve them; route through the
            // evaluated-builtin handler table by name instead.
            if let Some(sexp) = active_owner().sexp(method.method).ok()
                && let Some(name) = crate::eval::primitive::portable_primitive_name(sexp)
                && let Some(handler) = crate::eval::builtin::evaluated_builtin_handler(&name)
            {
                return Some(handler(call, method.method, args, rho));
            }
        }
        None
    }
}

/// R's `methods(generic)` — list methods known to the Rust runtime.
pub fn do_methods(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let mut class_arg = R_NilValue();
    let mut generic_arg = R_NilValue();
    let mut cell = args;
    while !cell.is_null() && cell != R_NilValue() {
        let tag = TAG(cell);
        let name = if tag.is_null() || tag == R_NilValue() {
            String::new()
        } else {
            charsxp_text(PRINTNAME(tag)).unwrap_or_default()
        };
        if name == "class" {
            class_arg = CAR(cell);
        } else if name.is_empty() && generic_arg == R_NilValue() {
            generic_arg = CAR(cell);
        }
        cell = CDR(cell);
    }
    if !class_arg.is_null() && class_arg != R_NilValue() {
        if XLENGTH(class_arg) > 1 {
            crate::mainutils::errors::Rf_warning_cstr(
                c"'class' is of length > 1; only the first element will be used",
            );
        }
        let class_name = elt_to_string(class_arg, 0);
        let suffix = format!(".{class_name}");
        let mut methods = all_runtime_method_names()
            .into_iter()
            .filter(|name| name.ends_with(&suffix))
            .collect::<Vec<_>>();
        methods.sort();
        let ans = methods_function(string_vector(&methods));
        let info = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        let info_class = mk_string_cstr(c"data.frame");
        crate::sexp::attrib_core::setAttrib(
            info,
            crate::sexp::attrib_core::R_ClassSymbol(),
            info_class,
        );
        let info_names = Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        crate::sexp::attrib_core::setAttrib(
            info,
            crate::sexp::attrib_core::R_NamesSymbol(),
            info_names,
        );
        let row_names = Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        crate::sexp::attrib_core::setAttrib(info, install_str("row.names"), row_names);
        crate::sexp::attrib_core::setAttrib(ans, install_str("info"), info);
        return ans;
    }
    if generic_arg.is_null() || generic_arg == R_NilValue() {
        return methods_function(string_vector(&all_runtime_method_names()));
    }
    let mut generic = String::new();
    if TYPEOF(generic_arg) == SEXPTYPE::STRSXP {
        generic = elt_to_string(generic_arg, 0);
    } else {
        let mut call_cell = CDR(call);
        while !call_cell.is_null() && call_cell != R_NilValue() {
            let tag = TAG(call_cell);
            let tagged = !tag.is_null() && tag != R_NilValue();
            if !tagged {
                let expr = CAR(call_cell);
                if TYPEOF(expr) == SEXPTYPE::SYMSXP {
                    let pname = PRINTNAME(expr);
                    if !pname.is_null() {
                        generic = charsxp_text(pname).unwrap_or_default();
                    }
                }
                break;
            }
            call_cell = CDR(call_cell);
        }
    }
    let prefix = format!("{generic}.");
    let mut methods = all_runtime_method_names()
        .into_iter()
        .filter(|name| name.starts_with(&prefix))
        .collect::<Vec<_>>();
    let base = crate::eval::runtime::base_env();
    if !base.is_null() {
        for name in crate::mainutils::essentials::shared::frame_binding_names(base, true) {
            if name.starts_with(&prefix) && !methods.iter().any(|m| m == &name) {
                methods.push(name);
            }
        }
    }
    methods.sort();
    let ans = methods_function(string_vector(&methods));
    let n = methods.len() as i64;
    let info = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    let from = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    let visible = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    let rows = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    let owner = active_owner();
    if let (Ok(mut from_mut), Ok(mut visible_mut), Ok(mut rows_mut)) = (
        owner.sexp(from).and_then(SexpMut::try_from_checked),
        owner.sexp(visible).and_then(SexpMut::try_from_checked),
        owner.sexp(rows).and_then(SexpMut::try_from_checked),
    ) {
        for i in 0..n {
            if let Ok(charsxp) = owner.sexp(mk_char_cstr(c"base")) {
                let _ = from_mut.try_set_string_elt(i, charsxp);
            }
            let _ = visible_mut.try_set_logical_elt(i, 1);
            let cname = std::ffi::CString::new(methods[i as usize].as_str()).unwrap_or_default();
            if let Ok(charsxp) = owner.sexp(mk_char_cstr(&cname)) {
                let _ = rows_mut.try_set_string_elt(i, charsxp);
            }
        }
    }
    if let Ok(mut info_mut) = owner.sexp(info).and_then(SexpMut::try_from_checked) {
        for (i, value) in [from, visible].into_iter().enumerate() {
            if let Ok(value) = owner.sexp(value) {
                let _ = info_mut.try_set_vector_elt(i as R_xlen_t, value);
            }
        }
    }
    let inames = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
    if let Ok(mut inames_mut) = owner.sexp(inames).and_then(SexpMut::try_from_checked) {
        for (i, text) in [c"from", c"visible"].into_iter().enumerate() {
            if let Ok(charsxp) = owner.sexp(mk_char_cstr(&text)) {
                let _ = inames_mut.try_set_string_elt(i as R_xlen_t, charsxp);
            }
        }
    }
    crate::sexp::attrib_core::setAttrib(info, crate::sexp::attrib_core::R_NamesSymbol(), inames);
    crate::sexp::attrib_core::setAttrib(
        info,
        crate::sexp::attrib_core::R_ClassSymbol(),
        mk_string_cstr(c"data.frame"),
    );
    crate::sexp::attrib_core::setAttrib(info, install_str("row.names"), rows);
    crate::sexp::attrib_core::setAttrib(ans, install_str("info"), info);
    ans
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
fn methods_function(ans: SEXP) -> SEXP {
    let class = mk_string_cstr(c"MethodsFunction");
    let _g = protect(class);
    crate::sexp::attrib_core::setAttrib(ans, crate::sexp::attrib_core::R_ClassSymbol(), class);
    ans
}

// ---------------------------------------------------------------------------
// S3 dispatch, environment functions, I/O extensions
// ---------------------------------------------------------------------------

/// R's `UseMethod(generic, obj)` — delegate to the translated object-system
/// dispatch implementation.
pub fn do_usemethod(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    crate::mainutils::objects::do_usemethod(call, op, args, rho)
}

/// R's `missing(x)` — check if argument was missing in call.
pub fn do_missing(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    crate::eval::missing::do_missing(call, op, args, rho)
}

/// R's `parent.frame(n)` — get enclosing environment.
///
/// Upstream `R_findParentContext`: starting from the current context,
/// repeatedly find the next function context whose execution environment
/// (`cloenv`) equals the current context's `sysparent` — i.e. follow the
/// "where was this frame (seemingly) called from" links n times — and
/// return the found context's `sysparent`.
///
/// The start context is [`sys_query_context()`]: the engine pushes a bare
/// `CTXT_RETURN` context (null call, null sysparent, cloenv = the
/// promise's PRENV) while forcing promises, and sys.* queries must see
/// through the forcing machinery to the promise's HOME frame. Without
/// that, `parent.frame()` inside a forced argument promise resolved
/// against the forcing closure's caller instead of the promise creator's
/// caller (zeallot's `%<-%` passes `list_assign(pairs, parent.frame())`;
/// the promise is forced inside list_assign, yet must name `%<-%`'s
/// caller).
pub fn do_parent_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = context_index_arg(args, 1);
        if n == NA_INTEGER || n < 1 {
            base_error("invalid 'n' value");
        }

        // GNU do_parentframe: R_findParentContext(R_GlobalContext, n) then
        // that context's sysparent. Always walk at least once so a
        // `parent.frame` wrapper frame is skipped (eval.R).
        let mut cptr = sys_query_context();
        if cptr.is_null() {
            return crate::sexp::globals::R_GlobalEnv();
        }
        let mut remaining = n;
        loop {
            cptr = find_exec_context(cptr, (*cptr).sysparent.as_raw());
            if cptr.is_null() {
                return crate::sexp::globals::R_GlobalEnv();
            }
            if remaining == 1 {
                let sysparent = (*cptr).sysparent.as_raw();
                if sysparent.is_null() {
                    return crate::sexp::globals::R_GlobalEnv();
                }
                return sysparent;
            }
            remaining -= 1;
        }
    }
}

/// Find a function context at or older than `cptr` executing in `envir`
/// (upstream `R_findExecContext` — powers `parent.frame()`).
unsafe fn find_exec_context(
    mut cptr: *mut crate::sexp::context::RCNTXT,
    envir: SEXP,
) -> *mut crate::sexp::context::RCNTXT {
    unsafe {
        while !cptr.is_null() {
            if (*cptr).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION != 0
                && (*cptr).cloenv.as_raw() == envir
            {
                return cptr;
            }
            cptr = (*cptr).nextcontext;
        }
        std::ptr::null_mut()
    }
}
/// Effective context for `sys.*` queries.
///
/// The engine pushes a bare `CTXT_RETURN` context (null call, null
/// sysparent, cloenv = the promise's PRENV) while forcing promises.
/// `sys.*` must see through the forcing machinery to the promise's HOME
/// frame: upstream skips its own `sys.*()` closure frame via
/// `getLexicalContext(R_GlobalContext->sysparent)`, which also hides any
/// intermediate closures forcing the promise (e.g. `identical(a, b)`
/// evaluating `sys.frame(sys.parent(n))` as an argument must number
/// frames from the promise-creating frame, not identical's). The promise
/// context's cloenv names that home environment directly; resolve to the
/// first function context executing in it.
pub(crate) fn sys_query_context() -> *mut crate::sexp::context::RCNTXT {
    // SAFETY: R_GlobalContext returns null or the live top of the active
    // instance's context stack, which stays valid while this builtin runs;
    // only its plain fields are read here.
    unsafe {
        let top = crate::sexp::context::R_GlobalContext();
        if top.is_null() {
            return top;
        }
        let ctx = &*top;
        if ctx.callflag == crate::sexp::context::ctxt_flags::CTXT_RETURN
            && ctx.sysparent.as_raw().is_null()
            && !ctx.cloenv.as_raw().is_null()
        {
            let home = crate::eval::context::getLexicalContext(ctx.cloenv.as_raw());
            if !home.is_null() {
                return home;
            }
        }
        top
    }
}
/// R's `sys.call(which)` — get the call that's currently being evaluated.
pub fn do_sys_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let which = context_index_arg(args, 0);
        let top = sys_query_context();
        if top.is_null() {
            R_NilValue()
        } else {
            crate::eval::context::R_syscall(which, top)
        }
    }
}

/// R's `sys.calls()` — the calls for each frame on the context stack.
///
/// Upstream collects the call of every CTXT_FUNCTION context as a
/// PAIRLIST ordered outermost-first: `sys.calls()[[1]]` is the outermost
/// call and the last element is the `sys.calls()` expression itself
/// (upstream do_sys fills `allocList(nframe)` with `R_syscall(i, cptr)`
/// for `i` in `1..=nframe`; a positive n counts up from the globalEnv,
/// so syscall(1) is the outermost frame). Calls are shallow-duplicated
/// so later mutation cannot rewrite the stack; zeallot's error paths
/// deparse these to attribute assignment-operator calls.
pub fn do_sys_calls(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // sys.calls is a builtin here, so (outside forced promises) the
        // innermost context is its caller's frame already — no own closure
        // frame to skip, unlike upstream's
        // getLexicalContext(R_GlobalContext->sysparent).
        let cptr = sys_query_context();
        if cptr.is_null() {
            return R_NilValue();
        }
        let nframe = crate::eval::context::framedepth(cptr);
        let result = crate::sexp::constructors::Rf_allocList(nframe);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        let _result_guard = protect(result);
        let mut t = result;
        let mut i = 1;
        while i <= nframe {
            SETCAR(t, crate::eval::context::R_syscall(i, cptr));
            t = CDR(t);
            i += 1;
        }
        result
    }
}

/// R's `sys.parent(n = 1)` — S-frame number of the n-th parent frame,
/// such that `sys.frame(sys.parent(n))` is the n-th parent environment
/// (upstream do_sys: `i = framedepth(cptr); while n > 0 { i =
/// R_sysparent(nframe - i + 1, cptr) }`).
pub fn do_sys_parent(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = context_index_arg(args, 1);
        if n == NA_INTEGER {
            base_error("invalid 'n' argument");
        }
        let cptr = sys_query_context();
        if cptr.is_null() {
            return Rf_ScalarInteger(0);
        }
        let nframe = crate::eval::context::framedepth(cptr);
        let mut i = nframe;
        let mut count = n;
        while count > 0 {
            i = crate::eval::context::R_sysparent(nframe - i + 1, cptr);
            count -= 1;
        }
        Rf_ScalarInteger(i)
    }
}

/// R's `sys.frame(which)` — get frame at specified level.
pub fn do_sys_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let which = context_index_arg(args, 0);
        let top = sys_query_context();
        if top.is_null() {
            crate::sexp::globals::R_GlobalEnv()
        } else {
            crate::eval::context::R_sysframe(which, top)
        }
    }
}

pub(crate) fn pairlist_len(mut list: SEXP) -> c_int {
    {
        let mut len = 0;
        while !list.is_null() && list != R_NilValue() {
            len += 1;
            list = CDR(list);
        }
        len
    }
}

pub(crate) fn context_index_arg(args: SEXP, default: c_int) -> c_int {
    {
        if args.is_null() || args == R_NilValue() {
            default
        } else {
            real_or_default(CAR(args), default as f64) as c_int
        }
    }
}

/// R's `getwd()` — get working directory.
pub fn do_getwd(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    use crate::mainutils::browser_files;
    if browser_files::enabled() {
        let path = CString::new(browser_files::getcwd_current()).unwrap_or_default();
        return mk_string_cstr(&path);
    }
    match std::env::current_dir() {
        Ok(path) => {
            let s = path.to_string_lossy();
            let cstr = CString::new(s.as_ref()).unwrap_or_default();
            mk_string_cstr(&cstr)
        }
        Err(_) => R_NilValue(),
    }
}

/// R's `setwd(dir)` — set working directory.
pub fn do_setwd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::mainutils::browser_files;
        let dir_arg = CAR(args);
        if crate::sexp::accessors::TYPEOF(dir_arg) != SEXPTYPE::STRSXP
            || XLENGTH(dir_arg) != 1
            || owner_sexp(active_owner(), dir_arg)
                .try_string_value_elt(0)
                .map(|value| value.is_none())
                .unwrap_or(true)
        {
            crate::sexp::context::r_error("invalid 'dir' argument");
        }
        let path = elt_to_string(dir_arg, 0);
        let previous = if browser_files::enabled() {
            browser_files::chdir_current(&path)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error))
        } else {
            let allowed = crate::sexp::instance::with_required_current_instance(|inst| {
                (*inst).eval_state.capabilities.allow_environment_mutation
            });
            if !allowed {
                crate::sexp::context::r_error(
                    "changing the working directory is disabled for this session",
                );
            }
            let previous = std::env::current_dir();
            if std::env::set_current_dir(&path).is_err() {
                crate::sexp::context::r_error(format!(
                    "cannot change working directory to '{path}'"
                ));
            }
            let Ok(previous) = previous else {
                return R_NilValue();
            };
            previous.to_string_lossy().into_owned()
        };
        crate::sexp::globals::set_R_Visible(FALSE);
        let path = CString::new(previous).unwrap_or_default();
        mk_string_cstr(&path)
    }
}

#[cfg(test)]
mod host_state_tests {
    #[test]
    fn setwd_is_denied_without_host_environment_capability() {
        let session = crate::sexp::session::RSession::new();
        let denied = session.with_active(|| {
            let path = super::mk_string_cstr(c".");
            let _path = crate::sexp::protect::protect(path);
            let args = crate::sexp::constructors::Rf_cons(path, crate::sexp::globals::R_NilValue());
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                super::do_setwd(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    args,
                    std::ptr::null_mut(),
                )
            }))
        });
        let payload = denied.expect_err("setwd should be denied");
        let error = payload
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("denial should use the RError payload");
        assert!(error.message.contains("working directory"));
    }
}

/// R's `basename(path)` — final path component, vectorized over character input.
pub fn do_basename(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    map_path_strings(CAR(args), r_basename)
}

/// R's `dirname(path)` — parent path component, vectorized over character input.
pub fn do_dirname(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    map_path_strings(CAR(args), r_dirname)
}

/// R's `file.path(...)` — join path components element-wise with recycling.
pub fn do_file_path(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    crate::mainutils::paste_impl::do_filepath_public(call, op, args, env)
}

/// R's `dir.exists(paths)` — check if directories exist.
pub fn do_dir_exists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    let mut dst = owner_sexp_mut(result);
    for i in 0..n {
        let path = elt_to_string(x, i);
        sexp_ok(dst.try_set_logical_elt(
            (i as usize) as R_xlen_t,
            if std::path::Path::new(&path).is_dir() {
                TRUE
            } else {
                FALSE
            },
        ));
    }
    result
}

/// R's `file.create(...)` — create empty files.
pub fn do_file_create(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    let mut dst = owner_sexp_mut(result);
    for i in 0..n {
        let path = elt_to_string(x, i);
        sexp_ok(dst.try_set_logical_elt(
            (i as usize) as R_xlen_t,
            match crate::mainutils::platform::create_file_with_session_umask(&path) {
                Ok(_) => TRUE,
                Err(_) => FALSE,
            },
        ));
    }
    result
}

/// R's `unlink(x, recursive)` — delete files or directories.
fn unlink_glob(pat: &str, name: &str) -> bool {
    fn rec(pat: &[u8], name: &[u8]) -> bool {
        if pat.is_empty() {
            return name.is_empty();
        }
        match pat[0] {
            b'*' => rec(&pat[1..], name) || (!name.is_empty() && rec(pat, &name[1..])),
            b'?' => !name.is_empty() && rec(&pat[1..], &name[1..]),
            c => !name.is_empty() && name[0] == c && rec(&pat[1..], &name[1..]),
        }
    }
    rec(pat.as_bytes(), name.as_bytes())
}

pub fn do_unlink(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    // Browser paths are resolved only inside the owning session.
    if crate::mainutils::browser_files::enabled() {
        return crate::mainutils::platform::do_unlink(call, op, args, rho);
    }
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarInteger(0);
    }
    let n = XLENGTH(x);
    let recursive = {
        let flag = CADR(args);
        !flag.is_null()
            && flag != R_NilValue()
            && TYPEOF(flag) == SEXPTYPE::LGLSXP
            && XLENGTH(flag) > 0
            && owner_sexp(active_owner(), flag)
                .try_logical_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                != 0
    };
    let mut failed = false;
    for i in 0..n {
        let path = elt_to_string(x, i);
        let targets = if path.contains('*') || path.contains('?') {
            let dir = std::path::Path::new(&path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(std::path::Path::new("."));
            let pat = std::path::Path::new(&path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&path)
                .to_string();
            match std::fs::read_dir(dir) {
                Ok(entries) => entries
                    .filter_map(|e| e.ok())
                    .filter(|e| unlink_glob(&pat, &e.file_name().to_string_lossy()))
                    .map(|e| e.path())
                    .collect(),
                Err(_) => Vec::new(),
            }
        } else {
            vec![std::path::PathBuf::from(&path)]
        };
        for p in targets {
            let result = if p.is_dir() {
                if recursive {
                    std::fs::remove_dir_all(&p)
                } else {
                    std::fs::remove_dir(&p)
                }
            } else {
                std::fs::remove_file(&p)
            };
            if let Err(err) = result {
                if err.kind() != std::io::ErrorKind::NotFound {
                    failed = true;
                }
            }
        }
    }
    let result = Rf_ScalarInteger(if failed { 1 } else { 0 });

    crate::sexp::globals::set_R_Visible(FALSE);
    result
}

/// R's `nzchar(x)` — check if strings are non-empty.
pub fn do_nzchar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    let owner = active_owner();
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for i in 0..n {
        let s = elt_to_string(x, i);
        result_mut
            .try_set_logical_elt(i, if s.is_empty() { FALSE } else { TRUE })
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    result
}

// ---------------------------------------------------------------------------
// S3 generics
// ---------------------------------------------------------------------------

pub(crate) fn matrix_column(x: SEXP, nrow: R_xlen_t, column: R_xlen_t) -> SEXP {
    let ty = TYPEOF(x);
    let result = Rf_allocVector3(ty, nrow);
    if result.is_null() {
        return result;
    }
    let offset = column * nrow;
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for row in 0..nrow {
        let source = offset + row;
        match ty {
            t if t == SEXPTYPE::LGLSXP => {
                let v = x_owned
                    .try_logical_elt(source)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                result_mut
                    .try_set_logical_elt(row, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            t if t == SEXPTYPE::INTSXP => {
                let v = x_owned
                    .try_integer_elt(source)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                result_mut
                    .try_set_integer_elt(row, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            t if t == SEXPTYPE::REALSXP => {
                let v = x_owned
                    .try_real_elt(source)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                result_mut
                    .try_set_real_elt(row, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            t if t == SEXPTYPE::CPLXSXP => {
                let v = x_owned
                    .try_complex_elt(source)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                result_mut
                    .try_set_complex_elt(row, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            t if t == SEXPTYPE::RAWSXP => {
                let v = x_owned
                    .try_raw_elt(source)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                result_mut
                    .try_set_raw_elt(row, v)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            t if t == SEXPTYPE::STRSXP => {
                let elt = x_owned
                    .try_string_elt(source)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                result_mut
                    .try_set_string_elt(row, elt)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                let elt = x_owned
                    .try_vector_elt(source)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                result_mut
                    .try_set_vector_elt(row, elt)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            _ => base_error("cannot coerce matrix to a data frame"),
        }
    }
    result
}

fn matrix_as_data_frame(x: SEXP, dim: SEXP) -> SEXP {
    let owner = active_owner();
    let dim_owned = owner_sexp(owner, dim);
    let nrow = dim_owned
        .try_integer_elt(0)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        as R_xlen_t;
    let ncol = dim_owned
        .try_integer_elt(1)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncol);
    if result.is_null() {
        return result;
    }
    let _result_guard = protect(result);
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for column in 0..ncol {
        let value = matrix_column(x, nrow, column);
        let _value_guard = protect(value);
        let value_owned = owner
            .sexp(value)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        result_mut
            .try_set_vector_elt(column, value_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }

    let dimnames =
        crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimNamesSymbol());
    let dimnames_owned = owner_sexp(owner, dimnames);
    let row_names = if TYPEOF(dimnames) == SEXPTYPE::VECSXP && XLENGTH(dimnames) >= 1 {
        dimnames_owned
            .try_vector_elt(0)
            .map(|elt| elt.as_raw())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    } else {
        R_NilValue()
    };
    if !row_names.is_null() && row_names != R_NilValue() {
        crate::sexp::attrib_core::setAttrib(result, install_str("row.names"), row_names);
    } else {
        set_compact_row_names(result, nrow);
    }

    let column_names = if TYPEOF(dimnames) == SEXPTYPE::VECSXP && XLENGTH(dimnames) >= 2 {
        dimnames_owned
            .try_vector_elt(1)
            .map(|elt| elt.as_raw())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    } else {
        R_NilValue()
    };
    if !column_names.is_null() && column_names != R_NilValue() {
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            column_names,
        );
    } else {
        let names = (1..=ncol).map(|i| format!("V{i}")).collect::<Vec<_>>();
        set_string_names(result, &names);
    }
    set_data_frame_class(result);
    result
}

fn list_as_data_frame(x: SEXP) -> SEXP {
    let ncol = XLENGTH(x);
    let names_attr =
        crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    let mut nrow: R_xlen_t = 0;
    for i in 0..ncol {
        let col_len = x_owned
            .try_vector_elt(i)
            .map(|elt| elt.len())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        nrow = nrow.max(col_len);
    }
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncol);
    if result.is_null() {
        return result;
    }
    let _result_guard = protect(result);
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut names: Vec<String> = Vec::with_capacity(ncol as usize);
    for i in 0..ncol {
        let col = x_owned
            .try_vector_elt(i)
            .map(|elt| elt.as_raw())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let recycled = recycle_column_if_needed(col, nrow);
        let recycled_owned = owner
            .sexp(recycled)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        result_mut
            .try_set_vector_elt(i, recycled_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let name = if !names_attr.is_null()
            && names_attr != R_NilValue()
            && TYPEOF(names_attr) == SEXPTYPE::STRSXP
            && i < XLENGTH(names_attr)
        {
            string_at_or_empty(names_attr, i)
        } else {
            String::new()
        };
        names.push(if name.is_empty() {
            format!("V{}", i + 1)
        } else {
            name
        });
    }
    set_string_names(result, &names);
    set_compact_row_names(result, nrow);
    set_data_frame_class(result);
    result
}

/// R's `as.data.frame(x)` — convert to data.frame.
pub fn do_as_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let mut x = CAR(args);

    if x.is_null() || x == R_NilValue() {
        return empty_data_frame();
    }
    // If already a data.frame, return as-is
    let class = crate::sexp::attrib_core::getAttrib(x, install_str("class"));
    if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
        let cls_name = elt_to_string(class, 0);
        if cls_name == "data.frame" {
            return x;
        }
    }
    if crate::mainutils::essentials::sexp_has_class(x, "POSIXlt") && TYPEOF(x) == SEXPTYPE::VECSXP {
        let ct =
            crate::mainutils::essentials::do_as_POSIXct(_call, _op, Rf_cons(x, R_NilValue()), _rho);
        let _ct = protect(ct);
        x = ct;
    }
    if crate::mainutils::essentials::sexp_has_class(x, "table") {
        return crate::mainutils::essentials::functional::melt_table(x);
    }
    let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
    if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
        // GNU `as.data.frame.AsIs` for a 2-D object calls
        // `as.data.frame.model.matrix`: keep the matrix as one list column.
        if crate::mainutils::essentials::sexp_has_class(x, "AsIs")
            || crate::mainutils::essentials::sexp_has_class(x, "model.matrix")
        {
            return asis_matrix_as_data_frame(x, dim);
        }
        return matrix_as_data_frame(x, dim);
    }
    if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 1 {
        x = crate::mainutils::duplicate::shallow_duplicate(x);
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_DimSymbol(),
            R_NilValue(),
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
            R_NilValue(),
        );
    }

    if TYPEOF(x) == SEXPTYPE::VECSXP && !crate::mainutils::essentials::sexp_has_class(x, "POSIXct")
    {
        return list_as_data_frame(x);
    }

    // Wrap in a single-element list and set class
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = protect(result);
    let owner = active_owner();
    if let (Ok(mut result_mut), Ok(x_owned)) = (
        owner.sexp(result).and_then(SexpMut::try_from_checked),
        owner.sexp(x),
    ) {
        let _ = result_mut.try_set_vector_elt(0, x_owned);
    }

    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !class_vec.is_null() {
        let _class_guard = protect(class_vec);
        let cstr = c"data.frame";
        let charsxp = mk_char_cstr(&cstr);
        if let (Ok(mut class_mut), Ok(charsxp)) = (
            owner.sexp(class_vec).and_then(SexpMut::try_from_checked),
            owner.sexp(charsxp),
        ) {
            let _ = class_mut.try_set_string_elt(0, charsxp);
        }
        crate::sexp::attrib_core::setAttrib(result, install_str("class"), class_vec);
    }

    // Set row.names
    let nrow = XLENGTH(x);
    let rn = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !rn.is_null() {
        let _row_names_guard = protect(rn);
        if let Ok(mut rn_mut) = owner.sexp(rn).and_then(SexpMut::try_from_checked) {
            let _ = rn_mut.try_set_integer_elt(0, NA_INTEGER);
            let _ = rn_mut.try_set_integer_elt(1, -(nrow as i32));
        }
        crate::sexp::attrib_core::setAttrib(result, install_str("row.names"), rn);
    }

    // Set column name to "x"
    let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !names_vec.is_null() {
        let _names_guard = protect(names_vec);
        let cstr = c"x";
        let charsxp = mk_char_cstr(&cstr);
        if let (Ok(mut names_mut), Ok(charsxp)) = (
            owner.sexp(names_vec).and_then(SexpMut::try_from_checked),
            owner.sexp(charsxp),
        ) {
            let _ = names_mut.try_set_string_elt(0, charsxp);
        }
        crate::sexp::attrib_core::setAttrib(result, install_str("names"), names_vec);
    }

    result
}

/// GNU `as.data.frame(NULL)`: zero columns and zero rows.
fn empty_data_frame() -> SEXP {
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
    let _p = protect(result);
    crate::mainutils::essentials::set_data_frame_class(result);
    let rn = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    let _r = protect(rn);
    if let Ok(mut rn_mut) = active_owner().sexp(rn).and_then(SexpMut::try_from_checked) {
        let _ = rn_mut.try_set_integer_elt(0, crate::sexp::ffi::NA_INTEGER);
        let _ = rn_mut.try_set_integer_elt(1, 0);
    }
    crate::sexp::attrib_core::setAttrib(result, install_str("row.names"), rn);
    result
}

fn asis_matrix_as_data_frame(x: SEXP, dim: SEXP) -> SEXP {
    let owner = active_owner();
    let nrow = owner_sexp(owner, dim)
        .try_integer_elt(0)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
    if result.is_null() {
        return result;
    }
    let _result_guard = protect(result);
    if let (Ok(mut result_mut), Ok(x_owned)) = (
        owner.sexp(result).and_then(SexpMut::try_from_checked),
        owner.sexp(x),
    ) {
        let _ = result_mut.try_set_vector_elt(0, x_owned);
    }
    set_string_names(result, &["x".to_string()]);
    set_compact_row_names(result, nrow);
    set_data_frame_class(result);
    result
}

#[cfg(test)]
mod as_data_frame_tests {
    use crate::sexp::ffi::TRUE;
    use crate::sexp::session::RSession;

    #[test]
    fn matrix_conversion_splits_columns_and_preserves_dimnames() {
        let mut session = RSession::new();
        let (result, output, visible) = session.eval_code_with_output_capture(
            "x <- matrix(1:4, 2, 2, dimnames=list(c('abc','ab'), c('cde','cd'))); \
             y <- as.data.frame(x); \
             identical(dim(y), c(2L, 2L)) && \
             identical(names(y), c('cde', 'cd')) && \
             identical(row.names(y), c('abc', 'ab')) && \
             identical(y['ab',]$cde, 2L) && \
             identical(y['ab',]$cd, 4L) && \
             identical(y[, 'cd'], c(3L, 4L))",
        );
        let result = result.expect("matrix conversion should evaluate");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(output.stdout.is_empty());
        assert!(visible);
    }

    #[test]
    fn asis_matrix_stays_one_column() {
        let mut session = RSession::new();
        let (result, output, visible) = session.eval_code_with_output_capture(
            "d2 <- data.frame(b = I(matrix(1:6, 3, 2))); \
             identical(names(d2), 'b') && \
             identical(dim(d2), c(3L, 1L)) && \
             identical(dim(d2$b), c(3L, 2L)) && \
             identical(dim(d2[2,]), c(1L, 2L)) && \
             identical(as.vector(d2[2,]), c(2L, 5L)) && \
             identical(d2[-1,], d2[2:3,])",
        );
        let result = result.expect("AsIs matrix data.frame should evaluate");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(output.stdout.is_empty());
        assert!(visible);
    }

    #[test]
    fn data_frame_drop_keeps_factor_levels() {
        let mut session = RSession::new();
        let (result, output, visible) = session.eval_code_with_output_capture(
            "df <- data.frame(f = factor(c('a','b','c'), levels=c('a','b','c','d'))); \
             identical(levels(df[2, 1]), levels(df$f)) && \
             identical(as.character(df[2, 1]), 'b')",
        );
        let result = result.expect("factor column subset should evaluate");
        assert_eq!(result.logical_elt(0), Some(TRUE));
        assert!(output.stdout.is_empty());
        assert!(visible);
    }
}

/// R's `as.list(x)` — generic list conversion.
/// Delegates to do_as_list but available as a separate entry point.
pub fn do_as_list_generic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_as_list(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// Complete data operations
// ---------------------------------------------------------------------------

/// R's `reshape(x, direction, varying, v.names, timevar, idvar, times)` — reshape data.
///
/// Not ported: this runtime does not implement `reshape()`. Fail loudly
/// instead of silently returning the input unchanged.
pub fn do_reshape(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    package_error(
        "reshape() is not ported in this pure-R Android runtime; use aggregate(), melt-style helpers, or explicit subsetting/assignment instead",
    )
}

/// R's `complete_cases(...)` — returns logical vector: TRUE where all args are non-NA.
pub fn do_complete_cases(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    let mut dst = owner_sexp_mut(result);
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
        sexp_ok(dst.try_set_logical_elt((i as usize) as R_xlen_t, complete));
    }
    result
}

/// R's `na.omit(x)` — returns x with rows containing any NA removed (simplified: works on vectors).
pub fn do_na_omit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    na_omit_atomic(args, "omit")
}

/// R's `na.exclude(x)` — like na.omit with "exclude" na.action metadata.
pub fn do_na_exclude(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    na_omit_atomic(args, "exclude")
}

fn na_omit_atomic(args: SEXP, action_class: &str) -> SEXP {
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

fn set_selected_names_attribute(x: SEXP, result: SEXP, indices: &[R_xlen_t]) {
    let names = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
    if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
        return;
    }
    let selected = Rf_allocVector3(SEXPTYPE::STRSXP, indices.len() as R_xlen_t);
    if selected.is_null() {
        return;
    }
    let _selected_guard = protect(selected);
    let owner = active_owner();
    let names_owned = owner_sexp(owner, names);
    if let Ok(mut selected_mut) = owner.sexp(selected).and_then(SexpMut::try_from_checked) {
        for (out, &src) in indices.iter().enumerate() {
            if let Ok(elt) = names_owned.try_string_elt(src) {
                let _ = selected_mut.try_set_string_elt(out as R_xlen_t, elt);
            }
        }
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_NamesSymbol(),
        selected,
    );
}

fn set_na_action_attribute(source: SEXP, result: SEXP, dropped: &[R_xlen_t], action_class: &str) {
    let action = Rf_allocVector3(SEXPTYPE::INTSXP, dropped.len() as R_xlen_t);
    if action.is_null() {
        return;
    }
    let _action_guard = protect(action);
    let owner = active_owner();
    if let Ok(mut action_mut) = owner.sexp(action).and_then(SexpMut::try_from_checked) {
        for (out, &src) in dropped.iter().enumerate() {
            let _ = action_mut.try_set_integer_elt(out as R_xlen_t, (src + 1) as c_int);
        }
    }

    let names =
        crate::sexp::attrib_core::getAttrib(source, crate::sexp::attrib_core::R_NamesSymbol());
    if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
        let action_names = Rf_allocVector3(SEXPTYPE::STRSXP, dropped.len() as R_xlen_t);
        if !action_names.is_null() {
            let _names_guard = protect(action_names);
            let names_owned = owner_sexp(owner, names);
            if let Ok(mut action_names_mut) =
                owner.sexp(action_names).and_then(SexpMut::try_from_checked)
            {
                for (out, &src) in dropped.iter().enumerate() {
                    if let Ok(elt) = names_owned.try_string_elt(src) {
                        let _ = action_names_mut.try_set_string_elt(out as R_xlen_t, elt);
                    }
                }
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
        if let (Ok(mut class_mut), Ok(charsxp)) = (
            owner.sexp(class).and_then(SexpMut::try_from_checked),
            owner.sexp(mk_char_cstr(
                &CString::new(action_class).unwrap_or_default(),
            )),
        ) {
            let _ = class_mut.try_set_string_elt(0, charsxp);
        }
        crate::sexp::attrib_core::setAttrib(
            action,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
    }
    crate::sexp::attrib_core::setAttrib(result, install_str("na.action"), action);
}

/// R's `is_complete(x)` — logical vector of complete cases for a single vector.
pub fn do_is_complete(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    let mut dst = owner_sexp_mut(result);
    for i in 0..n {
        let na = atomic_value_is_missing(x, i);
        sexp_ok(dst.try_set_logical_elt((i as usize) as R_xlen_t, if na { FALSE } else { TRUE }));
    }
    result
}

// ---------------------------------------------------------------------------
// Complete S3
// ---------------------------------------------------------------------------

/// R-like `rownames_to_column(x, var)` — convert row names to a leading column.
pub fn do_rownames_to_column(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
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
pub fn do_column_to_rownames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

    let x_owned = owner_sexp(active_owner(), x);
    let mut out_names = Vec::new();
    let mut out_columns = Vec::new();
    for (i, name) in names.into_iter().enumerate() {
        if i != row_col {
            out_names.push(name);
            out_columns.push(
                x_owned
                    .try_vector_elt(i as R_xlen_t)
                    .map(|elt| elt.as_raw())
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            );
        }
    }
    let row_col_value = x_owned
        .try_vector_elt(row_col as R_xlen_t)
        .map(|elt| elt.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    build_data_frame(
        out_columns,
        out_names,
        string_vector(&vector_to_string_values(row_col_value)),
    )
}

/// R-like `relocate(x, cols, .before, .after)` — reorder data-frame columns.
pub fn do_relocate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    let x_owned = owner_sexp(active_owner(), x);
    for name in &rest {
        if let Some(idx) = names.iter().position(|column| column == name) {
            out_columns.push(
                x_owned
                    .try_vector_elt(idx as R_xlen_t)
                    .map(|elt| elt.as_raw())
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            );
        }
    }
    build_data_frame(out_columns, rest, data_frame_row_names_attr(x))
}

pub(crate) fn is_data_frame_like(x: SEXP) -> bool {
    {
        !x.is_null()
            && x != R_NilValue()
            && TYPEOF(x) == SEXPTYPE::VECSXP
            && is_data_frame_object(x)
    }
}

fn data_frame_column_names(x: SEXP) -> Vec<String> {
    let names = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
    if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
        return (0..XLENGTH(x)).map(|i| format!("V{}", i + 1)).collect();
    }
    (0..XLENGTH(x)).map(|i| elt_to_string(names, i)).collect()
}

fn data_frame_columns(x: SEXP) -> Vec<SEXP> {
    {
        let x_owned = owner_sexp(active_owner(), x);
        (0..x_owned.len())
            .map(|i| {
                x_owned
                    .try_vector_elt(i)
                    .map(|elt| elt.as_raw())
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            })
            .collect()
    }
}

fn data_frame_row_names_attr(x: SEXP) -> SEXP {
    crate::sexp::attrib_core::getAttrib(x, install_str("row.names"))
}

pub(crate) fn data_frame_row_names(x: SEXP) -> Vec<String> {
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
        let attr_owned = owner_sexp(active_owner(), attr);
        let first = attr_owned
            .try_integer_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let second = attr_owned
            .try_integer_elt(1)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if first == NA_INTEGER && second < 0 {
            return (1..=(-second as R_xlen_t)).map(|i| i.to_string()).collect();
        }
    }
    vector_to_string_values(attr)
}

fn vector_to_string_values(x: SEXP) -> Vec<String> {
    {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        (0..XLENGTH(x)).map(|i| elt_to_string(x, i)).collect()
    }
}

fn string_arg_values(x: SEXP) -> Vec<String> {
    {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        (0..XLENGTH(x)).map(|i| elt_to_string(x, i)).collect()
    }
}

pub(crate) fn condition_message_text(args: SEXP, option_names: &[&str]) -> String {
    {
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
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, columns.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _result_guard = protect(result);
    let owner = active_owner();
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for (i, column) in columns.into_iter().enumerate() {
        let column = owner
            .sexp(column)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        result_mut
            .try_set_vector_elt(i as R_xlen_t, column)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_NamesSymbol(),
        string_vector(&names),
    );
    crate::sexp::attrib_core::setAttrib(
        result,
        install_str("class"),
        mk_string_cstr(c"data.frame"),
    );
    if !row_names.is_null() && row_names != R_NilValue() {
        crate::sexp::attrib_core::setAttrib(result, install_str("row.names"), row_names);
    }
    result
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
    if x.is_null() || x == R_NilValue() {
        return Vec::new();
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    let x_owned = owner_sexp(active_owner(), x);
    let mut data = Vec::with_capacity(n as usize);
    if t == SEXPTYPE::REALSXP {
        for i in 0..n {
            data.push(
                x_owned
                    .try_real_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            );
        }
    } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        for i in 0..n {
            let v = integer_like_elt(&x_owned, i);
            if v == NA_INTEGER {
                data.push(NA_REAL);
            } else {
                data.push(v as f64);
            }
        }
    }
    data
}

/// Extract a single element from a vector as a SEXP (for use with real_or_default).
pub(crate) fn elt_to_sexp(x: SEXP, i: R_xlen_t) -> SEXP {
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    let idx = if n == 0 { 0 } else { i % n };

    if t == SEXPTYPE::REALSXP {
        let v = owner_sexp(active_owner(), x)
            .try_real_elt(idx)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        Rf_ScalarReal(v)
    } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        let x_owned = owner_sexp(active_owner(), x);
        let v = integer_like_elt(&x_owned, idx);
        Rf_ScalarInteger(v)
    } else {
        R_NilValue()
    }
}

fn constructor_length(value: SEXP) -> R_xlen_t {
    if value.is_null() || value == R_NilValue() {
        return 0;
    }
    if XLENGTH(value) == 0 {
        return 0;
    }

    let value_owned = owner_sexp(active_owner(), value);
    let raw_len = match TYPEOF(value) {
        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => integer_like_elt(&value_owned, 0),
        t if t == SEXPTYPE::REALSXP => {
            let value = value_owned
                .try_real_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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

fn first_constructor_arg(args: SEXP, name: &str, position: usize) -> SEXP {
    {
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

fn allocate_initialized_vector(sexptype: SEXPTYPE, length: R_xlen_t) -> SEXP {
    let result = Rf_allocVector3(sexptype, length);
    if result.is_null() {
        return R_NilValue();
    }
    let _guard = protect(result);
    match sexptype {
        t if t == SEXPTYPE::STRSXP => {
            let owner = active_owner();
            if let (Ok(mut result_mut), Ok(empty)) = (
                owner.sexp(result).and_then(SexpMut::try_from_checked),
                owner.sexp(mk_char_cstr(c"")),
            ) {
                for i in 0..length {
                    let _ = result_mut.try_set_string_elt(i, empty.clone());
                }
            }
        }
        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
            let owner = active_owner();
            if let (Ok(mut result_mut), Ok(nil)) = (
                owner.sexp(result).and_then(SexpMut::try_from_checked),
                owner.sexp(R_NilValue()),
            ) {
                for i in 0..length {
                    let _ = result_mut.try_set_vector_elt(i, nil.clone());
                }
            }
        }
        _ => {}
    }
    result
}

fn do_typed_vector_constructor(args: SEXP, sexptype: SEXPTYPE) -> SEXP {
    {
        let length_arg = first_constructor_arg(args, "length", 0);
        let length = constructor_length(length_arg);
        allocate_initialized_vector(sexptype, length)
    }
}

/// R's `logical(length = 0)` constructor.
pub fn do_logical_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_typed_vector_constructor(args, SEXPTYPE::LGLSXP)
}

/// R's `integer(length = 0)` constructor.
pub fn do_integer_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_typed_vector_constructor(args, SEXPTYPE::INTSXP)
}

/// R's `numeric(length = 0)` / `double(length = 0)` constructor.
pub fn do_numeric_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_typed_vector_constructor(args, SEXPTYPE::REALSXP)
}

/// R's legacy `single(length = 0)` constructor.
pub fn do_single_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let result = do_typed_vector_constructor(args, SEXPTYPE::REALSXP);
    if result.is_null() || result == R_NilValue() {
        return result;
    }

    let _result_guard = protect(result);
    let marker = Rf_ScalarLogical(TRUE);
    let _marker_guard = protect(marker);
    crate::sexp::attrib_core::setAttrib(result, install_str("Csingle"), marker);
    result
}

/// R's `complex(length.out = 0, real = numeric(), imaginary = numeric(),
/// modulus = 1, argument = 0)` constructor.
///
/// Stock base is a closure: with neither `modulus` nor `argument` supplied
/// it calls `.Internal(complex(length.out, real, imaginary))` (main/complex.c
/// do_complex — zero-fill then recycle `pans[i].r = re[i %% nr]`,
/// `pans[i].i = im[i %% ni]`, `n = max(asInteger(length.out), nr, ni)`);
/// otherwise it builds `rep_len(modulus, n) * exp(1i * rep_len(argument, n))`
/// with `n = max(length.out, length(argument), length(modulus))`.
pub fn do_complex_constructor(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Upstream evaluates this inside base::complex's closure, so
    // coercion warnings raised here attribute to that call ("In
    // complex(...) : NAs introduced by coercion"). The port folds the
    // wrapper into one builtin with no closure context of its own, so
    // install the call as the warning attribution override for the
    // handler body; the guard restores the previous value even when a
    // base_error panic unwinds through.
    let _restore = crate::mainutils::errors::warning_call_guard(call);
    let supplied = match_complex_formals(args);

    // Wrapper: `if (missing(modulus) && missing(argument))`.
    if supplied[3].is_none() && supplied[4].is_none() {
        return complex_from_real_imaginary(&supplied);
    }
    complex_from_polar(&supplied)
}

/// Formals of base's `complex` closure, in positional order.
const COMPLEX_FORMALS: [&str; 5] = ["length.out", "real", "imaginary", "modulus", "argument"];

/// Closure-style argument match for `complex`: tagged arguments are matched
/// first (partial matching — the five formals start with distinct letters, so
/// a prefix matches one formal or none), then positional arguments fill the
/// remaining slots in order.
fn match_complex_formals(args: SEXP) -> [Option<SEXP>; 5] {
    {
        let mut slots: [Option<SEXP>; 5] = [None; 5];
        let mut positionals: Vec<SEXP> = Vec::new();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            match tag_name(cell).as_deref() {
                Some(tag) => {
                    let slot = match COMPLEX_FORMALS.iter().position(|f| f.starts_with(tag)) {
                        Some(slot) => slot,
                        None => base_error(format!(
                            "unused argument ({} = {})",
                            tag,
                            described_constructor_arg(CAR(cell))
                        )),
                    };
                    if slots[slot].is_some() {
                        base_error(format!(
                            "formal argument \"{}\" matched by multiple actual arguments",
                            COMPLEX_FORMALS[slot]
                        ));
                    }
                    slots[slot] = Some(CAR(cell));
                }
                None => positionals.push(CAR(cell)),
            }
            cell = CDR(cell);
        }
        let mut next = 0;
        for value in positionals {
            while next < slots.len() && slots[next].is_some() {
                next += 1;
            }
            if next == slots.len() {
                base_error(format!(
                    "unused argument ({})",
                    described_constructor_arg(value)
                ));
            }
            slots[next] = Some(value);
        }
        slots
    }
}

/// Render a constructor argument for an error message.
fn described_constructor_arg(value: SEXP) -> String {
    let deparsed = crate::mainutils::deparse::deparse_symbolic(value, true);
    if deparsed.is_null() || deparsed == R_NilValue() {
        return "...".to_string();
    }
    let _guard = protect(deparsed);
    if XLENGTH(deparsed) > 0 {
        elt_to_string(deparsed, 0)
    } else {
        "...".to_string()
    }
}

/// Coerce a constructor argument to REALSXP (stock coerceVector returns the
/// input unchanged when it is already real; NULL coerces to an empty real).
fn complex_real_part(x: SEXP) -> SEXP {
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
    }
    if TYPEOF(x) == SEXPTYPE::REALSXP {
        return x;
    }
    crate::mainutils::coerce::coerceVector(x, SEXPTYPE::REALSXP.as_c_int())
}

/// `.Internal(complex(length.out, real, imaginary))`.
fn complex_from_real_imaginary(supplied: &[Option<SEXP>; 5]) -> SEXP {
    let na = crate::mainutils::coerce::asInteger(match supplied[0] {
        Some(v) => v,
        None => Rf_ScalarInteger(0), // wrapper default `length.out = 0L`
    });
    if na == NA_INTEGER || na < 0 {
        base_error("invalid length");
    }
    let re = complex_real_part(supplied[1].unwrap_or(R_NilValue()));
    let _re_guard = protect(re);
    let im = complex_real_part(supplied[2].unwrap_or(R_NilValue()));
    let _im_guard = protect(im);
    let nr = XLENGTH(re);
    let ni = XLENGTH(im);
    let mut n = na as R_xlen_t;
    if nr > n {
        n = nr;
    }
    if ni > n {
        n = ni;
    }

    let ans = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
    if ans.is_null() {
        return R_NilValue();
    }
    let _ans_guard = protect(ans);
    let mut pans = owner_sexp_mut(ans);
    for i in 0..n {
        let r = if nr > 0 {
            real_elt_checked(re, ((i % nr) as usize) as R_xlen_t)
        } else {
            0.0
        };
        let im_part = if ni > 0 {
            real_elt_checked(im, ((i % ni) as usize) as R_xlen_t)
        } else {
            0.0
        };
        sexp_ok(pans.try_set_complex_elt(i as R_xlen_t, Rcomplex { r, i: im_part }));
    }
    ans
}

/// Wrapper's polar branch:
/// `n <- max(length.out, length(argument), length(modulus));
///  rep_len(modulus, n) * exp(1i * rep_len(argument, n))`.
fn complex_from_polar(supplied: &[Option<SEXP>; 5]) -> SEXP {
    let m = complex_real_part(match supplied[3] {
        Some(v) => v,               // supplied `modulus`
        None => Rf_ScalarReal(1.0), // wrapper default `modulus = 1`
    });
    let _m_guard = protect(m);
    let a = complex_real_part(match supplied[4] {
        Some(v) => v,               // supplied `argument`
        None => Rf_ScalarReal(0.0), // wrapper default `argument = 0`
    });
    let _a_guard = protect(a);

    // max(<length.out elements>, length(argument), length(modulus));
    // any NA in length.out propagates (na.rm = FALSE).
    let lo = complex_real_part(supplied[0].unwrap_or(R_NilValue()));
    let _lo_guard = protect(lo);
    let mut lo_max = f64::NEG_INFINITY;
    let lo_n = XLENGTH(lo);
    let lo_owned = owner_sexp(active_owner(), lo);
    for i in 0..lo_n {
        let e = lo_owned
            .try_real_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if e.is_nan() {
            lo_max = f64::NAN;
            break;
        }
        if e > lo_max {
            lo_max = e;
        }
    }
    let n = if lo_n == 0 {
        (XLENGTH(a) as f64).max(XLENGTH(m) as f64)
    } else if lo_max.is_nan() {
        // max(na.rm = FALSE): an NA in length.out propagates. f64::max
        // would silently swallow it.
        f64::NAN
    } else {
        lo_max.max(XLENGTH(a) as f64).max(XLENGTH(m) as f64)
    };

    // do_rep_len: length.out goes through asInteger (with its coercion
    // warning), and NA/negative lengths error.
    let n_len = crate::mainutils::coerce::asInteger(Rf_ScalarReal(n));
    if n_len == NA_INTEGER || n_len < 0 {
        base_error("invalid 'length.out' value");
    }
    if n_len == 0 {
        return Rf_allocVector3(SEXPTYPE::CPLXSXP, 0);
    }

    let n = n_len as R_xlen_t;
    let mlen = XLENGTH(m);
    let alen = XLENGTH(a);
    let ans = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
    if ans.is_null() {
        return R_NilValue();
    }
    let _ans_guard = protect(ans);
    let mut pans = owner_sexp_mut(ans);
    for i in 0..n {
        // rep_len: an empty operand rep_len's to NA.
        let mv = if mlen == 0 {
            NA_REAL
        } else {
            real_elt_checked(m, ((i % mlen) as usize) as R_xlen_t)
        };
        let av = if alen == 0 {
            NA_REAL
        } else {
            real_elt_checked(a, (i % alen) as R_xlen_t)
        };
        // exp(1i * av) is (cos, sin); the full C99 complex multiply by
        // (mv, 0) reproduces stock edge cases (modulus=Inf, argument=0
        // gives Inf+NaNi, an all-NA operand gives NA).
        let (ct, st) = (av.cos(), av.sin());
        sexp_ok(pans.try_set_complex_elt(
            (i as usize) as R_xlen_t,
            Rcomplex {
                r: mv * ct - 0.0 * st,
                i: mv * st + 0.0 * ct,
            },
        ));
    }
    ans
}

/// R's `character(length = 0)` constructor.
pub fn do_character_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_typed_vector_constructor(args, SEXPTYPE::STRSXP)
}
/// R's `raw(length = 0)` constructor.
pub fn do_raw_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_typed_vector_constructor(args, SEXPTYPE::RAWSXP)
}

/// R's `vector(mode = "logical", length = 0)` constructor.
pub fn do_vector_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
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
pub fn do_as_complex(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner =
            current_owner().unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let pin = owner
            .pin()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let factory = owner.node_factory();
        let _arguments = factory
            .wrap(args)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let _call = (!call.is_null()).then(|| {
            factory
                .wrap(call)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        });
        let _operation = (!op.is_null()).then(|| {
            factory
                .wrap(op)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        });
        let _environment = (!rho.is_null()).then(|| {
            factory
                .wrap(rho)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        });
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            }
            factory
                .require_active()
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        };
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchOrEval(
            call,
            op,
            c"as.complex".as_ptr(),
            args,
            rho,
            &mut ans,
            0,
            1,
        ) != 0
        {
            check();
            return factory
                .wrap(ans)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
                .as_raw();
        }
        let source = factory
            .wrap(CAR(args))
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        // The shared default path strips metadata and copies same-type attributed
        // values before clearing it; method results above retain their attributes.
        let result = crate::mainutils::coerce::as_atomic_safe(source, 3)
            .unwrap_or_else(|message| crate::mainutils::errors::errorcall_str(call, &message));
        check();
        factory
            .wrap(result)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
            .as_raw()
    }
}

/// R's `as.raw(x)` — coerce to RAWSXP.
pub fn do_as_raw(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner =
            current_owner().unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let pin = owner
            .pin()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let factory = owner.node_factory();
        let _arguments = factory
            .wrap(args)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let _call = (!call.is_null()).then(|| {
            factory
                .wrap(call)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        });
        let _operation = (!op.is_null()).then(|| {
            factory
                .wrap(op)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        });
        let _environment = (!rho.is_null()).then(|| {
            factory
                .wrap(rho)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        });
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            }
            factory
                .require_active()
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        };
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchOrEval(
            call,
            op,
            c"as.raw".as_ptr(),
            args,
            rho,
            &mut ans,
            0,
            1,
        ) != 0
        {
            check();
            return factory
                .wrap(ans)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
                .as_raw();
        }
        let source = factory
            .wrap(CAR(args))
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        // The shared default path strips metadata and copies same-type attributed
        // values before clearing it; method results above retain their attributes.
        let result = crate::mainutils::coerce::as_atomic_safe(source, 5)
            .unwrap_or_else(|message| crate::mainutils::errors::errorcall_str(call, &message));
        check();
        factory
            .wrap(result)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
            .as_raw()
    }
}

/// R's `as(x, Class)` — S4-style coercion (simplified: delegates to appropriate as.* function).
pub fn do_as(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let x = CAR(args);
        let class_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || class_arg.is_null() || class_arg == R_NilValue() {
            return x;
        }
        let class_name = elt_to_string(class_arg, 0);
        match class_name.as_str() {
            "numeric" if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::REALSXP => x,
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
pub fn do_subset_named(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        // Delegate to existing do_subset
        do_subset(_call, _op, args, _rho)
    }
}

// ---------------------------------------------------------------------------
// Complete S3 — method dispatch
// ---------------------------------------------------------------------------

/// Names in `tools::nonS3methods` that exist in base R and are not methods.
fn is_known_non_s3_method(generic: &str, class: &str) -> bool {
    matches!(
        (generic, class),
        ("t", "test")
            | ("all", "equal")
            | ("all", "names")
            | ("all", "vars")
            | ("max", "col")
            | ("rep", "int")
            | ("seq", "int")
            | ("sort", "int")
            | ("sort", "list")
            | ("format", "info")
            | ("format", "pval")
            | ("plot", "new")
            | ("plot", "window")
            | ("plot", "xy")
            | ("close", "screen")
            | ("split", "screen")
            | ("anova", "lmlist")
            | ("lag", "plot")
    )
}

/// R's `getS3method(generic, class)` — get S3 method function.
pub fn do_getS3method(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
    if is_function_value(method) && !is_known_non_s3_method(&generic, &class) {
        return method;
    }
    let optional = {
        let rest = CDR(CDR(args));
        if rest.is_null() || rest == R_NilValue() {
            false
        } else {
            let value = CAR(rest);
            TYPEOF(value) == SEXPTYPE::LGLSXP
                && LENGTH(value) > 0
                && owner_sexp(active_owner(), value)
                    .try_logical_elt(0)
                    .map(|v| v == TRUE)
                    .unwrap_or(false)
        }
    };
    if optional {
        R_NilValue()
    } else {
        package_error(format!("S3 method '{generic}.{class}' not found"));
    }
}

/// R's `hasS3method(generic, class)` — check if S3 method exists.
pub fn do_hasS3method(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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

/// R's `registerS3method(generic, class, method)` — register S3 method.
pub fn do_registerS3method(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let generic = elt_to_string(CAR(args), 0);
    let class = elt_to_string(CAR(CDR(args)), 0);
    let mut method = CAR(CDR(CDR(args)));
    let env_arg = CDR(CDR(CDR(args)));
    let target_env = if !env_arg.is_null() && env_arg != R_NilValue() {
        CAR(env_arg)
    } else {
        rho
    };
    if TYPEOF(method) == SEXPTYPE::PROMSXP {
        force_promise(method);
        method = crate::sexp::accessors::PRVALUE(method);
    }
    if TYPEOF(method) == SEXPTYPE::SYMSXP {
        let looked = find_fun(method, target_env);
        if is_function_value(looked) {
            method = looked;
        } else {
            let looked = find_fun(method, rho);
            if is_function_value(looked) {
                method = looked;
            }
        }
    }
    if !is_function_value(method) {
        let composed = format!("{generic}.{class}");
        if let Ok(cname) = CString::new(composed) {
            let sym = install_cstr(&cname);
            let mut bound = find_var_in_frame(target_env, sym);
            if TYPEOF(bound) == SEXPTYPE::PROMSXP {
                force_promise(bound);
                bound = crate::sexp::accessors::PRVALUE(bound);
            }
            if is_function_value(bound) {
                method = bound;
            }
        }
    }
    let gen_sym = {
        let cname = CString::new(generic.clone()).unwrap_or_default();
        install_cstr(&cname)
    };
    let genfun = find_fun(gen_sym, target_env);
    let defenv = if TYPEOF(genfun) == SEXPTYPE::CLOSXP {
        crate::sexp::accessors::CLOENV(genfun)
    } else {
        crate::eval::runtime::base_env()
    };
    if let Err(message) = define_s3_method(defenv, &generic, &class, method) {
        package_error(message);
    }
    crate::sexp::globals::set_R_Visible(FALSE);
    R_NilValue()
}

pub(crate) fn s3_methods_table_symbol() -> SEXP {
    crate::mainutils::objects::S3MethodsTable_symbol()
}

pub(crate) fn ensure_s3_methods_table(env: SEXP) -> Result<SEXP, String> {
    if env.is_null() || env == R_NilValue() || TYPEOF(env) != SEXPTYPE::ENVSXP {
        return Err("S3 method registration requires an environment".to_string());
    }

    let table_sym = s3_methods_table_symbol();
    let existing = find_var_in_frame(env, table_sym);
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
    define_var(table_sym, table, env);
    Ok(table)
}

pub(crate) fn define_s3_method(
    env: SEXP,
    generic: &str,
    class: &str,
    method: SEXP,
) -> Result<(), String> {
    if !is_function_value(method) && TYPEOF(method) != SEXPTYPE::PROMSXP {
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
    define_var(method_sym, method, table);
    Ok(())
}

fn effective_s3_defrho(rho: SEXP) -> SEXP {
    if rho.is_null() || rho == R_NilValue() || TYPEOF(rho) != SEXPTYPE::ENVSXP {
        crate::sexp::globals::R_GlobalEnv()
    } else {
        let namespace_env = find_var_in_frame(rho, namespace_env_symbol());
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

pub(crate) fn is_function_value(value: SEXP) -> bool {
    {
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

fn initialize_generic_dispatch_tables(generic: SEXP) {
    if generic.is_null() || generic == R_NilValue() || TYPEOF(generic) != SEXPTYPE::CLOSXP {
        return;
    }

    let f_env = crate::sexp::accessors::CLOENV(generic);
    if f_env.is_null() || TYPEOF(f_env) != SEXPTYPE::ENVSXP {
        return;
    }

    let all_mtable_sym = install_str(".AllMTable");
    let existing_mtable = find_var_in_frame(f_env, all_mtable_sym);
    if existing_mtable == R_UnboundValue() || TYPEOF(existing_mtable) != SEXPTYPE::ENVSXP {
        let mtable = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), f_env, R_NilValue());
        if !mtable.is_null() {
            let _table_guard = protect(mtable);
            define_var(all_mtable_sym, mtable, f_env);
        }
    }

    let sig_args_sym = install_str(".SigArgs");
    let sig_length_sym = install_str(".SigLength");
    let existing_sig_args = find_var_in_frame(f_env, sig_args_sym);
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
    let _sigargs_guard = protect(sigargs);
    let owner = active_owner();
    if let Ok(mut sigargs_mut) = owner.sexp(sigargs).and_then(SexpMut::try_from_checked) {
        for (index, sym) in arg_syms.iter().enumerate() {
            if let Ok(sym_owned) = owner.sexp(*sym) {
                let _ = sigargs_mut.try_set_vector_elt(index as R_xlen_t, sym_owned);
            }
        }
    }
    define_var(sig_args_sym, sigargs, f_env);
    define_var(
        sig_length_sym,
        Rf_ScalarInteger(arg_syms.len() as c_int),
        f_env,
    );
}

/// R's `setGeneric(f, fdef, ...)` — set generic function.
pub fn do_setGeneric(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let f_arg = CAR(args);
    let fdef_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
        R_NilValue()
    } else {
        CAR(CDR(args))
    };

    let mut generic = if !fdef_arg.is_null() && fdef_arg != R_NilValue() {
        fdef_arg
    } else {
        f_arg
    };

    // Upstream setGeneric rebinds the name in the defining environment to
    // the generic closure and marks it as an object carrying the "generic"
    // attribute, so standardGeneric can find it via get_this_generic.
    if !generic.is_null() && generic != R_NilValue() && TYPEOF(generic) == SEXPTYPE::CLOSXP {
        let source_generic = generic;
        generic = crate::mainutils::duplicate::Rf_duplicate(generic);
        let _generic_guard = protect(generic);
        // Each generic owns its dispatch metadata. Its lexical environment
        // remains the parent, so captured bindings keep their usual meaning.
        let dispatch_env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::accessors::CLOENV(generic),
            R_NilValue(),
        );
        let _dispatch_guard = protect(dispatch_env);
        crate::sexp::accessors::SET_CLOENV(generic, dispatch_env);
        initialize_generic_dispatch_tables(generic);

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
            let name_sym = find_binding_name_for_value(target_env, source_generic);
            if let Some(nm) = name_sym {
                let pname = PRINTNAME(nm);
                if !pname.is_null() && pname != R_NilValue() {
                    name_sexp = Some(elt_to_string(pname, 0));
                }
            }
        }

        if let Some(name) = name_sexp.filter(|n| !n.is_empty()) {
            let cname = CString::new(name.clone()).unwrap_or_default();
            let name_sym = install_cstr(&cname);
            let name_str = mk_string_cstr(&cname);
            let _name_guard = protect(name_str);

            // Mark the closure as a generic function object: attribute
            // "generic" = the generic's name, plus the OBJECT bit.
            let generic_sym = install_str("generic");
            crate::mainutils::relop::setAttrib(generic, generic_sym, name_str);
            crate::sexp::accessors::SET_OBJECT(generic, 1);

            // Rebind the name to the generic closure in the calling frame.
            let target_env =
                if !rho.is_null() && rho != R_NilValue() && TYPEOF(rho) == SEXPTYPE::ENVSXP {
                    rho
                } else {
                    crate::sexp::globals::R_GlobalEnv()
                };
            let existing = find_var_in_frame(target_env, name_sym);
            let already_generic = !existing.is_null()
                && existing != R_UnboundValue()
                && is_function_type(existing)
                && crate::eval::attrib_core::isObject(existing) != FALSE;
            if !already_generic {
                define_var(name_sym, generic, target_env);
            }
        }
    }

    generic
}

fn is_function_type(value: SEXP) -> bool {
    {
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
fn find_binding_name_for_value(env: SEXP, value: SEXP) -> Option<SEXP> {
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
        let hashtab_owned = owner_sexp(active_owner(), hashtab);
        for i in 0..n {
            let bucket = hashtab_owned
                .try_vector_elt(i)
                .map(|elt| elt.as_raw())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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

/// R's `setMethod(f, signature, definition, ...)` — set S4 method.
pub fn do_setMethod(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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

    let all_mtable_sym = install_str(".AllMTable");
    let existing = find_var_in_frame(f_env, all_mtable_sym);
    let mtable = if !existing.is_null()
        && existing != R_UnboundValue()
        && TYPEOF(existing) == SEXPTYPE::ENVSXP
    {
        existing
    } else {
        let table = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), f_env, R_NilValue());
        if table.is_null() {
            return definition;
        }
        let _table_guard = protect(table);
        define_var(all_mtable_sym, table, f_env);
        table
    };

    let label = if signature_arg.is_null() || signature_arg == R_NilValue() {
        String::new()
    } else if TYPEOF(signature_arg) == SEXPTYPE::STRSXP {
        let n = XLENGTH(signature_arg);
        let names = crate::sexp::attrib_core::getAttrib(signature_arg, install_str("names"));
        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            let sigargs = find_var_in_frame(f_env, install_str(".SigArgs"));
            if sigargs == R_UnboundValue() || TYPEOF(sigargs) != SEXPTYPE::VECSXP {
                base_error("generic has no valid signature arguments");
            }
            let sigargs_owned = owner_sexp(active_owner(), sigargs);
            let formals: Vec<String> = (0..XLENGTH(sigargs))
                .map(|i| {
                    let elt = sigargs_owned
                        .try_vector_elt(i)
                        .map(|elt| elt.as_raw())
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    elt_to_string(elt, 0)
                })
                .collect();
            let mut ordered = vec!["ANY".to_string(); formals.len()];
            let mut assigned = vec![false; formals.len()];
            for i in 0..n {
                let name = elt_to_string(names, i);
                let index = if name.is_empty() {
                    i as usize
                } else {
                    formals
                        .iter()
                        .position(|formal| formal == &name)
                        .unwrap_or_else(|| {
                            base_error("named signature argument is not in the generic")
                        })
                };
                if index >= ordered.len() || assigned[index] {
                    base_error("invalid or duplicated method signature argument");
                }
                ordered[index] = elt_to_string(signature_arg, i);
                assigned[index] = true;
            }
            ordered.join("#")
        } else {
            let sigargs = find_var_in_frame(f_env, install_str(".SigArgs"));
            if sigargs == R_UnboundValue() || TYPEOF(sigargs) != SEXPTYPE::VECSXP {
                base_error("generic has no valid signature arguments");
            }
            let arity = XLENGTH(sigargs);
            if n > arity {
                base_error("method signature is longer than the generic signature");
            }
            (0..arity)
                .map(|i| {
                    if i < n {
                        elt_to_string(signature_arg, i)
                    } else {
                        "ANY".to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("#")
        }
    } else {
        elt_to_string(signature_arg, 0)
    };

    let method_sym = install_cstr(&CString::new(label).unwrap_or_default());
    define_var(method_sym, definition, mtable);

    definition
}
