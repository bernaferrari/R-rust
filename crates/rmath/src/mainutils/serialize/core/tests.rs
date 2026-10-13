use super::*;

#[cfg(test)]
mod bytecode_reader_tests {
    use super::*;

    #[test]
    fn invalid_repetition_references_and_definitions_are_rejected() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let reps = Rf_allocVector(SEXPTYPE::VECSXP, 1);
            let _reps = protect(reps);
            SET_VECTOR_ELT(reps, 0, R_NilValue());
            for index in [-1_i32, 0, 1, i32::MAX] {
                let bytes = index.to_ne_bytes();
                let mut reader = BinaryReader::new(&bytes);
                let mut refs = ReadRefTable::new();
                assert!(read_bc_language(243, &mut reader, &mut refs, reps).is_err());
                assert_eq!(reader.item_depth, 0);
            }
            let mut bytes = 0_i32.to_ne_bytes().to_vec();
            bytes.extend_from_slice(&13_i32.to_ne_bytes()); // integer is not a language definition
            let mut reader = BinaryReader::new(&bytes);
            assert!(read_bc_language(244, &mut reader, &mut ReadRefTable::new(), reps).is_err());
            assert_eq!(reader.item_depth, 0);
        }
    }
}

#[cfg(test)]
mod special_environment_identity_tests {
    use super::*;
    use crate::sexp::{
        object::{SessionNodeFactory, Sexp, SexpMut},
        session::RSession,
    };

    // Exact GNU R 4.7, XDR version 3 bytes produced by the pinned oracle:
    // serialize(asNamespace("base"), NULL, xdr=TRUE, version=3).
    const BASE_NAMESPACE_RDS: &[u8] =
        b"X\n\x00\x00\x00\x03\x00\x04\x07\x00\x00\x03\x05\x00\x00\x00\x00\x05UTF-8\x00\x00\x00\xfa";

    fn raw<'s>(factory: &SessionNodeFactory<'s>, bytes: &[u8]) -> Sexp<'s> {
        let value = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t)))
            .unwrap();
        let mut value = SexpMut::try_from_checked(value).unwrap();
        for (index, byte) in bytes.iter().copied().enumerate() {
            value.try_set_raw_elt(index as R_xlen_t, byte).unwrap();
        }
        value.freeze()
    }

    fn read<'s>(factory: &SessionNodeFactory<'s>, bytes: &[u8]) -> Sexp<'s> {
        let bytes = raw(factory, bytes);
        // The original factory/input own their exact heap through deserialization.
        factory
            .wrap(unsafe {
                crate::mainutils::serialize::R_unserialize(bytes.as_raw(), R_NilValue())
            })
            .unwrap()
    }

    fn minimal_original_namespace<'s>(
        session: &'s RSession,
        factory: &SessionNodeFactory<'s>,
    ) -> Sexp<'s> {
        let base = factory.wrap(unsafe { R_BaseEnv() }).unwrap();
        let symbol = factory
            .wrap(unsafe { crate::sexp::symbol::Rf_install(c".BaseNamespaceEnv".as_ptr()) })
            .unwrap();
        // Bootstrap uses a shared original base frame, then replaces its placeholder.
        unsafe { crate::sexp::envir::define_var_safe(symbol.clone(), base.clone(), base.clone()) };
        let namespace = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::ENVSXP)))
            .unwrap();
        let frame = base.try_frame().unwrap();
        let global = session.global_env().unwrap();
        unsafe {
            crate::sexp::accessors::SET_FRAME(namespace.as_raw(), frame.as_raw());
            crate::sexp::accessors::SET_ENCLOS(namespace.as_raw(), global.as_raw());
            crate::sexp::envir::define_var_safe(symbol, namespace.clone(), base);
        }
        assert_eq!(
            unsafe { crate::sexp::envir::R_BaseNamespace() },
            namespace.as_raw()
        );
        namespace
    }

    #[test]
    fn original_gnu_base_namespace_rds_preserves_real_base_bootstrap_identity() {
        let session = RSession::new_without_default_packages();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let value = read(&factory, BASE_NAMESPACE_RDS);
        assert_eq!(value.as_raw(), unsafe {
            crate::sexp::envir::R_BaseNamespace()
        });
        assert_ne!(value.as_raw(), unsafe { R_BaseEnv() });
        session.owner_token().unwrap().full_gc().unwrap();
        assert_eq!(value.as_raw(), unsafe {
            crate::sexp::envir::R_BaseNamespace()
        });
    }

    #[test]
    fn original_gnu_special_environment_tokens_preserve_original_owner_after_gc() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let namespace = minimal_original_namespace(&session, &factory);
        // All four exact oracle streams differ only in the final type byte.
        let cases = unsafe {
            [
                (0xf2, R_EmptyEnv()),
                (0xfd, R_GlobalEnv()),
                (0xf1, R_BaseEnv()),
                (0xfa, namespace.as_raw()),
            ]
        };
        let mut values = Vec::new();
        for (token, expected) in cases {
            let mut bytes = BASE_NAMESPACE_RDS.to_vec();
            *bytes.last_mut().unwrap() = token;
            let value = read(&factory, &bytes);
            assert_eq!(value.as_raw(), expected);
            values.push(value);
        }
        session.owner_token().unwrap().full_gc().unwrap();
        for (value, (_, expected)) in values.iter().zip(cases) {
            assert!(value.allocation().unwrap().is_live());
            assert_eq!(value.as_raw(), expected);
        }
        assert_ne!(values[2], values[3]);
        assert_eq!(
            values[3].try_enclos().unwrap(),
            session.global_env().unwrap()
        );
    }

    #[test]
    fn original_gnu_namespace_token_uses_each_exact_session_domain() {
        let left = RSession::new_for_gc_tests();
        let left_factory = SessionNodeFactory::new(left.owner_token().unwrap());
        let left_namespace = minimal_original_namespace(&left, &left_factory)
            .into_owned()
            .unwrap();
        let left_value = read(&left_factory, BASE_NAMESPACE_RDS)
            .into_owned()
            .unwrap();
        let right = RSession::new_for_gc_tests();
        let right_factory = SessionNodeFactory::new(right.owner_token().unwrap());
        let right_namespace = minimal_original_namespace(&right, &right_factory);
        let right_value = read(&right_factory, BASE_NAMESPACE_RDS);
        assert_eq!(left_value, left_namespace);
        assert_eq!(right_value, right_namespace);
        assert_ne!(left_value.as_raw(), right_value.as_raw());
        right.owner_token().unwrap().full_gc().unwrap();
        left.with_active(|| left.owner_token().unwrap().full_gc().unwrap());
        assert!(left_value.allocation().unwrap().is_live());
        assert!(right_value.allocation().unwrap().is_live());
    }
}

#[cfg(test)]
mod bounded_frame_reader_tests {
    use super::*;
    use crate::sexp::{owner::OwnerToken, session::RSession};

    fn nested_vectors(depth: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for _ in 0..depth {
            bytes.extend_from_slice(&SEXPTYPE::VECSXP.as_c_int().to_ne_bytes());
            bytes.extend_from_slice(&1_i32.to_ne_bytes());
        }
        bytes.extend_from_slice(&NILVALUE_SXP.to_ne_bytes());
        bytes
    }

    fn compiled_constant() -> Vec<i32> {
        vec![
            SEXPTYPE::BCODESXP.as_c_int(),
            1, // Outer WriteBC repetition table.
            SEXPTYPE::INTSXP.as_c_int(),
            4,
            crate::eval::bytecode::GNU_BC_MAX_VERSION,
            16, // LDCONST
            0,
            1,                           // RETURN
            1,                           // One constant, also the original source expression.
            SEXPTYPE::INTSXP.as_c_int(), // WriteBC constant kind.
            SEXPTYPE::INTSXP.as_c_int(),
            1,
            42,
        ]
    }

    #[test]
    fn owned_decoder_compiled_promise_expression_survives_collection_and_forcing() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let mut words = vec![
                SEXPTYPE::PROMSXP.as_c_int() | HAS_TAG_BIT_MASK,
                BASEENV_SXP,
                UNBOUNDVALUE_SXP,
            ];
            words.extend(compiled_constant());
            let bytes: Vec<u8> = words.into_iter().flat_map(i32::to_ne_bytes).collect();
            let mut reader = BinaryReader::new(&bytes);
            let owner = OwnerToken::current().unwrap();
            let promise = owner
                .sexp(ReadItemInternal(&mut reader, &mut ReadRefTable::new()).unwrap())
                .unwrap()
                .into_owned()
                .unwrap();
            assert_eq!(reader.pos, bytes.len());
            assert_eq!(reader.item_depth, 0);
            assert_eq!(reader.bc_source_depth, 0);
            assert_eq!(TYPEOF(PRCODE(promise.as_raw())), SEXPTYPE::BCODESXP);
            assert_eq!(PRVALUE(promise.as_raw()), R_UnboundValue());
            owner.full_gc().unwrap();
            let result = owner
                .sexp(crate::sexp::envir::forcePromise(promise.as_raw()))
                .unwrap()
                .into_owned()
                .unwrap();
            owner.full_gc().unwrap();
            assert_eq!(result.try_integer_elt(0).unwrap(), 42);
            assert_eq!(PRVALUE(promise.as_raw()), result.as_raw());
        });
    }

    #[test]
    fn decoder_compiled_field_context_keeps_data_and_malformed_stream_rejections() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let standalone: Vec<u8> = compiled_constant()
                .into_iter()
                .flat_map(i32::to_ne_bytes)
                .collect();
            let error = ReadItemInternal(
                &mut BinaryReader::new(&standalone),
                &mut ReadRefTable::new(),
            )
            .unwrap_err();
            assert_eq!(
                error,
                "GNU R BCODESXP is well-framed but execution adapter is unavailable"
            );
            // PRVALUE is data even when the enclosing item is a promise.
            let mut value = vec![SEXPTYPE::PROMSXP.as_c_int()];
            value.extend(compiled_constant());
            value.push(NILVALUE_SXP);
            let bytes: Vec<u8> = value.into_iter().flat_map(i32::to_ne_bytes).collect();
            assert_eq!(
                ReadItemInternal(&mut BinaryReader::new(&bytes), &mut ReadRefTable::new())
                    .unwrap_err(),
                error
            );
            let mut expression = vec![SEXPTYPE::PROMSXP.as_c_int(), UNBOUNDVALUE_SXP];
            let mut invalid = compiled_constant();
            invalid[5] = crate::eval::bytecode::GNU_BC_OPCODE_COUNT as i32;
            expression.extend(invalid);
            let bytes: Vec<u8> = expression.into_iter().flat_map(i32::to_ne_bytes).collect();
            let mut reader = BinaryReader::new(&bytes);
            let error = ReadItemInternal(&mut reader, &mut ReadRefTable::new()).unwrap_err();
            assert!(error.contains("unknown GNU R bytecode opcode"), "{error}");
            assert_eq!(reader.item_depth, 0);
            assert_eq!(reader.bc_source_depth, 0);
        });
    }

    #[test]
    fn owned_small_decoder_frames_preserve_deep_vector_graph_and_collection() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let bytes = nested_vectors(96);
            let mut reader = BinaryReader::new(&bytes);
            let mut refs = ReadRefTable::new();
            let owner = OwnerToken::current().unwrap();
            let raw = ReadItemInternal(&mut reader, &mut refs).unwrap();
            let result = owner.sexp(raw).unwrap().into_owned().unwrap();
            assert_eq!(reader.item_depth, 0);
            owner.full_gc().unwrap();
            let mut current = result;
            for _ in 0..96 {
                assert_eq!(current.typeof_(), SEXPTYPE::VECSXP);
                assert_eq!(current.len(), 1);
                current = current.try_vector_elt(0).unwrap().into_owned().unwrap();
            }
            assert!(current.is_nil());
        });
    }

    #[test]
    fn environment_decoder_preserves_explicit_hash_policy_for_small_frames() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            for hashed in [false, true] {
                let mut words = vec![SEXPTYPE::ENVSXP.as_c_int(), 0, EMPTYENV_SXP, NILVALUE_SXP];
                if hashed {
                    words.extend([SEXPTYPE::VECSXP.as_c_int(), 1, NILVALUE_SXP]);
                } else {
                    words.push(NILVALUE_SXP);
                }
                words.push(NILVALUE_SXP);
                let bytes: Vec<u8> = words.into_iter().flat_map(i32::to_ne_bytes).collect();
                let mut reader = BinaryReader::new(&bytes);
                let owner = OwnerToken::current().unwrap();
                let raw = ReadItemInternal(&mut reader, &mut ReadRefTable::new()).unwrap();
                let environment = owner.sexp(raw).unwrap().into_owned().unwrap();
                assert_eq!(crate::sexp::env_hash::env_has_hash_table(raw), hashed);
                owner.full_gc().unwrap();
                assert_eq!(
                    crate::sexp::env_hash::env_has_hash_table(environment.as_raw()),
                    hashed
                );
            }
            let raw = crate::sexp::envir::R_NewHashedEnv(crate::sexp::globals::R_EmptyEnv(), 29);
            assert!(crate::sexp::env_hash::env_has_hash_table(raw));
        });
    }

    #[test]
    fn owned_small_decoder_frames_preserve_environment_reference_identity() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let words = [
                SEXPTYPE::VECSXP.as_c_int(),
                2,
                SEXPTYPE::ENVSXP.as_c_int(),
                0,
                EMPTYENV_SXP,
                NILVALUE_SXP,
                NILVALUE_SXP,
                NILVALUE_SXP,
                (1 << 8) | REFSXP,
            ];
            let bytes: Vec<u8> = words.into_iter().flat_map(i32::to_ne_bytes).collect();
            let mut reader = BinaryReader::new(&bytes);
            let owner = OwnerToken::current().unwrap();
            let value = owner
                .sexp(ReadItemInternal(&mut reader, &mut ReadRefTable::new()).unwrap())
                .unwrap()
                .into_owned()
                .unwrap();
            let left = value.try_vector_elt(0).unwrap().into_owned().unwrap();
            let right = value.try_vector_elt(1).unwrap().into_owned().unwrap();
            assert_eq!(left, right);
            assert_eq!(left.typeof_(), SEXPTYPE::ENVSXP);
            owner.full_gc().unwrap();
            assert_eq!(
                value.try_vector_elt(0).unwrap(),
                value.try_vector_elt(1).unwrap()
            );
            assert!(left.allocation().unwrap().is_live());
            assert_eq!(reader.item_depth, 0);
        });
    }

    #[test]
    fn small_decoder_frames_keep_original_depth_and_truncation_rejections() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let mut truncated = nested_vectors(96);
            truncated.truncate(truncated.len() - 4);
            let mut reader = BinaryReader::new(&truncated);
            assert!(ReadItemInternal(&mut reader, &mut ReadRefTable::new()).is_err());
            assert_eq!(reader.item_depth, 0);
            let bytes = nested_vectors(128);
            let mut reader = BinaryReader::new(&bytes);
            let error = ReadItemInternal(&mut reader, &mut ReadRefTable::new()).unwrap_err();
            assert_eq!(
                error,
                "read error: serialized object nesting exceeds 128 levels"
            );
            assert_eq!(reader.item_depth, 0);
        });
    }
}
