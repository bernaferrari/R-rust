#![forbid(unsafe_code)]
//! Simple casing over sealed original-heap bytes and admitted output storage.

use crate::sexp::{
    diagnostic::{Diagnostic, EvaluationError},
    encoding::{self, TextEncoding},
    ffi::SEXPTYPE,
    memory::WorkspaceReservation,
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::RuntimeAccess,
    payload::PayloadLease,
};

fn error(message: impl Into<String>) -> SexpError {
    SexpError::EvaluationFailed {
        message: message.into(),
    }
}
fn allocation() -> SexpError {
    SexpError::AllocationFailed {
        object: "case conversion workspace",
    }
}

struct Source {
    payload: PayloadLease,
    len: usize,
    encoding: TextEncoding,
}
impl Source {
    fn read(value: &Sexp<'_>) -> SexpResult<Self> {
        let node = value.allocation()?;
        let heap = node.heap_identity();
        let header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
        let len = usize::try_from(value.try_char_len()?).map_err(|_| allocation())?;
        let payload = heap.payload_lease(node).ok_or(SexpError::MissingData {
            sexptype: SEXPTYPE::CHARSXP,
        })?;
        if !payload.is_immutable() || len >= payload.capacity() || payload.byte_elt(len) != Some(0)
        {
            return Err(SexpError::MissingData {
                sexptype: SEXPTYPE::CHARSXP,
            });
        }
        let len = (0..len)
            .find(|&i| payload.byte_elt(i) == Some(0))
            .unwrap_or(len);
        Ok(Self {
            payload,
            len,
            encoding: TextEncoding::from_gp(header.sxpinfo.gp()),
        })
    }
    fn byte(&self, i: usize) -> u8 {
        self.payload
            .byte_elt(i)
            .expect("validated sealed character payload")
    }
    fn visit(
        &self,
        upper: bool,
        native_utf8: bool,
        wide: bool,
        index: i64,
        mut emit: impl FnMut(&[u8]) -> SexpResult<()>,
    ) -> SexpResult<()> {
        if self.encoding == TextEncoding::Bytes {
            return Err(error(
                "translating strings with \"bytes\" encoding is not allowed",
            ));
        }
        let unicode = self.encoding != TextEncoding::Native || native_utf8;
        let mut i = 0;
        while i < self.len {
            let b = self.byte(i);
            if !unicode {
                if wide && b >= 128 {
                    return Err(error(format!("invalid multibyte string {}", index + 1)));
                }
                emit(&[if upper {
                    b.to_ascii_uppercase()
                } else {
                    b.to_ascii_lowercase()
                }])?;
                i += 1;
                continue;
            }
            let ch = if self.encoding == TextEncoding::Latin1 {
                i += 1;
                let Some(ch) = encoding::declared_latin1::character(b) else {
                    let hex = if upper {
                        b"0123456789ABCDEF"
                    } else {
                        b"0123456789abcdef"
                    };
                    emit(&[b'<', hex[(b >> 4) as usize], hex[(b & 15) as usize], b'>'])?;
                    continue;
                };
                ch
            } else {
                let width = match b {
                    0..=0x7f => 1,
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf4 => 4,
                    _ => 0,
                };
                let invalid = || {
                    if self.encoding == TextEncoding::Utf8 {
                        SexpError::InvalidUtf8
                    } else {
                        error(format!("invalid multibyte string {}", index + 1))
                    }
                };
                if width == 0 || width > self.len - i {
                    return Err(invalid());
                }
                let mut raw = [0; 4];
                for (j, out) in raw[..width].iter_mut().enumerate() {
                    *out = self.byte(i + j);
                }
                let ch = std::str::from_utf8(&raw[..width])
                    .map_err(|_| invalid())?
                    .chars()
                    .next()
                    .ok_or_else(invalid)?;
                i += width;
                ch
            };
            let ch = if upper {
                super::rlocale::simple_upper(ch)
            } else {
                super::rlocale::simple_lower(ch)
            };
            let mut encoded = [0; 4];
            emit(ch.encode_utf8(&mut encoded).as_bytes())?;
        }
        Ok(())
    }
}

struct Output {
    bytes: Vec<u8>,
    // The bytes deallocate before their original interpreter's charge releases.
    _admission: Option<WorkspaceReservation>,
}
fn convert(
    source: &Source,
    parent: &Sexp<'_>,
    upper: bool,
    native_utf8: bool,
    wide: bool,
    index: i64,
) -> SexpResult<Output> {
    let mut len = 0usize;
    source.visit(upper, native_utf8, wide, index, |part| {
        len = len.checked_add(part.len()).ok_or_else(allocation)?;
        Ok(())
    })?;
    let ledger = parent.allocation()?.heap_identity().byte_ledger();
    let mut admission = if len == 0 {
        None
    } else {
        Some(ledger.reserve(len).ok_or_else(allocation)?)
    };
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| allocation())?;
    if let Some(ticket) = &mut admission {
        ticket.try_resize(bytes.capacity()).ok_or_else(allocation)?;
    }
    source.visit(upper, native_utf8, wide, index, |part| {
        bytes.extend_from_slice(part);
        Ok(())
    })?;
    Ok(Output {
        bytes,
        _admission: admission,
    })
}

pub(crate) fn transform(
    access: &RuntimeAccess,
    x: Sexp<'static>,
    na: Sexp<'static>,
    upper: bool,
) -> Result<Sexp<'static>, EvaluationError> {
    let domain = access.domain();
    domain.link(&x)?;
    domain.link(&na)?;
    if x.typeof_() != SEXPTYPE::STRSXP {
        return Err(error("non-character argument").into());
    }
    let native_utf8 = super::platform::locale_is_utf8();
    let mut wide = native_utf8;
    for i in 0..x.len() {
        let el = x.try_string_elt(i)?;
        if !el.is_na_string() && encoding::flags(&el)?.known {
            wide = true;
        }
    }
    // Retain the original list before the first allocation can notify user code.
    let attributes = x.try_attrib()?.into_owned()?;
    let object = x.is_object();
    let s4 = x.header().sxpinfo.gp() & crate::sexp::accessors::S4_OBJECT_MASK;
    let mut out = SexpMut::try_from_checked(
        access
            .allocator(&domain)?
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, x.len())))?,
    )?;
    for i in 0..x.len() {
        let el = x.try_string_elt(i)?.into_owned()?;
        if el.is_na_string() {
            out.try_set_string_elt(i, na.clone())?;
            continue;
        }
        let source = Source::read(&el)?;
        let converted = match convert(&source, &x, upper, native_utf8, wide, i) {
            Err(SexpError::InvalidUtf8) if source.encoding == TextEncoding::Utf8 => {
                const PREFIX: &[u8] = b"invalid input '";
                const SUFFIX: &[u8] = b"' in 'utf8towcs'";
                let length = PREFIX
                    .len()
                    .checked_add(source.len)
                    .and_then(|n| n.checked_add(SUFFIX.len()))
                    .ok_or_else(allocation)?;
                let limit = access.with_native(|original| {
                    let limit = crate::mainutils::errors::diagnostic_message_limit();
                    original.require_active()?;
                    Ok(limit)
                })?;
                let diagnostic = Diagnostic::new(access, &el, length, limit, native_utf8, |i| {
                    if i < PREFIX.len() {
                        PREFIX[i]
                    } else if i - PREFIX.len() < source.len {
                        source.byte(i - PREFIX.len())
                    } else {
                        SUFFIX[i - PREFIX.len() - source.len]
                    }
                })?;
                return Err(EvaluationError::Diagnostic(diagnostic));
            }
            result => result?,
        };
        let tag = if source.encoding == TextEncoding::Latin1 {
            TextEncoding::Utf8
        } else {
            source.encoding
        };
        let character = access
            .allocator(&domain)?
            .allocate(|a| encoding::allocate_character(a, &converted.bytes, tag))?;
        out.try_set_string_elt(i, character)?;
    }
    // GNU shallow-copies the attribute spine while sharing attribute values.
    let mut current = attributes;
    let mut head = domain.nil();
    let mut tail = None;
    while !current.is_nil() {
        let value = current.try_car()?.into_owned()?;
        let tag = current.try_tag()?.into_owned()?;
        let next = access
            .allocator(&domain)?
            .pairlist_cell(&value, &domain.nil(), &tag)?;
        if let Some(previous) = tail {
            SexpMut::try_from_checked(previous)?.try_set_pairlist_cdr(&next)?;
        } else {
            head = next.clone();
        }
        tail = Some(next);
        current = current.try_cdr()?.into_owned()?;
    }
    out.try_set_attribute(&head)?;
    let out = out.freeze();
    let node = out.allocation()?;
    let heap = node.heap_identity();
    let mut header = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
    header.sxpinfo.set_obj(object);
    header
        .sxpinfo
        .set_gp((header.sxpinfo.gp() & !crate::sexp::accessors::S4_OBJECT_MASK) | s4);
    heap.replace_node(node, header)
        .ok_or(SexpError::StaleAllocation)?;
    Ok(out)
}

#[cfg(test)]
mod tests;
