use super::{
    iconv_codec::{Codec, lookup, visit},
    iconv_owned::convert,
};
use crate::sexp::{RSession, memory::ArenaBudget};
fn run(bytes: &[u8], from: Codec, to: Codec, sub: Option<&[u8]>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    visit(bytes, from, to, sub, false, |part| {
        out.extend_from_slice(part);
        Ok::<(), ()>(())
    })
    .unwrap()
    .then_some(out)
}
#[test]
fn iconv_codec_surrogate_pairs_and_endian_outputs_match_gnu_bytes() {
    let utf8 = "Aé€🙂".as_bytes();
    for (codec, raw) in [
        (
            Codec::Utf16Le,
            &[0x41, 0, 0xe9, 0, 0xac, 0x20, 0x3d, 0xd8, 0x42, 0xde][..],
        ),
        (
            Codec::Utf16Be,
            &[0, 0x41, 0, 0xe9, 0x20, 0xac, 0xd8, 0x3d, 0xde, 0x42],
        ),
        (
            Codec::Utf32Le,
            &[
                0x41, 0, 0, 0, 0xe9, 0, 0, 0, 0xac, 0x20, 0, 0, 0x42, 0xf6, 1, 0,
            ],
        ),
        (
            Codec::Utf32Be,
            &[
                0, 0, 0, 0x41, 0, 0, 0, 0xe9, 0, 0, 0x20, 0xac, 0, 1, 0xf6, 0x42,
            ],
        ),
    ] {
        assert_eq!(run(utf8, Codec::Utf8, codec, None).unwrap(), raw);
        assert_eq!(run(raw, codec, Codec::Utf8, None).unwrap(), utf8);
    }
}
#[test]
fn iconv_codec_invalid_and_partial_units_resume_without_losing_valid_neighbors() {
    for (raw, from, expected) in [
        (&[65, 255, 66][..], Codec::Utf8, "A<ff>B"),
        (&[65, 0, 0, 0xd8, 66, 0], Codec::Utf16Le, "A<00><d8>B"),
        (&[65, 0, 233], Codec::Utf16Le, "A<e9>"),
        (&[0, 0, 0, 65, 0xd8], Codec::Utf32Be, "A<d8>"),
        (&[0, 0xd8, 0, 0], Codec::Utf32Le, "<00><d8><00><00>"),
    ] {
        assert_eq!(
            run(raw, from, Codec::Utf8, Some(b"byte")).unwrap(),
            expected.as_bytes()
        );
        assert!(run(raw, from, Codec::Utf8, None).is_none());
    }
}
#[test]
fn iconv_codec_unmapped_substitution_uses_original_bytes_and_unicode_policy() {
    assert_eq!(
        run("é".as_bytes(), Codec::Utf8, Codec::Ascii, Some(b"byte")).unwrap(),
        b"<c3><a9>"
    );
    assert_eq!(
        run("é".as_bytes(), Codec::Utf8, Codec::Ascii, Some(b"?")).unwrap(),
        b"??"
    );
    assert_eq!(
        run(&[233], Codec::Latin1, Codec::Ascii, Some(b"?")).unwrap(),
        b"?"
    );
    assert_eq!(
        run("🙂".as_bytes(), Codec::Utf8, Codec::Ascii, Some(b"Unicode")).unwrap(),
        b"<U+1F642>"
    );
    assert_eq!(
        run("🙂".as_bytes(), Codec::Utf8, Codec::Ascii, Some(b"c99")).unwrap(),
        b"\\U0001f642"
    );
    assert_eq!(
        run("é".as_bytes(), Codec::Utf8, Codec::Ascii, Some(b"")).unwrap(),
        b""
    );
}
#[test]
fn iconv_codec_unknown_names_reject_and_gnu_extended_utf32_never_forms_invalid_rust_text() {
    assert!(lookup("not-an-encoding").is_none());
    assert_eq!(lookup("uTf-16lE"), Some(Codec::Utf16Le));
    for (raw, expected) in [
        (&[0, 0, 0x11, 0][..], &[0xf4, 0x90, 0x80, 0x80][..]),
        (&[0, 0, 0x20, 0], &[0xf8, 0x88, 0x80, 0x80, 0x80]),
        (&[255, 255, 255, 127], &[0xfd, 0xbf, 0xbf, 0xbf, 0xbf, 0xbf]),
    ] {
        let result = run(raw, Codec::Utf32Le, Codec::Utf8, None).unwrap();
        assert_eq!(result, expected);
        assert!(std::str::from_utf8(&result).is_err());
    }
}
#[test]
fn iconv_workspace_exact_boundary_denies_before_copy_and_recovers_at_same_limit() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let parent = owner
            .node_factory()
            .strings(&["parent"])
            .unwrap()
            .into_owned()
            .unwrap();
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before + 7, 0)))
            .unwrap();
        assert!(
            convert(
                &parent,
                "é".as_bytes(),
                Codec::Utf8,
                Codec::Ascii,
                Some(b"byte"),
                false
            )
            .is_err()
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
        let small = convert(
            &parent,
            "é".as_bytes(),
            Codec::Utf8,
            Codec::Ascii,
            Some(b"?"),
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(small.data, b"??");
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 2);
        drop(small);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert_eq!(
            owner.with_arena(|a| a.budget().max_bytes).unwrap(),
            before + 7
        );
    });
}
#[test]
fn iconv_workspace_original_heap_keeps_charge_across_foreign_activation_and_unwind() {
    let original = RSession::new_for_gc_tests();
    let foreign = RSession::new_for_gc_tests();
    let owner = original.owner_token().unwrap();
    let parent = original.with_active(|| {
        owner
            .node_factory()
            .strings(&["parent"])
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let foreign_owner = foreign.owner_token().unwrap();
    foreign.with_active(|| {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let bytes = convert(
                &parent,
                "é".as_bytes(),
                Codec::Utf8,
                Codec::Ascii,
                Some(b"byte"),
                false,
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                original.with_active(|| owner.with_arena(|a| a.workspace_bytes()).unwrap()),
                8
            );
            assert_eq!(
                foreign_owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                0
            );
            assert_eq!(bytes.data, b"<c3><a9>");
            std::panic::panic_any(912u32);
        }));
        assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 912);
        assert_eq!(
            original.with_active(|| owner.with_arena(|a| a.workspace_bytes()).unwrap()),
            0
        );
        assert_eq!(
            foreign_owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            0
        );
    });
}
