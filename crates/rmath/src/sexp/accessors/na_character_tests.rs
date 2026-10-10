//! Checked canonical missing-character bytes agree with original GNU names.c.
use super::*;
use crate::sexp::{
    object::{SessionNodeFactory, Sexp},
    session::RSession,
};

fn bytes(pointer: *const c_char) -> Vec<u8> {
    assert!(
        !pointer.is_null(),
        "a canonical missing string still has native NA bytes"
    );
    // SAFETY: each caller retains the admitted character or static singleton,
    // and the const projection has a checked trailing NUL.
    unsafe { std::ffi::CStr::from_ptr(pointer).to_bytes().to_vec() }
}

#[test]
fn owned_na_character_const_bytes_preserve_typed_missing_identity() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let missing = f
            .wrap(unsafe { crate::sexp::globals::R_NaString() })
            .unwrap();
        let literal = f.character("NA").unwrap();
        let utf8 = f.character("é").unwrap();
        assert_eq!(
            bytes(unsafe { translateCharUTF8(utf8.as_raw()) }),
            "é".as_bytes()
        );
        let values = f.strings(&["present", "NA"]).unwrap();
        unsafe {
            SET_STRING_ELT(values.as_raw(), 0, missing.as_raw());
        }
        assert_eq!(values.try_string_value_elt(0).unwrap(), None);
        assert_eq!(
            values.try_string_value_elt(1).unwrap().as_deref(),
            Some("NA")
        );
        assert_ne!(missing.as_raw(), literal.as_raw());
        for pointer in unsafe {
            [
                CHAR(missing.as_raw()),
                translateChar(missing.as_raw()),
                translateCharUTF8(missing.as_raw()),
                ROBJ_DATAPTR(missing.as_raw()).cast(),
            ]
        } {
            assert_eq!(bytes(pointer), b"NA");
        }
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                DATAPTR(missing.as_raw());
            }))
            .is_err()
        );
        assert_eq!(bytes(unsafe { CHAR(literal.as_raw()) }), b"NA");
    });
}

#[test]
fn owned_na_character_admission_retains_original_bank_and_rejects_forged_headers() {
    let session = RSession::new_for_gc_tests();
    let f = session.owner_token().unwrap().node_factory();
    let missing = f
        .wrap(unsafe { crate::sexp::globals::R_NaString() })
        .unwrap();
    let holder = session.with_active(|| {
        let holder = f.strings(&[""]).unwrap();
        unsafe {
            SET_STRING_ELT(holder.as_raw(), 0, missing.as_raw());
        }
        holder
    });
    crate::sexp::globals::close_immutable_singletons_for_test();
    assert_ne!(missing.as_raw(), unsafe {
        crate::sexp::globals::R_NaString()
    });
    session.with_active(|| {
        assert!(missing.is_na_string());
        assert_eq!(holder.try_string_value_elt(0).unwrap(), None);
        assert_eq!(bytes(unsafe { CHAR(missing.as_raw()) }), b"NA");
        let fake = Box::new(SexprecCore::new(SEXPTYPE::CHARSXP));
        let pointer = ptr::from_ref(fake.as_ref()).cast_mut();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                CHAR(pointer);
            }))
            .is_err()
        );
        let retired = {
            let mut arena = crate::sexp::memory::RArena::new();
            arena.alloc_charsxp(b"retired")
        };
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                CHAR(retired);
            }))
            .is_err()
        );
        // Registered foreign/standalone const reads keep their existing checked
        // contract: an active runtime never supplies a replacement identity.
        let mut foreign = crate::sexp::memory::RArena::new();
        let character = foreign.alloc_charsxp(b"foreign");
        assert_eq!(bytes(unsafe { CHAR(character) }), b"foreign");
        let empty_character = foreign.alloc_node(SEXPTYPE::CHARSXP);
        // An initialized empty CHARSXP shares the sentinel's type/length, but
        // these fields do not grant canonical missing-string identity.
        assert_eq!(bytes(unsafe { CHAR(empty_character) }), b"");
        assert!(
            !Sexp::from_arena_raw(empty_character, &foreign)
                .unwrap()
                .is_na_string()
        );
        assert!(unsafe { CHAR(ptr::null_mut()) }.is_null());
    });
}

fn vector<'s>(
    f: &SessionNodeFactory<'s>,
    kind: SEXPTYPE,
    length: usize,
    initialize: impl FnOnce(SEXP),
) -> Sexp<'s> {
    f.allocate(|arena| {
        let pointer = arena.alloc_vector(kind, length as _);
        initialize(pointer);
        Some(pointer)
    })
    .unwrap()
}

fn paste<'s>(f: &SessionNodeFactory<'s>, value: &Sexp<'s>) -> Sexp<'s> {
    let inputs = vector(f, SEXPTYPE::VECSXP, 1, |pointer| unsafe {
        SET_VECTOR_ELT(pointer, 0, value.as_raw());
    });
    let separator = f.strings(&[""]).unwrap();
    let mut args = f.nil();
    for value in [&f.domain().logical(false), &f.nil(), &separator, &inputs] {
        args = f.pairlist_cell(value, &args, &f.nil()).unwrap();
    }
    let primitive = f
        .wrap(unsafe {
            crate::eval::primitive::make_primitive_binding("paste", SEXPTYPE::BUILTINSXP)
        })
        .unwrap();
    f.wrap(unsafe {
        crate::mainutils::paste::do_paste(
            f.nil().as_raw(),
            primitive.as_raw(),
            args.as_raw(),
            f.nil().as_raw(),
        )
    })
    .unwrap()
}

#[test]
fn owned_na_character_paste_preserves_missing_text_for_every_atomic_kind() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let integer = vector(&f, SEXPTYPE::INTSXP, 3, |pointer| unsafe {
            for (i, x) in [3, NA_INTEGER, 4].into_iter().enumerate() {
                SET_INTEGER_ELT(pointer, i as _, x);
            }
        });
        let logical = vector(&f, SEXPTYPE::LGLSXP, 3, |pointer| unsafe {
            for (i, x) in [1, NA_INTEGER, 0].into_iter().enumerate() {
                SET_LOGICAL_ELT(pointer, i as _, x);
            }
        });
        let real = vector(&f, SEXPTYPE::REALSXP, 3, |pointer| unsafe {
            for (i, x) in [3., NA_REAL, 4.].into_iter().enumerate() {
                SET_REAL_ELT(pointer, i as _, x);
            }
        });
        let characters = f.strings(&["a", "", "b"]).unwrap();
        let all_missing = f.strings(&["", ""]).unwrap();
        let missing = f
            .wrap(unsafe { crate::sexp::globals::R_NaString() })
            .unwrap();
        unsafe {
            SET_STRING_ELT(characters.as_raw(), 1, missing.as_raw());
            SET_STRING_ELT(all_missing.as_raw(), 0, missing.as_raw());
            SET_STRING_ELT(all_missing.as_raw(), 1, missing.as_raw());
        }
        for (input, expected) in [
            (&integer, ["3", "NA", "4"].as_slice()),
            (&logical, ["TRUE", "NA", "FALSE"].as_slice()),
            (&real, ["3", "NA", "4"].as_slice()),
            (&characters, ["a", "NA", "b"].as_slice()),
            (&all_missing, ["NA", "NA"].as_slice()),
        ] {
            let output = paste(&f, input);
            let actual: Vec<_> = (0..output.len())
                .map(|i| output.try_string_value_elt(i).unwrap().unwrap())
                .collect();
            assert_eq!(actual, expected);
        }
        assert_eq!(integer.try_integer_elt(1).unwrap(), NA_INTEGER);
        assert_eq!(logical.try_logical_elt(1).unwrap(), NA_INTEGER);
        assert!(crate::sexp::ffi::is_na_real(real.try_real_elt(1).unwrap()));
        assert_eq!(characters.try_string_value_elt(1).unwrap(), None);
    });
}
