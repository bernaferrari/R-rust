//! Bounded UTF-8 decoding for the portable charClass bridge.
#![forbid(unsafe_code)]

use crate::sexp::{
    encoding::{ASCII_MASK, BYTES_MASK, LATIN1_MASK, UTF8_MASK},
    ffi::{NodeBody, SEXPTYPE},
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::StoredOwner,
    payload::PayloadLease,
};

enum Bytes {
    Missing,
    Empty,
    Payload(PayloadLease),
}

pub(super) enum ClassError<'s> {
    Access(SexpError),
    InvalidClassArgument,
    InvalidClass(Sexp<'s>),
    InvalidCharacterArgument,
}

impl From<SexpError> for ClassError<'_> {
    fn from(error: SexpError) -> Self {
        Self::Access(error)
    }
}

pub(super) enum Classification<'s> {
    Complete(Sexp<'s>),
    NeedsIntegerCoercion {
        input: Sexp<'s>,
        descriptor: std::num::NonZeroU32,
    },
}

/// Validate and retain both arguments before any element callback. The
/// adapter performs R's numeric coercion only for the indicated branch.
pub(super) fn prepare<'s>(
    owner: StoredOwner<'s>,
    input: Sexp<'s>,
    class: Sexp<'s>,
) -> Result<Classification<'s>, ClassError<'s>> {
    owner.require_active()?;
    let factory = owner.node_factory()?;
    factory.link(&input)?;
    factory.link(&class)?;
    if class.typeof_() != SEXPTYPE::STRSXP || class.len() != 1 {
        return Err(ClassError::InvalidClassArgument);
    }
    let class_scalar = class.try_string_elt(0);
    owner.require_active()?;
    let class_scalar = class_scalar?;
    let descriptor = class_descriptor(owner.clone(), class_scalar.clone())?
        .ok_or(ClassError::InvalidClass(class_scalar))?;
    if input.typeof_() == SEXPTYPE::STRSXP {
        if input.len() != 1 {
            return Err(ClassError::InvalidCharacterArgument);
        }
        let scalar = input.try_string_elt(0);
        owner.require_active()?;
        Ok(Classification::Complete(classify_character(
            owner, scalar?, descriptor,
        )?))
    } else if input.typeof_() == SEXPTYPE::INTSXP {
        Ok(Classification::Complete(classify_integers(
            owner, input, descriptor,
        )?))
    } else {
        Ok(Classification::NeedsIntegerCoercion { input, descriptor })
    }
}

/// GNU R's error buffer holds at most 999 bytes and drops a truncated UTF-8
/// tail. Keep that bounded representation on the stack until publication.
pub(super) struct InvalidClassMessage {
    bytes: [u8; 999],
    len: usize,
}

impl InvalidClassMessage {
    fn append(&mut self, text: &str) -> bool {
        for scalar in text.chars() {
            let mut encoded = [0; 4];
            let bytes = scalar.encode_utf8(&mut encoded).as_bytes();
            if bytes.len() > self.bytes.len() - self.len {
                return false;
            }
            self.bytes[self.len..self.len + bytes.len()].copy_from_slice(bytes);
            self.len += bytes.len();
        }
        true
    }

    pub(super) fn as_str(&self) -> &str {
        // Every append uses complete UTF-8 scalars.
        std::str::from_utf8(&self.bytes[..self.len]).expect("complete UTF-8 scalars")
    }
}

pub(super) fn invalid_class_message<'s>(
    owner: StoredOwner<'s>,
    character: Sexp<'s>,
) -> SexpResult<InvalidClassMessage> {
    let mut message = InvalidClassMessage {
        bytes: [0; 999],
        len: 0,
    };
    message.append("character class \"");
    let mut scalars = CharacterScalars::new(owner, character)?;
    while let Some(scalar) = scalars.next_scalar()? {
        let mut bytes = [0; 4];
        if !message.append(scalar.encode_utf8(&mut bytes)) {
            return Ok(message);
        }
    }
    message.append("\" is invalid");
    Ok(message)
}

/// Class names are ASCII and at most six bytes in GNU's table. Look them up
/// using a bounded stack buffer; invalid names remain rooted for the caller
/// to report in the original error message.
fn class_descriptor<'s>(
    owner: StoredOwner<'s>,
    character: Sexp<'s>,
) -> SexpResult<Option<std::num::NonZeroU32>> {
    let mut scalars = CharacterScalars::new(owner, character)?;
    let mut bytes = [0u8; 6];
    let mut length = 0;
    while let Some(scalar) = scalars.next_scalar()? {
        if !scalar.is_ascii() || length == bytes.len() {
            return Ok(None);
        }
        bytes[length] = scalar as u8;
        length += 1;
    }
    let name = std::str::from_utf8(&bytes[..length]).map_err(|_| SexpError::InvalidUtf8)?;
    Ok(std::num::NonZeroU32::new(
        crate::mainutils::rlocale::Ri18n_wctype_str(name),
    ))
}

/// Classify a rooted character payload without allocating an intermediate
/// wide-string buffer. The caller validates the class descriptor first.
fn classify_character<'s>(
    owner: StoredOwner<'s>,
    character: Sexp<'s>,
    descriptor: std::num::NonZeroU32,
) -> SexpResult<Sexp<'s>> {
    let mut scalars = CharacterScalars::new(owner.clone(), character)?;
    let mut count = 0i64;
    while scalars.next_scalar()?.is_some() {
        count = count.checked_add(1).ok_or(SexpError::AllocationFailed {
            object: "logical vector",
        })?;
    }
    let output = allocate_logical(&owner, count)?;
    let mut output = SexpMut::try_from_checked(output)?;
    scalars.position = 0;
    for index in 0..count {
        let scalar = scalars.next_scalar()?.ok_or(SexpError::InvalidUtf8)?;
        let classified =
            crate::mainutils::rlocale::Ri18n_iswctype_internal(scalar as u32, descriptor.get());
        output.try_set_logical_elt(index, i32::from(classified != 0))?;
    }
    owner.require_active()?;
    Ok(output.freeze())
}

fn allocate_logical<'s>(owner: &StoredOwner<'s>, length: i64) -> SexpResult<Sexp<'s>> {
    let factory = owner.node_factory()?;
    let allocate = || factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::LGLSXP, length)));
    match allocate() {
        Err(SexpError::AllocationFailed { .. }) => {
            owner.full_gc()?;
            allocate()
        }
        result => result,
    }
}

/// The coercion boundary supplies a rooted integer vector. Recheck the owner
/// after each element access because ALTREP element access can call R code.
pub(super) fn classify_integers<'s>(
    owner: StoredOwner<'s>,
    values: Sexp<'s>,
    descriptor: std::num::NonZeroU32,
) -> SexpResult<Sexp<'s>> {
    owner.require_active()?;
    owner.node_factory()?.link(&values)?;
    if values.typeof_() != SEXPTYPE::INTSXP {
        return Err(SexpError::TypeMismatch {
            expected: "integer vector",
            actual: values.typeof_(),
        });
    }
    let length = values.len();
    let mut output = SexpMut::try_from_checked(allocate_logical(&owner, length)?)?;
    for index in 0..length {
        let scalar = values.try_integer_elt(index);
        owner.require_active()?;
        let scalar = scalar?;
        let result = if scalar < 0 {
            crate::sexp::ffi::NA_LOGICAL
        } else {
            i32::from(
                crate::mainutils::rlocale::Ri18n_iswctype_internal(scalar as u32, descriptor.get())
                    != 0,
            )
        };
        output.try_set_logical_elt(index, result)?;
    }
    owner.require_active()?;
    Ok(output.freeze())
}

/// Retain the actual character and its physical payload without borrowing
/// either across an allocator, evaluator or callback boundary.
struct CharacterScalars<'s> {
    owner: StoredOwner<'s>,
    _character: Sexp<'s>,
    bytes: Bytes,
    len: usize,
    position: usize,
}

impl<'s> CharacterScalars<'s> {
    fn new(owner: StoredOwner<'s>, character: Sexp<'s>) -> SexpResult<Self> {
        owner.require_active()?;
        owner.node_factory()?.link(&character)?;
        if character.is_na_string() {
            return Ok(Self {
                owner,
                _character: character,
                bytes: Bytes::Missing,
                len: 2,
                position: 0,
            });
        }
        let header = character.header();
        if header.type_of() != SEXPTYPE::CHARSXP {
            return Err(SexpError::TypeMismatch {
                expected: "character scalar",
                actual: header.type_of(),
            });
        }
        let flags = header.sxpinfo.gp();
        if flags & (ASCII_MASK | UTF8_MASK) == 0 && flags & (BYTES_MASK | LATIN1_MASK) != 0 {
            return Err(SexpError::InvalidUtf8);
        }
        let NodeBody::Vector(vector) = header.body else {
            return Err(SexpError::MissingData {
                sexptype: SEXPTYPE::CHARSXP,
            });
        };
        let len = usize::try_from(vector.length).map_err(|_| SexpError::MissingData {
            sexptype: SEXPTYPE::CHARSXP,
        })?;
        let bytes = if len == 0 {
            Bytes::Empty
        } else {
            Bytes::Payload(
                header
                    .payload_lease()
                    .cloned()
                    .ok_or(SexpError::MissingData {
                        sexptype: SEXPTYPE::CHARSXP,
                    })?,
            )
        };
        Ok(Self {
            owner,
            _character: character,
            bytes,
            len,
            position: 0,
        })
    }

    fn next_scalar(&mut self) -> SexpResult<Option<char>> {
        self.owner.require_active()?;
        if self.position == self.len {
            return Ok(None);
        }
        let read = |index| match &self.bytes {
            Bytes::Missing => b"NA".get(index).copied(),
            Bytes::Empty => None,
            Bytes::Payload(payload) => payload.byte_elt(index),
        };
        let (scalar, end) =
            scalar_at(self.len, self.position, read).ok_or(SexpError::InvalidUtf8)?;
        // GNU's wide-string bridge stops at the first NUL even if a malformed
        // internal character payload contains later bytes.
        if scalar == '\0' {
            self.position = self.len;
            return Ok(None);
        }
        self.position = end;
        Ok(Some(scalar))
    }
}

/// Decode one UTF-8 scalar from copied payload bytes. The caller retains the
/// actual character payload; no borrowed R buffer or temporary heap vector is
/// needed. `None` means the bounded payload is malformed or truncated.
fn scalar_at(
    len: usize,
    start: usize,
    mut byte_at: impl FnMut(usize) -> Option<u8>,
) -> Option<(char, usize)> {
    let first = byte_at(start)?;
    let width = match first {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let end = start.checked_add(width)?;
    if end > len {
        return None;
    }
    let mut bytes = [0; 4];
    bytes[0] = first;
    for (offset, slot) in bytes.iter_mut().enumerate().take(width).skip(1) {
        *slot = byte_at(start.checked_add(offset)?)?;
    }
    let text = std::str::from_utf8(&bytes[..width]).ok()?;
    Some((text.chars().next()?, end))
}

#[cfg(test)]
mod tests {
    use super::{
        CharacterScalars, ClassError, Classification, class_descriptor, classify_character,
        classify_integers, invalid_class_message, prepare, scalar_at,
    };
    use crate::sexp::owner::StoredOwner;
    use crate::sexp::session::RSession;

    #[test]
    fn integer_altrep_callbacks_can_collect_while_result_is_rooted() {
        use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::object::SexpResult;
        struct Collecting;
        impl AltrepClass for Collecting {
            fn vector_type(&self) -> SEXPTYPE {
                SEXPTYPE::INTSXP
            }
            fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
                Ok(3)
            }
            fn element<'s>(
                &self,
                context: &AltrepContext<'s>,
                index: i64,
            ) -> SexpResult<AltrepElement<'s>> {
                context.gc()?;
                Ok(AltrepElement::Integer([65, 57, 225][index as usize]))
            }
        }
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let class = session
            .register_altrep_class("charclass.collect", Collecting)
            .unwrap();
        let input = AltrepBuilder::new(class).build().unwrap();
        let descriptor =
            std::num::NonZeroU32::new(crate::mainutils::rlocale::Ri18n_wctype_str("alpha"))
                .unwrap();
        let result = classify_integers(StoredOwner::from_token(owner), input, descriptor).unwrap();
        for (index, expected) in [1, 0, 1].into_iter().enumerate() {
            assert_eq!(result.try_logical_elt(index as i64).unwrap(), expected);
        }
    }

    #[test]
    fn integer_altrep_callback_cannot_publish_after_owner_revocation() {
        use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::object::SexpResult;
        use std::{cell::RefCell, rc::Rc};
        struct Closing(Rc<RefCell<Option<RSession>>>);
        impl AltrepClass for Closing {
            fn vector_type(&self) -> SEXPTYPE {
                SEXPTYPE::INTSXP
            }
            fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
                Ok(1)
            }
            fn element<'s>(&self, _: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
                self.0.borrow_mut().as_mut().unwrap().close();
                Ok(AltrepElement::Integer(65))
            }
        }
        let session = RSession::new_for_gc_tests();
        let facade = Rc::new(RefCell::new(None));
        let class = session
            .register_altrep_class("charclass.close", Closing(facade.clone()))
            .unwrap();
        let input = AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let owner = StoredOwner::from_token(session.owner_token().unwrap())
            .into_owned()
            .unwrap();
        *facade.borrow_mut() = Some(session);
        let descriptor =
            std::num::NonZeroU32::new(crate::mainutils::rlocale::Ri18n_wctype_str("alpha"))
                .unwrap();
        assert!(matches!(
            classify_integers(owner, input, descriptor),
            Err(crate::sexp::object::SexpError::RootUnavailable)
        ));
        drop(facade.borrow_mut().take());
    }

    #[test]
    fn prepare_checks_class_before_input_and_roots_vector_elements() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let input = factory.strings(&["á9"]).unwrap();
        let class = factory.strings(&["alpha"]).unwrap();
        let Classification::Complete(result) =
            prepare(StoredOwner::from_token(owner), input.clone(), class)
                .ok()
                .unwrap()
        else {
            panic!("expected completed classification")
        };
        assert_eq!(result.len(), 2);
        assert_eq!(result.try_logical_elt(0).unwrap(), 1);
        assert_eq!(result.try_logical_elt(1).unwrap(), 0);
        let bad_input = factory.strings(&["a", "b"]).unwrap();
        let bad_class = factory.strings(&["bogus"]).unwrap();
        assert!(matches!(
            prepare(StoredOwner::from_token(owner), bad_input.clone(), bad_class),
            Err(ClassError::InvalidClass(_))
        ));
        let class = factory.strings(&["alpha"]).unwrap();
        assert!(matches!(
            prepare(StoredOwner::from_token(owner), bad_input, class),
            Err(ClassError::InvalidCharacterArgument)
        ));
        let class = factory.strings(&[]).unwrap();
        assert!(matches!(
            prepare(StoredOwner::from_token(owner), input, class),
            Err(ClassError::InvalidClassArgument)
        ));
    }

    #[test]
    fn invalid_class_message_matches_gnu_buffer_boundaries() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        for (name, expected_length, suffix) in [
            ("x".repeat(970), 999, "\" is invalid"),
            ("x".repeat(971), 999, "\" is invali"),
            ("中".repeat(700), 998, "中"),
            (format!("x{}", "😀".repeat(700)), 998, "😀"),
        ] {
            let message = invalid_class_message(
                StoredOwner::from_token(owner),
                factory.character(&name).unwrap(),
            )
            .unwrap();
            assert_eq!(message.as_str().len(), expected_length);
            assert!(message.as_str().ends_with(suffix));
        }
    }

    #[test]
    fn class_names_are_checked_without_unbounded_temporary_storage() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        for name in [
            "upper", "lower", "alpha", "digit", "xdigit", "space", "print", "graph", "blank",
            "cntrl", "punct", "alnum",
        ] {
            let descriptor = class_descriptor(
                StoredOwner::from_token(owner),
                factory.character(name).unwrap(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                descriptor.get(),
                crate::mainutils::rlocale::Ri18n_wctype_str(name)
            );
        }
        for name in ["", "alph", "alphabet", "álpha", "ALPHA"] {
            assert_eq!(
                class_descriptor(
                    StoredOwner::from_token(owner),
                    factory.character(name).unwrap()
                )
                .unwrap(),
                None
            );
        }
    }

    #[test]
    fn classify_character_matches_pinned_gnu_scalars() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        for (class, expected) in [
            ("alpha", [1, 1, 1, 0, 0, 0, 0]),
            ("digit", [0, 0, 0, 0, 0, 0, 1]),
            ("alnum", [1, 1, 1, 0, 0, 0, 1]),
        ] {
            let descriptor =
                std::num::NonZeroU32::new(crate::mainutils::rlocale::Ri18n_wctype_str(class))
                    .unwrap();
            let value = factory.character("Aá١²́😀9").unwrap();
            let result =
                classify_character(StoredOwner::from_token(owner), value, descriptor).unwrap();
            for (index, expected) in expected.into_iter().enumerate() {
                assert_eq!(
                    result.try_logical_elt(index as i64).unwrap(),
                    expected,
                    "{class}"
                );
            }
        }
    }

    #[test]
    fn classify_character_respects_allocation_limit() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let length = if cfg!(miri) { 128 } else { 8192 };
        let value = owner.node_factory().character(&"a".repeat(length)).unwrap();
        owner
            .with_arena(|arena| {
                arena.set_budget(crate::sexp::memory::ArenaBudget::new(1, 0));
            })
            .unwrap();
        let descriptor =
            std::num::NonZeroU32::new(crate::mainutils::rlocale::Ri18n_wctype_str("alpha"))
                .unwrap();
        assert!(matches!(
            classify_character(StoredOwner::from_token(owner), value.clone(), descriptor),
            Err(crate::sexp::object::SexpError::AllocationFailed { .. })
        ));
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
        owner
            .with_arena(|arena| arena.set_budget(crate::sexp::memory::ArenaBudget::unlimited()))
            .unwrap();
        let result = classify_character(StoredOwner::from_token(owner), value, descriptor).unwrap();
        assert_eq!(result.len(), length as i64);
        assert_eq!(result.try_logical_elt(length as i64 - 1).unwrap(), 1);
    }

    #[test]
    fn classify_empty_and_missing_character_scalars() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let descriptor =
            std::num::NonZeroU32::new(crate::mainutils::rlocale::Ri18n_wctype_str("alpha"))
                .unwrap();
        let empty = classify_character(
            StoredOwner::from_token(owner),
            factory.character("").unwrap(),
            descriptor,
        )
        .unwrap();
        assert_eq!(empty.len(), 0);
        let missing = factory
            .wrap(crate::sexp::globals::immutable_na_string_projection().unwrap())
            .unwrap();
        let result =
            classify_character(StoredOwner::from_token(owner), missing, descriptor).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.try_logical_elt(0).unwrap(), 1);
        assert_eq!(result.try_logical_elt(1).unwrap(), 1);
    }

    #[test]
    fn classify_integers_matches_gnu_for_negative_and_out_of_range_values() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let input = factory
            .allocate(|arena| Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 6)))
            .unwrap();
        let mut input = crate::sexp::object::SexpMut::try_from_checked(input).unwrap();
        for (index, value) in [65, 225, -1, i32::MIN, 0x110000, 57]
            .into_iter()
            .enumerate()
        {
            input.try_set_integer_elt(index as i64, value).unwrap();
        }
        let descriptor =
            std::num::NonZeroU32::new(crate::mainutils::rlocale::Ri18n_wctype_str("alpha"))
                .unwrap();
        let result =
            classify_integers(StoredOwner::from_token(owner), input.freeze(), descriptor).unwrap();
        for (index, expected) in [1, 1, i32::MIN, i32::MIN, 0, 0].into_iter().enumerate() {
            assert_eq!(result.try_logical_elt(index as i64).unwrap(), expected);
        }
    }

    #[test]
    fn scalar_decoder_is_bounded_and_rejects_invalid_utf8() {
        let text = "Aá١中😀";
        let bytes = text.as_bytes();
        let mut position = 0;
        for expected in text.chars() {
            let (actual, end) =
                scalar_at(bytes.len(), position, |i| bytes.get(i).copied()).unwrap();
            assert_eq!(actual, expected);
            assert!(end > position && end <= bytes.len());
            position = end;
        }
        assert_eq!(position, bytes.len());
        assert!(scalar_at(bytes.len(), position, |i| bytes.get(i).copied()).is_none());
        for invalid in [
            &[0xff][..],
            &[0xc0, 0x80],
            &[0xe2, 0x82],
            &[0xed, 0xa0, 0x80],
            &[0xf4, 0x90, 0x80, 0x80],
        ] {
            assert!(
                scalar_at(invalid.len(), 0, |i| invalid.get(i).copied()).is_none(),
                "{invalid:?}"
            );
        }
        assert!(scalar_at(1, usize::MAX, |_| Some(b'a')).is_none());
        assert!(scalar_at(4, 0, |_| None).is_none());
    }

    #[test]
    fn character_scalars_keep_original_payload_rooted_across_collection() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let value = factory.character("Aá١中😀").unwrap();
        let mut scalars = CharacterScalars::new(StoredOwner::from_token(owner), value).unwrap();
        owner.full_gc().unwrap();
        for expected in "Aá١中😀".chars() {
            assert_eq!(scalars.next_scalar().unwrap(), Some(expected));
        }
        assert_eq!(scalars.next_scalar().unwrap(), None);
        let missing = factory
            .wrap(crate::sexp::globals::immutable_na_string_projection().unwrap())
            .unwrap();
        let mut missing = CharacterScalars::new(StoredOwner::from_token(owner), missing).unwrap();
        assert_eq!(missing.next_scalar().unwrap(), Some('N'));
        assert_eq!(missing.next_scalar().unwrap(), Some('A'));
        assert_eq!(missing.next_scalar().unwrap(), None);
        let embedded_nul = factory.character("a\0b").unwrap();
        let mut embedded_nul =
            CharacterScalars::new(StoredOwner::from_token(owner), embedded_nul).unwrap();
        assert_eq!(embedded_nul.next_scalar().unwrap(), Some('a'));
        assert_eq!(embedded_nul.next_scalar().unwrap(), None);
        let invalid = factory
            .allocate(|arena| Some(arena.alloc_charsxp(&[0xff])))
            .unwrap();
        let mut invalid = CharacterScalars::new(StoredOwner::from_token(owner), invalid).unwrap();
        assert_eq!(
            invalid.next_scalar(),
            Err(crate::sexp::object::SexpError::InvalidUtf8)
        );
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }
    #[test]
    fn character_scalars_reject_foreign_heap_and_inactive_owner() {
        let session = RSession::new_for_gc_tests();
        let other = RSession::new_for_gc_tests();
        let foreign = other.with_active_in(|_| {
            other
                .owner_token()
                .unwrap()
                .node_factory()
                .character("β")
                .unwrap()
                .into_owned()
                .unwrap()
        });
        session.with_active_in(|_| {
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            assert!(matches!(
                CharacterScalars::new(StoredOwner::from_token(owner), foreign),
                Err(crate::sexp::object::SexpError::UnownedPointer { .. })
            ));
            let value = factory.character("áβ").unwrap();
            let mut scalars = CharacterScalars::new(StoredOwner::from_token(owner), value).unwrap();
            other.with_active_in(|_| {
                assert_eq!(
                    scalars.next_scalar(),
                    Err(crate::sexp::object::SexpError::OwnerNotActive)
                );
            });
            assert_eq!(scalars.next_scalar().unwrap(), Some('á'));
        });
    }

    #[test]
    fn character_scalars_reject_revoked_owner() {
        let mut session = RSession::new_for_gc_tests();
        let mut scalars = session.with_active_in(|_| {
            let owner = session.owner_token().unwrap();
            let value = owner
                .node_factory()
                .character("áβ")
                .unwrap()
                .into_owned()
                .unwrap();
            let owner = StoredOwner::from_token(owner).into_owned().unwrap();
            CharacterScalars::new(owner, value).unwrap()
        });
        session.close();
        assert!(scalars.next_scalar().is_err());
    }
}
