#![forbid(unsafe_code)]
use super::charset::Kind;

#[derive(Clone, Copy, Default)]
pub(super) struct Flags {
    i: bool,
    m: bool,
    s: bool,
    x: bool,
    more: bool,
    ungreedy: bool,
    ascii_word: bool,
    ascii_digit: bool,
    ascii_space: bool,
    ascii_posix: bool,
    ascii_posix_digit: bool,
}
impl Flags {
    pub(super) fn new(i: bool) -> Self {
        Self {
            i,
            ..Self::default()
        }
    }
    pub(super) fn caseless(self) -> bool {
        self.i
    }
    pub(super) fn extended(self) -> bool {
        self.x
    }
    pub(super) fn multiline(self) -> bool {
        self.m
    }
    pub(super) fn extended_more(self) -> bool {
        self.more
    }

    pub(super) fn unicode(self, ucp: bool, kind: Kind, posix: bool) -> bool {
        ucp && !(if posix {
            self.ascii_posix
                || (matches!(kind, Kind::Digit | Kind::Xdigit) && self.ascii_posix_digit)
        } else {
            match kind {
                Kind::Word => self.ascii_word,
                Kind::Digit => self.ascii_digit,
                Kind::Space => self.ascii_space,
                _ => false,
            }
        })
    }
}

pub(super) fn parse_flags(
    source: &str,
    at: usize,
    mut flags: Flags,
) -> Option<(usize, bool, Flags)> {
    let bytes = source.as_bytes();
    if !source[at..].starts_with("(?") {
        return None;
    }
    let mut index = at + 2;
    let mut enabled = true;
    let mut seen = false;
    while let Some(&byte) = bytes.get(index) {
        if byte == b':' || byte == b')' {
            return seen.then_some((index + 1, byte == b':', flags));
        }
        if byte == b'-' {
            enabled = false;
            index += 1;
            continue;
        }
        if byte == b'^' {
            flags.i = false;
            flags.m = false;
            flags.s = false;
            flags.x = false;
            flags.more = false;
            flags.ungreedy = false;
        } else {
            match byte {
                b'i' => flags.i = enabled,
                b'm' => flags.m = enabled,
                b's' => flags.s = enabled,
                b'x' => {
                    flags.x = enabled;
                    flags.more = enabled && bytes.get(index + 1) == Some(&b'x');
                    if bytes.get(index + 1) == Some(&b'x') {
                        index += 1;
                    }
                }
                b'U' => flags.ungreedy = enabled,
                b'a' => match bytes.get(index + 1) {
                    Some(b'W') => {
                        flags.ascii_word = enabled;
                        index += 1;
                    }
                    Some(b'D') => {
                        flags.ascii_digit = enabled;
                        index += 1;
                    }
                    Some(b'S') => {
                        flags.ascii_space = enabled;
                        index += 1;
                    }
                    Some(b'P') => {
                        flags.ascii_posix = enabled;
                        flags.ascii_posix_digit = enabled;
                        index += 1;
                    }
                    Some(b'T') => {
                        flags.ascii_posix_digit = enabled;
                        index += 1;
                    }
                    _ => {
                        flags.ascii_word = enabled;
                        flags.ascii_digit = enabled;
                        flags.ascii_space = enabled;
                        flags.ascii_posix = enabled;
                        flags.ascii_posix_digit = enabled;
                    }
                },
                _ => return None,
            }
        }
        seen = true;
        index += 1;
    }
    None
}
pub(super) fn flags_text(flags: Flags, scoped: bool, emit: &mut impl FnMut(&str)) {
    let values = [
        (flags.i, "i"),
        (flags.m, "m"),
        (flags.s, "s"),
        (flags.x, "x"),
        (flags.ungreedy, "U"),
    ];
    emit("(?");
    for (yes, name) in values {
        if yes {
            emit(name);
        }
    }
    if values.iter().any(|(yes, _)| !yes) {
        emit("-");
        for (yes, name) in values {
            if !yes {
                emit(name);
            }
        }
    }
    emit(if scoped { ":" } else { ")" });
}
