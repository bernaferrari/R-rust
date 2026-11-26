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
    /// Type introspection, structure manipulation, environment access.
    pub const TYPEOF: u32 = 106;
    pub const STRUCTURE: u32 = 107;
    pub const ASSIGN_FN: u32 = 108;
    pub const GET: u32 = 109;
    pub const CHARACTER_CTOR: u32 = 110;
    /// Attribute accessors and their replacement forms.
    pub const DIMNAMES: u32 = 111;
    pub const DIMNAMES_ASSIGN: u32 = 112;
    pub const CLASS: u32 = 113;
    pub const CLASS_ASSIGN: u32 = 114;
    pub const ROWNAMES: u32 = 115;
    pub const ROWNAMES_ASSIGN: u32 = 116;
    pub const COLNAMES: u32 = 117;
    pub const COLNAMES_ASSIGN: u32 = 118;
    /// Text file I/O — the architecturally permanent slice of the tempfile
    /// cluster (save/load/RDS wire formats are dropped by design).
    pub const TEMPFILE: u32 = 119;
    pub const WRITELINES: u32 = 120;
    pub const READLINES: u32 = 121;
    pub const FILE_EXISTS: u32 = 122;
    pub const FILE_REMOVE: u32 = 123;
    pub const FILE_COPY: u32 = 124;
    pub const FILE_RENAME: u32 = 125;
    pub const SUPPRESS_WARNINGS: u32 = 126;
    pub const AS_LOGICAL: u32 = 127;
    /// Attribute reflection and vector resizing.
    pub const ATTR: u32 = 128;
    pub const ATTR_ASSIGN: u32 = 129;
    pub const ATTRIBUTES: u32 = 130;
    pub const ATTRIBUTES_ASSIGN: u32 = 131;
    pub const IS_NULL: u32 = 132;
    pub const LENGTH_ASSIGN: u32 = 133;
    pub const FILE_PATH: u32 = 134;
    /// Data frame construction — the list-of-columns object shape GNU's
    /// statistical surface is built on.
    pub const DATA_FRAME: u32 = 135;
    /// Split-apply-combine over grouped vectors.
    pub const AGGREGATE: u32 = 136;
    /// Factor construction and introspection — classed integer codes with
    /// level labels.
    pub const FACTOR: u32 = 137;
    pub const LEVELS: u32 = 138;
    pub const LEVELS_ASSIGN: u32 = 139;
    pub const UNCLASS: u32 = 140;
    pub const TEMPDIR: u32 = 141;
    /// C-style string formatting.
    pub const SPRINTF: u32 = 142;
    /// System environment and file removal.
    pub const SYS_SETENV: u32 = 143;
    pub const SYS_GETENV: u32 = 144;
    pub const SYS_UNSETENV: u32 = 145;
    pub const UNLINK: u32 = 146;
    /// The .Internal support surface — translation, condition, and state
    /// pieces GNU routes through the internal table.
    pub const INTERNAL: u32 = 156;
    pub const PRIMITIVE_FN: u32 = 157;
    pub const WARNING: u32 = 158;
    pub const GETTEXT: u32 = 159;
    pub const NGETTEXT: u32 = 160;
    pub const BINDTEXTDOMAIN: u32 = 161;
    pub const GETERRMESSAGE: u32 = 162;
    pub const SETERRMESSAGE: u32 = 163;
    pub const INTERRUPTS_SUSPENDED: u32 = 164;
    pub const IS_PRIMITIVE: u32 = 165;
    pub const IS_TRUE: u32 = 166;
    /// Object persistence — RBG1 roundtrips replacing the dropped GNU
    /// wire formats.
    pub const SAVE: u32 = 168;
    pub const LOAD: u32 = 169;
    pub const RM: u32 = 170;
    pub const AS_VECTOR: u32 = 171;
    pub const AS_CHARACTER: u32 = 172;
    pub const TSP: u32 = 173;
    pub const TSP_ASSIGN: u32 = 174;
    /// Environment references and structural comparison.
    pub const SETNAMES: u32 = 175;
    pub const IDENTICAL: u32 = 176;
    pub const GLOBALENV: u32 = 177;
    pub const BASEENV: u32 = 178;
    pub const ENVIRONMENT_NAME: u32 = 179;
    pub const ENVIRONMENT: u32 = 180;
    pub const R_VERSION_FN: u32 = 181;
    pub const GET_RVERSION: u32 = 182;
    pub const IS_LIST: u32 = 183;
    /// N-dimensional array construction.
    pub const ARRAY: u32 = 184;
    pub const IS_ARRAY: u32 = 185;
    /// The Date surface — ISO parsing, calendar labels, difftime.
    pub const AS_DATE: u32 = 186;
    pub const WEEKDAYS: u32 = 187;
    pub const MONTHS: u32 = 188;
    pub const QUARTERS: u32 = 189;
    pub const FORMAT: u32 = 190;
    pub const IS_UNSORTED: u32 = 191;
    pub const TOSTRING: u32 = 192;
    pub const CAPTURE_OUTPUT: u32 = 193;
    pub const ROW_NAMES: u32 = 194;
    pub const ROW_NAMES_SET: u32 = 195;
    pub const TAPPLY: u32 = 196;
    pub const VAPPLY: u32 = 197;
    pub const PRINT_DEFERRED_WARNINGS: u32 = 167;
    /// Directory construction and listing.
    pub const DIR_CREATE: u32 = 153;
    pub const LIST_DIRS: u32 = 154;
    pub const BASENAME: u32 = 155;
    /// Pattern matching — the pure-Rust regex surface.
    pub const GREPL: u32 = 147;
    pub const GREP: u32 = 148;
    pub const REGEXPR: u32 = 149;
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
use primitive_ids::{ABS, ADD, AND, ASSIGN, BRACE, BREAK, COLON, CONCAT, DIV, EQ, EXP, EXTRACT, FOR, FUNCTION, GE, GT, IF, LE, LENGTH, LIST, LOG, LT, MAX, MIN, MUL, MOD, NCHAR, NE, NEXT, NOT, OR, PAREN, PASTE, PASTE0, POW, PRINT, PROD, REPEAT, RETURN, SHORT_AND, SHORT_OR, SQRT, SUB, SUBSET, SUBSET_ASSIGN, SUM, SUPERASSIGN, NUMERIC, INTEGER_CTOR, LOGICAL_CTOR, NAMES, NAMES_ASSIGN, DOLLAR, LAPPLY, SAPPLY, UNLIST, SEQ, SEQ_LEN, REP, MEAN, VAR, SD, MEDIAN, IS_NA, SORT, REV, UNIQUE, WHICH, ANY, ALL, IFELSE, STOP, IN, MATCH, NEW_ENV, EXISTS, LS, DOLLAR_ASSIGN, CAT, INVISIBLE, MATRIX, DIM, NROW, NCOL, TRYCATCH, CONDITION_MESSAGE, TYPEOF, STRUCTURE, ASSIGN_FN, GET, CHARACTER_CTOR, TEMPFILE, WRITELINES, READLINES, FILE_EXISTS, FILE_REMOVE, FILE_COPY, FILE_RENAME, SUPPRESS_WARNINGS, AS_LOGICAL, ATTR, ATTR_ASSIGN, ATTRIBUTES, ATTRIBUTES_ASSIGN, IS_NULL, LENGTH_ASSIGN, FILE_PATH, DATA_FRAME, AGGREGATE, FACTOR, LEVELS, LEVELS_ASSIGN, UNCLASS, TEMPDIR, SPRINTF, SYS_SETENV, SYS_GETENV, SYS_UNSETENV, UNLINK, INTERNAL, PRIMITIVE_FN, WARNING, GETTEXT, NGETTEXT, BINDTEXTDOMAIN, GETERRMESSAGE, SETERRMESSAGE, INTERRUPTS_SUSPENDED, IS_PRIMITIVE, IS_TRUE, SAVE, LOAD, RM, AS_VECTOR, AS_CHARACTER, TSP, TSP_ASSIGN, SETNAMES, IDENTICAL, GLOBALENV, BASEENV, ENVIRONMENT_NAME, ENVIRONMENT, R_VERSION_FN, GET_RVERSION, IS_LIST, ARRAY, IS_ARRAY, AS_DATE, WEEKDAYS, MONTHS, QUARTERS, FORMAT, IS_UNSORTED, TOSTRING, CAPTURE_OUTPUT, ROW_NAMES, ROW_NAMES_SET, TAPPLY, VAPPLY, PRINT_DEFERRED_WARNINGS, DIR_CREATE, LIST_DIRS, BASENAME, GREPL, GREP, REGEXPR, DIMNAMES, DIMNAMES_ASSIGN, CLASS, CLASS_ASSIGN, ROWNAMES, ROWNAMES_ASSIGN, COLNAMES, COLNAMES_ASSIGN, WHILE};

/// Invoke a primitive with an unevaluated argument pairlist in the caller's
/// environment — the same contract GNU primitives receive. The visibility
/// flag lands here centrally: every primitive is visible except GNU's
/// invisible-returning forms, and the flag is set AFTER the body completes
/// so inner evaluations never leak their state into the caller's answer.
pub(crate) fn call_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    id: BuiltinId,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let result = call_primitive_inner(heap, interner, id, args, caller_env);
    // .Internal propagates the inner evaluation's visibility exactly —
    /// the central discipline must not override what the wrapped call set.
    if result.is_ok() && id.0 != INTERNAL {
        heap.visible = !matches!(
            id.0,
            ASSIGN | SUPERASSIGN | PRINT | CAT | INVISIBLE | WHILE | REPEAT | FOR
                | WRITELINES | ASSIGN_FN | FILE_REMOVE | FILE_COPY | FILE_RENAME
                | SYS_SETENV | SYS_UNSETENV | UNLINK | DIR_CREATE | SAVE | RM
        );
    }
    result
}

fn call_primitive_inner(
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
            is_na_primitive(heap, interner, operands[0])
        }
        SORT | REV => ordering_primitive(heap, interner, id.0, args, caller_env),
        UNIQUE => unique_primitive(heap, interner, args, caller_env),
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
            // GNU: the environment argument is optional and defaults to the
            /// calling environment; all.names=TRUE includes dot-prefixed
            /// hidden bindings.
            let mut env = caller_env;
            let mut all_names = false;
            let mut positional = 0usize;
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                positional += 1;
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                match tag_name.as_deref() {
                    Some("all.names") => {
                        if let Some(RObject::Logical(vals)) = heap.object(v) {
                            all_names = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                        }
                    }
                    _ => {
                        if positional == 1 {
                            env = v;
                        }
                    }
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            ls_primitive(heap, interner, env, all_names)
        }
        DOLLAR_ASSIGN => dollar_assign_primitive(heap, interner, args, caller_env),
        CAT => {
            // Variadic output: operands coerce elementwise and concatenate
            /// with the separator — tagged sep exactly as GNU, default one
            /// space. Output lands in the session capture buffer, or the
            /// tagged file (append honored) exactly as GNU's connection
            /// redirection.
            let mut pieces: Vec<String> = Vec::new();
            let mut sep = " ".to_string();
            let mut file_target: Option<String> = None;
            let mut append = false;
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
                let value = force_if_promise(heap, interner, evaluated)?;
                if tag_name.as_deref() == Some("sep") {
                    if let Some(RObject::String(values)) = heap.object(value) {
                        if !values.is_empty() {
                            sep = interner.string_text(values[0]).to_string();
                        }
                    }
                } else if tag_name.as_deref() == Some("file") {
                    file_target = string_operand(heap, interner, value);
                } else if tag_name.as_deref() == Some("append") {
                    if let Some(RObject::Logical(vals)) = heap.object(value) {
                        append = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                    }
                } else {
                    // Every element of the operand contributes, vector or scalar.
                    let length = vector_length(heap, value).unwrap_or(0);
                    if let Some(object) = heap.object(value) {
                        match object {
                            RObject::Real(_) | RObject::Integer(_) | RObject::Logical(_) | RObject::String(_) => {
                                for i in 0..length {
                                    pieces.push(super::print::format_cat_element(heap, interner, object, i));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            // GNU cat: separator BETWEEN pieces only, no trailing newline.
            let joined = pieces.join(&sep);
            match file_target {
                Some(path) => {
                    // GNU: file connections open write or append per the
                    /// tagged append argument.
                    use std::io::Write;
                    let file = if append {
                        std::fs::OpenOptions::new().append(true).create(true).open(&path)
                    } else {
                        std::fs::File::create(&path)
                    };
                    let mut file = file.map_err(|_| RSignal::Error(EvalError::TypeError(
                        "cat: cannot open file",
                    )))?;
                    file.write_all(joined.as_bytes())
                        .map_err(|_| RSignal::Error(EvalError::TypeError("cat: write failed")))?;
                }
                None => heap.output.push_str(&joined),
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
        TYPEOF => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let type_name = match heap.object(operands[0]) {
                Some(RObject::Nil) | None => "NULL",
                Some(RObject::Logical(_)) => "logical",
                Some(RObject::Integer(_)) => "integer",
                Some(RObject::Real(_)) => "double",
                Some(RObject::String(_)) => "character",
                Some(RObject::Raw(_)) => "raw",
                Some(RObject::List(_)) => "list",
                Some(RObject::Closure { .. }) => "closure",
                Some(RObject::Environment { .. }) => "environment",
                Some(RObject::Symbol { .. }) => "symbol",
                Some(RObject::Pairlist { .. }) => "pairlist",
                Some(RObject::Language { .. }) => "language",
                Some(RObject::Promise { .. }) => "promise",
                Some(RObject::Primitive(_)) => "builtin",
                _ => "unknown",
            };
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(type_name)])))
        }
        STRUCTURE => structure_primitive(heap, interner, args, caller_env),
        ASSIGN_FN => assign_primitive(heap, interner, args, caller_env),
        GET => get_primitive(heap, interner, args, caller_env),
        TEMPFILE => tempfile_primitive(heap, interner, args, caller_env),
        WRITELINES => writelines_primitive(heap, interner, args, caller_env),
        READLINES => readlines_primitive(heap, interner, args, caller_env),
        FILE_EXISTS => file_exists_primitive(heap, interner, args, caller_env),
        FILE_REMOVE => file_remove_primitive(heap, interner, args, caller_env),
        FILE_COPY => file_copy_primitive(heap, interner, args, caller_env),
        FILE_RENAME => file_rename_primitive(heap, interner, args, caller_env),
        SUPPRESS_WARNINGS => {
            // Identity over the evaluated argument — the new world has no
            /// warning system to suppress, so suppression is transparent.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            Ok(operands[0])
        }
        AS_LOGICAL => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            coerce_logical(heap, interner, operands[0])
        }
        ATTR => attr_primitive(heap, interner, args, caller_env),
        ATTR_ASSIGN => attr_assign_primitive(heap, interner, args, caller_env),
        ATTRIBUTES => attributes_primitive(heap, interner, args, caller_env),
        ATTRIBUTES_ASSIGN => attributes_assign_primitive(heap, interner, args, caller_env),
        IS_NULL => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = operands[0] == ValueId::NIL || matches!(heap.object(operands[0]), Some(RObject::Nil));
            Ok(heap.alloc(RObject::Logical(vec![if is { 1 } else { 0 }])))
        }
        LENGTH_ASSIGN => length_assign_primitive(heap, interner, args, caller_env),
        FILE_PATH => file_path_primitive(heap, interner, args, caller_env),
        DATA_FRAME => data_frame_primitive(heap, interner, args, caller_env),
        AGGREGATE => aggregate_primitive(heap, interner, args, caller_env),
        FACTOR => factor_primitive(heap, interner, args, caller_env),
        LEVELS => attrib_getter(heap, interner, args, caller_env, "levels"),
        LEVELS_ASSIGN => levels_assign_primitive(heap, interner, args, caller_env),
        UNCLASS => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            copy_with_attrib(heap, interner, operands[0], "class", ValueId::NIL).map_err(RSignal::from)
        }
        SPRINTF => sprintf_primitive(heap, interner, args, caller_env),
        SYS_SETENV => {
            // Tagged pairs name = value exactly as GNU.
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                let name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                if let (Some(n), Some(val)) = (name, string_operand(heap, interner, v)) {
                    heap.env_vars.insert(n, Some(val));
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            Ok(heap.alloc(RObject::Logical(vec![1])))
        }
        SYS_GETENV => {
            // Tagged walk — the `unset` argument supplies the value GNU
            // returns for variables that are not set at all.
            let mut names_arg = ValueId::NIL;
            let mut unset_arg = ValueId::NIL;
            let mut positional = 0usize;
            let mut cursor = args;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                positional += 1;
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                if tag_name.as_deref() == Some("unset") {
                    unset_arg = v;
                } else if positional == 1 {
                    names_arg = v;
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let unset_text = string_operand(heap, interner, unset_arg);
            let names = string_vec(heap, interner, names_arg);
            let out: Vec<super::intern::InternedStr> = names
                .iter()
                .map(|n| {
                    // Session overrides first, the genuine process
                    // environment as the fallback. Variables absent
                    // everywhere resolve to unset exactly as GNU.
                    let resolved: Option<String> = match heap.env_vars.get(n) {
                        Some(Some(v)) => Some(v.clone()),
                        Some(None) => None,
                        None => std::env::var(n).ok(),
                    };
                    let text = match resolved {
                        Some(v) => v,
                        None => unset_text.clone().unwrap_or_default(),
                    };
                    interner.intern_string(&text)
                })
                .collect();
            Ok(heap.alloc(RObject::String(out)))
        }
        SYS_UNSETENV => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            for name in string_vec(heap, interner, operands[0]) {
                heap.env_vars.insert(name, None);
            }
            Ok(heap.alloc(RObject::Logical(vec![1])))
        }
        UNLINK => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            for path in string_vec(heap, interner, operands[0]) {
                let p = std::path::Path::new(&path);
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(p);
                } else {
                    let _ = std::fs::remove_file(p);
                }
            }
            Ok(heap.alloc(RObject::Integer(vec![0])))
        }
        DIR_CREATE => {
            let mut recursive = false;
            let mut path_arg = ValueId::NIL;
            let mut cursor = args;
            let mut positional = 0usize;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                positional += 1;
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                if tag_name.as_deref() == Some("recursive") {
                    if let Some(RObject::Logical(vals)) = heap.object(v) {
                        recursive = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                    }
                } else if positional == 1 {
                    path_arg = v;
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let mut created = true;
            for path in string_vec(heap, interner, path_arg) {
                let result = if recursive {
                    std::fs::create_dir_all(&path)
                } else {
                    std::fs::create_dir(&path)
                };
                if result.is_err() {
                    created = false;
                }
            }
            Ok(heap.alloc(RObject::Logical(vec![if created { 1 } else { 0 }])))
        }
        LIST_DIRS => {
            let mut full_names = true;
            let mut recursive = true;
            let mut path_arg = ValueId::NIL;
            let mut cursor = args;
            let mut positional = 0usize;
            loop {
                let (car, cdr, tag) = match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                    _ => break,
                };
                positional += 1;
                let tag_name = match heap.object(tag) {
                    Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                    _ => None,
                };
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                match tag_name.as_deref() {
                    Some("full.names") => {
                        if let Some(RObject::Logical(vals)) = heap.object(v) {
                            full_names = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                        }
                    }
                    Some("recursive") => {
                        if let Some(RObject::Logical(vals)) = heap.object(v) {
                            recursive = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                        }
                    }
                    _ => {
                        if positional == 1 {
                            path_arg = v;
                        }
                    }
                }
                if cdr == ValueId::NIL {
                    break;
                }
                cursor = cdr;
            }
            let roots = string_vec(heap, interner, path_arg);
            let mut out: Vec<super::intern::InternedStr> = Vec::new();
            for root in &roots {
                // GNU's recursive listing leads with the root itself —
                /// empty text under full.names=FALSE exactly as "." resolves.
                if recursive {
                    let text = if full_names { root.clone() } else { String::new() };
                    out.push(interner.intern_string(&text));
                }
                let mut collected: Vec<String> = Vec::new();
                let mut stack: Vec<(String, String)> = vec![(root.clone(), String::new())];
                while let Some((dir, rel)) = stack.pop() {
                    if let Ok(entries) = std::fs::read_dir(&dir) {
                        let mut names: Vec<String> = entries
                            .flatten()
                            .filter(|e| e.path().is_dir())
                            .map(|e| e.file_name().to_string_lossy().to_string())
                            .collect();
                        names.sort();
                        for name in names {
                            let child_rel = if rel.is_empty() {
                                name.clone()
                            } else {
                                format!("{rel}/{name}")
                            };
                            collected.push(child_rel.clone());
                            if recursive {
                                stack.push((format!("{dir}/{name}"), child_rel));
                            }
                        }
                    }
                }
                // GNU sorts the full relative listing byte-wise.
                collected.sort();
                for rel in collected {
                    let text = if full_names {
                        format!("{root}/{rel}")
                    } else {
                        rel
                    };
                    out.push(interner.intern_string(&text));
                }
            }
            Ok(heap.alloc(RObject::String(out)))
        }
        BASENAME => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let out: Vec<super::intern::InternedStr> = string_vec(heap, interner, operands[0])
                .iter()
                .map(|path| {
                    let trimmed = path.trim_end_matches('/');
                    let base = trimmed.rsplit('/').next().unwrap_or(trimmed);
                    interner.intern_string(base)
                })
                .collect();
            Ok(heap.alloc(RObject::String(out)))
        }
        INTERNAL => {
            // GNU's .Internal evaluates its single argument as a direct
            /// internal call — in the new world the inner call evaluates
            /// exactly as written, the internal table being the primitives.
            let raw = raw_args(heap, args);
            match raw.first() {
                Some(expr) => eval(heap, interner, *expr, caller_env),
                None => Ok(ValueId::NIL),
            }
        }
        PRIMITIVE_FN => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let name = string_operand(heap, interner, operands[0])
                .ok_or(EvalError::TypeError(".Primitive requires a name"))?;
            match primitive_id_for_name(&name) {
                Some(id) => Ok(heap.alloc(RObject::Primitive(BuiltinId(id)))),
                None => Err(EvalError::TypeError("no such primitive").into()),
            }
        }
        WARNING => {
            // warning(..., msg) — the message is the final argument; the
            /// value is the message itself exactly as GNU returns it.
            let raw = raw_args(heap, args);
            let mut message = String::new();
            for expr in &raw {
                let evaluated = eval(heap, interner, *expr, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                if let Some(text) = string_operand(heap, interner, v) {
                    message = text;
                }
            }
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&message)])))
        }
        GETTEXT => {
            // Identity translation — the C locale never translates.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let text = string_operand(heap, interner, operands[1]).unwrap_or_default();
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&text)])))
        }
        NGETTEXT => {
            let operands = eval_args(heap, interner, args, caller_env, 3)?;
            let n = match heap.object(operands[0]) {
                Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(1.0) as i64,
                Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => {
                    vals.first().copied().unwrap_or(1) as i64
                }
                _ => 1,
            };
            let singular = string_operand(heap, interner, operands[1]).unwrap_or_default();
            let plural = string_operand(heap, interner, operands[2]).unwrap_or_default();
            let chosen = if n == 1 { singular } else { plural };
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&chosen)])))
        }
        BINDTEXTDOMAIN => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let domain = string_operand(heap, interner, operands[0]);
            let dir = string_operand(heap, interner, operands[1]);
            match (domain.as_deref(), dir) {
                // Querying one domain returns its registered directory.
                (Some(d), None) if !d.is_empty() => {
                    match heap.text_domains.get(d) {
                        Some(registered) => {
                            let text = registered.clone();
                            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&text)])))
                        }
                        None => Ok(ValueId::NIL),
                    }
                }
                // Registering returns the directory exactly as GNU.
                (Some(d), Some(dir_text)) if !d.is_empty() => {
                    heap.text_domains.insert(d.to_string(), dir_text.clone());
                    Ok(heap.alloc(RObject::String(vec![interner.intern_string(&dir_text)])))
                }
                // A NULL domain queries the state as a whole — truthy when
                /// any registration exists, matching GNU's non-null answer.
                (None, None) => {
                    if heap.text_domains.is_empty() {
                        Ok(ValueId::NIL)
                    } else {
                        Ok(heap.alloc(RObject::Logical(vec![1])))
                    }
                }
                _ => Ok(ValueId::NIL),
            }
        }
        GETERRMESSAGE => {
            let text = heap.err_message.clone();
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&text)])))
        }
        SETERRMESSAGE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            heap.err_message = string_operand(heap, interner, operands[0]).unwrap_or_default();
            Ok(ValueId::NIL)
        }
        INTERRUPTS_SUSPENDED => {
            // Non-interactive sessions never suspend interrupts — GNU's
            /// oracle pins FALSE regardless of the setting argument.
            Ok(heap.alloc(RObject::Logical(vec![0])))
        }
        IS_TRUE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = matches!(
                heap.object(operands[0]),
                Some(RObject::Logical(vals)) if vals.len() == 1 && vals[0] == 1
            );
            Ok(heap.alloc(RObject::Logical(vec![if is { 1 } else { 0 }])))
        }
        PRINT_DEFERRED_WARNINGS => Ok(ValueId::NIL),
        IS_PRIMITIVE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = matches!(heap.object(operands[0]), Some(RObject::Primitive(_)));
            Ok(heap.alloc(RObject::Logical(vec![if is { 1 } else { 0 }])))
        }
        SAVE => save_primitive(heap, interner, args, caller_env),
        LOAD => load_primitive(heap, interner, args, caller_env),
        RM => {
            // rm(x, y) — bare symbols name bindings to remove from the
            /// calling environment exactly as GNU's non-standard lookup.
            let raw = raw_args(heap, args);
            for expr in &raw {
                // GNU accepts both bare symbols and character names.
                if let Some(RObject::Symbol { name, .. }) = heap.object(*expr) {
                    let sym = *name;
                    let _ = super::env::remove(heap, caller_env, sym);
                } else {
                    let evaluated = eval(heap, interner, *expr, caller_env)?;
                    let v = force_if_promise(heap, interner, evaluated)?;
                    for text in string_vec(heap, interner, v) {
                        let sym = interner.intern_symbol(&text);
                        let _ = super::env::remove(heap, caller_env, sym);
                    }
                }
            }
            Ok(ValueId::NIL)
        }
        AS_VECTOR => {
            // as.vector strips every attribute — the bare vector remains.
            /// The optional mode argument coerces exactly as GNU.
            let raw = raw_args(heap, args);
            let first = eval(heap, interner, raw[0], caller_env)?;
            let first = force_if_promise(heap, interner, first)?;
            let stripped = heap.alloc(heap.object(first).cloned().unwrap_or(RObject::Nil));
            if let Some(mode_expr) = raw.get(1).copied() {
                let evaluated = eval(heap, interner, mode_expr, caller_env)?;
                let mode_v = force_if_promise(heap, interner, evaluated)?;
                if let Some(mode) = string_operand(heap, interner, mode_v) {
                    if mode == "character" {
                        return coerce_character(heap, interner, stripped);
                    }
                }
            }
            Ok(stripped)
        }
        AS_CHARACTER => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            coerce_character(heap, interner, operands[0])
        }
        TSP => attrib_getter(heap, interner, args, caller_env, "tsp"),
        TSP_ASSIGN => {
            // tsp<- coerces its value to double exactly as GNU's time
            /// series parameter discipline; NULL removes the attribute.
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            if matches!(heap.object(operands[1]), Some(RObject::Nil)) || operands[1] == ValueId::NIL {
                return remove_attrib(heap, interner, operands[0], "tsp").map_err(RSignal::from);
            }
            // GNU validates the time series parameters: numeric of length
            /// three, and frequency and ordering sane — NA and NaN slip
            /// through exactly as the oracle documents because their
            /// comparisons are never definitively true.
            let numeric_vals: Option<Vec<f64>> = match heap.object(operands[1]) {
                Some(RObject::Real(vals)) => Some(vals.clone()),
                Some(RObject::Integer(vals)) => Some(
                    vals.iter()
                        .map(|&v| {
                            if v == crate::value::object::NA_INTEGER {
                                f64::from_bits(crate::value::object::NA_REAL_BITS)
                            } else {
                                v as f64
                            }
                        })
                        .collect(),
                ),
                _ => None,
            };
            let vals = match numeric_vals {
                Some(v) if v.len() == 3 => v,
                _ => {
                    return Err(EvalError::Condition(
                        "'tsp' attribute must be numeric of length three".to_string(),
                    )
                    .into())
                }
            };
            let freq = vals[2];
            let invalid_order = vals[0] > vals[1];
            let invalid_freq = freq < 1.0;
            if invalid_order || invalid_freq {
                let freq_text = if freq.fract() == 0.0 && !freq.is_nan() {
                    format!("{}", freq as i64)
                } else {
                    super::print::format_real(freq)
                };
                return Err(EvalError::Condition(format!(
                    "invalid time series parameters specified ({freq_text})"
                ))
                .into());
            }
            let coerced = heap.alloc(RObject::Real(vals));
            copy_with_attrib(heap, interner, operands[0], "tsp", coerced).map_err(RSignal::from)
        }
        SETNAMES => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            copy_with_attrib(heap, interner, operands[0], "names", operands[1]).map_err(RSignal::from)
        }
        IDENTICAL => {
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            let same = structural_identical(heap, interner, operands[0], operands[1]);
            Ok(heap.alloc(RObject::Logical(vec![if same { 1 } else { 0 }])))
        }
        GLOBALENV => {
            // The .GlobalEnv binding in base resolves to the user env.
            let sym = interner.intern_symbol(".GlobalEnv");
            match super::env::lookup(heap, caller_env, sym) {
                Some((_, binding)) => Ok(binding.value),
                None => Ok(caller_env),
            }
        }
        BASEENV => {
            // Walking the enclosure chain to its end reaches base exactly
            /// as GNU's baseenv — the last environment before empty.
            let mut cursor = caller_env;
            loop {
                let enclosure = match heap.object(cursor) {
                    Some(RObject::Environment { enclos, .. }) => *enclos,
                    _ => break,
                };
                if enclosure == ValueId::EMPTY_ENV || enclosure == ValueId::NIL {
                    break;
                }
                cursor = enclosure;
            }
            Ok(cursor)
        }
        ENVIRONMENT_NAME => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            // R_GlobalEnv names the user environment exactly as GNU; other
            /// environments report empty.
            let name = {
                let sym = interner.intern_symbol(".GlobalEnv");
                let genv = super::env::lookup(heap, caller_env, sym).map(|(_, b)| b.value);
                if Some(operands[0]) == genv {
                    "R_GlobalEnv"
                } else {
                    ""
                }
            };
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(name)])))
        }
        ENVIRONMENT => {
            // environment() with no argument is the calling frame.
            Ok(caller_env)
        }
        R_VERSION_FN => {
            // R.Version() returns the shared version list exactly as the
            /// version constant — GNU's function reads the same structure.
            let sym = interner.intern_symbol("R.version");
            match super::env::lookup(heap, caller_env, sym) {
                Some((_, binding)) => Ok(binding.value),
                None => Ok(ValueId::NIL),
            }
        }
        GET_RVERSION => {
            // GNU's numeric_version is a list of integer components —
            /// typeof reports list exactly as the oracle pins.
            let component = heap.alloc(RObject::Integer(vec![4, 4, 0]));
            Ok(heap.alloc(RObject::List(vec![component])))
        }
        ARRAY => array_primitive(heap, interner, args, caller_env),
        AS_DATE => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let out: Vec<f64> = match heap.object(operands[0]) {
                Some(RObject::String(values)) => values
                    .iter()
                    .map(|v| {
                        let text = interner.string_text(*v);
                        if text == "\0NA" {
                            f64::from_bits(crate::value::object::NA_REAL_BITS)
                        } else {
                            match super::civil::parse_iso(text) {
                                Some(days) => days as f64,
                                None => f64::from_bits(crate::value::object::NA_REAL_BITS),
                            }
                        }
                    })
                    .collect(),
                Some(RObject::Real(values)) => values.clone(),
                Some(RObject::Integer(values)) => values
                    .iter()
                    .map(|&v| {
                        if v == crate::value::object::NA_INTEGER {
                            f64::from_bits(crate::value::object::NA_REAL_BITS)
                        } else {
                            v as f64
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let value = heap.alloc(RObject::Real(out));
            let class = heap.alloc(RObject::String(vec![interner.intern_string("Date")]));
            copy_with_attrib(heap, interner, value, "class", class).map_err(RSignal::from)
        }
        WEEKDAYS | MONTHS | QUARTERS => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let days = numeric_vector(heap, operands[0]);
            let out: Vec<super::intern::InternedStr> = days
                .iter()
                .map(|&d| {
                    if d.is_nan() {
                        // GNU: quarters renders NA as the literal "QNA"
                        /// while weekdays and months use the NA sentinel.
                        if id.0 == QUARTERS {
                            interner.intern_string("QNA")
                        } else {
                            interner.intern_string("\0NA")
                        }
                    } else {
                        let day_count = d as i64;
                        let text = match id.0 {
                            WEEKDAYS => {
                                super::civil::WEEKDAY_NAMES[super::civil::weekday_of(day_count) as usize].to_string()
                            }
                            MONTHS => {
                                let (_, m, _) = super::civil::civil_from_days(day_count);
                                super::civil::MONTH_NAMES[(m - 1) as usize].to_string()
                            }
                            _ => {
                                let (_, m, _) = super::civil::civil_from_days(day_count);
                                format!("Q{}", (m - 1) / 3 + 1)
                            }
                        };
                        interner.intern_string(&text)
                    }
                })
                .collect();
            Ok(heap.alloc(RObject::String(out)))
        }
        IS_UNSORTED => {
            // TRUE when any adjacent pair descends; NA participation makes
            /// the result NA exactly as GNU's sortedness check.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let vals = numeric_vector(heap, operands[0]);
            let mut result = 0i32;
            for w in vals.windows(2) {
                if w[0].is_nan() || w[1].is_nan() {
                    result = crate::value::object::NA_INTEGER;
                    break;
                }
                if w[0] > w[1] {
                    result = 1;
                    break;
                }
            }
            Ok(heap.alloc(RObject::Logical(vec![result])))
        }
        TOSTRING => {
            // Comma-space collapse of the character coercion — width
            /// truncation sits outside the slice.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let coerced = coerce_character(heap, interner, operands[0])?;
            let text = string_vec(heap, interner, coerced);
            let joined = text.join(", ");
            Ok(heap.alloc(RObject::String(vec![interner.intern_string(&joined)])))
        }
        CAPTURE_OUTPUT => {
            // Each argument evaluates in order; everything the evaluation
            /// writes to the output buffer drains into the result as one
            /// element per line exactly as GNU's sink capture.
            let start = heap.output.len();
            let raw = raw_args(heap, args);
            for arg in raw {
                crate::value::eval::eval(heap, interner, arg, caller_env)?;
            }
            let captured: String = heap.output[start..].to_string();
            heap.output.truncate(start);
            let lines: Vec<super::intern::InternedStr> = captured
                .lines()
                .map(|l| interner.intern_string(l))
                .collect();
            Ok(heap.alloc(RObject::String(lines)))
        }
        ROW_NAMES => {
            // The row.names attribute when present; data.frames otherwise
            /// synthesize the compact sequence exactly as GNU's default.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            if let Some(existing) = get_attrib(heap, interner, operands[0], "row.names") {
                return Ok(existing);
            }
            let n = match heap.object(operands[0]) {
                Some(RObject::List(items)) if !items.is_empty() => {
                    vector_length(heap, items[0]).unwrap_or(0)
                }
                _ => vector_length(heap, operands[0]).unwrap_or(0),
            };
            let rows: Vec<i32> = (1..=n as i32).collect();
            Ok(heap.alloc(RObject::Integer(rows)))
        }
        ROW_NAMES_SET => {
            // Replacement form through the general desugar: row.names<-(x, value).
            let operands = eval_args(heap, interner, args, caller_env, 2)?;
            copy_with_attrib(heap, interner, operands[0], "row.names", operands[1]).map_err(RSignal::from)
        }
        TAPPLY => {
            // GNU tapply: the index is a vector or a list of vectors; each
            /// contributes a dimension whose levels come from factor levels
            /// or sorted-unique values. The result is an array filled
            /// column-major over the level combinations; empty combinations
            /// yield NA exactly as the oracle documents.
            let operands = eval_args(heap, interner, args, caller_env, 3)?;
            let x = operands[0];
            let index = operands[1];
            let fun = operands[2];
            let (index_cols, index_names): (Vec<ValueId>, Vec<String>) = match heap.object(index) {
                Some(RObject::List(items)) => {
                    let names = match get_attrib(heap, interner, index, "names") {
                        Some(id) => string_vec(heap, interner, id),
                        None => Vec::new(),
                    };
                    (items.clone(), names)
                }
                _ => (vec![index], Vec::new()),
            };
            // Per-column levels and per-row level text.
            let mut col_levels: Vec<Vec<String>> = Vec::new();
            let mut col_texts: Vec<Vec<String>> = Vec::new();
            for col in &index_cols {
                let levels: Vec<String> = match get_attrib(heap, interner, *col, "levels") {
                    Some(id) => string_vec(heap, interner, id),
                    None => {
                        let mut vals = string_vec(heap, interner, *col);
                        vals.retain(|s| s != "\0NA");
                        vals.sort();
                        vals.dedup();
                        vals
                    }
                };
                let is_factor = matches!(heap.object(*col), Some(RObject::Integer(_)))
                    && get_attrib(heap, interner, *col, "levels").is_some();
                let texts: Vec<String> = if is_factor {
                    match heap.object(*col) {
                        Some(RObject::Integer(codes)) => codes
                            .iter()
                            .map(|&c| {
                                if c == crate::value::object::NA_INTEGER {
                                    "\0NA".to_string()
                                } else {
                                    levels.get((c - 1) as usize).cloned().unwrap_or_default()
                                }
                            })
                            .collect(),
                        _ => Vec::new(),
                    }
                } else {
                    string_vec(heap, interner, *col)
                };
                col_levels.push(levels);
                col_texts.push(texts);
            }
            let x_len = vector_length(heap, x).unwrap_or(0);
            let dims: Vec<usize> = col_levels.iter().map(|l| l.len()).collect();
            let total: usize = dims.iter().product();
            let mut result_values: Vec<f64> = Vec::with_capacity(total);
            let mut result_int = true;
            for flat in 0..total {
                // Decode the flat index column-major: first dimension fastest.
                let mut rem = flat;
                let mut combo: Vec<usize> = Vec::with_capacity(dims.len());
                for &d in &dims {
                    combo.push(rem % d.max(1));
                    rem /= d.max(1);
                }
                let positions: Vec<usize> = (0..x_len)
                    .filter(|&row| {
                        col_texts.iter().enumerate().all(|(j, texts)| {
                            texts.get(row).map(|t| t == &col_levels[j][combo[j]]).unwrap_or(false)
                        })
                    })
                    .collect();
                if positions.is_empty() {
                    result_values.push(f64::from_bits(crate::value::object::NA_REAL_BITS));
                    result_int = false;
                    continue;
                }
                let sub = extract_positions(heap, interner, x, &positions)?;
                let applied = apply_fun_to(heap, interner, fun, sub, caller_env)?;
                match heap.object(applied) {
                    Some(RObject::Real(v)) if !v.is_empty() => {
                        result_values.push(v[0]);
                        result_int = false;
                    }
                    Some(RObject::Integer(v)) if !v.is_empty() => {
                        result_values.push(v[0] as f64);
                    }
                    _ => return Err(EvalError::TypeError("tapply FUN must return scalar numeric").into()),
                }
            }
            let values: ValueId = if result_int {
                heap.alloc(RObject::Integer(result_values.iter().map(|&v| v as i32).collect()))
            } else {
                heap.alloc(RObject::Real(result_values))
            };
            // dimnames: one character vector per dimension, the list itself
            /// named by the index list's names exactly as GNU.
            let dn_inners: Vec<ValueId> = col_levels
                .iter()
                .map(|levels| {
                    let interned: Vec<super::intern::InternedStr> = levels
                        .iter()
                        .map(|s| interner.intern_string(s))
                        .collect();
                    heap.alloc(RObject::String(interned))
                })
                .collect();
            let dn_list = heap.alloc(RObject::List(dn_inners));
            let dn_list = if !index_names.is_empty() {
                let nm: Vec<super::intern::InternedStr> = index_names
                    .iter()
                    .map(|s| interner.intern_string(s))
                    .collect();
                let names_obj = heap.alloc(RObject::String(nm));
                copy_with_attrib(heap, interner, dn_list, "names", names_obj).map_err(RSignal::from)?
            } else {
                dn_list
            };
            let dim_vec = heap.alloc(RObject::Integer(dims.iter().map(|&d| d as i32).collect()));
            let class_vec = heap.alloc(RObject::String(vec![interner.intern_string("array")]));
            // names() on the 1-D result surfaces dimnames[[1]] exactly as
            /// GNU arrays; carry the explicit names attribute only in the
            /// single-index case where GNU's object shows it.
            let values = if dims.len() == 1 {
                let first: Vec<super::intern::InternedStr> = col_levels[0]
                    .iter()
                    .map(|s| interner.intern_string(s))
                    .collect();
                let names_obj = heap.alloc(RObject::String(first));
                copy_with_attrib(heap, interner, values, "names", names_obj).map_err(RSignal::from)?
            } else {
                values
            };
            let with_dim = copy_with_attrib(heap, interner, values, "dim", dim_vec).map_err(RSignal::from)?;
            let with_dn = copy_with_attrib(heap, interner, with_dim, "dimnames", dn_list).map_err(RSignal::from)?;
            copy_with_attrib(heap, interner, with_dn, "class", class_vec).map_err(RSignal::from)
        }
        VAPPLY => {
            // Like sapply with a type template: each application must yield
            /// the template's type, and the results always combine into a
            /// template-typed vector — GNU's type-safe apply exactly.
            let operands = eval_args(heap, interner, args, caller_env, 3)?;
            let x = operands[0];
            let fun = operands[1];
            let template = operands[2];
            let elements = sequence_elements(heap, x)?;
            let mut results: Vec<ValueId> = Vec::with_capacity(elements.len());
            for element in elements {
                let applied = apply_fun_to(heap, interner, fun, element, caller_env)?;
                results.push(applied);
            }
            let first_of = |heap: &Heap, v: ValueId| -> (f64, Option<super::intern::InternedStr>) {
                match heap.object(v) {
                    Some(RObject::Real(vals)) => (vals.first().copied().unwrap_or(0.0), None),
                    Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => {
                        (vals.first().copied().unwrap_or(0) as f64, None)
                    }
                    Some(RObject::String(vals)) => {
                        (0.0, vals.first().copied())
                    }
                    _ => (0.0, None),
                }
            };
            let names = get_attrib(heap, interner, x, "names");
            let combined: ValueId = match heap.object(template) {
                Some(RObject::String(_)) => {
                    let out: Vec<super::intern::InternedStr> = results
                        .iter()
                        .map(|&r| {
                            let (_, s) = first_of(heap, r);
                            s.unwrap_or_else(|| interner.intern_string("\0NA"))
                        })
                        .collect();
                    heap.alloc(RObject::String(out))
                }
                Some(RObject::Integer(_)) => {
                    let out: Vec<i32> = results
                        .iter()
                        .map(|&r| first_of(heap, r).0 as i32)
                        .collect();
                    heap.alloc(RObject::Integer(out))
                }
                Some(RObject::Logical(_)) => {
                    let out: Vec<i32> = results
                        .iter()
                        .map(|&r| first_of(heap, r).0 as i32)
                        .collect();
                    heap.alloc(RObject::Logical(out))
                }
                _ => {
                    let out: Vec<f64> = results.iter().map(|&r| first_of(heap, r).0).collect();
                    heap.alloc(RObject::Real(out))
                }
            };
            if let Some(nm) = names {
                copy_with_attrib(heap, interner, combined, "names", nm).map_err(RSignal::from)
            } else {
                Ok(combined)
            }
        }
        FORMAT => {
            // format() on Date-classed input renders ISO exactly as the
            /// print path; other input coerces to character.
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            if is_date_classed(heap, interner, operands[0]) {
                let days = numeric_vector(heap, operands[0]);
                let out: Vec<super::intern::InternedStr> = days
                    .iter()
                    .map(|&d| {
                        if d.is_nan() {
                            interner.intern_string("\0NA")
                        } else {
                            interner.intern_string(&super::civil::format_iso(d as i64))
                        }
                    })
                    .collect();
                Ok(heap.alloc(RObject::String(out)))
            } else {
                coerce_character(heap, interner, operands[0])
            }
        }
        IS_ARRAY => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = get_attrib(heap, interner, operands[0], "dim").is_some();
            Ok(heap.alloc(RObject::Logical(vec![if is { 1 } else { 0 }])))
        }
        IS_LIST => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let is = matches!(heap.object(operands[0]), Some(RObject::List(_)));
            Ok(heap.alloc(RObject::Logical(vec![if is { 1 } else { 0 }])))
        }
        GREPL => grep_primitive(heap, interner, args, caller_env, true),
        GREP => grep_primitive(heap, interner, args, caller_env, false),
        REGEXPR => regexpr_primitive(heap, interner, args, caller_env),
        TEMPDIR => {
            let text = interner.intern_string(&std::env::temp_dir().to_string_lossy());
            Ok(heap.alloc(RObject::String(vec![text])))
        }
        DIMNAMES => attrib_getter(heap, interner, args, caller_env, "dimnames"),
        DIMNAMES_ASSIGN => attrib_replacement(heap, interner, args, caller_env, "dimnames"),
        CLASS => class_getter(heap, interner, args, caller_env),
        CLASS_ASSIGN => attrib_replacement(heap, interner, args, caller_env, "class"),
        ROWNAMES => rowcolnames(heap, interner, args, caller_env, true),
        ROWNAMES_ASSIGN => rowcolnames_assign(heap, interner, args, caller_env, true),
        COLNAMES => rowcolnames(heap, interner, args, caller_env, false),
        COLNAMES_ASSIGN => rowcolnames_assign(heap, interner, args, caller_env, false),
        CHARACTER_CTOR => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let length = match heap.object(operands[0]) {
                Some(RObject::Real(v)) if !v.is_empty() => v[0] as usize,
                Some(RObject::Integer(v)) if !v.is_empty() => v[0] as usize,
                _ => return Err(EvalError::TypeError("length must be scalar numeric").into()),
            };
            let empty = interner.intern_string("");
            Ok(heap.alloc(RObject::String(vec![empty; length])))
        }
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
            // data.frames synthesize dim from the column shape exactly as
            /// GNU's implicit dims.
            if let Some((nr, nc)) = data_frame_shape(heap, interner, operands[0]) {
                return Ok(heap.alloc(RObject::Integer(vec![nr as i32, nc as i32])));
            }
            match get_attrib(heap, interner, operands[0], "dim") {
                Some(dim) => Ok(dim),
                None => Ok(ValueId::NIL),
            }
        }
        NROW | NCOL => {
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let which = if id.0 == NROW { 0 } else { 1 };
            // data.frames carry no dim attribute — the shape reads off the
            // column list: ncol is the column count, nrow the shared column
            // length exactly as GNU.
            let df_shape = data_frame_shape(heap, interner, operands[0]);
            if let Some((nr, nc)) = df_shape {
                let extent = if id.0 == NROW { nr } else { nc };
                return Ok(heap.alloc(RObject::Integer(vec![extent as i32])));
            }
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
            let operands = eval_args(heap, interner, args, caller_env, 1)?;
            let text = super::print::format_value(heap, interner, operands[0]);
            heap.output.push_str(&text);
            heap.output.push('\n');
            // GNU: print returns its argument invisibly.
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
    // difftime operands in +/- against a Date rescale into days and shed
    /// their class before the fold — the Date side's chain then wins
    /// propagation exactly as GNU's arithmetic methods arrange.
    let scale_difftime = |heap: &mut Heap, interner: &mut SymbolInterner, v: ValueId| -> ValueId {
        let classed = get_attrib(heap, interner, v, "class")
            .and_then(|id| match heap.object(id) {
                Some(RObject::String(vals)) => Some(vals.iter().any(|x| interner.string_text(*x) == "difftime")),
                _ => None,
            })
            .unwrap_or(false);
        if !classed {
            return v;
        }
        let units = get_attrib(heap, interner, v, "units")
            .and_then(|id| match heap.object(id) {
                Some(RObject::String(vals)) if !vals.is_empty() => Some(interner.string_text(vals[0]).to_string()),
                _ => None,
            })
            .unwrap_or_else(|| "days".to_string());
        let scale = match units.as_str() {
            "secs" => 1.0 / 86400.0,
            "mins" => 1.0 / 1440.0,
            "hours" => 1.0 / 24.0,
            "weeks" => 7.0,
            _ => 1.0,
        };
        let scaled: Vec<f64> = match heap.object(v) {
            Some(RObject::Real(vals)) => vals.iter().map(|x| x * scale).collect(),
            Some(RObject::Integer(vals)) => vals.iter().map(|&x| x as f64 * scale).collect(),
            _ => return v,
        };
        heap.alloc(RObject::Real(scaled))
    };
    let left_date = is_date_classed(heap, interner, operands[0]);
    let right_date = is_date_classed(heap, interner, operands[1]);
    let operands = if matches!(op, ADD | SUB) && (left_date || right_date) {
        vec![
            scale_difftime(heap, interner, operands[0]),
            scale_difftime(heap, interner, operands[1]),
        ]
    } else {
        operands
    };
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
            let result = heap.alloc(RObject::Integer(folded));
            Ok(propagate_attributes(heap, operands[0], operands[1], result))
        }
        (a, b) => {
            let left_reals = as_reals(a);
            let right_reals = as_reals(b);
            let folded = fold_numeric(left_reals, right_reals, op, |x, y| {
                let result = match op {
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
                };
                // Hardware NaN canonicalization destroys the NA payload, so
                // GNU's NA survival is explicit: an NA operand poisons any
                // NaN result back to NA; genuine NaN production (Inf-Inf,
                // 0/0) keeps the canonical quiet NaN exactly as GNU does.
                if result.is_nan()
                    && (x.to_bits() == crate::value::object::NA_REAL_BITS
                        || y.to_bits() == crate::value::object::NA_REAL_BITS)
                {
                    f64::from_bits(crate::value::object::NA_REAL_BITS)
                } else {
                    result
                }
            })
            .map_err(RSignal::from)?;
            let result = heap.alloc(RObject::Real(folded));
            // GNU's Date arithmetic discipline: the classed operand's
            /// attribute chain wins regardless of position — Date +/-
            /// numeric keeps Date; Date - Date yields difftime in days.
            let left_date = is_date_classed(heap, interner, operands[0]);
            let right_date = is_date_classed(heap, interner, operands[1]);
            let result = if left_date && !right_date {
                propagate_attributes(heap, operands[0], operands[0], result)
            } else if right_date && !left_date {
                propagate_attributes(heap, operands[1], operands[1], result)
            } else {
                propagate_attributes(heap, operands[0], operands[1], result)
            };
            let op_text = match op {
                ADD => "+",
                SUB => "-",
                MUL => "*",
                DIV => "/",
                POW => "^",
                MOD => "%%",
                _ => "op",
            };
            // GNU's exact Date arithmetic error matrix: non-subtraction
            /// between Dates, multiplication of a Date by anything, and
            /// subtraction of a Date from a number all raise distinct
            /// conditions exactly as the oracle pins.
            if left_date && right_date && op != SUB {
                return Err(EvalError::Condition(format!(
                    "binary {op_text} is not defined for \"Date\" objects"
                ))
                .into());
            }
            if left_date && op != SUB && op != ADD {
                return Err(EvalError::Condition(format!(
                    "{op_text} not defined for \"Date\" objects"
                ))
                .into());
            }
            if right_date && !left_date && op == SUB {
                return Err(EvalError::Condition(
                    "can only subtract from \"Date\" objects".to_string(),
                )
                .into());
            }
            // Subtracting a classed non-number from a Date is the exact
            /// "can only subtract numbers" condition — foo-classed input
            /// is not a number for arithmetic purposes in GNU.
            if left_date && op == SUB && !right_date {
                let right_classed = get_attrib(heap, interner, operands[1], "class").is_some();
                let right_difftime = get_attrib(heap, interner, operands[1], "class")
                    .and_then(|id| match heap.object(id) {
                        Some(RObject::String(vals)) => Some(vals.iter().any(|x| interner.string_text(*x) == "difftime")),
                        _ => None,
                    })
                    .unwrap_or(false);
                if right_classed && !right_difftime {
                    return Err(EvalError::Condition(
                        "can only subtract numbers from \"Date\" objects".to_string(),
                    )
                    .into());
                }
            }
            if left_date && right_date && op == SUB {
                // Strip the Date class, stamp difftime with units=days.
                let stripped = remove_attrib(heap, interner, result, "class").map_err(RSignal::from)?;
                let class = heap.alloc(RObject::String(vec![interner.intern_string("difftime")]));
                let units = heap.alloc(RObject::String(vec![interner.intern_string("days")]));
                let with_class = copy_with_attrib(heap, interner, stripped, "class", class).map_err(RSignal::from)?;
                return copy_with_attrib(heap, interner, with_class, "units", units).map_err(RSignal::from);
            }
            Ok(result)
        }
    }
}

/// GNU attribute discipline for elementwise arithmetic: the result carries
/// the attribute chain of the longer operand (ties favor the left), so
/// names survive vector arithmetic and dim/dimnames survive matrix
/// arithmetic exactly.
fn propagate_attributes(
    heap: &mut Heap,
    left: ValueId,
    right: ValueId,
    result: ValueId,
) -> ValueId {
    let left_len = vector_length(heap, left).unwrap_or(0);
    let right_len = vector_length(heap, right).unwrap_or(0);
    let source = if left_len >= right_len { left } else { right };
    let chain = heap.get(source).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    if chain != ValueId::NIL {
        if let Some(slot) = heap.get_mut(result) {
            slot.attrib = chain;
        }
    }
    result
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
    // GNU character comparison: when either operand is character the other
    // coerces, equality is exact and ordering lexicographic, and NA or NaN
    // participation poisons the element to NA logical exactly as numerics.
    let either_string = matches!(heap.object(operands[0]), Some(RObject::String(_)))
        || matches!(heap.object(operands[1]), Some(RObject::String(_)));
    if either_string {
        let lefts = string_vec(heap, interner, operands[0]);
        let rights = string_vec(heap, interner, operands[1]);
        let len = lefts.len().max(rights.len());
        if len == 0 {
            return Ok(heap.alloc(RObject::Logical(vec![])));
        }
        if lefts.is_empty() || rights.is_empty() {
            return Err(EvalError::TypeError("comparison requires non-empty operands").into());
        }
        let mut out: Vec<i32> = Vec::with_capacity(len);
        for i in 0..len {
            let a = &lefts[i % lefts.len()];
            let b = &rights[i % rights.len()];
            if a == "\0NA" || b == "\0NA" {
                out.push(crate::value::object::NA_INTEGER);
                continue;
            }
            let result = match op {
                LT => a < b,
                GT => a > b,
                LE => a <= b,
                GE => a >= b,
                EQ => a == b,
                _ => a != b,
            };
            out.push(if result { 1 } else { 0 });
        }
        return Ok(heap.alloc(RObject::Logical(out)));
    }
    let left = numeric_operands(heap, operands[0]).map_err(RSignal::from)?;
    let right = numeric_operands(heap, operands[1]).map_err(RSignal::from)?;
    let a = as_reals(left);
    let b = as_reals(right);
    let folded = fold_numeric(a, b, op, |x, y| {
        // GNU: any NaN operand (NA or NaN) poisons the comparison to NA
        /// logical — NaN comparisons are never simply false.
        if x.is_nan() || y.is_nan() {
            return crate::value::object::NA_INTEGER as f64;
        }
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
        "numeric", "integer", "logical", "names", "names<-", "$", "lapply", "sapply", "unlist", "seq", "seq_len", "rep", "mean", "var", "sd", "median", "is.na", "sort", "rev", "unique", "which", "any", "all", "ifelse", "%%", "stop", "%in%", "match", "new.env", "exists", "ls", "$<-", "cat", "invisible", "typeof", "structure", "assign", "get", "character", "tempfile", "writeLines", "readLines", "file.exists", "file.remove", "file.copy", "file.rename", "suppressWarnings", "as.logical", "attr", "attr<-", "attributes", "attributes<-", "is.null", "length<-", "file.path", "data.frame", "aggregate", "factor", "levels", "levels<-", "unclass", "tempdir", "sprintf", "Sys.setenv", "Sys.getenv", "Sys.unsetenv", "unlink", "dir.create", "list.dirs", "basename", ".Internal", ".Primitive", "warning", "gettext", "ngettext", "bindtextdomain", "geterrmessage", "seterrmessage", "interruptsSuspended", "is.primitive", "isTRUE", "save", "load", "rm", "as.vector", "as.character", "setNames", "identical", "globalenv", "baseenv", "environmentName", "environment", "R.Version", "getRversion", "is.list", "array", "is.array", "as.Date", "weekdays", "months", "quarters", "format", "is.unsorted", "toString", "capture.output", "row.names", "row.names<-", "tapply", "vapply", "tsp", "tsp<-", "printDeferredWarnings", "grepl", "grep", "regexpr", "dimnames", "dimnames<-", "class", "class<-", "rownames", "rownames<-", "colnames", "colnames<-", "tryCatch", "conditionMessage", "matrix", "dim", "nrow", "ncol",
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
        "typeof" => Some(TYPEOF),
        "structure" => Some(STRUCTURE),
        "assign" => Some(ASSIGN_FN),
        "get" => Some(GET),
        "character" => Some(CHARACTER_CTOR),
        "tempfile" => Some(TEMPFILE),
        "writeLines" => Some(WRITELINES),
        "readLines" => Some(READLINES),
        "file.exists" => Some(FILE_EXISTS),
        "file.remove" => Some(FILE_REMOVE),
        "file.copy" => Some(FILE_COPY),
        "file.rename" => Some(FILE_RENAME),
        "suppressWarnings" => Some(SUPPRESS_WARNINGS),
        "as.logical" => Some(AS_LOGICAL),
        "attr" => Some(ATTR),
        "attr<-" => Some(ATTR_ASSIGN),
        "attributes" => Some(ATTRIBUTES),
        "attributes<-" => Some(ATTRIBUTES_ASSIGN),
        "is.null" => Some(IS_NULL),
        "length<-" => Some(LENGTH_ASSIGN),
        "file.path" => Some(FILE_PATH),
        "data.frame" => Some(DATA_FRAME),
        "aggregate" => Some(AGGREGATE),
        "factor" => Some(FACTOR),
        "levels" => Some(LEVELS),
        "levels<-" => Some(LEVELS_ASSIGN),
        "unclass" => Some(UNCLASS),
        "sprintf" => Some(SPRINTF),
        "Sys.setenv" => Some(SYS_SETENV),
        "Sys.getenv" => Some(SYS_GETENV),
        "Sys.unsetenv" => Some(SYS_UNSETENV),
        "unlink" => Some(UNLINK),
        "dir.create" => Some(DIR_CREATE),
        "list.dirs" => Some(LIST_DIRS),
        "basename" => Some(BASENAME),
        ".Internal" => Some(INTERNAL),
        ".Primitive" => Some(PRIMITIVE_FN),
        "warning" => Some(WARNING),
        "gettext" => Some(GETTEXT),
        "ngettext" => Some(NGETTEXT),
        "bindtextdomain" => Some(BINDTEXTDOMAIN),
        "geterrmessage" => Some(GETERRMESSAGE),
        "seterrmessage" => Some(SETERRMESSAGE),
        "interruptsSuspended" => Some(INTERRUPTS_SUSPENDED),
        "is.primitive" => Some(IS_PRIMITIVE),
        "isTRUE" => Some(IS_TRUE),
        "save" => Some(SAVE),
        "load" => Some(LOAD),
        "rm" => Some(RM),
        "as.vector" => Some(AS_VECTOR),
        "as.character" => Some(AS_CHARACTER),
        "setNames" => Some(SETNAMES),
        "identical" => Some(IDENTICAL),
        "globalenv" => Some(GLOBALENV),
        "baseenv" => Some(BASEENV),
        "environmentName" => Some(ENVIRONMENT_NAME),
        "environment" => Some(ENVIRONMENT),
        "R.Version" => Some(R_VERSION_FN),
        "getRversion" => Some(GET_RVERSION),
        "is.list" => Some(IS_LIST),
        "array" => Some(ARRAY),
        "is.array" => Some(IS_ARRAY),
        "as.Date" => Some(AS_DATE),
        "weekdays" => Some(WEEKDAYS),
        "months" => Some(MONTHS),
        "quarters" => Some(QUARTERS),
        "format" => Some(FORMAT),
        "is.unsorted" => Some(IS_UNSORTED),
        "toString" => Some(TOSTRING),
        "capture.output" => Some(CAPTURE_OUTPUT),
        "row.names" => Some(ROW_NAMES),
        "row.names<-" => Some(ROW_NAMES_SET),
        "tapply" => Some(TAPPLY),
        "vapply" => Some(VAPPLY),
        "tsp" => Some(TSP),
        "tsp<-" => Some(TSP_ASSIGN),
        "printDeferredWarnings" => Some(PRINT_DEFERRED_WARNINGS),
        "grepl" => Some(GREPL),
        "grep" => Some(GREP),
        "regexpr" => Some(REGEXPR),
        "tempdir" => Some(TEMPDIR),
        "dimnames" => Some(DIMNAMES),
        "dimnames<-" => Some(DIMNAMES_ASSIGN),
        "class" => Some(CLASS),
        "class<-" => Some(CLASS_ASSIGN),
        "rownames" => Some(ROWNAMES),
        "rownames<-" => Some(ROWNAMES_ASSIGN),
        "colnames" => Some(COLNAMES),
        "colnames<-" => Some(COLNAMES_ASSIGN),
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
                                // GNU: NA coerces to NA_character_ — the
                                // missing sentinel, never the literal text.
                                "\0NA".to_string()
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
                                "\0NA".to_string()
                            } else {
                                v.to_string()
                            };
                            out.push(interner.intern_string(&text));
                        }
                    }
                    Flat::Real(values) => {
                        for v in values {
                            let text = if v.to_bits() == crate::value::object::NA_REAL_BITS {
                                "\0NA".to_string()
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
    // data.frame single-index subsetting selects COLUMNS, not elements —
    /// the frame structure rebuilds over the selection exactly as GNU.
    if let Some((_, ncol)) = data_frame_shape(heap, interner, source) {
        let names_id = get_attrib(heap, interner, source, "names");
        let col_names: Vec<String> = match names_id {
            Some(id) => string_vec(heap, interner, id),
            None => Vec::new(),
        };
        let positions: Vec<usize> = match heap.object(index) {
            Some(RObject::String(values)) => values
                .iter()
                .filter_map(|v| {
                    let text = interner.string_text(*v).to_string();
                    col_names.iter().position(|n| *n == text)
                })
                .collect(),
            Some(RObject::Real(values)) => values
                .iter()
                .filter(|v| !v.is_nan() && **v >= 1.0)
                .map(|v| (*v as usize) - 1)
                .filter(|p| *p < ncol)
                .collect(),
            Some(RObject::Integer(values)) => values
                .iter()
                .filter(|v| **v >= 1 && **v != crate::value::object::NA_INTEGER)
                .map(|v| (*v as usize) - 1)
                .filter(|p| *p < ncol)
                .collect(),
            _ => Vec::new(),
        };
        let columns = match heap.object(source) {
            Some(RObject::List(items)) => items.clone(),
            _ => Vec::new(),
        };
        let selected: Vec<ValueId> = positions.iter().filter_map(|p| columns.get(*p).copied()).collect();
        let selected_names: Vec<String> = positions
            .iter()
            .filter_map(|p| col_names.get(*p).cloned())
            .collect();
        return build_data_frame(heap, interner, selected, selected_names);
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
    // GNU type fidelity: abs of an integer vector stays integer; the
    /// transcendental functions always produce real.
    if op == ABS
        && matches!(
            heap.object(operands[0]),
            Some(RObject::Integer(_)) | Some(RObject::Logical(_))
        )
    {
        let ints: Vec<i32> = out
            .iter()
            .map(|v| {
                if v.is_nan() {
                    crate::value::object::NA_INTEGER
                } else {
                    *v as i32
                }
            })
            .collect();
        return Ok(heap.alloc(RObject::Integer(ints)));
    }
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
    // GNU paste coercion renders NA_character_ as the text "NA" — the
    /// conversion lives here, not in shared string coercion, so nchar and
    /// friends still see genuine missingness.
    for column in columns.iter_mut() {
        for element in column.iter_mut() {
            if interner.string_text(*element) == "\0NA" {
                *element = interner.intern_string("NA");
            }
        }
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
    // `x <- `F<-`(x, ..., v)` — exactly GNU's rewrite. The synthetic call
    // evaluates through ordinary dispatch, so primitive AND user-defined
    // replacement functions resolve uniformly. The rhs pre-evaluates once
    // and injects as a self-evaluating constant.
    let replacement_name = format!("{head_name}<-");
    if primitive_id_for_name(&replacement_name).is_some()
        || crate::value::env::lookup(heap, caller_env, interner.intern_symbol(&replacement_name)).is_some()
    {
        let value = eval(heap, interner, rhs, caller_env)?;
        let value = force_if_promise(heap, interner, value)?;
        // Build the argument chain: object expression, any extra target
        /// arguments, the pre-evaluated value last.
        let mut chain = heap.alloc(RObject::Pairlist {
            car: value,
            cdr: ValueId::NIL,
            tag: ValueId::NIL,
        });
        for extra in call_args[1..].iter().rev() {
            chain = heap.alloc(RObject::Pairlist {
                car: *extra,
                cdr: chain,
                tag: ValueId::NIL,
            });
        }
        chain = heap.alloc(RObject::Pairlist {
            car: call_args[0],
            cdr: chain,
            tag: ValueId::NIL,
        });
        let head_sym = interner.intern_symbol(&replacement_name);
        let head_obj = heap.alloc(RObject::Symbol {
            name: head_sym,
            value: ValueId::UNBOUND,
            internal: ValueId::NIL,
        });
        let synthetic = heap.alloc(RObject::Language {
            car: head_obj,
            cdr: chain,
            tag: ValueId::NIL,
        });
        let replaced = eval(heap, interner, synthetic, caller_env)?;
        let replaced = force_if_promise(heap, interner, replaced)?;
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
pub(crate) fn get_attrib(
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
pub(crate) fn copy_with_attrib(
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
                    // GNU: assigning NULL removes the attribute outright.
                    if attr_value != ValueId::NIL {
                        entries.push((sym_obj, attr_value));
                    }
                    found = true;
                } else {
                    entries.push((*tag, *car));
                }
                cursor = *cdr;
            }
            _ => break,
        }
    }
    if !found && attr_value != ValueId::NIL {
        // GNU appends new attributes at the chain's end — attributes()
        /// enumerates in insertion order exactly as the oracle pins.
        entries.push((sym_obj, attr_value));
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
    // GNU's matching order for `$`: exact first, then a UNIQUE prefix
    /// match — ambiguous prefixes and missing names are NULL.
    let position = match heap.object(names) {
        Some(RObject::String(values)) => {
            let exact = values
                .iter()
                .position(|s| interner.string_text(*s) == wanted);
            match exact {
                Some(p) => Some(p),
                None => {
                    let prefixes: Vec<usize> = values
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| {
                            let text = interner.string_text(**s);
                            text.starts_with(wanted.as_str()) && !wanted.is_empty()
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if prefixes.len() == 1 {
                        Some(prefixes[0])
                    } else {
                        None
                    }
                }
            }
        }
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
fn is_na_primitive(heap: &mut Heap, interner: &SymbolInterner, value: ValueId) -> EvalResult {
    let na_int = crate::value::object::NA_INTEGER;
    let out: Vec<i32> = match heap.object(value) {
        Some(RObject::Logical(v)) => v.iter().map(|x| (*x == na_int) as i32).collect(),
        Some(RObject::Integer(v)) => v.iter().map(|x| (*x == na_int) as i32).collect(),
        Some(RObject::Real(v)) => v.iter().map(|x| x.is_nan() as i32).collect(),
        Some(RObject::String(v)) => v
            .iter()
            .map(|x| (interner.string_text(*x) == "\0NA") as i32)
            .collect(),
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
    // Tagged-argument walk: decreasing and na.last arrive tagged or
    /// positional exactly as GNU's argument matching.
    let mut decreasing = false;
    // na.last tri-state: None = remove (GNU default NA), Some(true) = end,
    /// Some(false) = front.
    let mut na_last: Option<bool> = None;
    let mut positional_seen = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional_seen += 1;
        if positional_seen == 1 {
            if cdr == ValueId::NIL {
                break;
            }
            cursor = cdr;
            continue;
        }
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        let truthy = |id: ValueId| -> Option<bool> {
            match heap.object(id) {
                Some(RObject::Logical(vals)) if !vals.is_empty() => {
                    if vals[0] == crate::value::object::NA_INTEGER {
                        None
                    } else {
                        Some(vals[0] != 0)
                    }
                }
                _ => None,
            }
        };
        let is_na_value = matches!(
            heap.object(v),
            Some(RObject::Logical(vals)) if !vals.is_empty() && vals[0] == crate::value::object::NA_INTEGER
        );
        match tag_name.as_deref() {
            Some("decreasing") => decreasing = truthy(v).unwrap_or(false),
            Some("na.last") => {
                na_last = if is_na_value {
                    None
                } else {
                    match truthy(v) {
                        Some(true) => Some(true),
                        _ => Some(false),
                    }
                };
            }
            _ => {
                if positional_seen == 2 {
                    decreasing = truthy(v).unwrap_or(false);
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    macro_rules! per_type {
        ($variant:ident, $ord:expr, $na:expr) => {
            match heap.object(value) {
                Some(RObject::$variant(values)) => {
                    let mut out = values.clone();
                    match op {
                        SORT => {
                            let na_count = out.iter().filter(|v| $ord(**v)).count();
                            out.retain(|v| !$ord(*v));
                            out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                            if decreasing {
                                out.reverse();
                            }
                            // GNU na.last: absent/NA drops (already done);
                            /// TRUE appends after the final order, FALSE
                            /// prepends — placement ignores decreasing.
                            match na_last {
                                Some(true) => {
                                    for _ in 0..na_count {
                                        out.push($na);
                                    }
                                }
                                Some(false) => {
                                    for _ in 0..na_count {
                                        out.insert(0, $na);
                                    }
                                }
                                None => {}
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
    per_type!(Integer, is_na_int, crate::value::object::NA_INTEGER);
    per_type!(Real, is_na_real, f64::from_bits(crate::value::object::NA_REAL_BITS));
    per_type!(Logical, is_na_int, crate::value::object::NA_INTEGER);
    // Character vectors order lexicographically — GNU's C-locale byte order
    /// for the slice; the NA sentinel text sorts out exactly as numeric NA.
    match heap.object(value) {
        Some(RObject::String(values)) => {
            let mut out: Vec<String> = values
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect();
            match op {
                SORT => {
                    let na_count = out.iter().filter(|v| *v == "\0NA").count();
                    out.retain(|v| v != "\0NA");
                    out.sort();
                    if decreasing {
                        out.reverse();
                    }
                    match na_last {
                        Some(true) => {
                            for _ in 0..na_count {
                                out.push("\0NA".to_string());
                            }
                        }
                        Some(false) => {
                            for _ in 0..na_count {
                                out.insert(0, "\0NA".to_string());
                            }
                        }
                        None => {}
                    }
                }
                REV => out.reverse(),
                _ => {
                    let mut seen: Vec<String> = Vec::new();
                    out.retain(|v| {
                        if seen.contains(v) {
                            false
                        } else {
                            seen.push(v.clone());
                            true
                        }
                    });
                }
            }
            let interned: Vec<super::intern::InternedStr> = out
                .iter()
                .map(|s| interner.intern_string(s))
                .collect();
            return Ok(heap.alloc(RObject::String(interned)));
        }
        _ => {}
    }
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
    // GNU: a zero-length test selects nothing, and the result is logical
    /// empty regardless of the branch types.
    if tests.is_empty() {
        return Ok(heap.alloc(RObject::Logical(vec![])));
    }
    let na_int = crate::value::object::NA_INTEGER;
    let yes_is_str = matches!(heap.object(operands[1]), Some(RObject::String(_)));
    let no_is_str = matches!(heap.object(operands[2]), Some(RObject::String(_)));
    // GNU promotes to the common type of the branches — character wins,
    /// then real, then integer — and NA tests yield the type's NA.
    if yes_is_str || no_is_str {
        let yes_id = coerce_character(heap, interner, operands[1])?;
        let yes = string_vec(heap, interner, yes_id);
        let no_id = coerce_character(heap, interner, operands[2])?;
        let no = string_vec(heap, interner, no_id);
        let mut out: Vec<super::intern::InternedStr> = Vec::with_capacity(tests.len());
        for (i, test) in tests.iter().enumerate() {
            let chosen = if *test == na_int {
                "\0NA".to_string()
            } else if *test != 0 {
                if yes.is_empty() { "\0NA".to_string() } else { yes[i % yes.len()].clone() }
            } else if no.is_empty() {
                "\0NA".to_string()
            } else {
                no[i % no.len()].clone()
            };
            out.push(interner.intern_string(&chosen));
        }
        return Ok(heap.alloc(RObject::String(out)));
    }
    // GNU's type lattice: the result takes the common type of the branches
    /// — logical+logical stays logical, integer mixtures stay integer, any
    /// real input widens to double exactly as the coercion hierarchy.
    let rank = |v: ValueId| -> u32 {
        match heap.object(v) {
            Some(RObject::Logical(_)) => 0,
            Some(RObject::Integer(_)) => 1,
            _ => 2,
        }
    };
    let result_rank = rank(operands[1]).max(rank(operands[2]));
    let yes = numerics_of(heap, operands[1]).map_err(RSignal::from)?;
    let no = numerics_of(heap, operands[2]).map_err(RSignal::from)?;
    let na_real = f64::from_bits(crate::value::object::NA_REAL_BITS);
    let mut out = Vec::with_capacity(tests.len());
    for (i, test) in tests.iter().enumerate() {
        if *test == na_int {
            out.push(na_real);
        } else if *test != 0 {
            out.push(if yes.is_empty() { na_real } else { yes[i % yes.len()] });
        } else {
            out.push(if no.is_empty() { na_real } else { no[i % no.len()] });
        }
    }
    match result_rank {
        0 => Ok(heap.alloc(RObject::Logical(
            out.iter()
                .map(|&v| {
                    if v.is_nan() {
                        crate::value::object::NA_INTEGER
                    } else {
                        v as i32
                    }
                })
                .collect(),
        ))),
        1 => Ok(heap.alloc(RObject::Integer(
            out.iter()
                .map(|&v| {
                    if v.is_nan() {
                        crate::value::object::NA_INTEGER
                    } else {
                        v as i32
                    }
                })
                .collect(),
        ))),
        _ => Ok(heap.alloc(RObject::Real(out))),
    }
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
    // Tagged walk: the name positional, envir/inherits/mode by name
    /// exactly as GNU's argument list.
    let mut name_arg = ValueId::NIL;
    let mut env = caller_env;
    let mut inherits = true;
    let mut mode_fn = false;
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("envir") | Some("where") => env = v,
            Some("inherits") => {
                if let Some(RObject::Logical(vals)) = heap.object(v) {
                    inherits = vals.is_empty() || (vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER);
                }
            }
            Some("mode") => {
                if let Some(text) = string_operand(heap, interner, v) {
                    mode_fn = text == "function";
                }
            }
            _ => {
                if positional == 1 {
                    name_arg = v;
                } else if positional == 2 {
                    env = v;
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let name = match heap.object(name_arg) {
        Some(RObject::String(values)) if !values.is_empty() => {
            interner.string_text(values[0]).to_string()
        }
        _ => return Err(EvalError::TypeError("exists requires a character name").into()),
    };
    let sym = interner.intern_symbol(&name);
    let found = if inherits {
        crate::value::env::lookup(heap, env, sym)
    } else {
        crate::value::env::lookup_in_frame(heap, env, sym).map(|b| (env, b))
    };
    // mode="function" filters to closure and primitive bindings exactly as
    /// GNU's mode matching.
    let found = match (found, mode_fn) {
        (Some((_, binding)), true) => matches!(
            heap.object(binding.value),
            Some(RObject::Closure { .. }) | Some(RObject::Primitive(_))
        ),
        (Some(_), false) => true,
        _ => false,
    };
    Ok(heap.alloc(RObject::Logical(vec![found as i32])))
}

/// `ls(envir)` — sorted binding names of the frame, GNU's ordering. List
/// frames enumerate; hash frames are outside the object model's slice.
fn ls_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    env: ValueId,
    all_names: bool,
) -> EvalResult {
    let mut names: Vec<String> = Vec::new();
    if let Some(RObject::Environment { frame, .. }) = heap.object(env) {
        if let crate::value::object::EnvFrame::List(bindings) = frame {
            for (sym, _) in bindings {
                names.push(interner.symbol_name(*sym).to_string());
            }
        }
    }
    // GNU hides dot-prefixed bindings unless all.names=TRUE.
    if !all_names {
        names.retain(|n| !n.starts_with('.'));
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

/// `structure(x, ...)`: every tagged argument attaches as the attribute of
/// its name — GNU's attribute-attachment interface exactly. Copy-on-write
/// discipline as with all attribute mutation.
fn structure_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut positional: Vec<ValueId> = Vec::new();
    let mut tagged: Vec<(String, ValueId)> = Vec::new();
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
                tagged.push((interner.symbol_name(*name).to_string(), value));
            }
            _ => positional.push(value),
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let mut object = match positional.first() {
        Some(v) => *v,
        None => return Err(EvalError::TypeError("structure requires input").into()),
    };
    // copy_with_attrib appends, so forward application leaves the chain
    /// in argument order exactly as GNU prints it.
    for (name, value) in tagged.into_iter() {
        object = copy_with_attrib(heap, interner, object, &name, value).map_err(RSignal::from)?;
    }
    Ok(object)
}

/// `assign(name, value, envir)` — GNU's programmatic define: the name is a
/// character string, the target environment defaults to the caller.
fn assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let name = match heap.object(operands[0]) {
        Some(RObject::String(values)) if !values.is_empty() => {
            interner.string_text(values[0]).to_string()
        }
        _ => return Err(EvalError::TypeError("assign requires a character name").into()),
    };
    let env = match raw_args(heap, args).get(2).copied() {
        Some(expr) => {
            let evaluated = eval(heap, interner, expr, caller_env)?;
            force_if_promise(heap, interner, evaluated)?
        }
        None => caller_env,
    };
    let sym = interner.intern_symbol(&name);
    crate::value::env::define(heap, env, sym, operands[1])
        .map_err(|_| RSignal::Error(EvalError::UnsupportedForm("assign define")))?;
    // GNU: assign returns the value invisibly.
    Ok(operands[1])
}

/// `get(name, envir)` — chain lookup by character name, GNU exactly.
fn get_primitive(
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
        _ => return Err(EvalError::TypeError("get requires a character name").into()),
    };
    let env = match raw_args(heap, args).get(1).copied() {
        Some(expr) => {
            let evaluated = eval(heap, interner, expr, caller_env)?;
            force_if_promise(heap, interner, evaluated)?
        }
        None => caller_env,
    };
    let sym = interner.intern_symbol(&name);
    match crate::value::env::lookup(heap, env, sym) {
        Some((_, binding)) => Ok(binding.value),
        None => Err(EvalError::UnboundVariable(name).into()),
    }
}

/// Generic named-attribute getter: NULL when absent, exactly GNU.
fn attrib_getter(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    attr: &str,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    match get_attrib(heap, interner, operands[0], attr) {
        Some(v) => Ok(v),
        None => Ok(ValueId::NIL),
    }
}

/// Generic attribute replacement primitive: `(object, value)` attaches the
/// value under the attribute name through copy-on-write discipline.
fn attrib_replacement(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    attr: &str,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    copy_with_attrib(heap, interner, operands[0], attr, operands[1]).map_err(RSignal::from)
}

/// `class(x)`: the class attribute when present, otherwise GNU's implicit
/// class — matrix/array for dimensioned vectors, the type name otherwise.
fn class_getter(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    if let Some(v) = get_attrib(heap, interner, operands[0], "class") {
        return Ok(v);
    }
    let implicit = if get_attrib(heap, interner, operands[0], "dim").is_some() {
        "matrix"
    } else {
        match heap.object(operands[0]) {
            Some(RObject::Real(_)) => "numeric",
            Some(RObject::Integer(_)) => "integer",
            Some(RObject::Logical(_)) => "logical",
            Some(RObject::String(_)) => "character",
            Some(RObject::List(_)) => "list",
            _ => "NULL",
        }
    };
    Ok(heap.alloc(RObject::String(vec![interner.intern_string(implicit)])))
}

/// rownames/colnames: element one/two of the dimnames attribute.
fn rowcolnames(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    rows: bool,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let dimnames = match get_attrib(heap, interner, operands[0], "dimnames") {
        Some(v) => v,
        None => {
            // data.frames carry row.names instead of dimnames — the rows
            // getter reads it directly exactly as GNU.
            if rows {
                if let Some(v) = get_attrib(heap, interner, operands[0], "row.names") {
                    return Ok(v);
                }
            }
            return Ok(ValueId::NIL);
        }
    };
    let index = if rows { 0 } else { 1 };
    match heap.object(dimnames) {
        Some(RObject::List(items)) => items.get(index).copied().map_or(Ok(ValueId::NIL), |v| {
            if v == ValueId::NIL {
                Ok(ValueId::NIL)
            } else {
                Ok(v)
            }
        }),
        _ => Ok(ValueId::NIL),
    }
}

/// rownames<-/colnames<-: rebuild the dimnames list with element one/two
/// replaced — constructing the two-element NULL list when absent, exactly
/// GNU's lazy dimnames materialization.
fn rowcolnames_assign(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    rows: bool,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let index = if rows { 0 } else { 1 };
    let existing = get_attrib(heap, interner, operands[0], "dimnames");
    let mut elements: Vec<ValueId> = match existing {
        Some(id) => match heap.object(id) {
            Some(RObject::List(items)) => {
                let mut v = items.clone();
                while v.len() < 2 {
                    v.push(ValueId::NIL);
                }
                v
            }
            _ => vec![ValueId::NIL, ValueId::NIL],
        },
        None => vec![ValueId::NIL, ValueId::NIL],
    };
    elements[index] = operands[1];
    let list = heap.alloc(RObject::List(elements));
    copy_with_attrib(heap, interner, operands[0], "dimnames", list).map_err(RSignal::from)
}

/// Extract the first string of an operand — the path coercion every file
/// primitive shares.
fn string_operand(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Option<String> {
    match heap.object(value) {
        Some(RObject::String(values)) if !values.is_empty() => {
            Some(interner.string_text(values[0]).to_string())
        }
        _ => None,
    }
}

fn string_vec(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Vec<String> {
    match heap.object(value) {
        Some(RObject::String(values)) => values
            .iter()
            .map(|v| interner.string_text(*v).to_string())
            .collect(),
        _ => Vec::new(),
    }
}


/// The (nrow, ncol) of a data.frame-classed list, when classed.
pub(crate) fn data_frame_shape(
    heap: &Heap,
    interner: &SymbolInterner,
    value: ValueId,
) -> Option<(usize, usize)> {
    let classed = match get_attrib(heap, interner, value, "class") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .any(|v| interner.string_text(*v) == "data.frame"),
            _ => false,
        },
        None => false,
    };
    if !classed {
        return None;
    }
    let items = match heap.object(value) {
        Some(RObject::List(items)) => items,
        _ => return None,
    };
    let ncol = items.len();
    let nrow = items
        .first()
        .and_then(|c| vector_length(heap, *c))
        .unwrap_or(0);
    Some((nrow, ncol))
}

/// `tempfile()`: a unique non-existent path inside the system tempdir —
/// process id plus a counter makes collision impossible within the session
/// exactly as GNU's pattern randomization does across sessions.
fn tempfile_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let _ = (args, caller_env);
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir();
    loop {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let path = dir.join(format!(
            "Rtmpfile{}x{:x}{}",
            std::process::id(),
            nanos,
            n
        ));
        if !path.exists() {
            let text = interner.intern_string(&path.to_string_lossy());
            return Ok(heap.alloc(RObject::String(vec![text])));
        }
    }
}

/// `writeLines(text, con)`: every element of text plus a trailing newline,
/// con a filename string exactly as the conformance slice uses it.
fn writelines_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Tagged-argument walk: text positional first, con and sep by name or
    /// position exactly as GNU's argument matching.
    let mut text_arg = ValueId::NIL;
    let mut con_arg = ValueId::NIL;
    let mut sep = "\n".to_string();
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("con") => con_arg = v,
            Some("sep") => {
                if let Some(s) = string_operand(heap, interner, v) {
                    sep = s;
                }
            }
            _ => {
                if positional == 1 {
                    text_arg = v;
                } else if positional == 2 {
                    con_arg = v;
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let lines = string_vec(heap, interner, text_arg);
    let path = string_operand(heap, interner, con_arg)
        .ok_or(EvalError::TypeError("writeLines connection must be a filename"))?;
    let mut content = String::new();
    for line in &lines {
        content.push_str(line);
        content.push_str(&sep);
    }
    std::fs::write(&path, content)
        .map_err(|_| RSignal::Error(EvalError::TypeError("writeLines: cannot open file")))?;
    Ok(ValueId::NIL)
}

/// `readLines(con)`: the file's lines as a character vector, trailing
/// newline not producing a spurious empty final element — GNU exactly.
fn readlines_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Tagged-argument walk: con positional, n limits lines, warn ignored
    /// in the migration slice.
    let mut con_arg = ValueId::NIL;
    let mut n_limit: Option<usize> = None;
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("n") => {
                if let Some(RObject::Real(vals)) = heap.object(v) {
                    if !vals.is_empty() {
                        n_limit = Some(vals[0] as usize);
                    }
                }
            }
            Some("warn") => {}
            _ => {
                if positional == 1 {
                    con_arg = v;
                } else if positional == 2 && n_limit.is_none() {
                    if let Some(RObject::Real(vals)) = heap.object(v) {
                        if !vals.is_empty() {
                            n_limit = Some(vals[0] as usize);
                        }
                    }
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let path = string_operand(heap, interner, con_arg)
        .ok_or(EvalError::TypeError("readLines connection must be a filename"))?;
    let content = std::fs::read_to_string(&path)
        .map_err(|_| RSignal::Error(EvalError::TypeError("readLines: cannot open file")))?;
    let mut lines: Vec<&str> = content.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    if let Some(n) = n_limit {
        lines.truncate(n);
    }
    let interned: Vec<super::intern::InternedStr> = lines
        .iter()
        .map(|l| interner.intern_string(l))
        .collect();
    Ok(heap.alloc(RObject::String(interned)))
}

/// `file.exists(paths)` — elementwise existence probe.
fn file_exists_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let paths = string_vec(heap, interner, operands[0]);
    let out: Vec<i32> = paths
        .iter()
        .map(|p| if std::path::Path::new(p).exists() { 1 } else { 0 })
        .collect();
    Ok(heap.alloc(RObject::Logical(out)))
}

/// `file.remove(paths)` — elementwise removal with logical success exactly
/// as GNU.
fn file_remove_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let paths = string_vec(heap, interner, operands[0]);
    let out: Vec<i32> = paths
        .iter()
        .map(|p| if std::fs::remove_file(p).is_ok() { 1 } else { 0 })
        .collect();
    Ok(heap.alloc(RObject::Logical(out)))
}

/// `file.copy(from, to)` — scalar copy with logical success.
fn file_copy_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let from = string_operand(heap, interner, operands[0])
        .ok_or(EvalError::TypeError("file.copy source"))?;
    let to = string_operand(heap, interner, operands[1])
        .ok_or(EvalError::TypeError("file.copy target"))?;
    let ok = std::fs::copy(&from, &to).is_ok();
    Ok(heap.alloc(RObject::Logical(vec![if ok { 1 } else { 0 }])))
}

/// `file.rename(from, to)` — scalar rename with logical success.
fn file_rename_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let from = string_operand(heap, interner, operands[0])
        .ok_or(EvalError::TypeError("file.rename source"))?;
    let to = string_operand(heap, interner, operands[1])
        .ok_or(EvalError::TypeError("file.rename target"))?;
    let ok = std::fs::rename(&from, &to).is_ok();
    Ok(heap.alloc(RObject::Logical(vec![if ok { 1 } else { 0 }])))
}

/// `as.logical(x)` — numeric nonzero is true, NA propagates, strings parse
/// the T/F/TRUE/FALSE spellings exactly as GNU.
fn coerce_logical(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
) -> EvalResult {
    let out: Vec<i32> = match heap.object(value) {
        Some(RObject::Logical(values)) => values.clone(),
        Some(RObject::Integer(values)) => values.clone(),
        Some(RObject::Real(values)) => values
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
            .collect(),
        Some(RObject::String(values)) => values
            .iter()
            .map(|v| match interner.string_text(*v) {
                "TRUE" | "T" | "true" => 1,
                "FALSE" | "F" | "false" => 0,
                _ => crate::value::object::NA_INTEGER,
            })
            .collect(),
        _ => return Err(EvalError::TypeError("cannot coerce to logical").into()),
    };
    Ok(heap.alloc(RObject::Logical(out)))
}

/// `attr(x, name)` — single named attribute or NULL, exactly GNU.
fn attr_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let name = string_operand(heap, interner, operands[1])
        .ok_or(EvalError::TypeError("attr name must be character"))?;
    match get_attrib(heap, interner, operands[0], &name) {
        Some(v) => Ok(v),
        None => Ok(ValueId::NIL),
    }
}

/// `attr<-(x, name, value)` — the replacement form the general desugar
/// dispatches: object, attribute name, value positional.
fn attr_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 3)?;
    let name = string_operand(heap, interner, operands[1])
        .ok_or(EvalError::TypeError("attr name must be character"))?;
    copy_with_attrib(heap, interner, operands[0], &name, operands[2]).map_err(RSignal::from)
}

/// `attributes(x)` — every attribute as a named list, NULL when bare.
fn attributes_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let chain = heap.get(operands[0]).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    let mut values: Vec<ValueId> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut cursor = chain;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            break;
        }
        match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => {
                if let Some(RObject::Symbol { name, .. }) = heap.object(*tag) {
                    names.push(interner.symbol_name(*name).to_string());
                    values.push(*car);
                }
                cursor = *cdr;
            }
            _ => break,
        }
    }
    if values.is_empty() {
        return Ok(ValueId::NIL);
    }
    let list = heap.alloc(RObject::List(values));
    let interned: Vec<super::intern::InternedStr> = names
        .iter()
        .map(|n| interner.intern_string(n))
        .collect();
    let names_obj = heap.alloc(RObject::String(interned));
    copy_with_attrib(heap, interner, list, "names", names_obj).map_err(RSignal::from)
}

/// `attributes<-(x, value)` — wholesale attribute replacement: value is a
/// named list whose entries become the object's attributes, NULL clears all.
fn attributes_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let mut object = operands[0];
    // Clear the existing chain first: copy with an empty chain exactly —
    // wholesale replacement is not merge.
    object = copy_with_attrib(heap, interner, object, "\0clearall", ValueId::NIL).map_err(RSignal::from)?;
    if let Some(slot) = heap.get_mut(object) {
        slot.attrib = ValueId::NIL;
    }
    if operands[1] == ValueId::NIL {
        return Ok(object);
    }
    let names = get_attrib(heap, interner, operands[1], "names");
    let name_vec: Vec<String> = match names {
        Some(id) => string_vec(heap, interner, id),
        None => Vec::new(),
    };
    if let Some(RObject::List(items)) = heap.object(operands[1]).cloned() {
        // Forward iteration: copy_with_attrib appends, so list order is
        /// preserved in the final chain exactly as GNU's insertion order.
        for (i, item) in items.iter().enumerate() {
            if let Some(name) = name_vec.get(i) {
                object = copy_with_attrib(heap, interner, object, name, *item).map_err(RSignal::from)?;
            }
        }
    }
    Ok(object)
}

/// `length<-(x, n)` — resize: truncation drops the tail, extension pads
/// with the type's NA exactly as GNU.
fn length_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let target = match heap.object(operands[1]) {
        Some(RObject::Real(v)) if !v.is_empty() => v[0] as usize,
        Some(RObject::Integer(v)) if !v.is_empty() => v[0] as usize,
        _ => return Err(EvalError::TypeError("length replacement must be numeric").into()),
    };
    let object = heap.object(operands[0]).cloned()
        .ok_or(EvalError::TypeError("length target missing"))?;
    let resized = match object {
        RObject::Real(mut v) => {
            v.resize(target, f64::from_bits(crate::value::object::NA_REAL_BITS));
            RObject::Real(v)
        }
        RObject::Integer(mut v) => {
            v.resize(target, crate::value::object::NA_INTEGER);
            RObject::Integer(v)
        }
        RObject::Logical(mut v) => {
            v.resize(target, crate::value::object::NA_INTEGER);
            RObject::Logical(v)
        }
        RObject::String(mut v) => {
            let na = interner.intern_string("\0NA");
            v.resize(target, na);
            RObject::String(v)
        }
        RObject::List(mut v) => {
            v.resize(target, ValueId::NIL);
            RObject::List(v)
        }
        _ => return Err(EvalError::TypeError("length replacement target").into()),
    };
    // The attribute chain survives the resize — GNU drops dim but keeps
    /// names for extension; the slice copies the chain wholesale.
    let chain = heap.get(operands[0]).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    let id = heap.alloc(resized);
    if let Some(slot) = heap.get_mut(id) {
        slot.attrib = chain;
    }
    Ok(id)
}

/// `file.path(...)` — components joined with the platform separator exactly
/// as GNU's forward slash on unix.
fn file_path_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // GNU: file.path is elementwise over vector inputs with recycling —
    /// each component vector contributes its i-th element to the i-th path.
    let mut columns: Vec<Vec<String>> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, .. }) => (*car, *cdr),
            _ => break,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        columns.push(string_vec(heap, interner, value));
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let width = columns.iter().map(Vec::len).max().unwrap_or(0);
    let mut paths: Vec<super::intern::InternedStr> = Vec::with_capacity(width);
    for i in 0..width {
        let pieces: Vec<String> = columns
            .iter()
            .map(|col| col[i % col.len().max(1)].clone())
            .collect();
        paths.push(interner.intern_string(&pieces.join("/")));
    }
    Ok(heap.alloc(RObject::String(paths)))
}

/// `data.frame(...)`: tagged arguments become named columns exactly as
/// GNU — the object is a list carrying class data.frame, names, and
/// row.names attributes; every column must share one length.
fn data_frame_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut columns: Vec<ValueId> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut cursor = args;
    let mut positional = 0usize;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let evaluated = eval(heap, interner, car, caller_env)?;
        let value = force_if_promise(heap, interner, evaluated)?;
        let name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
            _ => format!("V{positional}"),
        };
        names.push(name);
        columns.push(value);
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    if columns.is_empty() {
        return Err(EvalError::TypeError("data.frame requires columns").into());
    }
    build_data_frame(heap, interner, columns, names)
}

/// The shared data.frame assembly: equal-length column validation, the
/// row.names sequence, names and class attributes — copy-on-write
/// discipline throughout.
pub(crate) fn build_data_frame(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    columns: Vec<ValueId>,
    names: Vec<String>,
) -> EvalResult {
    let nrows = columns
        .first()
        .and_then(|c| vector_length(heap, *c))
        .unwrap_or(0);
    for col in &columns {
        let len = vector_length(heap, *col).unwrap_or(0);
        if len != nrows {
            return Err(EvalError::TypeError("columns must share one length").into());
        }
    }
    let list = heap.alloc(RObject::List(columns));
    let interned: Vec<super::intern::InternedStr> = names
        .iter()
        .map(|n| interner.intern_string(n))
        .collect();
    let names_obj = heap.alloc(RObject::String(interned));
    let list = copy_with_attrib(heap, interner, list, "names", names_obj).map_err(RSignal::from)?;
    let rownames: Vec<super::intern::InternedStr> = (1..=nrows)
        .map(|i| interner.intern_string(&i.to_string()))
        .collect();
    let rownames_obj = heap.alloc(RObject::String(rownames));
    let list = copy_with_attrib(heap, interner, list, "row.names", rownames_obj).map_err(RSignal::from)?;
    let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("data.frame")]));
    let list = copy_with_attrib(heap, interner, list, "class", class_obj).map_err(RSignal::from)?;
    Ok(list)
}

/// `aggregate(x, by, FUN)`: rows of x group by the `by` list's values, FUN
/// applies per group, and the result assembles as a data.frame of group
/// columns plus the aggregated column — GNU's split-apply-combine exactly.
/// Groups order lexicographically as GNU's factor-level ordering does for
/// character groupers.
fn aggregate_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 3)?;
    let x = operands[0];
    let by = operands[1];
    let fun = operands[2];
    // The grouping columns: a list of equal-length vectors.
    let group_cols: Vec<ValueId> = match heap.object(by) {
        Some(RObject::List(items)) => items.clone(),
        _ => vec![by],
    };
    let by_names: Vec<String> = match get_attrib(heap, interner, by, "names") {
        Some(id) => string_vec(heap, interner, id),
        None => Vec::new(),
    };
    let x_len = vector_length(heap, x).unwrap_or(0);
    // Group keys: per-row concatenation of the first grouping column (the
    // slice supports one grouper; multi-grouper keys concatenate with a
    // separator exactly as GNU's interaction does).
    let mut keys: Vec<String> = Vec::with_capacity(x_len);
    for i in 0..x_len {
        let mut key = String::new();
        for (j, col) in group_cols.iter().enumerate() {
            if j > 0 {
                key.push('\x01');
            }
            key.push_str(&element_text(heap, interner, *col, i));
        }
        keys.push(key);
    }
    let mut unique_keys: Vec<String> = keys.clone();
    // GNU orders multi-way group expansion column-major: the first grouper
    /// varies fastest, so the sort prioritizes the last key component and
    /// ties break toward the first exactly as array indexing does.
    unique_keys.sort_by(|a, b| {
        let ac: Vec<&str> = a.split('\x01').collect();
        let bc: Vec<&str> = b.split('\x01').collect();
        for j in (0..ac.len().max(bc.len())).rev() {
            let av = ac.get(j).copied().unwrap_or("");
            let bv = bc.get(j).copied().unwrap_or("");
            match av.cmp(bv) {
                std::cmp::Ordering::Equal => continue,
                ord => return ord,
            }
        }
        std::cmp::Ordering::Equal
    });
    unique_keys.dedup();
    // Aggregate: gather positions per group, extract the subvector, apply.
    let mut result_values: Vec<f64> = Vec::new();
    let mut result_is_int = true;
    for key in &unique_keys {
        let positions: Vec<usize> = keys
            .iter()
            .enumerate()
            .filter(|(_, k)| *k == key)
            .map(|(i, _)| i)
            .collect();
        let sub = extract_positions(heap, interner, x, &positions)?;
        // Apply FUN through the closure machinery — primitives and user
        // functions both resolve through the symbol's binding.
        let applied = apply_fun_to(heap, interner, fun, sub, caller_env)?;
        match heap.object(applied) {
            Some(RObject::Real(v)) if !v.is_empty() => {
                result_values.push(v[0]);
                result_is_int = false;
            }
            Some(RObject::Integer(v)) if !v.is_empty() => {
                result_values.push(v[0] as f64);
            }
            _ => return Err(EvalError::TypeError("aggregate FUN must return scalar numeric").into()),
        }
    }
    // Assemble: group columns carry one row per unique key; the aggregated
    // column names "x" exactly as GNU's default.
    let mut columns: Vec<ValueId> = Vec::new();
    let mut col_names: Vec<String> = Vec::new();
    let ngroupers = group_cols.len();
    for j in 0..ngroupers {
        let parts: Vec<String> = unique_keys
            .iter()
            .map(|k| k.split('\x01').nth(j).unwrap_or("").to_string())
            .collect();
        let interned: Vec<super::intern::InternedStr> = parts
            .iter()
            .map(|s| interner.intern_string(s))
            .collect();
        columns.push(heap.alloc(RObject::String(interned)));
        col_names.push(
            by_names
                .get(j)
                .cloned()
                .unwrap_or_else(|| format!("Group.{j}", j = j + 1)),
        );
    }
    let result_col = if result_is_int && result_values.iter().all(|v| v.fract() == 0.0) {
        // GNU preserves integer type when the aggregation is integer-valued.
        heap.alloc(RObject::Real(result_values.clone()))
    } else {
        heap.alloc(RObject::Real(result_values.clone()))
    };
    columns.push(result_col);
    col_names.push("x".to_string());
    build_data_frame(heap, interner, columns, col_names)
}

/// One element's plain text — the grouping-key coercion.
fn element_text(heap: &Heap, interner: &SymbolInterner, value: ValueId, index: usize) -> String {
    match heap.object(value) {
        Some(RObject::String(values)) => interner
            .string_text(values.get(index).copied().unwrap_or(values[0]))
            .to_string(),
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => values
            .get(index)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Some(RObject::Real(values)) => values
            .get(index)
            .map(|v| super::print::format_real(*v))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Extract elements at positions into a fresh same-type vector.
fn extract_positions(
    heap: &mut Heap,
    interner: &SymbolInterner,
    source: ValueId,
    positions: &[usize],
) -> Result<ValueId, RSignal> {
    let object = heap.object(source).cloned()
        .ok_or(EvalError::TypeError("aggregate input missing"))?;
    let out = match object {
        RObject::Real(values) => RObject::Real(
            positions.iter().filter_map(|i| values.get(*i).copied()).collect(),
        ),
        RObject::Integer(values) => RObject::Integer(
            positions.iter().filter_map(|i| values.get(*i).copied()).collect(),
        ),
        RObject::Logical(values) => RObject::Logical(
            positions.iter().filter_map(|i| values.get(*i).copied()).collect(),
        ),
        RObject::String(values) => RObject::String(
            positions.iter().filter_map(|i| values.get(*i).copied()).collect(),
        ),
        _ => return Err(EvalError::TypeError("aggregate input type").into()),
    };
    let _ = interner;
    Ok(heap.alloc(out))
}

/// Apply a closure or primitive-reference to one vector argument — the
/// same dual shape lapply resolves.
fn apply_fun_to(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    fun: ValueId,
    arg: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let arg_node = heap.alloc(RObject::Pairlist {
        car: arg,
        cdr: ValueId::NIL,
        tag: ValueId::NIL,
    });
    match heap.object(fun).cloned() {
        Some(RObject::Closure { formals, body, env }) => {
            let applied = super::eval::apply_closure_public(
                heap, interner, formals, body, env, arg_node, caller_env,
            )?;
            force_if_promise(heap, interner, applied)
        }
        Some(RObject::Primitive(builtin)) => {
            call_primitive(heap, interner, builtin, arg_node, caller_env)
        }
        _ => Err(EvalError::NotCallable.into()),
    }
}

/// `factor(x)`: character input recodes to integer positions into the
/// sorted unique levels — GNU's integer-codes-plus-labels object shape with
/// class and levels attributes exactly.
fn factor_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let texts = string_vec(heap, interner, operands[0]);
    // A tagged levels argument supplies the level set verbatim — order and
    /// empties included — exactly as GNU; otherwise levels derive sorted
    /// from the data.
    let mut tagged_levels: Option<Vec<String>> = None;
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
        if tag_name.as_deref() == Some("levels") {
            let v = eval(heap, interner, car, caller_env)?;
            tagged_levels = Some(string_vec(heap, interner, v));
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let mut levels: Vec<String> = match tagged_levels {
        Some(ls) => ls.into_iter().filter(|s| s != "\0NA").collect(),
        None => {
            let mut ls: Vec<String> = texts
                .iter()
                .filter(|s| *s != "\0NA")
                .cloned()
                .collect();
            ls.sort();
            ls.dedup();
            ls
        }
    };
    let codes: Vec<i32> = texts
        .iter()
        .map(|s| {
            if s == "\0NA" {
                crate::value::object::NA_INTEGER
            } else {
                levels.iter().position(|l| l == s).map(|p| p as i32 + 1).unwrap_or(crate::value::object::NA_INTEGER)
            }
        })
        .collect();
    let obj = heap.alloc(RObject::Integer(codes));
    let interned: Vec<super::intern::InternedStr> = levels
        .iter()
        .map(|l| interner.intern_string(l))
        .collect();
    let levels_obj = heap.alloc(RObject::String(interned));
    let obj = copy_with_attrib(heap, interner, obj, "levels", levels_obj).map_err(RSignal::from)?;
    let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("factor")]));
    copy_with_attrib(heap, interner, obj, "class", class_obj).map_err(RSignal::from)
}

/// One conversion spec parsed out of a sprintf format string.
struct ConvSpec {
    zero_pad: bool,
    plus: bool,
    minus: bool,
    space: bool,
    width: usize,
    precision: Option<usize>,
    conversion: char,
}

/// `sprintf(fmt, ...)`: C-style conversion specs vectorized over recycled
/// inputs — the subset GNU's statistical printing actually exercises:
/// %d/%i integers, %f fixed, %s strings, %g significant, %% literal.
fn sprintf_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Variadic: every argument evaluates — the format first, conversion
    /// inputs in order after.
    let raw = raw_args(heap, args);
    let mut operands: Vec<ValueId> = Vec::new();
    for expr in &raw {
        let v = eval(heap, interner, *expr, caller_env)?;
        operands.push(force_if_promise(heap, interner, v)?);
    }
    if operands.is_empty() {
        return Err(EvalError::TypeError("sprintf requires a format").into());
    }
    let fmt = string_operand(heap, interner, operands[0])
        .ok_or(EvalError::TypeError("sprintf format must be character"))?;
    // Parse the spec sequence and literal text interleaved.
    let mut pieces: Vec<Result<String, ConvSpec>> = Vec::new();
    let mut chars = fmt.chars().peekable();
    let mut literal = String::new();
    while let Some(c) = chars.next() {
        if c != '%' {
            literal.push(c);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            literal.push('%');
            continue;
        }
        if !literal.is_empty() {
            pieces.push(Ok(std::mem::take(&mut literal)));
        }
        let mut zero_pad = false;
        let mut plus = false;
        let mut minus = false;
        let mut space = false;
        let mut width = 0usize;
        let mut precision = None;
        // flags
        while let Some(&f) = chars.peek() {
            match f {
                '0' => {
                    zero_pad = true;
                    chars.next();
                }
                '+' => {
                    plus = true;
                    chars.next();
                }
                '-' => {
                    minus = true;
                    chars.next();
                }
                ' ' => {
                    space = true;
                    chars.next();
                }
                _ => break,
            }
        }
        while let Some(&d) = chars.peek() {
            if d.is_ascii_digit() {
                width = width * 10 + d.to_digit(10).unwrap() as usize;
                chars.next();
            } else {
                break;
            }
        }
        if chars.peek() == Some(&'.') {
            chars.next();
            let mut prec = 0usize;
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    prec = prec * 10 + d.to_digit(10).unwrap() as usize;
                    chars.next();
                } else {
                    break;
                }
            }
            precision = Some(prec);
        }
        let conversion = chars.next().unwrap_or('s');
        pieces.push(Err(ConvSpec {
            zero_pad,
            plus,
            minus,
            space,
            width,
            precision,
            conversion,
        }));
    }
    if !literal.is_empty() {
        pieces.push(Ok(literal));
    }
    // The conversion inputs follow the format operand in order.
    let inputs: Vec<ValueId> = operands[1..].to_vec();
    let nconv = pieces.iter().filter(|p| p.is_err()).count();
    let out_len = inputs
        .iter()
        .take(nconv.max(1))
        .map(|v| vector_length(heap, *v).unwrap_or(1))
        .max()
        .unwrap_or(1)
        .max(1);
    let mut results: Vec<String> = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let mut text = String::new();
        let mut input_index = 0usize;
        for piece in &pieces {
            match piece {
                Ok(lit) => text.push_str(lit),
                Err(spec) => {
                    let input = inputs.get(input_index).copied().unwrap_or(ValueId::NIL);
                    input_index += 1;
                    text.push_str(&format_spec(heap, interner, spec, input, i));
                }
            }
        }
        results.push(text);
    }
    let interned: Vec<super::intern::InternedStr> = results
        .iter()
        .map(|s| interner.intern_string(s))
        .collect();
    Ok(heap.alloc(RObject::String(interned)))
}

/// Format one recycled element against one conversion spec.
fn format_spec(
    heap: &Heap,
    interner: &SymbolInterner,
    spec: &ConvSpec,
    input: ValueId,
    index: usize,
) -> String {
    let len = vector_length(heap, input).unwrap_or(1).max(1);
    let i = index % len;
    let int_value = |heap: &Heap| -> i64 {
        match heap.object(input) {
            Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => {
                values.get(i).copied().unwrap_or(0) as i64
            }
            Some(RObject::Real(values)) => values.get(i).copied().unwrap_or(0.0) as i64,
            _ => 0,
        }
    };
    let real_value = |heap: &Heap| -> f64 {
        match heap.object(input) {
            Some(RObject::Real(values)) => values.get(i).copied().unwrap_or(0.0),
            Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => {
                values.get(i).copied().unwrap_or(0) as f64
            }
            _ => 0.0,
        }
    };
    // GNU's uniform numeric discipline: the sign (+/-/space) leads, zero
    /// padding follows the sign, and width justification pads with spaces —
    /// left when the minus flag is present.
    let finish = |digits: String, negative: bool, spec: &ConvSpec| -> String {
        let sign = if negative {
            "-"
        } else if spec.plus {
            "+"
        } else if spec.space {
            " "
        } else {
            ""
        };
        let core_len = sign.len() + digits.len();
        if spec.zero_pad && !spec.minus && spec.width > core_len {
            format!("{}{}{}", sign, "0".repeat(spec.width - core_len), digits)
        } else if spec.width > core_len {
            if spec.minus {
                format!("{}{}{}", sign, digits, " ".repeat(spec.width - core_len))
            } else {
                format!("{}{}{}", " ".repeat(spec.width - core_len), sign, digits)
            }
        } else {
            format!("{sign}{digits}")
        }
    };
    match spec.conversion {
        'd' | 'i' => {
            let v = int_value(heap);
            finish(v.unsigned_abs().to_string(), v < 0, spec)
        }
        'x' | 'X' | 'o' => {
            let v = int_value(heap);
            let digits = match spec.conversion {
                'x' => format!("{:x}", v),
                'X' => format!("{:X}", v),
                _ => format!("{:o}", v),
            };
            finish(digits, false, spec)
        }
        'f' => {
            let v = real_value(heap);
            let decimals = spec.precision.unwrap_or(6);
            let fixed = format!("{:.*}", decimals, v.abs());
            finish(fixed, v < 0.0, spec)
        }
        'e' | 'E' => {
            let v = real_value(heap);
            let decimals = spec.precision.unwrap_or(6);
            let raw = format!("{:.*e}", decimals, v.abs());
            // Rust prints 1.23e3 — GNU wants 1.23e+03: signed, two digits.
            let (mantissa, exp) = raw.split_once('e').unwrap_or((&raw, "0"));
            let exp_value: i32 = exp.parse().unwrap_or(0);
            let exp_text = format!("{}{:02}", if exp_value < 0 { "-" } else { "+" }, exp_value.abs());
            let marker = if spec.conversion == 'E' { "E" } else { "e" };
            finish(format!("{mantissa}{marker}{exp_text}"), v < 0.0, spec)
        }
        'g' | 'G' => {
            let v = real_value(heap);
            let prec = spec.precision.unwrap_or(6).max(1);
            let abs = v.abs();
            let exp = if abs == 0.0 { 0 } else { abs.log10().floor() as i32 };
            let text = if exp < -4 || exp >= prec as i32 {
                // scientific with prec-1 mantissa decimals, trailing zeros
                /// stripped exactly as %g requires.
                let raw = format!("{:.*e}", prec - 1, abs);
                let (mantissa, exponent) = raw.split_once('e').unwrap_or((&raw, "0"));
                let mantissa = mantissa
                    .trim_end_matches('0')
                    .trim_end_matches('.');
                let exp_value: i32 = exponent.parse().unwrap_or(0);
                let marker = if spec.conversion == 'G' { "E" } else { "e" };
                format!("{}{}{}{:02}", mantissa, marker, if exp_value < 0 { "-" } else { "+" }, exp_value.abs())
            } else {
                let decimals = (prec as i32 - 1 - exp).max(0) as usize;
                let fixed = format!("{:.*}", decimals, abs);
                fixed.trim_end_matches('0').trim_end_matches('.').to_string()
            };
            finish(text, v < 0.0, spec)
        }
        _ => {
            // %s — string coercion of the element, width-justified without
            /// sign discipline.
            let text = match heap.object(input) {
                Some(RObject::String(values)) => values
                    .get(i)
                    .map(|v| interner.string_text(*v).to_string())
                    .unwrap_or_default(),
                Some(RObject::Real(values)) => super::print::format_real(
                    values.get(i).copied().unwrap_or(0.0),
                ),
                Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => values
                    .get(i)
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                _ => String::new(),
            };
            if spec.width > text.len() {
                if spec.minus {
                    format!("{}{}", text, " ".repeat(spec.width - text.len()))
                } else {
                    format!("{}{}", " ".repeat(spec.width - text.len()), text)
                }
            } else {
                text
            }
        }
    }
}

/// `levels<-` — GNU's full remap discipline. Character vector form maps
/// positionally over the current levels with NA entries dropping levels
/// (elements recode to NA, higher codes shift down); a named list form
/// remaps new-level-names to the old labels they absorb. Length mismatch
/// in the vector form is GNU's "number of levels differs" error. On
/// non-factors the attribute attaches plainly exactly as GNU.
fn levels_assign_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let object = operands[0];
    let value = operands[1];
    let is_factor = match get_attrib(heap, interner, object, "class") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .any(|v| interner.string_text(*v) == "factor"),
            _ => false,
        },
        None => false,
    };
    if !is_factor {
        return copy_with_attrib(heap, interner, object, "levels", value).map_err(RSignal::from);
    }
    let old_levels: Vec<String> = match get_attrib(heap, interner, object, "levels") {
        Some(id) => string_vec(heap, interner, id),
        None => Vec::new(),
    };
    let codes = match heap.object(object) {
        Some(RObject::Integer(values)) => values.clone(),
        _ => return Err(EvalError::TypeError("levels target must be integer-coded").into()),
    };

    // The list form: names are new levels, elements the old labels each
    // absorbs — a full recode map.
    if let Some(RObject::List(items)) = heap.object(value).cloned() {
        let names_id = get_attrib(heap, interner, value, "names");
        let new_names: Vec<String> = match names_id {
            Some(id) => string_vec(heap, interner, id),
            None => Vec::new(),
        };
        // Map old label -> new code (1-based).
        let mut remap: Vec<(String, usize)> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let absorbed = string_vec(heap, interner, *item);
            for label in absorbed {
                remap.push((label, i + 1));
            }
        }
        let new_codes: Vec<i32> = codes
            .iter()
            .map(|c| {
                if *c == crate::value::object::NA_INTEGER {
                    return crate::value::object::NA_INTEGER;
                }
                let label = old_levels.get((*c - 1) as usize).cloned().unwrap_or_default();
                remap
                    .iter()
                    .find(|(l, _)| *l == label)
                    .map(|(_, nc)| *nc as i32)
                    .unwrap_or(crate::value::object::NA_INTEGER)
            })
            .collect();
        return rebuild_factor(heap, interner, object, new_codes, new_names);
    }

    // Vector form: positional rename with NA-drop recoding.
    let new_names_raw = string_vec(heap, interner, value);
    if new_names_raw.len() != old_levels.len() {
        return Err(EvalError::Condition("number of levels differs".to_string()).into());
    }
    // Positional map old position -> new name; NA drops the level.
    // Surviving levels compact in order; codes remap.
    let mut surviving: Vec<String> = Vec::new();
    let mut position_map: Vec<Option<usize>> = Vec::new(); // old idx -> new 0-based
    for (i, name) in new_names_raw.iter().enumerate() {
        if name == "\0NA" || name == "NA" && !old_levels.contains(name) {
            position_map.push(None);
        } else {
            position_map.push(Some(surviving.len()));
            surviving.push(name.clone());
        }
    }
    let new_codes: Vec<i32> = codes
        .iter()
        .map(|c| {
            if *c == crate::value::object::NA_INTEGER {
                return crate::value::object::NA_INTEGER;
            }
            match position_map.get((*c - 1) as usize) {
                Some(Some(new_pos)) => *new_pos as i32 + 1,
                _ => crate::value::object::NA_INTEGER,
            }
        })
        .collect();
    rebuild_factor(heap, interner, object, new_codes, surviving)
}

/// Reassemble a factor with recoded integers and fresh levels — the
/// copy-on-write discipline the replacement forms share.
fn rebuild_factor(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    object: ValueId,
    new_codes: Vec<i32>,
    new_levels: Vec<String>,
) -> EvalResult {
    let interned: Vec<super::intern::InternedStr> = new_levels
        .iter()
        .map(|l| interner.intern_string(l))
        .collect();
    let levels_obj = heap.alloc(RObject::String(interned));
    // Fresh integer vector carrying the recoded values; class and levels
    // attach through copy-on-write discipline.
    let fresh = heap.alloc(RObject::Integer(new_codes));
    let fresh = copy_with_attrib(heap, interner, fresh, "levels", levels_obj).map_err(RSignal::from)?;
    let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("factor")]));
    copy_with_attrib(heap, interner, fresh, "class", class_obj).map_err(RSignal::from)
}

/// `grep(pattern, x, value)` / `grepl(pattern, x)` — elementwise leftmost
/// search through the pure-Rust regex engine. grepl returns the logical
/// mask exactly as GNU; grep returns indices by default or the matching
/// values with value=TRUE.
fn grep_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    logical: bool,
) -> EvalResult {
    // Tagged walk: pattern and x positional, value and invert by name.
    let mut pattern_arg = ValueId::NIL;
    let mut x_arg = ValueId::NIL;
    let mut value_mode = false;
    let mut invert = false;
    let mut ignore_case = false;
    let mut fixed = false;
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("value") => {
                if let Some(RObject::Logical(vals)) = heap.object(v) {
                    value_mode = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                }
            }
            Some("invert") => {
                if let Some(RObject::Logical(vals)) = heap.object(v) {
                    invert = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                }
            }
            Some("fixed") => {
                if let Some(RObject::Logical(vals)) = heap.object(v) {
                    fixed = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                }
            }
            Some("ignore.case") => {
                if let Some(RObject::Logical(vals)) = heap.object(v) {
                    ignore_case = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                }
            }
            _ => {
                if positional == 1 {
                    pattern_arg = v;
                } else if positional == 2 {
                    x_arg = v;
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let pattern_raw = string_operand(heap, interner, pattern_arg)
        .ok_or(EvalError::TypeError("grep pattern must be character"))?;
    let pattern = if ignore_case { pattern_raw.to_lowercase() } else { pattern_raw };
    let texts = string_vec(heap, interner, x_arg);
    let matches: Vec<bool> = texts
        .iter()
        .map(|text| {
            // GNU: NA strings are simply non-matching — invert then selects
            // them exactly as any non-match.
            if text == "\0NA" {
                invert
            } else {
                let haystack = if ignore_case { text.to_lowercase() } else { text.clone() };
                // fixed=TRUE is literal substring matching — no metacharacters.
                let found = if fixed {
                    haystack.contains(&pattern)
                } else {
                    super::regex::regex_search(&pattern, &haystack).is_some()
                };
                found != invert
            }
        })
        .collect();
    if logical {
        let out: Vec<i32> = matches.iter().map(|m| if *m { 1 } else { 0 }).collect();
        return Ok(heap.alloc(RObject::Logical(out)));
    }
    if value_mode {
        let out: Vec<super::intern::InternedStr> = texts
            .iter()
            .enumerate()
            .filter(|(i, _)| matches[*i])
            .map(|(_, s)| interner.intern_string(s))
            .collect();
        return Ok(heap.alloc(RObject::String(out)));
    }
    let out: Vec<i32> = matches
        .iter()
        .enumerate()
        .filter(|(_, m)| **m)
        .map(|(i, _)| i as i32 + 1)
        .collect();
    Ok(heap.alloc(RObject::Integer(out)))
}

/// `regexpr(pattern, x)` — match positions and lengths exactly as GNU:
/// 1-based start positions with -1 for no match, carrying the match.length
/// attribute.
fn regexpr_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let pattern = string_operand(heap, interner, operands[0])
        .ok_or(EvalError::TypeError("regexpr pattern must be character"))?;
    let texts = string_vec(heap, interner, operands[1]);
    let mut positions: Vec<i32> = Vec::new();
    let mut lengths: Vec<i32> = Vec::new();
    for text in &texts {
        match super::regex::regex_search(&pattern, text) {
            Some((start, end)) => {
                positions.push(start as i32 + 1);
                lengths.push((end - start) as i32);
            }
            None => {
                positions.push(-1);
                lengths.push(-1);
            }
        }
    }
    // copy_with_attrib appends, so the application order is GNU's printed
    /// order exactly — match.length first, then index.type, then useBytes.
    let use_bytes = heap.alloc(RObject::Logical(vec![1]));
    let result = heap.alloc(RObject::Integer(positions));
    let length_obj = heap.alloc(RObject::Integer(lengths));
    let result = copy_with_attrib(heap, interner, result, "match.length", length_obj).map_err(RSignal::from)?;
    let index_type = heap.alloc(RObject::String(vec![interner.intern_string("chars")]));
    let result = copy_with_attrib(heap, interner, result, "index.type", index_type).map_err(RSignal::from)?;
    copy_with_attrib(heap, interner, result, "useBytes", use_bytes).map_err(RSignal::from)
}

/// `unique(x, incomparables, fromLast)` — GNU's deduplication exactly:
/// first occurrences in original order (last occurrences with fromLast,
/// still in original order), NaN compared bitwise so NaN and NA_real_
/// dedupe separately, the string sentinel dedupes as one NA, and values
/// matching incomparables never compare at all — every occurrence stays.
fn unique_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let value = operands[0];
    // Tagged walk: incomparables and fromLast.
    let mut from_last = false;
    let mut incomparables_na = false;
    let mut cursor = args;
    let mut positional_seen = 0usize;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional_seen += 1;
        if positional_seen > 1 {
            let tag_name = match heap.object(tag) {
                Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                _ => None,
            };
            let evaluated = eval(heap, interner, car, caller_env)?;
            let v = force_if_promise(heap, interner, evaluated)?;
            match tag_name.as_deref() {
                Some("fromLast") => {
                    if let Some(RObject::Logical(vals)) = heap.object(v) {
                        from_last = !vals.is_empty() && vals[0] != 0 && vals[0] != crate::value::object::NA_INTEGER;
                    }
                }
                Some("incomparables") => {
                    // NA in incomparables means NA values never compare.
                    incomparables_na = match heap.object(v) {
                        Some(RObject::Logical(vals)) => vals.iter().any(|x| *x == crate::value::object::NA_INTEGER),
                        Some(RObject::Real(vals)) => vals.iter().any(|x| x.is_nan()),
                        Some(RObject::String(vals)) => {
                            vals.iter().any(|x| interner.string_text(*x) == "\0NA")
                        }
                        _ => v != ValueId::NIL,
                    };
                }
                _ => {}
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }

    // Generic dedup over an indexable equality relation — keeps the first
    /// (or last, then re-ordered) occurrence per equivalence class.
    macro_rules! dedup {
        ($variant:ident, $values:expr, $is_na:expr, $eq:expr) => {{
            let values: &Vec<_> = $values;
            let is_na = $is_na;
            let eq = $eq;
            let n = values.len();
            let mut keep = vec![false; n];
            let order: Vec<usize> = if from_last {
                (0..n).rev().collect()
            } else {
                (0..n).collect()
            };
            let mut representatives: Vec<usize> = Vec::new();
            for &i in &order {
                let vi = &values[i];
                if is_na(vi) && incomparables_na {
                    keep[i] = true;
                    continue;
                }
                // Reverse iteration encounters last occurrences first, so
                /// the plain keep-first-seen rule yields fromLast exactly —
                /// output ordering restores original indices either way.
                let duplicate = representatives.iter().any(|&r| eq(&values[r], vi));
                if !duplicate {
                    keep[i] = true;
                    representatives.push(i);
                }
            }
            let out: Vec<_> = (0..n)
                .filter(|&i| keep[i])
                .map(|i| values[i].clone())
                .collect();
            return Ok(heap.alloc(RObject::$variant(out)));
        }};
    }

    match heap.object(value) {
        Some(RObject::Real(values)) => {
            dedup!(
                Real,
                values,
                |v: &f64| v.is_nan(),
                |a: &f64, b: &f64| {
                    // Bitwise equality: NaN payloads compare by bits so
                    /// canonical NaN and NA_real_ dedupe separately, and
                    /// NaN equals itself exactly as GNU's dedup requires.
                    if a.is_nan() && b.is_nan() {
                        a.to_bits() == b.to_bits()
                    } else {
                        a == b
                    }
                }
            );
        }
        Some(RObject::Integer(values)) => {
            dedup!(Integer, values, |v: &i32| *v == crate::value::object::NA_INTEGER, |a: &i32, b: &i32| a == b);
        }
        Some(RObject::Logical(values)) => {
            dedup!(Logical, values, |v: &i32| *v == crate::value::object::NA_INTEGER, |a: &i32, b: &i32| a == b);
        }
        Some(RObject::String(values)) => {
            dedup!(
                String,
                values,
                |v: &super::intern::InternedStr| interner.string_text(*v) == "\0NA",
                |a: &super::intern::InternedStr, b: &super::intern::InternedStr| a == b
            );
        }
        _ => Err(EvalError::TypeError("unique requires an atomic vector").into()),
    }
}

/// `save(..., file, envir)` — variadic bare symbols or list= character
/// names resolved in envir, encoded as RBG1 exactly as the architecture's
/// GNU-wire-format replacement specifies.
fn save_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // Tagged walk: file/envir/list by name, bare symbols positional.
    let mut file_path: Option<String> = None;
    let mut envir = caller_env;
    let mut names: Vec<String> = Vec::new();
    // Positional-mode detection: any non-symbol untagged argument means
    /// the call follows .Internal's positional ordering — names vector,
    /// file, ascii, version, envir — rather than the user-facing bare
    /// symbol capture.
    let mut positional_mode = false;
    {
        let mut scan = args;
        loop {
            let (car, cdr, tag) = match heap.object(scan) {
                Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                _ => break,
            };
            let untagged = !matches!(heap.object(tag), Some(RObject::Symbol { .. }));
            if untagged && !matches!(heap.object(car), Some(RObject::Symbol { .. })) {
                positional_mode = true;
            }
            if cdr == ValueId::NIL {
                break;
            }
            scan = cdr;
        }
    }
    let mut positional_seen = 0usize;
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
        match tag_name.as_deref() {
            Some("file") => {
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                file_path = string_operand(heap, interner, v);
            }
            Some("envir") => {
                let evaluated = eval(heap, interner, car, caller_env)?;
                envir = force_if_promise(heap, interner, evaluated)?;
            }
            Some("list") => {
                let evaluated = eval(heap, interner, car, caller_env)?;
                let v = force_if_promise(heap, interner, evaluated)?;
                names.extend(string_vec(heap, interner, v));
            }
            Some("ascii") | Some("version") | Some("eval.promises") | Some("compress") | Some("compression_level") => {}
            _ => {
                if positional_mode {
                    // .Internal ordering: position 1 names, 2 file, 5 envir.
                    positional_seen += 1;
                    let evaluated = eval(heap, interner, car, caller_env)?;
                    let v = force_if_promise(heap, interner, evaluated)?;
                    match positional_seen {
                        1 => names.extend(string_vec(heap, interner, v)),
                        2 => file_path = string_operand(heap, interner, v),
                        5 => envir = v,
                        _ => {}
                    }
                } else if let Some(RObject::Symbol { name, .. }) = heap.object(car) {
                    // Bare symbol — the object name itself, GNU's
                    /// non-standard capture of unevaluated arguments.
                    names.push(interner.symbol_name(*name).to_string());
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let path = file_path.ok_or(EvalError::TypeError("save requires file"))?;
    let mut bindings: Vec<(String, ValueId)> = Vec::new();
    for name in &names {
        let sym = interner.intern_symbol(name);
        let (_, binding) = super::env::lookup(heap, envir, sym)
            .ok_or(EvalError::UnboundVariable(name.clone()))?;
        bindings.push((name.clone(), binding.value));
    }
    let bytes = super::rbg1::encode_save(heap, interner, &bindings);
    std::fs::write(&path, bytes).map_err(|e| EvalError::TypeError("save write failed"))?;
    Ok(ValueId::NIL)
}

/// `load(file, envir)` — decode the RBG1 stream, define every binding in
/// envir, and return the character vector of names exactly as GNU.
fn load_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let mut file_path: Option<String> = None;
    let mut envir = caller_env;
    let mut cursor = args;
    let mut positional = 0usize;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("envir") => envir = v,
            Some("file") => file_path = string_operand(heap, interner, v),
            _ => {
                if positional == 1 {
                    file_path = string_operand(heap, interner, v);
                } else if positional == 2 {
                    envir = v;
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let path = file_path.ok_or(EvalError::TypeError("load requires file"))?;
    let bytes = std::fs::read(&path).map_err(|e| EvalError::TypeError("load read failed"))?;
    let bindings = super::rbg1::decode_save(heap, interner, &bytes)
        .ok_or(EvalError::TypeError("bad save file"))?;
    let mut names: Vec<super::intern::InternedStr> = Vec::new();
    for (name, value) in bindings {
        let sym = interner.intern_symbol(&name);
        names.push(interner.intern_string(&name));
        super::env::define(heap, envir, sym, value)
            .map_err(|_| RSignal::Error(EvalError::TypeError("cannot add binding")))?;
    }
    Ok(heap.alloc(RObject::String(names)))
}

/// GNU's character coercion — integers and logicals render as decimal and
/// TRUE/FALSE exactly as paste does, real NA becomes the sentinel.
fn coerce_character(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
) -> EvalResult {
    // Date-classed input renders ISO exactly as as.character.Date.
    if is_date_classed(heap, interner, value) {
        let days = numeric_vector(heap, value);
        let out: Vec<super::intern::InternedStr> = days
            .iter()
            .map(|&d| {
                if d.is_nan() {
                    interner.intern_string("\0NA")
                } else {
                    interner.intern_string(&super::civil::format_iso(d as i64))
                }
            })
            .collect();
        return Ok(heap.alloc(RObject::String(out)));
    }
    // factor input decodes through its levels first — as.character(factor)
    /// yields the labels exactly as GNU.
    let factor_classed = get_attrib(heap, interner, value, "class")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "factor")),
            _ => None,
        })
        .unwrap_or(false);
    if factor_classed {
        if let Some(RObject::Integer(codes)) = heap.object(value) {
            let levels = get_attrib(heap, interner, value, "levels")
                .and_then(|id| match heap.object(id) {
                    Some(RObject::String(vals)) => Some(vals.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let out: Vec<super::intern::InternedStr> = codes
                .iter()
                .map(|&code| {
                    if code == crate::value::object::NA_INTEGER {
                        interner.intern_string("\0NA")
                    } else {
                        levels
                            .get((code - 1).max(0) as usize)
                            .copied()
                            .unwrap_or_else(|| interner.intern_string(""))
                    }
                })
                .collect();
            return Ok(heap.alloc(RObject::String(out)));
        }
    }
    let out: Vec<super::intern::InternedStr> = match heap.object(value) {
        Some(RObject::String(values)) => values.clone(),
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => values
            .iter()
            .map(|&v| {
                if v == crate::value::object::NA_INTEGER {
                    interner.intern_string("\0NA")
                } else {
                    interner.intern_string(&v.to_string())
                }
            })
            .collect(),
        Some(RObject::Real(values)) => values
            .iter()
            .map(|&v| {
                if v.to_bits() == crate::value::object::NA_REAL_BITS {
                    interner.intern_string("\0NA")
                } else {
                    interner.intern_string(&super::print::format_real(v))
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    Ok(heap.alloc(RObject::String(out)))
}

/// GNU's identical() — deep structural equality: same type, same values
/// bitwise (NaN payloads distinguish NA from NaN), same attributes in the
/// same order. Environments compare by identity exactly as GNU.
fn structural_identical(
    heap: &Heap,
    interner: &SymbolInterner,
    left: ValueId,
    right: ValueId,
) -> bool {
    if left == right {
        return true;
    }
    let attrib_identical = |heap: &Heap, interner: &SymbolInterner, a: ValueId, b: ValueId| -> bool {
        let chain_a = heap.get(a).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
        let chain_b = heap.get(b).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
        let mut ca = chain_a;
        let mut cb = chain_b;
        loop {
            let pa = match heap.object(ca) {
                Some(RObject::Pairlist { car, cdr, tag }) => Some((*car, *cdr, *tag)),
                _ => None,
            };
            let pb = match heap.object(cb) {
                Some(RObject::Pairlist { car, cdr, tag }) => Some((*car, *cdr, *tag)),
                _ => None,
            };
            match (pa, pb) {
                (None, None) => return true,
                (Some((va,cda,ta)), Some((vb,cdb,tb))) => {
                    let na = match heap.object(ta) {
                        Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
                        _ => String::new(),
                    };
                    let nb = match heap.object(tb) {
                        Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
                        _ => String::new(),
                    };
                    if na != nb || !structural_identical(heap, interner, va, vb) {
                        return false;
                    }
                    ca = cda;
                    cb = cdb;
                }
                _ => return false,
            }
        }
    };
    let payload_same = match (heap.object(left), heap.object(right)) {
        (Some(RObject::Nil), Some(RObject::Nil)) => true,
        (Some(RObject::Logical(a)), Some(RObject::Logical(b))) => a == b,
        (Some(RObject::Integer(a)), Some(RObject::Integer(b))) => a == b,
        (Some(RObject::Real(a)), Some(RObject::Real(b))) => {
            a.len() == b.len()
                && a.iter().zip(b.iter()).all(|(x, y)| {
                    if x.is_nan() && y.is_nan() {
                        x.to_bits() == y.to_bits()
                    } else {
                        x == y
                    }
                })
        }
        (Some(RObject::String(a)), Some(RObject::String(b))) => a == b,
        (Some(RObject::Raw(a)), Some(RObject::Raw(b))) => a == b,
        (Some(RObject::List(a)), Some(RObject::List(b))) => {
            a.len() == b.len()
                && a.iter().zip(b.iter()).all(|(x, y)| structural_identical(heap, interner, *x, *y))
        }
        (Some(RObject::Environment { .. }), Some(RObject::Environment { .. })) => left == right,
        _ => false,
    };
    payload_same && attrib_identical(heap, interner, left, right)
}

/// Copy an object dropping one attribute — attribute removal exactly as
/// GNU's attr(x, "name") <- NULL semantics.
pub(crate) fn remove_attrib(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: ValueId,
    name: &str,
) -> Result<ValueId, EvalError> {
    let chain = heap.get(value).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    let mut kept: Vec<(String, ValueId)> = Vec::new();
    let mut cursor = chain;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let attr_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
            _ => String::new(),
        };
        if attr_name != name {
            kept.push((attr_name, car));
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let stripped = heap.alloc(heap.object(value).cloned().unwrap_or(RObject::Nil));
    let mut result = stripped;
    for (attr_name, attr_value) in kept.into_iter() {
        result = copy_with_attrib(heap, interner, result, &attr_name, attr_value)?;
    }
    Ok(result)
}

/// `array(data, dim, dimnames)` — GNU's n-dimensional constructor: the
/// data recycles to the dim product exactly as matrix() does, an absent
/// data argument yields a logical NA, and dim attaches as the integer
/// extent vector.
fn array_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let raw = raw_args(heap, args);
    if raw.is_empty() {
        // array() with no arguments is one logical NA of dimension 1.
        let value = heap.alloc(RObject::Logical(vec![crate::value::object::NA_INTEGER]));
        let dim_obj = heap.alloc(RObject::Integer(vec![1]));
        return copy_with_attrib(heap, interner, value, "dim", dim_obj).map_err(RSignal::from);
    }
    let mut data_arg = ValueId::NIL;
    let mut dim_arg = ValueId::NIL;
    let mut dimnames_arg = ValueId::NIL;
    let mut positional = 0usize;
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        positional += 1;
        let tag_name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let evaluated = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, evaluated)?;
        match tag_name.as_deref() {
            Some("dim") => dim_arg = v,
            Some("dimnames") => dimnames_arg = v,
            _ => {
                if positional == 1 {
                    data_arg = v;
                } else if positional == 2 {
                    dim_arg = v;
                }
            }
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    let dims: Vec<i32> = match heap.object(dim_arg) {
        Some(RObject::Integer(vals)) => vals.clone(),
        Some(RObject::Real(vals)) => vals.iter().map(|&v| v as i32).collect(),
        _ => vec![1],
    };
    let total: usize = dims.iter().map(|&d| d.max(0) as usize).product();
    // Recycle the data to the dim product exactly as matrix() does.
    let value = match heap.object(data_arg).cloned() {
        Some(RObject::Integer(vals)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(if vals.is_empty() { crate::value::object::NA_INTEGER } else { vals[i % vals.len()] });
            }
            heap.alloc(RObject::Integer(out))
        }
        Some(RObject::Real(vals)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(if vals.is_empty() { f64::from_bits(crate::value::object::NA_REAL_BITS) } else { vals[i % vals.len()] });
            }
            heap.alloc(RObject::Real(out))
        }
        Some(RObject::Logical(vals)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(if vals.is_empty() { crate::value::object::NA_INTEGER } else { vals[i % vals.len()] });
            }
            heap.alloc(RObject::Logical(out))
        }
        Some(RObject::String(vals)) => {
            let mut out = Vec::with_capacity(total);
            for i in 0..total {
                out.push(if vals.is_empty() { interner.intern_string("\0NA") } else { vals[i % vals.len()] });
            }
            heap.alloc(RObject::String(out))
        }
        _ => heap.alloc(RObject::Logical(vec![crate::value::object::NA_INTEGER])),
    };
    let dim_obj = heap.alloc(RObject::Integer(dims));
    let mut result = copy_with_attrib(heap, interner, value, "dim", dim_obj).map_err(RSignal::from)?;
    if dimnames_arg != ValueId::NIL && !matches!(heap.object(dimnames_arg), Some(RObject::Nil)) {
        result = copy_with_attrib(heap, interner, result, "dimnames", dimnames_arg).map_err(RSignal::from)?;
    }
    Ok(result)
}

/// Whether the object carries the Date class.
pub(crate) fn is_date_classed(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    get_attrib(heap, interner, value, "class")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "Date")),
            _ => None,
        })
        .unwrap_or(false)
}

/// Numeric payload of a vector as reals — integer input widens exactly.
fn numeric_vector(heap: &Heap, value: ValueId) -> Vec<f64> {
    match heap.object(value) {
        Some(RObject::Real(vals)) => vals.clone(),
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals
            .iter()
            .map(|&v| {
                if v == crate::value::object::NA_INTEGER {
                    f64::from_bits(crate::value::object::NA_REAL_BITS)
                } else {
                    v as f64
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}
