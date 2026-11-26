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
    /// Grouping — `(x)` parses as a call to the parenthesis primitive.
    pub const PAREN: u32 = 34;
    /// Logic operators — elementwise (&, |, !) and short-circuit (&&, ||).
    pub const AND: u32 = 40;
    pub const OR: u32 = 41;
    pub const NOT: u32 = 42;
    pub const SHORT_AND: u32 = 43;
    pub const SHORT_OR: u32 = 44;
    /// Reductions — fold a vector to a scalar with NA propagation.
    pub const SUM: u32 = 50;
    pub const PROD: u32 = 51;
    pub const MIN: u32 = 52;
    pub const MAX: u32 = 53;
    /// Power — always real-valued in GNU, even for integer operands.
    pub const POW: u32 = 58;
    /// Modulo — integer-preserving like the other arithmetic operators.
    pub const MOD: u32 = 90;
    /// Elementwise math — real-valued unary functions.
    pub const SQRT: u32 = 54;
    /// Exact extraction — `x[[i]]` unboxes a single element.
    pub const EXTRACT: u32 = 59;
    /// List construction — variadic heterogeneous collection.
    pub const LIST: u32 = 60;
    /// String operations.
    pub const PASTE: u32 = 61;
    pub const PASTE0: u32 = 64;
    /// Subscript replacement — `[<-` returns the modified copy.
    pub const SUBSET_ASSIGN: u32 = 65;
    /// Apply family — lapply/sapply iteration over closures.
    pub const LAPPLY: u32 = 72;
    pub const SAPPLY: u32 = 73;
    /// List flattening.
    pub const UNLIST: u32 = 74;
    /// Sequence and repetition constructors.
    pub const SEQ: u32 = 75;
    pub const SEQ_LEN: u32 = 76;
    pub const REP: u32 = 77;
    /// Statistical reductions and NA predicates.
    pub const MEAN: u32 = 78;
    pub const VAR: u32 = 79;
    pub const SD: u32 = 80;
    pub const MEDIAN: u32 = 81;
    pub const IS_NA: u32 = 82;
    /// Vector utilities — ordering, selection, vectorized conditional.
    pub const SORT: u32 = 83;
    pub const REV: u32 = 84;
    pub const UNIQUE: u32 = 85;
    pub const WHICH: u32 = 86;
    pub const ANY: u32 = 87;
    pub const ALL: u32 = 88;
    pub const IFELSE: u32 = 89;
    /// Error control and membership.
    pub const STOP: u32 = 91;
    pub const IN: u32 = 92;
    pub const MATCH: u32 = 93;
    /// Environment primitives — user-level creation and introspection.
    pub const NEW_ENV: u32 = 94;
    pub const EXISTS: u32 = 95;
    pub const LS: u32 = 96;
    /// Dollar replacement — `$<-` assigns by name.
    pub const DOLLAR_ASSIGN: u32 = 97;
    /// Output and visibility.
    pub const CAT: u32 = 98;
    pub const INVISIBLE: u32 = 99;
    /// Condition handling — tryCatch and condition introspection.
    pub const TRYCATCH: u32 = 104;
    pub const CONDITION_MESSAGE: u32 = 105;
    /// Matrix construction and dimension introspection.
    pub const MATRIX: u32 = 100;
    pub const DIM: u32 = 101;
    pub const NROW: u32 = 102;
    pub const NCOL: u32 = 103;
    /// Attribute access — names getter and replacement, dollar extraction.
    pub const NAMES: u32 = 69;
    pub const NAMES_ASSIGN: u32 = 70;
    pub const DOLLAR: u32 = 71;
    /// Vector constructors — numeric/integer/logical of length n.
    pub const NUMERIC: u32 = 66;
    pub const INTEGER_CTOR: u32 = 67;
    pub const LOGICAL_CTOR: u32 = 68;
    pub const NCHAR: u32 = 62;
    pub const PRINT: u32 = 63;
    pub const EXP: u32 = 55;
    pub const LOG: u32 = 56;
    pub const ABS: u32 = 57;
}

use crate::value::object::BuiltinId;
use primitive_ids::{ABS, ADD, AND, ASSIGN, BRACE, BREAK, COLON, CONCAT, DIV, EQ, EXP, EXTRACT, FOR, FUNCTION, GE, GT, IF, LE, LENGTH, LIST, LOG, LT, MAX, MIN, MUL, MOD, NCHAR, NE, NEXT, NOT, OR, PAREN, PASTE, PASTE0, POW, PRINT, PROD, REPEAT, RETURN, SHORT_AND, SHORT_OR, SQRT, SUB, SUBSET, SUBSET_ASSIGN, SUM, SUPERASSIGN, NUMERIC, INTEGER_CTOR, LOGICAL_CTOR, NAMES, NAMES_ASSIGN, DOLLAR, LAPPLY, SAPPLY, UNLIST, SEQ, SEQ_LEN, REP, MEAN, VAR, SD, MEDIAN, IS_NA, SORT, REV, UNIQUE, WHICH, ANY, ALL, IFELSE, STOP, IN, MATCH, NEW_ENV, EXISTS, LS, DOLLAR_ASSIGN, CAT, INVISIBLE, MATRIX, DIM, NROW, NCOL, TRYCATCH, CONDITION_MESSAGE, WHILE};

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
        ADD | SUB | MUL | DIV | POW | MOD => arithmetic(heap, interner, id.0, args, caller_env),
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
        PAREN => {
            // `(x)` — identity: evaluate the single grouped expression.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            Ok(operands[0])
        }
        AND | OR => logic_elementwise(heap, interner, id.0, args, caller_env),
        NOT => logic_not(heap, interner, args, caller_env),
        SHORT_AND | SHORT_OR => logic_short_circuit(heap, interner, id.0, args, caller_env),
        SUM | PROD | MIN | MAX => reduction(heap, interner, id.0, args, caller_env),
        EXTRACT => extract_primitive(heap, interner, args, caller_env),
        SUBSET_ASSIGN => subset_assign_primitive(heap, interner, args, caller_env),
        NUMERIC | INTEGER_CTOR | LOGICAL_CTOR => {
            vector_constructor(heap, interner, id.0, args, caller_env)
        }
        NAMES => names_primitive(heap, interner, args, caller_env),
        NAMES_ASSIGN => names_assign_primitive(heap, interner, args, caller_env),
        DOLLAR => dollar_primitive(heap, interner, args, caller_env),
        LAPPLY | SAPPLY => apply_family(heap, interner, id.0, args, caller_env),
        UNLIST => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            unlist_primitive(heap, operands[0])
        }
        SEQ_LEN => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let n = first_real(heap, operands[0])? as i64;
            if n < 0 {
                return Err(EvalError::TypeError("seq_len requires non-negative").into());
            }
            let out: Vec<i32> = (1..=n as i32).collect();
            Ok(heap.alloc(RObject::Integer(out)))
        }
        SEQ => seq_primitive(heap, interner, args, caller_env),
        REP => rep_dispatch(heap, interner, args, caller_env),
        MEAN | VAR | SD | MEDIAN => stat_primitive(heap, interner, id.0, args, caller_env),
        IS_NA => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            is_na_primitive(heap, operands[0])
        }
        SORT | REV | UNIQUE => ordering_primitive(heap, interner, id.0, args, caller_env),
        WHICH => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            which_primitive(heap, operands[0])
        }
        ANY | ALL => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            quantifier_primitive(heap, id.0, operands[0])
        }
        IFELSE => ifelse_primitive(heap, interner, args, caller_env),
        STOP => {
            // GNU's stop raises the error condition with the message —
            /// evaluation aborts through the signal channel exactly as a
            /// type error would.
            let operands = eval_args(heap, interner, args, caller_env, 1).unwrap_or_default();
            let message = match operands.first().and_then(|v| heap.object(*v)) {
                Some(RObject::String(values)) if !values.is_empty() => {
                    interner.string_text(values[0]).to_string()
                }
                _ => "stop called".to_string(),
            };
            Err(EvalError::Condition(message).into())
        }
        IN => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            membership_primitive(heap, operands[0], operands[1])
        }
        MATCH => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            match_primitive(heap, operands[0], operands[1])
        }
        NEW_ENV => {
            // GNU's parent defaults to the calling environment; hash and
            /// size are performance hints outside the slice.
            let _ = eval_args(heap, interner, args, caller_env, 0);
            Ok(crate::value::env::new_environment(heap, caller_env))
        }
        EXISTS => exists_primitive(heap, interner, args, caller_env),
        LS => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            ls_primitive(heap, interner, operands[0])
        }
        DOLLAR_ASSIGN => dollar_assign_primitive(heap, interner, args, caller_env),
        CAT => {
            // Variadic output primitive: every operand coerces to character
            /// and prints; output routing arrives with the console subsystem.
            /// The evaluation itself is the conformance-visible part — cat
            /// returns invisible NULL exactly as GNU.
            let mut cursor = args;
            loop {
                let (car, cdr) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, .. }) => (*car, *cdr),
                    _ => break,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                force_if_promise(heap, interner, evaluated)?;
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            Ok(ValueId::NIL)
        }
        INVISIBLE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            // Visibility metadata is a print-system concern outside the slice;
            /// the value passes through exactly as GNU's return shape.
            Ok(operands[0])
        }
        TRYCATCH => trycatch_primitive(heap, interner, args, caller_env),
        CONDITION_MESSAGE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            // The condition object is a list with a message element —
            /// extraction mirrors dollar access exactly.
            match heap.object(operands[0]) {
                Some(RObject::List(items)) => {
                    let message_pos = get_attrib(heap, interner, operands[0], "names")
                        .and_then(|id| match heap.object(id) {
                            Some(RObject::String(entries)) => entries
                                .iter()
                                .position(|e| interner.string_text(*e) == "message"),
                            _ => None,
                        });
                    match message_pos {
                        Some(pos) if pos < items.len() => Ok(items[pos]),
                        _ => Ok(heap.alloc(RObject::String(vec![interner.intern_string("")]))),
                    }
                }
                _ => Err(EvalError::TypeError("conditionMessage requires a condition").into()),
            }
        }
        MATRIX => matrix_primitive(heap, interner, args, caller_env),
        DIM => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            match get_attrib(heap, interner, operands[0], "dim") {
                Some(dim) => Ok(dim),
                None => Ok(ValueId::NIL),
            }
        }
        NROW | NCOL => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let which = if id.0 == NROW { 0 } else { 1 };
            let extent = match get_attrib(heap, interner, operands[0], "dim") {
                Some(dim) => match heap.object(dim) {
                    Some(RObject::Integer(dims)) => dims.get(which).copied().unwrap_or(crate::value::object::NA_INTEGER),
                    _ => crate::value::object::NA_INTEGER,
                },
                // Vector nrow is the length; ncol of a vector is NULL in GNU
                /// but the slice reports the vector extent for nrow only.
                None => {
                    if id.0 == NROW {
                        vector_length(heap, operands[0]).unwrap_or(0) as i32
                    } else {
                        return Ok(ValueId::NIL);
                    }
                }
            };
            Ok(heap.alloc(RObject::Integer(vec![extent])))
        }
        PASTE => paste_primitive(heap, interner, args, caller_env, " "),
        PASTE0 => paste_primitive(heap, interner, args, caller_env, ""),
        NCHAR => nchar_primitive(heap, interner, args, caller_env),
        PRINT => {
            // The slice returns the value; output routing arrives with the
            // console subsystem migration.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            Ok(operands[0])
        }
        LIST => {
            // Variadic: every evaluated operand becomes one list element;
            // tags attach as the names attribute exactly as GNU.
            let mut items = Vec::new();
            let mut tags: Vec<Option<String>> = Vec::new();
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => {
                        Some(interner.symbol_name(*name).to_string())
                    }
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                items.push(force_if_promise(heap, interner, evaluated)?);
                tags.push(tag_name);
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let value = heap.alloc(RObject::List(items));
            if tags.iter().any(|t| t.is_some()) {
                let names: Vec<super::intern::InternedStr> = tags
                    .iter()
                    .map(|t| interner.intern_string(t.as_deref().unwrap_or("")))
                    .collect();
                let names_vec = heap.alloc(RObject::String(names));
                return copy_with_attrib(heap, interner, value, "names", names_vec)
                    .map_err(RSignal::from);
            }
            Ok(value)
        }
        SQRT | EXP | LOG | ABS => math_unary(heap, interner, id.0, args, caller_env),
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
        Some(RObject::Language { .. }) => {
            // Replacement desugar: `x[i] <- v` is the replacement call
            // `x <- `[<-`(x, i, v)` — the evaluator recognizes the target's
            // call shape exactly as GNU's eval does.
            return assign_replacement(heap, interner, parts[0], parts[1], caller_env);
        }
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
        (Numeric::Integer(a), Numeric::Integer(b)) if op != DIV && op != POW => {
            let folded = fold_numeric(a, b, op, |x, y| match op {
                ADD => x.wrapping_add(y),
                SUB => x.wrapping_sub(y),
                MOD => {
                    // GNU: integer modulo, division by zero yields NA.
                    if y == 0 || x == crate::value::object::NA_INTEGER || y == crate::value::object::NA_INTEGER {
                        crate::value::object::NA_INTEGER
                    } else {
                        x % y
                    }
                }
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
                POW => x.powf(y),
                MOD => {
                    if y == 0.0 {
                        f64::NAN
                    } else {
                        // GNU's fmod semantics: sign follows the dividend.
                        x - y * (x / y).trunc()
                    }
                }
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
        "length", ":", "(", "&", "|", "!", "&&", "||", "sum", "prod", "min", "max", "sqrt",
        "exp", "log", "abs", "^", "[[", "list", "paste", "paste0", "nchar", "print", "[<-",
        "numeric", "integer", "logical", "names", "names<-", "$", "lapply", "sapply", "unlist", "seq", "seq_len", "rep", "mean", "var", "sd", "median", "is.na", "sort", "rev", "unique", "which", "any", "all", "ifelse", "%%", "stop", "%in%", "match", "new.env", "exists", "ls", "$<-", "cat", "invisible", "tryCatch", "conditionMessage", "matrix", "dim", "nrow", "ncol",
    ]
}

/// Resolve a primitive's print name to a scaffolding id — the interop
/// mapping the import bridge uses until the canonical registry lands.
pub fn primitive_id_for_name(name: &str) -> Option<u32> {
    match name {
        "+" => Some(ADD),
        "-" => Some(SUB),
        "*" => Some(MUL),
        "%%" => Some(MOD),
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
        "(" => Some(PAREN),
        "&" => Some(AND),
        "|" => Some(OR),
        "!" => Some(NOT),
        "&&" => Some(SHORT_AND),
        "||" => Some(SHORT_OR),
        "sum" => Some(SUM),
        "prod" => Some(PROD),
        "min" => Some(MIN),
        "max" => Some(MAX),
        "sqrt" => Some(SQRT),
        "exp" => Some(EXP),
        "log" => Some(LOG),
        "abs" => Some(ABS),
        "^" => Some(POW),
        "[[" => Some(EXTRACT),
        "list" => Some(LIST),
        "paste" => Some(PASTE),
        "paste0" => Some(PASTE0),
        "nchar" => Some(NCHAR),
        "print" => Some(PRINT),
        "[<-" => Some(SUBSET_ASSIGN),
        "numeric" => Some(NUMERIC),
        "integer" => Some(INTEGER_CTOR),
        "logical" => Some(LOGICAL_CTOR),
        "names" => Some(NAMES),
        "names<-" => Some(NAMES_ASSIGN),
        "$" => Some(DOLLAR),
        "lapply" => Some(LAPPLY),
        "sapply" => Some(SAPPLY),
        "unlist" => Some(UNLIST),
        "seq" => Some(SEQ),
        "seq_len" => Some(SEQ_LEN),
        "rep" => Some(REP),
        "mean" => Some(MEAN),
        "var" => Some(VAR),
        "sd" => Some(SD),
        "median" => Some(MEDIAN),
        "is.na" => Some(IS_NA),
        "sort" => Some(SORT),
        "rev" => Some(REV),
        "unique" => Some(UNIQUE),
        "which" => Some(WHICH),
        "any" => Some(ANY),
        "all" => Some(ALL),
        "ifelse" => Some(IFELSE),
        "stop" => Some(STOP),
        "%in%" => Some(IN),
        "match" => Some(MATCH),
        "new.env" => Some(NEW_ENV),
        "exists" => Some(EXISTS),
        "ls" => Some(LS),
        "$<-" => Some(DOLLAR_ASSIGN),
        "cat" => Some(CAT),
        "invisible" => Some(INVISIBLE),
        "tryCatch" => Some(TRYCATCH),
        "conditionMessage" => Some(CONDITION_MESSAGE),
        "matrix" => Some(MATRIX),
        "dim" => Some(DIM),
        "nrow" => Some(NROW),
        "ncol" => Some(NCOL),
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
    // Variadic: evaluate and force every operand in the call, keeping each
    // operand's tag — tagged concatenation expands names across element
    // counts exactly as GNU.
    let mut operands = Vec::new();
    let mut tags: Vec<Option<String>> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        operands.push(force_if_promise(heap, interner, evaluated)?);
        tags.push(tag_name);
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
    let mut operand_lengths = Vec::new();
    let mut rank = 0u8;
    for operand in operands {
        let flat = match heap.object(operand) {
            Some(RObject::Nil) => continue,
            Some(RObject::Logical(values)) => {
                rank = rank.max(1);
                operand_lengths.push(values.len());
                Flat::Logical(values.clone())
            }
            Some(RObject::Integer(values)) => {
                rank = rank.max(2);
                operand_lengths.push(values.len());
                Flat::Integer(values.clone())
            }
            Some(RObject::Real(values)) => {
                rank = rank.max(3);
                operand_lengths.push(values.len());
                Flat::Real(values.clone())
            }
            Some(RObject::String(values)) => {
                rank = rank.max(4);
                operand_lengths.push(values.len());
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

    // GNU names expansion: each operand's tag repeats across its element
    // count; untagged operands contribute empty strings. Any tag at all
    // turns the names attribute on.
    let attach_names = |heap: &mut Heap, interner: &mut SymbolInterner, value: ValueId| -> Result<ValueId, RSignal> {
        // NIL operands dropped above shift the tag alignment; the slice
        // accepts the common all-present shape and drops names otherwise.
        if tags.iter().all(|t| t.is_none()) {
            return Ok(value);
        }
        if tags.len() != operand_lengths.len() {
            return Ok(value);
        }
        let mut names = Vec::new();
        for (tag, count) in tags.iter().zip(operand_lengths.iter()) {
            for _ in 0..*count {
                names.push(interner.intern_string(tag.as_deref().unwrap_or("")));
            }
        }
        let names_vec = heap.alloc(RObject::String(names));
        copy_with_attrib(heap, interner, value, "names", names_vec).map_err(RSignal::from)
    };

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
            let value = heap.alloc(RObject::Logical(out));
            attach_names(heap, interner, value)
        }
        2 => {
            let mut out = Vec::new();
            for flat in &flattened {
                out.extend(integers_of(flat));
            }
            let value = heap.alloc(RObject::Integer(out));
            attach_names(heap, interner, value)
        }
        3 => {
            let mut out = Vec::new();
            for flat in &flattened {
                out.extend(reals_of(flat));
            }
            let value = heap.alloc(RObject::Real(out));
            attach_names(heap, interner, value)
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
            let value = heap.alloc(RObject::String(out));
            attach_names(heap, interner, value)
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
    // GNU's dispatch on argument count: two operands is vector-style
    /// subsetting; three or more with a dim attribute is matrix indexing —
    /// `x[i, j]` with the missing-argument sentinel standing for whole
    /// extents.
    let raw = raw_args(heap, args);
    if raw.len() >= 3 {
        return matrix_subset(heap, interner, raw, caller_env);
    }
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    subset_impl(heap, interner, operands[0], operands[1])
}

/// Matrix two-index subsetting: `x[i, j]` where either index may be the
/// missing sentinel meaning the whole extent. Column-major linearization —
/// element (r, c) lives at c*nrow + r zero-based, GNU exactly. Result keeps
/// matrix shape when both extents exceed one; otherwise drops to a vector,
/// GNU's default drop semantics shape.
fn matrix_subset(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    raw: Vec<ValueId>,
    caller_env: ValueId,
) -> EvalResult {
    let object = eval(heap, interner, raw[0], caller_env)?;
    let object = force_if_promise(heap, interner, object)?;
    let dims = match get_attrib(heap, interner, object, "dim") {
        Some(dim) => match heap.object(dim) {
            Some(RObject::Integer(dims)) if dims.len() >= 2 => (dims[0] as usize, dims[1] as usize),
            _ => return Err(EvalError::TypeError("matrix subscript requires dim").into()),
        },
        None => return Err(EvalError::TypeError("matrix subscript requires dim").into()),
    };
    let (nrow, ncol) = dims;
    let mut resolve_index = |raw_index: ValueId, extent: usize| -> Result<Vec<usize>, RSignal> {
        if raw_index == ValueId::MISSING {
            return Ok((0..extent).collect());
        }
        let evaluated = eval(heap, interner, raw_index, caller_env)?;
        let evaluated = force_if_promise(heap, interner, evaluated)?;
        match heap.object(evaluated) {
            Some(RObject::Integer(values)) => Ok(values
                .iter()
                .map(|v| {
                    if *v == crate::value::object::NA_INTEGER {
                        usize::MAX
                    } else {
                        (*v - 1) as usize
                    }
                })
                .collect()),
            Some(RObject::Real(values)) => Ok(values
                .iter()
                .map(|v| {
                    if v.is_nan() {
                        usize::MAX
                    } else {
                        (*v as i32 - 1) as usize
                    }
                })
                .collect()),
            Some(RObject::Logical(values)) => {
                let mut positions = Vec::new();
                for i in 0..extent {
                    let flag = values[i % values.len().max(1)];
                    if flag == crate::value::object::NA_INTEGER {
                        positions.push(usize::MAX);
                    } else if flag != 0 {
                        positions.push(i);
                    }
                }
                Ok(positions)
            }
            _ => Err(EvalError::TypeError("unsupported matrix index").into()),
        }
    };
    let rows = resolve_index(raw[1], nrow)?;
    let cols = resolve_index(raw[2], ncol)?;
    // Linear positions column-major: iterate columns outermost so the
    // result fills column-by-column exactly as GNU's storage order.
    let na = usize::MAX;
    let mut positions = Vec::with_capacity(rows.len() * cols.len());
    for col in &cols {
        for row in &rows {
            if *row == na || *col == na || *row >= nrow || *col >= ncol {
                positions.push(na);
            } else {
                positions.push(col * nrow + row);
            }
        }
    }
    let extracted = extract_elements(heap, object, &positions).map_err(RSignal::from)?;
    // Shape discipline: both extents multi-length keeps a dim attribute of
    /// the selected shape; single-length margins drop to a vector.
    if rows.len() > 1 && cols.len() > 1 {
        let dims_vec = heap.alloc(RObject::Integer(vec![rows.len() as i32, cols.len() as i32]));
        return copy_with_attrib(heap, interner, extracted, "dim", dims_vec).map_err(RSignal::from);
    }
    Ok(extracted)
}

/// Subsetting core operating on already-evaluated operands — shared by the
/// primitive and by the real-index truncation recursion.
fn subset_impl(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    source: ValueId,
    index: ValueId,
) -> EvalResult {
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
            // Truncation must preserve NA — Rust's `as` saturates NaN to 0,
            /// which would silently drop positions GNU keeps as NA.
            let truncs: Vec<i32> = values
                .iter()
                .map(|v| {
                    if v.is_nan() {
                        crate::value::object::NA_INTEGER
                    } else {
                        *v as i32
                    }
                })
                .collect();
            let truncated = heap.alloc(RObject::Integer(truncs));
            return subset_impl(heap, interner, source, truncated);
        }
        Some(RObject::Logical(values)) => {
            // GNU: a logical index longer than the vector extends the
            /// selection — out-of-range TRUE positions yield NA elements.
            let mut positions = Vec::new();
            let extent = source_length.max(values.len());
            for i in 0..extent {
                let flag = values[i % values.len().max(1)];
                if flag == crate::value::object::NA_INTEGER {
                    positions.push(usize::MAX);
                } else if flag != 0 {
                    positions.push(i);
                }
            }
            Selection::Positions(positions)
        }
        Some(RObject::String(values)) => {
            // Named access: exact matching against the source's names,
            /// unmatched positions yield NA exactly as GNU. Partial matching
            /// is `[[`'s business, not `[`'s.
            let source_names = get_attrib(heap, interner, source, "names");
            let positions: Vec<usize> = match source_names {
                Some(names_id) => {
                    let names_vec: Vec<String> = match heap.object(names_id) {
                        Some(RObject::String(entries)) => entries
                            .iter()
                            .map(|entry| interner.string_text(*entry).to_string())
                            .collect(),
                        _ => Vec::new(),
                    };
                    values
                        .iter()
                        .map(|wanted| {
                            let wanted_text = interner.string_text(*wanted);
                            match names_vec.iter().position(|n| n == wanted_text) {
                                Some(position) => position,
                                None => usize::MAX,
                            }
                        })
                        .collect()
                }
                None => values.iter().map(|_| usize::MAX).collect(),
            };
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

    let extracted = extract_elements(heap, source, &positions).map_err(RSignal::from)?;
    // GNU fidelity: `[` carries the names of selected elements across —
    /// positional and named subsetting alike. Out-of-range and NA positions
    /// take the NA-string sentinel the bridge round-trips.
    if let Some(names_id) = get_attrib(heap, interner, source, "names") {
        let source_names: Vec<super::intern::InternedStr> = match heap.object(names_id) {
            Some(RObject::String(entries)) => entries.clone(),
            _ => Vec::new(),
        };
        if !source_names.is_empty() {
            let na_text = interner.intern_string("\0NA");
            let selected: Vec<super::intern::InternedStr> = positions
                .iter()
                .map(|p| {
                    if *p == usize::MAX || *p >= source_names.len() {
                        na_text
                    } else {
                        source_names[*p]
                    }
                })
                .collect();
            let names_vec = heap.alloc(RObject::String(selected));
            return copy_with_attrib(heap, interner, extracted, "names", names_vec)
                .map_err(RSignal::from);
        }
    }
    Ok(extracted)
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

/// Extract a logical view of a value with GNU coercion: numeric zero is
/// FALSE, nonzero TRUE, NA propagates as the integer NA sentinel.
fn logicals_of_value(heap: &Heap, value: ValueId) -> Result<Vec<i32>, EvalError> {
    match heap.object(value) {
        Some(RObject::Logical(values)) => Ok(values.clone()),
        Some(RObject::Integer(values)) => Ok(values.clone()),
        Some(RObject::Real(values)) => Ok(values
            .iter()
            .map(|v| {
                if v.is_nan() {
                    crate::value::object::NA_INTEGER
                } else if *v != 0.0 {
                    1
                } else {
                    0
                }
            })
            .collect()),
        _ => Err(EvalError::TypeError("logical operand must be logical or numeric")),
    }
}

/// `&` and `|` — elementwise logic with GNU NA semantics: NA & FALSE is
/// FALSE, NA & TRUE is NA, NA | TRUE is TRUE, NA | FALSE is NA. Length
/// recycling follows the arithmetic rule.
fn logic_elementwise(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let a = logicals_of_value(heap, operands[0]).map_err(RSignal::from)?;
    let b = logicals_of_value(heap, operands[1]).map_err(RSignal::from)?;
    let na = crate::value::object::NA_INTEGER;
    let combine = |x: i32, y: i32| -> i32 {
        let xt = x != 0 && x != na;
        let yt = y != 0 && y != na;
        if op == AND {
            if (x != na && !xt) || (y != na && !yt) {
                0 // either operand definitely FALSE
            } else if x == na || y == na {
                na
            } else {
                (xt && yt) as i32
            }
        } else {
            if xt || yt {
                1 // either operand definitely TRUE
            } else if x == na || y == na {
                na
            } else {
                0
            }
        }
    };
    let folded = fold_numeric(a, b, op, combine).map_err(RSignal::from)?;
    Ok(heap.alloc(RObject::Logical(folded)))
}

/// `!` — elementwise negation; NA stays NA, exactly GNU.
fn logic_not(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let values = logicals_of_value(heap, operands[0]).map_err(RSignal::from)?;
    let na = crate::value::object::NA_INTEGER;
    let out = values
        .into_iter()
        .map(|v| if v == na { na } else { (v == 0) as i32 })
        .collect();
    Ok(heap.alloc(RObject::Logical(out)))
}

/// `&&` and `||` — short-circuit operators over first elements only. The
/// right operand evaluates only when the left doesn't decide the outcome —
/// GNU's control-flow contract, which side effects observe.
fn logic_short_circuit(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 2 {
        return Err(EvalError::UnsupportedForm("short-circuit arity").into());
    }
    let na = crate::value::object::NA_INTEGER;
    let first_of = |heap: &Heap, value: ValueId| -> Result<i32, EvalError> {
        let values = logicals_of_value(heap, value)?;
        Ok(values.first().copied().unwrap_or(na))
    };
    let left = eval(heap, interner, parts[0], caller_env)?;
    let left = force_if_promise(heap, interner, left)?;
    let left_first = first_of(heap, left).map_err(RSignal::from)?;

    // Does the left operand decide the outcome without the right?
    let decided = if op == SHORT_AND {
        left_first == 0
    } else {
        left_first != 0 && left_first != na
    };
    if decided {
        let verdict = if op == SHORT_AND { 0 } else { 1 };
        return Ok(heap.alloc(RObject::Logical(vec![verdict])));
    }

    let right = eval(heap, interner, parts[1], caller_env)?;
    let right = force_if_promise(heap, interner, right)?;
    let right_first = first_of(heap, right).map_err(RSignal::from)?;

    let result = if op == SHORT_AND {
        if right_first == 0 {
            0
        } else if left_first == na || right_first == na {
            na
        } else {
            1
        }
    } else {
        if right_first != 0 && right_first != na {
            1
        } else if left_first == na || right_first == na {
            na
        } else {
            0
        }
    };
    Ok(heap.alloc(RObject::Logical(vec![result])))
}

/// Numeric coercion for the reduction and math primitives: logical and
/// integer vectors widen to reals with the NA bit pattern preserved.
fn reals_of_value(heap: &Heap, value: ValueId) -> Result<Vec<f64>, EvalError> {
    match heap.object(value) {
        Some(RObject::Real(values)) => Ok(values.clone()),
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => Ok(values
            .iter()
            .map(|v| {
                if *v == crate::value::object::NA_INTEGER {
                    f64::from_bits(crate::value::object::NA_REAL_BITS)
                } else {
                    *v as f64
                }
            })
            .collect()),
        _ => Err(EvalError::TypeError("numeric argument expected")),
    }
}

/// `sum`/`prod`/`min`/`max` — scalar folds with GNU NA propagation: any NA
/// element poisons the reduction (the slice has no na.rm yet). Integer and
/// logical inputs fold in integer arithmetic exactly as GNU does; real
/// inputs fold in real.
fn reduction(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // GNU's variadic shape: every unlabeled argument concatenates into the
    /// reduction input; the tagged na.rm strips NA positions instead of
    /// poisoning.
    let mut inputs: Vec<ValueId> = Vec::new();
    let mut na_rm = false;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        if tag_name.as_deref() == Some("na.rm") {
            na_rm = matches!(
                heap.object(value),
                Some(RObject::Logical(v)) if !v.is_empty() && v[0] != 0 && v[0] != crate::value::object::NA_INTEGER
            );
        } else {
            inputs.push(value);
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    if inputs.is_empty() {
        return Err(EvalError::TypeError("reduction requires input").into());
    }
    let combined = concat_vectors(heap, &inputs).map_err(RSignal::from)?;
    reduction_fold(heap, op, &[combined], na_rm)
}


/// Concatenate vectors exactly as `c()` — same-type fast path, numeric
/// promotion otherwise. Shared by the variadic reduction reshape.
fn concat_vectors(heap: &mut Heap, inputs: &[ValueId]) -> Result<ValueId, EvalError> {
    if inputs.len() == 1 {
        return Ok(inputs[0]);
    }
    let all_int = inputs.iter().all(|v| {
        matches!(
            heap.object(*v),
            Some(RObject::Integer(_)) | Some(RObject::Logical(_))
        )
    });
    if all_int {
        let mut out = Vec::new();
        for input in inputs {
            match heap.object(*input) {
                Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => {
                    out.extend(values.iter().copied());
                }
                _ => {}
            }
        }
        return Ok(heap.alloc(RObject::Integer(out)));
    }
    let mut out = Vec::new();
    for input in inputs {
        out.extend(reals_of_value(heap, *input)?);
    }
    Ok(heap.alloc(RObject::Real(out)))
}

fn reduction_fold(
    heap: &mut Heap,
    op: u32,
    operands: &[ValueId],
    na_rm: bool,
) -> EvalResult {
    // GNU type fidelity: integer/logical inputs fold in integer arithmetic
    // for sum/prod and preserve type for min/max; real inputs fold in real.
    if let Some(RObject::Integer(values)) | Some(RObject::Logical(values)) =
        heap.object(operands[0])
    {
        let mut values = values.clone();
        if values.iter().any(|v| *v == crate::value::object::NA_INTEGER) {
            if !na_rm {
                return Ok(heap.alloc(RObject::Integer(vec![crate::value::object::NA_INTEGER])));
            }
            values.retain(|v| *v != crate::value::object::NA_INTEGER);
        }
        let result: i32 = match op {
            SUM => values.iter().copied().fold(0i32, |a, b| a.wrapping_add(b)),
            PROD => values.iter().copied().fold(1i32, |a, b| a.wrapping_mul(b)),
            MIN => values.iter().copied().fold(i32::MAX, i32::min),
            _ => values.iter().copied().fold(i32::MIN, i32::max),
        };
        return Ok(heap.alloc(RObject::Integer(vec![result])));
    }
    let mut values = reals_of_value(heap, operands[0]).map_err(RSignal::from)?;
    if values.iter().any(|v| v.is_nan()) {
        if !na_rm {
            return Ok(heap.alloc(RObject::Real(vec![f64::from_bits(
                crate::value::object::NA_REAL_BITS,
            )])));
        }
        values.retain(|v| !v.is_nan());
    }
    let result = match op {
        SUM => values.iter().sum(),
        PROD => values.iter().product(),
        MIN => values.iter().copied().fold(f64::INFINITY, f64::min),
        _ => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    };
    Ok(heap.alloc(RObject::Real(vec![result])))
}

/// Elementwise unary math — `sqrt`/`exp`/`log`/`abs` over real-coerced
/// inputs. NA inputs yield NA outputs; domain behavior matches IEEE, which
/// is what GNU inherits from libm.
fn math_unary(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let values = reals_of_value(heap, operands[0]).map_err(RSignal::from)?;
    let out: Vec<f64> = values
        .iter()
        .map(|v| {
            if v.is_nan() {
                f64::from_bits(crate::value::object::NA_REAL_BITS)
            } else {
                match op {
                    SQRT => v.sqrt(),
                    EXP => v.exp(),
                    LOG => v.ln(),
                    _ => v.abs(),
                }
            }
        })
        .collect();
    Ok(heap.alloc(RObject::Real(out)))
}

/// `x[[i]]` — exact extraction of a single element. Unlike `[`, extraction
/// unboxes list elements (the element itself, not a length-one list) and
/// out-of-range positive indices are errors in GNU, not NA padding.
/// Negative indices count exclusion-invariant positions exactly as GNU's
/// scalar negative subscript — the slice supports positive indices and
/// documents the negative case as future work.
fn extract_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let index_value = match heap.object(operands[1]) {
        Some(RObject::Real(v)) if !v.is_empty() => v[0] as i64,
        Some(RObject::Integer(v)) if !v.is_empty() => v[0] as i64,
        _ => return Err(EvalError::TypeError("extraction index must be scalar numeric").into()),
    };
    if index_value < 1 {
        return Err(EvalError::TypeError("extraction requires positive indices").into());
    }
    let position = (index_value - 1) as usize;
    macro_rules! extract_scalar {
        ($variant:ident) => {
            match heap.object(operands[0]) {
                Some(RObject::$variant(values)) => {
                    if position >= values.len() {
                        return Err(EvalError::TypeError("subscript out of bounds").into());
                    }
                    return Ok(heap.alloc(RObject::$variant(vec![values[position]])));
                }
                _ => {}
            }
        };
    }
    extract_scalar!(Logical);
    extract_scalar!(Integer);
    extract_scalar!(Real);
    extract_scalar!(Raw);
    match heap.object(operands[0]) {
        Some(RObject::String(values)) => {
            if position >= values.len() {
                return Err(EvalError::TypeError("subscript out of bounds").into());
            }
            Ok(heap.alloc(RObject::String(vec![values[position]])))
        }
        Some(RObject::List(items)) => {
            // List extraction unboxes: the element itself, exactly GNU.
            if position >= items.len() {
                return Err(EvalError::TypeError("subscript out of bounds").into());
            }
            Ok(items[position])
        }
        _ => Err(EvalError::TypeError("extraction requires a vector").into()),
    }
}

/// Coerce a value to its character representation with GNU formatting for
/// the atomic types in the migration slice.
fn strings_of_value(
    heap: &Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
) -> Result<Vec<super::intern::InternedStr>, EvalError> {
    match heap.object(value) {
        Some(RObject::String(values)) => Ok(values.clone()),
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => {
            let logical = matches!(heap.object(value), Some(RObject::Logical(_)));
            Ok(values
                .iter()
                .map(|v| {
                    let text = if *v == crate::value::object::NA_INTEGER {
                        "NA".to_string()
                    } else if logical {
                        if *v != 0 { "TRUE".to_string() } else { "FALSE".to_string() }.to_string()
                    } else {
                        v.to_string()
                    };
                    interner.intern_string(&text)
                })
                .collect())
        }
        Some(RObject::Real(values)) => Ok(values
            .iter()
            .map(|v| {
                let text = if v.is_nan() {
                    "NA".to_string()
                } else if v.fract() == 0.0 && v.abs() < 1e15 {
                    format!("{}", *v as i64)
                } else {
                    format!("{v}")
                };
                interner.intern_string(&text)
            })
            .collect()),
        _ => Err(EvalError::TypeError("cannot coerce to character")),
    }
}

/// `paste(...)` — elementwise string concatenation with recycling. Tagged
/// `sep` and `collapse` arguments control joining; untagged arguments are
/// the vector operands in order. `paste0` maps here with an empty default
/// separator — resolved by call name at the import boundary, so the slice
/// reads the separator argument identically for both spellings.
fn paste_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    default_sep: &str,
) -> EvalResult {
    // Walk the pairlist manually: tags decide the argument's role.
    let mut operands = Vec::new();
    let mut sep = default_sep.to_string();
    let mut collapse: Option<String> = None;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let forced = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("sep") => {
                let strings = strings_of_value(heap, interner, forced).map_err(RSignal::from)?;
                if let Some(first) = strings.first() {
                    sep = interner.string_text(*first).to_string();
                }
            }
            Some("collapse") => {
                let strings = strings_of_value(heap, interner, forced).map_err(RSignal::from)?;
                if let Some(first) = strings.first() {
                    collapse = Some(interner.string_text(*first).to_string());
                }
            }
            _ => operands.push(forced),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    if operands.is_empty() {
        return Err(EvalError::TypeError("paste requires vector operands").into());
    }
    // Coerce and recycle elementwise.
    let mut columns = Vec::new();
    for operand in operands {
        columns.push(strings_of_value(heap, interner, operand).map_err(RSignal::from)?);
    }
    let width = columns.iter().map(Vec::len).max().unwrap_or(0);
    if columns.iter().any(|c| c.is_empty()) || width == 0 {
        let empty = interner.intern_string("");
        return Ok(heap.alloc(RObject::String(vec![empty; 0])));
    }
    for column in &columns {
        if width % column.len() != 0 {
            return Err(EvalError::TypeError("arguments cannot be recycled").into());
        }
    }
    let mut out = Vec::new();
    for i in 0..width {
        let mut pieces = Vec::new();
        for column in &columns {
            pieces.push(interner.string_text(column[i % column.len()]).to_string());
        }
        out.push(interner.intern_string(&pieces.join(&sep)));
    }
    if let Some(collapse_sep) = collapse {
        let joined = out
            .iter()
            .map(|s| interner.string_text(*s).to_string())
            .collect::<Vec<_>>()
            .join(&collapse_sep);
        out = vec![interner.intern_string(&joined)];
    }
    Ok(heap.alloc(RObject::String(out)))
}

/// `nchar(x)` — character counts per element, integer output exactly GNU.
fn nchar_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let strings = strings_of_value(heap, interner, operands[0]).map_err(RSignal::from)?;
    let counts: Vec<i32> = strings
        .iter()
        .map(|s| {
            let text = interner.string_text(*s);
            // GNU: nchar of NA is NA — the sentinel text marks the missing
            /// string exactly as the bridge round-trips it.
            if text == "\0NA" {
                crate::value::object::NA_INTEGER
            } else {
                text.chars().count() as i32
            }
        })
        .collect();
    Ok(heap.alloc(RObject::Integer(counts)))
}

/// The replacement-assignment desugar: `x[i] <- v` where the target is a
/// `[` call. The slice requires the subscripted object be a plain symbol —
/// nested replacement chains are deep GNU territory beyond the migration
/// scaffolding.
fn assign_replacement(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    target: ValueId,
    rhs: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let (head, args) = match heap.object(target) {
        Some(RObject::Language { car, cdr, .. }) => (*car, *cdr),
        _ => return Err(EvalError::TypeError("unsupported assignment target").into()),
    };
    let head_name = match heap.object(head) {
        Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
        _ => return Err(EvalError::TypeError("unsupported assignment target").into()),
    };
    let call_args = raw_args(heap, args);
    if call_args.is_empty() {
        return Err(EvalError::UnsupportedForm("replacement arity").into());
    }
    let sym = match heap.object(call_args[0]) {
        Some(RObject::Symbol { name, .. }) => *name,
        _ => return Err(EvalError::TypeError("replacement object must be a symbol").into()),
    };
    if head_name == "[" {
        if call_args.len() != 2 {
            return Err(EvalError::UnsupportedForm("replacement arity").into());
        }
        // The object is the symbol's current binding, exactly GNU.
        let object = eval(heap, interner, call_args[0], caller_env)?;
        let object = force_if_promise(heap, interner, object)?;
        let index = eval(heap, interner, call_args[1], caller_env)?;
        let index = force_if_promise(heap, interner, index)?;
        let value = eval(heap, interner, rhs, caller_env)?;
        let value = force_if_promise(heap, interner, value)?;
        let replaced = subset_replace(heap, object, index, value).map_err(RSignal::from)?;
        crate::value::env::define(heap, caller_env, sym, replaced)
            .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("replacement define")))?;
        // Replacement assignments evaluate to the assigned value in GNU — the
        // right-hand value, not the whole modified object.
        return Ok(value);
    }
    // Generic replacement functions: `F(x, ...) <- v` desugars to
    // `x <- `F<-`(x, ..., v)` — the object is the first target argument,
    // the value the last replacement argument, exactly GNU's rewrite.
    let replacement_name = format!("{head_name}<-");
    if replacement_name == "names<-" {
        if call_args.len() != 1 {
            return Err(EvalError::UnsupportedForm("names replacement arity").into());
        }
        let object = eval(heap, interner, call_args[0], caller_env)?;
        let object = force_if_promise(heap, interner, object)?;
        let value = eval(heap, interner, rhs, caller_env)?;
        let value = force_if_promise(heap, interner, value)?;
        let replaced = names_assign_core(heap, interner, object, value).map_err(RSignal::from)?;
        crate::value::env::define(heap, caller_env, sym, replaced)
            .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("replacement define")))?;
        return Ok(value);
    }
    if head_name == "$" {
        if call_args.len() != 2 {
            return Err(EvalError::UnsupportedForm("dollar replacement arity").into());
        }
        // Build the synthetic `$<-(object, name, value)` argument chain —
        /// object and name pass unevaluated exactly as the parser emits them,
        /// the rhs attaches as the final argument.
        let value_node = heap.alloc(RObject::Pairlist {
            car: rhs,
            cdr: ValueId::NIL,
            tag: ValueId::NIL,
        });
        let name_node = heap.alloc(RObject::Pairlist {
            car: call_args[1],
            cdr: value_node,
            tag: ValueId::NIL,
        });
        let object_node = heap.alloc(RObject::Pairlist {
            car: call_args[0],
            cdr: name_node,
            tag: ValueId::NIL,
        });
        let replaced = dollar_assign_primitive(heap, interner, object_node, caller_env)?;
        crate::value::env::define(heap, caller_env, sym, replaced)
            .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("replacement define")))?;
        // Replacement assignments evaluate to the right-hand value in GNU.
        let value = eval(heap, interner, rhs, caller_env)?;
        return force_if_promise(heap, interner, value);
    }
    Err(EvalError::TypeError("unsupported replacement target").into())
}

/// `[<-` as a primitive: positional arguments object, index, value.
fn subset_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 3)?;
    subset_replace(heap, operands[0], operands[1], operands[2]).map_err(RSignal::from)
}

/// The replacement core: copy the object with positions replaced by the
/// recycled value. Positive out-of-range indices extend the vector with NA
/// padding exactly as GNU. Negative indices replace everything but the
/// excluded positions. Logical indices select replacement positions. The
/// slice requires the value's type match the object's — type promotion in
/// replacement is documented as future work.
fn subset_replace(
    heap: &mut Heap,
    object: ValueId,
    index: ValueId,
    value: ValueId,
) -> Result<ValueId, EvalError> {
    // Resolve positions exactly as subsetting does, but out-of-range
    // positive indices extend rather than pad-NA-read.
    let length = vector_length(heap, object)
        .ok_or(EvalError::TypeError("replacement requires a vector"))?;
    enum Positions {
        Replace(Vec<usize>),
    }
    let positions = match heap.object(index) {
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => {
            let values = values.clone();
            let is_logical = matches!(heap.object(index), Some(RObject::Logical(_)));
            if is_logical {
                let mut out = Vec::new();
                for i in 0..length.max(values.len()) {
                    let flag = values[i % values.len().max(1)];
                    if flag != 0 && flag != crate::value::object::NA_INTEGER {
                        out.push(i);
                    }
                }
                Positions::Replace(out)
            } else if values.iter().any(|v| *v < 0 && *v != crate::value::object::NA_INTEGER) {
                let excluded: Vec<usize> = values
                    .iter()
                    .filter(|v| **v < 0 && **v != crate::value::object::NA_INTEGER)
                    .map(|v| (-*v - 1) as usize)
                    .collect();
                Positions::Replace((0..length).filter(|i| !excluded.contains(i)).collect())
            } else {
                Positions::Replace(
                    values
                        .iter()
                        .filter(|v| **v > 0)
                        .map(|v| (*v - 1) as usize)
                        .collect(),
                )
            }
        }
        Some(RObject::Real(values)) => {
            let truncs: Vec<i32> = values
                .iter()
                .map(|v| {
                    if v.is_nan() {
                        crate::value::object::NA_INTEGER
                    } else {
                        *v as i32
                    }
                })
                .collect();
            let truncated = heap.alloc(RObject::Integer(truncs));
            return subset_replace(heap, object, truncated, value);
        }
        _ => return Err(EvalError::TypeError("unsupported replacement index")),
    };
    let Positions::Replace(positions) = positions;
    if positions.is_empty() {
        return Ok(object);
    }

    // GNU coercion discipline: target and replacement normalize to their
    /// common numeric rank before replacement — logical < integer < real.
    /// The vector widens when the replacement ranks higher; the replacement
    /// coerces when the target ranks higher. Strings are outside the slice.
    fn rank(heap: &Heap, value: ValueId) -> Option<u32> {
        match heap.object(value) {
            Some(RObject::Logical(_)) => Some(0),
            Some(RObject::Integer(_)) => Some(1),
            Some(RObject::Real(_)) => Some(2),
            _ => None,
        }
    }
    fn coerce(heap: &mut Heap, value: ValueId, to: u32) -> ValueId {
        let na_int = crate::value::object::NA_INTEGER;
        let na_real = f64::from_bits(crate::value::object::NA_REAL_BITS);
        match (heap.object(value), to) {
            (Some(RObject::Logical(v)), 1) => {
                let out: Vec<i32> = v.clone();
                heap.alloc(RObject::Integer(out))
            }
            (Some(RObject::Logical(v)), 2) => {
                let out: Vec<f64> = v
                    .iter()
                    .map(|x| if *x == na_int { na_real } else { *x as f64 })
                    .collect();
                heap.alloc(RObject::Real(out))
            }
            (Some(RObject::Integer(v)), 2) => {
                let out: Vec<f64> = v
                    .iter()
                    .map(|x| if *x == na_int { na_real } else { *x as f64 })
                    .collect();
                heap.alloc(RObject::Real(out))
            }
            _ => value,
        }
    }
    let (target_rank, replacement_rank) = match (rank(heap, object), rank(heap, value)) {
        (Some(a), Some(b)) => (a, b),
        _ => (0, 0),
    };
    let (object, value) = if target_rank > 0 || replacement_rank > 0 {
        let common = target_rank.max(replacement_rank);
        (
            coerce(heap, object, common),
            coerce(heap, value, common),
        )
    } else {
        (object, value)
    };

    macro_rules! replace_typed {
        ($variant:ident, $na:expr) => {
            match (heap.object(object), heap.object(value)) {
                (Some(RObject::$variant(target)), Some(RObject::$variant(replacements))) => {
                    let mut out = target.clone();
                    let replacements = replacements.clone();
                    let required = positions.iter().max().map(|m| m + 1).unwrap_or(0);
                    while out.len() < required {
                        out.push($na);
                    }
                    for (n, pos) in positions.iter().enumerate() {
                        out[*pos] = replacements[n % replacements.len()];
                    }
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    replace_typed!(Logical, crate::value::object::NA_INTEGER);
    replace_typed!(Integer, crate::value::object::NA_INTEGER);
    replace_typed!(Real, f64::from_bits(crate::value::object::NA_REAL_BITS));
    replace_typed!(Raw, 0u8);
    // Integer object with real value: GNU widens the whole vector to real.
    Err(EvalError::TypeError("replacement type mismatch"))
}

/// `numeric(n)`/`integer(n)`/`logical(n)` — zero/FALSE-filled vectors of
/// the requested length, GNU's constructor defaults exactly.
fn vector_constructor(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let length = match heap.object(operands[0]) {
        Some(RObject::Real(v)) if !v.is_empty() => v[0] as usize,
        Some(RObject::Integer(v)) if !v.is_empty() => v[0] as usize,
        _ => return Err(EvalError::TypeError("length must be scalar numeric").into()),
    };
    let body = match op {
        NUMERIC => RObject::Real(vec![0.0; length]),
        INTEGER_CTOR => RObject::Integer(vec![0; length]),
        _ => RObject::Logical(vec![0; length]),
    };
    Ok(heap.alloc(body))
}

/// Read one named attribute off an object's slot attribute chain — the
/// pairlist walk every attribute access shares. Attribute chains are
/// pairlist nodes whose tags are the attribute name symbols.
fn get_attrib(
    heap: &Heap,
    interner: &SymbolInterner,
    obj: ValueId,
    attr_name: &str,
) -> Option<ValueId> {
    let mut cursor = heap.get(obj)?.attrib;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            return None;
        }
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => {
                if let Some(RObject::Symbol { name, .. }) = heap.object(*tag) {
                    if interner.symbol_name(*name) == attr_name {
                        return Some(*car);
                    }
                }
                cursor = *cdr;
            }
            _ => return None,
        }
    }
    None
}

/// Copy an object with one attribute replaced (or prepended when absent).
/// R's copy-on-write discipline: the original object is never mutated —
/// assignment binds the fresh copy, so aliases keep the old attributes.
/// The attribute chain itself is rebuilt node-by-node for the same reason.
fn copy_with_attrib(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    obj: ValueId,
    attr_name: &str,
    attr_value: ValueId,
) -> Result<ValueId, EvalError> {
    let body = heap
        .object(obj)
        .cloned()
        .ok_or(EvalError::TypeError("attribute target missing"))?;
    let attr_sym = interner.intern_symbol(attr_name);
    let sym_obj = heap.alloc(RObject::Symbol {
        name: attr_sym,
        value: ValueId::UNBOUND,
        internal: ValueId::UNBOUND,
    });
    // Walk the original chain, rebuilding nodes; substitute the matching tag.
    let original_chain = heap.get(obj).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    let mut entries: Vec<(ValueId, ValueId)> = Vec::new();
    let mut found = false;
    let mut cursor = original_chain;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            break;
        }
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => {
                let matches = match heap.object(*tag) {
                    Some(RObject::Symbol { name, .. }) => {
                        interner.symbol_name(*name) == attr_name
                    }
                    _ => false,
                };
                if matches {
                    entries.push((sym_obj, attr_value));
                    found = true;
                } else {
                    entries.push((*tag, *car));
                }
                cursor = *cdr;
            }
            _ => break,
        }
    }
    if !found {
        entries.insert(0, (sym_obj, attr_value));
    }
    // Rebuild the chain tail-to-head.
    let mut chain = ValueId::NIL;
    for (tag, car) in entries.into_iter().rev() {
        chain = heap.alloc(RObject::Pairlist {
            car,
            cdr: chain,
            tag,
        });
    }
    let copy = heap.alloc(body);
    if let Some(slot) = heap.get_mut(copy) {
        slot.attrib = chain;
    }
    Ok(copy)
}

/// `names(x)` — the names attribute as a character vector, or NIL when the
/// object has none, exactly GNU.
fn names_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    match get_attrib(heap, interner, operands[0], "names") {
        Some(names) => Ok(names),
        None => Ok(ValueId::NIL),
    }
}

/// The names<- core, shared between the replacement primitive and the
/// assignment desugar: a copy of the object carrying the new names.
fn names_assign_core(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    obj: ValueId,
    value: ValueId,
) -> Result<ValueId, EvalError> {
    copy_with_attrib(heap, interner, obj, "names", value)
}

/// `` `names<-` `` as a primitive: positional object and value.
fn names_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    names_assign_core(heap, interner, operands[0], operands[1]).map_err(RSignal::from)
}

/// `x$name` — exact-match extraction by name. The second operand is a
/// literal symbol, never evaluated — GNU's argument discipline for `$`.
/// The slice implements exact matching only; GNU's partial matching is
/// documented as future work. Atomic vectors extract a length-one vector
/// of the same type; list elements unbox exactly as `[[`.
fn dollar_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let parts = raw_args(heap, args);
    if parts.len() != 2 {
        return Err(EvalError::UnsupportedForm("$ arity").into());
    }
    let object = eval(heap, interner, parts[0], caller_env)?;
    let object = force_if_promise(heap, interner, object)?;
    let wanted = match heap.object(parts[1]) {
        Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
        _ => return Err(EvalError::TypeError("$ name must be a symbol").into()),
    };
    // Environments extract frame-only, exactly GNU: no enclosure chain,
    /// missing bindings are NULL rather than an error.
    if matches!(heap.object(object), Some(RObject::Environment { .. })) {
        let sym = interner.intern_symbol(&wanted);
        return match crate::value::env::lookup_in_frame(heap, object, sym) {
            Some(binding) => Ok(binding.value),
            None => Ok(ValueId::NIL),
        };
    }
    let names = match get_attrib(heap, interner, object, "names") {
        Some(n) => n,
        // GNU: dollar on a nameless object is NULL, not an error.
        None => return Ok(ValueId::NIL),
    };
    let position = match heap.object(names) {
        Some(RObject::String(values)) => values
            .iter()
            .position(|s| interner.string_text(*s) == wanted),
        _ => None,
    };
    // GNU discipline: `$` is invalid for atomic vectors — only recursive
    /// objects (lists) extract by dollar. Atomic named access goes through
    /// `[` with character indices, a separate slice.
    match heap.object(object) {
        Some(RObject::List(items)) => {
            let position = match position {
                Some(p) => p,
                None => return Ok(ValueId::NIL),
            };
            if position >= items.len() {
                return Ok(ValueId::NIL);
            }
            Ok(items[position])
        }
        _ => Err(EvalError::TypeError("$ operator is invalid for atomic vectors").into()),
    }
}

/// `lapply(x, f)` / `sapply(x, f)` — apply a closure to each element.
/// Atomic vectors box elements as length-one vectors (GNU exactly); list
/// elements pass through unboxed. Application reuses the closure machinery
/// through a synthetic argument pairlist — the element is already a value,
/// and vector values self-evaluate when the argument promise forces.
/// sapply simplifies homogeneous length-one results into a vector, GNU's
/// canonical simplification shape.
fn apply_family(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let elements = sequence_elements(heap, operands[0])?;
    let fun = operands[1];
    let (formals, body, closure_env) = match heap.object(fun) {
        Some(RObject::Closure { formals, body, env }) => (*formals, *body, *env),
        _ => return Err(EvalError::TypeError("apply requires a closure").into()),
    };
    let mut results = Vec::new();
    for element in elements {
        // Synthetic argument pairlist: one node carrying the element. The
        // element is a value; self-evaluation makes the promise correct.
        let arg_node = heap.alloc(RObject::Pairlist {
            car: element,
            cdr: ValueId::NIL,
            tag: ValueId::NIL,
        });
        let applied = super::eval::apply_closure_public(
            heap, interner, formals, body, closure_env, arg_node, caller_env,
        )?;
        let applied = force_if_promise(heap, interner, applied)?;
        results.push(applied);
    }
    if op == LAPPLY {
        return Ok(heap.alloc(RObject::List(results)));
    }
    // sapply simplification: homogeneous length-one atomic results unbox
    // into a single vector of the common type; anything else stays a list.
    simplify_sapply(heap, results)
}

/// GNU's simplification rule shape: every result a length-one atomic vector
/// of one type unboxes elementwise; otherwise the list stands.
fn simplify_sapply(heap: &mut Heap, results: Vec<ValueId>) -> EvalResult {
    if results.is_empty() {
        return Ok(heap.alloc(RObject::List(results)));
    }
    enum Kind {
        Logical,
        Integer,
        Real,
    }
    let mut kind: Option<Kind> = None;
    for result in &results {
        let this = match heap.object(*result) {
            Some(RObject::Logical(v)) if v.len() == 1 => Kind::Logical,
            Some(RObject::Integer(v)) if v.len() == 1 => Kind::Integer,
            Some(RObject::Real(v)) if v.len() == 1 => Kind::Real,
            _ => return Ok(heap.alloc(RObject::List(results))),
        };
        match (&kind, &this) {
            (None, _) => kind = Some(this),
            (Some(Kind::Logical), Kind::Logical) => {}
            (Some(Kind::Integer), Kind::Integer) => {}
            (Some(Kind::Real), Kind::Real) => {}
            // Mixed numeric kinds simplify upward to real in GNU's exact
            // shape; logical/integer mixtures stay integer. The slice
            // simplifies any numeric mixture to real, documented.
            _ => kind = Some(Kind::Real),
        }
    }
    match kind {
        Some(Kind::Logical) => {
            let mut out = Vec::new();
            for result in results {
                match heap.object(result) {
                    Some(RObject::Logical(v)) => out.push(v[0]),
                    _ => return Err(EvalError::TypeError("simplify kind drift").into()),
                }
            }
            Ok(heap.alloc(RObject::Logical(out)))
        }
        Some(Kind::Integer) => {
            let mut out = Vec::new();
            for result in results {
                match heap.object(result) {
                    Some(RObject::Integer(v)) => out.push(v[0]),
                    _ => return Err(EvalError::TypeError("simplify kind drift").into()),
                }
            }
            Ok(heap.alloc(RObject::Integer(out)))
        }
        _ => {
            let mut out = Vec::new();
            for result in results {
                match heap.object(result) {
                    Some(RObject::Real(v)) => out.push(v[0]),
                    Some(RObject::Integer(v)) => {
                        if v[0] == crate::value::object::NA_INTEGER {
                            out.push(f64::from_bits(crate::value::object::NA_REAL_BITS));
                        } else {
                            out.push(v[0] as f64);
                        }
                    }
                    Some(RObject::Logical(v)) => out.push(v[0] as f64),
                    _ => return Err(EvalError::TypeError("simplify kind drift").into()),
                }
            }
            Ok(heap.alloc(RObject::Real(out)))
        }
    }
}

/// `unlist(x)` — recursive flattening to the common atomic type, GNU's
/// type-hierarchy shape: any real promotes to real, integer/logical
/// mixtures promote to integer. The slice covers numeric flattening;
/// string mixtures are future work.
fn unlist_primitive(heap: &mut Heap, value: ValueId) -> EvalResult {
    fn flatten(heap: &Heap, value: ValueId, ints: &mut Vec<i32>, reals: &mut Vec<f64>) {
        match heap.object(value) {
            Some(RObject::Logical(values)) => {
                for v in values {
                    ints.push(*v as i32);
                    reals.push(*v as f64);
                }
            }
            Some(RObject::Integer(values)) => {
                for v in values {
                    ints.push(*v);
                    if *v == crate::value::object::NA_INTEGER {
                        reals.push(f64::from_bits(crate::value::object::NA_REAL_BITS));
                    } else {
                        reals.push(*v as f64);
                    }
                }
            }
            Some(RObject::Real(values)) => {
                for v in values {
                    ints.push(0);
                    reals.push(*v);
                }
            }
            Some(RObject::List(items)) => {
                for item in items {
                    flatten(heap, *item, ints, reals);
                }
            }
            _ => {}
        }
    }
    fn any_real(heap: &Heap, value: ValueId) -> bool {
        match heap.object(value) {
            Some(RObject::Real(_)) => true,
            Some(RObject::List(items)) => items.iter().any(|i| any_real(heap, *i)),
            _ => false,
        }
    }
    match heap.object(value) {
        Some(RObject::List(_)) => {}
        // Atomic inputs pass through unchanged, GNU exactly.
        _ => return Ok(value),
    }
    let mut ints = Vec::new();
    let mut reals = Vec::new();
    flatten(heap, value, &mut ints, &mut reals);
    if any_real(heap, value) {
        Ok(heap.alloc(RObject::Real(reals)))
    } else {
        Ok(heap.alloc(RObject::Integer(ints)))
    }
}

/// First element of a numeric vector as a double — the scalar extraction
/// the constructors share.
fn first_real(heap: &Heap, value: ValueId) -> Result<f64, EvalError> {
    match heap.object(value) {
        Some(RObject::Real(v)) if !v.is_empty() => Ok(v[0]),
        Some(RObject::Integer(v)) if !v.is_empty() => {
            if v[0] == crate::value::object::NA_INTEGER {
                Err(EvalError::TypeError("NA scalar"))
            } else {
                Ok(v[0] as f64)
            }
        }
        Some(RObject::Logical(v)) if !v.is_empty() => Ok(v[0] as f64),
        _ => Err(EvalError::TypeError("numeric scalar required")),
    }
}

/// `seq(from, to, by)` — GNU's general sequence. Integer-valued from/to/by
/// yield integer sequences when from and by are both whole, matching the
/// colon operator's integer fidelity; otherwise real. Positional and named
/// argument shapes both resolve — named extraction uses the same eval_args
/// machinery since seq's operands are positional in practice.
fn seq_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Collect raw operands with tags so seq(from=1, to=5, by=2) resolves.
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let lookup = |name: &str, position: usize| -> Option<ValueId> {
        named
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .or_else(|| positional.get(position).copied())
    };
    let from = lookup("from", 0).ok_or(EvalError::TypeError("seq requires from"))?;
    let to = lookup("to", 1).ok_or(EvalError::TypeError("seq requires to"))?;
    let from_v = first_real(heap, from).map_err(RSignal::from)?;
    let to_v = first_real(heap, to).map_err(RSignal::from)?;
    let by_v = match lookup("by", 2) {
        Some(by) => first_real(heap, by).map_err(RSignal::from)?,
        None => 1.0,
    };
    if by_v == 0.0 {
        return Err(EvalError::TypeError("seq by zero").into());
    }
    let mut out: Vec<f64> = Vec::new();
    let mut current = from_v;
    for _ in 0..100_000_000 {
        if (by_v > 0.0 && current > to_v) || (by_v < 0.0 && current < to_v) {
            break;
        }
        out.push(current);
        current += by_v;
    }
    // GNU fidelity: integer-valued from and by produce integer sequences.
    let whole = from_v.fract() == 0.0 && by_v.fract() == 0.0 && out.iter().all(|v| v.fract() == 0.0);
    if whole && out.iter().all(|v| v.abs() <= i32::MAX as f64) {
        let ints: Vec<i32> = out.iter().map(|v| *v as i32).collect();
        return Ok(heap.alloc(RObject::Integer(ints)));
    }
    Ok(heap.alloc(RObject::Real(out)))
}

/// `rep(x, times)` — GNU's simplest repetition shape: the whole vector
/// repeats times over. Per-element repetition (times as a vector) is
/// future work.
fn rep_primitive(heap: &mut Heap, source: ValueId, times: ValueId) -> EvalResult {
    let n = first_real(heap, times).map_err(RSignal::from)? as usize;
    macro_rules! repeat {
        ($variant:ident) => {
            match heap.object(source) {
                Some(RObject::$variant(values)) => {
                    let mut out = Vec::with_capacity(values.len() * n);
                    for _ in 0..n {
                        out.extend(values.iter().copied());
                    }
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    repeat!(Logical);
    repeat!(Integer);
    repeat!(Real);
    repeat!(String);
    repeat!(Raw);
    match heap.object(source) {
        Some(RObject::List(values)) => {
            let mut out = Vec::with_capacity(values.len() * n);
            for _ in 0..n {
                out.extend(values.iter().copied());
            }
            Ok(heap.alloc(RObject::List(out)))
        }
        _ => Err(EvalError::TypeError("rep requires a vector").into()),
    }
}

/// Numeric contents of a vector as doubles — the extraction the statistics
/// share. Logicals and integers coerce up exactly as GNU's as-double.
fn numerics_of(heap: &Heap, value: ValueId) -> Result<Vec<f64>, EvalError> {
    let na = f64::from_bits(crate::value::object::NA_REAL_BITS);
    match heap.object(value) {
        Some(RObject::Real(v)) => Ok(v.clone()),
        Some(RObject::Integer(v)) => Ok(v
            .iter()
            .map(|x| {
                if *x == crate::value::object::NA_INTEGER {
                    na
                } else {
                    *x as f64
                }
            })
            .collect()),
        Some(RObject::Logical(v)) => Ok(v
            .iter()
            .map(|x| {
                if *x == crate::value::object::NA_INTEGER {
                    na
                } else {
                    *x as f64
                }
            })
            .collect()),
        _ => Err(EvalError::TypeError("statistic requires numeric")),
    }
}

/// `mean`/`var`/`sd`/`median` with GNU's na.rm handling: absent or FALSE,
/// any NA poisons the result to NA; TRUE drops NA positions first. The
/// na.rm argument resolves tagged or positional.
fn stat_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let input = positional
        .first()
        .copied()
        .ok_or(EvalError::TypeError("statistic requires input"))?;
    let na_rm = match named
        .iter()
        .find(|(n, _)| n == "na.rm")
        .map(|(_, v)| *v)
        .or_else(|| positional.get(1).copied())
    {
        Some(flag) => matches!(
            heap.object(flag),
            Some(RObject::Logical(v)) if !v.is_empty() && v[0] != 0 && v[0] != crate::value::object::NA_INTEGER
        ),
        None => false,
    };
    let mut xs = numerics_of(heap, input).map_err(RSignal::from)?;
    let na_real = f64::from_bits(crate::value::object::NA_REAL_BITS);
    let any_na = xs.iter().any(|v| v.is_nan());
    if any_na && !na_rm {
        return Ok(heap.alloc(RObject::Real(vec![na_real])));
    }
    xs.retain(|v| !v.is_nan());
    if xs.is_empty() {
        return Ok(heap.alloc(RObject::Real(vec![na_real])));
    }
    let result = match op {
        MEAN => xs.iter().sum::<f64>() / xs.len() as f64,
        VAR => {
            // GNU's sample variance with the mean-centered two-pass form.
            let mean = xs.iter().sum::<f64>() / xs.len() as f64;
            xs.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (xs.len() as f64 - 1.0)
        }
        SD => {
            let mean = xs.iter().sum::<f64>() / xs.len() as f64;
            (xs.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (xs.len() as f64 - 1.0))
                .sqrt()
        }
        _ => {
            // GNU's median: mean of the two central order statistics for
            // even lengths.
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = xs.len();
            if n % 2 == 1 {
                xs[n / 2]
            } else {
                (xs[n / 2 - 1] + xs[n / 2]) / 2.0
            }
        }
    };
    Ok(heap.alloc(RObject::Real(vec![result])))
}

/// `is.na(x)` — elementwise NA detection across the atomic types.
fn is_na_primitive(heap: &mut Heap, value: ValueId) -> EvalResult {
    let na_int = crate::value::object::NA_INTEGER;
    let out: Vec<i32> = match heap.object(value) {
        Some(RObject::Logical(v)) => v.iter().map(|x| (*x == na_int) as i32).collect(),
        Some(RObject::Integer(v)) => v.iter().map(|x| (*x == na_int) as i32).collect(),
        Some(RObject::Real(v)) => v.iter().map(|x| x.is_nan() as i32).collect(),
        Some(RObject::String(v)) => v.iter().map(|_| 0).collect(),
        _ => return Err(EvalError::TypeError("is.na requires a vector").into()),
    };
    Ok(heap.alloc(RObject::Logical(out)))
}

/// `sort`/`rev`/`unique` — ordering and deduplication over atomic vectors.
/// GNU's sort drops NAs by default (na.last=NA); the slice follows that.
fn ordering_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let value = operands[0];
    // GNU's tagged decreasing argument — positional second or tagged.
    let decreasing = match raw_args(heap, args).get(1).copied() {
        Some(expr) => {
            let evaluated = eval(heap, interner, expr, caller_env)?;
            let evaluated = force_if_promise(heap, interner, evaluated)?;
            matches!(
                heap.object(evaluated),
                Some(RObject::Logical(v)) if !v.is_empty() && v[0] != 0 && v[0] != crate::value::object::NA_INTEGER
            )
        }
        None => false,
    };
    macro_rules! per_type {
        ($variant:ident, $ord:expr) => {
            match heap.object(value) {
                Some(RObject::$variant(values)) => {
                    let mut out = values.clone();
                    match op {
                        SORT => {
                            out.retain(|v| !$ord(*v));
                            out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                            if decreasing {
                                out.reverse();
                            }
                        }
                        REV => out.reverse(),
                        _ => {
                            let mut seen: Vec<_> = Vec::new();
                            out.retain(|v| {
                                if seen.iter().any(|s| s == v) {
                                    false
                                } else {
                                    seen.push(*v);
                                    true
                                }
                            });
                        }
                    }
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    let is_na_int = |v: i32| v == crate::value::object::NA_INTEGER;
    let is_na_real = |v: f64| v.is_nan();
    per_type!(Integer, is_na_int);
    per_type!(Real, is_na_real);
    per_type!(Logical, is_na_int);
    Err(EvalError::TypeError("ordering requires a numeric vector").into())
}

/// `which(x)` — 1-based positions of TRUE entries, GNU exactly; NA
/// positions drop out.
fn which_primitive(heap: &mut Heap, value: ValueId) -> EvalResult {
    match heap.object(value) {
        Some(RObject::Logical(values)) => {
            let out: Vec<i32> = values
                .iter()
                .enumerate()
                .filter(|(_, v)| **v != 0 && **v != crate::value::object::NA_INTEGER)
                .map(|(i, _)| (i + 1) as i32)
                .collect();
            Ok(heap.alloc(RObject::Integer(out)))
        }
        _ => Err(EvalError::TypeError("which requires logical").into()),
    }
}

/// `any(x)`/`all(x)` — GNU's quantifier NA semantics: any is TRUE if any
/// entry is TRUE, NA if none TRUE but some NA, FALSE otherwise; all
/// mirrors.
fn quantifier_primitive(heap: &mut Heap, op: u32, value: ValueId) -> EvalResult {
    let values = logicals_of_value(heap, value).map_err(RSignal::from)?;
    let na = crate::value::object::NA_INTEGER;
    let trues = values.iter().filter(|v| **v != 0 && **v != na).count();
    let falses = values.iter().filter(|v| **v == 0).count();
    let nas = values.len() - trues - falses;
    let result = if op == ANY {
        if trues > 0 {
            1
        } else if nas > 0 {
            na
        } else {
            0
        }
    } else if falses > 0 {
        0
    } else if nas > 0 {
        na
    } else {
        1
    };
    Ok(heap.alloc(RObject::Logical(vec![result])))
}

/// `ifelse(test, yes, no)` — GNU's vectorized conditional: elementwise
/// selection with yes/no recycling. The slice covers numeric both-sides;
/// NA test positions yield NA.
fn ifelse_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 3)?;
    let tests = logicals_of_value(heap, operands[0]).map_err(RSignal::from)?;
    let yes = numerics_of(heap, operands[1]).map_err(RSignal::from)?;
    let no = numerics_of(heap, operands[2]).map_err(RSignal::from)?;
    let na_int = crate::value::object::NA_INTEGER;
    let na_real = f64::from_bits(crate::value::object::NA_REAL_BITS);
    let mut out = Vec::with_capacity(tests.len());
    for (i, test) in tests.iter().enumerate() {
        if *test == na_int {
            out.push(na_real);
        } else if *test != 0 {
            out.push(yes[i % yes.len().max(1)]);
        } else {
            out.push(no[i % no.len().max(1)]);
        }
    }
    Ok(heap.alloc(RObject::Real(out)))
}

/// `%in%` — elementwise membership: logical vector marking each left
/// element found in the right table. Numeric slice; NA never matches,
/// exactly as GNU.
fn membership_primitive(heap: &mut Heap, left: ValueId, right: ValueId) -> EvalResult {
    let lhs = numerics_of(heap, left).map_err(RSignal::from)?;
    let table = numerics_of(heap, right).map_err(RSignal::from)?;
    let out: Vec<i32> = lhs
        .iter()
        .map(|v| {
            if v.is_nan() {
                0
            } else {
                table.iter().any(|t| !t.is_nan() && t == v) as i32
            }
        })
        .collect();
    Ok(heap.alloc(RObject::Logical(out)))
}

/// `match(x, table)` — 1-based first-match positions, NA for no match,
/// GNU exactly. Numeric slice.
fn match_primitive(heap: &mut Heap, left: ValueId, right: ValueId) -> EvalResult {
    let lhs = numerics_of(heap, left).map_err(RSignal::from)?;
    let table = numerics_of(heap, right).map_err(RSignal::from)?;
    let na = crate::value::object::NA_INTEGER;
    let out: Vec<i32> = lhs
        .iter()
        .map(|v| {
            if v.is_nan() {
                return na;
            }
            match table.iter().position(|t| !t.is_nan() && t == v) {
                Some(position) => (position + 1) as i32,
                None => na,
            }
        })
        .collect();
    Ok(heap.alloc(RObject::Integer(out)))
}

/// `exists(name, envir)` — GNU's chain lookup by default: the binding may
/// live in the environment or any enclosure. The name arrives as a
/// one-element character vector exactly as GNU's interface.
fn exists_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let name = match heap.object(operands[0]) {
        Some(RObject::String(values)) if !values.is_empty() => {
            interner.string_text(values[0]).to_string()
        }
        _ => return Err(EvalError::TypeError("exists requires a character name").into()),
    };
    // Optional second argument: the environment to inspect. eval_args with
    /// count 1 consumed only the name; re-walk raw args for the second.
    let env = match raw_args(heap, args).get(1).copied() {
        Some(env_expr) => {
            let evaluated = eval(heap, interner, env_expr, caller_env)?;
            force_if_promise(heap, interner, evaluated)?
        }
        None => caller_env,
    };
    let sym = interner.intern_symbol(&name);
    let found = crate::value::env::lookup(heap, env, sym).is_some();
    Ok(heap.alloc(RObject::Logical(vec![found as i32])))
}

/// `ls(envir)` — sorted binding names of the frame, GNU's ordering. List
/// frames enumerate; hash frames are outside the object model's slice.
fn ls_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    env: ValueId,
) -> EvalResult {
    let mut names: Vec<String> = Vec::new();
    if let Some(RObject::Environment { frame, .. }) = heap.object(env) {
        if let crate::value::object::EnvFrame::List(bindings) = frame {
            for (sym, _) in bindings {
                names.push(interner.symbol_name(*sym).to_string());
            }
        }
    }
    names.sort();
    let interned: Vec<super::intern::InternedStr> = names
        .iter()
        .map(|n| interner.intern_string(n))
        .collect();
    Ok(heap.alloc(RObject::String(interned)))
}

/// `$<-(x, name, value)` — GNU's split discipline: environments are
/// reference semantics, the binding defines in place and the same object
/// returns; lists are copy-on-write value semantics exactly like vector
/// replacement — the element replaces or appends with the names attribute
/// extended.
fn dollar_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // The name operand arrives UNEVALUATED as a symbol exactly as `$` —
    /// evaluate operands selectively: object and value yes, name no.
    let raw = raw_args(heap, args);
    if raw.len() < 3 {
        return Err(EvalError::TypeError("$<- requires object, name, value").into());
    }
    let object = eval(heap, interner, raw[0], caller_env)?;
    let object = force_if_promise(heap, interner, object)?;
    let name = match heap.object(raw[1]) {
        Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
        _ => {
            // Evaluated form: a one-element character vector.
            let evaluated = eval(heap, interner, raw[1], caller_env)?;
            let evaluated = force_if_promise(heap, interner, evaluated)?;
            match heap.object(evaluated) {
                Some(RObject::String(values)) if !values.is_empty() => {
                    interner.string_text(values[0]).to_string()
                }
                _ => return Err(EvalError::TypeError("$<- name must be symbol or string").into()),
            }
        }
    };
    let value = eval(heap, interner, raw[2], caller_env)?;
    let value = force_if_promise(heap, interner, value)?;
    match heap.object(object) {
        Some(RObject::Environment { .. }) => {
            let sym = interner.intern_symbol(&name);
            crate::value::env::define(heap, object, sym, value)
                .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("env define")))?;
            Ok(object)
        }
        Some(RObject::List(items)) => {
            let items = items.clone();
            let existing_names: Vec<String> =
                match get_attrib(heap, interner, object, "names").and_then(|id| {
                    match heap.object(id) {
                        Some(RObject::String(entries)) => Some(
                            entries
                                .iter()
                                .map(|e| interner.string_text(*e).to_string())
                                .collect(),
                        ),
                        _ => None,
                    }
                }) {
                    Some(n) => n,
                    None => vec![String::new(); items.len()],
                };
            let mut new_items = items;
            let mut new_names = existing_names;
            if new_names.len() < new_items.len() {
                new_names.resize(new_items.len(), String::new());
            }
            match new_names.iter().position(|n| n == &name) {
                Some(position) => {
                    new_items[position] = value;
                }
                None => {
                    new_items.push(value);
                    new_names.push(name);
                }
            }
            let mut result = heap.alloc(RObject::List(new_items));
            let interned: Vec<super::intern::InternedStr> = new_names
                .iter()
                .map(|n| interner.intern_string(n))
                .collect();
            let names_vec = heap.alloc(RObject::String(interned));
            result = copy_with_attrib(heap, interner, result, "names", names_vec)
                .map_err(RSignal::from)?;
            Ok(result)
        }
        _ => Err(EvalError::TypeError("$<- requires environment or list").into()),
    }
}

/// `matrix(data, nrow, ncol)` — GNU's column-major construction: data
/// recycles to nrow*ncol, and because columns fill contiguously the linear
/// storage IS the data vector — the constructor is recycling plus the dim
/// attribute, nothing more.
fn matrix_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let lookup = |name: &str, position: usize| -> Option<ValueId> {
        named
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .or_else(|| positional.get(position).copied())
    };
    let data = lookup("data", 0).ok_or(EvalError::TypeError("matrix requires data"))?;
    let data_len = vector_length(heap, data)
        .ok_or(EvalError::TypeError("matrix data must be a vector"))
        .map_err(RSignal::from)?;
    let nrow = match lookup("nrow", 1) {
        Some(v) => first_real(heap, v).map_err(RSignal::from)? as usize,
        None => data_len,
    };
    let ncol = match lookup("ncol", 2) {
        Some(v) => first_real(heap, v).map_err(RSignal::from)? as usize,
        None => (data_len + nrow - 1) / nrow.max(1),
    };
    let total = nrow * ncol;
    // Recycle the data vector to the matrix extent — same-type copying.
    let result = match heap.object(data) {
        Some(RObject::Real(values)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(values[i % values.len().max(1)]);
            }
            heap.alloc(RObject::Real(out))
        }
        Some(RObject::Integer(values)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(values[i % values.len().max(1)]);
            }
            heap.alloc(RObject::Integer(out))
        }
        Some(RObject::Logical(values)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(values[i % values.len().max(1)]);
            }
            heap.alloc(RObject::Logical(out))
        }
        Some(RObject::String(values)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(values[i % values.len().max(1)]);
            }
            heap.alloc(RObject::String(out))
        }
        _ => return Err(EvalError::TypeError("matrix data must be atomic").into()),
    };
    let dims = heap.alloc(RObject::Integer(vec![nrow as i32, ncol as i32]));
    copy_with_attrib(heap, interner, result, "dim", dims).map_err(RSignal::from)
}

/// `rep` argument resolution: GNU's forms are rep(x, times), rep(x, each),
/// and per-element times vectors — all composable. Tagged arguments resolve
/// before positional exactly as GNU's formals ordering.
fn rep_dispatch(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let lookup = |name: &str, position: usize| -> Option<ValueId> {
        named
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
            .or_else(|| positional.get(position).copied())
    };
    let source = match lookup("x", 0) {
        Some(v) => v,
        None => return Err(EvalError::TypeError("rep requires input").into()),
    };
    let each = match lookup("each", 2) {
        Some(v) => first_real(heap, v).map_err(RSignal::from)? as usize,
        None => 1,
    };
    let times_value = lookup("times", 1);
    // Per-element times vector: replicate element i exactly times[i] times.
    if let Some(tv) = times_value {
        let per_element: Option<Vec<usize>> = match heap.object(tv) {
            Some(RObject::Integer(values)) if values.len() > 1 => {
                Some(values.iter().map(|v| (*v).max(0) as usize).collect())
            }
            Some(RObject::Real(values)) if values.len() > 1 => {
                Some(values.iter().map(|v| (*v as i32).max(0) as usize).collect())
            }
            _ => None,
        };
        if let Some(schedule) = per_element {
            return rep_per_element(heap, source, &schedule, each);
        }
    }
    let times = match times_value {
        Some(tv) => first_real(heap, tv).map_err(RSignal::from)? as usize,
        None => 1,
    };
    if each > 1 {
        return rep_each(heap, source, times, each);
    }
    let times_scalar = heap.alloc(RObject::Real(vec![times as f64]));
    rep_primitive(heap, source, times_scalar)
}

/// Elementwise repetition: element i appears times[i] times, each repeated
/// contiguously — GNU's rep(x, times=vector) shape.
fn rep_per_element(heap: &mut Heap, source: ValueId, schedule: &[usize], each: usize) -> EvalResult {
    macro_rules! per_type {
        ($variant:ident) => {
            match heap.object(source) {
                Some(RObject::$variant(values)) => {
                    let mut out = Vec::new();
                    for (i, value) in values.iter().enumerate() {
                        let count = schedule.get(i).copied().unwrap_or(0) * each;
                        for _ in 0..count {
                            out.push(*value);
                        }
                    }
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    per_type!(Integer);
    per_type!(Real);
    per_type!(Logical);
    per_type!(String);
    Err(EvalError::TypeError("rep requires an atomic vector").into())
}

/// `rep(x, each=k)` — every element repeats k times contiguously before
/// the next, then the whole pattern repeats times over.
fn rep_each(heap: &mut Heap, source: ValueId, times: usize, each: usize) -> EvalResult {
    macro_rules! per_type {
        ($variant:ident) => {
            match heap.object(source) {
                Some(RObject::$variant(values)) => {
                    let mut out = Vec::new();
                    for _ in 0..times {
                        for value in values.iter() {
                            for _ in 0..each {
                                out.push(*value);
                            }
                        }
                    }
                    return Ok(heap.alloc(RObject::$variant(out)));
                }
                _ => {}
            }
        };
    }
    per_type!(Integer);
    per_type!(Real);
    per_type!(Logical);
    per_type!(String);
    Err(EvalError::TypeError("rep requires an atomic vector").into())
}

/// `tryCatch(expr, error=handler)` — GNU's condition handling shape: the
/// expression evaluates inside the protection; an error condition invokes
/// the handler with a condition object and the handler's value becomes the
/// result. Non-error control flow (break/next/return) propagates exactly —
/// it is not a condition.
fn trycatch_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Collect UNEVALUATED operands with tags: the expression is positional
    /// first, handlers arrive tagged by condition class.
    let mut positional: Vec<ValueId> = Vec::new();
    let mut named: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => {
                named.push((interner.symbol_name(*name).to_string(), car));
            }
            _ => positional.push(car),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let expr = match positional.first() {
        Some(e) => *e,
        None => return Err(EvalError::TypeError("tryCatch requires an expression").into()),
    };
    match eval(heap, interner, expr, caller_env) {
        Ok(value) => Ok(value),
        Err(RSignal::Error(error)) => {
            let handler_expr = match named.iter().find(|(n, _)| n == "error").map(|(_, e)| *e) {
                Some(e) => e,
                // No matching handler: the condition propagates exactly as
                /// GNU's unwinding continues outward.
                None => return Err(RSignal::Error(error)),
            };
            let message = match &error {
                EvalError::Condition(text) => text.clone(),
                EvalError::UnboundVariable(name) => format!("object '{name}' not found"),
                other => format!("{other:?}"),
            };
            // The condition object: a named list carrying the message —
            /// conditionMessage reads exactly this shape.
            let message_str = heap.alloc(RObject::String(vec![interner.intern_string(&message)]));
            let condition = heap.alloc(RObject::List(vec![message_str]));
            let names_vec = heap.alloc(RObject::String(vec![interner.intern_string("message")]));
            let condition = copy_with_attrib(heap, interner, condition, "names", names_vec)
                .map_err(RSignal::from)?;
            let handler = eval(heap, interner, handler_expr, caller_env)?;
            let handler = force_if_promise(heap, interner, handler)?;
            let (formals, body, closure_env) = match heap.object(handler) {
                Some(RObject::Closure { formals, body, env }) => (*formals, *body, *env),
                _ => return Err(EvalError::TypeError("tryCatch handler must be a closure").into()),
            };
            let arg_node = heap.alloc(RObject::Pairlist {
                car: condition,
                cdr: ValueId::NIL,
                tag: ValueId::NIL,
            });
            super::eval::apply_closure_public(
                heap, interner, formals, body, closure_env, arg_node, caller_env,
            )
        }
        // Control flow is not a condition — break/next/return propagate.
        Err(signal) => Err(signal),
    }
}
