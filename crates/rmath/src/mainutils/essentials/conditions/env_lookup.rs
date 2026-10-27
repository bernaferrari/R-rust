use super::*;

/// GNU `exists`/`ls`: `where`/`pos` may be an environment, 1-based search
/// index, or search-path name (`"package:methods"`). `-1` means the caller env.
unsafe fn coerce_search_envir(arg: SEXP, default: SEXP) -> SEXP {
    unsafe {
        if arg.is_null() || arg == R_NilValue() {
            return default;
        }
        if TYPEOF(arg) == SEXPTYPE::ENVSXP {
            return arg;
        }
        if TYPEOF(arg) == SEXPTYPE::INTSXP || TYPEOF(arg) == SEXPTYPE::REALSXP {
            if XLENGTH(arg) < 1 {
                return default;
            }
            let pos = if TYPEOF(arg) == SEXPTYPE::INTSXP {
                INTEGER_ELT(arg, 0)
            } else {
                REAL_ELT(arg, 0) as c_int
            };
            if pos == -1 {
                return default;
            }
            return super::mathstats::search_env_from_position(pos);
        }
        if TYPEOF(arg) == SEXPTYPE::STRSXP && XLENGTH(arg) > 0 {
            return super::mathstats::search_env_from_name(&elt_to_string(arg, 0));
        }
        default
    }
}

/// R's `exists(x, envir)` — check name exists.
pub unsafe fn do_exists(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        let name = elt_to_string(name_arg, 0);
        let sym = Rf_install(CString::new(name.as_str()).unwrap_or_default().as_ptr());
        let env = coerce_search_envir(
            arg_by_name_or_position(args, &["envir", "where", "frame"], 1),
            rho,
        );

        let inherits = logical_arg_by_name_or_position(args, "inherits", 3).unwrap_or(true);
        // The base closure is
        // exists(x, where, envir, frame, mode, inherits) and calls
        // .Internal(exists(x, envir, mode, inherits)). mode is argument 2.
        let mode_arg = arg_by_name_or_position(args, &["mode"], 2);
        let mode = if mode_arg.is_null() || mode_arg == R_NilValue() || XLENGTH(mode_arg) == 0 {
            "any".to_string()
        } else if TYPEOF(mode_arg) == SEXPTYPE::STRSXP {
            elt_to_string(mode_arg, 0)
        } else {
            "any".to_string()
        };

        let found = if crate::eval::builtin::is_hidden_builtin_name(&name) {
            false
        } else if mode == "function" {
            let mut value = if inherits {
                crate::sexp::envir::R_findVar(sym, env)
            } else {
                crate::sexp::envir::R_findVarInFrame(env, sym)
            };
            if TYPEOF(value) == SEXPTYPE::PROMSXP {
                value = crate::eval::eval::Rf_eval(value, env);
            }
            is_function_value(value)
        } else {
            crate::sexp::envir::binding_exists_raw(env, sym, inherits)
        };

        Rf_ScalarLogical(if found { TRUE } else { FALSE })
    }
}

/// R's `find(what, mode = "any")` — locate a name on the search path.
pub unsafe fn do_find(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let what_arg = arg_by_name_or_position(args, &["what"], 0);
        if what_arg.is_null() || what_arg == R_NilValue() || XLENGTH(what_arg) == 0 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let name = elt_to_string(what_arg, 0);
        if name.is_empty() || crate::eval::builtin::is_hidden_builtin_name(&name) {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let mode_arg = arg_by_name_or_position(args, &["mode"], 1);
        let mode = if mode_arg.is_null() || mode_arg == R_NilValue() || XLENGTH(mode_arg) == 0 {
            "any".to_string()
        } else {
            elt_to_string(mode_arg, 0)
        };
        let want_function = mode == "function";
        let numeric = logical_arg_by_name_or_position(args, "numeric", 2).unwrap_or(false);

        let sym = Rf_install(CString::new(name.as_str()).unwrap_or_default().as_ptr());
        let mut matches = Vec::new();
        for (label, env) in search_path_entries() {
            if find_matches_mode(env, sym, &name, want_function) {
                matches.push(label);
            }
        }

        if numeric {
            return find_numeric_result(&matches);
        }

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
        for (i, value) in matches.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

unsafe fn find_numeric_result(matches: &[String]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, matches.len() as R_xlen_t);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
        for (i, value) in matches.iter().enumerate() {
            *INTEGER(result).add(i) = (i + 1) as c_int;
            SET_STRING_ELT(
                names,
                i as R_xlen_t,
                Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        result
    }
}

unsafe fn find_matches_mode(env: SEXP, symbol: SEXP, name: &str, want_function: bool) -> bool {
    unsafe {
        if env.is_null() || env == R_NilValue() {
            return false;
        }
        let value = crate::sexp::envir::R_findVarInFrame(env, symbol);
        let is_base_builtin = env == crate::sexp::globals::R_BaseEnv()
            && crate::eval::builtin::has_builtin_handler(name);
        if value == R_UnboundValue() {
            return !want_function && is_base_builtin;
        }
        if want_function {
            is_function_value(value) || is_base_builtin
        } else {
            true
        }
    }
}

/// R's `get(x, envir)` — get value.
pub unsafe fn do_get(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        if name_arg.is_null()
            || name_arg == R_NilValue()
            || name_arg == crate::sexp::globals::R_MissingArg()
        {
            crate::mainutils::errors::R_MissingArgError_c(
                c"x".as_ptr(),
                _call,
                c"getvarError".as_ptr(),
            );
        }
        let name = elt_to_string(name_arg, 0);
        let env = lookup_get_environment(args, rho, _call, _op, true);
        let _env_root = crate::sexp::protect::protect(env);
        let mode =
            lookup_mode_argument(args, 3).unwrap_or_else(|| base_error("invalid 'mode' argument"));
        let sym = Rf_install(CString::new(name.as_str()).unwrap_or_default().as_ptr());
        let inherits = lookup_inherits_argument(args, 4, true)
            .unwrap_or_else(|| base_error("invalid 'inherits' argument"));
        let selected = mget::lookup_one(sym, env, &mode, inherits)
            .unwrap_or_else(|error| base_error(error.to_string()));
        let Some(selected) = selected else {
            crate::mainutils::errors::R_ObjectNotFoundError(
                sym,
                _call,
                (mode != "any").then_some(mode.as_str()),
            );
        };
        let value = selected.as_raw();
        if value == crate::sexp::globals::R_MissingArg() {
            let c_name = CString::new(name.as_str()).unwrap_or_default();
            crate::mainutils::errors::R_MissingArgError_c(
                c_name.as_ptr(),
                _call,
                c"getMissingError".as_ptr(),
            );
        }
        ensure_get_value_named(value)
    }
}

/// R's `get0(x, envir, mode = "any", inherits = TRUE, ifnotfound = NULL)`.
/// Returns the original fallback value unchanged when no matching binding is
/// found; function fallbacks are data here and must not be invoked.
pub unsafe fn do_get0(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        if name_arg.is_null()
            || name_arg == R_NilValue()
            || name_arg == crate::sexp::globals::R_MissingArg()
        {
            crate::mainutils::errors::R_MissingArgError_c(
                c"x".as_ptr(),
                _call,
                c"getvarError".as_ptr(),
            );
        }
        let name = elt_to_string(name_arg, 0);
        let env = lookup_get_environment(args, rho, _call, _op, false);
        let _env_root = crate::sexp::protect::protect(env);
        let mode =
            lookup_mode_argument(args, 2).unwrap_or_else(|| base_error("invalid 'mode' argument"));
        let inherits = lookup_inherits_argument(args, 3, true)
            .unwrap_or_else(|| base_error("invalid 'inherits' argument"));
        let ifnotfound = arg_by_name_or_position(args, &["ifnotfound"], 4);
        let fallback = if ifnotfound.is_null() {
            R_NilValue()
        } else {
            ifnotfound
        };
        let sym = Rf_install(CString::new(name.as_str()).unwrap_or_default().as_ptr());
        match mget::lookup_one(sym, env, &mode, inherits)
            .unwrap_or_else(|error| base_error(error.to_string()))
        {
            Some(value) => {
                let value = value.as_raw();
                if value == crate::sexp::globals::R_MissingArg() {
                    let c_name = CString::new(name).unwrap_or_default();
                    crate::mainutils::errors::R_MissingArgError_c(
                        c_name.as_ptr(),
                        _call,
                        c"getMissingError".as_ptr(),
                    );
                }
                ensure_get_value_named(value)
            }
            None => fallback,
        }
    }
}

/// GNU GET_VALUE forces a selected promise and then raises NAMED from zero to
/// one so later copy-on-modify operations do not mutate the binding in place.
unsafe fn ensure_get_value_named(value: SEXP) -> SEXP {
    unsafe {
        if crate::sexp::accessors::NAMED(value) == 0 {
            crate::sexp::accessors::SET_NAMED(value, 1);
        }
        value
    }
}

unsafe fn lookup_mode_argument(args: SEXP, position: usize) -> Option<String> {
    unsafe {
        let mode = arg_by_name_or_position(args, &["mode"], position);
        if mode.is_null() || mode == R_NilValue() {
            return Some("any".to_string());
        }
        (TYPEOF(mode) == SEXPTYPE::STRSXP).then(|| elt_to_string(mode, 0))
    }
}

unsafe fn lookup_get_environment(
    args: SEXP,
    default: SEXP,
    call: SEXP,
    op: SEXP,
    allow_pos: bool,
) -> SEXP {
    unsafe {
        if let Some(environment) = named_argument(args, "envir") {
            return strict_get_environment(environment);
        }

        if allow_pos {
            if let Some(position) =
                named_argument(args, "pos").or_else(|| positional_argument(args, 1))
            {
                if TYPEOF(position) == SEXPTYPE::ENVSXP {
                    return position;
                }
                return as_get_position_environment(call, op, position, default);
            }
            return default;
        }

        positional_argument(args, 1)
            .map(|environment| strict_get_environment(environment))
            .unwrap_or(default)
    }
}

unsafe fn strict_get_environment(environment: SEXP) -> SEXP {
    unsafe {
        if environment.is_null() || environment == R_NilValue() {
            base_error("use of NULL environment is defunct");
        }
        let environment = if crate::mainutils::coerce::IS_S4_OBJECT(environment) != FALSE {
            crate::mainutils::subassign::R_getS4DataSlot(
                environment,
                SEXPTYPE::ANYSXP.as_c_int(),
            )
        } else {
            environment
        };
        if TYPEOF(environment) != SEXPTYPE::ENVSXP {
            base_error("invalid 'envir' argument");
        }
        environment
    }
}

unsafe fn as_get_position_environment(call: SEXP, op: SEXP, position: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if position.is_null() || position == R_NilValue() {
            base_error("use of NULL environment is defunct");
        }
        let conversion_args = Rf_cons(position, R_NilValue());
        let _args_root = crate::sexp::protect::protect(conversion_args);
        let environment =
            crate::mainutils::essentials::do_as_environment(call, op, conversion_args, rho);
        if environment.is_null() || TYPEOF(environment) != SEXPTYPE::ENVSXP {
            base_error("invalid 'envir' argument");
        }
        environment
    }
}

unsafe fn named_argument(args: SEXP, name: &str) -> Option<SEXP> {
    unsafe {
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            if tag_name(cell).as_deref() == Some(name) {
                return Some(CAR(cell));
            }
            cell = CDR(cell);
        }
        None
    }
}

unsafe fn positional_argument(args: SEXP, position: usize) -> Option<SEXP> {
    unsafe {
        let mut cell = args;
        let mut index = 0;
        while !cell.is_null() && cell != R_NilValue() {
            if tag_name(cell).is_none() {
                if index == position {
                    return Some(CAR(cell));
                }
                index += 1;
            }
            cell = CDR(cell);
        }
        None
    }
}

unsafe fn lookup_inherits_argument(args: SEXP, position: usize, default: bool) -> Option<bool> {
    unsafe {
        let inherits = arg_by_name_or_position(args, &["inherits"], position);
        if inherits.is_null() || inherits == R_NilValue() {
            return Some(default);
        }
        let value = crate::mainutils::coerce::asLogical(inherits);
        if value == crate::sexp::ffi::NA_LOGICAL {
            None
        } else {
            Some(value != FALSE)
        }
    }
}

/// R's `mget(x, envir, mode = "any", ifnotfound, inherits = FALSE)` —
/// look up each name of a character vector, returning a named list.
/// Lookups force selected promises and filter by the requested GNU mode.
pub unsafe fn do_mget(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    match unsafe { mget::invoke(args, rho) } {
        Ok(value) => value.as_raw(),
        Err(error) => base_error(error.to_string()),
    }
}

/// R's `assign(x, value, envir)` — assign value.
pub unsafe fn do_assign(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        let name = elt_to_string(name_arg, 0);
        let val = arg_by_name_or_position(args, &["value"], 1);
        if val.is_null() {
            return R_NilValue();
        }
        let env = environment_arg_or_default(args, &["envir", "pos"], 2, rho);
        crate::sexp::envir::defineVar(
            Rf_install(CString::new(name).unwrap_or_default().as_ptr()),
            val,
            env,
        );
        crate::sexp::globals::set_R_Visible(FALSE);
        val
    }
}

pub(crate) unsafe fn symbol_name(sym: SEXP) -> Option<String> {
    unsafe {
        if sym.is_null() || sym == R_NilValue() || TYPEOF(sym) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(sym);
        if printname.is_null() || printname == R_NilValue() {
            return None;
        }
        let ptr = CHAR(printname);
        if ptr.is_null() {
            return None;
        }
        Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

pub(crate) unsafe fn logical_arg(arg: SEXP, default: bool) -> bool {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) < 1 {
            return default;
        }
        if TYPEOF(arg) == SEXPTYPE::LGLSXP {
            return *LOGICAL(arg) != FALSE;
        }
        if TYPEOF(arg) == SEXPTYPE::INTSXP {
            return *INTEGER(arg) != 0;
        }
        default
    }
}

/// R's `ls(envir)` — list objects.
pub unsafe fn do_ls(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut env = rho;
        let mut all_names = false;
        let mut sorted = true;

        let mut positional = 0;
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let arg = CAR(cell);
            let name = symbol_name(TAG(cell));
            match name.as_deref() {
                Some("name") | Some("pos") | Some("envir") => {
                    env = coerce_search_envir(arg, env);
                }
                Some("all.names") => all_names = logical_arg(arg, all_names),
                Some("sorted") => sorted = logical_arg(arg, sorted),
                _ => {
                    match positional {
                        0 => env = coerce_search_envir(arg, env),
                        1 => all_names = logical_arg(arg, all_names),
                        2 => sorted = logical_arg(arg, sorted),
                        _ => {}
                    }
                    positional += 1;
                }
            }
            cell = CDR(cell);
        }

        let mut names = if TYPEOF(env) == SEXPTYPE::ENVSXP {
            super::shared::frame_binding_names_for_listing(env, all_names)
        } else {
            Vec::new()
        };

        if sorted {
            names.sort_by(|a, b| super::sets::collate_str(a, b));
        }

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        for (i, name) in names.iter().enumerate() {
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }
        result
    }
}

/// `ls.str(envir)` — list names and `str` each value without forcing symbols.
pub unsafe fn do_ls_str(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let names = do_ls(call, op, args, rho);
        let _n = protect(names);
        let mut env = rho;
        let first = CAR(args);
        if !first.is_null() && TYPEOF(first) == SEXPTYPE::ENVSXP {
            env = first;
        }
        for i in 0..XLENGTH(names) {
            let ch = STRING_ELT(names, i);
            if ch.is_null() {
                continue;
            }
            let sym = Rf_install(CHAR(ch));
            let val = crate::sexp::envir::R_findVarInFrame(env, sym);
            if val.is_null() || val == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            let _ = super::print::do_str(call, op, Rf_cons(val, R_NilValue()), rho);
        }
        names
    }
}

/// R's `rm(list, envir)` — remove objects.
pub unsafe fn do_rm(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // Upstream rm is a closure: `...` dots arrive UNEVALUATED (this is
        // an unevaluated builtin) and must each be a symbol or character
        // string naming a binding; the named formals are evaluated here.
        let mut names: Vec<String> = Vec::new();
        let mut list_arg: SEXP = R_NilValue();
        let mut pos_arg: SEXP = crate::sexp::globals::R_MissingArg();
        let mut envir_arg: SEXP = crate::sexp::globals::R_MissingArg();
        let mut current = args;
        while current != R_NilValue() && !current.is_null() {
            let tag = TAG(current);
            let expr = CAR(current);
            let tagged = !tag.is_null() && tag != R_NilValue();
            if !tagged {
                match TYPEOF(expr) {
                    t if t == SEXPTYPE::SYMSXP => {
                        // PRINTNAME -> string
                        let pn = PRINTNAME(expr);
                        if !pn.is_null() && pn != R_NilValue() {
                            names.push(
                                std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(pn))
                                    .to_string_lossy()
                                    .into_owned(),
                            );
                        }
                    }
                    t if t == SEXPTYPE::STRSXP && XLENGTH(expr) > 0 => {
                        let pn = crate::sexp::accessors::STRING_ELT(expr, 0);
                        if !pn.is_null() && pn != crate::sexp::globals::R_NaString() {
                            names.push(
                                std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(pn))
                                    .to_string_lossy()
                                    .into_owned(),
                            );
                        }
                    }
                    t if t == SEXPTYPE::LANGSXP => {
                        // Constant-folded string like '"x"' may arrive as a
                        // call node; evaluate it and accept a string.
                        let val = crate::eval::eval::Rf_eval(expr, rho);
                        if !val.is_null() && TYPEOF(val) == SEXPTYPE::STRSXP && XLENGTH(val) == 1 {
                            let pn = crate::sexp::accessors::STRING_ELT(val, 0);
                            if !pn.is_null() && pn != crate::sexp::globals::R_NaString() {
                                names.push(
                                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(pn))
                                        .to_string_lossy()
                                        .into_owned(),
                                );
                            }
                        } else {
                            std::panic::panic_any(crate::sexp::context::RError {
                                message: "... must contain names or character strings".into(),
                            });
                        }
                    }
                    _ => {
                        std::panic::panic_any(crate::sexp::context::RError {
                            message: "... must contain names or character strings".into(),
                        });
                    }
                }
            } else {
                let name = std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned();
                match name.as_str() {
                    "list" => list_arg = crate::eval::eval::Rf_eval(expr, rho),
                    "pos" => pos_arg = crate::eval::eval::Rf_eval(expr, rho),
                    "envir" => envir_arg = crate::eval::eval::Rf_eval(expr, rho),
                    _ => {}
                }
            }
            current = CDR(current);
        }
        // Merge an explicit list= argument into the dot names.
        if !list_arg.is_null() && list_arg != R_NilValue() {
            if TYPEOF(list_arg) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(list_arg) {
                    names.push(elt_to_string(list_arg, i));
                }
            }
        }
        if names.is_empty() {
            crate::sexp::globals::set_R_Visible(FALSE);
            return R_NilValue();
        }
        // Resolve the target environment: envir= wins, then pos=.
        let env = if !envir_arg.is_null() && envir_arg != crate::sexp::globals::R_MissingArg() {
            envir_arg
        } else if !pos_arg.is_null() && pos_arg != crate::sexp::globals::R_MissingArg() {
            // pos=-1 (default) means the global environment.
            let pos_val = crate::eval::eval::Rf_eval(pos_arg, rho);
            if !pos_val.is_null()
                && TYPEOF(pos_val) == SEXPTYPE::INTSXP
                && XLENGTH(pos_val) > 0
                && INTEGER_ELT(pos_val, 0) == -1
            {
                crate::sexp::globals::R_GlobalEnv()
            } else {
                crate::sexp::globals::R_GlobalEnv()
            }
        } else {
            rho
        };
        for name in names {
            let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            crate::sexp::envir::remove_binding_raw(env, sym);
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}
