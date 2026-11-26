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
    /// Data construction and access.
    pub const CONCAT: u32 = 30;
    pub const SUBSET: u32 = 31;
    pub const LENGTH: u32 = 32;
    pub const COLON: u32 = 33;
}

use crate::value::object::BuiltinId;
use primitive_ids::{ADD, ASSIGN, BRACE, BREAK, COLON, CONCAT, DIV, EQ, FOR, FUNCTION, GE, GT, IF, LE, LENGTH, LT, MUL, NE, NEXT, REPEAT, RETURN, SUB, SUBSET, SUPERASSIGN, WHILE};

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
        CONCAT => concat_primitive(heap, interner, args, caller_env),
        SUBSET => subset_primitive(heap, interner, args, caller_env),
        LENGTH => length_primitive(heap, interner, args, caller_env),
        COLON => colon_primitive(heap, interner, args, caller_env),
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
    // Unary forms: `+x` is the identity, `-x` negates — GNU folds both into
    // the same primitive as the binary operators. Arity is decided by the
    // call's actual operand count, not by successful prefix extraction.
    let arity = raw_args(heap, args).len();
    if matches!(op, ADD | SUB) && arity == 1 {
        if let Ok(unary) = eval_args(heap, interner, args, caller_env, 1) {
            let operand = numeric_operands(heap, unary[0]).map_err(RSignal::from)?;
            return match operand {
                Numeric::Integer(values) => {
                    let folded = values
                        .into_iter()
                        .map(|v| {
                            if v == crate::value::object::NA_INTEGER || op == ADD {
                                v
                            } else {
                                v.wrapping_neg()
                            }
                        })
                        .collect();
                    Ok(heap.alloc(RObject::Integer(folded)))
                }
                Numeric::Real(values) => {
                    let folded = values
                        .into_iter()
                        .map(|v| if op == ADD { v } else { -v })
                        .collect();
                    Ok(heap.alloc(RObject::Real(folded)))
                }
            };
        }
    }
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

/// Every primitive print name registered in the scaffolding table — the
/// canonical list bootstrap environments iterate so the two never drift.
pub fn primitive_names() -> &'static [&'static str] {
    &[
        "+", "-", "*", "/", "<", ">", "<=", ">=", "==", "!=", "if", "{", "<-", "=",
        "<<-", "return", "while", "repeat", "for", "break", "next", "function", "c", "[",
        "length", ":",
    ]
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
        "c" => Some(CONCAT),
        "[" => Some(SUBSET),
        "length" => Some(LENGTH),
        ":" => Some(COLON),
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

/// `c(...)` — concatenation with GNU coercion. Arguments evaluate eagerly;
/// NULL operands drop out. All operands coerce to the highest type present
/// in the logical < integer < real < character hierarchy. Names are outside
/// the migration slice.
fn concat_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Variadic: evaluate and force every operand in the call.
    let mut operands = Vec::new();
    let mut cursor = args;
    loop {
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

    enum Flat {
        Logical(Vec<i32>),
        Integer(Vec<i32>),
        Real(Vec<f64>),
        String(Vec<super::intern::InternedStr>),
    }
    let mut flattened = Vec::new();
    let mut rank = 0u8;
    for operand in operands {
        let flat = match heap.object(operand) {
            Some(RObject::Nil) => continue,
            Some(RObject::Logical(values)) => {
                rank = rank.max(1);
                Flat::Logical(values.clone())
            }
            Some(RObject::Integer(values)) => {
                rank = rank.max(2);
                Flat::Integer(values.clone())
            }
            Some(RObject::Real(values)) => {
                rank = rank.max(3);
                Flat::Real(values.clone())
            }
            Some(RObject::String(values)) => {
                rank = rank.max(4);
                Flat::String(values.clone())
            }
            _ => {
                return Err(EvalError::TypeError("c() operands must be atomic vectors").into());
            }
        };
        flattened.push(flat);
    }
    if flattened.is_empty() {
        return Ok(ValueId::NIL);
    }

    fn logicals_of(flat: &Flat) -> Vec<i32> {
        match flat {
            Flat::Logical(v) => v.clone(),
            _ => unreachable!("lower ranks coerce upward before extraction"),
        }
    }
    fn integers_of(flat: &Flat) -> Vec<i32> {
        match flat {
            Flat::Logical(v) | Flat::Integer(v) => v.clone(),
            _ => unreachable!("lower ranks coerce upward before extraction"),
        }
    }
    fn reals_of(flat: &Flat) -> Vec<f64> {
        match flat {
            Flat::Logical(v) | Flat::Integer(v) => v
                .iter()
                .map(|x| {
                    if *x == crate::value::object::NA_INTEGER {
                        f64::from_bits(crate::value::object::NA_REAL_BITS)
                    } else {
                        *x as f64
                    }
                })
                .collect(),
            Flat::Real(v) => v.clone(),
            Flat::String(_) => unreachable!("lower ranks coerce upward before extraction"),
        }
    }

    match rank {
        1 => {
            let mut out = Vec::new();
            for flat in &flattened {
                out.extend(logicals_of(flat));
            }
            Ok(heap.alloc(RObject::Logical(out)))
        }
        2 => {
            let mut out = Vec::new();
            for flat in &flattened {
                out.extend(integers_of(flat));
            }
            Ok(heap.alloc(RObject::Integer(out)))
        }
        3 => {
            let mut out = Vec::new();
            for flat in &flattened {
                out.extend(reals_of(flat));
            }
            Ok(heap.alloc(RObject::Real(out)))
        }
        _ => {
            // Character coercion: every operand renders through its ASCII
            // element text, matching GNU's paste-style coercion for the
            // atomic numeric types in the migration slice.
            let mut out = Vec::new();
            for flat in flattened {
                match flat {
                    Flat::String(values) => out.extend(values),
                    Flat::Logical(values) => {
                        for v in values {
                            let text = if v == crate::value::object::NA_INTEGER {
                                "NA".to_string()
                            } else if v != 0 {
                                "TRUE".to_string()
                            } else {
                                "FALSE".to_string()
                            };
                            out.push(interner.intern_string(&text));
                        }
                    }
                    Flat::Integer(values) => {
                        for v in values {
                            let text = if v == crate::value::object::NA_INTEGER {
                                "NA".to_string()
                            } else {
                                v.to_string()
                            };
                            out.push(interner.intern_string(&text));
                        }
                    }
                    Flat::Real(values) => {
                        for v in values {
                            let text = if v.to_bits() == crate::value::object::NA_REAL_BITS {
                                "NA".to_string()
                            } else if v.fract() == 0.0 && v.abs() < 1e15 {
                                format!("{}", v as i64)
                            } else {
                                format!("{v}")
                            };
                            out.push(interner.intern_string(&text));
                        }
                    }
                }
            }
            Ok(heap.alloc(RObject::String(out)))
        }
    }
}

/// `length(x)` — element count as a length-one integer, GNU exactly for
/// atomic and list vectors.
fn length_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let length = match heap.object(operands[0]) {
        Some(RObject::Logical(v)) => v.len(),
        Some(RObject::Integer(v)) => v.len(),
        Some(RObject::Real(v)) => v.len(),
        Some(RObject::Complex(v)) => v.len(),
        Some(RObject::String(v)) => v.len(),
        Some(RObject::Raw(v)) => v.len(),
        Some(RObject::List(v)) => v.len(),
        Some(RObject::Nil) => 0,
        _ => return Err(EvalError::TypeError("length requires a vector").into()),
    };
    Ok(heap.alloc(RObject::Integer(vec![length as i32])))
}

/// `from:to` — unit-step sequence, ascending or descending, inclusive of
/// `to` when it lands exactly. Integer when both operands are integer,
/// real otherwise — GNU's documented behaviour for the slice.
fn colon_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let numeric = |value: ValueId| -> Result<(f64, bool), RSignal> {
        match heap.object(value) {
            Some(RObject::Integer(v)) if !v.is_empty() => Ok((v[0] as f64, true)),
            Some(RObject::Real(v)) if !v.is_empty() => Ok((v[0], false)),
            Some(RObject::Logical(v)) if !v.is_empty() => Ok((v[0] as f64, true)),
            _ => Err(EvalError::TypeError("NA/NaN argument").into()),
        }
    };
    let (from, _from_type) = numeric(operands[0])?;
    let (to, _to_type) = numeric(operands[1])?;
    // GNU's ncolon: integer result when both operands are integer-valued
    // and in int range — literal storage type (double) is irrelevant.
    let from_int = from.fract() == 0.0 && from.abs() <= i32::MAX as f64;
    let to_int = to.fract() == 0.0 && to.abs() <= i32::MAX as f64;
    let mut out = Vec::new();
    if from <= to {
        let mut x = from;
        while x <= to {
            out.push(x);
            x += 1.0;
        }
    } else {
        let mut x = from;
        while x >= to {
            out.push(x);
            x -= 1.0;
        }
    }
    if from_int && to_int {
        Ok(heap.alloc(RObject::Integer(out.into_iter().map(|x| x as i32).collect())))
    } else {
        Ok(heap.alloc(RObject::Real(out)))
    }
}

/// `x[i]` — subsetting with GNU indexing semantics for the atomic vector
/// types in the migration slice. Positive integer indices are 1-based;
/// out-of-range positive indices and NA indices yield NA elements.
/// Negative indices exclude. Logical indices recycle across the vector.
/// Zero indices drop out. Real indices truncate toward zero, exactly GNU.
fn subset_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    subset_impl(heap, operands[0], operands[1])
}

/// Subsetting core operating on already-evaluated operands — shared by the
/// primitive and by the real-index truncation recursion.
fn subset_impl(heap: &mut Heap, source: ValueId, index: ValueId) -> EvalResult {
    enum Selection {
        /// Explicit 0-based positions; usize::MAX encodes an NA element.
        Positions(Vec<usize>),
        /// Positions to exclude (0-based, no duplicates required).
        Excluding(Vec<usize>),
    }
    let source_length = vector_length(heap, source)
        .ok_or(EvalError::TypeError("subscript requires a vector"))
        .map_err(RSignal::from)?;
    let selection = match heap.object(index) {
        Some(RObject::Integer(values)) => {
            if values.iter().any(|v| *v < 0 && *v != crate::value::object::NA_INTEGER) {
                Selection::Excluding(
                    values
                        .iter()
                        .filter(|v| **v < 0 && **v != crate::value::object::NA_INTEGER)
                        .map(|v| (-*v - 1) as usize)
                        .collect(),
                )
            } else {
                Selection::Positions(
                    values
                        .iter()
                        .map(|v| {
                            if *v == crate::value::object::NA_INTEGER {
                                usize::MAX
                            } else if *v == 0 {
                                usize::MAX - 1 // drop marker, filtered below
                            } else {
                                (*v - 1) as usize
                            }
                        })
                        .collect(),
                )
            }
        }
        Some(RObject::Real(values)) => {
            let truncs: Vec<i32> = values.iter().map(|v| *v as i32).collect();
            let truncated = heap.alloc(RObject::Integer(truncs));
            return subset_impl(heap, source, truncated);
        }
        Some(RObject::Logical(values)) => {
            let mut positions = Vec::new();
            for i in 0..source_length {
                let flag = values[i % values.len().max(1)];
                if flag == crate::value::object::NA_INTEGER {
                    positions.push(usize::MAX);
                } else if flag != 0 {
                    positions.push(i);
                }
            }
            Selection::Positions(positions)
        }
        _ => return Err(EvalError::TypeError("unsupported subscript type").into()),
    };

    // Materialize the final 0-based position list.
    let positions: Vec<usize> = match selection {
        Selection::Positions(list) => list
            .into_iter()
            .filter(|p| *p != usize::MAX - 1) // drop zero-index markers
            .collect(),
        Selection::Excluding(excluded) => (0..source_length)
            .filter(|i| !excluded.contains(i))
            .collect(),
    };

    extract_elements(heap, source, &positions).map_err(RSignal::from)
}

fn vector_length(heap: &Heap, value: ValueId) -> Option<usize> {
    match heap.object(value) {
        Some(RObject::Logical(v)) => Some(v.len()),
        Some(RObject::Integer(v)) => Some(v.len()),
        Some(RObject::Real(v)) => Some(v.len()),
        Some(RObject::String(v)) => Some(v.len()),
        Some(RObject::Raw(v)) => Some(v.len()),
        Some(RObject::List(v)) => Some(v.len()),
        _ => None,
    }
}

/// Extract positions from an atomic vector into a fresh vector of the same
/// type; usize::MAX positions yield the type's NA element.
fn extract_elements(
    heap: &mut Heap,
    source: ValueId,
    positions: &[usize],
) -> Result<ValueId, EvalError> {
    macro_rules! extract {
        ($variant:ident, $na:expr) => {
            match heap.object(source) {
                Some(RObject::$variant(values)) => {
                    let out: Vec<_> = positions
                        .iter()
                        .map(|p| {
                            if *p == usize::MAX || *p >= values.len() {
                                $na
                            } else {
                                values[*p]
                            }
                        })
                        .collect();
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    extract!(Logical, crate::value::object::NA_INTEGER);
    extract!(Integer, crate::value::object::NA_INTEGER);
    extract!(Real, f64::from_bits(crate::value::object::NA_REAL_BITS));
    extract!(Raw, 0u8);
    match heap.object(source) {
        Some(RObject::String(values)) => {
            let na_text = values.first().copied();
            let out: Vec<_> = positions
                .iter()
                .map(|p| {
                    if *p == usize::MAX || *p >= values.len() {
                        na_text.unwrap_or(super::intern::InternedStr(0))
                    } else {
                        values[*p]
                    }
                })
                .collect();
            Ok(heap.alloc(RObject::String(out)))
        }
        Some(RObject::List(values)) => {
            // List out-of-range positions are NULL elements in GNU's [[;
            // the migration slice rejects them rather than fabricate.
            if positions.iter().any(|p| *p == usize::MAX || *p >= values.len()) {
                return Err(EvalError::TypeError("list subscript out of bounds"));
            }
            let out: Vec<_> = positions.iter().map(|p| values[*p]).collect();
            Ok(heap.alloc(RObject::List(out)))
        }
        _ => Err(EvalError::TypeError("subscript requires a vector")),
    }
}
