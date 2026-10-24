#![forbid(unsafe_code)]

/// Engine-specific progression for GNU's empty-pattern path.
#[derive(Clone, Copy, Debug)]
pub(crate) enum EmptyPatternMode {
    EreCharacters,
    EreBytes,
    PerlCharacters,
    Bytes,
}

/// PCRE projects interior UTF-8 byte offsets without forming invalid str slices.
/// ERE has no empty-subject match; PCRE and fixed do. No table is allocated here.
pub(crate) fn empty_pattern_positions(
    text: &str,
    mode: EmptyPatternMode,
) -> impl ExactSizeIterator<Item = usize> + '_ {
    let count = match mode {
        EmptyPatternMode::EreCharacters => text.chars().count(),
        EmptyPatternMode::EreBytes => text.len(),
        EmptyPatternMode::PerlCharacters | EmptyPatternMode::Bytes => text.len().max(1),
    };
    let mut characters = text.char_indices().peekable();
    let mut position = 1;
    (0..count).map(move |offset| {
        if !matches!(mode, EmptyPatternMode::PerlCharacters) {
            return offset + 1;
        }
        while characters.peek().is_some_and(|(start, _)| *start < offset) {
            characters.next();
            position += 1;
        }
        position
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_positions_match_pinned_gnu_utf8_and_byte_coordinate_rows() {
        for (text, expected) in [
            ("", vec![1]),
            ("a", vec![1]),
            ("é", vec![1, 2]),
            ("éa", vec![1, 2, 2]),
            ("aé", vec![1, 2, 3]),
            ("😀a", vec![1, 2, 2, 2, 2]),
            ("a😀", vec![1, 2, 3, 3, 3]),
            ("é😀", vec![1, 2, 2, 3, 3, 3]),
        ] {
            let positions = empty_pattern_positions(text, EmptyPatternMode::PerlCharacters);
            assert_eq!(positions.len(), text.len().max(1));
            assert_eq!(positions.collect::<Vec<_>>(), expected, "{text:?}");
            assert_eq!(
                empty_pattern_positions(text, EmptyPatternMode::Bytes).collect::<Vec<_>>(),
                (1..=text.len().max(1)).collect::<Vec<_>>(),
                "byte positions for {text:?}"
            );
            for (mode, count) in [
                (EmptyPatternMode::EreCharacters, text.chars().count()),
                (EmptyPatternMode::EreBytes, text.len()),
            ] {
                let positions = empty_pattern_positions(text, mode);
                assert_eq!(positions.len(), count);
                assert_eq!(
                    positions.collect::<Vec<_>>(),
                    (1..=count).collect::<Vec<_>>(),
                    "{mode:?}: {text:?}"
                );
            }
        }
    }
    #[test]
    fn owned_gregexpr_position_clear_retains_original_charge_with_foreign_ambient() {
        use crate::sexp::session::RSession;
        let first = RSession::new_for_gc_tests();
        let owner = first.owner_token().unwrap();
        let witness = owner.node_factory().strings(&["position-ledger"]).unwrap();
        let ledger = witness.allocation().unwrap().heap_identity().byte_ledger();
        let workspace = super::super::regex_workspace();
        let second = RSession::new_for_gc_tests();
        let foreign = second.owner_token().unwrap();
        let before = foreign
            .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
            .unwrap();
        let mut positions = workspace.buffer::<usize>(8);
        positions.push(1);
        let retained = 8 * std::mem::size_of::<usize>();
        assert_eq!(ledger.workspace_bytes_for_test(), retained);
        positions.clear();
        assert!(positions.is_empty());
        assert_eq!(ledger.workspace_bytes_for_test(), retained);
        assert_eq!(
            foreign
                .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
                .unwrap(),
            before
        );
        drop(positions);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(
            foreign
                .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
                .unwrap(),
            before
        );
    }
}
