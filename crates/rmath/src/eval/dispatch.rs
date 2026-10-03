#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Argument evaluation and dispatch — ports parts of eval.c.
//!
//! Handles:
//! - evalList: evaluate argument lists
//! - promiseArgs: create promises for closure arguments
//! - forcePromise: force evaluation of promises
//! - DispatchOrEval: S3/S4 method dispatch
//! - DispatchGroup: group generic dispatch (Math, Summary, Ops, Complex)
//! - findmethod: find S3 method in class hierarchy

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::eval::attrib_core::{R_ClassSymbol, getAttrib, isObject};
use crate::sexp::accessors::{
    CADR, CAR, CDR, CHAR, LENGTH, PRINTNAME, SET_STRING_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT,
    TAG, TYPEOF,
};
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::envir::{R_findVar, R_findVarInFrame, R_isMissing, forcePromise};
use crate::sexp::ffi::{FALSE, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::memory_ext::{CONS_NR, NewEnvironment, vmaxget, vmaxset};
use crate::sexp::object::{PairlistBuilder, SessionNodeFactory, Sexp};
use crate::sexp::protect::protect;
use crate::sexp::symbol::{R_DotsSymbol, Rf_install};

use super::builtin::PRIMNAME;
use super::eval::Rf_eval;

/// Resolve an Ops conflict in a temporary frame, as GNU R does. Passing
/// symbols bound to the evaluated values preserves the hook's promise
/// expressions (`x`, `y`, ..., `rev`) and its caller environment.
unsafe fn choose_ops_method(
    x: SEXP,
    y: SEXP,
    mx: SEXP,
    my: SEXP,
    call: SEXP,
    reverse: bool,
    rho: SEXP,
) -> bool {
    unsafe {
        let newrho = NewEnvironment(R_NilValue(), rho, R_NilValue());
        let _rho_guard = protect(newrho);
        let reverse_value = Rf_ScalarLogical(if reverse { TRUE } else { FALSE });
        let _reverse_guard = protect(reverse_value);
        let mut actuals = PairlistBuilder::new();
        for (name, value) in [
            (c"x", x),
            (c"y", y),
            (c"mx", mx),
            (c"my", my),
            (c"cl", call),
            (c"rev", reverse_value),
        ] {
            let symbol = Rf_install(name.as_ptr());
            crate::sexp::envir::defineVar(symbol, value, newrho);
            let named = crate::sexp::accessors::NAMED(value);
            if named < 2 {
                crate::sexp::accessors::SET_NAMED(value, named + 1);
            }
            actuals
                .push_cell(
                    actuals
                        .wrap(symbol)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string())),
                    None,
                )
                .unwrap_or_else(|_| {
                    crate::sexp::context::r_error("failed to allocate chooseOpsMethod arguments")
                });
        }
        let actuals_owner = actuals.finish().unwrap_or_else(|_| {
            crate::sexp::context::r_error("failed to allocate chooseOpsMethod arguments")
        });
        let actuals = actuals_owner.as_raw();
        let head = Rf_lang3(
            Rf_install(c"::".as_ptr()),
            Rf_install(c"base".as_ptr()),
            Rf_install(c"chooseOpsMethod".as_ptr()),
        );
        let _head_guard = protect(head);
        let expression = Rf_lang2(head, R_NilValue());
        SETCDR(expression, actuals);
        let _expression_guard = protect(expression);
        let fun = R_findVar(
            Rf_install(c"chooseOpsMethod".as_ptr()),
            super::runtime::base_env(),
        );
        let result = crate::eval::closure::applyClosure(
            expression,
            fun,
            actuals,
            newrho,
            R_NilValue(),
            TRUE,
        );
        let _result_guard = protect(result);
        result != R_NilValue() && crate::mainutils::coerce::asRbool(result, call) != FALSE
    }
}

unsafe fn method_name_is(method: SEXP, name: &[u8]) -> bool {
    unsafe {
        if method.is_null() || TYPEOF(method) != SEXPTYPE::SYMSXP {
            return false;
        }
        let printed = PRINTNAME(method);
        !printed.is_null()
            && !CHAR(printed).is_null()
            && std::ffi::CStr::from_ptr(CHAR(printed)).to_bytes() == name
    }
}

/// Capture the active owner once at a translated evaluator entry.
/// # Safety
/// The caller retains this owner for `'s` and excludes overlapping payload loans.
pub(super) unsafe fn active_argument_factory<'s>() -> SessionNodeFactory<'s> {
    SessionNodeFactory::new(
        unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
    )
}

/// Normalize a raw entry argument into the captured allocation domain.
pub(super) fn argument_value<'s>(factory: &SessionNodeFactory<'s>, value: SEXP) -> Sexp<'s> {
    if value.is_null() {
        factory.nil()
    } else {
        factory
            .wrap(value)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    }
}

// ---------------------------------------------------------------------------
// evalList — evaluate each element of a pairlist
// ---------------------------------------------------------------------------

/// Evaluate rooted arguments while retaining earlier results across callbacks.
/// The returned pairlist owns its automatic root; callers borrow raw projections
/// only while keeping this handle alive.
pub fn evalList<'a>(el: Sexp<'a>, rho: Sexp<'a>, call: Option<Sexp<'a>>, nargs: c_int) -> Sexp<'a> {
    let factory = rho
        .node_factory()
        .or_else(|_| el.node_factory())
        .or_else(|error| call.as_ref().map_or(Err(error), Sexp::node_factory))
        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
    factory
        .require_active()
        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
    let el = factory
        .wrap(el.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
    let call = call.map(|call| {
        factory
            .wrap(call.as_raw())
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()))
    });
    let mut result = PairlistBuilder::from_factory(factory.clone());
    let mut bumped = NamedArguments { values: Vec::new() };
    let mut current = el;
    let mut count: c_int = 0;
    while !current.is_nil() {
        if nargs >= 0 && count >= nargs {
            break;
        }
        let expr = current
            .try_car()
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        if expr.as_raw() == unsafe { R_DotsSymbol() } {
            // Lookup may force active bindings; acquire its root before any
            // later promise evaluation or list-cell allocation.
            let h = factory
                .wrap(unsafe { R_findVar(expr.as_raw(), rho.as_raw()) })
                .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
            if h.typeof_() == SEXPTYPE::DOTSXP || h.is_nil() {
                let mut dh = h;
                while !dh.is_nil() {
                    if nargs >= 0 && count >= nargs {
                        break;
                    }
                    let expr = dh
                        .try_car()
                        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
                    let value = factory
                        .wrap(unsafe { Rf_eval(expr.as_raw(), rho.as_raw()) })
                        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
                    bumped.retain(&value);
                    result
                        .push(value, dh.tag())
                        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
                    dh = dh
                        .try_cdr()
                        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
                    count += 1;
                }
            } else if h.as_raw() != unsafe { R_MissingArg() } {
                crate::sexp::context::r_error("'...' used in an incorrect context");
            }
        } else if expr.as_raw() == unsafe { R_MissingArg() } {
            let head = call.as_ref().and_then(Sexp::car);
            let name = head
                .filter(|head| head.typeof_() == SEXPTYPE::SYMSXP)
                .and_then(|head| head.printname())
                .and_then(|name| name.try_as_string().ok());
            if name.as_deref().is_some_and(|name| {
                matches!(
                    name,
                    "colMeans" | "colSums" | "rowMeans" | "rowSums" | "dput" | "round" | "signif"
                )
            }) {
                result
                    .push(expr, current.tag())
                    .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
            } else {
                crate::sexp::context::r_error(&format!("argument {} is empty", count + 1));
            }
        } else {
            let value = factory
                .wrap(unsafe { Rf_eval(expr.as_raw(), rho.as_raw()) })
                .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
            bumped.retain(&value);
            result
                .push(value, current.tag())
                .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        }
        current = current
            .try_cdr()
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        count += 1;
    }
    result
        .finish()
        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()))
}

/// NAMED link accounting is independent of root ownership. Restore the
/// temporary sharing metadata on both normal return and an R error unwind.
pub(super) struct NamedArguments<'a> {
    values: Vec<Sexp<'a>>,
}
impl<'a> NamedArguments<'a> {
    pub(super) fn new() -> Self {
        Self { values: Vec::new() }
    }

    pub(super) fn retain(&mut self, value: &Sexp<'a>) {
        unsafe {
            bump_named_link(value.as_raw());
        }
        self.values.push(value.clone());
    }
}
impl Drop for NamedArguments<'_> {
    fn drop(&mut self) {
        for value in &self.values {
            unsafe {
                drop_named_link(value.as_raw());
            }
        }
    }
}

/// GNU `INCREMENT_LINKS`: a later argument can still see this value as shared.
/// 3 is sticky so a value that was already `NAMEDMAX` is not unshared later.
unsafe fn bump_named_link(val: SEXP) {
    unsafe {
        if val.is_null() {
            return;
        }
        let n = crate::sexp::accessors::NAMED(val);
        if n < 3 {
            crate::sexp::accessors::SET_NAMED(val, n + 1);
        }
    }
}

unsafe fn drop_named_link(val: SEXP) {
    unsafe {
        if val.is_null() {
            return;
        }
        let n = crate::sexp::accessors::NAMED(val);
        if n > 0 && n < 3 {
            crate::sexp::accessors::SET_NAMED(val, n - 1);
        }
    }
}

// ---------------------------------------------------------------------------
// promiseArgs — create promises for closure arguments
// ---------------------------------------------------------------------------

/// Retain promised arguments across allocation and dispatch callbacks.
/// The explicit factory also supports promises whose environment is nil.
/// # Safety
/// Activate the factory's original owner and exclude Rust payload loans across
/// the translated variable lookup and R error bridges.
pub(crate) unsafe fn promiseArgs<'s>(
    factory: &SessionNodeFactory<'s>,
    arguments: Sexp<'s>,
    rho: Sexp<'s>,
) -> Sexp<'s> {
    factory
        .require_active()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    factory
        .link(&arguments)
        .and_then(|_| factory.link(&rho))
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let missing = factory.missing();
    let dots_symbol = unsafe { R_DotsSymbol() };
    let mut result = PairlistBuilder::from_factory(factory.clone());
    let mut remaining = arguments;
    while !remaining.is_nil() {
        let expression = remaining
            .try_car()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if expression.as_raw() == dots_symbol {
            // Lookup can invoke active bindings. Root the returned dots before
            // constructing promises or allocating any new list cell.
            let mut dots = argument_value(factory, unsafe { R_findVar(dots_symbol, rho.as_raw()) });
            if dots.typeof_() == SEXPTYPE::DOTSXP || dots.is_nil() {
                while !dots.is_nil() {
                    let expression = dots
                        .try_car()
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    let value = if expression.as_raw() == missing.as_raw() {
                        expression
                    } else {
                        factory.promise(&expression, &rho).unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        })
                    };
                    result
                        .push(value, dots.tag())
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    dots = dots
                        .try_cdr()
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                }
            } else if dots.as_raw() != missing.as_raw() {
                crate::sexp::context::r_error("'...' used in an incorrect context");
            }
        } else {
            let value = if expression.as_raw() == missing.as_raw() {
                expression
            } else {
                // Incoming promises are intentionally wrapped again: their
                // original expression and environment must survive redispatch.
                factory
                    .promise(&expression, &rho)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
            };
            result
                .push(value, remaining.tag())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        remaining = remaining
            .try_cdr()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    result
        .finish()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

// ---------------------------------------------------------------------------
// Helper: evalArgs — evaluate arguments with dropmissing support
// ---------------------------------------------------------------------------

/// Evaluate arguments, optionally dropping missing values.
/// Used by DispatchOrEval when args need to be evaluated before passing
/// to the generic code.
unsafe fn evalArgs<'a>(
    args: SEXP,
    rho: SEXP,
    dropmissing: c_int,
    call: SEXP,
    _argument_offset: c_int,
) -> Sexp<'a> {
    // SAFETY: this translated dispatch boundary retains the active owner.
    let factory = SessionNodeFactory::new(
        unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string())),
    );
    if dropmissing != 0 {
        let args = factory
            .wrap(args)
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        let rho = factory
            .wrap(rho)
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        let call = if call.is_null() {
            None
        } else {
            Some(
                factory
                    .wrap(call)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string())),
            )
        };
        evalList(args, rho, call, -1)
    } else {
        let args = factory
            .wrap(args)
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        let rho = factory
            .wrap(rho)
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        evalListKeepMissing(args, rho)
    }
}

// ---------------------------------------------------------------------------
// Helper: isFunction — check if SEXP is a function
// ---------------------------------------------------------------------------

unsafe fn isFunction(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return FALSE;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            TRUE
        } else {
            FALSE
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: isSymbol — check if SEXP is a symbol
// ---------------------------------------------------------------------------

unsafe fn isSymbol(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return FALSE;
        }
        if TYPEOF(x) == SEXPTYPE::SYMSXP {
            TRUE
        } else {
            FALSE
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: translateChar — get C string from CHARSXP
// ---------------------------------------------------------------------------

unsafe fn translateChar(x: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(x) }
}

// ---------------------------------------------------------------------------
// Helper: streql — compare two C strings
// ---------------------------------------------------------------------------

unsafe fn streql(a: *const c_char, b: *const c_char) -> c_int {
    unsafe {
        if a.is_null() || b.is_null() {
            return FALSE;
        }
        if std::ffi::CStr::from_ptr(a).to_bytes() == std::ffi::CStr::from_ptr(b).to_bytes() {
            TRUE
        } else {
            FALSE
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: Rf_strrchr — find last occurrence of char in string
// ---------------------------------------------------------------------------

unsafe fn Rf_strrchr(s: *const c_char, c: c_char) -> *const c_char {
    unsafe {
        if s.is_null() {
            return ptr::null();
        }
        let mut len = 0;
        let mut p = s;
        while *p != 0 {
            p = p.add(1);
            len += 1;
        }
        if len == 0 {
            return ptr::null();
        }
        p = s.add(len as usize);
        while p != s {
            p = p.sub(1);
            if *p == c {
                return p;
            }
        }
        ptr::null()
    }
}

// ---------------------------------------------------------------------------
// Helper: R_mkString — create a length-1 character vector
// ---------------------------------------------------------------------------

unsafe fn R_mkString(s: *const c_char) -> SEXP {
    unsafe {
        if s.is_null() {
            return R_NilValue();
        }
        Rf_mkString(s)
    }
}

// ---------------------------------------------------------------------------
// Helper: stringSuffix — get suffix of character vector starting at pos
// ---------------------------------------------------------------------------

unsafe fn stringSuffix(klass: SEXP, pos: c_int) -> SEXP {
    unsafe {
        if klass.is_null() || pos < 0 {
            return R_NilValue();
        }
        let n = LENGTH(klass);
        if pos >= n {
            return R_NilValue();
        }
        let len = n - pos;
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, len);
        let _ans_guard = protect(ans);
        for i in 0..len {
            let src = STRING_ELT(klass, (pos + i) as R_xlen_t);
            SET_STRING_ELT(ans, i as R_xlen_t, src);
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// Helper: stringPositionTr — find string in character vector
// ---------------------------------------------------------------------------

unsafe fn stringPositionTr(klass: SEXP, what: *const c_char) -> c_int {
    unsafe {
        if klass.is_null() || what.is_null() {
            return -1;
        }
        let n = LENGTH(klass);
        for i in 0..n {
            let elt = STRING_ELT(klass, i as R_xlen_t);
            if !elt.is_null() {
                let cs = CHAR(elt);
                if !cs.is_null()
                    && std::ffi::CStr::from_ptr(cs).to_bytes()
                        == std::ffi::CStr::from_ptr(what).to_bytes()
                {
                    return i;
                }
            }
        }
        -1
    }
}

// ---------------------------------------------------------------------------
// Helper: R_data_class — get the class of an object (S3)
// ---------------------------------------------------------------------------

/// Get the data class of an object, equivalent to R's R_data_class2.
unsafe fn R_data_class(obj: SEXP) -> SEXP {
    unsafe {
        if obj.is_null() || obj == R_NilValue() {
            return R_NilValue();
        }
        crate::eval::attrib_core::R_data_class(obj)
    }
}

// ---------------------------------------------------------------------------
// Helper: R_BlankScalarString — return a blank scalar string
// ---------------------------------------------------------------------------

unsafe fn R_BlankScalarString_val() -> SEXP {
    unsafe { Rf_mkString(b"\x00".as_ptr() as *const c_char) }
}

// ---------------------------------------------------------------------------
// R_forceAndCall — force a specific number of promises and call a function
// ---------------------------------------------------------------------------

/// Force the first n promises in an argument list and call a function.
///
/// This is the equivalent of R's `R_forceAndCall()` in eval.c.
pub unsafe fn R_forceAndCall(e: SEXP, op: SEXP, args: SEXP, rho: SEXP, n: c_int) -> SEXP {
    unsafe {
        // Force the first n promises
        let forced_args = args;
        let mut count: c_int = 0;
        let tail: SEXP = ptr::null_mut();

        let mut current = args;
        while !current.is_null() && current != R_NilValue() && count < n {
            let val = CAR(current);
            if TYPEOF(val) == SEXPTYPE::PROMSXP {
                let forced_val = forcePromise(val);
                SETCAR(current, forced_val);
            }
            count += 1;
            current = CDR(current);
        }

        // Call the function
        if TYPEOF(op) == SEXPTYPE::BUILTINSXP {
            // Builtin: pass already-evaluated args
            if let Some(primfun) = super::eval::get_primfun(op) {
                primfun(e, op, args, rho)
            } else {
                R_NilValue()
            }
        } else if TYPEOF(op) == SEXPTYPE::CLOSXP {
            super::closure::applyClosure(e, op, args, rho, R_NilValue(), TRUE)
        } else {
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// DispatchOrEval — S3/S4 dispatch
// ---------------------------------------------------------------------------

/// Dispatch or evaluate an expression, handling S3/S4 method dispatch.
///
/// This is the equivalent of R's `DispatchOrEval()` in eval.c.
/// Returns 1 if a method was dispatched (result in *ans), 0 if not
/// (evaluated args in *ans).
pub unsafe fn DispatchOrEval(
    call: SEXP,
    op: SEXP,
    generic: *const c_char,
    args: SEXP,
    rho: SEXP,
    ans: *mut SEXP,
    dropmissing: c_int,
    argsevald: c_int,
) -> c_int {
    unsafe {
        let factory = active_argument_factory();
        let arguments_owner = argument_value(&factory, args);
        let environment_owner = argument_value(&factory, rho);
        let _call_owner = argument_value(&factory, call);
        let mut x: SEXP = R_NilValue();
        let mut dots: c_int = FALSE;
        let mut guards = Vec::new();

        if generic.is_null() || ans.is_null() {
            return 0;
        }

        // Step 1: Find the object to dispatch on
        if argsevald != 0 {
            // Args are already evaluated
            x = CAR(args);
            if !x.is_null() {
                guards.push(protect(x));
            }
        } else {
            // Find the object, dropping leading ... with missing/empty values
            let mut args_iter = args;
            while !args_iter.is_null() && args_iter != R_NilValue() {
                if CAR(args_iter) == R_DotsSymbol() {
                    let h = R_findVar(R_DotsSymbol(), rho);
                    if TYPEOF(h) == SEXPTYPE::DOTSXP {
                        dots = TRUE;
                        x = Rf_eval(CAR(h), rho);
                        break;
                    } else if h != R_NilValue() && h != R_MissingArg() {
                        // '...' used in incorrect context — skip
                        args_iter = CDR(args_iter);
                        continue;
                    }
                } else {
                    dots = FALSE;
                    x = Rf_eval(CAR(args_iter), rho);
                    break;
                }
                args_iter = CDR(args_iter);
            }
            if !x.is_null() {
                guards.push(protect(x));
            }
        }

        // Step 2: Try to dispatch if x is an object
        if !x.is_null() && x != R_NilValue() && isObject(x) != FALSE {
            // Check if the generic name ends with ".default" — if so, no dispatch
            let mut pt: *const c_char = ptr::null();
            if isSymbol(CAR(call)) != FALSE {
                let pname = PRINTNAME(CAR(call));
                if !pname.is_null() {
                    let cs = CHAR(pname);
                    if !cs.is_null() {
                        pt = Rf_strrchr(cs, '.' as c_char);
                    }
                }
            }

            // Only dispatch if not already the default method
            if pt.is_null() || streql(pt, b".default\x00".as_ptr() as *const c_char) == FALSE {
                // Create promises for the arguments
                let pargs_owner =
                    promiseArgs(&factory, arguments_owner.clone(), environment_owner.clone());
                let pargs = pargs_owner.as_raw();

                // Create a new environment for dispatch context
                let rho1 = NewEnvironment(R_NilValue(), rho, R_NilValue());
                guards.push(protect(rho1));

                // Set the evaluated value as the first promise's value
                // (IF_PROMSXP_SET_PRVALUE)
                if !pargs.is_null()
                    && pargs != R_NilValue()
                    && TYPEOF(CAR(pargs)) == SEXPTYPE::PROMSXP
                {
                    // Force the first promise to be x
                    crate::sexp::accessors::SET_PRVALUE(CAR(pargs), x);
                }

                // usemethod reads its call and promises from the active
                // context, including when a primitive is invoked at top level.
                let _context_guard = crate::sexp::context::begin_context_guard(
                    crate::sexp::context::ctxt_flags::CTXT_RETURN,
                    call,
                    rho1,
                    rho,
                    None,
                    op,
                    pargs,
                );
                if crate::mainutils::coerce::IS_S4_OBJECT(x) != FALSE
                    && crate::mainutils::objects::R_has_methods(op) != FALSE
                {
                    let value =
                        crate::mainutils::objects::R_possible_dispatch(call, op, pargs, rho, TRUE);
                    if !value.is_null() {
                        *ans = value;
                        return 1;
                    }
                }

                let dispatched = crate::mainutils::objects::usemethod(
                    generic,
                    x,
                    call,
                    pargs,
                    rho1,
                    rho,
                    super::runtime::base_env(),
                    ans,
                );

                if dispatched != FALSE {
                    return 1;
                }
            }
        }

        // Step 3: No dispatch — evaluate arguments and return them
        if argsevald == 0 {
            if dots != FALSE {
                let evaluated = evalArgs(args, rho, dropmissing, call, 0);
                *ans = evaluated.as_raw();
            } else {
                bump_named_link(x);
                let rest = evalArgs(CDR(args), rho, dropmissing, call, 1);
                drop_named_link(x);
                let arglist = CONS_NR(x, rest.as_raw());
                SETTAG(arglist, TAG(args));
                *ans = arglist;
            }
        } else {
            *ans = args;
        }

        0
    }
}

// ---------------------------------------------------------------------------
// findmethod — find an S3 method (for group dispatch)
// ---------------------------------------------------------------------------

/// Find an S3 method by interleaving group and generic method lookups.
///
/// For each class in the hierarchy, first looks for "generic.class",
/// then "group.class". Returns via output parameters.
///
/// `gr` must be protected by the caller after this function returns.
unsafe fn findmethod(
    class: SEXP,
    group: *const c_char,
    generic: *const c_char,
    sxp: *mut SEXP,
    gr: *mut SEXP,
    meth: *mut SEXP,
    which: *mut c_int,
    _objSlot: SEXP,
    rho: SEXP,
) {
    unsafe {
        if class.is_null() || class == R_NilValue() {
            *sxp = R_NilValue();
            *gr = R_NilValue();
            *meth = R_NilValue();
            *which = 0;
            return;
        }

        let len = LENGTH(class);
        let _vmax = vmaxget();
        let mut whichclass: c_int = 0;

        // Interleave: for each class, try generic then group
        for wc in 0..len {
            whichclass = wc;
            let ss = translateChar(STRING_ELT(class, wc as R_xlen_t));
            if ss.is_null() {
                continue;
            }

            // Try generic.class
            let m = crate::mainutils::names::installS3Signature(generic, ss);
            *meth = m;
            let val =
                crate::mainutils::objects::R_LookupMethod(m, rho, rho, super::runtime::base_env());
            *sxp = val;
            if isFunction(val) != FALSE {
                *gr = R_BlankScalarString_val();
                break;
            }

            // Try group.class
            let mg = crate::mainutils::names::installS3Signature(group, ss);
            *meth = mg;
            let valg =
                crate::mainutils::objects::R_LookupMethod(mg, rho, rho, super::runtime::base_env());
            *sxp = valg;
            if isFunction(valg) != FALSE {
                // Keep portable unit arithmetic and summaries on their Rust
                // evaluator path. User supplied closures with the same S3
                // name still dispatch normally.
                let class_name = std::ffi::CStr::from_ptr(ss).to_bytes();
                let group_name = std::ffi::CStr::from_ptr(group).to_bytes();
                if class_name == b"unit"
                    && (group_name == b"Ops" || group_name == b"Summary")
                    && TYPEOF(valg) != SEXPTYPE::CLOSXP
                {
                    *sxp = R_NilValue();
                    continue;
                }
                *gr = R_mkString(group);
                break;
            }
        }

        vmaxset(_vmax);
        *which = whichclass;
    }
}

// ---------------------------------------------------------------------------
// DispatchGroup — group dispatch for Math/Summary/Ops/Complex
// ---------------------------------------------------------------------------

/// Dispatch to a group generic method.
///
/// This is the equivalent of R's `DispatchGroup()` in eval.c.
/// Returns 1 if dispatched (result in *ans), 0 if not dispatched.
pub unsafe fn DispatchGroup(
    group: *const c_char,
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
    ans: *mut SEXP,
) -> c_int {
    unsafe {
        let factory = active_argument_factory();
        let _arguments_owner = argument_value(&factory, args);
        let environment_owner = argument_value(&factory, rho);
        let call_owner = argument_value(&factory, call);
        if args.is_null() || args == R_NilValue() || ans.is_null() {
            return 0;
        }

        // Pre-test: skip if first arg isn't an object and there's no second arg
        // that's an object either.  NOTE: isObject returns c_int; do not use
        // Rust `!` (bitwise not) — that made this pre-test always true.
        if isObject(CAR(args)) == FALSE
            && (CDR(args).is_null() || CDR(args) == R_NilValue() || isObject(CADR(args)) == FALSE)
        {
            return 0;
        }

        // Check if we're already processing the default method
        if isSymbol(CAR(call)) != FALSE {
            let pname = PRINTNAME(CAR(call));
            if !pname.is_null() {
                let cs = CHAR(pname);
                if !cs.is_null() {
                    let cs_bytes = std::ffi::CStr::from_ptr(cs).to_bytes();
                    let dot = match cs_bytes.iter().position(|&c| c == b'.') {
                        Some(idx) => cs.add(idx),
                        None => ptr::null(),
                    };
                    if !dot.is_null() {
                        let after_dot = dot.add(1);
                        if streql(after_dot, b"default\x00".as_ptr() as *const c_char) != FALSE {
                            return 0;
                        }
                    }
                }
            }
        }
        if crate::mainutils::coerce::IS_S4_OBJECT(CAR(args)) != FALSE
            || (!CDR(args).is_null()
                && CDR(args) != R_NilValue()
                && crate::mainutils::coerce::IS_S4_OBJECT(CADR(args)) != FALSE)
        {
            if crate::mainutils::objects::R_has_methods(op) != FALSE {
                let value =
                    crate::mainutils::objects::R_possible_dispatch(call, op, args, rho, FALSE);
                if !value.is_null() {
                    *ans = value;
                    return 1;
                }
            }
        }

        // For Ops group, check both args; for others, only the first
        let is_ops = streql(group, b"Ops\x00".as_ptr() as *const c_char) != FALSE
            || streql(group, b"matrixOps\x00".as_ptr() as *const c_char) != FALSE;
        let nargs: c_int = if is_ops {
            crate::sexp::constructors::Rf_length(args)
        } else {
            1
        };

        if nargs == 1 && isObject(CAR(args)) == FALSE {
            return 0;
        }

        // Get generic name from op
        let generic_name = Sexp::from_raw(op)
            .and_then(crate::eval::primitive::portable_primitive_name)
            .unwrap_or_else(|| PRIMNAME(op).to_owned());
        let generic = std::ffi::CString::new(generic_name).expect("primitive name contains NUL");

        // Get class of first arg
        let mut guards = Vec::new();

        let mut lclass = R_data_class(CAR(args));
        guards.push(protect(lclass));
        let rclass = if nargs == 2 {
            let class = R_data_class(CADR(args));
            guards.push(protect(class));
            class
        } else {
            R_NilValue()
        };

        let mut lmeth: SEXP = R_NilValue();
        let mut lsxp: SEXP = R_NilValue();
        let mut lgr: SEXP = R_NilValue();
        let mut rmeth: SEXP = R_NilValue();
        let mut rsxp: SEXP = R_NilValue();
        let mut rgr: SEXP = R_NilValue();
        let mut lwhich: c_int = 0;
        let mut rwhich: c_int = 0;

        findmethod(
            lclass,
            group,
            generic.as_ptr() as *const c_char,
            &mut lsxp,
            &mut lgr,
            &mut lmeth,
            &mut lwhich,
            args,
            rho,
        );
        guards.push(protect(lgr));

        if nargs == 2 {
            findmethod(
                rclass,
                group,
                generic.as_ptr() as *const c_char,
                &mut rsxp,
                &mut rgr,
                &mut rmeth,
                &mut rwhich,
                CDR(args),
                rho,
            );
            guards.push(protect(rgr));
        }

        // If no method found for either side, use default
        if isFunction(lsxp) == FALSE && isFunction(rsxp) == FALSE {
            return 0;
        }

        // Distinct methods must not silently select the left operand.
        if lsxp != rsxp {
            if isFunction(lsxp) != FALSE && isFunction(rsxp) != FALSE {
                // GNU R gives the date/time methods a deliberate precedence:
                // Ops.difftime yields to +/- .Date/.POSIXt, and yields to a
                // right-hand +.Date/.POSIXt in the reverse arrangement.
                if method_name_is(rmeth, b"Ops.difftime")
                    && (method_name_is(lmeth, b"+.POSIXt")
                        || method_name_is(lmeth, b"-.POSIXt")
                        || method_name_is(lmeth, b"+.Date")
                        || method_name_is(lmeth, b"-.Date"))
                {
                    rsxp = R_NilValue();
                } else if method_name_is(lmeth, b"Ops.difftime")
                    && (method_name_is(rmeth, b"+.POSIXt") || method_name_is(rmeth, b"+.Date"))
                {
                    lsxp = R_NilValue();
                } else if crate::mainutils::identical::R_compute_identical(lsxp, rsxp, 23) == 0 {
                    let left_name =
                        std::ffi::CStr::from_ptr(CHAR(PRINTNAME(lmeth))).to_string_lossy();
                    let right_name =
                        std::ffi::CStr::from_ptr(CHAR(PRINTNAME(rmeth))).to_string_lossy();
                    let warning = std::ffi::CString::new(format!(
                        "Incompatible methods (\"{left_name}\", \"{right_name}\") for \"{}\"",
                        generic.to_string_lossy()
                    ))
                    .expect("method names contain NUL");
                    if choose_ops_method(CAR(args), CADR(args), lsxp, rsxp, call, false, rho) {
                        rsxp = R_NilValue();
                    } else if choose_ops_method(CADR(args), CAR(args), rsxp, lsxp, call, true, rho)
                    {
                        lsxp = R_NilValue();
                    } else {
                        crate::mainutils::errors::Rf_warning(warning.as_ptr());
                        return 0;
                    }
                }
            }
            // If left side has no method, use right
            if isFunction(lsxp) == FALSE {
                lsxp = rsxp;
                lmeth = rmeth;
                lgr = rgr;
                lclass = rclass;
                lwhich = rwhich;
            }
        }

        // Build the method vector for each argument
        let dispatch_class_name = translateChar(STRING_ELT(lclass, lwhich as R_xlen_t));
        let _vmax = vmaxget();

        let m = Rf_allocVector(SEXPTYPE::STRSXP, nargs);
        guards.push(protect(m));

        let mut s = args;
        for i in 0..nargs {
            let t = R_data_class(CAR(s));
            if !t.is_null()
                && TYPEOF(t) == SEXPTYPE::STRSXP
                && stringPositionTr(t, dispatch_class_name) >= 0
            {
                SET_STRING_ELT(m, i as R_xlen_t, PRINTNAME(lmeth));
            } else {
                SET_STRING_ELT(m, i as R_xlen_t, STRING_ELT(R_BlankScalarString_val(), 0));
            }
            s = CDR(s);
        }
        vmaxset(_vmax);

        // Create the S3 dispatch variables
        let generic_str = R_mkString(generic.as_ptr() as *const c_char);
        guards.push(protect(generic_str));

        let dot_class = stringSuffix(lclass, lwhich);
        guards.push(protect(dot_class));

        let newvars = crate::mainutils::objects::createS3Vars(
            generic_str,
            lgr,
            dot_class,
            m,
            rho,
            super::runtime::base_env(),
        );
        guards.push(protect(newvars));

        // Build the new call: (method . rest-of-call)
        let newcall = Rf_lang2(lmeth, R_NilValue());
        SETCDR(newcall, CDR(call));
        guards.push(protect(newcall));

        // Create promises for the arguments
        let pargs_owner = promiseArgs(
            &factory,
            call_owner
                .try_cdr()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            environment_owner.clone(),
        );
        let pargs = pargs_owner.as_raw();

        // Set promise values to the evaluated args
        let mut pi = pargs;
        let mut ai = args;
        while !pi.is_null() && pi != R_NilValue() && !ai.is_null() && ai != R_NilValue() {
            if TYPEOF(CAR(pi)) == SEXPTYPE::PROMSXP {
                // Cache the value without discarding PRCODE: substitute()
                // in a method must still see the caller's expression.
                crate::sexp::accessors::SET_PRVALUE(CAR(pi), CAR(ai));
            }
            if is_ops {
                SETTAG(pi, R_NilValue());
            }
            pi = CDR(pi);
            ai = CDR(ai);
        }

        // Dispatch through the common method application path so closure
        // methods receive the S3 frame variables (.Generic, .Group,
        // .Class, and .Method).  NextMethod discovers those bindings in the
        // active method frame; calling applyClosure directly would silently
        // drop them and make group methods unable to continue dispatch.
        *ans = crate::mainutils::objects::applyMethod(newcall, lsxp, pargs, rho, newvars);

        1
    }
}

// ---------------------------------------------------------------------------
// evalListKeepMissing — evaluate pairlist preserving R_MissingArg
// ---------------------------------------------------------------------------

/// Evaluate each element of a pairlist, but preserve `R_MissingArg` arguments
/// rather than erroring on them.
///
/// Ported from R's `evalListKeepMissing()` in eval.c.
/// Iterative (not recursive) to avoid protection stack growth.
pub fn evalListKeepMissing<'a>(el: Sexp<'a>, rho: Sexp<'a>) -> Sexp<'a> {
    let factory = rho
        .node_factory()
        .or_else(|_| el.node_factory())
        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
    factory
        .require_active()
        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
    let mut remaining = factory
        .wrap(el.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
    let mut result = PairlistBuilder::from_factory(factory.clone());
    let mut bumped = NamedArguments::new();
    while !remaining.is_nil() {
        let expr = remaining
            .try_car()
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        if expr.as_raw() == unsafe { R_DotsSymbol() } {
            let mut dots = factory
                .wrap(unsafe { R_findVarInFrame(rho.as_raw(), expr.as_raw()) })
                .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
            if dots.typeof_() == SEXPTYPE::DOTSXP || dots.is_nil() {
                while !dots.is_nil() {
                    let expr = dots
                        .try_car()
                        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
                    let value = if expr.as_raw() == unsafe { R_MissingArg() } {
                        expr
                    } else {
                        let value = factory
                            .wrap(unsafe { Rf_eval(expr.as_raw(), rho.as_raw()) })
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(&error.to_string())
                            });
                        bumped.retain(&value);
                        value
                    };
                    result
                        .push(value, dots.tag())
                        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
                    dots = dots
                        .try_cdr()
                        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
                }
            } else if dots.as_raw() != unsafe { R_MissingArg() } {
                crate::sexp::context::r_error("'...' used in an incorrect context");
            }
        } else {
            let value = if expr.as_raw() == unsafe { R_MissingArg() } {
                expr
            } else {
                let value = factory
                    .wrap(unsafe { Rf_eval(expr.as_raw(), rho.as_raw()) })
                    .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
                bumped.retain(&value);
                value
            };
            result
                .push(value, remaining.tag())
                .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
        }
        remaining = remaining
            .try_cdr()
            .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()));
    }
    result
        .finish()
        .unwrap_or_else(|error| crate::sexp::context::r_error(&error.to_string()))
}

#[cfg(test)]
mod owned_argument_tests {
    use super::*;
    use crate::sexp::{object::PairlistIter, session::RSession};
    use std::{cell::Cell, rc::Rc};

    fn parse_arguments<'s>(
        session: &'s RSession,
        factory: &SessionNodeFactory<'s>,
        text: &str,
    ) -> Sexp<'s> {
        session
            .owner_token()
            .unwrap()
            .with_arena(|arena| crate::eval::parser::parse(text, arena, factory.clone()))
            .unwrap()
            .unwrap()
    }

    fn force_allocations_with_reentrant_gc(session: &RSession) -> Rc<Cell<usize>> {
        let notifications = Rc::new(Cell::new(0));
        let observed = notifications.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        notifications
    }

    #[test]
    fn owned_promise_arguments_preserve_dots_tags_and_caller_environment_through_gc() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let environment = session.global_env().unwrap();
        for (name, number) in [(c"x", 11), (c"y", 22)] {
            let value = factory.wrap(unsafe { Rf_ScalarInteger(number) }).unwrap();
            unsafe {
                crate::sexp::envir::defineVar(
                    Rf_install(name.as_ptr()),
                    value.as_raw(),
                    environment.as_raw(),
                );
            }
        }
        let call = parse_arguments(&session, &factory, "list(first=x, absent=, ..., last=33L)");
        let dots_source = parse_arguments(&session, &factory, "list(dot_missing=, dot_value=y)");
        let mut dots_builder = PairlistBuilder::from_factory(factory.clone());
        let mut inner_promise = None;
        for cell in PairlistIter::new(dots_source.try_cdr().unwrap()) {
            let expression = cell.try_car().unwrap();
            let value = if expression.as_raw() == factory.missing().as_raw() {
                expression
            } else {
                let promise = factory.promise(&expression, &environment).unwrap();
                inner_promise = Some(promise.clone());
                promise
            };
            dots_builder.push(value, cell.tag()).unwrap();
        }
        let dots = dots_builder.finish_as_type(SEXPTYPE::DOTSXP).unwrap();
        unsafe {
            crate::sexp::envir::defineVar(R_DotsSymbol(), dots.as_raw(), environment.as_raw());
        }
        let args = call.try_cdr().unwrap();
        let first_expression = args.try_car().unwrap();
        let first_code = first_expression.as_raw();
        let original_named = unsafe { crate::sexp::accessors::NAMED(first_code) };
        let inner_code = inner_promise.as_ref().unwrap().as_raw();
        let before = crate::sexp::protect::R_ProtectCount();
        let notifications = force_allocations_with_reentrant_gc(&session);
        let promised = unsafe { promiseArgs(&factory, args, environment.clone()) };
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 0;
        });
        assert!(notifications.get() >= 8);
        assert_eq!(
            unsafe { crate::sexp::accessors::NAMED(first_code) },
            original_named
        );
        drop(first_expression);
        drop(call);
        drop(dots_source);
        drop(dots);
        drop(inner_promise);
        unsafe {
            crate::sexp::envir::defineVar(
                R_DotsSymbol(),
                factory.nil().as_raw(),
                environment.as_raw(),
            );
        }
        crate::sexp::gengc::full_gc();
        let cells: Vec<_> = PairlistIter::new(promised).collect();
        assert_eq!(cells.len(), 5);
        let tags: Vec<_> = cells
            .iter()
            .map(|cell| {
                cell.try_tag()
                    .unwrap()
                    .try_printname()
                    .unwrap()
                    .try_as_string()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            tags,
            ["first", "absent", "dot_missing", "dot_value", "last"]
        );
        assert_eq!(
            cells[1].try_car().unwrap().as_raw(),
            factory.missing().as_raw()
        );
        assert_eq!(
            cells[2].try_car().unwrap().as_raw(),
            factory.missing().as_raw()
        );
        for (index, expected) in [(0, 11), (3, 22), (4, 33)] {
            let promise = cells[index].try_car().unwrap();
            assert_eq!(promise.typeof_(), SEXPTYPE::PROMSXP);
            assert_eq!(promise.try_prenv().unwrap(), environment);
            assert_eq!(
                promise.try_prvalue().unwrap().as_raw(),
                factory.unbound().as_raw()
            );
            if index == 0 {
                assert_eq!(promise.try_prcode().unwrap().as_raw(), first_code);
            }
            if index == 3 {
                assert_eq!(promise.try_prcode().unwrap().as_raw(), inner_code);
            }
            let value = factory
                .wrap(unsafe { crate::sexp::envir::forcePromise(promise.as_raw()) })
                .unwrap();
            assert_eq!(value.integer_elt(0), Some(expected));
        }
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
    }

    #[test]
    fn owned_promise_arguments_allow_nil_environment() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let call = parse_arguments(&session, &factory, "list(first=11L, second=22L)");
        let promised = unsafe { promiseArgs(&factory, call.try_cdr().unwrap(), factory.nil()) };
        drop(call);
        crate::sexp::gengc::full_gc();
        let cells: Vec<_> = PairlistIter::new(promised).collect();
        assert_eq!(cells.len(), 2);
        for (cell, expected) in cells.into_iter().zip([11, 22]) {
            let promise = cell.try_car().unwrap();
            assert!(promise.try_prenv().unwrap().is_nil());
            let value = factory
                .wrap(unsafe { crate::sexp::envir::forcePromise(promise.as_raw()) })
                .unwrap();
            assert_eq!(value.integer_elt(0), Some(expected));
        }
    }

    #[test]
    fn promise_arguments_reject_invalid_dots_after_partial_construction_and_retry() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let environment = session.global_env().unwrap();
        let invalid = factory.wrap(unsafe { Rf_ScalarInteger(7) }).unwrap();
        unsafe {
            crate::sexp::envir::defineVar(R_DotsSymbol(), invalid.as_raw(), environment.as_raw());
        }
        let call = parse_arguments(&session, &factory, "list(first=11L, ..., last=22L)");
        let args = call.try_cdr().unwrap();
        let before = crate::sexp::protect::R_ProtectCount();
        let notifications = force_allocations_with_reentrant_gc(&session);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            promiseArgs(&factory, args.clone(), environment.clone())
        }))
        .expect_err("invalid dots must fail after building the first promise");
        let message = failure
            .downcast_ref::<RError>()
            .map(|error| error.message.as_str())
            .or_else(|| {
                failure
                    .downcast_ref::<crate::sexp::context::RSignal>()
                    .and_then(|signal| {
                        if let crate::sexp::context::RSignal::Error { message } = signal {
                            Some(message.as_str())
                        } else {
                            None
                        }
                    })
            });
        assert_eq!(message, Some("'...' used in an incorrect context"));
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
        unsafe {
            crate::sexp::envir::defineVar(
                R_DotsSymbol(),
                factory.missing().as_raw(),
                environment.as_raw(),
            );
        }
        let retry = unsafe { promiseArgs(&factory, args.clone(), environment.clone()) };
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 0;
        });
        drop(call);
        drop(args);
        crate::sexp::gengc::full_gc();
        let cells: Vec<_> = PairlistIter::new(retry).collect();
        assert_eq!(cells.len(), 2);
        for (cell, expected) in cells.iter().zip([11, 22]) {
            let value = factory
                .wrap(unsafe { crate::sexp::envir::forcePromise(cell.try_car().unwrap().as_raw()) })
                .unwrap();
            assert_eq!(value.integer_elt(0), Some(expected));
        }
        assert!(notifications.get() >= 6);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
    }

    #[test]
    fn owned_keep_missing_preserves_dots_and_tags_through_reentrant_gc() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = SessionNodeFactory::new(owner);
        let call = owner
            .with_arena(|arena| {
                crate::eval::parser::parse(
                    "list(first=11L + 1L, absent=, ..., last=33L + 3L)",
                    arena,
                    factory.clone(),
                )
            })
            .unwrap()
            .unwrap();
        let dots_source = owner
            .with_arena(|arena| {
                crate::eval::parser::parse(
                    "list(dot_missing=, dot_value=22L + 2L)",
                    arena,
                    factory.clone(),
                )
            })
            .unwrap()
            .unwrap();
        let mut dots_builder = PairlistBuilder::from_factory(factory.clone());
        for cell in PairlistIter::new(dots_source.try_cdr().unwrap()) {
            dots_builder
                .push(cell.try_car().unwrap(), cell.tag())
                .unwrap();
        }
        let dots = dots_builder.finish_as_type(SEXPTYPE::DOTSXP).unwrap();
        let environment = session.global_env().unwrap();
        unsafe {
            crate::sexp::envir::defineVar(R_DotsSymbol(), dots.as_raw(), environment.as_raw());
        }
        let notifications = Rc::new(Cell::new(0));
        let observed = notifications.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        let before_roots = crate::sexp::protect::R_ProtectCount();
        let evaluated = evalListKeepMissing(call.try_cdr().unwrap(), environment);
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 0;
        });
        drop(call);
        drop(dots_source);
        drop(dots);
        crate::sexp::gengc::full_gc();
        let cells: Vec<_> = PairlistIter::new(evaluated).collect();
        let values: Vec<_> = cells.iter().map(|cell| cell.try_car().unwrap()).collect();
        assert_eq!(values.len(), 5);
        assert_eq!(values[0].integer_elt(0).unwrap(), 12);
        assert_eq!(values[1].as_raw(), unsafe { R_MissingArg() });
        assert_eq!(values[2].as_raw(), unsafe { R_MissingArg() });
        assert_eq!(values[3].integer_elt(0).unwrap(), 24);
        assert_eq!(values[4].integer_elt(0).unwrap(), 36);
        let tags: Vec<_> = cells
            .iter()
            .map(|cell| {
                cell.try_tag()
                    .unwrap()
                    .try_printname()
                    .unwrap()
                    .try_as_string()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            tags,
            ["first", "absent", "dot_missing", "dot_value", "last"]
        );
        assert!(notifications.get() >= 5);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before_roots);
    }

    #[test]
    fn keep_missing_argument_error_restores_named_links_without_manual_roots() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = SessionNodeFactory::new(owner);
        let call = owner
            .with_arena(|arena| {
                crate::eval::parser::parse(
                    "list(11L, preserved=, missing_after_preserved_arguments)",
                    arena,
                    factory,
                )
            })
            .unwrap()
            .unwrap();
        let args = call.try_cdr().unwrap();
        let first = args.try_car().unwrap();
        let before_named = unsafe { crate::sexp::accessors::NAMED(first.as_raw()) };
        let before_roots = crate::sexp::protect::R_ProtectCount();
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            evalListKeepMissing(args, session.global_env().unwrap())
        }));
        assert!(error.is_err());
        assert_eq!(
            unsafe { crate::sexp::accessors::NAMED(first.as_raw()) },
            before_named
        );
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before_roots);
    }

    #[test]
    fn owned_argument_list_survives_each_allocation_and_reentrant_gc() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = SessionNodeFactory::new(owner);
        let call = owner
            .with_arena(|arena| {
                crate::eval::parser::parse(
                    "list(first=11L + 1L, 22L + 2L, third=33L + 3L)",
                    arena,
                    factory.clone(),
                )
            })
            .unwrap()
            .unwrap();
        let args = call.try_cdr().unwrap();
        let environment = session.global_env().unwrap();
        let notifications = Rc::new(Cell::new(0));
        let observed = notifications.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        let before = crate::sexp::protect::R_ProtectCount();
        let evaluated = evalList(args, environment, Some(call.clone()), -1);
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 0;
        });
        drop(call);
        crate::sexp::gengc::full_gc();
        let cells: Vec<_> = PairlistIter::new(evaluated).collect();
        let values: Vec<_> = cells
            .iter()
            .map(|cell| cell.try_car().unwrap().integer_elt(0).unwrap())
            .collect();
        assert_eq!(values, [12, 24, 36]);
        assert_eq!(
            cells[0]
                .try_tag()
                .unwrap()
                .try_printname()
                .unwrap()
                .try_as_string()
                .unwrap(),
            "first"
        );
        assert_eq!(
            cells[2]
                .try_tag()
                .unwrap()
                .try_printname()
                .unwrap()
                .try_as_string()
                .unwrap(),
            "third"
        );
        assert!(notifications.get() >= 3);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
    }

    #[test]
    fn argument_error_restores_named_links_without_manual_roots() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = SessionNodeFactory::new(owner);
        let call = owner
            .with_arena(|arena| crate::eval::parser::parse("list(11L, )", arena, factory))
            .unwrap()
            .unwrap();
        let args = call.try_cdr().unwrap();
        let first = args.try_car().unwrap();
        let before_named = unsafe { crate::sexp::accessors::NAMED(first.as_raw()) };
        let before_roots = crate::sexp::protect::R_ProtectCount();
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            evalList(args, session.global_env().unwrap(), Some(call), -1)
        }));
        assert!(error.is_err());
        assert_eq!(
            unsafe { crate::sexp::accessors::NAMED(first.as_raw()) },
            before_named
        );
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before_roots);
    }
}
