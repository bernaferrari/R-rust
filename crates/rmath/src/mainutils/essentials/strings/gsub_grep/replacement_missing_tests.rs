use super::*;

#[test]
fn missing_replacements_use_selected_matcher_and_keep_original_values() {
    let session = crate::sexp::RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let domain = factory.domain();
        for (pattern, source, perl, fixed, bytes, missing_pattern, expected) in [
            ("\\b", ["é", "éa"], true, false, false, false, [false, true]),
            (
                "(?<=a)b",
                ["ab", "cb"],
                true,
                false,
                false,
                false,
                [true, false],
            ),
            ("a*", ["bbb", ""], true, false, false, false, [true, true]),
            ("^", ["ab", ""], false, false, false, false, [true, true]),
            (
                ".",
                ["a.b", "axb"],
                false,
                true,
                false,
                false,
                [true, false],
            ),
            ("[", ["[", "abc"], false, true, false, false, [true, false]),
            ("a", ["a", "b"], false, false, true, false, [true, false]),
            ("NA", ["a", "b"], false, false, true, true, [true, true]),
        ] {
            let pattern = factory.strings(&[pattern]).unwrap();
            if missing_pattern {
                SET_STRING_ELT(pattern.as_raw(), 0, crate::sexp::globals::R_NaString());
            }
            let replacement = factory.strings(&["placeholder"]).unwrap();
            SET_STRING_ELT(replacement.as_raw(), 0, crate::sexp::globals::R_NaString());
            let source = factory.strings(&source).unwrap();
            let flags = [
                domain.logical(false),
                domain.logical(perl),
                domain.logical(fixed),
                domain.logical(bytes),
            ];
            let values = [
                pattern.as_raw(),
                replacement.as_raw(),
                source.as_raw(),
                flags[0].as_raw(),
                flags[1].as_raw(),
                flags[2].as_raw(),
                flags[3].as_raw(),
            ];
            let mut args = R_NilValue();
            let mut roots = Vec::new();
            for value in values.into_iter().rev() {
                args = Rf_cons(value, args);
                roots.push(protect(args));
            }
            let result = super::do_string_replace(args, false);
            let _result = protect(result);
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
            assert_eq!(XLENGTH(result), 2);
            for (i, missing) in expected.into_iter().enumerate() {
                let value = STRING_ELT(result, i as R_xlen_t);
                if missing {
                    assert_eq!(value, crate::sexp::globals::R_NaString());
                } else {
                    assert_eq!(value, STRING_ELT(source.as_raw(), i as R_xlen_t));
                }
            }
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                0
            );
        }
    });
}
