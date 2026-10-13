//! simplify2array, match.arg, type.convert, environments, sort.list, outer, match.fun — extracted verbatim from essentials/mathstats.rs.

use super::*;

/// R's `simplify2array(x)` — simplify list to array.
pub unsafe fn do_simplify2array(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "simplify2array",
            include_str!("../../base_wrappers/simplify2array.R"),
            args,
            rho,
            true,
        )
    }
}

/// GNU `match.arg(arg, choices, several.ok=FALSE)`.
///
/// One-arg form (`match.arg(mm)`) is unevaluated so `substitute(arg)` can
/// name the caller's formal; choices come from `formals(sys.function(0))`.
pub unsafe fn do_match_arg(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let factory = crate::sexp::object::SessionNodeFactory::new(
            crate::sexp::owner::OwnerToken::current()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
        );
        let _args_owner = factory
            .wrap(args)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let rho_owner = factory
            .wrap(rho)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let call_owner = factory
            .wrap(call)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let arg_expr = CAR(args);
        let choices_cell = CDR(args);
        let choices_expr = if choices_cell.is_null() || choices_cell == R_NilValue() {
            R_MissingArg()
        } else {
            CAR(choices_cell)
        };
        let choices_missing = choices_expr.is_null()
            || choices_expr == R_NilValue()
            || choices_expr == R_MissingArg();

        let arg = if arg_expr.is_null() || arg_expr == R_NilValue() || arg_expr == R_MissingArg() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(arg_expr, rho)
        };
        let _arg_owner = factory
            .wrap(arg)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));

        let choices_owner = if choices_missing {
            factory.wrap(match_arg_choices_from_formals(arg_expr, rho))
        } else if choices_expr == crate::sexp::symbol::R_DotsSymbol() {
            let expression = factory
                .wrap(choices_expr)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let nil = factory.nil();
            let cell = factory
                .allocate(|arena| Some(arena.cons(expression.as_raw(), nil.as_raw(), nil.as_raw())))
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let spliced = crate::eval::dispatch::evalList(cell, rho_owner, Some(call_owner), -1);
            spliced.try_car()
        } else {
            factory.wrap(crate::eval::eval::Rf_eval(choices_expr, rho))
        }
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let choices = choices_owner.as_raw();

        if arg.is_null() || arg == R_NilValue() {
            return match_arg_first(choices);
        }
        if TYPEOF(arg) != SEXPTYPE::STRSXP {
            crate::mainutils::errors::errorcall_str(
                call,
                "'arg' must be NULL or a character vector",
            );
        }
        if TYPEOF(choices) != SEXPTYPE::STRSXP || XLENGTH(choices) == 0 {
            crate::mainutils::errors::errorcall_str(call, "'arg' should be one of");
        }
        let several_cell = CDR(choices_cell);
        let several_expr = if several_cell.is_null() || several_cell == R_NilValue() {
            R_MissingArg()
        } else {
            CAR(several_cell)
        };
        // GNU: 'several.ok' must be logical, or a string starting with "all"
        // (which also requires every element of arg to match).
        let mut all_match = false;
        let several_ok = if several_expr.is_null()
            || several_expr == R_NilValue()
            || several_expr == R_MissingArg()
        {
            false
        } else {
            let flag = crate::eval::eval::Rf_eval(several_expr, rho);
            let flag_type = TYPEOF(flag);
            if flag_type == SEXPTYPE::LGLSXP {
                match crate::main::coerce::asLogical(flag) {
                    1 => true,
                    0 => false,
                    _ => crate::mainutils::errors::errorcall_str(
                        call,
                        "missing value where TRUE/FALSE needed",
                    ),
                }
            } else if flag_type == SEXPTYPE::STRSXP {
                let text = elt_to_string(flag, 0);
                if text.starts_with("all") {
                    all_match = true;
                    true
                } else {
                    crate::mainutils::errors::errorcall_str(
                        call,
                        "'several.ok' must be logical or a string starting with \"all\"",
                    )
                }
            } else {
                crate::mainutils::errors::errorcall_str(
                    call,
                    "'several.ok' must be logical or a string starting with \"all\"",
                )
            }
        };
        if !several_ok {
            // The arg can be the whole of choices as a default argument.
            if crate::mainutils::identical::R_compute_identical(arg, choices, 0) != 0 {
                return match_arg_first(choices);
            }
            if XLENGTH(arg) != 1 {
                crate::mainutils::errors::errorcall_str(call, "'arg' must be of length 1");
            }
        } else if XLENGTH(arg) == 0 {
            crate::mainutils::errors::errorcall_str(call, "'arg' must be of length >= 1");
        }

        // Handle each element of arg separately:
        // pmatch(arg, choices, nomatch = 0, duplicates.ok = TRUE).
        let mut idxs: Vec<i64> = Vec::with_capacity(XLENGTH(arg) as usize);
        for a in 0..XLENGTH(arg) {
            let needle = elt_to_string(arg, a);
            idxs.push(match_arg_pmatch_index(&needle, choices));
        }
        let all_zero = idxs.iter().all(|&i| i < 0);
        let any_zero = idxs.iter().any(|&i| i < 0);
        if all_zero || (all_match && any_zero) {
            let mut shown: Vec<String> = Vec::new();
            for i in 0..XLENGTH(choices) {
                let choice = elt_to_string(choices, i);
                if !choice.is_empty() && !shown.contains(&choice) {
                    shown.push(choice);
                }
            }
            let list = shown
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let msg = if shown.len() == 1 {
                format!("'arg' should be {list}")
            } else {
                format!("'arg' should be one of {list}")
            };
            crate::mainutils::errors::errorcall_str(call, &msg);
        }
        let hits: Vec<String> = idxs
            .iter()
            .filter(|i| **i >= 0)
            .map(|&i| elt_to_string(choices, i))
            .collect();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, hits.len() as i64);
        let _result = protect(result);
        for (i, hit) in hits.iter().enumerate() {
            let cstr = std::ffi::CString::new(hit.as_str()).unwrap_or_default();
            SET_STRING_ELT(result, i as i64, Rf_mkChar(cstr.as_ptr()));
        }
        result
    }
}

/// `pmatch(x, table, nomatch = 0, duplicates.ok = TRUE)` for a single
/// element: an exact match wins, otherwise a unique prefix; -1 = nomatch.
unsafe fn match_arg_pmatch_index(needle: &str, choices: SEXP) -> i64 {
    unsafe {
        for i in 0..XLENGTH(choices) {
            if elt_to_string(choices, i) == needle {
                return i;
            }
        }
        let mut prefix: i64 = -1;
        let mut count = 0;
        for i in 0..XLENGTH(choices) {
            if elt_to_string(choices, i).starts_with(needle) {
                count += 1;
                prefix = i;
            }
        }
        if count == 1 { prefix } else { -1 }
    }
}

unsafe fn match_arg_choices_from_formals(arg_expr: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut arg_expr = arg_expr;
        if TYPEOF(arg_expr) == SEXPTYPE::PROMSXP {
            arg_expr = crate::sexp::accessors::PRCODE(arg_expr);
        }
        if TYPEOF(arg_expr) == SEXPTYPE::BCODESXP {
            arg_expr = crate::eval::bc_eval::BCODE_EXPR(arg_expr);
        }
        let name = if TYPEOF(arg_expr) == SEXPTYPE::SYMSXP {
            let pname = PRINTNAME(arg_expr);
            if pname.is_null() {
                return R_NilValue();
            }
            CStr::from_ptr(CHAR(pname)).to_string_lossy().into_owned()
        } else {
            return R_NilValue();
        };
        let _ = rho;
        let mut ctx = crate::sexp::context::R_GlobalContext();
        while !ctx.is_null() {
            if (*ctx).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION != 0 {
                let fun = (*ctx).callfun.as_raw();
                let mut cell = FORMALS(fun);
                while !cell.is_null() && cell != R_NilValue() {
                    let tag = TAG(cell);
                    if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                        let pname = PRINTNAME(tag);
                        if !pname.is_null() {
                            let tag_name = CStr::from_ptr(CHAR(pname)).to_string_lossy();
                            if tag_name == name {
                                let def = CAR(cell);
                                if !(def.is_null() || def == R_NilValue() || def == R_MissingArg())
                                {
                                    return crate::eval::eval::Rf_eval(def, (*ctx).cloenv.as_raw());
                                }
                            }
                        }
                    }
                    cell = CDR(cell);
                }
            }
            ctx = (*ctx).nextcontext;
        }
        R_NilValue()
    }
}

unsafe fn match_arg_first(choices: SEXP) -> SEXP {
    unsafe { match_arg_elt(choices, 0) }
}

unsafe fn match_arg_elt(choices: SEXP, idx: i64) -> SEXP {
    unsafe {
        if choices.is_null()
            || choices == R_NilValue()
            || TYPEOF(choices) != SEXPTYPE::STRSXP
            || idx < 0
            || idx >= XLENGTH(choices)
        {
            return R_NilValue();
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _g = protect(out);
        SET_STRING_ELT(out, 0, STRING_ELT(choices, idx));
        out
    }
}

/// R's `char.expand(input, target)` — expand abbreviations.
pub unsafe fn do_char_expand(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let target = CAR(CDR(args));
        let nomatch = CAR(CDR(CDR(args)));
        if input.is_null() || target.is_null() {
            return input;
        }
        let input_str = elt_to_string(input, 0);
        let n = if target == R_NilValue() {
            0
        } else {
            XLENGTH(target)
        };
        let mut matches: Vec<String> = Vec::new();
        for i in 0..n {
            let t = elt_to_string(target, i);
            if t.starts_with(&input_str) {
                matches.push(t);
            }
        }
        if matches.len() == 1 {
            Rf_mkString(CString::new(&matches[0][..]).unwrap_or_default().as_ptr())
        } else if matches.len() > 1 {
            Rf_allocVector3(SEXPTYPE::STRSXP, 0)
        } else if !nomatch.is_null() && nomatch != R_NilValue() && nomatch != R_MissingArg() {
            let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            if out.is_null() {
                return R_NilValue();
            }
            SET_STRING_ELT(out, 0, crate::sexp::globals::R_NaString());
            out
        } else {
            // Upstream char.expand ends with `eval(nomatch)` where nomatch
            // defaults to stop("no match"); stock R attributes the error to
            // that eval call.
            let sym = |name: &str| {
                crate::sexp::symbol::Rf_install(
                    std::ffi::CString::new(name).unwrap_or_default().as_ptr(),
                )
            };
            let eval_call = crate::sexp::constructors::Rf_lang2(sym("eval"), sym("nomatch"));
            crate::mainutils::errors::errorcall_str(eval_call, "no match");
        }
    }
}

/// R's `type.convert(x, ...)` — convert to appropriate type.
pub unsafe fn do_type_convert(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return x;
        }
        let tagged = |name: &str| -> SEXP {
            let mut cell = args;
            while !cell.is_null() && cell != R_NilValue() {
                let tag = TAG(cell);
                if !tag.is_null() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                    let pname = PRINTNAME(tag);
                    if !pname.is_null() {
                        let label = std::ffi::CStr::from_ptr(CHAR(pname)).to_string_lossy();
                        if label == name {
                            return CAR(cell);
                        }
                    }
                }
                cell = CDR(cell);
            }
            R_NilValue()
        };
        let numerals = {
            let spec = tagged("numerals");
            if TYPEOF(spec) == SEXPTYPE::STRSXP && XLENGTH(spec) > 0 {
                elt_to_string(spec, 0)
            } else {
                "allow.loss".to_string()
            }
        };
        let as_is = {
            let spec = tagged("as.is");
            if spec.is_null() || spec == R_NilValue() {
                true
            } else {
                crate::mainutils::coerce::asLogical(spec) != 0
            }
        };
        let n = XLENGTH(x);
        let mut all_i32 = n > 0;
        let mut all_real = n > 0;
        let mut exact_real = true;
        let mut loss = false;
        let mut values = Vec::with_capacity(n as usize);
        for i in 0..n {
            let s = elt_to_string(x, i);
            if let Ok(v) = s.parse::<i64>() {
                if let Ok(v) = i32::try_from(v) {
                    values.push(v as f64);
                    continue;
                }
            }
            all_i32 = false;
            let plain_int = {
                let body = s.trim_start_matches(['+', '-']);
                !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit())
            };
            if let Some(v) = crate::mainutils::coerce::parse_double_str(&s) {
                if plain_int {
                    let digits = s.trim_start_matches(['+', '-']).trim_start_matches('0');
                    let digits = if digits.is_empty() { "0" } else { digits };
                    let shown = format!("{v:.0}");
                    let shown = shown.trim_start_matches('-');
                    if shown != digits {
                        exact_real = false;
                        loss = true;
                    }
                }
                values.push(v);
            } else {
                all_real = false;
                values.push(f64::NAN);
            }
        }
        let keep_real = all_real && (numerals != "no.loss" || exact_real);
        if all_i32 {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            let _p = protect(result);
            for (i, v) in values.iter().enumerate() {
                *INTEGER(result).add(i) = *v as c_int;
            }
            return result;
        }
        if keep_real {
            if numerals == "warn.loss" && loss {
                let msg = format!(
                    "accuracy loss in conversion from \"{}\" to numeric",
                    elt_to_string(x, 0)
                );
                let cmsg = std::ffi::CString::new(msg).unwrap_or_default();
                crate::mainutils::errors::Rf_warning(cmsg.as_ptr());
            }
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            let _p = protect(result);
            for (i, v) in values.iter().enumerate() {
                *REAL(result).add(i) = *v;
            }
            return result;
        }
        if as_is {
            let na_spec = tagged("na.strings");
            if na_spec.is_null() || na_spec == R_NilValue() || TYPEOF(na_spec) != SEXPTYPE::STRSXP {
                return x;
            }
            let out = crate::mainutils::duplicate::duplicate(x);
            let _o = protect(out);
            for i in 0..n {
                let s = elt_to_string(x, i);
                for k in 0..XLENGTH(na_spec) {
                    if s == elt_to_string(na_spec, k) {
                        SET_STRING_ELT(out, i, crate::sexp::globals::R_NaString());
                        break;
                    }
                }
            }
            return out;
        }
        let na_spec = tagged("na.strings");
        let mut na_strings: Vec<String> = Vec::new();
        if na_spec.is_null() || na_spec == R_NilValue() {
            na_strings.push("NA".to_string());
        } else if TYPEOF(na_spec) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(na_spec) {
                na_strings.push(elt_to_string(na_spec, i));
            }
        }
        let mut levels_v: Vec<String> = Vec::new();
        let mut is_na = vec![false; n as usize];
        for i in 0..n {
            let ch = STRING_ELT(x, i);
            if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                is_na[i as usize] = true;
                continue;
            }
            let s = elt_to_string(x, i);
            if na_strings.iter().any(|n| n == &s) {
                is_na[i as usize] = true;
                continue;
            }
            if !levels_v.iter().any(|l| l == &s) {
                levels_v.push(s);
            }
        }
        levels_v.sort();
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        let _p = protect(result);
        for i in 0..n {
            if is_na[i as usize] {
                *INTEGER(result).add(i as usize) = NA_INTEGER;
            } else {
                let s = elt_to_string(x, i);
                let code = levels_v.iter().position(|l| l == &s).unwrap_or(0) as c_int + 1;
                *INTEGER(result).add(i as usize) = code;
            }
        }
        let levels = Rf_allocVector3(SEXPTYPE::STRSXP, levels_v.len() as i64);
        let _l = protect(levels);
        for (i, level) in levels_v.iter().enumerate() {
            let c = std::ffi::CString::new(level.as_str()).unwrap_or_default();
            SET_STRING_ELT(levels, i as i64, Rf_mkChar(c.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::symbol::Rf_install(c"levels".as_ptr()),
            levels,
        );
        let class = Rf_mkString(c"factor".as_ptr());
        let _c = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

/// R's `as.environment(x)` — convert to environment.
pub unsafe fn do_as_environment(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }
        // GNU envir.c: ENVSXP before DispatchOrEval so a classed environment
        // is returned as-is and does not look up as.environment.<class>.
        if TYPEOF(x) == SEXPTYPE::ENVSXP {
            return x;
        }
        let mut ans = R_NilValue();
        if crate::eval::attrib_core::isObject(x) != 0
            && crate::eval::dispatch::DispatchOrEval(
                call,
                op,
                c"as.environment".as_ptr(),
                args,
                rho,
                &mut ans,
                0,
                1,
            ) != 0
        {
            return ans;
        }
        if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::REALSXP {
            let pos = if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x)
            } else {
                *REAL(x) as c_int
            };
            return search_env_from_position(pos);
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            let name = if XLENGTH(x) == 0 {
                "NA".to_string()
            } else {
                elt_to_string(x, 0)
            };
            return search_env_from_name(&name);
        }
        if TYPEOF(x) == SEXPTYPE::VECSXP || TYPEOF(x) == SEXPTYPE::LISTSXP {
            // Upstream as.environment on a named list/pairlist: bindings
            // from the elements, parent = emptyenv() (whisker's partials
            // path relies on this).
            let n = XLENGTH(x);
            let env = crate::sexp::envir::R_NewHashedEnv(crate::sexp::globals::R_EmptyEnv(), 0);
            let _env_guard = protect(env);
            if n == 0 {
                return env;
            }
            let names =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            let names_ok = !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && XLENGTH(names) == n
                && (0..n).all(|i| {
                    let si = crate::sexp::accessors::STRING_ELT(names, i as i64);
                    !si.is_null() && si != crate::sexp::globals::R_NaString()
                });
            if !names_ok {
                std::panic::panic_any(RError {
                    message: "names(x) must be a character vector of the same length as x"
                        .to_string(),
                });
            }
            for i in 0..n {
                let value = if TYPEOF(x) == SEXPTYPE::VECSXP {
                    crate::sexp::accessors::VECTOR_ELT(x, i as i64)
                } else {
                    // pairlist walk: CAR of the i-th cons cell
                    let mut cell = x;
                    for _ in 0..i {
                        cell = CDR(cell);
                    }
                    CAR(cell)
                };
                let name = elt_to_string(names, i);
                let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
                crate::sexp::envir::defineVar(sym, value, env);
            }
            return env;
        }
        if TYPEOF(x) == SEXPTYPE::NILSXP {
            std::panic::panic_any(RError {
                message: "using 'as.environment(NULL)' is defunct".to_string(),
            });
        }
        if TYPEOF(x) == SEXPTYPE::OBJSXP {
            let data = crate::mainutils::subassign::R_getS4DataSlot(x, SEXPTYPE::ENVSXP.as_c_int());
            if TYPEOF(data) == SEXPTYPE::ENVSXP {
                return data;
            }
            std::panic::panic_any(RError {
                message: "S4 object does not extend class \"environment\"".to_string(),
            });
        }

        std::panic::panic_any(RError {
            message: "invalid object for as.environment".to_string(),
        });
    }
}

/// R's `pos.to.env(pos)` — map a search path position to an environment.
pub unsafe fn do_pos_to_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pos = integer_arg_by_name_or_position(args, "pos", 0).unwrap_or(NA_INTEGER);
        search_env_from_position(pos)
    }
}

pub(crate) unsafe fn search_env_from_position(pos: c_int) -> SEXP {
    unsafe {
        if pos == -1 {
            let mut ctx = crate::sexp::context::R_GlobalContext();
            while !ctx.is_null() {
                if (*ctx).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION != 0 {
                    let parent = (*ctx).sysparent.as_raw();
                    if !parent.is_null() && parent != R_NilValue() {
                        return parent;
                    }
                    break;
                }
                ctx = (*ctx).nextcontext;
            }
            return crate::sexp::globals::R_GlobalEnv();
        }
        if pos > 0
            && let Some((_, env)) = search_path_entries().get((pos - 1) as usize)
        {
            return *env;
        }
        std::panic::panic_any(RError {
            message: "invalid 'pos' argument".to_string(),
        });
    }
}

pub(crate) unsafe fn search_env_from_name(name: &str) -> SEXP {
    for (label, env) in unsafe { search_path_entries() } {
        if label == name || (name == "base" && label == "package:base") {
            return env;
        }
    }
    std::panic::panic_any(RError {
        message: format!("no item called \"{name}\" on the search list"),
    });
}

unsafe fn search_path_len() -> c_int {
    unsafe { search_path_entries().len() as c_int }
}

pub(crate) unsafe fn search_path_entries() -> Vec<(String, SEXP)> {
    unsafe {
        let global = crate::sexp::globals::R_GlobalEnv();
        let base = crate::sexp::globals::R_BaseEnv();
        if global.is_null() || base.is_null() {
            return Vec::new();
        }

        let mut entries = vec![(".GlobalEnv".to_string(), global)];
        let mut env = crate::sexp::accessors::ENCLOS(global);
        while !env.is_null() && env != base {
            entries.push((search_env_label(env), env));
            env = crate::sexp::accessors::ENCLOS(env);
        }
        entries.push(("package:base".to_string(), base));
        entries
    }
}

unsafe fn search_env_label(env: SEXP) -> String {
    unsafe {
        let name = crate::sexp::attrib_core::getAttrib(env, Rf_install(c"name".as_ptr()));
        if TYPEOF(name) == SEXPTYPE::STRSXP && XLENGTH(name) > 0 {
            let value = STRING_ELT(name, 0);
            if !value.is_null() && value != R_NilValue() {
                return CStr::from_ptr(CHAR(value)).to_string_lossy().into_owned();
            }
        }
        "(unknown)".to_string()
    }
}

/// R's `searchpaths()` — filesystem/search labels for entries on the search path.
pub unsafe fn do_searchpaths(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let entries = search_path_entries();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, entries.len() as R_xlen_t);
        for (i, (label, _)) in entries.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(label.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

/// R's `sort.list(x, partial, na.last, decreasing, method)` — indices for sorting.
pub unsafe fn do_sort_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let decreasing = sort_logical_arg(args, &["decreasing"], 3).unwrap_or(false);
        let na_placement = order_na_placement(args, 2);
        let mut indices = ordered_atomic_indices(x, decreasing, na_placement);
        if na_placement == SortNaPlacement::Remove {
            let compressed_positions = nonmissing_compressed_positions(x);
            for index in &mut indices {
                *index = compressed_positions[*index as usize];
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for (i, idx) in indices.iter().enumerate() {
            *INTEGER(result).add(i) = (*idx + 1) as c_int; // 1-indexed
        }
        result
    }
}

fn nonmissing_compressed_positions(x: SEXP) -> Vec<R_xlen_t> {
    unsafe {
        let n = XLENGTH(x);
        let mut positions = vec![0; n as usize];
        let mut next = 0;
        for i in 0..n {
            let missing = match TYPEOF(x) {
                t if t == SEXPTYPE::STRSXP => charsxp_is_na(STRING_ELT(x, i)),
                t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                    *INTEGER(x).add(i as usize) == NA_INTEGER
                }
                t if t == SEXPTYPE::REALSXP => ISNAN(*REAL(x).add(i as usize)),
                _ => ISNAN(elt_real_safe(x, i)),
            };
            if !missing {
                positions[i as usize] = next;
                next += 1;
            }
        }
        positions
    }
}

/// R's `outer(X, Y, FUN)` — outer product (enhanced).
pub unsafe fn do_outer_enhanced(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let fun = CAR(CDR(CDR(args)));
        if x.is_null() || y.is_null() {
            return R_NilValue();
        }
        let nx = XLENGTH(x);
        let ny = XLENGTH(y);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, nx * ny);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        // Default: multiplication
        if nx > 0 && ny > 0 {
            let dst = REAL(result);
            for i in 0..nx {
                let xi = elt_real_safe(x, i);
                for j in 0..ny {
                    let yj = elt_real_safe(y, j);
                    *dst.add((i * ny + j) as usize) = xi * yj;
                }
            }
        }

        // Set dim attribute
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = nx as c_int;
            *INTEGER(dim).add(1) = ny as c_int;
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"dim".as_ptr()), dim);
        }
        result
    }
}

/// GNU `match.fun(FUN)`: functions pass through; character/symbol names
/// are looked up as functions (sweep's `FUN = "-"`).
pub unsafe fn do_match_fun(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut x = CAR(args);
        if TYPEOF(x) == SEXPTYPE::PROMSXP {
            x = crate::sexp::envir::forcePromise(x);
        }
        if x.is_null() {
            return R_NilValue();
        }
        if TYPEOF(x) == SEXPTYPE::CLOSXP
            || TYPEOF(x) == SEXPTYPE::BUILTINSXP
            || TYPEOF(x) == SEXPTYPE::SPECIALSXP
        {
            return x;
        }
        let sym = if TYPEOF(x) == SEXPTYPE::SYMSXP {
            x
        } else if TYPEOF(x) == SEXPTYPE::STRSXP && XLENGTH(x) >= 1 {
            Rf_install(CHAR(STRING_ELT(x, 0)))
        } else {
            return x;
        };
        let val = crate::sexp::envir::findFun(sym, rho);
        if !val.is_null()
            && val != R_UnboundValue()
            && (TYPEOF(val) == SEXPTYPE::CLOSXP
                || TYPEOF(val) == SEXPTYPE::BUILTINSXP
                || TYPEOF(val) == SEXPTYPE::SPECIALSXP)
        {
            return val;
        }
        x
    }
}
