#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::{c_char, c_int};

use crate::mainutils::coerce::{ISNAN, R_FINITE, asInteger, asLogical, asReal};
use crate::mainutils::duplicate::duplicate;
use crate::mainutils::relop::{PRIMNAME, checkArity};
use crate::sexp::accessors::{
    BODY, CADDR, CADR, CAR, CDR, CHAR, CLOENV, COMPLEX, ENCLOS, FORMALS, INTEGER, LENGTH,
    PRINTNAME, RAW, REAL, SET_BODY, SET_CLOENV, SET_ENCLOS, SET_FORMALS, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::attrib_core::{getAttrib, setAttrib};
use crate::sexp::constructors::{
    Rf_allocVector, Rf_isEnvironment, Rf_isString, Rf_length, Rf_mkString,
};
use crate::sexp::context::R_GlobalContext;
use crate::sexp::envir::{R_findVarInFrame, defineVar, findFun};
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_BaseEnv, R_GlobalEnv, R_MissingArg, R_NilValue, R_UnboundValue};
use crate::sexp::memory_ext::{allocSExp, mkPROMISE};
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Local error helpers (same pattern as coerce.rs, objects.rs, etc.)
// ---------------------------------------------------------------------------

unsafe fn error(msg: &str) {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

unsafe fn errorcall(_call: SEXP, msg: &str) {
    unsafe {
        error(msg);
    }
}

unsafe fn warningcall(_call: SEXP, msg: &str) {
    // In embedded mode, warnings are logged but not fatal
    let _ = msg;
}

unsafe fn isNull(x: SEXP) -> bool {
    unsafe { crate::sexp::accessors::Rf_isNull(x) != 0 }
}

unsafe fn installTrChar(x: SEXP) -> SEXP {
    unsafe {
        let p = CHAR(x);
        if p.is_null() {
            error("invalid string in installTrChar");
        }
        Rf_install(p)
    }
}

unsafe fn first_string_char(x: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::CHARSXP {
            x
        } else {
            STRING_ELT(x, 0)
        }
    }
}

unsafe fn listAppend(s: SEXP, t: SEXP) -> SEXP {
    unsafe {
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
}

unsafe fn RAISE_NAMED(x: SEXP, v: c_int) {
    unsafe {
        let n = crate::sexp::accessors::NAMED(x);
        if n < v {
            crate::sexp::accessors::SET_NAMED(x, v);
        }
    }
}

fn na_str() -> *const c_char {
    b"NA\0".as_ptr() as *const c_char as *const c_char
}

// ---------------------------------------------------------------------------
// asVecSize — convert SEXP to vector length
// ---------------------------------------------------------------------------

pub unsafe fn asVecSize(x: SEXP) -> R_xlen_t {
    unsafe {
        if crate::sexp::object::raw_is_atomic_vector(x) && LENGTH(x) >= 1 {
            match TYPEOF(x) {
                t if t == SEXPTYPE::INTSXP => {
                    let res = *INTEGER(x);
                    if res == NA_INTEGER {
                        error("vector size cannot be NA");
                    }
                    return res as R_xlen_t;
                }
                t if t == SEXPTYPE::REALSXP => {
                    let d = *REAL(x);
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
    unsafe {
        checkArity(op, args);
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
        defineVar(name, mkPROMISE(expr, eenv), aenv);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_makelazy — makeLazy()
// ---------------------------------------------------------------------------

pub unsafe fn do_makelazy(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
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
            let val = crate::eval::eval::Rf_eval(VECTOR_ELT(values, i), eenv);
            let expr0 = duplicate(expr);
            SETCAR(CDR(expr0), val);
            defineVar(name, mkPROMISE(expr0, eenv), aenv);
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_onexit — on.exit()
// ---------------------------------------------------------------------------

pub unsafe fn do_onexit(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op);
        crate::eval::special::do_on_exit_from_args(args, rho)
    }
}

// ---------------------------------------------------------------------------
// do_args — args()
// ---------------------------------------------------------------------------

pub unsafe fn do_args(_call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let mut input = CAR(args);
        if TYPEOF(input) == SEXPTYPE::STRSXP && LENGTH(input) == 1 {
            let s = installTrChar(first_string_char(input));
            input = findFun(s, rho);
        }
        if TYPEOF(input) == SEXPTYPE::CLOSXP {
            let s = allocSExp(SEXPTYPE::CLOSXP);
            SET_FORMALS(s, FORMALS(input));
            SET_BODY(s, R_NilValue());
            SET_CLOENV(s, R_GlobalEnv());
            return s;
        }
        if TYPEOF(input) == SEXPTYPE::BUILTINSXP || TYPEOF(input) == SEXPTYPE::SPECIALSXP {
            let nm = PRIMNAME(input);
            let args_env = R_findVarInFrame(
                R_BaseEnv(),
                Rf_install(b".ArgsEnv\0".as_ptr() as *const c_char),
            );
            let env = if TYPEOF(args_env) == SEXPTYPE::PROMSXP {
                crate::eval::eval::Rf_eval(args_env, R_BaseEnv())
            } else {
                args_env
            };
            let nm_sym = Rf_install(nm);
            let s2 = R_findVarInFrame(env, nm_sym);
            if s2 != R_UnboundValue() {
                let s = duplicate(s2);
                SET_BODY(s, R_NilValue());
                SET_CLOENV(s, R_GlobalEnv());
                return s;
            }
            let generic_env = R_findVarInFrame(
                R_BaseEnv(),
                Rf_install(b".GenericArgsEnv\0".as_ptr() as *const c_char),
            );
            let env2 = if TYPEOF(generic_env) == SEXPTYPE::PROMSXP {
                crate::eval::eval::Rf_eval(generic_env, R_BaseEnv())
            } else {
                generic_env
            };
            let s3 = R_findVarInFrame(env2, nm_sym);
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
    unsafe {
        checkArity(op, args);
        let input = CAR(args);
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
    unsafe {
        checkArity(op, args);
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
    unsafe {
        checkArity(op, args);
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
    unsafe {
        checkArity(op, args);
        let input = CAR(args);
        if TYPEOF(input) == SEXPTYPE::CLOSXP {
            CLOENV(input)
        } else if isNull(input) {
            let ctxt = R_GlobalContext();
            if !ctxt.is_null() {
                (*ctxt).cloenv
            } else {
                R_GlobalEnv()
            }
        } else if Rf_isEnvironment(input) != 0 {
            input
        } else {
            getAttrib(
                input,
                Rf_install(b".Environment\0".as_ptr() as *const c_char),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// do_envirgets — environment<-()
// ---------------------------------------------------------------------------

pub unsafe fn do_envirgets(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let first = CAR(args);
        let second = CADR(args);
        let (x, val) = if Rf_isEnvironment(second) != 0 {
            (first, second)
        } else {
            (second, first)
        };
        if TYPEOF(x) == SEXPTYPE::CLOSXP {
            if Rf_isEnvironment(val) == 0 {
                errorcall(call, "invalid replacement for 'environment'");
            }
            SET_CLOENV(x, val);
            return x;
        }
        if Rf_isEnvironment(val) != 0 {
            setAttrib(
                x,
                Rf_install(b".Environment\0".as_ptr() as *const c_char),
                val,
            );
        } else {
            errorcall(call, "invalid replacement for 'environment'");
        }
        x
    }
}

// ---------------------------------------------------------------------------
// do_newenv — new.env()
// ---------------------------------------------------------------------------

pub unsafe fn do_newenv(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
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
            crate::sexp::envir::R_NewHashedEnv(parent, size)
        } else {
            crate::sexp::envir::Rf_createEnv(R_NilValue(), parent)
        }
    }
}

// ---------------------------------------------------------------------------
// do_parentenv, do_parentenvgets, do_envirName
// ---------------------------------------------------------------------------

pub unsafe fn do_parentenv(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
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
    unsafe {
        checkArity(op, args);
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
    unsafe {
        checkArity(op, args);
        let env = CAR(args);
        if isNull(env) {
            return Rf_mkString(b"R_EmptyEnv\0".as_ptr() as *const c_char);
        }
        if env == R_GlobalEnv() {
            return Rf_mkString(b"R_GlobalEnv\0".as_ptr() as *const c_char);
        }
        if env == R_BaseEnv() {
            return Rf_mkString(b"base\0".as_ptr() as *const c_char);
        }
        let name = getAttrib(env, Rf_install(b"name\0".as_ptr() as *const c_char));
        if Rf_isString(name) != 0 && LENGTH(name) > 0 {
            return name;
        }
        Rf_mkString(b"\0".as_ptr() as *const c_char)
    }
}

// ---------------------------------------------------------------------------
// do_cat — cat() (simplified for embedded use)
// ---------------------------------------------------------------------------

pub unsafe fn do_cat(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        // In embedded/headless mode, cat() is a no-op
        // Full implementation would need the connections subsystem
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_makelist — list()
// ---------------------------------------------------------------------------

pub unsafe fn do_makelist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
                    SET_STRING_ELT(names, i, Rf_mkString(b"\0".as_ptr() as *const c_char));
                }
                a = CDR(a);
                i += 1;
            }
            setAttrib(s, Rf_install(b"names\0".as_ptr() as *const c_char), names);
        }
        s
    }
}

// ---------------------------------------------------------------------------
// do_expression — expression()
// ---------------------------------------------------------------------------

pub unsafe fn do_expression(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = Rf_length(args);
        let s = Rf_allocVector(SEXPTYPE::EXPRSXP, n);
        let mut a = args;
        let mut i: R_xlen_t = 0;
        while !isNull(a) {
            SET_VECTOR_ELT(s, i, duplicate(CAR(a)));
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
    unsafe {
        checkArity(op, args);
        let mode = CAR(args);
        let len_arg = CADR(args);
        let mode_str = if Rf_isString(mode) != 0 && LENGTH(mode) > 0 {
            let s = CHAR(STRING_ELT(mode, 0));
            std::ffi::CStr::from_ptr(s).to_bytes()
        } else {
            b"logical"
        };
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
            let p = REAL(s);
            for i in 0..len as usize {
                *p.add(i) = 0.0;
            }
        } else if stype == SEXPTYPE::INTSXP || stype == SEXPTYPE::LGLSXP {
            let p = INTEGER(s);
            for i in 0..len as usize {
                *p.add(i) = 0;
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------
// xlengthgets / lengthgets / do_lengthgets — length<-()
// ---------------------------------------------------------------------------

pub unsafe fn xlengthgets(x: SEXP, len: R_xlen_t) -> SEXP {
    unsafe {
        if len == XLENGTH(x) {
            return x;
        }
        let xtype = TYPEOF(x);
        if xtype == SEXPTYPE::NILSXP {
            error("cannot set length of NULL");
        }
        let r = Rf_allocVector(xtype, len as c_int);
        let old_len = XLENGTH(x);
        let copy_len = (if len < old_len { len } else { old_len }) as usize;

        match xtype {
            t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {
                let px = INTEGER(x);
                let pr = INTEGER(r);
                for i in 0..copy_len {
                    *pr.add(i) = *px.add(i);
                }
                for i in copy_len..len as usize {
                    *pr.add(i) = NA_INTEGER;
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let px = REAL(x);
                let pr = REAL(r);
                for i in 0..copy_len {
                    *pr.add(i) = *px.add(i);
                }
                for i in copy_len..len as usize {
                    *pr.add(i) = NA_REAL;
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
                let px = COMPLEX(x);
                let pr = COMPLEX(r);
                for i in 0..copy_len {
                    *pr.add(i) = *px.add(i);
                }
            }
            t if t == SEXPTYPE::STRSXP => {
                for i in 0..copy_len as R_xlen_t {
                    SET_STRING_ELT(r, i, STRING_ELT(x, i));
                }
            }
            t if t == SEXPTYPE::RAWSXP => {
                let px = RAW(x);
                let pr = RAW(r);
                for i in 0..copy_len {
                    *pr.add(i) = *px.add(i);
                }
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                for i in 0..copy_len as R_xlen_t {
                    SET_VECTOR_ELT(r, i, VECTOR_ELT(x, i));
                }
            }
            _ => {
                error("unsupported type for length assignment");
            }
        }
        let names = getAttrib(x, Rf_install(b"names\0".as_ptr() as *const c_char));
        if !isNull(names) {
            setAttrib(r, Rf_install(b"names\0".as_ptr() as *const c_char), names);
        }
        r
    }
}

pub unsafe fn lengthgets(x: SEXP, len: c_int) -> SEXP {
    unsafe { xlengthgets(x, len as R_xlen_t) }
}

pub unsafe fn do_lengthgets(_call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let x = CAR(args);
        let len = CADR(args);
        let len_val = asVecSize(len);
        if len_val == -999 {
            error("invalid 'length' argument");
        }
        xlengthgets(x, len_val)
    }
}

// ---------------------------------------------------------------------------
// do_switch — switch() (SPECIALSXP)
// ---------------------------------------------------------------------------

pub unsafe fn do_switch(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let nargs = Rf_length(args);
        if nargs < 1 {
            errorcall(_call, "'EXPR' is missing");
        }

        let arg = crate::eval::eval::Rf_eval(CAR(args), rho);
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

        let alternatives = CDR(args);
        let mut dflt: SEXP = std::ptr::null_mut();

        if TYPEOF(arg) == SEXPTYPE::STRSXP {
            let target = STRING_ELT(arg, 0);
            let target_bytes = if target.is_null() {
                None
            } else {
                Some(std::ffi::CStr::from_ptr(CHAR(target)).to_bytes())
            };

            let mut current = alternatives;
            while !isNull(current) {
                let tag = TAG(current);
                if !isNull(tag) {
                    if let Some(target_bytes) = target_bytes {
                        let name = CHAR(PRINTNAME(tag));
                        let name_bytes = std::ffi::CStr::from_ptr(name).to_bytes();
                        if name_bytes == target_bytes {
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

                            return crate::eval::eval::Rf_eval(CAR(current), rho);
                        }
                    }
                } else {
                    dflt = switch_default(CAR(current), dflt);
                }
                current = CDR(current);
            }

            if !dflt.is_null() {
                return crate::eval::eval::Rf_eval(dflt, rho);
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
                    return crate::eval::eval::Rf_eval(alt, rho);
                }
                b = CDR(b);
            }
            crate::eval::runtime::set_visible(FALSE);
            R_NilValue()
        }
    }
}

unsafe fn switch_default(value: SEXP, current: SEXP) -> SEXP {
    unsafe {
        if !current.is_null() {
            error("duplicate switch defaults");
        }
        value
    }
}
