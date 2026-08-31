use super::*;


// ---------------------------------------------------------------------------
// do_call — R's call() primitive
// ---------------------------------------------------------------------------

/// Construct an unevaluated call from a function name and evaluated arguments.
///
/// Ported from R's `do_call()` in coerce.c.
pub unsafe fn do_call(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::eval::Rf_eval;
    use crate::mainutils::errors::Rf_error;
    use crate::sexp::accessors::{CAR, CDR, CHAR, SETCAR, STRING_ELT};
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::symbol::Rf_install;

    unsafe {
        if crate::sexp::constructors::Rf_length(args) < 1 {
            Rf_error(b"'name' is missing\0".as_ptr() as *const c_char);
        }

        let rfun = Rf_eval(CAR(args), rho);
        let _rfun_guard = protect(rfun);

        if !isString(rfun) || LENGTH(rfun) != 1 {
            Rf_error(b"first argument must be a character string\0".as_ptr() as *const c_char);
        }

        let str = CHAR(STRING_ELT(rfun, 0));
        if !str.is_null() {
            let s = std::ffi::CStr::from_ptr(str);
            if s.to_bytes() == b".Internal" {
                Rf_error(b"illegal usage\0".as_ptr() as *const c_char);
            }
        }

        let sym = Rf_install(str);
        let _sym_guard = protect(sym);

        // Evaluate remaining arguments
        let evargs = CDR(args);
        // Walk args and evaluate each
        let mut rest = evargs;
        while !rest.is_null() && rest != R_NilValue() {
            let tmp = Rf_eval(CAR(rest), rho);
            SETCAR(rest, tmp);
            rest = CDR(rest);
        }

        // Build LANGSXP: (sym arg1 arg2 ...)
        let result = Rf_cons(sym, evargs);
        if !result.is_null() {
            (*result).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_docall — R's do.call() primitive
// ---------------------------------------------------------------------------

/// Construct and evaluate a call from a function and argument list.
///
/// Ported from R's `do_docall()` in coerce.c.
pub unsafe fn do_docall(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::attrib_core::{R_NamesSymbol, getAttrib};
    use crate::mainutils::errors::Rf_error;
    use crate::mainutils::subset::installTrChar;
    use crate::sexp::accessors::{
        CADDR, CADR, CAR, CDR, CHAR, LENGTH, SETCAR, SETTAG, STRING_ELT, TYPEOF, VECTOR_ELT,
    };
    use crate::sexp::constructors::Rf_allocVector;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::protect::protect;
    use crate::sexp::symbol::Rf_install;

    unsafe {
        let fun = CAR(args);
        let envir = CADDR(args);
        let cargs = CADR(args);

        // fun must be a function or a single character string
        if !isFunction(fun) && !(isString(fun) && LENGTH(fun) == 1) {
            Rf_error(b"'what' must be a function or character string\0".as_ptr() as *const c_char);
        }

        if !cargs.is_null() && cargs != R_NilValue() && TYPEOF(cargs) != SEXPTYPE::VECSXP {
            Rf_error(b"'args' must be a list\0".as_ptr() as *const c_char);
        }

        if !isEnvironment(envir) {
            Rf_error(b"'envir' must be an environment\0".as_ptr() as *const c_char);
        }

        let n = if cargs.is_null() || cargs == R_NilValue() {
            0
        } else {
            LENGTH(cargs)
        };
        let names = if n > 0 {
            getAttrib(cargs, R_NamesSymbol())
        } else {
            R_NilValue()
        };
        let _names_guard = protect(names);

        // Build LANGSXP call: (fun arg1 arg2 ...)
        // LANGSXP has n+1 slots: function + n args
        let newcall = Rf_allocVector(SEXPTYPE::LANGSXP, n + 1);
        let _newcall_guard = protect(newcall);

        if isString(fun) {
            let str = CHAR(STRING_ELT(fun, 0));
            if !str.is_null() {
                let s = std::ffi::CStr::from_ptr(str);
                if s.to_bytes() == b".Internal" {
                    Rf_error(b"illegal usage\0".as_ptr() as *const c_char);
                }
            }
            SETCAR(newcall, Rf_install(str));
        } else {
            // Check for .Internal primitive
            let prim_name = crate::eval::builtin::PRIMNAME(fun);
            if prim_name == ".Internal" {
                Rf_error(b"illegal usage\0".as_ptr() as *const c_char);
            }
            SETCAR(newcall, fun);
        }

        let mut c = CDR(newcall);
        for i in 0..n as usize {
            if TYPEOF(cargs) == SEXPTYPE::VECSXP {
                SETCAR(c, VECTOR_ELT(cargs, i as R_xlen_t));
            }
            // Set tag from names attribute
            if !names.is_null() && names != R_NilValue() {
                let name_elt = STRING_ELT(names, i as R_xlen_t);
                if !name_elt.is_null() && name_elt != R_NilValue() {
                    let ch = CHAR(name_elt);
                    if !ch.is_null() && *ch != 0 {
                        SETTAG(c, installTrChar(name_elt));
                    }
                }
            }
            c = CDR(c);
        }

        let result = crate::eval::eval::Rf_eval(newcall, envir);
        result
    }
}

// ---------------------------------------------------------------------------
// substitute — core AST substitution
// ---------------------------------------------------------------------------

/// Substitute symbols in an expression using bindings from an environment.
///
/// Ported from R's `substitute()` in coerce.c.
pub unsafe fn substitute(lang: SEXP, rho: SEXP) -> SEXP {
    use crate::mainutils::errors::Rf_error;
    use crate::sexp::accessors::{PRCODE, TYPEOF};
    use crate::sexp::envir::R_findVarInFrame;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::{R_GlobalEnv, R_NilValue, R_UnboundValue};

    unsafe {
        match TYPEOF(lang) {
            t if t == SEXPTYPE::PROMSXP => substitute(PRCODE(lang), rho),
            t if t == SEXPTYPE::SYMSXP => {
                if rho != R_NilValue() {
                    let t = R_findVarInFrame(rho, lang);
                    if t != R_UnboundValue() {
                        if TYPEOF(t) == SEXPTYPE::PROMSXP {
                            let mut expr = PRCODE(t);
                            while TYPEOF(expr) == SEXPTYPE::PROMSXP {
                                expr = PRCODE(expr);
                            }
                            // ENSURE_NAMEDMAX
                            if NAMED(expr) < 2 {
                                SET_NAMED(expr, 2);
                            }
                            return expr;
                        } else if TYPEOF(t) == SEXPTYPE::DOTSXP {
                            Rf_error(
                                b"'...' used in an incorrect context\0".as_ptr() as *const c_char
                            );
                        }
                        if rho != R_GlobalEnv() {
                            return t;
                        }
                    }
                }
                lang
            }
            t if t == SEXPTYPE::LANGSXP => substitute_list(lang, rho),
            _ => lang,
        }
    }
}

// ---------------------------------------------------------------------------
// substituteList — substitute with ... expansion
// ---------------------------------------------------------------------------

/// Walk a pairlist performing substitution, expanding `...` bindings.
///
/// Ported from R's `substituteList()` in coerce.c.
pub unsafe fn substitute_list(el: SEXP, rho: SEXP) -> SEXP {
    use crate::sexp::accessors::{CAR, CDR, SETCDR, SETTAG, TAG, TYPEOF};
    use crate::sexp::envir::R_findVarInFrame;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::{R_MissingArg, R_NilValue, R_UnboundValue};
    use crate::sexp::symbol::R_DotsSymbol;

    unsafe {
        if el.is_null() || el == R_NilValue() {
            return el;
        }

        let mut res: SEXP = R_NilValue();
        let mut p: SEXP = ptr::null_mut();
        let mut remaining = el;
        let mut guards = Vec::new();

        while !remaining.is_null() && remaining != R_NilValue() {
            let mut h: SEXP;

            if CAR(remaining) == R_DotsSymbol() {
                if rho == R_NilValue() {
                    h = R_UnboundValue();
                } else {
                    h = R_findVarInFrame(rho, CAR(remaining));
                }
                if h == R_UnboundValue() {
                    h = Rf_cons(R_DotsSymbol(), R_NilValue());
                    guards.push(protect(h));
                } else if h == R_NilValue() || h == R_MissingArg() {
                    h = R_NilValue();
                } else if TYPEOF(h) == SEXPTYPE::DOTSXP {
                    guards.push(protect(h));
                    h = substitute_list(h, R_NilValue());
                    // h is now a substituted pairlist — don't unprotect the protected one yet
                } else {
                    crate::mainutils::errors::Rf_error(
                        b"'...' used in an incorrect context\0".as_ptr() as *const c_char,
                    );
                    unreachable!()
                }

                if TYPEOF(h) == SEXPTYPE::DOTSXP || (h != R_NilValue() && !h.is_null()) {
                    guards.push(protect(h));
                }
            } else {
                h = substitute(CAR(remaining), rho);
                // ENSURE_NAMEDMAX
                if !h.is_null() && NAMED(h) < 2 {
                    SET_NAMED(h, 2);
                }
                h = Rf_cons(h, R_NilValue());
                SETTAG(h, TAG(remaining));
            }

            if !h.is_null() && h != R_NilValue() {
                if res == R_NilValue() {
                    guards.push(protect(h));
                    res = h;
                } else {
                    SETCDR(p, h);
                }
                // Walk to end of h (dots may have expanded to multiple elements)
                let mut tail = h;
                while !CDR(tail).is_null() && CDR(tail) != R_NilValue() {
                    tail = CDR(tail);
                }
                p = tail;
            }

            remaining = CDR(remaining);
        }

        if res != R_NilValue() && TYPEOF(el) == SEXPTYPE::LANGSXP {
            (*res).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        res
    }
}

// ---------------------------------------------------------------------------
// do_substitute — R-level substitute() entry point
// ---------------------------------------------------------------------------

/// R's `substitute()` primitive.
///
/// Ported from R's `do_substitute()` in coerce.c.
pub unsafe fn do_substitute(call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::eval::eval::Rf_eval;
    use crate::sexp::accessors::TYPEOF;
    use crate::sexp::constructors::Rf_cons;
    use crate::sexp::ffi::SEXPTYPE;
    use crate::sexp::globals::{R_BaseEnv, R_GlobalEnv, R_MissingArg, R_NilValue};
    use crate::sexp::memory_ext::NewEnvironment;
    use crate::sexp::protect::protect;

    unsafe {
        let (expr, env_arg) = match_substitute_args(args);

        let mut env = if env_arg == R_MissingArg() {
            rho
        } else {
            Rf_eval(env_arg, rho)
        };

        // Historical: don't substitute in R_GlobalEnv
        if env == R_GlobalEnv() {
            env = R_NilValue();
        } else if TYPEOF(env) == SEXPTYPE::VECSXP {
            // Convert VECSXP to environment
            let plist = crate::mainutils::subassign::VectorToPairList(env);
            let _plist_guard = protect(plist);
            env = NewEnvironment(plist, R_BaseEnv(), R_NilValue());
        } else if TYPEOF(env) == SEXPTYPE::LISTSXP {
            // Convert pairlist to environment
            env = NewEnvironment(env, R_BaseEnv(), R_NilValue());
        }

        if env != R_NilValue() && TYPEOF(env) != SEXPTYPE::ENVSXP {
            crate::mainutils::errors::Rf_error(
                b"invalid environment specified\0".as_ptr() as *const c_char
            );
        }

        let _env_guard = protect(env);
        // Duplicate the expression and wrap in a list for substituteList
        let t = Rf_cons(expr, R_NilValue());
        let _t_guard = protect(t);
        let s = substitute_list(t, env);
        let result = if !s.is_null() && s != R_NilValue() {
            crate::sexp::accessors::CAR(s)
        } else {
            R_NilValue()
        };
        result
    }
}

pub unsafe fn match_substitute_args(args: SEXP) -> (SEXP, SEXP) {
    unsafe {
        let mut formals = [SubstituteFormal::new("expr"), SubstituteFormal::new("env")];
        let mut actuals = Vec::new();
        let mut current = args;
        let mut index = 1;

        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            let tag = TAG(current);
            let name = if tag.is_null() || tag == R_NilValue() {
                None
            } else {
                symbol_name(tag)
            };
            actuals.push(SubstituteActual {
                index,
                value,
                tag,
                name,
                used: false,
            });
            current = CDR(current);
            index += 1;
        }

        for actual in actuals.iter_mut().filter(|actual| actual.name.is_some()) {
            let Some(formal_index) = formals
                .iter()
                .position(|formal| Some(formal.name) == actual.name.as_deref())
            else {
                continue;
            };
            if formals[formal_index].value != R_MissingArg() {
                duplicate_substitute_arg(formals[formal_index].name);
            }
            formals[formal_index].value = actual.value;
            formals[formal_index].match_source = Some(SubstituteMatchSource::Exact);
            actual.used = true;
        }

        for actual in actuals.iter_mut().filter(|actual| actual.name.is_some()) {
            if actual.used {
                continue;
            }

            let name = actual.name.as_deref().unwrap_or_default();
            let matches = formals
                .iter()
                .enumerate()
                .filter(|(_, formal)| {
                    formal.name.starts_with(name)
                        && (formal.value == R_MissingArg()
                            || formal.match_source == Some(SubstituteMatchSource::Partial))
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();

            if matches.len() > 1 {
                substitute_error(format!(
                    "argument {} matches multiple formal arguments",
                    actual.index
                ));
            }

            if let Some(formal_index) = matches.first() {
                if formals[*formal_index].value != R_MissingArg() {
                    duplicate_substitute_arg(formals[*formal_index].name);
                }
                formals[*formal_index].value = actual.value;
                formals[*formal_index].match_source = Some(SubstituteMatchSource::Partial);
                actual.used = true;
            }
        }

        for actual in actuals.iter_mut().filter(|actual| actual.name.is_none()) {
            if let Some(formal) = formals
                .iter_mut()
                .find(|formal| formal.value == R_MissingArg())
            {
                formal.value = actual.value;
                formal.match_source = Some(SubstituteMatchSource::Positional);
                actual.used = true;
            }
        }

        if let Some(actual) = actuals.iter().find(|actual| !actual.used) {
            unused_substitute_arg(actual.tag, actual.value);
        }

        (formals[0].value, formals[1].value)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SubstituteMatchSource {
    Exact,
    Partial,
    Positional,
}

pub struct SubstituteFormal {
    pub name: &'static str,
    pub value: SEXP,
    pub match_source: Option<SubstituteMatchSource>,
}

impl SubstituteFormal {
    pub fn new(name: &'static str) -> Self {
        unsafe {
            Self {
                name,
                value: R_MissingArg(),
                match_source: None,
            }
        }
    }
}

pub struct SubstituteActual {
    pub index: usize,
    pub value: SEXP,
    pub tag: SEXP,
    pub name: Option<String>,
    pub used: bool,
}

pub fn duplicate_substitute_arg(name: &str) -> ! {
    substitute_error(format!(
        r#"formal argument "{name}" matched by multiple actual arguments"#
    ));
}

pub unsafe fn symbol_name(symbol: SEXP) -> Option<String> {
    unsafe {
        if symbol.is_null() || symbol == R_NilValue() || TYPEOF(symbol) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(symbol);
        if printname.is_null() || printname == R_NilValue() {
            return None;
        }
        let chars = CHAR(printname);
        if chars.is_null() {
            None
        } else {
            Some(CStr::from_ptr(chars).to_string_lossy().into_owned())
        }
    }
}

pub unsafe fn deparse_substitute_arg(value: SEXP) -> String {
    unsafe {
        if value == R_MissingArg() {
            return String::new();
        }
        let text = crate::mainutils::deparse::deparse1line(value, false);
        if text.is_null() || text == R_NilValue() || XLENGTH(text) == 0 {
            return String::new();
        }
        let elt = STRING_ELT(text, 0);
        if elt.is_null() || elt == R_NilValue() {
            return String::new();
        }
        let chars = CHAR(elt);
        if chars.is_null() {
            String::new()
        } else {
            CStr::from_ptr(chars).to_string_lossy().into_owned()
        }
    }
}

pub unsafe fn unused_substitute_arg(tag: SEXP, value: SEXP) -> ! {
    unsafe {
        let deparsed = deparse_substitute_arg(value);
        let arg = match symbol_name(tag) {
            Some(name) if !name.is_empty() => format!("{name} = {deparsed}"),
            _ => deparsed,
        };
        substitute_error(format!("unused argument ({arg})"));
    }
}

pub fn substitute_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

// ---------------------------------------------------------------------------
// do_storage_mode — storage.mode<- assignment
// ---------------------------------------------------------------------------

/// `storage.mode(x) <- value` — change the storage mode of an object.
///
/// Ported from R's `do_storage_mode()` in coerce.c.
pub unsafe fn do_storage_mode(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    use crate::mainutils::errors::Rf_error;
    use crate::sexp::accessors::{CADR, CAR, CHAR, SET_ATTRIB, STRING_ELT, TYPEOF};
    use crate::sexp::protect::protect;

    unsafe {
        let obj = CAR(args);
        let value = CADR(args);

        // value must be a non-null character string
        if !isString(value)
            || LENGTH(value) < 1
            || STRING_ELT(value, 0) == crate::sexp::globals::R_NaString()
        {
            Rf_error(b"'value' must be non-null character string\0".as_ptr() as *const c_char);
        }

        let type_str = CHAR(STRING_ELT(value, 0));
        let target_type = str2type(type_str);

        if target_type == -1 as c_int {
            let s = std::ffi::CStr::from_ptr(type_str);
            if s.to_bytes() == b"real" {
                Rf_error(
                    b"use of 'real' is defunct: use 'double' instead\0".as_ptr() as *const c_char
                );
            } else if s.to_bytes() == b"single" {
                Rf_error(
                    b"use of 'single' is defunct: use mode<- instead\0".as_ptr() as *const c_char
                );
            } else {
                Rf_error(b"invalid value\0".as_ptr() as *const c_char);
            }
        }

        if TYPEOF(obj) == target_type {
            return obj;
        }

        // Check for factor
        if crate::mainutils::apply::isFactor(obj) != 0 {
            Rf_error(b"invalid to change the storage mode of a factor\0".as_ptr() as *const c_char);
        }

        let ans = coerceVector(obj, target_type);
        let _ans_guard = protect(ans);

        // Copy attributes preserving OBJECT and S4 bits
        SET_ATTRIB(ans, crate::sexp::accessors::ATTRIB(obj));

        ans
    }
}

/// Map a type name string to a SEXPTYPE value.
///
/// Ported from R's `str2type()` in coerce.c.
pub fn str2type(s: *const c_char) -> c_int {
    use crate::sexp::ffi::SEXPTYPE;
    if s.is_null() {
        return -1;
    }
    let bytes = unsafe { std::ffi::CStr::from_ptr(s).to_bytes() };
    match bytes {
        b"logical" => SEXPTYPE::LGLSXP.into(),
        b"integer" => SEXPTYPE::INTSXP.into(),
        b"double" => SEXPTYPE::REALSXP.into(),
        b"complex" => SEXPTYPE::CPLXSXP.into(),
        b"character" => SEXPTYPE::STRSXP.into(),
        b"raw" => SEXPTYPE::RAWSXP.into(),
        b"list" => SEXPTYPE::VECSXP.into(),
        b"expression" => SEXPTYPE::EXPRSXP.into(),
        _ => -1,
    }
}
