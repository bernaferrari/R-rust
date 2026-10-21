#![forbid(unsafe_code)]
//! Checked, owned translation. No borrowed C span survives a callback.
use super::{ASCII_MASK, BYTES_MASK, LATIN1_MASK, UTF8_MASK};
use crate::sexp::{
    diagnostic::{EvaluationError, PreparedDiagnostic},
    ffi::SEXPTYPE,
    memory::TransientReservation,
    object::{Sexp, SexpError, SexpResult},
    owner::RuntimeAccess,
    payload::PayloadLease,
};

#[derive(Clone, Copy)]
pub(crate) enum Mode {
    Native,
    Utf8,
    Bytes,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct CharacterFlags {
    pub(crate) utf8: bool,
    pub(crate) bytes: bool,
    pub(crate) known: bool,
    pub(crate) ascii: bool,
}
fn failure() -> SexpError {
    SexpError::AllocationFailed {
        object: "character translation workspace",
    }
}
pub(crate) fn flags(value: &Sexp<'_>) -> SexpResult<CharacterFlags> {
    if value.is_na_string() {
        return Ok(CharacterFlags {
            ascii: true,
            ..Default::default()
        });
    }
    let node = value.allocation()?;
    let h = node
        .heap_identity()
        .node_snapshot(node)
        .ok_or(SexpError::StaleAllocation)?;
    if h.sxpinfo.type_of() != SEXPTYPE::CHARSXP {
        return Err(SexpError::TypeMismatch {
            expected: "character scalar",
            actual: h.sxpinfo.type_of(),
        });
    }
    let gp = h.sxpinfo.gp();
    Ok(CharacterFlags {
        utf8: gp & UTF8_MASK != 0,
        bytes: gp & BYTES_MASK != 0,
        known: gp & (UTF8_MASK | LATIN1_MASK) != 0,
        ascii: gp & ASCII_MASK != 0,
    })
}
struct Source {
    payload: Option<PayloadLease>,
    len: usize,
    gp: u16,
}
impl Source {
    fn read(value: &Sexp<'_>, parent: &Sexp<'_>) -> SexpResult<Self> {
        let anchor = parent.allocation()?;
        if value.is_na_string() {
            return Ok(Self {
                payload: None,
                len: 2,
                gp: ASCII_MASK,
            });
        }
        let node = value.allocation()?;
        let heap = node.heap_identity();
        if !heap.same_domain(&anchor.heap_identity()) {
            return Err(SexpError::HeapDomainMismatch);
        }
        let h = heap.node_snapshot(node).ok_or(SexpError::StaleAllocation)?;
        if h.sxpinfo.type_of() != SEXPTYPE::CHARSXP {
            return Err(SexpError::TypeMismatch {
                expected: "character scalar",
                actual: h.sxpinfo.type_of(),
            });
        }
        let len = usize::try_from(value.try_char_len()?).map_err(|_| failure())?;
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
            payload: Some(payload),
            len,
            gp: h.sxpinfo.gp(),
        })
    }
    fn byte(&self, i: usize) -> u8 {
        self.payload.as_ref().map_or_else(
            || b"NA"[i],
            |p| p.byte_elt(i).expect("validated character payload"),
        )
    }
    fn conversion(&self, mode: Mode) -> SexpResult<Conversion> {
        if matches!(mode, Mode::Bytes) {
            return Ok(Conversion::Copy);
        }
        if self.gp & BYTES_MASK != 0 {
            return Err(SexpError::EvaluationFailed {
                message: "translating strings with \"bytes\" encoding is not allowed".into(),
            });
        }
        let utf8 = matches!(mode, Mode::Utf8) || crate::mainutils::platform::locale_is_utf8();
        Ok(if self.gp & LATIN1_MASK != 0 {
            if utf8 {
                Conversion::LatinUtf8
            } else {
                Conversion::LatinEscape
            }
        } else if !utf8 && self.gp & UTF8_MASK != 0 {
            Conversion::Utf8Escape
        } else {
            Conversion::Copy
        })
    }
    fn visit(
        &self,
        conversion: Conversion,
        mut emit: impl FnMut(&[u8]) -> SexpResult<()>,
    ) -> SexpResult<()> {
        let mut i = 0;
        while i < self.len {
            let b = self.byte(i);
            let mut buf = [0u8; 16];
            let n = match conversion {
                Conversion::LatinUtf8 if b >= 128 => {
                    let mut encoded = [0; 4];
                    let text = super::declared_latin1::encode(b, &mut encoded);
                    buf[..text.len()].copy_from_slice(text.as_bytes());
                    text.len()
                }
                Conversion::LatinEscape if b >= 128 => {
                    buf[..4].copy_from_slice(&[
                        b'<',
                        b"0123456789abcdef"[(b >> 4) as usize],
                        b"0123456789abcdef"[(b & 15) as usize],
                        b'>',
                    ]);
                    4
                }
                Conversion::Utf8Escape if b >= 128 => {
                    let width = match b {
                        0xc2..=0xdf => 2,
                        0xe0..=0xef => 3,
                        0xf0..=0xf4 => 4,
                        _ => return Err(SexpError::InvalidUtf8),
                    };
                    if i + width > self.len {
                        return Err(SexpError::InvalidUtf8);
                    }
                    let mut raw = [0; 4];
                    for (j, out) in raw[..width].iter_mut().enumerate() {
                        *out = self.byte(i + j);
                    }
                    let ch = std::str::from_utf8(&raw[..width])
                        .map_err(|_| SexpError::InvalidUtf8)?
                        .chars()
                        .next()
                        .ok_or(SexpError::InvalidUtf8)? as u32;
                    i += width - 1;
                    let digits = if ch > 0xffff {
                        if ch > 0xfffff { 6 } else { 5 }
                    } else {
                        4
                    };
                    buf[..3].copy_from_slice(b"<U+");
                    for j in 0..digits {
                        buf[3 + j] =
                            b"0123456789ABCDEF"[((ch >> (4 * (digits - 1 - j))) & 15) as usize];
                    }
                    buf[3 + digits] = b'>';
                    4 + digits
                }
                _ => {
                    buf[0] = b;
                    1
                }
            };
            emit(&buf[..n])?;
            i += 1;
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
enum Conversion {
    Copy,
    LatinUtf8,
    LatinEscape,
    Utf8Escape,
}
pub(crate) struct TranslatedBytes {
    pub(crate) data: Vec<u8>,
    pub(crate) reservation: TransientReservation,
}
fn copy(source: &Source, mode: Mode, parent: &Sexp<'_>, nul: bool) -> SexpResult<TranslatedBytes> {
    let conversion = source.conversion(mode)?;
    let mut length = usize::from(nul);
    source.visit(conversion, |bytes| {
        length = length.checked_add(bytes.len()).ok_or_else(failure)?;
        Ok(())
    })?;
    let node = parent.allocation()?;
    let mut reservation = node
        .heap_identity()
        .byte_ledger()
        .reserve(length)
        .ok_or_else(failure)?;
    let mut data = Vec::new();
    data.try_reserve_exact(length).map_err(|_| failure())?;
    reservation
        .try_resize(data.capacity())
        .ok_or_else(failure)?;
    source.visit(conversion, |bytes| {
        data.extend_from_slice(bytes);
        Ok(())
    })?;
    if nul {
        data.push(0);
    }
    Ok(TranslatedBytes {
        data,
        reservation: reservation.into_fixed(),
    })
}

fn path_translation_error(
    source: &Source,
    parent: &Sexp<'_>,
    utf8: bool,
    limit: usize,
) -> SexpResult<PreparedDiagnostic> {
    // GNU bounds the native rendering at 255 bytes, or a valid 252-byte
    // prefix followed by an ellipsis. No full translated copy is needed.
    let mut body = [0u8; 256];
    let mut copied = 0;
    let mut total = 0usize;
    source.visit(Conversion::LatinUtf8, |chunk| {
        let text = std::str::from_utf8(chunk).map_err(|_| SexpError::InvalidUtf8)?;
        for ch in text.chars() {
            let mut encoded = [0u8; 12];
            let part = if utf8 || ch.is_ascii() {
                ch.encode_utf8(&mut encoded).as_bytes()
            } else {
                // reEnc(..., CE_UTF8, CE_NATIVE, 2) substitutes one dot
                // per unrepresentable input byte in the ASCII C profile.
                let width = ch.len_utf8();
                encoded[..width].fill(b'.');
                &encoded[..width]
            };
            total = total.checked_add(part.len()).ok_or_else(failure)?;
            let count = part.len().min(body.len() - copied);
            body[copied..copied + count].copy_from_slice(&part[..count]);
            copied += count;
        }
        Ok(())
    })?;
    let truncated = total > 255;
    let mut length = if truncated { 252 } else { total };
    while std::str::from_utf8(&body[..length]).is_err() {
        length -= 1;
    }
    let body = std::str::from_utf8(&body[..length]).map_err(|_| SexpError::InvalidUtf8)?;
    const PREFIX: &str = "unable to translate '";
    const SUFFIX: &str = "' to UTF-8";
    let size = (PREFIX.len() + body.len() + usize::from(truncated) * 3 + SUFFIX.len()).min(limit);
    let mut ticket = parent
        .allocation()?
        .heap_identity()
        .byte_ledger()
        .reserve(size)
        .ok_or_else(failure)?;
    let mut message = String::new();
    message.try_reserve_exact(size).map_err(|_| failure())?;
    ticket.try_resize(message.capacity()).ok_or_else(failure)?;
    'message: for part in [PREFIX, body, if truncated { "..." } else { "" }, SUFFIX] {
        for ch in part.chars() {
            if message.len() + ch.len_utf8() > size {
                break 'message;
            }
            message.push(ch);
        }
    }
    PreparedDiagnostic::from_admitted_string(message, ticket)
}

/// GNU's strict path conversion rejects undefined declared-Latin1 bytes.
/// Keep the selected original character rooted through error publication.
pub(crate) fn validate_utf8_path(
    access: &RuntimeAccess,
    value: &Sexp<'_>,
    parent: &Sexp<'_>,
) -> Result<(), EvaluationError> {
    access.require_active()?;
    access.domain().link(value)?;
    access.domain().link(parent)?;
    let source = Source::read(value, parent)?;
    if matches!(source.conversion(Mode::Utf8)?, Conversion::LatinUtf8)
        && (0..source.len).any(|i| super::declared_latin1::character(source.byte(i)).is_none())
    {
        let (utf8, limit) = access.with_native(|original| {
            let result = (
                crate::mainutils::platform::locale_is_utf8(),
                crate::mainutils::errors::diagnostic_message_limit(),
            );
            original.require_active()?;
            Ok(result)
        })?;
        let prepared = path_translation_error(&source, parent, utf8, limit)?;
        return Err(EvaluationError::Diagnostic(
            prepared.into_owned(access, parent)?,
        ));
    }
    access.require_active()?;
    Ok(())
}

pub(crate) fn translate(
    value: &Sexp<'_>,
    mode: Mode,
    parent: &Sexp<'_>,
) -> SexpResult<TranslatedBytes> {
    copy(&Source::read(value, parent)?, mode, parent, false)
}
/// Keeps the actual sealed payload alive for a native read; converted data owns
/// its admission ticket. Compatible strings require no extra allocation.
pub(crate) enum NulText {
    Payload(PayloadLease),
    StaticNa,
    Converted(TranslatedBytes),
}
impl NulText {
    /// Copy one byte while retaining the sealed payload or converted buffer.
    pub(crate) fn byte(&self, index: usize) -> Option<u8> {
        match self {
            Self::Payload(payload) => payload.byte_elt(index),
            Self::StaticNa => b"NA\0".get(index).copied(),
            Self::Converted(converted) => converted.data.get(index).copied(),
        }
    }

    pub(crate) fn new(value: &Sexp<'_>, mode: Mode, parent: &Sexp<'_>) -> SexpResult<Self> {
        let source = Source::read(value, parent)?;
        if matches!(source.conversion(mode)?, Conversion::Copy) {
            return Ok(source.payload.map_or(Self::StaticNa, Self::Payload));
        }
        Ok(Self::Converted(copy(&source, mode, parent, true)?))
    }
    pub(crate) fn as_ptr(&self) -> *const std::ffi::c_char {
        match self {
            Self::Payload(p) => p.native_projection().cast(),
            Self::StaticNa => c"NA".as_ptr(),
            Self::Converted(c) => c.data.as_ptr().cast(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{
        RSession,
        encoding::{TextEncoding, allocate_character},
        memory::ArenaBudget,
    };

    fn character(session: &RSession, bytes: &[u8], encoding: TextEncoding) -> Sexp<'static> {
        session.with_active(|| {
            session
                .owner_token()
                .unwrap()
                .node_factory()
                .allocate(|arena| allocate_character(arena, bytes, encoding))
                .unwrap()
                .into_owned()
                .unwrap()
        })
    }
    fn total(session: &RSession) -> usize {
        session.with_active(|| {
            session
                .owner_token()
                .unwrap()
                .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
                .unwrap()
        })
    }
    fn limit(session: &RSession, budget: ArenaBudget) {
        session.with_active(|| {
            session
                .owner_token()
                .unwrap()
                .with_arena(|a| a.set_budget(budget))
                .unwrap()
        });
    }
    #[test]
    fn declared_latin1_translation_admits_actual_expansion_and_recovers() {
        let arena = RSession::new_for_gc_tests();
        let value = character(&arena, &[0x80, 0x81], TextEncoding::Latin1);
        let smaller = character(&arena, &[0x80], TextEncoding::Latin1);
        let before = total(&arena);
        limit(&arena, ArenaBudget::new(before + 6, 100));
        assert!(matches!(
            translate(&value, Mode::Utf8, &value),
            Err(SexpError::AllocationFailed { .. })
        ));
        assert_eq!(total(&arena), before);
        // Reuse the unchanged limit: the independent three-byte copy fits.
        let translated = translate(&smaller, Mode::Utf8, &smaller).unwrap();
        assert_eq!(translated.data, "€".as_bytes());
        assert_eq!(translated.reservation.bytes(), translated.data.capacity());
        assert_eq!(total(&arena), before + translated.data.capacity());
        drop(translated);
        assert_eq!(total(&arena), before);
    }

    #[test]
    fn declared_latin1_translation_retains_original_charge_after_facade_teardown() {
        let arena = RSession::new_for_gc_tests();
        let value = character(&arena, &[0x80, 0x81], TextEncoding::Latin1);
        let ledger = value.allocation().unwrap().heap_identity().byte_ledger();
        let translated = translate(&value, Mode::Utf8, &value).unwrap();
        assert_eq!(translated.data, "€<81>".as_bytes());
        let capacity = translated.data.capacity();
        drop(value);
        drop(arena);
        let other = RSession::new_for_gc_tests();
        let before = total(&other);
        assert_eq!(ledger.workspace_bytes_for_test(), capacity);
        drop(translated);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(total(&other), before);
    }

    #[test]
    fn declared_latin1_path_returned_error_retains_original_formatting_charge() {
        let arena = RSession::new_for_gc_tests();
        let value = character(&arena, &[0x81], TextEncoding::Latin1);
        let ledger = value.allocation().unwrap().heap_identity().byte_ledger();
        let source = Source::read(&value, &value).unwrap();
        let diagnostic = path_translation_error(&source, &value, true, 999).unwrap();
        let capacity = diagnostic.capacity();
        assert_eq!(diagnostic.as_str(), "unable to translate '<81>' to UTF-8");
        assert_eq!(
            ledger.workspace_bytes_for_test(),
            capacity,
            "returned path diagnostic must keep its original physical charge"
        );
        drop(source);
        drop(value);
        drop(arena);
        let other = RSession::new_for_gc_tests();
        let before = total(&other);
        assert_eq!(ledger.workspace_bytes_for_test(), capacity);
        drop(diagnostic);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(total(&other), before);
    }

    #[test]
    fn owned_path_diagnostic_foreign_selection_rejects_before_current_allocation() {
        let original = RSession::new_for_gc_tests();
        let value = character(&original, &[0x81], TextEncoding::Latin1);
        let ledger = value.allocation().unwrap().heap_identity().byte_ledger();
        let other = RSession::new_for_gc_tests();
        let parent = character(&other, b"anchor", TextEncoding::Native);
        let before = total(&other);
        other.with_active(|| {
            let owner = other.owner_token().unwrap();
            let error = crate::sexp::owner::with_runtime(&owner.weak_owner().unwrap(), |access| validate_utf8_path(access, &value, &parent)).unwrap().unwrap_err();
            assert!(matches!(error, EvaluationError::Access(SexpError::UnownedPointer { address }) if address == value.as_raw().addr()));
        });
        assert_eq!(total(&other), before);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    }

    #[test]
    fn declared_latin1_path_errors_are_bounded_before_workspace_admission() {
        let arena = RSession::new_for_gc_tests();
        let mut bytes = vec![0xe9; 127];
        bytes.extend_from_slice(&[0x80, 0x81, 0x8c]);
        let value = character(&arena, &bytes, TextEncoding::Latin1);
        let source = Source::read(&value, &value).unwrap();
        let before = total(&arena);
        limit(&arena, ArenaBudget::new(before + 285, 100));
        assert!(matches!(
            path_translation_error(&source, &value, true, 999),
            Err(SexpError::AllocationFailed { .. })
        ));
        assert_eq!(total(&arena), before);
        limit(&arena, ArenaBudget::new(before + 286, 100));
        let diagnostic = path_translation_error(&source, &value, true, 999).unwrap();
        assert_eq!(
            diagnostic.as_str(),
            format!("unable to translate '{}...' to UTF-8", "é".repeat(126))
        );
        assert_eq!(diagnostic.as_str().len(), 286);
        assert_eq!(total(&arena), before + 286);
        drop(diagnostic);
        assert_eq!(total(&arena), before);
        let diagnostic = path_translation_error(&source, &value, false, 999).unwrap();
        assert_eq!(
            diagnostic.as_str(),
            format!("unable to translate '{}' to UTF-8", ".".repeat(255))
        );
        assert_eq!(total(&arena), before + diagnostic.capacity());
        drop(diagnostic);
        assert_eq!(total(&arena), before);
    }

    #[test]
    fn owned_encoding_translation_charges_expansion_and_releases_on_drop() {
        let arena = RSession::new_for_gc_tests();
        let value = character(&arena, &[0xe9, 0xff], TextEncoding::Latin1);
        let before = total(&arena);
        limit(&arena, ArenaBudget::new(before + 3, 100));
        assert!(matches!(
            translate(&value, Mode::Utf8, &value),
            Err(SexpError::AllocationFailed { .. })
        ));
        assert_eq!(total(&arena), before);
        limit(&arena, ArenaBudget::new(before + 4, 100));
        let translated = translate(&value, Mode::Utf8, &value).unwrap();
        assert_eq!(translated.data, [0xc3, 0xa9, 0xc3, 0xbf]);
        assert_eq!(total(&arena), before + 4);
        drop(translated);
        assert_eq!(total(&arena), before);
        assert!(NulText::new(&value, Mode::Utf8, &value).is_err());
        limit(&arena, ArenaBudget::new(before + 5, 100));
        let native = NulText::new(&value, Mode::Utf8, &value).unwrap();
        let NulText::Converted(converted) = &native else {
            panic!("Latin1 requires conversion")
        };
        assert_eq!(converted.data, [0xc3, 0xa9, 0xc3, 0xbf, 0]);
        assert_eq!(total(&arena), before + 5);
        drop(native);
        assert_eq!(total(&arena), before);
    }
    #[test]
    fn owned_encoding_translation_rejects_foreign_budget_anchor() {
        let first = RSession::new_for_gc_tests();
        let second = RSession::new_for_gc_tests();
        let value = character(&first, b"same", TextEncoding::Native);
        let anchor = character(&second, b"same", TextEncoding::Native);
        assert!(matches!(
            translate(&value, Mode::Utf8, &anchor),
            Err(SexpError::HeapDomainMismatch)
        ));
    }
    #[test]
    fn owned_encoding_translation_byte_mode_dominates_and_text_modes_reject_bytes() {
        let arena = RSession::new_for_gc_tests();
        let value = character(&arena, &[0xff, 0xe9], TextEncoding::Bytes);
        assert_eq!(
            translate(&value, Mode::Bytes, &value).unwrap().data,
            [0xff, 0xe9]
        );
        for mode in [Mode::Native, Mode::Utf8] {
            assert!(
                matches!(translate(&value, mode, &value), Err(SexpError::EvaluationFailed { message }) if message == "translating strings with \"bytes\" encoding is not allowed")
            );
        }
    }
    #[test]
    fn owned_encoding_native_projection_retains_original_sealed_storage_and_charge() {
        let arena = RSession::new_for_gc_tests();
        let value = character(&arena, "é".as_bytes(), TextEncoding::Utf8);
        let ledger = value.allocation().unwrap().heap_identity().byte_ledger();
        let original = value
            .allocation()
            .unwrap()
            .heap_identity()
            .payload_lease(value.allocation().unwrap())
            .unwrap()
            .native_projection();
        let guard = NulText::new(&value, Mode::Utf8, &value).unwrap();
        assert_eq!(guard.as_ptr().cast_mut().cast::<u8>(), original);
        drop(value);
        drop(arena);
        let NulText::Payload(payload) = &guard else {
            panic!("compatible text must retain payload")
        };
        assert_eq!(payload.byte_elt(0), Some(0xc3));
        assert_eq!(payload.byte_elt(1), Some(0xa9));
        assert_eq!(payload.byte_elt(2), Some(0));
        assert_eq!(ledger.allocated_bytes(), payload.storage_bytes());
        drop(guard);
        assert_eq!(ledger.allocated_bytes(), 0);
    }
    #[test]
    fn owned_encoding_native_c_profile_escapes_declared_text_only() {
        let arena = RSession::new_for_gc_tests();
        let latin = character(&arena, &[b'x', 0xe9], TextEncoding::Latin1);
        let unicode = character(&arena, "é😀".as_bytes(), TextEncoding::Utf8);
        for (value, conversion, expected) in [
            (&latin, Conversion::LatinEscape, b"x<e9>".as_slice()),
            (
                &unicode,
                Conversion::Utf8Escape,
                b"<U+00E9><U+1F600>".as_slice(),
            ),
        ] {
            let source = Source::read(value, value).unwrap();
            let mut bytes = Vec::new();
            source
                .visit(conversion, |chunk| {
                    bytes.extend_from_slice(chunk);
                    Ok(())
                })
                .unwrap();
            assert_eq!(bytes, expected);
        }
    }
    #[test]
    fn owned_encoding_constructor_preserves_requested_tag_and_ascii_unknown() {
        let arena = RSession::new_for_gc_tests();
        for encoding in [
            TextEncoding::Native,
            TextEncoding::Utf8,
            TextEncoding::Latin1,
            TextEncoding::Bytes,
        ] {
            let value = character(&arena, "é".as_bytes(), encoding);
            assert_eq!(
                super::super::TextEncoding::from_gp(value.header().sxpinfo.gp()),
                encoding
            );
            let ascii = character(&arena, b"plain", encoding);
            assert_eq!(
                super::super::TextEncoding::from_gp(ascii.header().sxpinfo.gp()),
                TextEncoding::Native
            );
        }
    }
    #[test]
    fn owned_path_diagnostic_lowered_original_cap_denies_canonical_publication() {
        let arena = RSession::new_for_gc_tests();
        let value = character(&arena, &[0x81], TextEncoding::Latin1);
        let source = Source::read(&value, &value).unwrap();
        let prepared = path_translation_error(&source, &value, true, 999).unwrap();
        let ledger = value.allocation().unwrap().heap_identity().byte_ledger();
        assert_eq!(ledger.workspace_bytes_for_test(), prepared.capacity());
        arena.with_active(|| {
            let owner = arena.owner_token().unwrap();
            owner
                .with_arena(|a| a.set_budget(ArenaBudget::new(1, 0)))
                .unwrap();
            let error = crate::sexp::owner::with_runtime(&owner.weak_owner().unwrap(), |access| {
                prepared.into_owned(access, &value)
            })
            .unwrap();
            assert!(matches!(error, Err(SexpError::AllocationFailed { .. })));
            assert_eq!(ledger.workspace_bytes_for_test(), 0);
            assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), 1);
            assert!(value.try_char_eq(&[0x81]).unwrap());
        });
    }
}
