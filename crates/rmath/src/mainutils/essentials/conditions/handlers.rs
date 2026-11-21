use super::*;
use crate::sexp::object::SexpMut;

// ---------------------------------------------------------------------------
// Complete error handling — calling handlers and restarts
// ---------------------------------------------------------------------------

/// R's `withCallingHandlers(expr, ...)` evaluates handlers before unwinding.
pub fn do_withCallingHandlers(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // GNU formals `(expr, ...)`. `expr=` binds by name; other tags are handlers.
        let expr_sym = install_str("expr");
        let mut expr = crate::sexp::globals::R_MissingArg();
        let mut handler_args = R_NilValue();
        let mut p = args;
        let mut handler_cells = Vec::new();
        while !p.is_null() && p != R_NilValue() {
            let tag = TAG(p);
            if !tag.is_null() && tag != R_NilValue() && tag == expr_sym {
                expr = CAR(p);
            } else if tag.is_null() || tag == R_NilValue() {
                if expr == crate::sexp::globals::R_MissingArg() {
                    expr = CAR(p);
                }
            } else {
                handler_cells.push((tag, CAR(p)));
            }
            p = CDR(p);
        }
        for (tag, val) in handler_cells.into_iter().rev() {
            let cell = Rf_cons(val, handler_args);
            SETTAG(cell, tag);
            handler_args = cell;
        }
        let _ha = protect(handler_args);
        if expr.is_null() || expr == R_NilValue() || expr == crate::sexp::globals::R_MissingArg() {
            crate::mainutils::errors::errorcall_str(
                _call,
                "argument \"expr\" is missing, with no default",
            );
        }

        let old_stack = condition_handler_stack();
        let new_stack = calling_handler_stack_from_args(handler_args, rho, old_stack);
        set_condition_handler_stack(new_stack);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        set_condition_handler_stack(old_stack);

        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

fn handlers_structurally_equal(a: SEXP, b: SEXP) -> bool {
    if a == b {
        return true;
    }
    if TYPEOF(a) != TYPEOF(b) {
        return false;
    }
    if TYPEOF(a) != SEXPTYPE::CLOSXP {
        return compute_identical(a, b, 32) != 0;
    }
    if crate::sexp::accessors::CLOENV(a) != crate::sexp::accessors::CLOENV(b) {
        return false;
    }
    if compute_identical(
        crate::sexp::accessors::FORMALS(a),
        crate::sexp::accessors::FORMALS(b),
        0,
    ) == 0
    {
        return false;
    }
    if !lang_equal_ignore_srcref(
        crate::sexp::accessors::BODY(a),
        crate::sexp::accessors::BODY(b),
    ) {
        return false;
    }
    attribs_equal_ignore_source(
        crate::sexp::accessors::ATTRIB(a),
        crate::sexp::accessors::ATTRIB(b),
    )
}

fn lang_equal_ignore_srcref(a: SEXP, b: SEXP) -> bool {
    if a == b {
        return true;
    }
    if a.is_null() || b.is_null() || a == R_NilValue() || b == R_NilValue() {
        return a == b || (a == R_NilValue() && b == R_NilValue());
    }
    if TYPEOF(a) != TYPEOF(b) {
        return false;
    }
    if TYPEOF(a) == SEXPTYPE::LANGSXP || TYPEOF(a) == SEXPTYPE::LISTSXP {
        lang_equal_ignore_srcref(CAR(a), CAR(b)) && lang_equal_ignore_srcref(CDR(a), CDR(b))
    } else {
        compute_identical(a, b, 0) != 0
    }
}

fn attribs_equal_ignore_source(mut a: SEXP, mut b: SEXP) -> bool {
    fn skip_src(mut p: SEXP) -> SEXP {
        unsafe {
            while !p.is_null() && p != R_NilValue() {
                let tag = TAG(p);
                let name = if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                    let c = crate::sexp::accessors::CHAR(PRINTNAME(tag));
                    if c.is_null() {
                        ""
                    } else {
                        std::ffi::CStr::from_ptr(c).to_str().unwrap_or("")
                    }
                } else {
                    ""
                };
                if name == "srcref" || name == "srcfile" || name == "wholeSrcref" {
                    p = CDR(p);
                    continue;
                }
                break;
            }
            p
        }
    }
    a = skip_src(a);
    b = skip_src(b);
    compute_identical(a, b, 0) != 0
}

/// GNU `globalCallingHandlers(...)` — register/inspect/clear global calling handlers.
pub fn do_globalCallingHandlers(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    if args.is_null() || args == R_NilValue() {
        crate::sexp::globals::set_R_Visible(TRUE);
        return global_handlers_list();
    }

    let first = CAR(args);
    let rest = CDR(args);
    let single = rest.is_null() || rest == R_NilValue();

    if single && (first.is_null() || first == R_NilValue()) {
        refuse_if_local_handlers();
        let old = global_handlers_list();
        let _old = protect(old);
        set_global_handlers_list(empty_named_list());
        install_global_handler_stack(rho);

        crate::sexp::globals::set_R_Visible(FALSE);
        return old;
    }

    let incoming = if single
        && TYPEOF(first) == SEXPTYPE::VECSXP
        && (TAG(args).is_null() || TAG(args) == R_NilValue())
    {
        first
    } else {
        named_list_from_dots(args)
    };
    let _incoming = protect(incoming);
    validate_named_handlers(incoming);
    refuse_if_local_handlers();

    let combined = prepend_handlers(incoming, global_handlers_list());
    let _combined = protect(combined);
    let combined = drop_duplicate_class_handlers(combined);
    let _dedup = protect(combined);
    set_global_handlers_list(combined);
    install_global_handler_stack(rho);

    crate::sexp::globals::set_R_Visible(FALSE);
    R_NilValue()
}

fn global_handlers_list() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        let gh = (*inst).error_state.global_calling_handlers.as_raw();
        if gh.is_null() || gh == R_NilValue() {
            empty_named_list()
        } else {
            gh
        }
    })
}

fn set_global_handlers_list(list: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).error_state.global_calling_handlers =
            crate::sexp::instance::RuntimeValue::from_raw_in(inst, list);
    });
}

fn empty_named_list() -> SEXP {
    Rf_allocVector3(SEXPTYPE::VECSXP, 0)
}

fn named_list_from_dots(mut args: SEXP) -> SEXP {
    let mut n = 0;
    let mut p = args;
    while !p.is_null() && p != R_NilValue() {
        n += 1;
        p = CDR(p);
    }
    let list = Rf_allocVector3(SEXPTYPE::VECSXP, n);
    let _list = protect(list);
    let names = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    let _names = protect(names);
    let owner = active_owner();
    let mut list_mut = owner
        .sexp(list)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut names_mut = owner
        .sexp(names)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut i = 0i64;
    while !args.is_null() && args != R_NilValue() {
        let value = owner_sexp(owner, CAR(args));
        list_mut
            .try_set_vector_elt(i, value)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let tag = TAG(args);
        let charsxp = if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP
        {
            owner
                .sexp(mk_char_cstr(c""))
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        } else {
            owner
                .sexp(PRINTNAME(tag))
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        };
        names_mut
            .try_set_string_elt(i, charsxp)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        i += 1;
        args = CDR(args);
    }
    crate::sexp::attrib_core::setAttrib(list, crate::sexp::attrib_core::R_NamesSymbol(), names);
    list
}

fn validate_named_handlers(list: SEXP) {
    let n = XLENGTH(list);
    let names =
        crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
    if n > 0 && (names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP) {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "condition handlers must be specified with a condition class".to_string(),
        });
    }
    let owner = active_owner();
    let names_owned = owner_sexp(owner, names);
    let list_owned = owner_sexp(owner, list);
    for i in 0..n {
        if names.is_null()
            || names == R_NilValue()
            || names_owned
                .try_string_elt(i)
                .map(|elt| {
                    elt.as_raw().is_null()
                        || elt
                            .header()
                            .char_bytes()
                            .map(|bytes| bytes.is_empty())
                            .unwrap_or(true)
                })
                .unwrap_or(true)
        {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "condition handlers must be specified with a condition class".to_string(),
            });
        }
        let elt = list_owned
            .try_vector_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if !crate::mainutils::essentials::is_function_value(elt.as_raw()) {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "condition handlers must be functions".to_string(),
            });
        }
    }
}

fn prepend_handlers(new: SEXP, old: SEXP) -> SEXP {
    let nn = XLENGTH(new);
    let no = if old.is_null() || old == R_NilValue() {
        0
    } else {
        XLENGTH(old)
    };
    let out = Rf_allocVector3(SEXPTYPE::VECSXP, nn + no);
    let _out = protect(out);
    let names = Rf_allocVector3(SEXPTYPE::STRSXP, nn + no);
    let _names = protect(names);
    let new_names =
        crate::sexp::attrib_core::getAttrib(new, crate::sexp::attrib_core::R_NamesSymbol());
    let old_names = if old.is_null() || old == R_NilValue() {
        R_NilValue()
    } else {
        crate::sexp::attrib_core::getAttrib(old, crate::sexp::attrib_core::R_NamesSymbol())
    };
    let owner = active_owner();
    let new_owned = owner_sexp(owner, new);
    let new_names_owned = owner_sexp(owner, new_names);
    let mut out_mut = owner
        .sexp(out)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut names_mut = owner
        .sexp(names)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for i in 0..nn {
        let elt = new_owned
            .try_vector_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        out_mut
            .try_set_vector_elt(i, elt)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if !new_names.is_null() && new_names != R_NilValue() {
            let name = new_names_owned
                .try_string_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            names_mut
                .try_set_string_elt(i, name)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
    let old_owned = owner_sexp(owner, old);
    let old_names_owned = owner_sexp(owner, old_names);
    for i in 0..no {
        let elt = old_owned
            .try_vector_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        out_mut
            .try_set_vector_elt(nn + i, elt)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if !old_names.is_null() && old_names != R_NilValue() {
            let name = old_names_owned
                .try_string_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            names_mut
                .try_set_string_elt(nn + i, name)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
    crate::sexp::attrib_core::setAttrib(out, crate::sexp::attrib_core::R_NamesSymbol(), names);
    out
}

fn drop_duplicate_class_handlers(list: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(list);
        if n <= 1 {
            return list;
        }
        let names =
            crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
        let mut keep = vec![true; n as usize];
        let owner = active_owner();
        let names_owned = owner_sexp(owner, names);
        let list_owned = owner_sexp(owner, list);
        for i in 0..n {
            if !keep[i as usize] {
                continue;
            }
            let mut dropped = false;
            for j in (i + 1)..n {
                if !keep[j as usize] {
                    continue;
                }
                let name_i = names_owned
                    .try_string_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    .as_raw();
                let name_j = names_owned
                    .try_string_elt(j)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    .as_raw();
                if !chars_equal(name_i, name_j) {
                    continue;
                }
                let handler_i = list_owned
                    .try_vector_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    .as_raw();
                let handler_j = list_owned
                    .try_vector_elt(j)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    .as_raw();
                if handlers_structurally_equal(handler_i, handler_j) {
                    keep[j as usize] = false;
                    dropped = true;
                }
            }
            if dropped {
                let class_name = elt_to_string(names, i);
                let msg = crate::sexp::constructors::Rf_mkString(
                    std::ffi::CString::new(format!(
                        "pushing duplicate `{class_name}` handler on top of the stack"
                    ))
                    .unwrap_or_default()
                    .as_ptr(),
                );
                crate::mainutils::essentials::do_message(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    crate::sexp::constructors::Rf_cons(msg, R_NilValue()),
                    R_NilValue(),
                );
            }
        }
        let kept = keep.iter().filter(|k| **k).count() as i64;
        if kept == n {
            return list;
        }
        let out = Rf_allocVector3(SEXPTYPE::VECSXP, kept);
        let _out = protect(out);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, kept);
        let _on = protect(out_names);
        let mut k = 0i64;
        let mut out_mut = owner
            .sexp(out)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut out_names_mut = owner
            .sexp(out_names)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        for i in 0..n {
            if keep[i as usize] {
                let elt = list_owned
                    .try_vector_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                out_mut
                    .try_set_vector_elt(k, elt)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let name = names_owned
                    .try_string_elt(i)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                out_names_mut
                    .try_set_string_elt(k, name)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                k += 1;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            out,
            crate::sexp::attrib_core::R_NamesSymbol(),
            out_names,
        );
        out
    }
}

fn chars_equal(a: SEXP, b: SEXP) -> bool {
    {
        if a.is_null() || b.is_null() {
            return a == b;
        }
        let owner = active_owner();
        let bytes =
            |value: SEXP| -> Option<Vec<u8>> { owner_sexp(owner, value).header().char_bytes() };
        match (bytes(a), bytes(b)) {
            (Some(ca), Some(cb)) => ca == cb,
            // Original compared raw CHAR pointers: both missing counts equal.
            (None, None) => true,
            _ => false,
        }
    }
}

fn refuse_if_local_handlers() {
    {
        let gh = global_handlers_list();
        let n = if gh.is_null() || gh == R_NilValue() {
            0
        } else {
            XLENGTH(gh)
        };
        if pairlist_len(condition_handler_stack()) > n {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "should not be called with handlers on the stack".to_string(),
            });
        }
    }
}

fn install_global_handler_stack(rho: SEXP) {
    let factory = condition_entry_factory();
    let rho = condition_entry_value(&factory, rho);
    let gh = condition_entry_value(&factory, global_handlers_list());
    let n = if gh.is_nil() { 0 } else { gh.len() };
    let names = if n == 0 {
        factory.nil()
    } else {
        condition_entry_value(
            &factory,
            crate::sexp::attrib_core::getAttrib(
                gh.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
            ),
        )
    };
    let mut stack = factory.nil();
    for i in (0..n).rev() {
        let class_name = elt_to_string(names.as_raw(), i);
        let handler = condition_entry_value(&factory, VECTOR_ELT(gh.as_raw(), i));
        let entry = calling_handler_entry(&factory, &class_name, handler, &rho);
        stack = factory
            .pairlist_cell(&entry, &stack, &factory.nil())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    set_condition_handler_stack(stack.as_raw());
}

fn pairlist_len(mut p: SEXP) -> i64 {
    {
        let mut n = 0i64;
        while !p.is_null() && p != R_NilValue() {
            n += 1;
            p = CDR(p);
        }
        n
    }
}

/// GNU `simpleCondition(message, call = NULL)`.
pub fn do_simpleCondition(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    super::condition_objects::native_simple_condition(args)
}

/// GNU `signalCondition(cond)` — invoke matching calling handlers.
pub fn do_signalCondition_r(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let mut cond = if args.is_null() || args == R_NilValue() {
        R_NilValue()
    } else {
        CAR(args)
    };
    if cond.is_null()
        || cond == R_NilValue()
        || crate::mainutils::objects::inherits2(cond, c"condition") == 0
    {
        let msg = elt_to_string(cond, 0);
        cond = simple_condition(&msg, &["simpleCondition", "condition"]);
    }
    let _cond = protect(cond);
    signal_calling_handlers(cond, rho);
    // GNU's .signalCondition primitive returns a visible NULL regardless
    // of a returning calling handler's own visibility.
    crate::sexp::globals::set_R_Visible(TRUE);
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Exiting handlers (tryCatch) for warning conditions
// ---------------------------------------------------------------------------
