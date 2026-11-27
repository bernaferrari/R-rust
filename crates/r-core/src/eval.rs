#![forbid(unsafe_code)]
//! The `ValueId`-native evaluator core.
//!
//! This module is the seed the evaluator migration grows into: expression
//! dispatch, symbol resolution with promise forcing, closure application
//! with lazy argument promises, and primitive dispatch. It implements GNU
//! R's core evaluation semantics — lazy arguments, promise value caching,
//! positional/named argument matching — for the subset of forms the
//! migration has reached. Everything here is safe Rust over the direct
//! heap; no legacy machinery participates.

use crate::env;
use crate::heap::{ContextFrame, Heap};
use crate::id::ValueId;
use crate::intern::SymId;
use crate::intern::SymbolInterner;
use crate::object::RObject;

/// Evaluation failures. Variants carrying `SymId`/`&'static str` describe
/// the failure the way the GNU error surface would; the migration replaces
/// this enum with structured `RSignal` values as control flow lands.
#[derive(Debug, Clone, PartialEq)]
pub enum EvalError {
    /// GNU: "object 'x' not found".
    UnboundVariable(String),
    /// GNU: "could not find function" / object is not callable.
    NotCallable,
    /// GNU: "missing argument" when a required formal is absent.
    MissingArgument(String),
    /// A promise whose forcing recurses into itself — GNU: circularity.
    RecursiveForcing,
    /// The form is outside the migrated subset; shrinks to zero as the
    /// migration completes.
    UnsupportedForm(&'static str),
    /// Operand types do not support the primitive — GNU: "non-numeric
    /// argument to binary operator" and friends.
    TypeError(&'static str),
    /// A user-raised condition through `stop()` — GNU's error objects
    /// carry messages; the migration slice carries the text.
    Condition(String),
}

/// Control-flow propagation — the pure-Rust replacement for GNU's longjmp
/// context system.
///
/// `return`, `break`, and `next` are not errors: they are control flow
/// values that unwind exactly one construct (the closure body, the loop)
/// and must never be swallowed by error handling. Errors ride the same
/// channel because both unwind evaluation, which is what makes
/// `Result<ValueId, RSignal>` the single evaluation result type the plan
/// specifies — no panics, no catch_unwind.
#[derive(Debug, Clone, PartialEq)]
pub enum RSignal {
    /// `return(value)` unwinding to the innermost closure body.
    Return(ValueId),
    /// `break` unwinding to the innermost loop.
    Break,
    /// `next` unwinding to the innermost loop iteration.
    Next,
    /// An evaluation error condition.
    Error(EvalError),
    /// `invokeRestart` unwinding to the owning withRestarts frame — the
    /// restart identity and the already-evaluated argument values.
    RestartInvoke { id: u64, args: Vec<crate::id::ValueId> },
}

/// The single evaluation result type.
pub type EvalResult = Result<ValueId, RSignal>;

impl From<EvalError> for RSignal {
    fn from(error: EvalError) -> RSignal {
        RSignal::Error(error)
    }
}

/// Evaluate an expression in an environment.
///
/// Atomic vectors self-evaluate. Symbols resolve through the environment
/// chain, forcing promise bindings. Language nodes are calls: the operator
/// evaluates to a primitive or closure, arguments become promises bound to
/// the closure's formals, and the body evaluates in the application frame.
/// Evaluate one expression node in the given environment — the single
/// recursion point of the tree-walking evaluator. The wrapper carries the
/// depth resource limit: every recursive descent passes through here, so
/// the guard decrements exactly once per frame however the node exits.
pub fn eval(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    expr: ValueId,
    env_id: ValueId,
) -> EvalResult {
    // GNU's nested-too-deeply ceiling — options(expressions=) in shape,
    /// the host's resource limit in authority. Zero configures the GNU
    /// default of 500 exactly as the facade's resource contract pins.
    let ceiling = if heap.max_eval_depth == 0 {
        500
    } else {
        heap.max_eval_depth
    };
    if heap.eval_depth >= ceiling {
        return Err(RSignal::from(EvalError::Condition(
            "evaluation nested too deeply: infinite recursion / options(expressions=)?".to_string(),
        )));
    }
    // The deadline shares the cancellation poll site — one cheap check
    /// pair per node against the tree walk's cost.
    if let Some(deadline) = heap.eval_deadline {
        if web_time::Instant::now() >= deadline {
            return Err(RSignal::from(EvalError::Condition(
                "evaluation time limit exceeded".to_string(),
            )));
        }
    }
    // The memory cap rides the same poll — payload bytes are the heap's
    // pressure measure exactly as the facade's arena accounting measures
    // the old world's occupancy.
    if let Some(cap) = heap.memory_cap_bytes {
        if heap.payload_bytes() > cap {
            return Err(RSignal::from(EvalError::Condition(
                "memory limit exceeded".to_string(),
            )));
        }
    }
    if let Some(cap) = heap.node_cap {
        if heap.live_count() > cap {
            return Err(RSignal::from(EvalError::Condition(
                "arena node limit exceeded".to_string(),
            )));
        }
    }
    heap.eval_depth += 1;
    let result = eval_node(heap, interner, expr, env_id);
    heap.eval_depth -= 1;
    result
}

fn eval_node(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    expr: ValueId,
    env_id: ValueId,
) -> EvalResult {
    // Cooperative cancellation: the host's token polls at every recursive
    // entry — a relaxed atomic load, nanoseconds against the tree walk —
    // so SIGINT unwinds long evaluation exactly as the legacy world's
    // does. The error text matches the facade's cancellation contract.
    if let Some(cell) = &heap.cancellation {
        if cell.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(RSignal::from(EvalError::Condition(
                "operation cancelled".to_string(),
            )));
        }
    }
    if expr == ValueId::NIL {
        return Ok(ValueId::NIL);
    }
    let body = match heap.object(expr) {
        Some(body) => body,
        None => return Err(EvalError::UnsupportedForm("dangling expression handle").into()),
    };
    match body {
        // Atomic vectors and scalars self-evaluate.
        RObject::Nil
        | RObject::Logical(_)
        | RObject::Integer(_)
        | RObject::Real(_)
        | RObject::Complex(_)
        | RObject::String(_)
        | RObject::Raw(_)
        | RObject::Primitive(_)
        | RObject::Bytecode(_)
        | RObject::External(_)
        | RObject::S4Object => {
            // Self-evaluating atoms are always visible — GNU's rule.
            heap.visible = true;
            Ok(expr)
        }
        RObject::Symbol { name, .. } => {
            let sym = *name;
            let label = interner.symbol_name(sym).to_string();
            heap.visible = true;
            match env::lookup(heap, env_id, sym) {
                Some((_, binding)) => {
                    if binding.active {
                        // GNU's active-binding read: the stored closure fires
                        // with zero arguments and its result is the value.
                        match heap.object(binding.value).cloned() {
                            Some(RObject::Closure { formals, body, env: clos_env }) => {
                                let applied = apply_closure(
                                    heap, interner, formals, body, clos_env, ValueId::NIL, env_id,
                                    ValueId::NIL, binding.value,
                                )?;
                                force_if_promise(heap, interner, applied)
                            }
                            _ => force_if_promise(heap, interner, binding.value),
                        }
                    } else {
                        force_if_promise(heap, interner, binding.value)
                    }
                }
                None => {
                    // `..N` reads the Nth dots element directly — GNU's
                    /// positional dots access resolved at lookup time.
                    if let Some(index) = parse_dotdot_index(&label) {
                        return dots_elt(heap, interner, env_id, index)
                            .ok_or(EvalError::UnboundVariable(label)).map_err(RSignal::from);
                    }
                    Err(RSignal::from(EvalError::UnboundVariable(label)))
                }
            }
        }
        RObject::Promise { .. } => force_if_promise(heap, interner, expr),
        RObject::Language { car, cdr, .. } => {
            let (car, cdr) = (*car, *cdr);
            // GNU's findFun: a symbolic call head resolves through function
            /// lookup, which skips non-callable bindings — a variable named
            /// `names` never shadows the primitive in function position.
            let callee = match heap.object(car) {
                Some(RObject::Symbol { name, .. }) => {
                    let sym = *name;
                    match lookup_callable(heap, interner, env_id, sym)? {
                        Some(value) => value,
                        None => {
                            let label = interner.symbol_name(sym).to_string();
                            return Err(RSignal::from(EvalError::UnboundVariable(label)));
                        }
                    }
                }
                _ => eval(heap, interner, car, env_id)?,
            };
            call(heap, interner, callee, cdr, env_id, expr)
        }
        RObject::List(_) | RObject::Expression(_) => Ok(expr),
        RObject::Pairlist { .. } | RObject::Closure { .. } | RObject::Environment { .. } => {
            // Closures and environments self-evaluate as values; pairlists
            // only appear as call structure, never as evaluable expressions.
            Ok(expr)
        }
        RObject::Altrep(_) | RObject::WeakRef { .. } => {
            Err(EvalError::UnsupportedForm("altrep/weakref evaluation").into())
        }
    }
}

/// Force a value if it is a promise, otherwise return it unchanged.
///
/// Promise forcing caches the result in the promise's value field — the
/// heart of R's call-by-need. A promise whose value is the unbound sentinel
/// has never been forced; one mid-forcing is detected through the
/// reentrancy marker and reported as circular.
pub fn force_if_promise(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
) -> EvalResult {
    let (expr, env_id, cached) = match heap.object(value) {
        Some(RObject::Promise { expr, env, value }) => (*expr, *env, *value),
        _ => return Ok(value),
    };
    if cached == ValueId::MISSING {
        // The reentrancy marker: this promise is already being forced
        // further up the stack.
        return Err(EvalError::RecursiveForcing.into());
    }
    if cached != ValueId::UNBOUND {
        return Ok(cached);
    }
    // Mark in-progress before evaluating so recursive forcing is circular
    // rather than divergent.
    if let Some(RObject::Promise { value: slot, .. }) = heap.object_mut(value) {
        *slot = ValueId::MISSING;
    }
    let forced = eval(heap, interner, expr, env_id)?;
    if let Some(RObject::Promise { value: slot, .. }) = heap.object_mut(value) {
        *slot = forced;
    }
    Ok(forced)
}

/// Allocate an unforced promise — what argument passing and lazy assignment
/// create.
pub fn new_promise(heap: &mut Heap, expr: ValueId, env: ValueId) -> ValueId {
    heap.alloc(RObject::Promise {
        expr,
        env,
        value: ValueId::UNBOUND,
    })
}

/// GNU's `findFun`: walk the enclosure chain for a callable binding,
/// skipping frames whose binding for the symbol is not a function. A
/// promise binding is forced before inspection exactly as GNU; active
/// bindings are skipped — GNU never fires them in function position.
fn lookup_callable(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    env: ValueId,
    sym: SymId,
) -> Result<Option<ValueId>, RSignal> {
    let mut cursor = env;
    for _ in 0..env::MAX_CHAIN_DEPTH {
        if cursor == ValueId::EMPTY_ENV || cursor.is_sentinel() {
            return Ok(None);
        }
        if let Some(binding) = env::lookup_in_frame(heap, cursor, sym) {
            if !binding.active {
                let candidate = match heap.object(binding.value) {
                    Some(RObject::Closure { .. }) | Some(RObject::Primitive(_)) => Some(binding.value),
                    Some(RObject::Promise { .. }) => {
                        let forced = force_if_promise(heap, interner, binding.value)?;
                        match heap.object(forced) {
                            Some(RObject::Closure { .. }) | Some(RObject::Primitive(_)) => Some(forced),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(value) = candidate {
                    return Ok(Some(value));
                }
            }
        }
        match env::enclosure_of(heap, cursor) {
            Some(parent) => cursor = parent,
            None => return Ok(None),
        }
    }
    Ok(None)
}

/// Call a resolved callee with an argument pairlist in the caller's
/// environment.
fn call(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    callee: ValueId,
    args: ValueId,
    caller_env: ValueId,
    call_expr: ValueId,
) -> EvalResult {
    // Dots forwarding: a bare `...` argument splices the caller's dots
    /// pairlist into the call exactly as GNU — promises carried intact so
    /// downstream forcing shares the original cells.
    let dots_sym = interner.intern_symbol("...");
    let needs_splice = {
        let mut cursor = args;
        let mut found = false;
        for _ in 0..MAX_LIST_LENGTH {
            match heap.object(cursor) {
                Some(RObject::Pairlist { car, cdr, .. }) => {
                    if matches!(heap.object(*car), Some(RObject::Symbol { name, .. }) if *name == dots_sym) {
                        found = true;
                        break;
                    }
                    if *cdr == ValueId::NIL {
                        break;
                    }
                    cursor = *cdr;
                }
                _ => break,
            }
        }
        found
    };
    let args = if needs_splice {
        // Rebuild the chain: ordinary cells copy through, `...` cells
        /// expand into the visible dots in order.
        let mut source: Vec<(ValueId, ValueId)> = Vec::new();
        let mut cursor = args;
        for _ in 0..MAX_LIST_LENGTH {
            match heap.object(cursor) {
                Some(RObject::Pairlist { car, cdr, tag }) => {
                    source.push((*car, *tag));
                    if *cdr == ValueId::NIL {
                        break;
                    }
                    cursor = *cdr;
                }
                _ => break,
            }
        }
        let mut expanded: Vec<(ValueId, ValueId)> = Vec::new();
        let mut spliced = false;
        for (car, tag) in source {
            if matches!(heap.object(car), Some(RObject::Symbol { name, .. }) if *name == dots_sym) {
                if let Some(cells) = dots_cells(heap, interner, caller_env) {
                    for (cell_car, cell_tag) in cells {
                        expanded.push((cell_car, cell_tag));
                    }
                    spliced = true;
                }
            } else {
                expanded.push((car, tag));
            }
        }
        if spliced {
            let mut rebuilt = ValueId::NIL;
            for (car, tag) in expanded.iter().rev() {
                rebuilt = heap.alloc(RObject::Pairlist { car: *car, cdr: rebuilt, tag: *tag });
            }
            rebuilt
        } else {
            args
        }
    } else {
        args
    };
    let result = match heap.object(callee) {
        Some(RObject::Primitive(id)) => {
            let id = *id;
            match heap.primitive_dispatcher {
                Some(dispatch) => dispatch(heap, interner, id, args, caller_env),
                None => Err(RSignal::Error(EvalError::TypeError(
                    "primitive dispatcher not installed",
                ))),
            }
        }
        Some(RObject::Closure { formals, body, env }) => {
            let (formals, body, closure_env) = (*formals, *body, *env);
            apply_closure(heap, interner, formals, body, closure_env, args, caller_env, call_expr, callee)
        }
        _ => Err(EvalError::NotCallable.into()),
    };
    // conditionCall fidelity — GNU's context stack answers the raising
    /// frame's call: attach this call when the error originates here
    /// (no inner frame claimed it), and never overwrite as the error
    /// unwinds through outer frames. Control flow is not a condition
    /// and attaches nothing.
    if let Err(RSignal::Error(_)) = &result {
        if heap.error_call == ValueId::NIL {
            heap.error_call = call_expr;
        }
    }
    result
}

/// Apply a closure: match arguments to formals as lazy promises, then
/// evaluate the body in the application frame.
/// One closure-activation record — GNU's context-stack entry shape: the
/// application frame, the originating call expression, the closure object,
/// and the supplied-argument count nargs() reports.
///
/// Argument matching implements GNU's order — exact names first, then
/// positional — with lazy promise construction. Formals without defaults
/// and without supplied arguments bind the missing-argument sentinel.
/// Cross-module access for the apply family — primitives need closure
/// application with synthetic argument pairlists.
pub fn apply_closure_public(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    formals: ValueId,
    body: ValueId,
    closure_env: ValueId,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    apply_closure(heap, interner, formals, body, closure_env, args, caller_env, ValueId::NIL, ValueId::NIL)
}

fn apply_closure(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    formals: ValueId,
    body: ValueId,
    closure_env: ValueId,
    args: ValueId,
    caller_env: ValueId,
    call_expr: ValueId,
    closure_id: ValueId,
) -> EvalResult {
    let frame = env::new_environment(heap, closure_env);

    // Collect formal parameters: (name, default-expr) pairs in order.
    let formal_list = collect_formals(heap, formals);
    // Collect supplied arguments: (optional name, expr) in order.
    let arg_list = collect_args(heap, args);

    // GNU's dots split: formals before `...` match exactly then
    // positionally; formals after it match exact-name only; every argument
    // unconsumed lands in the dots pairlist in supplied order.
    let dots_sym = interner.intern_symbol("...");
    let dots_position = formal_list.iter().position(|(sym, _)| *sym == dots_sym);
    let (pre_formals, post_formals): (Vec<(SymId, ValueId)>, Vec<(SymId, ValueId)>) = match dots_position {
        Some(pos) => (
            formal_list[..pos].to_vec(),
            formal_list[pos + 1..].to_vec(),
        ),
        None => (formal_list.clone(), Vec::new()),
    };

    // Exact-name matching across all non-dots formals first.
    let mut used_args = vec![false; arg_list.len()];
    let mut bindings: Vec<(SymId, ValueId)> = Vec::new();
    let mut pre_unmatched: Vec<(SymId, ValueId)> = Vec::new();
    let mut remaining: Vec<(SymId, ValueId)> = Vec::new();

    for (sym, default) in &pre_formals {
        let named = arg_list
            .iter()
            .enumerate()
            .find(|(index, (name, _))| !used_args[*index] && name == &Some(*sym))
            .map(|(index, (_, expr))| (index, expr));
        if let Some((index, expr)) = named {
            used_args[index] = true;
            bindings.push((*sym, *expr));
        } else {
            pre_unmatched.push((*sym, *default));
        }
    }
    for (sym, default) in &post_formals {
        let named = arg_list
            .iter()
            .enumerate()
            .find(|(index, (name, _))| !used_args[*index] && name == &Some(*sym))
            .map(|(index, (_, expr))| (index, expr));
        if let Some((index, expr)) = named {
            used_args[index] = true;
            bindings.push((*sym, *expr));
        } else {
            remaining.push((*sym, *default));
        }
    }

    // Positional distribution over unmatched pre-dots formals only.
    let positional: Vec<(usize, ValueId)> = arg_list
        .iter()
        .enumerate()
        .filter(|(index, (name, _))| !used_args[*index] && name.is_none())
        .map(|(index, (_, expr))| (index, *expr))
        .collect();
    let mut positional = positional.into_iter();
    for (sym, default) in pre_unmatched {
        if let Some((index, expr)) = positional.next() {
            used_args[index] = true;
            bindings.push((sym, expr));
        } else {
            remaining.push((sym, default));
        }
    }

    // Bind everything: supplied arguments become promises in the caller's
    // environment; defaults become promises in the application frame (GNU
    // evaluates defaults in the closure's own frame).
    for (sym, expr) in bindings {
        let promise = new_promise(heap, expr, caller_env);
        env::define(heap, frame, sym, promise).map_err(|_| RSignal::Error(EvalError::UnsupportedForm("frame define")))?;
    }
    for (sym, default) in remaining {
        let binding = if default == ValueId::NIL || default == ValueId::MISSING {
            ValueId::MISSING
        } else {
            new_promise(heap, default, frame)
        };
        env::define(heap, frame, sym, binding).map_err(|_| RSignal::Error(EvalError::UnsupportedForm("frame define")))?;
    }

    // Dots capture: every argument matching left unconsumed joins the
    /// pairlist as a promise in the caller's environment, tags preserved
    /// exactly as GNU — forwarding and introspection both read this.
    if dots_position.is_some() {
        let mut dots = ValueId::NIL;
        let cells: Vec<(usize, Option<SymId>, ValueId)> = arg_list
            .iter()
            .enumerate()
            .filter(|(index, _)| !used_args[*index])
            .map(|(index, (name, expr))| (index, *name, *expr))
            .collect();
        for (_, name, expr) in cells.iter().rev() {
            let promise = new_promise(heap, *expr, caller_env);
            let tag = match name {
                Some(sym) => heap.alloc(RObject::Symbol { name: *sym, value: ValueId::NIL, internal: ValueId::NIL }),
                None => ValueId::NIL,
            };
            dots = heap.alloc(RObject::Pairlist { car: promise, cdr: dots, tag });
        }
        env::define(heap, frame, dots_sym, dots).map_err(|_| RSignal::Error(EvalError::UnsupportedForm("dots define")))?;
    }

    // The context entry GNU's sys.* family reads — pushed for the body
    /// evaluation exactly as GNU's context stack, popped on every exit.
    heap.contexts.push(ContextFrame {
        frame,
        call: call_expr,
        closure: closure_id,
        nargs: arg_list.len(),
    });
    // The closure boundary absorbs a `return` signal — exactly one frame
    // of unwinding — while break/next/errors propagate outward.
    let result = eval(heap, interner, body, frame);
    heap.contexts.pop();
    // on.exit handlers fire on every exit path exactly as GNU — normal
    // completion, return, and error unwinding alike — in registration
    // order within the closure's own frame.
    if let Some(handlers) = heap.exit_handlers.remove(&frame) {
        for handler in handlers {
            let _ = eval(heap, interner, handler, frame);
        }
    }
    match result {
        Err(RSignal::Return(value)) => Ok(value),
        other => other,
    }
}

/// Flatten a pairlist chain into its cells in order, each as (car, tag).
fn collect_pairlist(heap: &Heap, mut cursor: ValueId) -> Vec<(ValueId, ValueId)> {
    let mut cells = Vec::new();
    for _ in 0..MAX_LIST_LENGTH {
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => {
                cells.push((*car, *tag));
                if *cdr == ValueId::NIL {
                    break;
                }
                cursor = *cdr;
            }
            _ => break,
        }
    }
    cells
}

/// Collect the formals pairlist into (name, default) pairs.
fn collect_formals(heap: &Heap, formals: ValueId) -> Vec<(SymId, ValueId)> {
    collect_pairlist(heap, formals)
        .into_iter()
        .filter_map(|(default, tag)| {
            if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
                Some((*name, default))
            } else {
                None
            }
        })
        .collect()
}

/// Collect call arguments: each cell's car is the argument expression and
/// its tag, when a symbol, is the supplied argument name.
fn collect_args(heap: &Heap, args: ValueId) -> Vec<(Option<SymId>, ValueId)> {
    collect_pairlist(heap, args)
        .into_iter()
        .map(|(expr, tag)| {
            let name = match heap.object(tag) {
                Some(RObject::Symbol { name, .. }) => Some(*name),
                _ => None,
            };
            (name, expr)
        })
        .collect()
}

/// Defensive ceiling for chain walking — real argument and formals lists
/// are tens of cells, never thousands.
const MAX_LIST_LENGTH: usize = 100_000;

/// Parse `..N` symbol names into their 1-based dots position.
pub fn parse_dotdot_index(label: &str) -> Option<usize> {
    let digits = label.strip_prefix("..")?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Force the Nth dots element visible from an environment — the shared
/// core of `..N`, `...elt`, and forwarding exactly as GNU's promise chain.
pub fn dots_elt(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    env_id: ValueId,
    index: usize,
) -> Option<ValueId> {
    let dots_sym = interner.intern_symbol("...");
    let (_, binding) = env::lookup(heap, env_id, dots_sym)?;
    let mut cursor = binding.value;
    let mut position = 1usize;
    for _ in 0..MAX_LIST_LENGTH {
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, .. }) => {
                if position == index {
                    return force_if_promise(heap, interner, *car).ok();
                }
                position += 1;
                if *cdr == ValueId::NIL {
                    return None;
                }
                cursor = *cdr;
            }
            _ => return None,
        }
    }
    None
}

/// Read the raw dots pairlist visible from an environment — forwarding
/// splices these cells exactly as GNU's promise-preserving expansion.
pub fn dots_cells(heap: &Heap, interner: &mut SymbolInterner, env_id: ValueId) -> Option<Vec<(ValueId, ValueId)>> {
    let dots_sym = interner.intern_symbol("...");
    let (_, binding) = env::lookup(heap, env_id, dots_sym)?;
    let mut cells = Vec::new();
    let mut cursor = binding.value;
    for _ in 0..MAX_LIST_LENGTH {
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => {
                cells.push((*car, *tag));
                if *cdr == ValueId::NIL {
                    return Some(cells);
                }
                cursor = *cdr;
            }
            _ => return Some(cells),
        }
    }
    Some(cells)

}
