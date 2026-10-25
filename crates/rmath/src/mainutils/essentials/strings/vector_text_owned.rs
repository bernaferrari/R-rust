#![forbid(unsafe_code)]
//! Checked UTF-8 string-vector operations with admitted construction storage.
use crate::sexp::{
    encoding::{self, Mode, TextEncoding},
    ffi::{NA_INTEGER, NA_LOGICAL, SEXPTYPE, TRUE},
    memory::WorkspaceReservation,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};
type Value = Sexp<'static>;
#[derive(Clone, Copy)]
pub(super) enum Operation {
    Count,
    Trim,
    Prefix,
    Suffix,
    Repeat,
}
fn error(message: impl Into<String>) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}
fn allocation() -> SexpError {
    SexpError::AllocationFailed {
        object: "string-vector workspace",
    }
}
fn argument(args: &Value, index: usize, name: &[u8]) -> SexpResult<Option<Value>> {
    let mut cell = args.clone();
    while !cell.is_nil() {
        if cell.try_tag_name_eq(name)? {
            return Ok(Some(cell.try_car()?.into_owned()?));
        }
        cell = cell.try_cdr()?.into_owned()?;
    }
    let mut cell = args.clone();
    let mut position = 0;
    while !cell.is_nil() {
        if cell.try_tag()?.is_nil() {
            if position == index {
                return Ok(Some(cell.try_car()?.into_owned()?));
            }
            position += 1;
        }
        cell = cell.try_cdr()?.into_owned()?;
    }
    Ok(None)
}
fn required(args: &Value, index: usize, name: &[u8]) -> SexpResult<Value> {
    argument(args, index, name)?.ok_or_else(|| error("missing string argument"))
}
fn vector(access: &RuntimeAccess, kind: SEXPTYPE, n: i64) -> SexpResult<SexpMut<'static>> {
    SexpMut::try_from_checked(
        access
            .allocator(&access.domain())?
            .allocate(|a| Some(a.alloc_vector(kind, n)))?,
    )
}
fn character(access: &RuntimeAccess, bytes: &[u8], tag: TextEncoding) -> SexpResult<Value> {
    access
        .allocator(&access.domain())?
        .allocate(|a| encoding::allocate_character(a, bytes, tag))
}
fn tag(value: &Value) -> SexpResult<TextEncoding> {
    let node = value.allocation()?;
    let h = node
        .heap_identity()
        .node_snapshot(node)
        .ok_or(SexpError::StaleAllocation)?;
    Ok(TextEncoding::from_gp(h.sxpinfo.gp()))
}
#[derive(Clone, Copy)]
enum Attributes {
    All,
    Shape,
    Names,
}
fn attributes(
    access: &RuntimeAccess,
    x: &Value,
    out: &mut SexpMut<'static>,
    policy: Attributes,
) -> SexpResult<()> {
    let domain = access.domain();
    let mut source = x.try_attrib()?.into_owned()?;
    let mut head = domain.nil();
    let mut tail = None;
    while !source.is_nil() {
        let keep = matches!(policy, Attributes::All)
            || source.try_tag_name_eq(b"names")?
            || (matches!(policy, Attributes::Shape)
                && (source.try_tag_name_eq(b"dim")? || source.try_tag_name_eq(b"dimnames")?));
        if keep {
            let value = source.try_car()?.into_owned()?;
            let tag = source.try_tag()?.into_owned()?;
            let next = access
                .allocator(&domain)?
                .pairlist_cell(&value, &domain.nil(), &tag)?;
            if let Some(previous) = tail {
                SexpMut::try_from_checked(previous)?.try_set_pairlist_cdr(&next)?;
            } else {
                head = next.clone();
            }
            tail = Some(next);
        }
        source = source.try_cdr()?.into_owned()?;
    }
    out.try_set_attribute(&head)
}
fn object_bit(value: &Value, object: bool) -> SexpResult<()> {
    let node = value.allocation()?;
    let heap = node.heap_identity();
    let mut h = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
    h.sxpinfo.set_obj(object);
    heap.replace_node(node, h).ok_or(SexpError::StaleAllocation)
}
fn factor(x: &Value) -> SexpResult<bool> {
    let mut attrs = x.try_attrib()?.into_owned()?;
    while !attrs.is_nil() {
        if attrs.try_tag_name_eq(b"class")? {
            let class = attrs.try_car()?.into_owned()?;
            if class.typeof_() == SEXPTYPE::STRSXP {
                for i in 0..class.len() {
                    if class.try_string_elt(i)?.try_char_eq(b"factor")? {
                        return Ok(true);
                    }
                }
            }
        }
        attrs = attrs.try_cdr()?.into_owned()?;
    }
    Ok(false)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Count {
    Chars,
    Bytes,
    Width,
}
fn count_type(value: Option<&Value>, parent: &Value) -> SexpResult<Count> {
    let Some(value) = value else {
        return Ok(Count::Chars);
    };
    if value.typeof_() != SEXPTYPE::STRSXP || value.len() != 1 {
        return Err(error("invalid 'type' argument"));
    }
    let value = value.try_string_elt(0)?.into_owned()?;
    if value.is_na_string() {
        return Err(error("invalid 'type' argument"));
    }
    let data = encoding::translate(&value, Mode::Bytes, parent)?;
    if data.data.is_empty() {
        return Err(error("invalid 'type' argument"));
    }
    for (label, mode) in [
        (b"chars".as_slice(), Count::Chars),
        (b"bytes".as_slice(), Count::Bytes),
        (b"width".as_slice(), Count::Width),
    ] {
        if label.starts_with(&data.data) {
            return Ok(mode);
        }
    }
    Err(error("invalid 'type' argument"))
}
fn logical(
    value: Option<Value>,
    default: i32,
    access: &RuntimeAccess,
    coerce: &mut impl FnMut(&RuntimeAccess, &Value, SEXPTYPE) -> SexpResult<Value>,
) -> SexpResult<i32> {
    let Some(mut value) = value else {
        return Ok(default);
    };
    if value.typeof_() != SEXPTYPE::LGLSXP {
        value = coerce(access, &value, SEXPTYPE::LGLSXP)?;
    }
    if value.is_empty() {
        Ok(NA_LOGICAL)
    } else {
        value.try_logical_elt(0)
    }
}
fn count(
    access: &RuntimeAccess,
    args: &Value,
    mut coerce: impl FnMut(&RuntimeAccess, &Value, SEXPTYPE) -> SexpResult<Value>,
) -> SexpResult<Value> {
    let original = required(args, 0, b"x")?;
    let type_arg = argument(args, 1, b"type")?;
    let allow_arg = argument(args, 2, b"allowNA")?;
    let keep_arg = argument(args, 3, b"keepNA")?;
    if factor(&original)? {
        return Err(error("'nchar()' requires a character vector"));
    }
    let kind = count_type(type_arg.as_ref(), args)?;
    let allow = logical(allow_arg, 0, access, &mut coerce)? == TRUE;
    let keep = logical(keep_arg, NA_LOGICAL, access, &mut coerce)?;
    let keep = if keep == NA_LOGICAL {
        kind != Count::Width
    } else {
        keep == TRUE
    };
    let x = if original.typeof_() == SEXPTYPE::STRSXP {
        original.clone()
    } else {
        coerce(access, &original, SEXPTYPE::STRSXP)?
    };
    let mut out = vector(access, SEXPTYPE::INTSXP, x.len())?;
    for i in 0..x.len() {
        let value = x.try_string_elt(i)?.into_owned()?;
        let answer = if value.is_na_string() {
            if keep { NA_INTEGER } else { 2 }
        } else if kind != Count::Bytes && tag(&value)? == TextEncoding::Bytes {
            if allow {
                NA_INTEGER
            } else {
                return Err(error(format!(
                    "{} is not computable in \"bytes\" encoding, element {}",
                    if kind == Count::Width {
                        "width"
                    } else {
                        "number of characters"
                    },
                    i + 1
                )));
            }
        } else {
            let data = encoding::translate(
                &value,
                if kind == Count::Bytes {
                    Mode::Bytes
                } else {
                    Mode::Utf8
                },
                args,
            )?;
            let count = if kind == Count::Bytes {
                Some(data.data.len())
            } else {
                std::str::from_utf8(&data.data).ok().map(|text| {
                    if kind == Count::Chars {
                        text.chars().count()
                    } else {
                        text.chars()
                            .map(crate::mainutils::unicode_width::columns)
                            .sum()
                    }
                })
            };
            match count {
                Some(n) => i32::try_from(n).map_err(|_| allocation())?,
                None if allow => NA_INTEGER,
                None => {
                    return Err(error(format!(
                        "invalid multibyte string, element {}",
                        i + 1
                    )));
                }
            }
        };
        out.try_set_integer_elt(i, answer)?;
    }
    attributes(access, &original, &mut out, Attributes::Shape)?;
    Ok(out.freeze())
}
fn trim(
    access: &RuntimeAccess,
    args: &Value,
    na: &Value,
    mut coerce: impl FnMut(&RuntimeAccess, &Value, SEXPTYPE) -> SexpResult<Value>,
) -> SexpResult<Value> {
    let original = required(args, 0, b"x")?;
    let width = required(args, 1, b"width")?;
    let x = if original.typeof_() == SEXPTYPE::STRSXP {
        original
    } else {
        coerce(access, &original, SEXPTYPE::STRSXP)?
    };
    let width = if width.typeof_() == SEXPTYPE::INTSXP {
        width
    } else {
        coerce(access, &width, SEXPTYPE::INTSXP)?
    };
    if x.is_empty() {
        return Ok(x);
    }
    if width.is_empty() || (width.len() < x.len() && x.len() % width.len() != 0) {
        return Err(error("invalid 'width' argument"));
    }
    for i in 0..width.len() {
        if width.try_integer_elt(i)? < 0 {
            return Err(error("invalid 'width' argument"));
        }
    }
    let mut out = vector(access, SEXPTYPE::STRSXP, x.len())?;
    for i in 0..x.len() {
        let value = x.try_string_elt(i)?.into_owned()?;
        if value.is_na_string() {
            out.try_set_string_elt(i, na.clone())?;
            continue;
        }
        let data = encoding::translate(&value, Mode::Utf8, args)?;
        let text = std::str::from_utf8(&data.data).map_err(|_| SexpError::InvalidUtf8)?;
        let max = width.try_integer_elt(i % width.len())? as usize;
        let mut used = 0usize;
        let mut end = 0;
        for (index, ch) in text.char_indices() {
            let columns = crate::mainutils::unicode_width::columns(ch);
            if columns > max - used {
                break;
            }
            used += columns;
            end = index + ch.len_utf8();
        }
        let output = character(
            access,
            &data.data[..end],
            if tag(&value)? == TextEncoding::Latin1 {
                TextEncoding::Utf8
            } else {
                tag(&value)?
            },
        )?;
        out.try_set_string_elt(i, output)?;
    }
    attributes(access, &x, &mut out, Attributes::All)?;
    let out = out.freeze();
    object_bit(&out, x.is_object())?;
    Ok(out)
}
fn affix(access: &RuntimeAccess, args: &Value, suffix: bool) -> SexpResult<Value> {
    let x = required(args, 0, b"x")?;
    let other = required(args, 1, if suffix { b"suffix" } else { b"prefix" })?;
    if x.typeof_() != SEXPTYPE::STRSXP || other.typeof_() != SEXPTYPE::STRSXP {
        return Err(error("non-character object(s)"));
    }
    let n = if x.is_empty() || other.is_empty() {
        0
    } else {
        x.len().max(other.len())
    };
    let mut out = vector(access, SEXPTYPE::LGLSXP, n)?;
    for i in 0..n {
        let value = x.try_string_elt(i % x.len())?.into_owned()?;
        let prefix = other.try_string_elt(i % other.len())?.into_owned()?;
        if prefix.is_na_string() {
            out.try_set_logical_elt(i, NA_LOGICAL)?;
            continue;
        }
        // GNU translates the affix even when the corresponding subject is NA.
        let prefix = encoding::translate(&prefix, Mode::Utf8, args)?;
        let matched = if value.is_na_string() {
            NA_LOGICAL
        } else if prefix.data.is_empty() {
            TRUE
        } else {
            let data = encoding::translate(&value, Mode::Utf8, args)?;
            i32::from(if suffix {
                data.data.ends_with(&prefix.data)
            } else {
                data.data.starts_with(&prefix.data)
            })
        };
        out.try_set_logical_elt(i, matched)?;
    }
    Ok(out.freeze())
}
struct Repeated {
    data: Vec<u8>,
    _ticket: WorkspaceReservation,
}
fn repeated(parent: &Value, source: &[u8], times: usize) -> SexpResult<Repeated> {
    let length = source
        .len()
        .checked_mul(times)
        .filter(|&n| i32::try_from(n).is_ok())
        .ok_or_else(|| error("R character strings are limited to 2^31-1 bytes"))?;
    let mut ticket = parent
        .allocation()?
        .heap_identity()
        .byte_ledger()
        .reserve(length)
        .ok_or_else(allocation)?;
    let mut data = Vec::new();
    data.try_reserve_exact(length).map_err(|_| allocation())?;
    ticket.try_resize(data.capacity()).ok_or_else(allocation)?;
    if !source.is_empty() {
        for _ in 0..times {
            data.extend_from_slice(source);
        }
    }
    Ok(Repeated {
        data,
        _ticket: ticket,
    })
}
fn repeat(
    access: &RuntimeAccess,
    args: &Value,
    na: &Value,
    mut coerce: impl FnMut(&RuntimeAccess, &Value, SEXPTYPE) -> SexpResult<Value>,
) -> SexpResult<Value> {
    let x = required(args, 0, b"x")?;
    let times = required(args, 1, b"times")?;
    let x = if x.typeof_() == SEXPTYPE::STRSXP {
        x
    } else {
        coerce(access, &x, SEXPTYPE::STRSXP)?
    };
    let times = if times.typeof_() == SEXPTYPE::INTSXP {
        times
    } else {
        coerce(access, &times, SEXPTYPE::INTSXP)?
    };
    let n = if x.is_empty() || times.is_empty() {
        0
    } else {
        x.len().max(times.len())
    };
    let mut out = vector(access, SEXPTYPE::STRSXP, n)?;
    for i in 0..n {
        let value = x.try_string_elt(i % x.len())?.into_owned()?;
        let times = times.try_integer_elt(i % times.len())?;
        let output = if value.is_na_string() || times == NA_INTEGER {
            na.clone()
        } else if times < 0 {
            return Err(error("invalid 'times' value"));
        } else if times == 1 {
            value.clone()
        } else if times == 0 {
            character(access, b"", TextEncoding::Native)?
        } else {
            let data = encoding::translate(&value, Mode::Bytes, args)?;
            let buffer = repeated(args, &data.data, times as usize)?;
            character(access, &buffer.data, tag(&value)?)?
        };
        out.try_set_string_elt(i, output)?;
    }
    if n == x.len() {
        attributes(access, &x, &mut out, Attributes::Names)?;
    }
    Ok(out.freeze())
}
pub(super) fn evaluate(
    access: &RuntimeAccess,
    args: &Value,
    na: &Value,
    operation: Operation,
    coerce: impl FnMut(&RuntimeAccess, &Value, SEXPTYPE) -> SexpResult<Value>,
) -> SexpResult<Value> {
    access.require_active()?;
    access.domain().link(args)?;
    access.domain().link(na)?;
    let result = match operation {
        Operation::Count => count(access, args, coerce),
        Operation::Trim => trim(access, args, na, coerce),
        Operation::Prefix => affix(access, args, false),
        Operation::Suffix => affix(access, args, true),
        Operation::Repeat => repeat(access, args, na, coerce),
    }?;
    access.require_active()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_vector_text_repeat_denial_recovers_at_same_cap() {
        use crate::sexp::{RSession, memory::ArenaBudget};
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .strings(&["parent"])
            .unwrap()
            .into_owned()
            .unwrap();
        let used = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(used + 8, 0)))
            .unwrap();
        assert!(repeated(&parent, "é".as_bytes(), 100).is_err());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        let data = repeated(&parent, "é".as_bytes(), 2).unwrap();
        assert_eq!(data.data, "éé".as_bytes());
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            data.data.capacity()
        );
        drop(data);
        assert!(repeated(&parent, b"", usize::MAX).unwrap().data.is_empty());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert_eq!(
            owner.with_arena(|a| a.budget().max_bytes).unwrap(),
            used + 8
        );
    }
}
