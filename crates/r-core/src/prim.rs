//! The primitive-authoring API — the shared helper layer every builtin
//! implementation consumes: argument evaluation and coercion, vector
//! extraction, scalar reads, and the par() surface constants. These
//! descended from the flat registry exactly as the foundation plan
//! organizes them: authoring utilities are core machinery, not base
//! builtins, so domain crates (r-stats, r-graphics) depend only on
//! r-core and r-math.

use crate::eval::{eval, force_if_promise, EvalError, EvalResult, RSignal};
use crate::heap::Heap;
use crate::id::ValueId;
use crate::intern::SymbolInterner;
use crate::object::RObject;

/// Collect the raw argument expressions of a call without evaluating them —
/// what special forms operate on.
pub fn raw_args(heap: &Heap, mut args: ValueId) -> Vec<ValueId> {
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

/// Evaluate and force the leading `expected` arguments of a call.
pub fn eval_args(
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
        // Empty actual arguments error with their 1-based call position
        // exactly as GNU's argument forcing reports them.
        if car == ValueId::MISSING {
            return Err(EvalError::Condition(format!(
                "argument {} is empty",
                operands.len() + 1
            ))
            .into());
        }
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

pub fn vector_length(heap: &Heap, value: ValueId) -> Option<usize> {
    match heap.object(value) {
        Some(RObject::Logical(v)) => Some(v.len()),
        Some(RObject::Integer(v)) => Some(v.len()),
        Some(RObject::Real(v)) => Some(v.len()),
        Some(RObject::String(v)) => Some(v.len()),
        Some(RObject::Raw(v)) => Some(v.len()),
        Some(RObject::Complex(v)) => Some(v.len()),
        Some(RObject::List(v)) => Some(v.len()),
        Some(RObject::Expression(v)) => Some(v.len()),
        // GNU: length of a call counts the operator plus every argument
        // exactly as the pairlist chain encodes.
        Some(RObject::Language { cdr, .. }) => {
            let mut count = 1usize;
            let mut cursor = *cdr;
            for _ in 0..100_000 {
                if cursor == ValueId::NIL {
                    break;
                }
                match heap.object(cursor) {
                    Some(RObject::Pairlist { cdr, .. }) => {
                        count += 1;
                        cursor = *cdr;
                    }
                    _ => break,
                }
            }
            Some(count)
        }
        _ => None,
    }
}

/// Numeric coercion for the reduction and math primitives: logical and
/// integer vectors widen to reals with the NA bit pattern preserved.
pub fn reals_of_value(heap: &Heap, value: ValueId) -> Result<Vec<f64>, EvalError> {
    match heap.object(value) {
        Some(RObject::Real(values)) => Ok(values.clone()),
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => Ok(values
            .iter()
            .map(|v| {
                if *v == crate::object::NA_INTEGER {
                    f64::from_bits(crate::object::NA_REAL_BITS)
                } else {
                    *v as f64
                }
            })
            .collect()),
        _ => Err(EvalError::TypeError("numeric argument expected")),
    }
}

/// Copy an object with one attribute replaced (or prepended when absent).
/// R's copy-on-write discipline: the original object is never mutated —
/// assignment binds the fresh copy, so aliases keep the old attributes.
/// The attribute chain itself is rebuilt node-by-node for the same reason.
pub fn copy_with_attrib(
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
        // enumerates in insertion order exactly as the oracle pins.
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

/// `x$name` — exact-match extraction by name. The second operand is a
/// literal symbol, never evaluated — GNU's argument discipline for `$`.
/// The slice implements exact matching only; GNU's partial matching is
/// documented as future work. Atomic vectors extract a length-one vector
/// of the same type; list elements unbox exactly as `[[`.
/// The 72 graphics parameters GNU's par() knows — the readonly device
/// six (cin cra csi cxy din page) excluded by no.readonly exactly.
pub const PAR_NAMES: &[&str] = &[
    "adj", "ann", "ask", "bg", "bty", "cex", "cex.axis", "cex.lab", "cex.main", "cex.sub",
    "cin", "col", "col.axis", "col.lab", "col.main", "col.sub", "cra", "crt", "csi", "cxy",
    "din", "err", "family", "fg", "fig", "fin", "font", "font.axis", "font.lab", "font.main",
    "font.sub", "lab", "las", "lend", "lheight", "ljoin", "lmitre", "lty", "lwd", "mai",
    "mar", "mex", "mfcol", "mfg", "mfrow", "mgp", "mkh", "new", "oma", "omd",
    "omi", "page", "pch", "pin", "plt", "ps", "pty", "smo", "srt", "tck",
    "tcl", "usr", "xaxp", "xaxs", "xaxt", "xlog", "xpd", "yaxp", "yaxs", "yaxt",
    "ylog", "ylbias",
];

pub const PAR_READONLY: &[&str] = &["cin", "cra", "csi", "cxy", "din", "page"];

/// Convert an evaluated argument into the cascade's value shape —
    /// GNU's elementwise coercion exactly, strings passing through for
    /// the character-valued parameters.
pub fn parvalue_of(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Option<crate::graphics::layout::ParValue> {
    use crate::graphics::layout::ParValue;
    match heap.object(value) {
        Some(RObject::Real(values)) => Some(ParValue::Real(values.clone())),
        Some(RObject::Integer(values)) => Some(ParValue::Integer(values.clone())),
        Some(RObject::Logical(values)) => Some(ParValue::Logical(values.clone())),
        Some(RObject::String(values)) if values.len() == 1 => {
            Some(ParValue::String(interner.string_text(values[0]).to_string()))
        }
        _ => None,
    }
}

/// First element of a numeric vector as a double — the scalar extraction
/// the constructors share.
pub fn first_real(heap: &Heap, value: ValueId) -> Result<f64, EvalError> {
    match heap.object(value) {
        Some(RObject::Real(v)) if !v.is_empty() => Ok(v[0]),
        Some(RObject::Integer(v)) if !v.is_empty() => {
            if v[0] == crate::object::NA_INTEGER {
                Err(EvalError::TypeError("NA scalar"))
            } else {
                Ok(v[0] as f64)
            }
        }
        Some(RObject::Logical(v)) if !v.is_empty() => Ok(v[0] as f64),
        _ => Err(EvalError::TypeError("numeric scalar required")),
    }
}

/// Extract the first string of an operand — the path coercion every file
/// primitive shares.
pub fn string_operand(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Option<String> {
    match heap.object(value) {
        Some(RObject::String(values)) if !values.is_empty() => {
            Some(interner.string_text(values[0]).to_string())
        }
        _ => None,
    }
}

/// GNU primitive argument matching exactly as pmatch's exact pass:
/// tagged arguments bind by exact name first, untagged arguments fill
/// the remaining slots in order. Tags read through the interner as
/// plain symbol names.
pub fn eval_matched_args(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    names: &[&str],
) -> Result<Vec<Option<ValueId>>, RSignal> {
    let mut slots: Vec<Option<ValueId>> = vec![None; names.len()];
    // Collect (tag, value) pairs without evaluating — matching is
    // structural exactly as GNU's argument resolution precedes forcing.
    let mut items: Vec<(Option<String>, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let name = if tag == ValueId::NIL {
            None
        } else if let Some(RObject::Symbol { name, .. }) = heap.object(tag) {
            Some(interner.symbol_name(*name).to_string())
        } else {
            None
        };
        items.push((name, car));
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    // Exact-name pass first.
    let mut used = vec![false; items.len()];
    for (slot, param) in names.iter().enumerate() {
        for (i, (tag, car)) in items.iter().enumerate() {
            if used[i] {
                continue;
            }
            if tag.as_deref() == Some(param) {
                let value = eval(heap, interner, *car, caller_env)?;
                slots[slot] = Some(force_if_promise(heap, interner, value)?);
                used[i] = true;
                break;
            }
        }
    }
    // Positional filling for the untagged remainder.
    let mut next_slot = 0;
    for (i, (tag, car)) in items.iter().enumerate() {
        if used[i] || tag.is_some() {
            continue;
        }
        while next_slot < names.len() && slots[next_slot].is_some() {
            next_slot += 1;
        }
        if next_slot >= names.len() {
            break;
        }
        let value = eval(heap, interner, *car, caller_env)?;
        slots[next_slot] = Some(force_if_promise(heap, interner, value)?);
        used[i] = true;
    }
    Ok(slots)
}

/// Numeric payload of a vector as reals — integer input widens exactly.
pub fn numeric_vector(heap: &Heap, value: ValueId) -> Vec<f64> {
    match heap.object(value) {
        Some(RObject::Real(vals)) => vals.clone(),
        Some(RObject::Integer(vals)) | Some(RObject::Logical(vals)) => vals
            .iter()
            .map(|&v| {
                if v == crate::object::NA_INTEGER {
                    f64::from_bits(crate::object::NA_REAL_BITS)
                } else {
                    v as f64
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// GNU's argument binding for fixed-formals primitives: named arguments
/// claim their exact formal slots first, unnamed arguments fill the open
/// slots in call order. Slots left open read as None.
pub fn bind_formals(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    formals: &[&str],
) -> Result<Vec<Option<ValueId>>, RSignal> {
    let mut slots: Vec<Option<ValueId>> = vec![None; formals.len()];
    let mut pending: Vec<(Option<String>, ValueId)> = Vec::new();
    let mut cursor = args;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        if car == ValueId::MISSING {
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
        pending.push((tag_name, force_if_promise(heap, interner, evaluated)?));
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    // Exact-name pass first, then positional fill of open slots.
    let mut positional: Vec<ValueId> = Vec::new();
    for (tag, value) in pending {
        match tag.as_deref().and_then(|n| formals.iter().position(|f| *f == n)) {
            Some(slot) if slots[slot].is_none() => slots[slot] = Some(value),
            Some(_) => {}
            None => positional.push(value),
        }
    }
    let mut iter = positional.into_iter();
    for slot in slots.iter_mut() {
        if slot.is_none()
            && let Some(v) = iter.next() {
                *slot = Some(v);
            }
    }
    Ok(slots)
}

pub fn logical_or_na(heap: &Heap, value: ValueId) -> Option<bool> {
    match heap.object(value) {
        Some(RObject::Logical(vals)) => {
            let first = vals.first().copied().unwrap_or(crate::object::NA_INTEGER);
            if first == crate::object::NA_INTEGER {
                None
            } else {
                Some(first != 0)
            }
        }
        _ => Some(false),
    }
}

/// First logical element as a boolean — the tagged-argument coercion every
/// flag-style parameter shares.
pub fn logical_scalar(heap: &Heap, value: ValueId) -> bool {
    match heap.object(value) {
        Some(RObject::Logical(vals)) => vals.first().copied().unwrap_or(0) != 0,
        Some(RObject::Integer(vals)) => vals.first().copied().unwrap_or(0) != 0,
        Some(RObject::Real(vals)) => vals.first().copied().unwrap_or(0.0) != 0.0,
        _ => false,
    }
}

/// Build a named list from (name, value) entries — GNU's list shape with
/// the names attribute exactly as introspection primitives report.
pub fn named_list(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    entries: Vec<(&str, ValueId)>,
) -> EvalResult {
    let names: Vec<_> = entries.iter().map(|(n, _)| interner.intern_string(n)).collect();
    let items: Vec<ValueId> = entries.iter().map(|(_, v)| *v).collect();
    let result = heap.alloc(RObject::List(items));
    let names_obj = heap.alloc(RObject::String(names));
    copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)
}
