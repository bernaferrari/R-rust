//! Pinned GNU default-locale display columns, without allocations or runtime access.
#![forbid(unsafe_code)]
mod tables;

fn contains(table: &[(u32, u32)], point: u32) -> bool {
    let i = table.partition_point(|&(first, _)| first <= point);
    i != 0 && point <= table[i - 1].1
}

pub(crate) fn columns(ch: char) -> usize {
    let point = ch as u32;
    if contains(tables::WIDE, point) {
        2
    } else if contains(tables::ZERO, point) {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_column_tables_are_ordered_and_disjoint() {
        for table in [tables::WIDE, tables::ZERO] {
            assert!(
                table
                    .iter()
                    .all(|&(first, last)| first <= last && last <= 0x10FFFF)
            );
            assert!(table.windows(2).all(|pair| pair[0].1 < pair[1].0));
        }
        for &(first, last) in tables::WIDE {
            assert!(!tables::ZERO.iter().any(|&(a, b)| first <= b && a <= last));
        }
    }

    #[test]
    fn unicode_columns_match_gnu_combining_wide_emoji_and_ascii() {
        for (text, expected) in [
            ("‘alpha’", 7),
            ("界", 2),
            ("e\u{301}", 1),
            ("🙂", 2),
            ("é", 1),
            ("\u{200B}", 0),
            ("\n", 0),
        ] {
            assert_eq!(text.chars().map(columns).sum::<usize>(), expected);
        }
    }
}
