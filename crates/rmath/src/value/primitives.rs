#![forbid(unsafe_code)]
//! Primitive builtin dispatch for the `ValueId` evaluator.
//!
//! The migration slice starts with the arithmetic operators, which exercise
//! the full primitive contract: lazy argument promises, element forcing,
//! type coercion, and recycling rules. The id space here is scaffolding —
//! the canonical registry arrives with the builtin migration and replaces
//! this module's table.

use super::eval::{eval, force_if_promise, EvalError, EvalResult, RSignal};
use super::heap::Heap;
use super::id::ValueId;
use super::intern::SymbolInterner;
use super::object::RObject;

/// Scaffolding primitive identifiers. Values are arbitrary within this
/// module; the migration registry will own the canonical numbering.
pub mod primitive_ids {
    pub const ADD: u32 = 1;
    pub const SUB: u32 = 2;
    pub const MUL: u32 = 3;
    pub const DIV: u32 = 4;
    /// Special forms — unevaluated operands, exactly as GNU treats them.
    pub const IF: u32 = 10;
    pub const BRACE: u32 = 11;
    pub const ASSIGN: u32 = 12;
    pub const SUPERASSIGN: u32 = 13;
    pub const RETURN: u32 = 14;
    pub const WHILE: u32 = 15;
    pub const REPEAT: u32 = 16;
    pub const FOR: u32 = 17;
    pub const BREAK: u32 = 18;
    pub const NEXT: u32 = 19;
    /// Comparisons — eager operands, logical results.
    pub const LT: u32 = 20;
    pub const GT: u32 = 21;
    pub const LE: u32 = 22;
    pub const GE: u32 = 23;
    pub const EQ: u32 = 24;
    pub const NE: u32 = 25;
    /// Closure construction — captures formals, body, and defining env.
    pub const FUNCTION: u32 = 26;
}

use crate::value::object::BuiltinId;
use primitive_ids::{ADD, ASSIGN, BRACE, BREAK, DIV, EQ, FOR, FUNCTION, GE, GT, IF, LE, LT, MUL, NE, NEXT, REPEAT, RETURN, SUB, SUPERASSIGN, WHILE};

/// Invoke a primitive with an unevaluated argument pairlist in the caller's
/// environment — the same contract GNU primitives receive.
pub(crate) fn call_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    id: BuiltinId,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    match id.0 {
        ADD | SUB | MUL | DIV => arithmetic(heap, interner, id.0, args, caller_env),
        IF => if_form(heap, interner, args, caller_env),
        BRACE => brace_form(heap, interner, args, caller_env),
        ASSIGN => assign_form(heap, interner, args, caller_env, false),
        SUPERASSIGN => assign_form(heap, interner, args, caller_env, true),
        RETURN => return_form(heap, interner, args, caller_env),
        WHILE => while_form(heap, interner, args, caller_env),
        REPEAT => repeat_form(heap, interner, args, caller_env),
        FOR => for_form(heap, interner, args, caller_env),
        BREAK => Err(RSignal::Break),
        NEXT => Err(RSignal::Next),
        LT | GT | LE | GE | EQ | NE => comparison(heap, interner, id.0, args, caller_env),
        FUNCTION => function_form(heap, args, caller_env),
        _ => Err(EvalError::UnsupportedForm("unregistered primitive").into()),
    }
}

/// Collect the raw argument expressions of a call without evaluating them —
/// what special forms operate on.
fn raw_args(heap: &Heap, mut args: ValueId) -> Vec<ValueId> {
    let mut out = Vec::new();
    for _ in 0..100_000 {
        match heap.object(args) {
            Some(RObject::Pairlist { car, cdr, .. }) => {
                out.push(*car);
                if *cdr == ValueId::NIL {
                    break;
                }
                args = *cdr;
            }
            _ => break,
        }
    }
    out
}

/// `if (test) yes else no` — the test is forced to a single logical;
/// a missing else branch yields NIL, exactly GNU's invisible NULL.
fn if_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() < 2 || parts.len() > 3 {
        return Err(EvalError::UnsupportedForm("if arity").into());
    }
    let test = eval(heap, interner, parts[0], caller_env)?;
    let test = force_if_promise(heap, interner, test)?;
    let truth = match heap.object(test) {
        Some(RObject::Logical(values)) if !values.is_empty() => values[0] != 0,
        Some(RObject::Integer(values)) if !values.is_empty() => values[0] != 0,
        Some(RObject::Real(values)) if !values.is_empty() => values[0] != 0.0,
        _ => {
            return Err(EvalError::TypeError("if condition must be logical").into());
        }
    };
    if truth {
        eval(heap, interner, parts[1], caller_env)
    } else if parts.len() == 3 {
        eval(heap, interner, parts[2], caller_env)
    } else {
        Ok(ValueId::NIL)
    }
}

/// `{ ... }` — sequence evaluation in the same environment, value of the
/// last expression; empty blocks yield NIL.
fn brace_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut result = ValueId::NIL;
    for expr in raw_args(heap, args) {
        result = eval(heap, interner, expr, caller_env)?;
    }
    Ok(result)
}

/// `<-` / `=` and `<<-` — assignment special forms. The left operand is an
/// unevaluated symbol; the right evaluates in the caller's environment and
/// is itself the value of the assignment expression, exactly GNU.
fn assign_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    super_assign: bool,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 2 {
        return Err(EvalError::UnsupportedForm("assignment arity").into());
    }
    let sym = match heap.object(parts[0]) {
        Some(RObject::Symbol { name, .. }) => *name,
        _ => {
            return Err(EvalError::TypeError("assignment target must be a symbol").into());
        }
    };
    let value = eval(heap, interner, parts[1], caller_env)?;
    let value = force_if_promise(heap, interner, value)?;
    if super_assign {
        match crate::value::env::assign(heap, caller_env, sym, value) {
            Ok(crate::value::env::AssignOutcome::Assigned)
            | Ok(crate::value::env::AssignOutcome::ReplacedActiveBinding) => {}
            Ok(crate::value::env::AssignOutcome::NoExistingBinding) => {
                // GNU: <<- with no existing binding defines in the global
                // environment. Sentinel resolution is pending; defining in
                // the current frame is the documented slice approximation.
                crate::value::env::define(heap, caller_env, sym, value)
                    .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("superassign define")))?;
            }
            Err(_) => {
                return Err(EvalError::TypeError("cannot change value of locked binding").into());
            }
        }
    } else {
        crate::value::env::define(heap, caller_env, sym, value)
            .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("assign define")))?;
    }
    Ok(value)
}

/// `return(value)` — evaluate the operand and signal the unwinding.
fn return_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    let value = match parts.len() {
        0 => ValueId::NIL,
        1 => {
            let value = eval(heap, interner, parts[0], caller_env)?;
            force_if_promise(heap, interner, value)?
        }
        _ => return Err(EvalError::UnsupportedForm("return arity").into()),
    };
    Err(RSignal::Return(value))
}

/// Binary arithmetic with GNU coercion and recycling semantics.
///
/// Arguments arrive unevaluated; each is evaluated in the caller's
/// environment and forced. Integer+integer stays integer (except division,
/// which is always real); any real operand yields real. Length recycling
/// requires the longer length be a multiple of the shorter.
fn arithmetic(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let left = numeric_operands(heap, operands[0]).map_err(RSignal::from)?;
    let right = numeric_operands(heap, operands[1]).map_err(RSignal::from)?;
    match (left, right) {
        (Numeric::Integer(a), Numeric::Integer(b)) if op != DIV => {
            let folded = fold_numeric(a, b, op, |x, y| match op {
                ADD => x.wrapping_add(y),
                SUB => x.wrapping_sub(y),
                _ => x.wrapping_mul(y),
            })
            .map_err(RSignal::from)?;
            Ok(heap.alloc(RObject::Integer(folded)))
        }
        (a, b) => {
            let left_reals = as_reals(a);
            let right_reals = as_reals(b);
            let folded = fold_numeric(left_reals, right_reals, op, |x, y| match op {
                ADD => x + y,
                SUB => x - y,
                MUL => x * y,
                _ => x / y,
            })
            .map_err(RSignal::from)?;
            Ok(heap.alloc(RObject::Real(folded)))
        }
    }
}

/// Evaluate and force the leading `expected` arguments of a call.
fn eval_args(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    expected: usize,
) -> Result<Vec<ValueId>, RSignal> {
    let mut operands = Vec::new();
    let mut cursor = args;
    while operands.len() < expected {
        let (car, cdr) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, .. }) => (*car, *cdr),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        operands.push(force_if_promise(heap, interner, evaluated)?);
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    if operands.len() != expected {
        return Err(EvalError::UnsupportedForm("wrong argument count").into());
    }
    Ok(operands)
}

enum Numeric {
    Integer(Vec<i32>),
    Real(Vec<f64>),
}

fn numeric_operands(heap: &Heap, value: ValueId) -> Result<Numeric, EvalError> {
    match heap.object(value) {
        Some(RObject::Integer(values)) => Ok(Numeric::Integer(values.clone())),
        Some(RObject::Real(values)) => Ok(Numeric::Real(values.clone())),
        Some(RObject::Logical(values)) => Ok(Numeric::Integer(values.clone())),
        _ => Err(EvalError::TypeError("non-numeric argument to binary operator")),
    }
}

fn as_reals(numeric: Numeric) -> Vec<f64> {
    match numeric {
        Numeric::Real(values) => values,
        Numeric::Integer(values) => values
            .into_iter()
            .map(|v| {
                if v == crate::value::object::NA_INTEGER {
                    f64::from_bits(crate::value::object::NA_REAL_BITS)
                } else {
                    v as f64
                }
            })
            .collect(),
    }
}

/// Elementwise binary fold with GNU recycling: the longer length must be a
/// multiple of the shorter; length-one operands recycle silently.
fn fold_numeric<T: Copy>(
    a: Vec<T>,
    b: Vec<T>,
    _op: u32,
    f: impl Fn(T, T) -> T,
) -> Result<Vec<T>, EvalError> {
    let (longer, shorter) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    if shorter.is_empty() || longer.len() % shorter.len() != 0 {
        return Err(EvalError::TypeError(
            "longer object length is not a multiple of shorter object length",
        ));
    }
    Ok(longer
        .iter()
        .enumerate()
        .map(|(i, x)| f(*x, shorter[i % shorter.len()]))
        .collect())
}

/// `while (test) body` — loops until the test forces false. `break`
/// unwinds out of the loop entirely; `next` skips to the next iteration.
/// The loop value is always NIL in GNU.
fn while_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 2 {
        return Err(EvalError::UnsupportedForm("while arity").into());
    }
    loop {
        let test = eval(heap, interner, parts[0], caller_env)?;
        let test = force_if_promise(heap, interner, test)?;
        if !truthy(heap, test)? {
            return Ok(ValueId::NIL);
        }
        match eval(heap, interner, parts[1], caller_env) {
            Err(RSignal::Break) => return Ok(ValueId::NIL),
            Err(RSignal::Next) => continue,
            other => {
                let _ = other?;
            }
        }
    }
}

/// `repeat body` — an unconditional loop; only `break` exits.
fn repeat_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 1 {
        return Err(EvalError::UnsupportedForm("repeat arity").into());
    }
    loop {
        match eval(heap, interner, parts[0], caller_env) {
            Err(RSignal::Break) => return Ok(ValueId::NIL),
            Err(RSignal::Next) => continue,
            other => {
                let _ = other?;
            }
        }
    }
}

/// `for (var in seq) body` — binds each sequence element in turn. Loop
/// variables in R persist after the loop and take the last element's
/// value; elements are boxed as length-one vectors, matching GNU.
fn for_form(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 3 {
        return Err(EvalError::UnsupportedForm("for arity").into());
    }
    let sym = match heap.object(parts[0]) {
        Some(RObject::Symbol { name, .. }) => *name,
        _ => return Err(EvalError::TypeError("for variable must be a symbol").into()),
    };
    let seq = eval(heap, interner, parts[1], caller_env)?;
    let seq = force_if_promise(heap, interner, seq)?;
    let elements = sequence_elements(heap, seq)?;
    for element in elements {
        crate::value::env::define(heap, caller_env, sym, element)
            .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("for define")))?;
        match eval(heap, interner, parts[2], caller_env) {
            Err(RSignal::Break) => break,
            Err(RSignal::Next) => continue,
            other => {
                let _ = other?;
            }
        }
    }
    Ok(ValueId::NIL)
}

/// Force a value to a single truth value, GNU's condition coercion.
fn truthy(heap: &Heap, value: ValueId) -> Result<bool, RSignal> {
    match heap.object(value) {
        Some(RObject::Logical(values)) if !values.is_empty() => Ok(values[0] != 0),
        Some(RObject::Integer(values)) if !values.is_empty() => Ok(values[0] != 0),
        Some(RObject::Real(values)) if !values.is_empty() => Ok(values[0] != 0.0),
        _ => Err(EvalError::TypeError("condition must be logical").into()),
    }
}

/// Explode a vector into per-element length-one vectors — what for-loop
/// iteration binds. List elements pass through unboxed, exactly GNU.
fn sequence_elements(heap: &mut Heap, seq: ValueId) -> Result<Vec<ValueId>, RSignal> {
    let elements: Vec<RObject> = match heap.object(seq) {
        Some(RObject::Real(values)) => values
            .iter()
            .map(|v| RObject::Real(vec![*v]))
            .collect(),
        Some(RObject::Integer(values)) => values
            .iter()
            .map(|v| RObject::Integer(vec![*v]))
            .collect(),
        Some(RObject::Logical(values)) => values
            .iter()
            .map(|v| RObject::Logical(vec![*v]))
            .collect(),
        Some(RObject::List(elements)) => return Ok(elements.clone()),
        _ => {
            return Err(EvalError::TypeError("for sequence must be a vector").into());
        }
    };
    Ok(elements.into_iter().map(|body| heap.alloc(body)).collect())
}

/// Binary comparisons with GNU elementwise semantics: numeric coercion,
/// recycling, logical results.
fn comparison(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let left = numeric_operands(heap, operands[0]).map_err(RSignal::from)?;
    let right = numeric_operands(heap, operands[1]).map_err(RSignal::from)?;
    let a = as_reals(left);
    let b = as_reals(right);
    let folded = fold_numeric(a, b, op, |x, y| {
        let result = match op {
            LT => x < y,
            GT => x > y,
            LE => x <= y,
            GE => x >= y,
            EQ => x == y,
            _ => x != y,
        };
        if result {
            1.0
        } else {
            0.0
        }
    })
    .map_err(RSignal::from)?;
    Ok(heap.alloc(RObject::Logical(
        folded.into_iter().map(|v| v as i32).collect(),
    )))
}

/// Resolve a primitive's print name to a scaffolding id — the interop
/// mapping the import bridge uses until the canonical registry lands.
pub fn primitive_id_for_name(name: &str) -> Option<u32> {
    match name {
        "+" => Some(ADD),
        "-" => Some(SUB),
        "*" => Some(MUL),
        "/" => Some(DIV),
        "if" => Some(IF),
        "{" => Some(BRACE),
        "<-" | "=" => Some(ASSIGN),
        "<<-" => Some(SUPERASSIGN),
        "return" => Some(RETURN),
        "while" => Some(WHILE),
        "repeat" => Some(REPEAT),
        "for" => Some(FOR),
        "break" => Some(BREAK),
        "next" => Some(NEXT),
        "<" => Some(LT),
        ">" => Some(GT),
        "<=" => Some(LE),
        ">=" => Some(GE),
        "==" => Some(EQ),
        "!=" => Some(NE),
        "function" => Some(FUNCTION),
        _ => None,
    }
}

/// `function (formals) body` — closure construction. Neither operand
/// evaluates: the formals pairlist and body expression capture verbatim
/// alongside the defining environment, exactly GNU's `mkClosure`.
fn function_form(heap: &mut Heap, args: ValueId, caller_env: ValueId) -> EvalResult {
    let parts = raw_args(heap, args);
    // The parser emits an optional third environment operand (NIL when
    // absent); explicit environments are outside the migration slice.
    if parts.len() < 2 || parts.len() > 3 {
        return Err(EvalError::UnsupportedForm("function arity").into());
    }
    Ok(heap.alloc(RObject::Closure {
        formals: parts[0],
        body: parts[1],
        env: caller_env,
    }))
}
