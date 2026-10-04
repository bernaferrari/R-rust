//! Exercise actual R serialization format selection, beyond scalar helpers.
use super::*;
use crate::sexp::{
    RSession, SexpValue,
    object::{SessionNodeFactory, Sexp, SexpMut},
};

fn scalar<'s>(factory: &SessionNodeFactory<'s>, kind: SEXPTYPE, value: i32) -> Sexp<'s> {
    let value_handle = factory
        .allocate(|arena| Some(arena.alloc_vector(kind, 1)))
        .unwrap();
    let mut handle = SexpMut::try_from_checked(value_handle).unwrap();
    if kind == SEXPTYPE::LGLSXP {
        handle.try_set_logical_elt(0, value).unwrap();
    } else {
        handle.try_set_integer_elt(0, value).unwrap();
    }
    handle.freeze()
}

fn serialize(
    factory: &SessionNodeFactory<'_>,
    value: &Sexp<'_>,
    mode: &Sexp<'_>,
    version: &Sexp<'_>,
) -> Vec<u8> {
    // All projected operands remain owning, on the activated original runtime.
    let nil = factory.nil();
    let result = factory
        .wrap(unsafe {
            R_serialize_with_xdr(
                value.as_raw(),
                nil.as_raw(),
                mode.as_raw(),
                nil.as_raw(),
                version.as_raw(),
                nil.as_raw(),
            )
        })
        .unwrap();
    let SexpValue::RawVector(bytes) = result.to_owned_value().unwrap() else {
        panic!("serialization must return an owning raw byte snapshot");
    };
    bytes
}

#[test]
fn public_ascii_modes_select_gnu_decimal_and_exact_hexadecimal_tokens() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let decimal_mode = scalar(&factory, SEXPTYPE::LGLSXP, 1);
        let hexadecimal_mode = scalar(&factory, SEXPTYPE::LGLSXP, crate::sexp::ffi::NA_LOGICAL);
        let cases: Vec<_> = super::ascii_numeric_tests::cases().collect();
        assert_eq!(cases.len(), 43);
        let input = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, cases.len() as R_xlen_t)))
            .unwrap();
        let mut input = SexpMut::try_from_checked(input).unwrap();
        for (index, (_, value, _, _)) in cases.iter().enumerate() {
            input.try_set_real_elt(index as R_xlen_t, *value).unwrap();
        }
        let input = input.freeze();
        session.owner_token().unwrap().full_gc().unwrap();
        for version in [2, 3] {
            let version_handle = scalar(&factory, SEXPTYPE::INTSXP, version);
            for (mode, hexadecimal) in [(&decimal_mode, false), (&hexadecimal_mode, true)] {
                let tokens = cases
                    .iter()
                    .map(|(_, _, decimal, hex)| if hexadecimal { *hex } else { *decimal })
                    .collect::<Vec<_>>()
                    .join("\n");
                let bytes = serialize(&factory, &input, mode, &version_handle);
                let text = String::from_utf8(bytes).unwrap();
                assert!(text.starts_with(&format!("A\n{version}\n")));
                assert!(text.ends_with(&format!("\n43\n{tokens}\n")), "{text}");
            }
        }
    });
}

#[test]
fn serialization_minimum_reader_matches_original_gnu_version_selection() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let input = scalar(&factory, SEXPTYPE::INTSXP, crate::sexp::ffi::NA_INTEGER);
        let mode = scalar(&factory, SEXPTYPE::LGLSXP, 1);
        for (version, minimum) in [(2, R_VERSION_230), (3, R_VERSION_350)] {
            let version = scalar(&factory, SEXPTYPE::INTSXP, version);
            let text = String::from_utf8(serialize(&factory, &input, &mode, &version)).unwrap();
            assert_eq!(text.lines().nth(3), Some(minimum.to_string().as_str()));
            assert!(text.ends_with("\n1\nNA\n"));
        }
    });
}
