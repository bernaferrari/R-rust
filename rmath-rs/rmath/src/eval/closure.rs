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
    BODY, CAR, CDR, CHAR, PRCODE, PRINTNAME, SETCAR, SETCDR, STRING_ELT, TAG, TYPEOF, XLENGTH,
};
use crate::sexp::envir::{Environment, addMissingVarsToNewEnv};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::memory_ext::{CONS_NR, NewEnvironment, mkPROMISE};
use crate::sexp::object::{PairlistBuilder, PairlistIter, Sexp, SexpError};
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
pub fn apply_closure_safe<'a>(
    closure: Sexp<'a>,
    args: Sexp<'a>,
    rho: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    if !closure.clone().is_closure() {
        return Err("not a closure".to_string());
    }

    let formals = closure
        .clone()
        .try_formals()
        .clone()
        .map_err(|err| sexp_err("closure formals lookup", err))?;
    let body = closure
        .clone()
        .try_body()
        .clone()
        .map_err(|err| sexp_err("closure body lookup", err))?;
    let cloenv = closure
        .try_cloenv()
        .map_err(|err| sexp_err("closure environment lookup", err))?;

    // Match arguments to formals
    let matched = match_args_safe(formals.clone(), args.clone())?;

    // Create new environment with matched arguments
    let new_env = create_env_safe(matched, cloenv)?;

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
            new_env_bindings.clone().define(sym, val).clone()?;
        }
    }

    // Add missing arguments
    unsafe {
        addMissingVarsToNewEnv(formals.as_raw(), args.as_raw(), new_env.clone().as_raw());
    }

    // Evaluate body in new environment.
    // If the body was compiled to BCODESXP (via auto in do_function or compile_closure),
    // the top-level eval_safe dispatch (EvalKind::Bytecode) will call bcEval for the fast VM path.
    crate::eval::eval::eval_safe(body, new_env)
}

/// Safe argument matching using Sexp<'a> and PairlistIter.
///
/// Matches actual arguments to formal parameters, building a new
/// pairlist with the matched values.
pub fn match_args_safe<'a>(formals: Sexp<'a>, args: Sexp<'a>) -> Result<Sexp<'a>, String> {
    if formals.clone().is_nil() {
        return Ok(args);
    }

    unsafe { match_closure_args(formals.as_raw(), args.as_raw()) }.and_then(|matched| {
        Sexp::try_from_raw(matched).map_err(|err| sexp_err("matched argument wrap", err))
    })
}

/// Safe environment creation.
///
/// Creates a new environment with the given bindings as its frame
/// and the given parent as its enclosing environment.
pub fn create_env_safe<'a>(bindings: Sexp<'a>, parent: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let env = unsafe { NewEnvironment(bindings.as_raw(), parent.as_raw(), ptr::null_mut()) };
    Sexp::try_from_raw(env).map_err(|err| sexp_err("failed to create environment", err))
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

        let newrho = make_applyClosure_env(op, arglist, rho);
        if newrho.is_null() || newrho == R_NilValue() {
            return R_NilValue();
        }

        let body = BODY(op);
        if body.is_null() {
            return R_NilValue();
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
            arglist,
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

        // Upstream eval.c stores the return value in cntxt.returnValue before
        // endcontext runs the on.exit expressions, implicitly protecting it
        // against a gc() called from a handler. The collector roots
        // RCNTXT::returnValue (mark/update in gengc.rs), so parking the value
        // there keeps it alive for the duration of the handlers.
        if let BodyOutcome::Value(val) | BodyOutcome::Returned(val) = &outcome {
            (*ctx).returnValue = *val;
        }

        // Stock endcontext (context.c) saves R_Visible before running the
        // on.exit expressions and restores it afterwards, so the visibility
        // of the body's/handler's return value travels with
        // (*ctx).returnValue even when an on.exit expression evaluates (and
        // would otherwise clobber the flag). Mirror that save/restore here.
        let saved_visible = super::runtime::visible();
        let onexit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            crate::eval::context::R_run_onexits_for_context(ctx);
        }));
        super::runtime::set_visible(saved_visible);
        if let Err(payload) = onexit {
            return crate::sexp::context::handle_closure_signal(payload);
        }

        // Re-read through the context: a handler's gc() may have moved the
        // value, and gengc.rs rewrote (*ctx).returnValue to the new location.
        match outcome {
            BodyOutcome::Value(_) => unsafe {
                super::jit::handle_exec_continuation((*ctx).returnValue)
            },
            BodyOutcome::Returned(_) => (*ctx).returnValue,
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
/// This is a helper that separates environment creation from body evaluation.
pub unsafe fn make_applyClosure_env(op: SEXP, arglist: SEXP, rho: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        match (
            Sexp::from_raw(op),
            Sexp::from_raw(arglist),
            Sexp::from_raw(rho),
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

                let promised_args = crate::eval::dispatch::promiseArgs(arglist, rho);
                let matched = match_closure_args(formals.clone().as_raw(), promised_args)
                    .unwrap_or_else(|message| {
                        std::panic::panic_any(crate::sexp::context::RSignal::Error { message })
                    });

                let new_env = match create_env_safe(Sexp::from_raw_unchecked(matched), cloenv) {
                    Ok(e) => e,
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
        if formal_tag.is_null() || formal_tag == R_NilValue() {
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
unsafe fn match_closure_args(formals: SEXP, supplied: SEXP) -> Result<SEXP, String> {
    unsafe {
        // Snapshot the supplied cells so we can index them alongside a
        // parallel `used` flag vector (upstream uses ARGUSED on the cells).
        let mut supplied_cells = Vec::new();
        let mut cur = supplied;
        while !cur.is_null() && cur != R_NilValue() {
            supplied_cells.push(cur);
            cur = CDR(cur);
        }
        // 0 = unused, 1 = partially matched, 2 = exactly matched.
        let mut used = vec![0u8; supplied_cells.len()];
        // fargused[i]: whether formal i has been matched (and how many times).
        let formal_count = {
            let mut n = 0usize;
            let mut f = formals;
            while !f.is_null() && f != R_NilValue() {
                n += 1;
                f = CDR(f);
            }
            n
        };
        let mut fargused = vec![false; formal_count];

        // Build the result as a chain of cells (one per formal, all initially
        // R_MissingArg), mirroring upstream matchArgs_NR's `actuals`. Each
        // cell carries its formal name as TAG: this pairlist becomes the new
        // environment's frame, whose lookups are tag-based.
        let mut result_cells: Vec<SEXP> = Vec::with_capacity(formal_count);
        {
            let mut f = formals;
            while !f.is_null() && f != R_NilValue() {
                let cell = CONS_NR(R_MissingArg(), R_NilValue());
                if cell.is_null() || cell == R_NilValue() {
                    return Err("failed to allocate matched argument cell".to_string());
                }
                if let Some(&last) = result_cells.last() {
                    SETCDR(last, cell);
                }
                let ftag = TAG(f);
                if !ftag.is_null() && ftag != R_NilValue() {
                    crate::sexp::accessors::SETTAG(cell, ftag);
                }
                result_cells.push(cell);
                f = CDR(f);
            }
        }

        // First pass: exact matches by tag.
        {
            let mut formal_idx = 0usize;
            let mut f = formals;
            while !f.is_null() && f != R_NilValue() {
                let ftag = TAG(f);
                if !ftag.is_null() && ftag != R_NilValue() && ftag != R_DotsSymbol() {
                    if let Some(ftag_name) = formal_tag_name(ftag) {
                        for i in 0..supplied_cells.len() {
                            let btag = TAG(supplied_cells[i]);
                            if btag.is_null() || btag == R_NilValue() {
                                continue;
                            }
                            let Some(btag_name) = formal_tag_name(btag) else {
                                continue;
                            };
                            if ftag_name == btag_name {
                                if fargused[formal_idx] {
                                    return Err(format!(
                                        "formal argument \"{ftag_name}\" matched by multiple actual arguments"
                                    ));
                                }
                                if used[i] == 2 {
                                    return Err(format!(
                                        "argument {} matches multiple formal arguments",
                                        i + 1
                                    ));
                                }
                                SETCAR(result_cells[formal_idx], CAR(supplied_cells[i]));
                                used[i] = 2;
                                fargused[formal_idx] = true;
                            }
                        }
                    }
                }
                f = CDR(f);
                formal_idx += 1;
            }
        }

        // Second pass: partial matches based on tags. An exact match is
        // required after the first ... ; its location is recorded so the ...
        // can gobble remaining args later.
        let mut dots_formal_index: Option<usize> = None;
        let mut seen_dots = false;
        {
            let mut formal_idx = 0usize;
            let mut f = formals;
            while !f.is_null() && f != R_NilValue() {
                if !fargused[formal_idx] {
                    let ftag = TAG(f);
                    if ftag == R_DotsSymbol() && !seen_dots {
                        // Record where ... value goes.
                        dots_formal_index = Some(formal_idx);
                        seen_dots = true;
                    } else if !seen_dots {
                        if let Some(ftag_name) = formal_tag_name(ftag) {
                            for i in 0..supplied_cells.len() {
                                let btag = TAG(supplied_cells[i]);
                                if btag.is_null() || btag == R_NilValue() || used[i] == 2 {
                                    continue;
                                }
                                let Some(btag_name) = formal_tag_name(btag) else {
                                    continue;
                                };
                                // Upstream psmatch: the supplied tag may be a
                                // prefix of the formal's name.
                                if ftag_name.starts_with(btag_name.as_str()) {
                                    if used[i] != 0 {
                                        return Err(format!(
                                            "argument {} matches multiple formal arguments",
                                            i + 1
                                        ));
                                    }
                                    if fargused[formal_idx] {
                                        return Err(format!(
                                            "formal argument \"{ftag_name}\" matched by multiple actual arguments"
                                        ));
                                    }
                                    crate::mainutils::match_mod::R_warn_partial_match_args(
                                        R_NilValue(),
                                        btag,
                                        ftag,
                                    );
                                    SETCAR(result_cells[formal_idx], CAR(supplied_cells[i]));
                                    used[i] = 1;
                                    fargused[formal_idx] = true;
                                }
                            }
                        }
                    }
                }
                f = CDR(f);
                formal_idx += 1;
            }
        }

        // Third pass: matches based on order. All tagged args have now been
        // matched. Bind untagged values in order to any unmatched formals,
        // stopping at the first ... (which gobbles all remaining args below).
        {
            let mut formal_idx = 0usize;
            let mut b = 0usize;
            let mut f = formals;
            let mut seendots = false;
            while !f.is_null() && f != R_NilValue() && b < supplied_cells.len() && !seendots {
                let ftag = TAG(f);
                if ftag == R_DotsSymbol() {
                    seendots = true;
                    f = CDR(f);
                    formal_idx += 1;
                } else if !is_missing_car(result_cells[formal_idx]) {
                    // Already matched by tag: skip to next formal.
                    f = CDR(f);
                    formal_idx += 1;
                } else if used[b] != 0 || !tag_is_nil(TAG(supplied_cells[b])) {
                    // This value is used or tagged: skip to next value.
                    b += 1;
                } else {
                    // Positional match.
                    SETCAR(result_cells[formal_idx], CAR(supplied_cells[b]));
                    used[b] = 1;
                    fargused[formal_idx] = true;
                    b += 1;
                    f = CDR(f);
                    formal_idx += 1;
                }
            }
        }

        // Finally: gobble up all unused actuals into ..., or error.
        if let Some(dots_idx) = dots_formal_index {
            let mut dots = PairlistBuilder::new();
            for i in 0..supplied_cells.len() {
                if used[i] != 0 {
                    continue;
                }
                used[i] = 1;
                let tag_raw = TAG(supplied_cells[i]);
                let tag = if tag_raw.is_null() || tag_raw == R_NilValue() {
                    None
                } else {
                    Some(Sexp::from_raw_unchecked(tag_raw))
                };
                dots.push(Sexp::from_raw_unchecked(CAR(supplied_cells[i])), tag)
                    .map_err(|err| sexp_err("dots argument pairlist build", err))?;
            }
            let dots_value = dots
                .finish_as_type(SEXPTYPE::DOTSXP)
                .map_err(|err| sexp_err("dots argument pairlist wrap", err))?;
            SETCAR(result_cells[dots_idx], dots_value.as_raw());
        } else {
            // Show bad arguments in the call without evaluating them:
            // unwrap promises back to their expressions for deparsing.
            // Stock match.c reports every unused argument, singular
            // ("unused argument (y = 1)") or plural
            // ("unused arguments (x = 1, y = 2)").
            let mut unused: Vec<String> = Vec::new();
            for i in 0..supplied_cells.len() {
                if used[i] != 0 {
                    continue;
                }
                let mut car_b = CAR(supplied_cells[i]);
                if TYPEOF(car_b) == SEXPTYPE::PROMSXP {
                    car_b = PRCODE(car_b);
                }
                let deparsed = deparse_for_error(car_b);
                let item = match formal_tag_name(TAG(supplied_cells[i])) {
                    Some(tag) => format!("{tag} = {deparsed}"),
                    None => deparsed,
                };
                unused.push(item);
            }
            if !unused.is_empty() {
                if unused.len() == 1 {
                    return Err(format!("unused argument ({})", unused[0]));
                }
                return Err(format!("unused arguments ({})", unused.join(", ")));
            }
        }

        // Return the head of the matched-arguments chain.
        Ok(if result_cells.is_empty() {
            R_NilValue()
        } else {
            result_cells[0]
        })
    }
}

unsafe fn is_missing_car(cell: SEXP) -> bool {
    unsafe { CAR(cell) == R_MissingArg() }
}

unsafe fn tag_is_nil(tag: SEXP) -> bool {
    unsafe { tag.is_null() || tag == R_NilValue() }
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
        let newrho = make_applyClosure_env(op, arglist, rho);
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
