//! Safe evaluation entry points and expression classification — extracted verbatim from eval/eval.rs

use super::*;

/// Evaluate an expression using owner-scoped Rust handles.
///
/// This function is the Rust-shaped evaluator entrypoint. It performs the
/// evaluator-side cancellation/visibility setup that the legacy raw `Rf_eval`
/// shim used to own, then delegates to the safe evaluator implementation.
/// # Safety
/// Activate the live owner of all inputs and retain their reachable graphs
/// through allocation and R reentry. No Rust payload loan may cross execution.
pub unsafe fn eval_expr<'a>(expr: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let _timer = EvalTimerGuard::start_if_needed();
    crate::sexp::instance::check_cancellation();
    super::super::runtime::set_visible(TRUE);

    match unsafe {
        /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
        eval_safe(expr.clone(), env)
    } {
        Ok(result) => Ok(result),
        Err(message) if is_simple_warning_hook_call(expr) => Ok(Sexp::nil()),
        Err(message) => Err(message),
    }
}

/// Typed evaluation of an R expression through the legacy core.
///
/// It catches R error signals and returns Result. The historical `_safe`
/// suffix describes error handling; direct calls still require unsafe authority.
///
/// # Arguments
/// * `expr` - The expression to evaluate
/// * `env` - The environment in which to evaluate
///
/// # Returns
/// * `Ok(Sexp)` - The result of evaluation
/// * `Err(String)` - A description of the error that occurred
/// # Safety
/// Activate the live owner of all inputs and retain their reachable graphs
/// through allocation and R reentry. No Rust payload loan may cross execution.
pub unsafe fn eval_safe<'a>(expr: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
    // Capture once and install automatic roots before the first callback.
    // Raw entry points are normalized here into the actual active heap.
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
        .map_err(|err| sexp_err("evaluation owner", err))?;
    let factory = crate::sexp::object::SessionNodeFactory::new(owner);
    let expr = factory
        .wrap(expr.as_raw())
        .map_err(|err| sexp_err("expression", err))?;
    let env = factory
        .wrap(env.as_raw())
        .map_err(|err| sexp_err("environment", err))?;
    let _guard = check_eval_depth()?;
    // GNU eval() sets R_Visible = TRUE at every entry (src/main/eval.c):
    // visibility is owned by the last evaluated expression, so a promise
    // forced mid-body cannot leak an earlier assignment's invisibility past
    // the body's final expression.
    super::super::runtime::set_visible(crate::sexp::ffi::TRUE);

    // Keep the rooted result here rather than copying it through each unwind
    // adapter in a nested evaluator chain.
    let mut evaluated = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        evaluated = Some(unsafe {
            /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
            eval_safe_inner(expr, env)
        });
    }));

    match result {
        Ok(()) => evaluated.expect("evaluation completed").and_then(|value| {
            factory
                .wrap(value.as_raw())
                .map_err(|err| sexp_err("evaluation result", err))
        }),
        Err(payload) => match payload.downcast::<crate::sexp::context::RSignal>() {
            Ok(signal) => match *signal {
                crate::sexp::context::RSignal::Error { message } => Err(message),
                other => std::panic::panic_any(other),
            },
            Err(payload) => match payload.downcast::<crate::sexp::context::RError>() {
                Ok(err) => Err(err.message.clone()),
                Err(payload) => std::panic::resume_unwind(payload),
            },
        },
    }
}

pub(crate) fn symbol_name_for_error(expr: Sexp<'_>) -> String {
    if let Some(name) = symbol_name_from_ptr(expr.clone().as_raw()) {
        return name;
    }
    unsafe {
        if TYPEOF(expr.clone().as_raw()) == SEXPTYPE::SYMSXP {
            let pname = PRINTNAME(expr.as_raw());
            if !pname.is_null() {
                let bytes = CHAR(pname);
                if !bytes.is_null() {
                    return std::ffi::CStr::from_ptr(bytes)
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
    }
    "<unknown>".to_string()
}

#[inline(never)]
pub(crate) unsafe fn eval_safe_inner<'a>(
    expr: Sexp<'a>,
    env: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    match classify_expr(&expr) {
        EvalKind::SelfEvaluating => Ok(expr),
        EvalKind::Symbol => unsafe { eval_symbol_safe(expr, env) },
        EvalKind::Language => unsafe {
            /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
            eval_lang_safe(expr, env)
        },
        EvalKind::Closure => Ok(expr),
        // Environments are first-class values (upstream Rf_eval returns
        // ENVSXP unchanged): `local({...})` blocks capture them via
        // `environment()`, and package shims return them as values.
        EvalKind::Environment => Ok(expr),
        EvalKind::Promise => unsafe {
            /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
            eval_promise_safe(expr, env)
        },
        EvalKind::Dots => eval_dots_safe(expr, env),
        EvalKind::Bytecode => unsafe { eval_bytecode_safe(expr, env) },
        EvalKind::Unsupported(kind) => Err(format!("cannot evaluate type {:?}", kind)),
    }
}

#[inline(never)]
pub(crate) unsafe fn eval_symbol_safe<'a>(
    expr: Sexp<'a>,
    env: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    if let Some(value) = (unsafe {
        /* SAFETY: internal caller retains the live owner and its roots across this scoped operation. */
        find_var_result(expr.clone(), env.clone())
    })? {
        return Ok(value);
    }
    match primitive_for_symbol(expr.clone()) {
        Some(primitive) => Ok(primitive),
        None => unsafe {
            crate::mainutils::errors::R_ObjectNotFoundError(
                expr.as_raw(),
                super::super::context::get_lexical_call(env.as_raw()),
                None,
            )
        },
    }
}

pub(crate) unsafe fn eval_bytecode_safe<'a>(
    expr: Sexp<'a>,
    env: Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    if super::super::jit::get_R_disable_bytecode() != 0 {
        return Err("bytecode evaluation is disabled for this R session".to_string());
    }
    // The call that entered this body was already counted in `eval_safe`.
    // Counting again made `options(expressions=)` about half of GNU's depth.
    let factory = env
        .node_factory()
        .map_err(|err| sexp_err("bytecode owner", err))?;
    let result = unsafe { super::super::bc_eval::bcEval(expr.as_raw(), env.as_raw()) };
    factory
        .wrap(result)
        .map_err(|err| sexp_err("bytecode result", err))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EvalKind {
    SelfEvaluating,
    Symbol,
    Language,
    Closure,
    Environment,
    Promise,
    Dots,
    Bytecode,
    Unsupported(SEXPTYPE),
}

pub(crate) fn classify_expr(expr: &Sexp<'_>) -> EvalKind {
    match expr.typeof_() {
        SEXPTYPE::NILSXP
        | SEXPTYPE::LISTSXP
        | SEXPTYPE::LGLSXP
        | SEXPTYPE::INTSXP
        | SEXPTYPE::REALSXP
        | SEXPTYPE::CPLXSXP
        | SEXPTYPE::STRSXP
        | SEXPTYPE::RAWSXP
        | SEXPTYPE::VECSXP
        // eval.c Rf_eval: an expression vector is a VALUE and is returned
        // unchanged; only do_eval() (R-level eval()) walks its elements.
        | SEXPTYPE::EXPRSXP
        | SEXPTYPE::EXTPTRSXP
        // Function objects are values too.  A call may legitimately contain
        // a function object in head position (for example, higher-order
        // helpers construct calls from an already matched `FUN`).  GNU R's
        // Rf_eval returns these objects unchanged before eval_lang dispatches
        // them; rejecting them here made `outer(..., paste)` fail while
        // evaluating the constructed call head.
        | SEXPTYPE::BUILTINSXP
        | SEXPTYPE::SPECIALSXP
        | SEXPTYPE::OBJSXP => EvalKind::SelfEvaluating,

        SEXPTYPE::SYMSXP => EvalKind::Symbol,
        SEXPTYPE::LANGSXP => EvalKind::Language,
        SEXPTYPE::CLOSXP => EvalKind::Closure,
        SEXPTYPE::ENVSXP => EvalKind::Environment,
        SEXPTYPE::PROMSXP => EvalKind::Promise,
        SEXPTYPE::DOTSXP => EvalKind::Dots,
        SEXPTYPE::BCODESXP => EvalKind::Bytecode,
        kind => EvalKind::Unsupported(kind),
    }
}

/// Safe evaluation of a language object (function call).
/// # Safety
/// Activate the live owner of all inputs and retain their reachable graphs
/// through allocation and R reentry. No Rust payload loan may cross execution.
#[inline(never)]
pub(crate) unsafe fn eval_lang_safe<'a>(e: Sexp<'a>, rho: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let fun = e.try_car().map_err(|err| sexp_err("empty call", err))?;
    let args = e.try_cdr().map_err(|err| sexp_err("missing args", err))?;

    // R uses function-position lookup for symbolic call heads: non-function
    // bindings are skipped while walking enclosing environments.
    let fun_val = unsafe { resolve_call_function(&fun, &e, &rho) }?;

    match fun_val.typeof_() {
        SEXPTYPE::CLOSXP => apply_closure_safe(fun_val, e, args, rho),
        SEXPTYPE::SPECIALSXP => apply_special_safe(fun_val, e, args, rho),
        SEXPTYPE::BUILTINSXP => apply_builtin_safe(fun_val, e, args, rho),
        kind => {
            let head = if fun.typeof_() == SEXPTYPE::SYMSXP {
                unsafe { get_symbol_name(fun.as_raw()) }
            } else {
                format!("{:?}", fun.typeof_())
            };
            Err(format!("cannot call type {kind:?} for {head}"))
        }
    }
}
