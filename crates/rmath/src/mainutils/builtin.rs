#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::{c_char, c_int};

use crate::mainutils::coerce::{ISNAN, R_FINITE, asInteger, asLogical, asReal};
use crate::mainutils::duplicate::duplicate;
use crate::mainutils::relop::{PRIMNAME, checkArity};
use crate::sexp::accessors::{
    BODY, CADDR, CADR, CAR, CDR, CHAR, CLOENV, ENCLOS, FORMALS, LENGTH, PRINTNAME, SET_BODY,
    SET_CLOENV, SET_ENCLOS, SET_FORMALS, SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETCDR,
    STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::attrib_core::{getAttrib, setAttrib};
use crate::sexp::constructors::{Rf_allocVector, Rf_isEnvironment, Rf_isString, Rf_length};
use crate::sexp::context::R_GlobalContext;
use crate::sexp::envir::{R_findVarInFrame, defineVar, findFun};
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_BaseEnv, R_GlobalEnv, R_MissingArg, R_NilValue, R_UnboundValue};
use crate::sexp::memory_ext::{allocSExp, mkPROMISE};
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Local error helpers (same pattern as coerce.rs, objects.rs, etc.)
// ---------------------------------------------------------------------------

fn error(msg: &str) {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

fn errorcall(_call: SEXP, msg: &str) {
    {
        error(msg);
    }
}

fn warningcall(call: SEXP, msg: &str) {
    let c = std::ffi::CString::new(msg).unwrap_or_default();
    crate::mainutils::errors::warningcall(call, c.as_ptr());
}

fn isNull(x: SEXP) -> bool {
    crate::sexp::accessors::Rf_isNull(x) != 0
}

// ---------------------------------------------------------------------------
// Checked-access boundary helpers
//
// The builtins below run inside evaluated R calls, so every raw SEXP they
// handle is live under the active runtime. The few operations that still
// need unsafe are kept here as small audited boundaries so the handlers can
// stay on checked element access.
// ---------------------------------------------------------------------------

/// Checked node factory bound to the active runtime session.
fn node_factory() -> crate::sexp::object::SessionNodeFactory<'static> {
    crate::sexp::object::SessionNodeFactory::new(
        // SAFETY: builtin handlers only run inside evaluated R calls.
        unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
    )
}

fn wrapped(x: SEXP) -> crate::sexp::object::Sexp<'static> {
    node_factory()
        .wrap(x)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

fn mutable_vector(x: SEXP) -> crate::sexp::object::SexpMut<'static> {
    node_factory()
        .wrap(x)
        .and_then(crate::sexp::object::SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked STRSXP(1) scalar; matches `Rf_mkString` on the session heap.
fn mk_string(text: &str) -> SEXP {
    node_factory()
        .strings(&[text])
        .map(|vector| vector.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Audited boundary: intern a live CHARSXP's bytes as a symbol.
fn installTrChar(x: SEXP) -> SEXP {
    // SAFETY: a live CHARSXP exposes NUL-terminated, immutable CHAR data;
    // Rf_install interns the name under the active runtime.
    unsafe {
        let p = CHAR(x);
        if p.is_null() {
            error("invalid string in installTrChar");
        }
        Rf_install(p)
    }
}

/// Audited boundary: read a live CHARSXP's bytes.
fn charsxp_bytes(x: SEXP) -> Vec<u8> {
    // SAFETY: a live CHARSXP exposes NUL-terminated, immutable CHAR data for
    // the duration of the copy.
    unsafe { std::ffi::CStr::from_ptr(CHAR(x)).to_bytes().to_vec() }
}

/// Audited boundary: intern a live primitive's static name as a symbol.
fn install_prim_name(op: SEXP) -> SEXP {
    // SAFETY: PRIMNAME of a live primitive yields a static NUL-terminated
    // name; Rf_install interns it under the active runtime.
    unsafe { Rf_install(PRIMNAME(op)) }
}

/// Audited boundary: evaluate a live expression in a live environment.
fn eval_sexp(expr: SEXP, env: SEXP) -> SEXP {
    // SAFETY: evaluates under the same ambient contract as the evaluator.
    unsafe { crate::eval::eval::Rf_eval(expr, env) }
}

/// Audited boundary: bind one symbol in a live environment.
fn define_var(symbol: SEXP, value: SEXP, env: SEXP) {
    // SAFETY: defineVar binds a value under the active runtime.
    unsafe { defineVar(symbol, value, env) }
}

/// Audited boundary: function lookup in a live environment.
fn find_fun(symbol: SEXP, env: SEXP) -> SEXP {
    // SAFETY: findFun searches bindings of a live environment.
    unsafe { findFun(symbol, env) }
}

/// Audited boundary: frame lookup in a live environment.
fn find_var_in_frame(rho: SEXP, symbol: SEXP) -> SEXP {
    // SAFETY: R_findVarInFrame reads bindings of a live environment.
    unsafe { R_findVarInFrame(rho, symbol) }
}

/// Audited boundary: deep duplicate of a live object.
fn duplicate_sexp(x: SEXP) -> SEXP {
    // SAFETY: duplicate copies a live object under the active runtime.
    unsafe { duplicate(x) }
}

/// Audited boundary: shallow duplicate of a live object.
fn shallow_duplicate(x: SEXP) -> SEXP {
    // SAFETY: shallow_duplicate copies a live object under the active runtime.
    unsafe { crate::mainutils::duplicate::shallow_duplicate(x) }
}

/// Audited boundary: enforce declared arity on live arguments.
fn check_arity(op: SEXP, args: SEXP) {
    // SAFETY: checkArity inspects a live primitive and argument list.
    unsafe { checkArity(op, args) }
}

/// Audited boundary: the value parked on the active return context, or null.
fn context_return_value() -> SEXP {
    // SAFETY: reads the parked value of a live global context.
    unsafe {
        let ctx = R_GlobalContext();
        if ctx.is_null() {
            std::ptr::null_mut()
        } else {
            (*ctx).returnValue.as_raw()
        }
    }
}

/// Audited boundary: the closure environment of the active context, or null.
fn context_cloenv() -> SEXP {
    // SAFETY: reads the cloenv slot of a live global context.
    unsafe {
        let ctx = R_GlobalContext();
        if ctx.is_null() {
            std::ptr::null_mut()
        } else {
            (*ctx).cloenv.as_raw()
        }
    }
}

fn first_string_char(x: SEXP) -> SEXP {
    if TYPEOF(x) == SEXPTYPE::CHARSXP {
        x
    } else {
        STRING_ELT(x, 0)
    }
}

#[allow(dead_code)]
fn listAppend(s: SEXP, t: SEXP) -> SEXP {
    if isNull(s) {
        return t;
    }
    let mut r = s;
    while !isNull(CDR(r)) {
        r = CDR(r);
    }
    SETCDR(r, t);
    s
}

fn RAISE_NAMED(x: SEXP, v: c_int) {
    let n = crate::sexp::accessors::NAMED(x);
    if n < v {
        crate::sexp::accessors::SET_NAMED(x, v);
    }
}

fn na_str() -> *const c_char {
    b"NA\0".as_ptr() as *const c_char as *const c_char
}

// ---------------------------------------------------------------------------
// asVecSize — convert SEXP to vector length
// ---------------------------------------------------------------------------

pub fn asVecSize(x: SEXP) -> R_xlen_t {
    {
        if crate::sexp::object::raw_is_atomic_vector(x) && LENGTH(x) >= 1 {
            match TYPEOF(x) {
                t if t == SEXPTYPE::INTSXP => {
                    let res = wrapped(x)
                        .try_integer_elt(0)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    if res == NA_INTEGER {
                        error("vector size cannot be NA");
                    }
                    return res as R_xlen_t;
                }
                t if t == SEXPTYPE::REALSXP => {
                    let d = wrapped(x)
                        .try_real_elt(0)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    if ISNAN(d) {
                        error("vector size cannot be NA/NaN");
                    }
                    if !R_FINITE(d) {
                        error("vector size cannot be infinite");
                    }
                    return d as R_xlen_t;
                }
                t if t == SEXPTYPE::STRSXP => {
                    let d = asReal(x);
                    if ISNAN(d) {
                        error("vector size cannot be NA/NaN");
                    }
                    if !R_FINITE(d) {
                        error("vector size cannot be infinite");
                    }
                    return d as R_xlen_t;
                }
                _ => {}
            }
        }
        -999 as R_xlen_t
    }
}

// ---------------------------------------------------------------------------
// do_delayed — delayedAssign()
// ---------------------------------------------------------------------------

pub unsafe fn do_delayed(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        if Rf_isString(CAR(args)) == 0 || LENGTH(CAR(args)) == 0 {
            error("invalid first argument");
        }
        let name = installTrChar(first_string_char(CAR(args)));
        let args = CDR(args);
        let expr = CAR(args);
        let args = CDR(args);
        let eenv = CAR(args);
        if isNull(eenv) {
            error("use of NULL environment is defunct");
        }
        if Rf_isEnvironment(eenv) == 0 {
            error("invalid 'eval.env' argument");
        }
        let args = CDR(args);
        let aenv = CAR(args);
        if isNull(aenv) {
            error("use of NULL environment is defunct");
        }
        if Rf_isEnvironment(aenv) == 0 {
            error("invalid 'assign.env' argument");
        }
        define_var(name, mkPROMISE(expr, eenv), aenv);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_makelazy — makeLazy()
// ---------------------------------------------------------------------------

pub unsafe fn do_makelazy(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let names = CAR(args);
        let args = CDR(args);
        if Rf_isString(names) == 0 {
            error("invalid first argument");
        }
        let values = CAR(args);
        let args = CDR(args);
        let expr = CAR(args);
        let args = CDR(args);
        let eenv = CAR(args);
        let args = CDR(args);
        if Rf_isEnvironment(eenv) == 0 {
            error("invalid 'eval.env' argument");
        }
        let aenv = CAR(args);
        if Rf_isEnvironment(aenv) == 0 {
            error("invalid 'assign.env' argument");
        }
        let n = XLENGTH(names);
        for i in 0..n {
            let name = installTrChar(if TYPEOF(names) == SEXPTYPE::CHARSXP {
                names
            } else {
                STRING_ELT(names, i)
            });
            let val = eval_sexp(VECTOR_ELT(values, i), eenv);
            let expr0 = duplicate_sexp(expr);
            SETCAR(CDR(expr0), val);
            define_var(name, mkPROMISE(expr0, eenv), aenv);
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_onexit — on.exit()
// ---------------------------------------------------------------------------

pub unsafe fn do_onexit(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        let _ = (call, op);
        // SAFETY: delegates to the audited on.exit implementation on live args.
        unsafe { crate::eval::special::do_on_exit_from_args(args, rho) }
    }
}

/// GNU `returnValue(default)`. The value parked on the current context
/// before `on.exit` runs; an error or a frame that did not return uses
/// the already-evaluated default.
pub unsafe fn do_returnValue(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let value = context_return_value();
        if !value.is_null() {
            return value;
        }
        if args.is_null() || args == crate::sexp::globals::R_NilValue() {
            crate::sexp::globals::R_NilValue()
        } else {
            CAR(args)
        }
    }
}

// ---------------------------------------------------------------------------
// do_args — args()
// ---------------------------------------------------------------------------

pub unsafe fn do_args(_call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let mut input = CAR(args);
        if TYPEOF(input) == SEXPTYPE::STRSXP && LENGTH(input) == 1 {
            let s = installTrChar(first_string_char(input));
            input = find_fun(s, rho);
        }
        if TYPEOF(input) == SEXPTYPE::CLOSXP {
            let s = allocSExp(SEXPTYPE::CLOSXP);
            SET_FORMALS(s, FORMALS(input));
            SET_BODY(s, R_NilValue());
            SET_CLOENV(s, R_GlobalEnv());
            return s;
        }
        if TYPEOF(input) == SEXPTYPE::BUILTINSXP || TYPEOF(input) == SEXPTYPE::SPECIALSXP {
            let args_env = find_var_in_frame(
                R_BaseEnv(),
                crate::sexp::symbol::install_str(".ArgsEnv"),
            );
            let env = if TYPEOF(args_env) == SEXPTYPE::PROMSXP {
                eval_sexp(args_env, R_BaseEnv())
            } else {
                args_env
            };
            let nm_sym = install_prim_name(input);
            let s2 = find_var_in_frame(env, nm_sym);
            if s2 != R_UnboundValue() {
                let s = duplicate_sexp(s2);
                SET_BODY(s, R_NilValue());
                SET_CLOENV(s, R_GlobalEnv());
                return s;
            }
            let generic_env = find_var_in_frame(
                R_BaseEnv(),
                crate::sexp::symbol::install_str(".GenericArgsEnv"),
            );
            let env2 = if TYPEOF(generic_env) == SEXPTYPE::PROMSXP {
                eval_sexp(generic_env, R_BaseEnv())
            } else {
                generic_env
            };
            let s3 = find_var_in_frame(env2, nm_sym);
            if s3 != R_UnboundValue() {
                let s = allocSExp(SEXPTYPE::CLOSXP);
                SET_FORMALS(s, FORMALS(s3));
                SET_BODY(s, R_NilValue());
                SET_CLOENV(s, R_GlobalEnv());
                return s;
            }
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_formals, do_body, do_bodyCode
// ---------------------------------------------------------------------------

pub unsafe fn do_formals(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let mut input = CAR(args);
        if TYPEOF(input) == SEXPTYPE::PROMSXP {
            input = eval_sexp(input, _rho);
        }
        if TYPEOF(input) == SEXPTYPE::CLOSXP {
            let f = FORMALS(input);
            RAISE_NAMED(f, crate::sexp::accessors::NAMED(input));
            f
        } else {
            if TYPEOF(input) != SEXPTYPE::BUILTINSXP && TYPEOF(input) != SEXPTYPE::SPECIALSXP {
                warningcall(call, "argument is not a function");
            }
            R_NilValue()
        }
    }
}

pub unsafe fn do_body(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let input = CAR(args);
        if TYPEOF(input) == SEXPTYPE::CLOSXP {
            let b = BODY(input);
            RAISE_NAMED(b, crate::sexp::accessors::NAMED(input));
            b
        } else {
            if TYPEOF(input) != SEXPTYPE::BUILTINSXP && TYPEOF(input) != SEXPTYPE::SPECIALSXP {
                warningcall(call, "argument is not a function");
            }
            R_NilValue()
        }
    }
}

pub unsafe fn do_bodyCode(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let input = CAR(args);
        if TYPEOF(input) == SEXPTYPE::CLOSXP {
            let bc = BODY(input);
            RAISE_NAMED(bc, crate::sexp::accessors::NAMED(input));
            bc
        } else {
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// do_envir — environment()
// ---------------------------------------------------------------------------

pub unsafe fn do_envir(_call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let _ = rho;
        let input = CAR(args);
        if TYPEOF(input) == SEXPTYPE::CLOSXP {
            CLOENV(input)
        } else if isNull(input) {
            let ctxt = context_cloenv();
            if !ctxt.is_null() {
                ctxt
            } else {
                R_GlobalEnv()
            }
        } else if Rf_isEnvironment(input) != 0 {
            input
        } else {
            getAttrib(input, crate::sexp::symbol::install_str(".Environment"))
        }
    }
}

// ---------------------------------------------------------------------------
// do_envirgets — environment<-()
// ---------------------------------------------------------------------------

pub unsafe fn do_envirgets(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        // GNU: no check1arg. CAR is the object, CADR the replacement env.
        let s = CAR(args);
        let env = CADR(args);
        let env_is_null = env.is_null() || env == R_NilValue();
        let env_is_env = Rf_isEnvironment(env) != 0;
        if TYPEOF(s) == SEXPTYPE::CLOSXP && (env_is_env || env_is_null) {
            if env_is_null {
                errorcall(call, "use of NULL environment is defunct");
            }
            let _s_guard = crate::sexp::protect::protect(s);
            let dup = shallow_duplicate(s);
            if dup.is_null() || dup == R_NilValue() {
                return s;
            }
            SET_CLOENV(dup, env);
            return dup;
        } else if env_is_null || env_is_env {
            if TYPEOF(s) == SEXPTYPE::BUILTINSXP || TYPEOF(s) == SEXPTYPE::SPECIALSXP {
                if !env_is_null {
                    warningcall(
                        call,
                        "setting environment(<primitive function>) is not possible and trying it is deprecated",
                    );
                }
            } else {
                setAttrib(s, crate::sexp::symbol::install_str(".Environment"), env);
            }
        } else {
            errorcall(call, "replacement object is not an environment");
        }
        s
    }
}

// ---------------------------------------------------------------------------
// do_newenv — new.env()
// ---------------------------------------------------------------------------

pub unsafe fn do_newenv(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let mut hash = FALSE;
        let mut parent = R_GlobalEnv();
        let mut size: c_int = 29;
        if !isNull(CAR(args)) {
            hash = asLogical(CAR(args));
            if hash == NA_INTEGER {
                errorcall(call, "invalid 'hash' argument");
            }
        }
        if !isNull(CADR(args)) {
            parent = CADR(args);
            if Rf_isEnvironment(parent) == 0 {
                errorcall(call, "invalid 'parent' argument");
            }
        }
        if !isNull(CADDR(args)) {
            size = asInteger(CADDR(args));
            if size == NA_INTEGER {
                size = 29;
            }
        }
        if hash != 0 {
            // SAFETY: creates an environment under the active runtime on live
            // parent and size inputs.
            unsafe { crate::sexp::envir::R_NewHashedEnv(parent, size) }
        } else {
            // SAFETY: creates an environment under the active runtime on a
            // live parent.
            unsafe { crate::sexp::envir::Rf_createEnv(R_NilValue(), parent) }
        }
    }
}

// ---------------------------------------------------------------------------
// do_parentenv, do_parentenvgets, do_envirName
// ---------------------------------------------------------------------------

pub unsafe fn do_parentenv(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let env = CAR(args);
        if isNull(env) {
            errorcall(call, "cannot get the parent of the empty environment");
        }
        if Rf_isEnvironment(env) == 0 {
            errorcall(call, "invalid argument");
        }
        ENCLOS(env)
    }
}

pub unsafe fn do_parentenvgets(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let env = CAR(args);
        let val = CADR(args);
        if isNull(env) {
            errorcall(call, "cannot set parent of empty environment");
        }
        if Rf_isEnvironment(val) == 0 {
            errorcall(call, "invalid 'parent' argument");
        }
        SET_ENCLOS(env, val);
        R_NilValue()
    }
}

pub unsafe fn do_envirName(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let env = CAR(args);
        if isNull(env) {
            return mk_string("R_EmptyEnv");
        }
        if env == R_GlobalEnv() {
            return mk_string("R_GlobalEnv");
        }
        if env == R_BaseEnv() {
            return mk_string("base");
        }
        let name = getAttrib(env, crate::sexp::symbol::install_str("name"));
        if Rf_isString(name) != 0 && LENGTH(name) > 0 {
            return name;
        }
        mk_string("")
    }
}

// ---------------------------------------------------------------------------
// do_cat — cat() (simplified for embedded use)
// ---------------------------------------------------------------------------

pub unsafe fn do_cat(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        // In embedded/headless mode, cat() is a no-op
        // Full implementation would need the connections subsystem
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_makelist — list()
// ---------------------------------------------------------------------------

pub unsafe fn do_makelist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let n = Rf_length(args);
        let s = Rf_allocVector(SEXPTYPE::VECSXP, n);
        let mut a = args;
        let mut i: R_xlen_t = 0;
        while !isNull(a) {
            SET_VECTOR_ELT(s, i, CAR(a));
            a = CDR(a);
            i += 1;
        }
        let mut has_names = false;
        let mut a = args;
        while !isNull(a) {
            if !isNull(TAG(a)) {
                has_names = true;
                break;
            }
            a = CDR(a);
        }
        if has_names {
            let names = Rf_allocVector(SEXPTYPE::STRSXP, n);
            let mut a = args;
            let mut i: R_xlen_t = 0;
            while !isNull(a) {
                let tag = TAG(a);
                if !isNull(tag) {
                    SET_STRING_ELT(names, i, PRINTNAME(tag));
                } else {
                    SET_STRING_ELT(names, i, STRING_ELT(mk_string(""), 0));
                }
                a = CDR(a);
                i += 1;
            }
            setAttrib(s, crate::sexp::symbol::install_str("names"), names);
        }
        s
    }
}

// ---------------------------------------------------------------------------
// do_expression — expression()
// ---------------------------------------------------------------------------

pub unsafe fn do_expression(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let n = Rf_length(args);
        let s = Rf_allocVector(SEXPTYPE::EXPRSXP, n);
        let mut a = args;
        let mut i: R_xlen_t = 0;
        while !isNull(a) {
            SET_VECTOR_ELT(s, i, duplicate_sexp(CAR(a)));
            a = CDR(a);
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// do_makevector — vector()
// ---------------------------------------------------------------------------

pub unsafe fn do_makevector(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let mode = CAR(args);
        let len_arg = CADR(args);
        let mode_bytes = if Rf_isString(mode) != 0 && LENGTH(mode) > 0 {
            charsxp_bytes(STRING_ELT(mode, 0))
        } else {
            b"logical".to_vec()
        };
        let mode_str = mode_bytes.as_slice();
        let len = asVecSize(len_arg);
        if len == -999 {
            error("invalid 'length' argument");
        }
        if len < 0 {
            error("negative length vectors are not allowed");
        }
        let stype = match mode_str {
            b"logical" => SEXPTYPE::LGLSXP.into(),
            b"integer" => SEXPTYPE::INTSXP.into(),
            b"numeric" | b"double" => SEXPTYPE::REALSXP.into(),
            b"complex" => SEXPTYPE::CPLXSXP.into(),
            b"character" => SEXPTYPE::STRSXP.into(),
            b"raw" => SEXPTYPE::RAWSXP.into(),
            b"list" => SEXPTYPE::VECSXP.into(),
            b"expression" => SEXPTYPE::EXPRSXP.into(),
            _ => {
                error("invalid 'mode' argument");
                0
            }
        };
        let s = Rf_allocVector(stype, len as c_int);
        if stype == SEXPTYPE::REALSXP {
            let mut sm = mutable_vector(s);
            for i in 0..len {
                sm.try_set_real_elt(i, 0.0)
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            }
        } else if stype == SEXPTYPE::INTSXP {
            let mut sm = mutable_vector(s);
            for i in 0..len {
                sm.try_set_integer_elt(i, 0)
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            }
        } else if stype == SEXPTYPE::LGLSXP {
            let mut sm = mutable_vector(s);
            for i in 0..len {
                sm.try_set_logical_elt(i, 0)
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------
// xlengthgets / lengthgets / do_lengthgets — length<-()
// ---------------------------------------------------------------------------

pub unsafe fn xlengthgets(x: SEXP, len: R_xlen_t) -> SEXP {
    {
        if len == XLENGTH(x) {
            return x;
        }
        let xtype = TYPEOF(x);
        if xtype == SEXPTYPE::NILSXP {
            error("cannot set length of NULL");
        }
        if xtype == SEXPTYPE::LISTSXP || xtype == SEXPTYPE::LANGSXP {
            if len <= 0 {
                return R_NilValue();
            }
            let mut src = x;
            let mut out = R_NilValue();
            let mut tail = R_NilValue();
            let mut i = 0i64;
            while i < len {
                let (car, tag, nxt) = if src.is_null() || src == R_NilValue() {
                    (R_NilValue(), R_NilValue(), R_NilValue())
                } else {
                    (CAR(src), TAG(src), CDR(src))
                };
                let cell = crate::sexp::constructors::Rf_cons(car, R_NilValue());
                crate::sexp::accessors::SETTAG(cell, tag);
                if out.is_null() || out == R_NilValue() {
                    out = cell;
                } else {
                    SETCDR(tail, cell);
                }
                tail = cell;
                src = nxt;
                i += 1;
            }
            return out;
        }
        let r = Rf_allocVector(xtype, len as c_int);
        let old_len = XLENGTH(x);
        let copy_len = (if len < old_len { len } else { old_len }) as usize;
        let xs = wrapped(x);
        let mut rm = mutable_vector(r);

        match xtype {
            t if t == SEXPTYPE::INTSXP => {
                for i in 0..copy_len {
                    let v = xs
                        .try_integer_elt(i as R_xlen_t)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    rm.try_set_integer_elt(i as R_xlen_t, v)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
                for i in copy_len..len as usize {
                    rm.try_set_integer_elt(i as R_xlen_t, NA_INTEGER)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
            }
            t if t == SEXPTYPE::LGLSXP => {
                for i in 0..copy_len {
                    let v = xs
                        .try_logical_elt(i as R_xlen_t)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    rm.try_set_logical_elt(i as R_xlen_t, v)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
                for i in copy_len..len as usize {
                    rm.try_set_logical_elt(i as R_xlen_t, NA_INTEGER)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..copy_len {
                    let v = xs
                        .try_real_elt(i as R_xlen_t)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    rm.try_set_real_elt(i as R_xlen_t, v)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
                for i in copy_len..len as usize {
                    rm.try_set_real_elt(i as R_xlen_t, NA_REAL)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
                for i in 0..copy_len {
                    let v = xs
                        .try_complex_elt(i as R_xlen_t)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    rm.try_set_complex_elt(i as R_xlen_t, v)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
                for i in copy_len..len as usize {
                    rm.try_set_complex_elt(
                        i as R_xlen_t,
                        crate::sexp::ffi::Rcomplex {
                            r: NA_REAL,
                            i: NA_REAL,
                        },
                    )
                    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
            }
            t if t == SEXPTYPE::STRSXP => {
                for i in 0..copy_len as R_xlen_t {
                    let v = xs
                        .try_string_elt(i)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    rm.try_set_string_elt(i, v)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
                for i in copy_len as R_xlen_t..len {
                    let v = wrapped(crate::sexp::globals::R_NaString());
                    rm.try_set_string_elt(i, v)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
            }
            t if t == SEXPTYPE::RAWSXP => {
                for i in 0..copy_len {
                    let v = xs
                        .try_raw_elt(i as R_xlen_t)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    rm.try_set_raw_elt(i as R_xlen_t, v)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                for i in 0..copy_len as R_xlen_t {
                    let v = xs
                        .try_vector_elt(i)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                    rm.try_set_vector_elt(i, v)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
                for i in copy_len as R_xlen_t..len {
                    let v = wrapped(R_NilValue());
                    rm.try_set_vector_elt(i, v)
                        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
                }
            }
            _ => {
                error("unsupported type for length assignment");
            }
        }
        let names = getAttrib(x, crate::sexp::symbol::install_str("names"));
        if !isNull(names) {
            setAttrib(r, crate::sexp::symbol::install_str("names"), names);
        }
        r
    }
}

pub unsafe fn lengthgets(x: SEXP, len: c_int) -> SEXP {
    // SAFETY: delegates to the checked-length setter on a live vector.
    unsafe { xlengthgets(x, len as R_xlen_t) }
}

pub unsafe fn do_lengthgets(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        check_arity(op, args);
        let x = CAR(args);
        // GNU builtin.c: DispatchOrEval internal generic: length<-
        if crate::sexp::accessors::OBJECT(x) != 0 {
            let mut ans = R_NilValue();
            // SAFETY: dispatch evaluates live arguments under the active
            // runtime, matching the surrounding evaluator contract.
            if unsafe {
                crate::eval::dispatch::DispatchOrEval(
                    call,
                    op,
                    b"length<-\0".as_ptr() as *const c_char,
                    args,
                    rho,
                    &mut ans,
                    0,
                    1,
                )
            } != 0
            {
                return ans;
            }
        }
        let len = CADR(args);
        let len_val = asVecSize(len);
        if len_val == -999 {
            error("invalid 'length' argument");
        }
        // SAFETY: resizes a live vector under the active runtime.
        unsafe { xlengthgets(x, len_val) }
    }
}

// ---------------------------------------------------------------------------
// do_switch — switch() (SPECIALSXP)
// ---------------------------------------------------------------------------

pub unsafe fn do_switch(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        let nargs = Rf_length(args);
        if nargs < 1 {
            errorcall(_call, "'EXPR' is missing");
        }

        let arg = eval_sexp(CAR(args), rho);
        if Rf_length(arg) != 1 {
            errorcall(_call, "EXPR must be a length 1 vector");
        }

        // Stock warns (and invisibly returns NULL) when there are no
        // alternatives beyond EXPR.
        if nargs == 1 {
            crate::mainutils::errors::warningcall(
                _call,
                b"'switch' with no alternatives\0".as_ptr() as *const c_char,
            );
            crate::eval::runtime::set_visible(FALSE);
            return R_NilValue();
        }

        let mut alternatives = CDR(args);
        if !isNull(alternatives)
            && CAR(alternatives) == crate::sexp::symbol::R_DotsSymbol()
            && isNull(CDR(alternatives))
        {
            let dots = find_var_in_frame(rho, crate::sexp::symbol::R_DotsSymbol());
            if TYPEOF(dots) == SEXPTYPE::DOTSXP {
                alternatives = dots;
            }
        }
        let mut dflt: SEXP = std::ptr::null_mut();

        if TYPEOF(arg) == SEXPTYPE::STRSXP {
            let target = STRING_ELT(arg, 0);
            let target_bytes = if target.is_null() || target == crate::sexp::globals::R_NaString() {
                None
            } else {
                Some(charsxp_bytes(target))
            };

            let mut current = alternatives;
            while !isNull(current) {
                let tag = TAG(current);
                if !isNull(tag) {
                    if let Some(target_bytes) = &target_bytes {
                        let name_bytes = charsxp_bytes(PRINTNAME(tag));
                        if name_bytes == *target_bytes {
                            while !isNull(current) && CAR(current) == R_MissingArg() {
                                current = CDR(current);
                                if !isNull(current) && isNull(TAG(current)) {
                                    dflt = switch_default(CAR(current), dflt);
                                }
                            }
                            if isNull(current) {
                                crate::eval::runtime::set_visible(FALSE);
                                return R_NilValue();
                            }

                            let mut rest = CDR(current);
                            while !isNull(rest) {
                                if isNull(TAG(rest)) {
                                    dflt = switch_default(CAR(rest), dflt);
                                }
                                rest = CDR(rest);
                            }

                            return eval_sexp(CAR(current), rho);
                        }
                    }
                } else {
                    dflt = switch_default(CAR(current), dflt);
                }
                current = CDR(current);
            }

            if !dflt.is_null() {
                return eval_sexp(dflt, rho);
            }
            crate::eval::runtime::set_visible(FALSE);
            R_NilValue()
        } else {
            let n = asInteger(arg);
            let alt_len = Rf_length(alternatives);
            if n == NA_INTEGER || n < 1 || n > alt_len {
                crate::eval::runtime::set_visible(FALSE);
                return R_NilValue();
            }

            let mut idx = 0;
            let mut b = alternatives;
            while !isNull(b) {
                idx += 1;
                if idx == n {
                    let alt = CAR(b);
                    if alt == R_MissingArg() {
                        error("empty alternative in numeric switch");
                    }
                    return eval_sexp(alt, rho);
                }
                b = CDR(b);
            }
            crate::eval::runtime::set_visible(FALSE);
            R_NilValue()
        }
    }
}

fn switch_default(value: SEXP, current: SEXP) -> SEXP {
    {
        if !current.is_null() {
            error("duplicate switch defaults");
        }
        value
    }
}
