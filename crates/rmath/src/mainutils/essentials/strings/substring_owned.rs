#![forbid(unsafe_code)]
//! Encoding-aware substrings with original-owner storage through publication.
use crate::sexp::{
    encoding::{self, Mode, TextEncoding},
    ffi::{NA_INTEGER, SEXPTYPE},
    memory::WorkspaceReservation,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};
use std::ops::Range;
type Value = Sexp<'static>;

fn error(message: &str) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}
fn allocation() -> SexpError {
    SexpError::AllocationFailed {
        object: "substring workspace",
    }
}
fn argument(args: &Value, index: usize) -> SexpResult<Value> {
    let mut cell = args.clone();
    for _ in 0..index {
        cell = cell.try_cdr()?.into_owned()?;
    }
    cell.try_car()?.into_owned()
}
fn tag(value: &Value) -> SexpResult<TextEncoding> {
    let node = value.allocation()?;
    let header = node
        .heap_identity()
        .node_snapshot(node)
        .ok_or(SexpError::StaleAllocation)?;
    Ok(TextEncoding::from_gp(header.sxpinfo.gp()))
}
fn units(bytes: &[u8], byte_mode: bool) -> SexpResult<usize> {
    if byte_mode {
        Ok(bytes.len())
    } else {
        Ok(std::str::from_utf8(bytes)
            .map_err(|_| SexpError::InvalidUtf8)?
            .chars()
            .count())
    }
}
fn offset(bytes: &[u8], index: usize, byte_mode: bool) -> SexpResult<usize> {
    if byte_mode {
        Ok(index.min(bytes.len()))
    } else {
        Ok(std::str::from_utf8(bytes)
            .map_err(|_| SexpError::InvalidUtf8)?
            .char_indices()
            .nth(index)
            .map_or(bytes.len(), |(i, _)| i))
    }
}
fn span(bytes: &[u8], first: i32, last: Option<i32>, byte_mode: bool) -> SexpResult<Range<usize>> {
    let count = units(bytes, byte_mode)?;
    let start = usize::try_from(first.max(1) - 1)
        .map_err(|_| allocation())?
        .min(count);
    let end = last
        .map_or(count, |last| last.max(0) as usize)
        .min(count)
        .max(start);
    Ok(offset(bytes, start, byte_mode)?..offset(bytes, end, byte_mode)?)
}
struct Buffer {
    data: Vec<u8>,
    _ticket: WorkspaceReservation,
}
fn replace_bytes(
    parent: &Value,
    source: &[u8],
    replacement: &[u8],
    range: Range<usize>,
) -> SexpResult<Buffer> {
    let length = source
        .len()
        .checked_sub(range.len())
        .and_then(|n| n.checked_add(replacement.len()))
        .ok_or_else(allocation)?;
    let mut ticket = parent
        .allocation()?
        .heap_identity()
        .byte_ledger()
        .reserve(length)
        .ok_or_else(allocation)?;
    let mut data = Vec::new();
    data.try_reserve_exact(length).map_err(|_| allocation())?;
    ticket.try_resize(data.capacity()).ok_or_else(allocation)?;
    data.extend_from_slice(&source[..range.start]);
    data.extend_from_slice(replacement);
    data.extend_from_slice(&source[range.end..]);
    Ok(Buffer {
        data,
        _ticket: ticket,
    })
}
fn character(access: &RuntimeAccess, bytes: &[u8], encoding: TextEncoding) -> SexpResult<Value> {
    let domain = access.domain();
    access
        .allocator(&domain)?
        .allocate(|arena| encoding::allocate_character(arena, bytes, encoding))
}

pub(super) fn evaluate(
    access: &RuntimeAccess,
    args: &Value,
    na: &Value,
    replacement: bool,
) -> SexpResult<Value> {
    access.require_active()?;
    let domain = access.domain();
    domain.link(args)?;
    domain.link(na)?;
    let x = argument(args, 0)?;
    let first = argument(args, 1)?;
    let last = argument(args, 2)?;
    if x.typeof_() != SEXPTYPE::STRSXP {
        return Err(error(if replacement {
            "replacing substrings in a non-character object"
        } else {
            "extracting substrings from a non-character object"
        }));
    }
    if x.is_empty() {
        return Ok(x);
    }
    if first.typeof_() != SEXPTYPE::INTSXP
        || first.is_empty()
        || (!last.is_nil() && (last.typeof_() != SEXPTYPE::INTSXP || last.is_empty()))
    {
        return Err(error("invalid substring arguments"));
    }
    let value = if replacement {
        Some(argument(args, 3)?)
    } else {
        None
    };
    if value
        .as_ref()
        .is_some_and(|v| v.typeof_() != SEXPTYPE::STRSXP || v.is_empty())
    {
        return Err(error("invalid value"));
    }
    let mut out = SexpMut::try_from_checked(
        access
            .allocator(&domain)?
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, x.len())))?,
    )?;
    for i in 0..x.len() {
        let source = x.try_string_elt(i)?.into_owned()?;
        let start = first.try_integer_elt(i % first.len())?;
        let stop = if last.is_nil() {
            None
        } else {
            Some(last.try_integer_elt(i % last.len())?)
        };
        if source.is_na_string() || start == NA_INTEGER || stop == Some(NA_INTEGER) {
            out.try_set_string_elt(i, na.clone())?;
            continue;
        }
        let encoding = tag(&source)?;
        let byte_mode = matches!(encoding, TextEncoding::Bytes | TextEncoding::Latin1);
        let source_data = encoding::translate(&source, Mode::Bytes, args)?;
        let range = span(&source_data.data, start, stop, byte_mode)?;
        let result = if let Some(value) = &value {
            let selected = value.try_string_elt(i % value.len())?.into_owned()?;
            if selected.is_na_string() {
                out.try_set_string_elt(i, na.clone())?;
                continue;
            }
            // A reversed or out-of-range replacement leaves the original tag intact.
            if range.is_empty() {
                source.clone()
            } else {
                let mode = if byte_mode { Mode::Bytes } else { Mode::Utf8 };
                let replacement_data = encoding::translate(&selected, mode, args)?;
                let count = units(&source_data.data[range.clone()], byte_mode)?
                    .min(units(&replacement_data.data, byte_mode)?);
                let end = offset(&replacement_data.data, count, byte_mode)?;
                let consumed = offset(&source_data.data[range.clone()], count, byte_mode)?;
                let buffer = replace_bytes(
                    args,
                    &source_data.data,
                    &replacement_data.data[..end],
                    range.start..range.start + consumed,
                )?;
                character(access, &buffer.data, encoding)?
            }
        } else {
            character(access, &source_data.data[range], encoding)?
        };
        out.try_set_string_elt(i, result)?;
    }
    super::iconv_owned::copy_attributes(access, &x, &mut out)?;
    let result = out.freeze();
    let node = result.allocation()?;
    let heap = node.heap_identity();
    let mut header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
    header.sxpinfo.set_obj(x.is_object());
    heap.replace_node(node, header)
        .ok_or(SexpError::StaleAllocation)?;
    access.require_active()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_substring_workspace_denial_recovers_at_original_cap() {
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
            .with_arena(|a| a.set_budget(ArenaBudget::new(used + 4, 0)))
            .unwrap();
        assert!(replace_bytes(&parent, b"large subject", b"a", 0..1).is_err());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        let smaller = replace_bytes(&parent, b"abc", b"Z", 1..2).unwrap();
        assert_eq!(smaller.data, b"aZc");
        assert_eq!(
            owner.with_arena(|a| a.budget().max_bytes).unwrap(),
            used + 4
        );
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            smaller.data.capacity()
        );
        drop(smaller);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
}
