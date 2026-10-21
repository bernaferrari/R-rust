#![forbid(unsafe_code)]
//! Single-scalar upstream mappings; full Unicode string expansions are different.
use super::case_tables::{LOWER, UPPER};
fn mapped(character: char, table: &[(char, char)]) -> char {
    table
        .binary_search_by_key(&character, |&(from, _)| from)
        .map_or(character, |index| table[index].1)
}
pub(crate) fn simple_upper(character: char) -> char {
    if character.is_ascii() {
        character.to_ascii_uppercase()
    } else {
        mapped(character, UPPER)
    }
}
pub(crate) fn simple_lower(character: char) -> char {
    if character.is_ascii() {
        character.to_ascii_lowercase()
    } else {
        mapped(character, LOWER)
    }
}
