#![forbid(unsafe_code)]
//! Original-runtime conversion, retaining source storage through publication.
use super::iconv_codec::{self, Codec};
use crate::sexp::{
    encoding::{self, Mode, TextEncoding},
    ffi::{NA_LOGICAL, SEXPTYPE},
    memory::TransientReservation,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
};
type Value = Sexp<'static>;
pub(super) struct Bytes {
    pub(super) data: Vec<u8>,
    _ticket: TransientReservation,
}
fn error(message: impl Into<String>) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}
fn allocation() -> SexpError {
    SexpError::AllocationFailed {
        object: "iconv workspace",
    }
}
fn reserve(parent: &Value, n: usize) -> SexpResult<TransientReservation> {
    let node = parent.allocation()?;
    node.heap_identity()
        .reserve_payload_bytes(node, n)
        .ok_or_else(allocation)
}
fn vector(access: &RuntimeAccess, kind: SEXPTYPE, n: i64) -> SexpResult<SexpMut<'static>> {
    let domain = access.domain();
    SexpMut::try_from_checked(
        access
            .allocator(&domain)?
            .allocate(|a| Some(a.alloc_vector(kind, n)))?,
    )
}
fn character(access: &RuntimeAccess, bytes: &[u8], tag: TextEncoding) -> SexpResult<Value> {
    let domain = access.domain();
    access
        .allocator(&domain)?
        .allocate(|a| encoding::allocate_character(a, bytes, tag))
}
fn argument(args: &Value, index: usize, name: &[u8]) -> SexpResult<Option<Value>> {
    let mut current = args.clone();
    while !current.is_nil() {
        if current.try_tag_name_eq(name)? {
            return Ok(Some(current.try_car()?.into_owned()?));
        }
        current = current.try_cdr()?.into_owned()?;
    }
    let mut current = args.clone();
    let mut position = 0;
    while !current.is_nil() {
        if current.try_tag()?.is_nil() {
            if position == index {
                return Ok(Some(current.try_car()?.into_owned()?));
            }
            position += 1;
        }
        current = current.try_cdr()?.into_owned()?;
    }
    Ok(None)
}
fn text(value: &Value, parent: &Value, mode: Mode) -> SexpResult<Bytes> {
    let data = encoding::translate(value, mode, parent)?;
    Ok(Bytes {
        data: data.data,
        _ticket: data.reservation,
    })
}
fn label(args: &Value, index: usize, name: &'static str) -> SexpResult<Option<Bytes>> {
    let Some(value) = argument(args, index, name.as_bytes())? else {
        return Ok(None);
    };
    if value.typeof_() != SEXPTYPE::STRSXP || value.len() != 1 {
        return Err(error(format!("invalid '{name}' argument")));
    }
    text(&value.try_string_elt(0)?.into_owned()?, args, Mode::Bytes).map(Some)
}
fn label_str(value: Option<&Bytes>) -> SexpResult<&str> {
    match value {
        None => Ok(""),
        Some(bytes) => std::str::from_utf8(&bytes.data).map_err(|_| error("invalid encoding name")),
    }
}

fn native() -> Codec {
    if crate::mainutils::platform::locale_is_utf8() {
        Codec::Utf8
    } else {
        Codec::Ascii
    }
}
fn codec(name: &str) -> Option<Codec> {
    if matches!(name, "" | "native.enc") {
        Some(native())
    } else {
        iconv_codec::lookup(name)
    }
}
fn read_raw(value: &Value, parent: &Value) -> SexpResult<Bytes> {
    let n = usize::try_from(value.len()).map_err(|_| allocation())?;
    let ticket = reserve(parent, n)?;
    let mut data = Vec::new();
    data.try_reserve_exact(n).map_err(|_| allocation())?;
    for i in 0..n {
        data.push(value.try_raw_elt(i as i64)?);
    }
    Ok(Bytes {
        data,
        _ticket: ticket,
    })
}
pub(super) fn convert(
    parent: &Value,
    source: &[u8],
    from: Codec,
    to: Codec,
    sub: Option<&[u8]>,
    translit: bool,
) -> SexpResult<Option<Bytes>> {
    let mut n = 0usize;
    if !iconv_codec::visit(source, from, to, sub, translit, |part| {
        n = n.checked_add(part.len()).ok_or_else(allocation)?;
        Ok(())
    })? {
        return Ok(None);
    }
    let ticket = reserve(parent, n)?;
    let mut data = Vec::new();
    data.try_reserve_exact(n).map_err(|_| allocation())?;
    let complete = iconv_codec::visit(source, from, to, sub, translit, |part| {
        data.extend_from_slice(part);
        Ok::<(), SexpError>(())
    })?;
    debug_assert!(complete && data.len() == n);
    Ok(Some(Bytes {
        data,
        _ticket: ticket,
    }))
}
fn copy_attributes(
    access: &RuntimeAccess,
    x: &Value,
    out: &mut SexpMut<'static>,
) -> SexpResult<()> {
    let domain = access.domain();
    let mut attributes = x.try_attrib()?.into_owned()?;
    let mut head = domain.nil();
    let mut tail = None;
    while !attributes.is_nil() {
        let value = attributes.try_car()?.into_owned()?;
        let tag = attributes.try_tag()?.into_owned()?;
        let next = access
            .allocator(&domain)?
            .pairlist_cell(&value, &domain.nil(), &tag)?;
        if let Some(previous) = tail {
            SexpMut::try_from_checked(previous)?.try_set_pairlist_cdr(&next)?;
        } else {
            head = next.clone();
        }
        tail = Some(next);
        attributes = attributes.try_cdr()?.into_owned()?;
    }
    out.try_set_attribute(&head)
}
pub(super) fn inventory(access: &RuntimeAccess) -> SexpResult<Value> {
    let names = iconv_codec::names();
    let mut out = vector(access, SEXPTYPE::STRSXP, names.len() as i64)?;
    for (i, name) in names.enumerate() {
        out.try_set_string_elt(
            i as i64,
            character(access, name.as_bytes(), TextEncoding::Native)?,
        )?;
    }
    Ok(out.freeze())
}
pub(super) fn evaluate(
    access: &RuntimeAccess,
    args: Value,
    na: Value,
    boolean: impl Fn(&Value) -> SexpResult<i32>,
) -> SexpResult<Value> {
    let x = argument(&args, 0, b"x")?.unwrap_or_else(|| access.domain().nil());
    if x.is_nil() {
        return inventory(access);
    }
    let from = label(&args, 1, "from")?;
    let to = label(&args, 2, "to")?;
    let from_name = label_str(from.as_ref())?;
    let to_name = label_str(to.as_ref())?;
    let sub_value = argument(&args, 3, b"sub")?;
    let sub = if let Some(value) = sub_value {
        // The public GNU wrapper coerces sub to character. The direct
        // evaluator entry also historically accepts the default logical NA.
        if value.typeof_() == SEXPTYPE::LGLSXP
            && value.len() == 1
            && value.try_logical_elt(0)? == NA_LOGICAL
        {
            None
        } else {
            if value.typeof_() != SEXPTYPE::STRSXP || value.len() != 1 {
                return Err(error("invalid 'sub' argument"));
            }
            let value = value.try_string_elt(0)?.into_owned()?;
            if value.is_na_string() {
                None
            } else {
                Some(text(&value, &args, Mode::Native)?)
            }
        }
    } else {
        None
    };
    let flag = |index, name, default| -> SexpResult<bool> {
        if let Some(value) = argument(&args, index, name)? {
            let flag = boolean(&value)?;
            access.require_active()?;
            if flag == NA_LOGICAL {
                return Err(error(format!(
                    "invalid '{}' argument",
                    std::str::from_utf8(name).expect("constant flag name")
                )));
            }
            Ok(flag != 0)
        } else {
            Ok(default)
        }
    };
    let mark = flag(4, b"mark", true)?;
    let raw = flag(5, b"toRaw", false)?;
    let list = x.typeof_() == SEXPTYPE::VECSXP;
    if !list && x.typeof_() != SEXPTYPE::STRSXP {
        return Err(error("'x' must be a character vector"));
    }
    let n = x.len();
    access.require_active()?;
    let mut out = vector(
        access,
        if raw {
            SEXPTYPE::VECSXP
        } else {
            SEXPTYPE::STRSXP
        },
        n,
    )?;
    copy_attributes(access, &x, &mut out)?;
    for i in 0..n {
        let value = if list {
            x.try_vector_elt(i)?
        } else {
            x.try_string_elt(i)?
        }
        .into_owned()?;
        access.require_active()?;
        if !raw {
            out.try_set_string_elt(i, na.clone())?;
        }
        if value.is_nil() || value.is_na_string() {
            continue;
        }
        if list && value.typeof_() != SEXPTYPE::RAWSXP {
            return Err(error(
                "'x' must be a character vector or a list of NULL or raw vectors",
            ));
        }
        let mut source_codec = codec(from_name);
        if !list && from_name.is_empty() {
            let flags = encoding::flags(&value)?;
            if flags.utf8 {
                source_codec = Some(Codec::Utf8)
            } else if flags.known {
                source_codec = Some(Codec::Latin1)
            }
        }
        let (Some(source_codec), Some(target)) = (source_codec, codec(to_name)) else {
            return Err(error(format!(
                "unsupported conversion from '{from_name}' to '{to_name}'"
            )));
        };
        let bytes = if list {
            read_raw(&value, &args)?
        } else {
            text(&value, &args, Mode::Bytes)?
        };
        let translit = to_name
            .split("//")
            .skip(1)
            .any(|s| s.eq_ignore_ascii_case("TRANSLIT"));
        let Some(bytes) = convert(
            &args,
            &bytes.data,
            source_codec,
            target,
            sub.as_ref().map(|b| b.data.as_slice()),
            translit,
        )?
        else {
            continue;
        };
        if raw {
            let mut result = vector(access, SEXPTYPE::RAWSXP, bytes.data.len() as i64)?;
            for (j, &b) in bytes.data.iter().enumerate() {
                result.try_set_raw_elt(j as i64, b)?;
            }
            out.try_set_vector_elt(i, result.freeze())?;
        } else {
            // GNU marks by the requested spelling, independently of codec
            // admission. ISO-8859-1 and utf-8 are accepted but stay native.
            let tag = if !mark {
                TextEncoding::Native
            } else if to_name.eq_ignore_ascii_case("latin1")
                || to_name.eq_ignore_ascii_case("ISO_8859-1")
            {
                TextEncoding::Latin1
            } else if to_name == "UTF-8"
                || to_name.eq_ignore_ascii_case("UTF8")
                || (to_name.is_empty() && native() == Codec::Utf8)
            {
                TextEncoding::Utf8
            } else {
                TextEncoding::Native
            };
            if bytes.data.contains(&0) {
                let diagnostic = super::iconv_diagnostic::format(
                    &args,
                    &bytes.data,
                    tag,
                    crate::mainutils::platform::locale_is_utf8(),
                )?;
                return Err(error(diagnostic.into_message()));
            }
            out.try_set_string_elt(i, character(access, &bytes.data, tag)?)?;
        }
    }
    let out = out.freeze();
    let node = out.allocation()?;
    let heap = node.heap_identity();
    let mut header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
    let original = x.header().sxpinfo;
    header.sxpinfo.set_obj(original.obj());
    let s4 = crate::sexp::accessors::S4_OBJECT_MASK;
    header
        .sxpinfo
        .set_gp((header.sxpinfo.gp() & !s4) | (original.gp() & s4));
    heap.replace_node(node, header)
        .ok_or(SexpError::StaleAllocation)?;
    access.require_active()?;
    Ok(out)
}

#[cfg(test)]
mod nul_diagnostic_tests {
    use super::*;
    fn message(bytes: &[u8]) -> String {
        let session = crate::sexp::RSession::new_for_gc_tests();
        session.with_active(|| {
            let parent = session
                .owner_token()
                .unwrap()
                .node_factory()
                .strings(&["parent"])
                .unwrap();
            super::super::iconv_diagnostic::format(&parent, bytes, TextEncoding::Utf8, true)
                .unwrap()
                .into_message()
        })
    }
    #[test]
    fn iconv_nul_diagnostic_matches_pinned_gnu_999_byte_bound() {
        let error = message(&[0; 10000]);
        assert_eq!(error.len(), 999);
        assert!(error.starts_with("embedded nul in string: '"));
    }
    #[test]
    fn iconv_nul_diagnostic_preserves_printable_utf8() {
        assert_eq!(message(b"\xc3\xa9\0"), "embedded nul in string: 'é\\0'");
    }
    #[test]
    fn iconv_nul_diagnostic_quote_zero_keeps_literal_apostrophes() {
        assert_eq!(message(b"\"'\\\0"), r#"embedded nul in string: '"'\\\0'"#);
    }
    #[test]
    fn iconv_nul_diagnostic_short_ascii_nul_control() {
        assert_eq!(message(b"A\0B"), "embedded nul in string: 'A\\0B'");
    }
}
