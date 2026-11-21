use super::*;
use crate::sexp::object::SexpMut;

/// GNU `exists`/`ls`: `where`/`pos` may be an environment, 1-based search
/// index, or search-path name (`"package:methods"`). `-1` means the caller env.
fn coerce_search_envir(arg: SEXP, default: SEXP) -> SEXP {
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
        let owner = active_owner();
        let arg_owned = owner_sexp(owner, arg);
        let pos = if TYPEOF(arg) == SEXPTYPE::INTSXP {
            integer_like_elt(&arg_owned, 0)
        } else {
            arg_owned
                .try_real_elt(0)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                as c_int
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

/// R's `exists(x, envir)` — check name exists.
pub fn do_exists(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        let name = elt_to_string(name_arg, 0);
        let sym = install_cstr(&CString::new(name.as_str()).unwrap_or_default());
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
                find_var(sym, env)
            } else {
                find_var_in_frame(env, sym)
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
pub fn do_find(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

    let sym = install_cstr(&CString::new(name.as_str()).unwrap_or_default());
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
    let owner = active_owner();
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for (i, value) in matches.iter().enumerate() {
        let charsxp = owner
            .sexp(mk_char_cstr(
                &CString::new(value.as_str()).unwrap_or_default(),
            ))
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        result_mut
            .try_set_string_elt(i as R_xlen_t, charsxp)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    result
}

fn find_numeric_result(matches: &[String]) -> SEXP {
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, matches.len() as R_xlen_t);
    let names = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
    let owner = active_owner();
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let mut names_mut = owner
        .sexp(names)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for (i, value) in matches.iter().enumerate() {
        result_mut
            .try_set_integer_elt(i as R_xlen_t, (i + 1) as c_int)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let charsxp = owner
            .sexp(mk_char_cstr(
                &CString::new(value.as_str()).unwrap_or_default(),
            ))
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        names_mut
            .try_set_string_elt(i as R_xlen_t, charsxp)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_NamesSymbol(), names);
    result
}

fn find_matches_mode(env: SEXP, symbol: SEXP, name: &str, want_function: bool) -> bool {
    if env.is_null() || env == R_NilValue() {
        return false;
    }
    let value = find_var_in_frame(env, symbol);
    let is_base_builtin =
        env == crate::sexp::globals::R_BaseEnv() && crate::eval::builtin::has_builtin_handler(name);
    if value == R_UnboundValue() {
        return !want_function && is_base_builtin;
    }
    if want_function {
        is_function_value(value) || is_base_builtin
    } else {
        true
    }
}

/// R's `get(x, envir)` — get value.
pub fn do_get(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let name_arg = arg_by_name_or_position(args, &["x"], 0);
    if name_arg.is_null()
        || name_arg == R_NilValue()
        || name_arg == crate::sexp::globals::R_MissingArg()
    {
        crate::mainutils::errors::R_MissingArgError_c(Some(c"x"), _call, Some(c"getvarError"));
    }
    let name = elt_to_string(name_arg, 0);
    let env = lookup_get_environment(args, rho, _call, _op, true);
    let _env_root = crate::sexp::protect::protect(env);
    let mode =
        lookup_mode_argument(args, 3).unwrap_or_else(|| base_error("invalid 'mode' argument"));
    let sym = install_cstr(&CString::new(name.as_str()).unwrap_or_default());
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
            Some(&c_name),
            _call,
            Some(c"getMissingError"),
        );
    }
    ensure_get_value_named(value)
}

/// R's `get0(x, envir, mode = "any", inherits = TRUE, ifnotfound = NULL)`.
/// Returns the original fallback value unchanged when no matching binding is
/// found; function fallbacks are data here and must not be invoked.
pub fn do_get0(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let name_arg = arg_by_name_or_position(args, &["x"], 0);
    if name_arg.is_null()
        || name_arg == R_NilValue()
        || name_arg == crate::sexp::globals::R_MissingArg()
    {
        crate::mainutils::errors::R_MissingArgError_c(Some(c"x"), _call, Some(c"getvarError"));
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
    let sym = install_cstr(&CString::new(name.as_str()).unwrap_or_default());
    match mget::lookup_one(sym, env, &mode, inherits)
        .unwrap_or_else(|error| base_error(error.to_string()))
    {
        Some(value) => {
            let value = value.as_raw();
            if value == crate::sexp::globals::R_MissingArg() {
                let c_name = CString::new(name).unwrap_or_default();
                crate::mainutils::errors::R_MissingArgError_c(
                    Some(&c_name),
                    _call,
                    Some(c"getMissingError"),
                );
            }
            ensure_get_value_named(value)
        }
        None => fallback,
    }
}

/// GNU GET_VALUE forces a selected promise and then raises NAMED from zero to
/// one so later copy-on-modify operations do not mutate the binding in place.
fn ensure_get_value_named(value: SEXP) -> SEXP {
    if crate::sexp::accessors::NAMED(value) == 0 {
        crate::sexp::accessors::SET_NAMED(value, 1);
    }
    value
}

fn lookup_mode_argument(args: SEXP, position: usize) -> Option<String> {
    {
        let mode = arg_by_name_or_position(args, &["mode"], position);
        if mode.is_null() || mode == R_NilValue() {
            return Some("any".to_string());
        }
        (TYPEOF(mode) == SEXPTYPE::STRSXP).then(|| elt_to_string(mode, 0))
    }
}

fn lookup_get_environment(
    args: SEXP,
    default: SEXP,
    call: SEXP,
    op: SEXP,
    allow_pos: bool,
) -> SEXP {
    {
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

fn strict_get_environment(environment: SEXP) -> SEXP {
    if environment.is_null() || environment == R_NilValue() {
        base_error("use of NULL environment is defunct");
    }
    let environment = if crate::mainutils::coerce::IS_S4_OBJECT(environment) != FALSE {
        crate::mainutils::subassign::R_getS4DataSlot(environment, SEXPTYPE::ANYSXP.as_c_int())
    } else {
        environment
    };
    if TYPEOF(environment) != SEXPTYPE::ENVSXP {
        base_error("invalid 'envir' argument");
    }
    environment
}

fn as_get_position_environment(call: SEXP, op: SEXP, position: SEXP, rho: SEXP) -> SEXP {
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

fn named_argument(args: SEXP, name: &str) -> Option<SEXP> {
    {
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

fn positional_argument(args: SEXP, position: usize) -> Option<SEXP> {
    {
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

fn lookup_inherits_argument(args: SEXP, position: usize, default: bool) -> Option<bool> {
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

/// R's `mget(x, envir, mode = "any", ifnotfound, inherits = FALSE)` —
/// look up each name of a character vector, returning a named list.
/// Lookups force selected promises and filter by the requested GNU mode.
pub fn do_mget(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    match { mget::invoke(args, rho) } {
        Ok(value) => value.as_raw(),
        Err(error) => base_error(error.to_string()),
    }
}

/// R's `assign(x, value, envir)` — assign value.
pub fn do_assign(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let name_arg = arg_by_name_or_position(args, &["x"], 0);
    let name = elt_to_string(name_arg, 0);
    let val = arg_by_name_or_position(args, &["value"], 1);
    if val.is_null() {
        return R_NilValue();
    }
    let env = environment_arg_or_default(args, &["envir", "pos"], 2, rho);
    define_var(
        install_cstr(&CString::new(name).unwrap_or_default()),
        val,
        env,
    );
    crate::sexp::globals::set_R_Visible(FALSE);
    val
}

pub(crate) fn symbol_name(sym: SEXP) -> Option<String> {
    if sym.is_null() || sym == R_NilValue() || TYPEOF(sym) != SEXPTYPE::SYMSXP {
        return None;
    }
    let printname = PRINTNAME(sym);
    if printname.is_null() || printname == R_NilValue() {
        return None;
    }
    let owner = active_owner();
    charsxp_utf8_lossy(&owner_sexp(owner, printname))
}

pub(crate) fn logical_arg(arg: SEXP, default: bool) -> bool {
    if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) < 1 {
        return default;
    }
    let owner = active_owner();
    let arg_owned = owner_sexp(owner, arg);
    if TYPEOF(arg) == SEXPTYPE::LGLSXP {
        return arg_owned.try_logical_elt(0).unwrap_or(FALSE) != FALSE;
    }
    if TYPEOF(arg) == SEXPTYPE::INTSXP {
        return arg_owned.try_integer_elt(0).unwrap_or(0) != 0;
    }
    default
}

/// R's `ls(envir)` — list objects.
pub fn do_ls(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
    let owner = active_owner();
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for (i, name) in names.iter().enumerate() {
        let cstr = CString::new(name.as_str()).unwrap_or_default();
        let charsxp = owner
            .sexp(mk_char_cstr(&cstr))
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        result_mut
            .try_set_string_elt(i as R_xlen_t, charsxp)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    result
}

/// `ls.str(envir)` — list names and `str` each value without forcing symbols.
pub fn do_ls_str(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let names = do_ls(call, op, args, rho);
    let _n = protect(names);
    let mut env = rho;
    let first = CAR(args);
    if !first.is_null() && TYPEOF(first) == SEXPTYPE::ENVSXP {
        env = first;
    }
    let owner = active_owner();
    let names_owned = owner_sexp(owner, names);
    for i in 0..XLENGTH(names) {
        let ch = names_owned
            .try_string_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if ch.as_raw().is_null() {
            continue;
        }
        let Some(text) = charsxp_utf8_lossy(&ch) else {
            continue;
        };
        let sym = install_cstr(&CString::new(text).unwrap_or_default());
        let val = find_var_in_frame(env, sym);
        if val.is_null() || val == crate::sexp::globals::R_UnboundValue() {
            continue;
        }
        let _ = super::print::do_str(call, op, Rf_cons(val, R_NilValue()), rho);
    }
    names
}

/// R's `rm(list, envir)` — remove objects.
pub fn do_rm(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
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
                            let owner = active_owner();
                            if let Some(text) = charsxp_utf8_lossy(&owner_sexp(owner, pn)) {
                                names.push(text);
                            }
                        }
                    }
                    t if t == SEXPTYPE::STRSXP && XLENGTH(expr) > 0 => {
                        let owner = active_owner();
                        let pn =
                            owner_sexp(owner, expr)
                                .try_string_elt(0)
                                .unwrap_or_else(|error| {
                                    crate::sexp::context::r_error(error.to_string())
                                });
                        if !pn.as_raw().is_null() && !pn.is_na_string() {
                            if let Some(text) = charsxp_utf8_lossy(&pn) {
                                names.push(text);
                            }
                        }
                    }
                    t if t == SEXPTYPE::LANGSXP => {
                        // Constant-folded string like '"x"' may arrive as a
                        // call node; evaluate it and accept a string.
                        let val = crate::eval::eval::Rf_eval(expr, rho);
                        if !val.is_null() && TYPEOF(val) == SEXPTYPE::STRSXP && XLENGTH(val) == 1 {
                            let owner = active_owner();
                            let pn =
                                owner_sexp(owner, val)
                                    .try_string_elt(0)
                                    .unwrap_or_else(|error| {
                                        crate::sexp::context::r_error(error.to_string())
                                    });
                            if !pn.as_raw().is_null() && !pn.is_na_string() {
                                if let Some(text) = charsxp_utf8_lossy(&pn) {
                                    names.push(text);
                                }
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
                let name = {
                    let owner = active_owner();
                    charsxp_utf8_lossy(&owner_sexp(owner, PRINTNAME(tag))).unwrap_or_default()
                };
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
                && {
                    let owner = active_owner();
                    owner_sexp(owner, pos_val).try_integer_elt(0).unwrap_or(0) == -1
                }
            {
                crate::sexp::globals::R_GlobalEnv()
            } else {
                crate::sexp::globals::R_GlobalEnv()
            }
        } else {
            rho
        };
        for name in names {
            let sym = install_cstr(&CString::new(name).unwrap_or_default());
            crate::sexp::envir::remove_binding_raw(env, sym);
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}
