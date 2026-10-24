#![forbid(unsafe_code)]

pub(super) fn scalar_end(source: &str, at: usize) -> usize {
    at + source[at..]
        .chars()
        .next()
        .expect("character boundary")
        .len_utf8()
}

/// The letter category uses the same pinned set as POSIX alpha, with or
/// without UCP. Other property families remain with the regex backend.
pub(super) fn letter_property(source: &str, at: usize) -> Option<(usize, bool)> {
    let bytes = source.as_bytes();
    if bytes.get(at) != Some(&b'\\') || !matches!(bytes.get(at + 1), Some(b'p' | b'P')) {
        return None;
    }
    let start = at + 2;
    bytes.get(start)?;
    let (name, end) = if bytes.get(start) == Some(&b'{') {
        let close = source[start + 1..].find('}')? + start + 1;
        (&source[start + 1..close], close + 1)
    } else {
        let end = scalar_end(source, start);
        (&source[start..end], end)
    };
    (name.eq_ignore_ascii_case("L") || name.eq_ignore_ascii_case("Letter"))
        .then_some((end, bytes[at + 1] == b'P'))
}
pub(super) fn quote_end(source: &str, start: usize) -> (usize, usize) {
    source[start..]
        .find(r"\E")
        .map_or((source.len(), source.len()), |n| (start + n, start + n + 2))
}
pub(super) fn quoted(source: &str, class: bool, emit: &mut impl FnMut(&str)) {
    for ch in source.chars() {
        if (if class { r"\[]^-&" } else { r"\.^$|()[]{}*+?" }).contains(ch) {
            emit(r"\");
        }
        let mut encoded = [0; 4];
        emit(ch.encode_utf8(&mut encoded));
    }
}
