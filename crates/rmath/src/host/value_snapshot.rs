//! Owned-value snapshot export from the new-world heap.
//!
//! The host facade's migration onto `value::Session` needs every evaluated
//! `ValueId` converted into the FFI-safe owned `RValue` tree embedders
//! consume today. This module is that bridge: it reads the generational
//! heap without mutating it and mirrors the old world's `SexpValue`
//! conversion semantics exactly — length-1 atomic vectors surface as
//! scalars, NA sentinels surface as `None`, attributes collect into
//! `RMetadata`, and non-data objects surface as `Unsupported`.

use crate::value::heap::Heap;
use crate::value::id::ValueId;
use crate::value::intern::SymbolInterner;
use crate::value::object::{RObject, NA_INTEGER, NA_REAL_BITS};
use crate::value::primitives::get_attrib_public;

use super::values::{RAttribute, RComplexValue, RMetadata, RValue};

/// Snapshot recursion ceiling — pathologically nested lists surface as
/// `Unsupported` rather than risking embedder-thread stack exhaustion.
const MAX_SNAPSHOT_DEPTH: usize = 128;

/// Convert the heap object behind `id` into an owned `RValue` tree.
pub fn snapshot_value(heap: &Heap, interner: &SymbolInterner, id: ValueId) -> RValue {
    snapshot_at(heap, interner, id, 0)
}

fn snapshot_at(heap: &Heap, interner: &SymbolInterner, id: ValueId, depth: usize) -> RValue {
    if depth > MAX_SNAPSHOT_DEPTH {
        return RValue::Unsupported {
            type_name: "deeply nested value".to_string(),
        };
    }
    let Some(object) = heap.object(id) else {
        return RValue::Null;
    };
    let base = match object {
        RObject::Nil => RValue::Null,
        RObject::Logical(values) => {
            let converted: Vec<Option<bool>> = values
                .iter()
                .map(|v| {
                    if *v == NA_INTEGER {
                        None
                    } else {
                        Some(*v != 0)
                    }
                })
                .collect();
            if converted.len() == 1 {
                RValue::Logical(converted[0])
            } else {
                RValue::LogicalVector(converted)
            }
        }
        RObject::Integer(values) => {
            let converted: Vec<Option<i32>> = values
                .iter()
                .map(|v| if *v == NA_INTEGER { None } else { Some(*v) })
                .collect();
            if converted.len() == 1 {
                RValue::Integer(converted[0])
            } else {
                RValue::IntegerVector(converted)
            }
        }
        RObject::Real(values) => {
            let converted: Vec<Option<f64>> = values
                .iter()
                .map(|v| {
                    if v.to_bits() == NA_REAL_BITS {
                        None
                    } else {
                        Some(*v)
                    }
                })
                .collect();
            if converted.len() == 1 {
                RValue::Real(converted[0])
            } else {
                RValue::RealVector(converted)
            }
        }
        RObject::Complex(values) => {
            let converted: Vec<Option<RComplexValue>> = values
                .iter()
                .map(|c| {
                    if c.r.to_bits() == NA_REAL_BITS || c.i.to_bits() == NA_REAL_BITS {
                        None
                    } else {
                        Some(RComplexValue {
                            real: c.r,
                            imaginary: c.i,
                        })
                    }
                })
                .collect();
            RValue::ComplexVector(converted)
        }
        RObject::String(values) => {
            let converted: Vec<Option<String>> = values
                .iter()
                .map(|s| {
                    let text = interner.string_text(*s);
                    if text == "\0NA" {
                        None
                    } else {
                        Some(text.to_string())
                    }
                })
                .collect();
            RValue::StringVector(converted)
        }
        RObject::Raw(values) => RValue::RawVector(values.clone()),
        RObject::List(items) => RValue::List(
            items
                .iter()
                .map(|item| snapshot_at(heap, interner, *item, depth + 1))
                .collect(),
        ),
        RObject::Expression(_) => RValue::Unsupported {
            type_name: "expression".to_string(),
        },
        RObject::Symbol { .. } => RValue::Unsupported {
            type_name: "symbol".to_string(),
        },
        RObject::Pairlist { .. } => RValue::Unsupported {
            type_name: "pairlist".to_string(),
        },
        RObject::Language { .. } => RValue::Unsupported {
            type_name: "language".to_string(),
        },
        RObject::Closure { .. } => RValue::Unsupported {
            type_name: "closure".to_string(),
        },
        RObject::Environment { .. } => RValue::Unsupported {
            type_name: "environment".to_string(),
        },
        RObject::Promise { .. } => RValue::Unsupported {
            type_name: "promise".to_string(),
        },
        RObject::Primitive(_) => RValue::Unsupported {
            type_name: "builtin".to_string(),
        },
        RObject::Bytecode(_) => RValue::Unsupported {
            type_name: "bytecode".to_string(),
        },
        RObject::Altrep(_) => RValue::Unsupported {
            type_name: "altrep".to_string(),
        },
        RObject::External(_) => RValue::Unsupported {
            type_name: "external".to_string(),
        },
        RObject::WeakRef { .. } => RValue::Unsupported {
            type_name: "weakref".to_string(),
        },
        RObject::S4Object => RValue::Unsupported {
            type_name: "S4".to_string(),
        },
    };
    attach_metadata(heap, interner, id, base, depth)
}

/// Wrap the payload in `Attributed` when the object carries attributes —
/// names/dim/class/levels occupy their dedicated metadata slots exactly as
/// the old world's conversion pins; every other attribute joins the
/// attribute list in pairlist order.
fn attach_metadata(
    heap: &Heap,
    interner: &SymbolInterner,
    id: ValueId,
    base: RValue,
    depth: usize,
) -> RValue {
    let names = attribute_names_present(heap, interner, id);
    if names.is_empty() {
        return base;
    }
    let mut metadata = RMetadata::default();
    for name in &names {
        let Some(attr_id) = get_attrib_public(heap, interner, id, name) else {
            continue;
        };
        match name.as_str() {
            "names" => {
                metadata.names = string_vector_parts(heap, interner, attr_id);
            }
            "dim" => {
                if let Some(RObject::Integer(values)) = heap.object(attr_id) {
                    metadata.dim = Some(values.clone());
                }
            }
            "class" => {
                metadata.class = string_vector_parts(heap, interner, attr_id);
            }
            "levels" => {
                metadata.levels = string_vector_parts(heap, interner, attr_id);
            }
            other => {
                metadata.attributes.push(RAttribute {
                    name: other.to_string(),
                    value: snapshot_at(heap, interner, attr_id, depth + 1),
                });
            }
        }
    }
    // Attributes whose conversion found nothing leave no metadata — the
    /// un-attributed shape surfaces bare exactly as the old world.
    if metadata.names.is_none()
        && metadata.dim.is_none()
        && metadata.class.is_none()
        && metadata.levels.is_none()
        && metadata.attributes.is_empty()
    {
        return base;
    }
    RValue::Attributed {
        value: Box::new(base),
        metadata,
    }
}

/// Enumerate the attribute names the object carries — the pairlist chain's
/// tag symbols in chain order exactly as GNU's ATTRIB walk.
fn attribute_names_present(heap: &Heap, interner: &SymbolInterner, id: ValueId) -> Vec<String> {
    let mut out = Vec::new();
    let Some(slot) = heap.get(id) else {
        return out;
    };
    let mut cursor = slot.attrib;
    for _ in 0..100_000 {
        if cursor == ValueId::NIL {
            break;
        }
        match heap.object(cursor) {
            Some(RObject::Pairlist { cdr, tag, .. }) => {
                if let Some(RObject::Symbol { name, .. }) = heap.object(*tag) {
                    out.push(interner.symbol_name(*name).to_string());
                }
                cursor = *cdr;
            }
            _ => break,
        }
    }
    out
}

/// Read a string vector as owned optional strings — the shared shape of
/// names/class/levels metadata.
fn string_vector_parts(
    heap: &Heap,
    interner: &SymbolInterner,
    id: ValueId,
) -> Option<Vec<Option<String>>> {
    match heap.object(id) {
        Some(RObject::String(values)) => Some(
            values
                .iter()
                .map(|s| {
                    let text = interner.string_text(*s);
                    if text == "\0NA" {
                        None
                    } else {
                        Some(text.to_string())
                    }
                })
                .collect(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::session::Session;

    /// Evaluate a one-expression script and snapshot its final value.
    fn eval_snapshot(source: &str) -> RValue {
        let mut session = Session::new();
        let value = session
            .eval_value(source)
            .expect("evaluation succeeds");
        snapshot_value(&session.heap, &session.interner, value)
    }

    #[test]
    fn scalar_atomic_values_snapshot_as_scalars() {
        assert_eq!(eval_snapshot("42L"), RValue::Integer(Some(42)));
        assert_eq!(eval_snapshot("1.5"), RValue::Real(Some(1.5)));
        assert_eq!(eval_snapshot("TRUE"), RValue::Logical(Some(true)));
    }

    #[test]
    fn vectors_snapshot_with_na_holes() {
        assert_eq!(
            eval_snapshot("c(1L, NA, 3L)"),
            RValue::IntegerVector(vec![Some(1), None, Some(3)])
        );
        assert_eq!(
            eval_snapshot("c('a', NA_character_, 'c')"),
            RValue::StringVector(vec![Some("a".into()), None, Some("c".into())])
        );
    }

    #[test]
    fn named_vectors_carry_names_metadata() {
        match eval_snapshot("c(a=1, b=2)") {
            RValue::Attributed { metadata, .. } => {
                assert_eq!(
                    metadata.names,
                    Some(vec![Some("a".into()), Some("b".into())])
                );
            }
            other => panic!("expected attributed value, got {other:?}"),
        }
    }

    #[test]
    fn lists_recursively_snapshot() {
        let bare = match eval_snapshot("list(1L, c('x', 'y'))") {
            RValue::Attributed { value, .. } => *value,
            direct => direct,
        };
        match bare {
            RValue::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], RValue::Integer(Some(1)));
            }
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn closures_surface_as_unsupported() {
        match eval_snapshot("function(x) x") {
            RValue::Unsupported { type_name } => assert_eq!(type_name, "closure"),
            other => panic!("expected unsupported, got {other:?}"),
        }
    }

    #[test]
    fn null_snapshots_as_null() {
        assert_eq!(eval_snapshot("NULL"), RValue::Null);
    }

    #[test]
    fn dim_metadata_surfaces_matrices() {
        match eval_snapshot("matrix(1:4, 2, 2)") {
            RValue::Attributed { metadata, .. } => {
                assert_eq!(metadata.dim, Some(vec![2, 2]));
            }
            other => panic!("expected attributed matrix, got {other:?}"),
        }
    }

    #[test]
    fn factors_carry_class_and_levels() {
        match eval_snapshot("factor(c('a', 'b', 'a'))") {
            RValue::Attributed { metadata, .. } => {
                assert_eq!(metadata.class, Some(vec![Some("factor".into())]));
                assert_eq!(
                    metadata.levels,
                    Some(vec![Some("a".into()), Some("b".into())])
                );
            }
            other => panic!("expected factor metadata, got {other:?}"),
        }
    }
}
