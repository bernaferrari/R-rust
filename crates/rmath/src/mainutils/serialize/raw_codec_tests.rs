use super::*;
use crate::sexp::{RSession, Sexp, SexpMut, memory::ArenaBudget, object::SessionNodeFactory};

type Encoder = unsafe fn(SEXP) -> SEXP;
#[cfg(not(miri))]
type Decoder = unsafe fn(SEXP, *mut Rboolean) -> SEXP;

#[test]
#[cfg(not(miri))]
fn serialization_workspace_lazy_file_input_and_r_result_share_peak_admission() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/serialization-workspace-evidence/lazy-input-budget.bin");
    std::fs::write(&path, vec![7; 32768]).unwrap();
    let file = factory.strings(&[path.to_str().unwrap()]).unwrap();
    let key = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 2)))
        .unwrap();
    let mut key = SexpMut::try_from_checked(key).unwrap();
    key.try_set_integer_elt(0, 0).unwrap();
    key.try_set_integer_elt(1, 32768).unwrap();
    let key = key.freeze();
    let limit = owner
        .with_arena(|a| a.total_bytes_allocated() + 32768 + 4096)
        .unwrap();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
        .unwrap();
    let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        readRawFromFile(file.as_raw(), key.as_raw())
    }));
    std::fs::remove_file(path).unwrap();
    assert!(
        denied.is_err(),
        "the native read buffer and final R raw vector coexist"
    );
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
}

#[test]
#[cfg(not(miri))]
fn serialization_workspace_raw_codecs_decode_gnu_and_export_independent_records() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let expected = include_bytes!("fixtures/gnu-lazy-codecs/expected.rds");
    let source = raw(&factory, expected);
    let cases: [(&[u8], Encoder, Decoder); 3] = [
        (
            include_bytes!("fixtures/gnu-lazy-codecs/mode1.bin"),
            R_compress1,
            R_decompress1,
        ),
        (
            include_bytes!("fixtures/gnu-lazy-codecs/mode2.bin"),
            R_compress2,
            R_decompress2,
        ),
        (
            include_bytes!("fixtures/gnu-lazy-codecs/mode3.bin"),
            R_compress3,
            R_decompress3,
        ),
    ];
    for (index, (fixture, encode, decode)) in cases.into_iter().enumerate() {
        let input = raw(&factory, fixture);
        let mut invalid = 0;
        let decoded = factory
            .wrap(unsafe { decode(input.as_raw(), &mut invalid) })
            .unwrap();
        let crate::sexp::SexpValue::RawVector(bytes) = decoded.to_owned_value().unwrap() else {
            panic!("raw decode");
        };
        assert_eq!(invalid, 0);
        assert_eq!(bytes, expected);
        let encoded = factory.wrap(unsafe { encode(source.as_raw()) }).unwrap();
        let crate::sexp::SexpValue::RawVector(bytes) = encoded.to_owned_value().unwrap() else {
            panic!("raw encode");
        };
        std::fs::write(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../target/serialization-workspace-evidence/rust-raw-mode{}.bin",
                index + 1
            )),
            bytes,
        )
        .unwrap();
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
}

#[test]
#[cfg(not(miri))]
fn serialization_workspace_raw_empty_records_roundtrip() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let source = raw(&factory, &[]);
    let operations: [(Encoder, Decoder); 3] = [
        (R_compress1, R_decompress1),
        (R_compress2, R_decompress2),
        (R_compress3, R_decompress3),
    ];
    for (encode, decode) in operations {
        let encoded = factory.wrap(unsafe { encode(source.as_raw()) }).unwrap();
        let mut invalid = 0;
        let decoded = factory
            .wrap(unsafe { decode(encoded.as_raw(), &mut invalid) })
            .unwrap();
        assert_eq!(invalid, 0);
        assert_eq!(decoded.len(), 0);
    }
}

#[test]
#[cfg(not(miri))]
fn serialization_workspace_raw_truncated_huge_record_never_reserves_declared_extent() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let limit = owner
        .with_arena(|a| a.total_bytes_allocated() + 128 * 1024)
        .unwrap();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
        .unwrap();
    assert!(matches!(
        super::super::raw_codecs::decode(&owner, &[255, 255, 255, 255, 1], 1),
        Err(super::super::raw_codecs::DecodeError::Invalid)
    ));
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
}

fn raw<'s>(factory: &SessionNodeFactory<'s>, bytes: &[u8]) -> Sexp<'s> {
    let value = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::RAWSXP, bytes.len() as _)))
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    for (i, byte) in bytes.iter().enumerate() {
        value.try_set_raw_elt(i as _, *byte).unwrap();
    }
    value.freeze()
}

#[test]
fn serialization_workspace_raw_encoders_deny_before_unaccounted_codec_allocation() {
    let encoders: [Encoder; 3] = [R_compress1, R_compress2, R_compress3];
    for encode in encoders {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let input = raw(&factory, b"raw database record");
        let limit = owner
            .with_arena(|a| a.total_bytes_allocated() + 4096)
            .unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
            .unwrap();
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            encode(input.as_raw())
        }));
        let error = denied.expect_err("raw codec must reserve its own scratch");
        assert!(
            error
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("session memory limit")
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
    }
}

#[test]
#[cfg(not(miri))]
fn serialization_workspace_raw_decoders_distinguish_budget_denial_from_corrupt_data() {
    let operations: [(Encoder, Decoder); 3] = [
        (R_compress1, R_decompress1),
        (R_compress2, R_decompress2),
        (R_compress3, R_decompress3),
    ];
    for (encode, decode) in operations {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let input = raw(&factory, &[7; 1024]);
        let encoded = factory.wrap(unsafe { encode(input.as_raw()) }).unwrap();
        let limit = owner
            .with_arena(|a| a.total_bytes_allocated() + 4096)
            .unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
            .unwrap();
        let mut invalid = 0;
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            decode(encoded.as_raw(), &mut invalid)
        }));
        let error = denied.expect_err("raw decoder scratch cannot bypass the cap");
        assert!(
            error
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("session memory limit")
        );
        assert_eq!(invalid, 0, "resource denial is not corrupt serialized data");
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
}

#[test]
#[cfg(not(miri))]
fn serialization_workspace_small_gnu_lzma_record_uses_only_possible_history() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let fixture = include_bytes!("fixtures/gnu-lazy-codecs/mode3.bin");
    let expected = include_bytes!("fixtures/gnu-lazy-codecs/expected.rds");
    let input = raw(&factory, fixture);
    let limit = owner
        .with_arena(|arena| arena.total_bytes_allocated() + 100 * 1024)
        .unwrap();
    owner
        .with_arena(|arena| arena.set_budget(ArenaBudget::new(limit, 0)))
        .unwrap();
    let mut invalid = 0;
    let decoded = factory
        .wrap(unsafe { R_decompress3(input.as_raw(), &mut invalid) })
        .unwrap();
    assert_eq!(invalid, 0);
    let crate::sexp::SexpValue::RawVector(bytes) = decoded.to_owned_value().unwrap() else {
        panic!("raw decode");
    };
    assert_eq!(bytes, expected);
    // Two original GNU chunks reset their properties between records. The
    // replacement model must coexist with the previous one under admission.
    let mut reset_chunks = ((expected.len() * 2) as u32).to_be_bytes().to_vec();
    reset_chunks.push(b'Z');
    reset_chunks.extend_from_slice(&fixture[5..fixture.len() - 1]);
    reset_chunks.extend_from_slice(&fixture[5..]);
    let restored = super::super::raw_codecs::decode(&owner, &reset_chunks, 3).unwrap();
    assert_eq!(&*restored, expected.repeat(2));
    drop(restored);
    // A dishonest short extent must still be rejected even though the decoder
    // can no longer allocate GNU's full dictionary for this small record.
    let mut malformed = fixture.to_vec();
    malformed[..4].copy_from_slice(&1u32.to_be_bytes());
    let input = raw(&factory, &malformed);
    let result = unsafe { R_decompress3(input.as_raw(), &mut invalid) };
    assert_eq!(invalid, 1);
    assert_eq!(result, unsafe { R_NilValue() });
    assert_eq!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
        0
    );
    assert_eq!(
        owner.with_arena(|arena| arena.budget().max_bytes).unwrap(),
        limit
    );
}

#[test]
#[cfg(not(miri))]
fn serialization_workspace_lzma_declared_large_history_is_bounded_and_validated() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let limit = owner
        .with_arena(|arena| arena.total_bytes_allocated() + 12 * 1024 * 1024)
        .unwrap();
    owner
        .with_arena(|arena| arena.set_budget(ArenaBudget::new(limit, 0)))
        .unwrap();
    let mut malformed = include_bytes!("fixtures/gnu-lazy-codecs/mode3.bin").to_vec();
    malformed[..4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(matches!(
        super::super::raw_codecs::decode(&owner, &malformed, 3),
        Err(super::super::raw_codecs::DecodeError::Invalid)
    ));
    assert_eq!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
        0
    );
    // This valid stream needs more than the minimum dictionary. Its original
    // bytes must survive beyond the minimum dictionary and output-buffer growth.
    let original: Vec<_> = (0..200_000).map(|index| (index % 251) as u8).collect();
    // Encoding has its own larger, independently checked workspace budget.
    owner
        .with_arena(|arena| arena.set_budget(ArenaBudget::new(0, 0)))
        .unwrap();
    let encoded = super::super::raw_codecs::encode(&owner, &original, 3).unwrap();
    owner
        .with_arena(|arena| arena.set_budget(ArenaBudget::new(limit, 0)))
        .unwrap();
    let restored = super::super::raw_codecs::decode(&owner, &encoded, 3).unwrap();
    assert_eq!(&*restored, original);
    drop(restored);
    drop(encoded);
    assert_eq!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
        0
    );
}

#[test]
#[cfg(not(miri))]
fn serialization_workspace_gnu_zlib_uses_the_existing_input_buffer() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let input = raw(
        &factory,
        include_bytes!("fixtures/gnu-lazy-codecs/mode1.bin"),
    );
    let baseline = owner
        .with_arena(|arena| arena.total_bytes_allocated())
        .unwrap();
    // Admit the input snapshot but leave decoder scratch one byte short.
    let denied_limit = baseline + 64 * 1024 + input.len() as usize - 1;
    owner
        .with_arena(|arena| arena.set_budget(ArenaBudget::new(denied_limit, 0)))
        .unwrap();
    let mut invalid = 0;
    let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        R_decompress1(input.as_raw(), &mut invalid)
    }))
    .expect_err("decoder scratch must be admitted before construction");
    assert!(
        denied
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap()
            .message
            .contains("session memory limit")
    );
    assert_eq!(invalid, 0, "budget denial must not report corrupt data");
    assert_eq!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
        0
    );
    let limit = baseline + 80 * 1024;
    owner
        .with_arena(|arena| arena.set_budget(ArenaBudget::new(limit, 0)))
        .unwrap();
    let decoded = factory
        .wrap(unsafe { R_decompress1(input.as_raw(), &mut invalid) })
        .unwrap();
    assert_eq!(invalid, 0);
    let crate::sexp::SexpValue::RawVector(bytes) = decoded.to_owned_value().unwrap() else {
        panic!("raw decode");
    };
    assert_eq!(
        bytes,
        include_bytes!("fixtures/gnu-lazy-codecs/expected.rds")
    );
    assert_eq!(
        owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
        0
    );
    assert_eq!(
        owner.with_arena(|arena| arena.budget().max_bytes).unwrap(),
        limit
    );
}

#[test]
#[cfg(not(miri))]
fn serialization_workspace_framed_outputs_release_chunks_and_retain_actual_final_capacity() {
    let fixtures: [(&[u8], u8); 3] = [
        (include_bytes!("fixtures/gnu-lazy-codecs/mode1.bin"), 1),
        (include_bytes!("fixtures/gnu-lazy-codecs/mode2.bin"), 2),
        (include_bytes!("fixtures/gnu-lazy-codecs/mode3.bin"), 3),
    ];
    let expected = include_bytes!("fixtures/gnu-lazy-codecs/expected.rds");
    for (input, mode) in fixtures {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let output = super::super::raw_codecs::decode(&owner, input, mode).unwrap();
        assert_eq!(&*output, expected);
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            output.capacity()
        );
        drop(output);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
}

#[test]
#[cfg(not(miri))]
fn serialization_workspace_chunk_coalescing_releases_decoder_scratch_and_recovers_denial() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let original: Vec<u8> = (0..200_000).map(|n| (n % 251) as u8).collect();
    let encoded = super::super::raw_codecs::encode(&owner, &original, 1).unwrap();
    let baseline = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    let retained = owner.with_arena(|a| a.workspace_bytes()).unwrap();
    let denied_limit = baseline + retained + 350_000;
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(denied_limit, 0)))
        .unwrap();
    assert!(matches!(
        super::super::raw_codecs::decode(&owner, &encoded, 1),
        Err(super::super::raw_codecs::DecodeError::Resource(_))
    ));
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), retained);
    let limit = baseline + retained + 416_000;
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
        .unwrap();
    // A live 64KiB decoder alongside both complete 200000-byte buffers
    // would exceed this cap, so success requires the decoder to be destroyed.
    let decoded = super::super::raw_codecs::decode(&owner, &encoded, 1).unwrap();
    assert_eq!(&*decoded, original);
    assert_eq!(
        owner.with_arena(|a| a.workspace_bytes()).unwrap(),
        retained + decoded.capacity()
    );
    drop(decoded);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), retained);
    assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
    drop(encoded);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}

#[test]
#[cfg(not(miri))]
fn serialization_workspace_chunk_headers_are_admitted_and_foreign_activation_cannot_redirect_them()
{
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let original = vec![7u8; 8192];
    let encoded = super::super::raw_codecs::encode(&owner, &original, 1).unwrap();
    let baseline = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    let retained = owner.with_arena(|a| a.workspace_bytes()).unwrap();
    // Exactly enough for byte storage plus zlib scratch, with no room for
    // the owning chunk headers. They must not allocate outside admission.
    let limit = baseline + retained + 65536 + original.len();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
        .unwrap();
    assert!(matches!(
        super::super::raw_codecs::decode(&owner, &encoded, 1),
        Err(super::super::raw_codecs::DecodeError::Resource(_))
    ));
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), retained);
    let replacement = RSession::new_for_gc_tests();
    let replacement_owner = replacement.owner_token().unwrap();
    replacement.with_active(|| {
        let Err(super::super::raw_codecs::DecodeError::Resource(message)) =
            super::super::raw_codecs::decode(&owner, &encoded, 1)
        else {
            panic!("foreign activation must not substitute its heap");
        };
        assert!(message.contains("activate the owning session"), "{message}");
        assert_eq!(
            replacement_owner
                .with_arena(|a| a.workspace_bytes())
                .unwrap(),
            0
        );
    });
    session.with_active(|| {
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), retained);
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        let decoded = super::super::raw_codecs::decode(&owner, &encoded, 1).unwrap();
        assert_eq!(&*decoded, original);
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            retained + decoded.capacity()
        );
        drop(decoded);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), retained);
        drop(encoded);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    });
}
