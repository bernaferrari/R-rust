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

use super::env;
use super::heap::Heap;
use super::id::ValueId;
use super::intern::SymId;
use super::intern::SymbolInterner;
use super::object::RObject;

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
}

/// Evaluate an expression in an environment.
///
/// Atomic vectors self-evaluate. Symbols resolve through the environment
/// chain, forcing promise bindings. Language nodes are calls: the operator
/// evaluates to a primitive or closure, arguments become promises bound to
/// the closure's formals, and the body evaluates in the application frame.
pub fn eval(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    expr: ValueId,
    env_id: ValueId,
) -> Result<ValueId, EvalError> {
    if expr == ValueId::NIL {
        return Ok(ValueId::NIL);
    }
    let body = match heap.object(expr) {
        Some(body) => body,
        None => return Err(EvalError::UnsupportedForm("dangling expression handle")),
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
        | RObject::S4Object => Ok(expr),
        RObject::Symbol { name, .. } => {
            let sym = *name;
            let label = interner.symbol_name(sym).to_string();
            let (_, binding) = env::lookup(heap, env_id, sym)
                .ok_or(EvalError::UnboundVariable(label))?;
            force_if_promise(heap, interner, binding.value)
        }
        RObject::Promise { .. } => force_if_promise(heap, interner, expr),
        RObject::Language { car, cdr, .. } => {
            let (car, cdr) = (*car, *cdr);
            let callee = eval(heap, interner, car, env_id)?;
            call(heap, interner, callee, cdr, env_id)
        }
        RObject::List(_) | RObject::Expression(_) => Ok(expr),
        RObject::Pairlist { .. } | RObject::Closure { .. } | RObject::Environment { .. } => {
            // Closures and environments self-evaluate as values; pairlists
            // only appear as call structure, never as evaluable expressions.
            Ok(expr)
        }
        RObject::Altrep(_) | RObject::WeakRef { .. } => {
            Err(EvalError::UnsupportedForm("altrep/weakref evaluation"))
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
) -> Result<ValueId, EvalError> {
    let (expr, env_id, cached) = match heap.object(value) {
        Some(RObject::Promise { expr, env, value }) => (*expr, *env, *value),
        _ => return Ok(value),
    };
    if cached == ValueId::MISSING {
        // The reentrancy marker: this promise is already being forced
        // further up the stack.
        return Err(EvalError::RecursiveForcing);
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

/// Call a resolved callee with an argument pairlist in the caller's
/// environment.
fn call(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    callee: ValueId,
    args: ValueId,
    caller_env: ValueId,
) -> Result<ValueId, EvalError> {
    match heap.object(callee) {
        Some(RObject::Primitive(id)) => {
            let id = *id;
            crate::value::primitives::call_primitive(heap, interner, id, args, caller_env)
        }
        Some(RObject::Closure { formals, body, env }) => {
            let (formals, body, closure_env) = (*formals, *body, *env);
            apply_closure(heap, interner, formals, body, closure_env, args, caller_env)
        }
        _ => Err(EvalError::NotCallable),
    }
}

/// Apply a closure: match arguments to formals as lazy promises, then
/// evaluate the body in the application frame.
///
/// Argument matching implements GNU's order — exact names first, then
/// positional — with lazy promise construction. Formals without defaults
/// and without supplied arguments bind the missing-argument sentinel.
fn apply_closure(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    formals: ValueId,
    body: ValueId,
    closure_env: ValueId,
    args: ValueId,
    caller_env: ValueId,
) -> Result<ValueId, EvalError> {
    let frame = env::new_environment(heap, closure_env);

    // Collect formal parameters: (name, default-expr) pairs in order.
    let formal_list = collect_formals(heap, formals);
    // Collect supplied arguments: (optional name, expr) in order.
    let arg_list = collect_args(heap, args);

    // Exact-name matching first, then positional distribution of the rest —
    // GNU's matchArgs ordering, minus partial matching for now.
    let mut used_args = vec![false; arg_list.len()];
    let mut bindings: Vec<(SymId, ValueId)> = Vec::new();
    let mut unnamed_formals: Vec<(SymId, ValueId)> = Vec::new();

    for (sym, default) in &formal_list {
        let named = arg_list
            .iter()
            .enumerate()
            .find(|(index, (name, _))| !used_args[*index] && name == &Some(*sym))
            .map(|(index, (_, expr))| (index, expr));
        if let Some((index, expr)) = named {
            used_args[index] = true;
            bindings.push((*sym, *expr));
        } else {
            unnamed_formals.push((*sym, *default));
        }
    }

    // Positional distribution over the remaining formals. Collected eagerly
    // so the distribution loop can mark arguments used.
    let positional: Vec<(usize, ValueId)> = arg_list
        .iter()
        .enumerate()
        .filter(|(index, (name, _))| !used_args[*index] && name.is_none())
        .map(|(index, (_, expr))| (index, *expr))
        .collect();
    let mut positional = positional.into_iter();
    let mut remaining: Vec<(SymId, ValueId)> = Vec::new();
    for (sym, default) in unnamed_formals {
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
        env::define(heap, frame, sym, promise).map_err(|_| EvalError::UnsupportedForm("frame define"))?;
    }
    for (sym, default) in remaining {
        let binding = if default == ValueId::NIL || default == ValueId::MISSING {
            ValueId::MISSING
        } else {
            new_promise(heap, default, frame)
        };
        env::define(heap, frame, sym, binding).map_err(|_| EvalError::UnsupportedForm("frame define"))?;
    }

    eval(heap, interner, body, frame)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::env;
    use crate::value::object::BuiltinId;
    use crate::value::primitives::primitive_ids;

    fn fixture() -> (Heap, SymbolInterner) {
        (Heap::new(), SymbolInterner::new())
    }

    fn symbol(heap: &mut Heap, interner: &mut SymbolInterner, name: &str) -> ValueId {
        let sym = interner.intern_symbol(name);
        heap.alloc(RObject::Symbol {
            name: sym,
            value: ValueId::UNBOUND,
            internal: ValueId::NIL,
        })
    }

    fn pairlist(heap: &mut Heap, cells: &[(ValueId, ValueId)]) -> ValueId {
        // build back-to-front so the chain order matches the input order
        let mut cursor = ValueId::NIL;
        for (car, tag) in cells.iter().rev() {
            cursor = heap.alloc(RObject::Pairlist {
                car: *car,
                cdr: cursor,
                tag: *tag,
            });
        }
        cursor
    }

    fn tagged_symbol(
        heap: &mut Heap,
        interner: &mut SymbolInterner,
        name: &str,
    ) -> ValueId {
        symbol(heap, interner, name)
    }

    fn call_expr(heap: &mut Heap, callee: ValueId, args: ValueId) -> ValueId {
        heap.alloc(RObject::Language {
            car: callee,
            cdr: args,
            tag: ValueId::NIL,
        })
    }

    fn real(heap: &mut Heap, values: &[f64]) -> ValueId {
        heap.alloc(RObject::Real(values.to_vec()))
    }

    fn primitive(heap: &mut Heap, id: u32) -> ValueId {
        heap.alloc(RObject::Primitive(BuiltinId(id)))
    }

    #[test]
    fn atomic_vectors_self_evaluate() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let v = real(&mut heap, &[1.0, 2.0]);
        let result = eval(&mut heap, &mut interner, v, env_id).expect("self-eval");
        assert_eq!(result, v);
    }

    #[test]
    fn symbol_resolves_through_environment() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let x = interner.intern_symbol("x");
        let value = real(&mut heap, &[42.0]);
        env::define(&mut heap, env_id, x, value).expect("define");
        let x_expr = symbol(&mut heap, &mut interner, "x");
        let result = eval(&mut heap, &mut interner, x_expr, env_id).expect("lookup");
        assert_eq!(result, value);
    }

    #[test]
    fn unbound_symbol_errors_with_name() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let x_expr = symbol(&mut heap, &mut interner, "nonexistent");
        assert_eq!(
            eval(&mut heap, &mut interner, x_expr, env_id),
            Err(EvalError::UnboundVariable("nonexistent".to_string()))
        );
    }

    #[test]
    fn arithmetic_primitive_computes() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let add = primitive(&mut heap, primitive_ids::ADD);
        let one = real(&mut heap, &[1.0]);
        let two = real(&mut heap, &[2.0]);
        let args = pairlist(&mut heap, &[(one, ValueId::NIL), (two, ValueId::NIL)]);
        let call = call_expr(&mut heap, add, args);
        let result = eval(&mut heap, &mut interner, call, env_id).expect("addition");
        match heap.object(result) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[3.0]),
            other => panic!("expected real, got {other:?}"),
        }
    }

    #[test]
    fn closure_application_binds_and_evaluates() {
        let (mut heap, mut interner) = fixture();
        let global = env::new_environment(&mut heap, ValueId::EMPTY_ENV);

        // function(x) x + 1, constructed directly:
        // formals: pairlist cell tag=x-symbol, car=missing
        let x_tag = tagged_symbol(&mut heap, &mut interner, "x");
        let formals = pairlist(&mut heap, &[(ValueId::MISSING, x_tag)]);
        // body: the call x + 1 — operator is a primitive directly for the slice
        let add = primitive(&mut heap, primitive_ids::ADD);
        let x_ref = symbol(&mut heap, &mut interner, "x");
        let one = real(&mut heap, &[1.0]);
        let body_args = pairlist(&mut heap, &[(x_ref, ValueId::NIL), (one, ValueId::NIL)]);
        let body = call_expr(&mut heap, add, body_args);
        let closure = heap.alloc(RObject::Closure {
            formals,
            body,
            env: global,
        });

        // apply it to 5: the call's operator is the closure object itself
        let five = real(&mut heap, &[5.0]);
        let call_args = pairlist(&mut heap, &[(five, ValueId::NIL)]);
        let call = call_expr(&mut heap, closure, call_args);
        let result = eval(&mut heap, &mut interner, call, global).expect("application");
        match heap.object(result) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[6.0]),
            other => panic!("expected 6.0, got {other:?}"),
        }
    }

    #[test]
    fn arguments_are_lazy_unreferenced_promises_never_evaluate() {
        let (mut heap, mut interner) = fixture();
        let global = env::new_environment(&mut heap, ValueId::EMPTY_ENV);

        // function(x) 42 — the argument is never referenced, so a poisoned
        // argument (an unbound symbol, which errors when evaluated) must not
        // affect the result. This is the sharpest test of call-by-need.
        let x_tag = tagged_symbol(&mut heap, &mut interner, "x");
        let formals = pairlist(&mut heap, &[(ValueId::MISSING, x_tag)]);
        let forty_two = real(&mut heap, &[42.0]);
        let closure = heap.alloc(RObject::Closure {
            formals,
            body: forty_two,
            env: global,
        });

        let poison = symbol(&mut heap, &mut interner, "this_symbol_does_not_exist");
        let call_args = pairlist(&mut heap, &[(poison, ValueId::NIL)]);
        let call = call_expr(&mut heap, closure, call_args);
        let result = eval(&mut heap, &mut interner, call, global);
        match result {
            Ok(value) => match heap.object(value) {
                Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[42.0]),
                other => panic!("expected 42.0, got {other:?}"),
            },
            Err(error) => panic!("lazy arguments must never evaluate: {error:?}"),
        }
    }

    #[test]
    fn promise_forcing_caches_the_value() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let three = real(&mut heap, &[3.0]);
        let promise = new_promise(&mut heap, three, env_id);

        let first = force_if_promise(&mut heap, &mut interner, promise).expect("force");
        assert_eq!(first, three);
        // cached: the promise's value field now holds the forced value
        match heap.object(promise) {
            Some(RObject::Promise { value, .. }) => assert_eq!(*value, three),
            other => panic!("expected promise, got {other:?}"),
        }
    }

    #[test]
    fn missing_formal_evaluates_default_in_closure_frame() {
        let (mut heap, mut interner) = fixture();
        let global = env::new_environment(&mut heap, ValueId::EMPTY_ENV);

        // function(x = 7) x — calling with no arguments uses the default
        let x_tag = tagged_symbol(&mut heap, &mut interner, "x");
        let seven = real(&mut heap, &[7.0]);
        let formals = pairlist(&mut heap, &[(seven, x_tag)]);
        let x_ref = symbol(&mut heap, &mut interner, "x");
        let closure = heap.alloc(RObject::Closure {
            formals,
            body: x_ref,
            env: global,
        });
        let call = call_expr(&mut heap, closure, ValueId::NIL);
        let result = eval(&mut heap, &mut interner, call, global).expect("default eval");
        match heap.object(result) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[7.0]),
            other => panic!("expected 7.0, got {other:?}"),
        }
    }

    #[test]
    fn recycling_expands_shorter_operand() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let add = primitive(&mut heap, primitive_ids::ADD);
        let long = real(&mut heap, &[1.0, 2.0, 3.0, 4.0]);
        let short = real(&mut heap, &[10.0, 20.0]);
        let args = pairlist(&mut heap, &[(long, ValueId::NIL), (short, ValueId::NIL)]);
        let call = call_expr(&mut heap, add, args);
        let result = eval(&mut heap, &mut interner, call, env_id).expect("recycled add");
        match heap.object(result) {
            Some(RObject::Real(values)) => {
                assert_eq!(values.as_slice(), &[11.0, 22.0, 13.0, 24.0])
            }
            other => panic!("expected recycled result, got {other:?}"),
        }
    }
}
