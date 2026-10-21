use super::*;

// ---------------------------------------------------------------------------
// Complete error handling — calling handlers and restarts
// ---------------------------------------------------------------------------

/// R's `withCallingHandlers(expr, ...)` evaluates handlers before unwinding.
pub unsafe fn do_withCallingHandlers(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // GNU formals `(expr, ...)`. `expr=` binds by name; other tags are handlers.
        let expr_sym = Rf_install(c"expr".as_ptr());
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

unsafe fn handlers_structurally_equal(a: SEXP, b: SEXP) -> bool {
    unsafe {
        if a == b {
            return true;
        }
        if TYPEOF(a) != TYPEOF(b) {
            return false;
        }
        if TYPEOF(a) != SEXPTYPE::CLOSXP {
            return crate::mainutils::identical::R_compute_identical(a, b, 32) != 0;
        }
        if crate::sexp::accessors::CLOENV(a) != crate::sexp::accessors::CLOENV(b) {
            return false;
        }
        if crate::mainutils::identical::R_compute_identical(
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
}

unsafe fn lang_equal_ignore_srcref(a: SEXP, b: SEXP) -> bool {
    unsafe {
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
            crate::mainutils::identical::R_compute_identical(a, b, 0) != 0
        }
    }
}

unsafe fn attribs_equal_ignore_source(mut a: SEXP, mut b: SEXP) -> bool {
    unsafe {
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
        crate::mainutils::identical::R_compute_identical(a, b, 0) != 0
    }
}

/// GNU `globalCallingHandlers(...)` — register/inspect/clear global calling handlers.
pub unsafe fn do_globalCallingHandlers(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
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

unsafe fn empty_named_list() -> SEXP {
    unsafe { Rf_allocVector3(SEXPTYPE::VECSXP, 0) }
}

unsafe fn named_list_from_dots(mut args: SEXP) -> SEXP {
    unsafe {
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
        let mut i = 0i64;
        while !args.is_null() && args != R_NilValue() {
            SET_VECTOR_ELT(list, i, CAR(args));
            let tag = TAG(args);
            if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
                SET_STRING_ELT(names, i, Rf_mkChar(c"".as_ptr()));
            } else {
                SET_STRING_ELT(names, i, PRINTNAME(tag));
            }
            i += 1;
            args = CDR(args);
        }
        crate::sexp::attrib_core::setAttrib(list, crate::sexp::attrib_core::R_NamesSymbol(), names);
        list
    }
}

unsafe fn validate_named_handlers(list: SEXP) {
    unsafe {
        let n = XLENGTH(list);
        let names =
            crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
        if n > 0 && (names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP)
        {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "condition handlers must be specified with a condition class".to_string(),
            });
        }
        for i in 0..n {
            if names.is_null()
                || names == R_NilValue()
                || STRING_ELT(names, i).is_null()
                || crate::sexp::accessors::CHAR(STRING_ELT(names, i)).is_null()
                || std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(STRING_ELT(names, i)))
                    .to_bytes()
                    .is_empty()
            {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "condition handlers must be specified with a condition class"
                        .to_string(),
                });
            }
            if !crate::mainutils::essentials::is_function_value(VECTOR_ELT(list, i)) {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "condition handlers must be functions".to_string(),
                });
            }
        }
    }
}

unsafe fn prepend_handlers(new: SEXP, old: SEXP) -> SEXP {
    unsafe {
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
        for i in 0..nn {
            SET_VECTOR_ELT(out, i, VECTOR_ELT(new, i));
            if !new_names.is_null() && new_names != R_NilValue() {
                SET_STRING_ELT(names, i, STRING_ELT(new_names, i));
            }
        }
        for i in 0..no {
            SET_VECTOR_ELT(out, nn + i, VECTOR_ELT(old, i));
            if !old_names.is_null() && old_names != R_NilValue() {
                SET_STRING_ELT(names, nn + i, STRING_ELT(old_names, i));
            }
        }
        crate::sexp::attrib_core::setAttrib(out, crate::sexp::attrib_core::R_NamesSymbol(), names);
        out
    }
}

unsafe fn drop_duplicate_class_handlers(list: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(list);
        if n <= 1 {
            return list;
        }
        let names =
            crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
        let mut keep = vec![true; n as usize];
        for i in 0..n {
            if !keep[i as usize] {
                continue;
            }
            let mut dropped = false;
            for j in (i + 1)..n {
                if !keep[j as usize] {
                    continue;
                }
                if !chars_equal(STRING_ELT(names, i), STRING_ELT(names, j)) {
                    continue;
                }
                if handlers_structurally_equal(VECTOR_ELT(list, i), VECTOR_ELT(list, j)) {
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
        for i in 0..n {
            if keep[i as usize] {
                SET_VECTOR_ELT(out, k, VECTOR_ELT(list, i));
                SET_STRING_ELT(out_names, k, STRING_ELT(names, i));
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

unsafe fn chars_equal(a: SEXP, b: SEXP) -> bool {
    unsafe {
        if a.is_null() || b.is_null() {
            return a == b;
        }
        let ca = crate::sexp::accessors::CHAR(a);
        let cb = crate::sexp::accessors::CHAR(b);
        if ca.is_null() || cb.is_null() {
            return ca == cb;
        }
        std::ffi::CStr::from_ptr(ca) == std::ffi::CStr::from_ptr(cb)
    }
}

unsafe fn refuse_if_local_handlers() {
    unsafe {
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

unsafe fn install_global_handler_stack(rho: SEXP) {
    unsafe {
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
}

unsafe fn pairlist_len(mut p: SEXP) -> i64 {
    unsafe {
        let mut n = 0i64;
        while !p.is_null() && p != R_NilValue() {
            n += 1;
            p = CDR(p);
        }
        n
    }
}

/// GNU `simpleCondition(message, call = NULL)`.
pub unsafe fn do_simpleCondition(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { super::condition_objects::native_simple_condition(args) }
}

/// GNU `signalCondition(cond)` — invoke matching calling handlers.
pub unsafe fn do_signalCondition_r(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut cond = if args.is_null() || args == R_NilValue() {
            R_NilValue()
        } else {
            CAR(args)
        };
        if cond.is_null()
            || cond == R_NilValue()
            || crate::mainutils::objects::inherits2(cond, c"condition".as_ptr()) == 0
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
}

// ---------------------------------------------------------------------------
// Exiting handlers (tryCatch) for warning conditions
// ---------------------------------------------------------------------------
