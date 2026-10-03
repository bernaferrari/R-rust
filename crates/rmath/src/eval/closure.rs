#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Closure application — ports R's applyClosure from eval.c.
//!
//! Handles calling R closures (user-defined functions) by:
//! 1. Creating a new environment
//! 2. Binding formal parameters to actual arguments
//! 3. Evaluating the body in the new environment

use std::ffi::CStr;
use std::os::raw::c_int;
use std::ptr;



use crate::sexp::accessors::{
    BODY, CAR, CDR, CHAR, CLOENV, PRCODE, PRINTNAME, SETCAR, SETCDR, STRING_ELT, TAG, TYPEOF,
    XLENGTH,
};
use crate::sexp::envir::{Environment, addMissingVarsToNewEnv};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::memory_ext::{NewEnvironment, mkPROMISE};
use crate::sexp::object::{PairlistBuilder, PairlistIter, SessionNodeFactory, Sexp, SexpError};
use crate::sexp::protect::protect;
use crate::sexp::symbol::R_DotsSymbol;

use super::eval::Rf_eval;

fn sexp_err(context: &str, err: SexpError) -> String {
    format!("{context}: {err}")
}

// ---------------------------------------------------------------------------
// Safe closure application — the primary internal implementation
// ---------------------------------------------------------------------------

/// Safe closure application using Sexp<'a>.
///
/// This is the idiomatic Rust API for applying R closures.
/// It extracts formals, body, and environment from the closure,
/// matches arguments to formals, creates a new evaluation environment,
/// and evaluates the body.
/// # Safety
/// Activate the live owner of all inputs and retain their reachable graphs
/// through allocation and R reentry. No Rust payload loan may cross execution.
pub unsafe fn apply_closure_safe<'a>(
    closure: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    if !closure.clone().is_closure() {
        return Err("not a closure".to_string());
    }
    let _closure_guard = unsafe { protect(closure.clone().as_raw()) };
    unsafe { super::jit::R_CheckJIT(closure.clone().as_raw()) };

    let formals = closure
        .clone()
        .try_formals()
        .clone()
        .map_err(|err| sexp_err("closure formals lookup", err))?;
    let mut body = closure
        .clone()
        .try_body()
        .clone()
        .map_err(|err| sexp_err("closure body lookup", err))?;
    if unsafe { TYPEOF(body.clone().as_raw()) } == SEXPTYPE::BCODESXP {
        if let Some(source) =
            unsafe { methods_matchsignature_source(closure.clone().as_raw(), body.clone().as_raw()) }
        {
            unsafe {
                crate::sexp::accessors::SET_BODY(closure.clone().as_raw(), source);
                body = Sexp::from_raw_unchecked(source);
            }
        }
    }

    let cloenv = closure
        .try_cloenv()
        .map_err(|err| sexp_err("closure environment lookup", err))?;
    let cloenv = unsafe {
        Sexp::from_raw_unchecked(remap_methods_snapshot_cloenv(
            closure.as_raw(),
            cloenv.as_raw(),
        ))
    };


    // Match arguments to formals
    let matched = unsafe { match_args_safe(formals.clone(), args.clone()) }?;

    // Create new environment with matched arguments
    let new_env = unsafe { create_env_safe(matched, cloenv) }?;

    // Bind the matched arguments into the new environment
    let frame = new_env
        .clone()
        .try_frame()
        .clone()
        .map_err(|err| sexp_err("new closure environment frame lookup", err))?;
    let new_env_bindings = Environment::new(new_env.clone())?;
    for cell in PairlistIter::new(frame) {
        let sym = cell
            .clone()
            .try_tag()
            .clone()
            .map_err(|err| sexp_err("matched argument tag lookup", err))?;
        if !sym.clone().is_nil() {
            let val = cell
                .try_car()
                .map_err(|err| sexp_err("matched argument value lookup", err))?;
            unsafe { new_env_bindings.clone().define(sym, val) }.clone()?;
        }
    }

    // Add missing arguments
    unsafe {
        addMissingVarsToNewEnv(formals.as_raw(), args.as_raw(), new_env.clone().as_raw());
    }

    // Evaluate body in new environment.
    // If the body was compiled to BCODESXP by cmpfun or the invocation JIT,
    // the top-level eval_safe dispatch (EvalKind::Bytecode) calls bcEval.
    unsafe { crate::eval::eval::eval_safe(body, new_env)
}
}

/// Safe argument matching using Sexp<'a> and PairlistIter.
///
/// Matches actual arguments to formal parameters, building a new
/// pairlist with the matched values.
/// # Safety
/// Activate the live owner of all inputs and retain their reachable graphs
/// through allocation and R reentry. No Rust payload loan may cross execution.
pub unsafe fn match_args_safe<'a>(formals: Sexp<'a>, args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    if formals.clone().is_nil() {
        return Ok(args);
    }

    let factory = formals
        .node_factory()
        .or_else(|_| args.node_factory())
        .map_err(|err| sexp_err("argument matching owner", err))?;
    unsafe { match_closure_args(&factory, formals, args) }
}

/// Safe environment creation.
///
/// Creates a new environment with the given bindings as its frame
/// and the given parent as its enclosing environment.
/// # Safety
/// Activate the live owner of all inputs and retain their reachable graphs
/// through allocation and R reentry. No Rust payload loan may cross execution.
pub unsafe fn create_env_safe<'a>(bindings: Sexp<'a>, parent: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    let env = unsafe { NewEnvironment(bindings.as_raw(), parent.as_raw(), ptr::null_mut()) };
    unsafe { Sexp::try_from_raw(env) }.map_err(|err| sexp_err("failed to create environment", err))
}

// ---------------------------------------------------------------------------
// FFI closure functions — thin shims delegating to safe versions
// ---------------------------------------------------------------------------

/// Apply a closure to arguments.
///
/// This is the equivalent of R's `applyClosure()` from eval.c.
///
/// Parameters:
/// - call: the original call (for error reporting)
/// - op: the closure (CLOSXP)
/// - arglist: the evaluated or promised argument list
/// - rho: the calling environment
/// - suppliedenv: the environment of the caller (for sys.call/sys.parent)
pub unsafe fn applyClosure(
    call: SEXP,
    op: SEXP,
    arglist: SEXP,
    rho: SEXP,
    suppliedenv: SEXP,
    _R_verbose: c_int,
) -> SEXP {
    unsafe {
        applyClosureWithFrameVars(
            call,
            op,
            arglist,
            rho,
            suppliedenv,
            R_NilValue(),
            _R_verbose,
        )
    }
}

pub(crate) unsafe fn applyClosureWithFrameVars(
    call: SEXP,
    op: SEXP,
    arglist: SEXP,
    rho: SEXP,
    suppliedenv: SEXP,
    frame_vars: SEXP,
    _R_verbose: c_int,
) -> SEXP {
    unsafe {
        if op.is_null() || TYPEOF(op) != SEXPTYPE::CLOSXP {
            return R_NilValue();
        }

        let factory = super::dispatch::active_argument_factory();
        let arguments_owner = super::dispatch::argument_value(&factory, arglist);
        let environment_owner = super::dispatch::argument_value(&factory, rho);
        let _call_owner = super::dispatch::argument_value(&factory, call);

        // Keep the closure rooted while the JIT compiler allocates its code
        // and constant pool. Compilation installs BODY(op) only after the
        // complete bytecode object exists; an unsupported body stays source.
        let _op_guard = protect(op);
        super::jit::R_CheckJIT(op);



        // Upstream applyClosure_core passes the *promised* arguments
        // (`actuals = promiseArgs(arglist, rho)`) to begincontext as the
        // context's promargs. UseMethod, Recall, and NextMethod later
        // re-apply `cptr->promargs` when redispatching, so they must find
        // promises carrying the original caller's environment. Storing the
        // raw unevaluated expressions instead made those re-applications
        // rebuild the promises in the redispatch frame, losing caller-local
        // variables (e.g. the dispatched method's forced argument looked up
        // `v` in the generic's frame instead of the caller's).
        // Double-wrapping is transparent: forcing the outer promise
        // evaluates the inner one in its own recorded environment.
        let promised_args_owner =
            super::dispatch::promiseArgs(&factory, arguments_owner, environment_owner);
        let promised_args = promised_args_owner.as_raw();

        let newrho = make_applyClosure_env(call, op, arglist, rho);
        if newrho.is_null() || newrho == R_NilValue() {
            return R_NilValue();
        }

        let mut body = BODY(op);
        if body.is_null() {
            return R_NilValue();
        }
        if TYPEOF(body) == SEXPTYPE::BCODESXP {
            if let Some(source) = methods_matchsignature_source(op, body) {
                crate::sexp::accessors::SET_BODY(op, source);
                body = source;
            }
        }

        install_frame_vars(frame_vars, newrho);

        let sysparent = if suppliedenv.is_null() || suppliedenv == R_NilValue() {
            rho
        } else {
            suppliedenv
        };
        let ctx_guard = crate::sexp::context::begin_context_guard(
            crate::sexp::context::ctxt_flags::CTXT_FUNCTION
                | crate::sexp::context::ctxt_flags::CTXT_RETURN,
            call,
            newrho,
            sysparent,
            None,
            op,
            promised_args,
        );
        let ctx = ctx_guard.context();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(body, newrho)
        }));

        // A `return(v)` unwinds with RSignal::Return(v); extract the value so
        // it can be rooted before the on.exit expressions run. Non-Return
        // signals keep unwinding only after the handlers have run, matching
        // upstream's ordering (on.exits run before the jump).
        enum BodyOutcome {
            Value(SEXP),
            Returned(SEXP),
            Signal(Box<dyn std::any::Any + Send>),
        }
        let outcome = match result {
            Ok(val) => BodyOutcome::Value(val),
            Err(payload) => match payload.downcast::<crate::sexp::context::RSignal>() {
                Ok(signal) => match *signal {
                    crate::sexp::context::RSignal::Return(val) => BodyOutcome::Returned(val),
                    other => BodyOutcome::Signal(Box::new(other)),
                },
                Err(payload) => BodyOutcome::Signal(payload),
            },
        };

        if let BodyOutcome::Returned(val) = &outcome {
            if unsafe { (*ctx).jumped } == 0 {
                std::panic::panic_any(crate::sexp::context::RSignal::Return(*val));
            }
        }
        if let BodyOutcome::Value(val) = &outcome {
            unsafe { (*ctx).returnValue.replace_from_raw(*val); }
        }

        // Stock endcontext (context.c) saves R_Visible before running the
        // on.exit expressions and restores it afterwards, so the visibility
        // of the body's/handler's return value travels with
        // the owning context return field even when an on.exit expression evaluates (and
        // would otherwise clobber the flag). Mirror that save/restore here.
        let saved_visible = super::runtime::visible();
        let onexit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            crate::eval::context::R_run_onexits_for_context(ctx);
        }));
        super::runtime::set_visible(saved_visible);
        if let Err(payload) = onexit {
            return crate::sexp::context::handle_closure_signal(payload);
        }

        // Borrow the projection from the original return-value lease, which
        // remains owned while on.exit handlers allocate or collect.
        match outcome {
            BodyOutcome::Value(_) => unsafe {
                super::jit::handle_exec_continuation((*ctx).returnValue.as_raw())
            },
            BodyOutcome::Returned(_) => unsafe { super::jit::handle_exec_continuation((*ctx).returnValue.as_raw()) },
            BodyOutcome::Signal(payload) => crate::sexp::context::handle_closure_signal(payload),
        }
    }
}

unsafe fn install_frame_vars(mut vars: SEXP, rho: SEXP) {
    unsafe {
        while !vars.is_null() && vars != R_NilValue() {
            let tag = TAG(vars);
            if !tag.is_null() && tag != R_NilValue() {
                crate::sexp::envir::defineVar(tag, CAR(vars), rho);
            }
            vars = CDR(vars);
        }
    }
}

// ---------------------------------------------------------------------------
// make_applyClosure_env — create environment for closure application
// ---------------------------------------------------------------------------

/// Create the environment for a closure application.
///
unsafe fn env_on_enclos_chain(mut start: SEXP, needle: SEXP) -> bool {
    unsafe {
        if needle.is_null() {
            return false;
        }
        let empty = crate::sexp::globals::R_EmptyEnv();
        let mut hops = 0;
        while !start.is_null() && start != crate::sexp::globals::R_NilValue() && hops < 64 {
            if start == needle {
                return true;
            }
            if start == empty {
                break;
            }
            start = crate::sexp::accessors::ENCLOS(start);
            hops += 1;
        }
        false
    }
}

unsafe fn disconnected_methods_snapshot(cloenv: SEXP, empty: SEXP, base: SEXP) -> bool {
    unsafe {
        let parent = crate::sexp::accessors::ENCLOS(cloenv);
        parent.is_null() || parent == empty || parent == base
    }
}

unsafe fn methods_namespace_owns_closure(methods: SEXP, op: SEXP) -> bool {
    unsafe {
        if op.is_null() || crate::sexp::accessors::TYPEOF(op) != SEXPTYPE::CLOSXP {
            return false;
        }
        for name in [
            c"asMethodDefinition",
            c"makeGeneric",
            c"setGeneric",
            c"getGeneric",
            c"implicitGeneric",
            c"setMethod",
            c".derivedDefaultMethod",
            c"matchSignature",
            c".isSealedMethod",
            c".copyMethodDefaults",
            c"rematchDefinition",
            c".matchSigLength",
        ] {
            let mut bound = crate::sexp::envir::R_findVarInFrame(
                methods,
                crate::sexp::symbol::Rf_install(name.as_ptr()),
            );
            if bound.is_null() || bound == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            if TYPEOF(bound) == SEXPTYPE::PROMSXP {
                bound = crate::sexp::envir::forcePromise(bound);
            }
            if bound == op {
                return true;
            }
        }
        false
    }
}

unsafe fn remap_methods_snapshot_cloenv(op: SEXP, cloenv: SEXP) -> SEXP {
    unsafe {
        let Some(methods) = crate::mainutils::essentials::cached_namespace_by_name("methods")
        else {
            return cloenv;
        };
        if cloenv.is_null() || cloenv == methods {
            return cloenv;
        }
        let empty = crate::sexp::globals::R_EmptyEnv();
        let base = crate::sexp::globals::R_BaseEnv();
        let global = crate::sexp::globals::R_GlobalEnv();
        if cloenv == empty || cloenv == base || cloenv == global {
            return cloenv;
        }
        if env_on_enclos_chain(methods, cloenv) {
            return cloenv;
        }
        if crate::sexp::accessors::ENCLOS(cloenv) == methods {
            return cloenv;
        }
        // Lazy-load sometimes leaves methods helpers in an empty snapshot
        // whose parent is EmptyEnv/base. asMethodDefinition's default
        // `list(.anyClassName)` then cannot see the methods namespace.
        if disconnected_methods_snapshot(cloenv, empty, base)
            && methods_namespace_owns_closure(methods, op)
        {
            crate::sexp::accessors::SET_CLOENV(op, methods);
            return methods;
        }
        let all_mtable = crate::sexp::symbol::Rf_install(c".AllMTable".as_ptr());
        let tables = crate::sexp::envir::R_findVarInFrame(cloenv, all_mtable);
        let is_generic_snapshot = !tables.is_null()
            && tables != crate::sexp::globals::R_UnboundValue()
            && crate::sexp::accessors::TYPEOF(tables) == SEXPTYPE::ENVSXP;
        let is_method_def = crate::mainutils::coerce::IS_S4_OBJECT(op) != 0
            && crate::sexp::accessors::TYPEOF(op) == SEXPTYPE::CLOSXP;
        if is_method_def || is_generic_snapshot {
            let parent = crate::sexp::accessors::ENCLOS(cloenv);
            if parent.is_null() || parent == empty || parent == base {
                crate::sexp::accessors::SET_ENCLOS(cloenv, methods);
                crate::mainutils::essentials::bind_methods_base_primitives(cloenv);
                return cloenv;
            }
        }
        if !is_generic_snapshot {
            return cloenv;
        }
        let sg = crate::sexp::symbol::Rf_install(c"standardGeneric".as_ptr());
        let snap = crate::sexp::envir::R_findVarInFrame(cloenv, sg);
        let live = crate::sexp::envir::R_findVarInFrame(methods, sg);
        if is_function_sexp(live) && !is_function_sexp(snap) {
            crate::sexp::accessors::SET_ENCLOS(cloenv, methods);
        }
        crate::mainutils::essentials::bind_methods_base_primitives(cloenv);
        cloenv
    }
}

unsafe fn collect_unwrap_methods_closures(methods: SEXP) -> Vec<SEXP> {
    unsafe {
        let mut out = Vec::new();
        for name in [
            c"matchSignature",
            c".isSealedMethod",
            c".copyMethodDefaults",
            c"rematchDefinition",
            c"setMethod",
            c".matchSigLength",
            // Private JIT: rep("ANY", n) in .resetTable becomes n, so the
            // default method is stored as ANY#2 after a 2-arg setMethod.
            c".resetTable",
            c".fillSignatures",
            // Private JIT: rep(FALSE, length(found)) in .getGroupMethods
            // becomes a scalar length, so mget looks up Logic keys in the
            // member generic's empty table (`value for 'brob#ANY' not found`).
            c".getGroupMethods",
            c".findInheritedMethods",
            c".getAllGroups",
            // Private JIT: rep(2, length(contains)) in .inhDistances becomes
            // a scalar, so match() yields NA distances and
            // if(any(fromGroup[best])) is if(NA).
            c".inhDistances",
            c".leastMethodDistance",
            c".getBestMethods",
            c".disambiguateMethods",
            // Private JIT miscompiles S3Class <- c(cl, S3Class) / attr<-
            // so every setOldClass proto keeps .S3Class="oldClass" (rport-d4jyb).
            c"setOldClass",
            // Private JIT / GNU methods bytecode drops attr(funNames, "package")
            // so cacheMetaData's rep(packages, ...) sees a non-vector NULL.
            c".getGenerics",


        ] {
            let mut bound = crate::sexp::envir::R_findVarInFrame(
                methods,
                crate::sexp::symbol::Rf_install(name.as_ptr()),
            );
            if bound.is_null() || bound == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            if TYPEOF(bound) == SEXPTYPE::PROMSXP {
                bound = crate::sexp::envir::forcePromise(bound);
            }
            if !bound.is_null() && bound != crate::sexp::globals::R_UnboundValue() {
                out.push(bound);
            }
        }
        out
    }
}

pub(crate) unsafe fn is_methods_matchsignature_closure(op: SEXP) -> bool {
    unsafe {
        let Some(methods) = crate::mainutils::essentials::cached_namespace_by_name("methods")
        else {
            return false;
        };
        let hit = crate::sexp::instance::with_required_current_instance(|inst| {
            if (*inst).unwrap_methods_ns == methods {
                Some(
                    (*inst)
                        .unwrap_methods_closures
                        .iter()
                        .any(|&bound| bound == op),
                )
            } else {
                (*inst).unwrap_methods_ns = methods;
                (*inst).unwrap_methods_closures.clear();
                None
            }
        });
        if let Some(found) = hit {
            return found;
        }
        let built = collect_unwrap_methods_closures(methods);
        crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst).unwrap_methods_closures = built;
            (*inst)
                .unwrap_methods_closures
                .iter()
                .any(|&bound| bound == op)
        })

    }
}


unsafe fn methods_matchsignature_source(op: SEXP, body: SEXP) -> Option<SEXP> {
    unsafe {
        if !is_methods_matchsignature_closure(op) {
            return None;
        }
        let source = super::bc_eval::BCODE_EXPR(body);
        if !source.is_null()
            && source != crate::sexp::globals::R_NilValue()
            && TYPEOF(source) == SEXPTYPE::LANGSXP
        {
            Some(source)
        } else {
            None
        }
    }
}




fn is_function_sexp(value: SEXP) -> bool {
    let kind = unsafe { TYPEOF(value) };
    kind == SEXPTYPE::CLOSXP || kind == SEXPTYPE::BUILTINSXP || kind == SEXPTYPE::SPECIALSXP
}






/// This is a helper that separates environment creation from body evaluation.
unsafe fn reparent_empty_utils_runner(op: SEXP, cloenv: SEXP) -> SEXP {
    unsafe {
        let empty = crate::sexp::globals::R_EmptyEnv();
        if cloenv.is_null() {
            return cloenv;
        }
        let parent = crate::sexp::accessors::ENCLOS(cloenv);
        if !cloenv.is_null() && !parent.is_null() && parent != empty {
            return cloenv;
        }
        let Some(utils) = crate::mainutils::essentials::cached_namespace_by_name("utils") else {
            return cloenv;
        };
        let symbol = crate::sexp::symbol::Rf_install(c"RweaveLatexRuncode".as_ptr());
        let mut bound = crate::sexp::envir::R_findVarInFrame(utils, symbol);
        if crate::sexp::accessors::TYPEOF(bound) == crate::sexp::ffi::SEXPTYPE::PROMSXP {
            bound = crate::sexp::envir::forcePromise(bound);
        }
        if bound == op {
            crate::sexp::accessors::SET_ENCLOS(cloenv, utils);
        }
        cloenv
    }
}

pub unsafe fn make_applyClosure_env(call: SEXP, op: SEXP, arglist: SEXP, rho: SEXP) -> SEXP {
    // Retain the raw entry frame before method remapping or argument matching
    // can allocate and invoke the collector.
    let factory = unsafe { super::dispatch::active_argument_factory() };
    let _call_owner = super::dispatch::argument_value(&factory, call);
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        match (
            factory.wrap(op).ok(),
            Some(super::dispatch::argument_value(&factory, arglist)),
            Some(super::dispatch::argument_value(&factory, rho)),
        ) {
            (Some(closure), Some(args), Some(env)) => {
                if !closure.clone().is_closure() {
                    return R_NilValue();
                }

                let formals = match closure.clone().try_formals() {
                    Ok(f) => f,
                    Err(_) => return R_NilValue(),
                };
                let cloenv = match closure.try_cloenv() {
                    Ok(e) => e,
                    Err(_) => return R_NilValue(),
                };
                let cloenv = super::dispatch::argument_value(
                    &factory,
                    remap_methods_snapshot_cloenv(op, cloenv.as_raw()),
                );
                let cloenv = reparent_empty_utils_runner(op, cloenv.as_raw());
                let cloenv = super::dispatch::argument_value(&factory, cloenv);

                let promised_args_owner = crate::eval::dispatch::promiseArgs(&factory, args, env);
                let matched_owner =
                    match_closure_args(&factory, formals.clone(), promised_args_owner)
                        .unwrap_or_else(|message| {
                            crate::mainutils::errors::record_error_call(call, true);
                            crate::mainutils::errors::save_error_traceback();
                            std::panic::panic_any(crate::sexp::context::RSignal::Error { message })
                        });
                let matched = matched_owner.as_raw();

                let new_env = match create_env_safe(matched_owner, cloenv) {
                    Ok(e) => super::dispatch::argument_value(&factory, e.as_raw()),
                    Err(_) => return R_NilValue(),
                };


                install_default_promises(formals.as_raw(), matched, new_env.clone().as_raw());

                new_env.as_raw()
            }
            _ => R_NilValue(),
        }
    }))
    .unwrap_or_else(|payload| {
        if payload
            .downcast_ref::<crate::sexp::context::RSignal>()
            .is_some()
            || payload
                .downcast_ref::<crate::sexp::context::RError>()
                .is_some()
        {
            std::panic::resume_unwind(payload);
        }
        unsafe { R_NilValue() }
    })
}

unsafe fn formal_tag_name(formal_tag: SEXP) -> Option<String> {
    unsafe {
        if formal_tag.is_null() || TYPEOF(formal_tag) != SEXPTYPE::SYMSXP {
            return None;
        }
        let pname = PRINTNAME(formal_tag);
        if pname.is_null() || pname == R_NilValue() {
            return None;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            return None;
        }
        Some(CStr::from_ptr(chars).to_string_lossy().into_owned())
    }
}

/// Port of R's `matchArgs_NR` (r-source/src/main/match.c).
///
/// Matches the supplied argument list against the formals using, in order:
/// 1. exact tag matching,
/// 2. partial (prefix) tag matching — exact matching is required after the
///    first `...` formal,
/// 3. positional matching of untagged values to unmatched non-`...` formals.
///
/// Any remaining unused arguments are collected into the first `...` formal as
/// a DOTSXP; with no `...` formal present, an "unused arguments" error is
/// raised. The returned pairlist has one element per formal, in formal order,
/// each holding the matched value or `R_MissingArg`.
pub(super) unsafe fn match_closure_args<'s>(
    factory: &SessionNodeFactory<'s>,
    formals: Sexp<'s>,
    supplied: Sexp<'s>,
) -> Result<Sexp<'s>, String> {
    factory
        .require_active()
        .map_err(|error| sexp_err("argument matching owner", error))?;
    factory
        .link(&formals)
        .and_then(|_| factory.link(&supplied))
        .map_err(|error| sexp_err("argument matching domain", error))?;
    // Each indexed cell owns its root independently. A callback may detach
    // cells from either original chain while warning or allocating a result.
    let supplied_cells: Vec<_> = PairlistIter::new(supplied).collect();
    let formal_cells: Vec<_> = PairlistIter::new(formals).collect();
    let mut used = vec![0u8; supplied_cells.len()];
    let mut fargused = vec![false; formal_cells.len()];
    let missing = factory.missing();
    let dots_symbol = unsafe { R_DotsSymbol() };
    let mut result_owner = PairlistBuilder::from_factory(factory.clone());
    let mut result_cells = Vec::with_capacity(formal_cells.len());
    for formal in &formal_cells {
        let tag = formal
            .try_tag()
            .map_err(|error| sexp_err("formal tag", error))?;
        let tag = if tag.is_nil() { None } else { Some(tag) };
        result_cells.push(
            result_owner
                .push_cell(missing.clone(), tag)
                .map_err(|error| sexp_err("matched argument cell", error))?,
        );
    }

    // First pass: exact tag matching.
    for (formal_idx, formal) in formal_cells.iter().enumerate() {
        let ftag = formal
            .try_tag()
            .map_err(|error| sexp_err("formal tag", error))?;
        if ftag.is_nil() || ftag.as_raw() == dots_symbol {
            continue;
        }
        if let Some(ftag_name) = unsafe { formal_tag_name(ftag.as_raw()) } {
            for (index, supplied) in supplied_cells.iter().enumerate() {
                let btag = supplied
                    .try_tag()
                    .map_err(|error| sexp_err("supplied tag", error))?;
                if btag.is_nil() {
                    continue;
                }
                let Some(btag_name) = (unsafe { formal_tag_name(btag.as_raw()) }) else {
                    continue;
                };
                if ftag_name != btag_name {
                    continue;
                }
                if fargused[formal_idx] {
                    return Err(format!(
                        "formal argument \"{ftag_name}\" matched by multiple actual arguments"
                    ));
                }
                if used[index] == 2 {
                    return Err(format!(
                        "argument {} matches multiple formal arguments",
                        index + 1
                    ));
                }
                let value = supplied
                    .try_car()
                    .map_err(|error| sexp_err("supplied value", error))?;
                unsafe {
                    SETCAR(result_cells[formal_idx].as_raw(), value.as_raw());
                }
                used[index] = 2;
                fargused[formal_idx] = true;
            }
        }
    }

    // Second pass: partial tags; exact matching remains required after ... .
    let mut dots_formal_index = None;
    let mut seen_dots = false;
    for (formal_idx, formal) in formal_cells.iter().enumerate() {
        if fargused[formal_idx] {
            continue;
        }
        let ftag = formal
            .try_tag()
            .map_err(|error| sexp_err("formal tag", error))?;
        if ftag.as_raw() == dots_symbol && !seen_dots {
            dots_formal_index = Some(formal_idx);
            seen_dots = true;
        } else if !seen_dots {
            if let Some(ftag_name) = unsafe { formal_tag_name(ftag.as_raw()) } {
                for (index, supplied) in supplied_cells.iter().enumerate() {
                    let btag = supplied
                        .try_tag()
                        .map_err(|error| sexp_err("supplied tag", error))?;
                    if btag.is_nil() || used[index] == 2 {
                        continue;
                    }
                    let Some(btag_name) = (unsafe { formal_tag_name(btag.as_raw()) }) else {
                        continue;
                    };
                    if !ftag_name.starts_with(btag_name.as_str()) {
                        continue;
                    }
                    if used[index] != 0 {
                        return Err(format!(
                            "argument {} matches multiple formal arguments",
                            index + 1
                        ));
                    }
                    if fargused[formal_idx] {
                        return Err(format!(
                            "formal argument \"{ftag_name}\" matched by multiple actual arguments"
                        ));
                    }
                    let value = supplied
                        .try_car()
                        .map_err(|error| sexp_err("supplied value", error))?;
                    unsafe {
                        crate::mainutils::match_mod::R_warn_partial_match_args(
                            factory.nil().as_raw(),
                            btag.as_raw(),
                            ftag.as_raw(),
                        );
                        SETCAR(result_cells[formal_idx].as_raw(), value.as_raw());
                    }
                    used[index] = 1;
                    fargused[formal_idx] = true;
                }
            }
        }
    }

    // Third pass: positional matching stops at the first ... formal.
    let mut formal_idx = 0;
    let mut supplied_idx = 0;
    while formal_idx < formal_cells.len() && supplied_idx < supplied_cells.len() {
        let formal_tag = formal_cells[formal_idx]
            .try_tag()
            .map_err(|error| sexp_err("formal tag", error))?;
        if formal_tag.as_raw() == dots_symbol {
            break;
        }
        let matched = result_cells[formal_idx]
            .try_car()
            .map_err(|error| sexp_err("matched value", error))?;
        if matched.as_raw() != missing.as_raw() {
            formal_idx += 1;
        } else if used[supplied_idx] != 0
            || !supplied_cells[supplied_idx]
                .try_tag()
                .map_err(|error| sexp_err("supplied tag", error))?
                .is_nil()
        {
            supplied_idx += 1;
        } else {
            let value = supplied_cells[supplied_idx]
                .try_car()
                .map_err(|error| sexp_err("supplied value", error))?;
            unsafe {
                SETCAR(result_cells[formal_idx].as_raw(), value.as_raw());
            }
            used[supplied_idx] = 1;
            fargused[formal_idx] = true;
            supplied_idx += 1;
            formal_idx += 1;
        }
    }

    if let Some(dots_idx) = dots_formal_index {
        let mut dots = PairlistBuilder::from_factory(factory.clone());
        let mut collected = 0;
        for (index, supplied) in supplied_cells.iter().enumerate() {
            if used[index] != 0 {
                continue;
            }
            used[index] = 1;
            let tag = supplied
                .try_tag()
                .map_err(|error| sexp_err("supplied tag", error))?;
            let tag = if tag.is_nil() { None } else { Some(tag) };
            let value = supplied
                .try_car()
                .map_err(|error| sexp_err("supplied value", error))?;
            dots.push(value, tag)
                .map_err(|error| sexp_err("dots argument pairlist build", error))?;
            collected += 1;
        }
        if collected == 0 {
            unsafe {
                SETCAR(result_cells[dots_idx].as_raw(), missing.as_raw());
            }
        } else {
            let dots = dots
                .finish_as_type(SEXPTYPE::DOTSXP)
                .map_err(|error| sexp_err("dots argument pairlist wrap", error))?;
            unsafe {
                SETCAR(result_cells[dots_idx].as_raw(), dots.as_raw());
            }
        }
    } else {
        let mut unused = Vec::new();
        for (index, supplied) in supplied_cells.iter().enumerate() {
            if used[index] != 0 {
                continue;
            }
            let mut value = supplied
                .try_car()
                .map_err(|error| sexp_err("unused value", error))?;
            if value.typeof_() == SEXPTYPE::PROMSXP {
                value = value
                    .try_prcode()
                    .map_err(|error| sexp_err("unused promise code", error))?;
            }
            let deparsed = deparse_for_error(value.as_raw());
            let tag = supplied
                .try_tag()
                .map_err(|error| sexp_err("supplied tag", error))?;
            let item = match unsafe { formal_tag_name(tag.as_raw()) } {
                Some(tag) => format!("{tag} = {deparsed}"),
                None => deparsed,
            };
            unused.push(item);
        }
        if !unused.is_empty() {
            return Err(if unused.len() == 1 {
                format!("unused argument ({})", unused[0])
            } else {
                format!("unused arguments ({})", unused.join(", "))
            });
        }
    }
    // Defaults retain MISSING=1 even after their promise is installed.
    for cell in &result_cells {
        if cell
            .try_car()
            .map_err(|error| sexp_err("matched value", error))?
            .as_raw()
            == missing.as_raw()
        {
            unsafe {
                crate::sexp::accessors::SET_MISSING(cell.as_raw(), 1);
            }
        }
    }
    result_owner
        .finish()
        .map_err(|error| sexp_err("matched argument pairlist", error))
}

fn deparse_for_error(expr: SEXP) -> String {
    unsafe {
        let text = crate::mainutils::deparse::deparse1line(expr, false);
        if text.is_null() || text == R_NilValue() || XLENGTH(text) == 0 {
            return String::new();
        }
        let chars = CHAR(STRING_ELT(text, 0));
        if chars.is_null() {
            return String::new();
        }
        CStr::from_ptr(chars).to_string_lossy().into_owned()
    }
}

unsafe fn install_default_promises(formals: SEXP, frame: SEXP, new_env: SEXP) {
    unsafe {
        let mut formal = formals;
        let mut actual = frame;

        while !formal.is_null()
            && formal != R_NilValue()
            && !actual.is_null()
            && actual != R_NilValue()
        {
            if CAR(actual) == R_MissingArg() && CAR(formal) != R_MissingArg() {
                SETCAR(actual, mkPROMISE(CAR(formal), new_env));
            }
            formal = CDR(formal);
            actual = CDR(actual);
        }
    }
}

// ---------------------------------------------------------------------------
// R_execClosure — execute a closure body in a new environment
// ---------------------------------------------------------------------------

/// Execute a closure, returning the result.
///
/// Uses catch_unwind for error recovery.
pub unsafe fn R_execClosure(
    op: SEXP,
    arglist: SEXP,
    rho: SEXP,
) -> Result<SEXP, crate::sexp::context::RError> {
    unsafe {
        let newrho = make_applyClosure_env(
            crate::mainutils::errors::R_getCurrentCall(),
            op,
            arglist,
            rho,
        );
        if newrho.is_null() || newrho == R_NilValue() {
            return Err(crate::sexp::context::RError {
                message: "failed to create closure environment".to_string(),
            });
        }

        let body = BODY(op);

        // Use catch_unwind for error recovery
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Rf_eval(body, newrho)));

        match result {
            Ok(val) => Ok(val),
            Err(payload) => {
                if let Some(err) = payload.downcast_ref::<crate::sexp::context::RError>() {
                    Err(crate::sexp::context::RError {
                        message: err.message.clone(),
                    })
                } else if let Some(signal) = payload.downcast_ref::<crate::sexp::context::RSignal>()
                {
                    match signal {
                        crate::sexp::context::RSignal::Error { message } => {
                            Err(crate::sexp::context::RError {
                                message: message.clone(),
                            })
                        }
                        _ => std::panic::resume_unwind(payload),
                    }
                } else {
                    Err(crate::sexp::context::RError {
                        message: "unknown error".to_string(),
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod owned_matcher_tests {
    use super::*;
    use crate::sexp::session::RSession;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn owned_matcher_retains_cells_detached_by_reentrant_gc_callbacks() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let mut formals = PairlistBuilder::from_factory(factory.clone());
        let mut arguments = PairlistBuilder::from_factory(factory.clone());
        for (name, number) in [(c"first", 10), (c"second", 20), (c"third", 30)] {
            let tag = factory
                .wrap(unsafe { crate::sexp::symbol::Rf_install(name.as_ptr()) })
                .unwrap();
            formals.push(factory.missing(), Some(tag)).unwrap();
            let value = factory
                .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(number) })
                .unwrap();
            arguments.push(value, None).unwrap();
        }
        let formals = formals.finish().unwrap();
        let arguments = arguments.finish().unwrap();
        let original_formals = formals.as_raw();
        let original_arguments = arguments.as_raw();
        let nil = factory.nil().as_raw();
        let detached = Rc::new(Cell::new(false));
        let changed = detached.clone();
        let notifications = Rc::new(Cell::new(0));
        let observed = notifications.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            if !changed.replace(true) {
                // The caller retains both original heads for this callback.
                // Every detached indexed cell must now be owned by the matcher.
                unsafe {
                    SETCDR(original_formals, nil);
                    SETCDR(original_arguments, nil);
                }
            }
            crate::sexp::gengc::full_gc();
        }));
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        let before = crate::sexp::protect::R_ProtectCount();
        let matched =
            unsafe { match_closure_args(&factory, formals.clone(), arguments.clone()) }.unwrap();
        session.with_active_in(|instance| unsafe {
            (*instance).memory_state.gc_force_gap = 0;
        });
        assert!(detached.get());
        assert!(formals.try_cdr().unwrap().is_nil());
        assert!(arguments.try_cdr().unwrap().is_nil());
        drop(formals);
        drop(arguments);
        crate::sexp::gengc::full_gc();
        let cells: Vec<_> = PairlistIter::new(matched).collect();
        assert_eq!(cells.len(), 3);
        let values: Vec<_> = cells
            .iter()
            .map(|cell| cell.try_car().unwrap().integer_elt(0).unwrap())
            .collect();
        assert_eq!(values, [10, 20, 30]);
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
        assert_eq!(tags, ["first", "second", "third"]);
        assert!(notifications.get() >= 3);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
    }

    #[test]
    fn owned_matcher_partial_warning_collects_unwinds_and_retries() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = SessionNodeFactory::new(owner);
        let global = session.global_env().unwrap();
        session.with_active_in(|instance| unsafe {
            (*instance).eval_state.jit_enabled = 0;
        });
        let parse = |input: &str| {
            owner
                .with_arena(|arena| crate::eval::parser::parse(input, arena, factory.clone()))
                .unwrap()
                .unwrap()
        };
        let function_expression = parse("function(alpha) alpha");
        let function = factory
            .wrap(unsafe { Rf_eval(function_expression.as_raw(), global.as_raw()) })
            .unwrap();
        let selected = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(37) })
            .unwrap();
        let handled = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(0) })
            .unwrap();
        let symbol = |name: &CStr| {
            factory
                .wrap(unsafe { crate::sexp::symbol::Rf_install(name.as_ptr()) })
                .unwrap()
        };
        let function_name = symbol(c"f");
        let selected_name = symbol(c"selected");
        let handled_name = symbol(c"handled");
        unsafe {
            crate::sexp::envir::defineVar(
                function_name.as_raw(),
                function.as_raw(),
                global.as_raw(),
            );
            crate::sexp::envir::defineVar(
                selected_name.as_raw(),
                selected.as_raw(),
                global.as_raw(),
            );
            crate::sexp::envir::defineVar(handled_name.as_raw(), handled.as_raw(), global.as_raw());
            crate::mainutils::options::SetOptionByName(
                "warnPartialMatchArgs",
                factory
                    .wrap(crate::sexp::constructors::Rf_ScalarLogical(1))
                    .unwrap()
                    .as_raw(),
            );
        }
        let expression = parse(
            "withCallingHandlers(f(al=selected), warning=function(w) { handled <<- handled + 1L; gc() })",
        );
        // Keep the actual selected formal, supplied tag, and argument value
        // independently owned across the warning handler and error unwind.
        let formal = function.try_formals().unwrap();
        let formal_tag = formal.try_tag().unwrap();
        let call = expression.try_cdr().unwrap().try_car().unwrap();
        let supplied = call.try_cdr().unwrap();
        let supplied_tag = supplied.try_tag().unwrap();
        let supplied_value = supplied.try_car().unwrap();
        let notifications = Rc::new(Cell::new(0));
        let observed = notifications.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        let set_warn = |level| unsafe {
            let value = factory
                .wrap(crate::sexp::constructors::Rf_ScalarInteger(level))
                .unwrap();
            crate::mainutils::options::SetOptionByName("warn", value.as_raw());
        };
        let runtime_state = || {
            session.with_active_in(|instance| unsafe {
                (
                    (*instance).context_stack.len(),
                    (*instance).error_state.handler_stack,
                    (*instance).error_state.restart_stack,
                    (*instance).error_state.in_warning,
                )
            })
        };
        let handled_count = || unsafe {
            let count = factory
                .wrap(crate::sexp::envir::R_findVar(
                    handled_name.as_raw(),
                    global.as_raw(),
                ))
                .unwrap();
            count.integer_elt(0).unwrap()
        };
        set_warn(0);
        let before_protection = crate::sexp::protect::R_ProtectCount();
        let before_runtime = runtime_state();
        let first = factory
            .wrap(unsafe { Rf_eval(expression.as_raw(), global.as_raw()) })
            .unwrap();
        assert_eq!(first.integer_elt(0).unwrap(), 37);
        assert_eq!(handled_count(), 1);
        assert!(notifications.get() > 0);
        assert!(crate::mainutils::essentials::warning_handler_invoked());
        assert_eq!(runtime_state(), before_runtime);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before_protection);

        set_warn(2);
        let collections_before_error = notifications.get();
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            Rf_eval(expression.as_raw(), global.as_raw())
        }));
        let payload = error.expect_err("warn=2 must unwind after the collecting calling handler");
        let message = if let Some(error) = payload.downcast_ref::<crate::sexp::context::RError>() {
            &error.message
        } else if let Some(crate::sexp::context::RSignal::Error { message }) =
            payload.downcast_ref::<crate::sexp::context::RSignal>()
        {
            message
        } else {
            std::panic::resume_unwind(payload);
        };
        assert!(message.contains("converted from warning"), "{message}");
        assert!(
            message.contains("partial argument match of 'al' to 'alpha'"),
            "{message}"
        );
        assert_eq!(handled_count(), 2);
        assert!(notifications.get() > collections_before_error);
        assert_eq!(runtime_state(), before_runtime);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before_protection);

        set_warn(0);
        let collections_before_retry = notifications.get();
        let retry = factory
            .wrap(unsafe { Rf_eval(expression.as_raw(), global.as_raw()) })
            .unwrap();
        crate::sexp::gengc::full_gc();
        assert_eq!(retry.integer_elt(0).unwrap(), 37);
        assert_eq!(first.integer_elt(0).unwrap(), 37);
        assert_eq!(selected.integer_elt(0).unwrap(), 37);
        assert_eq!(
            formal_tag.try_printname().unwrap().try_as_string().unwrap(),
            "alpha"
        );
        assert_eq!(
            supplied_tag
                .try_printname()
                .unwrap()
                .try_as_string()
                .unwrap(),
            "al"
        );
        assert_eq!(
            supplied_value
                .try_printname()
                .unwrap()
                .try_as_string()
                .unwrap(),
            "selected"
        );
        assert_eq!(handled_count(), 3);
        assert!(notifications.get() > collections_before_retry);
        assert_eq!(runtime_state(), before_runtime);
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before_protection);
    }
}
