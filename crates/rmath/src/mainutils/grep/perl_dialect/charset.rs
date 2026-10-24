#![forbid(unsafe_code)]
use super::{flags::Flags, tables};

#[derive(Clone, Copy, Debug)]
pub(super) enum Kind {
    Word,
    Digit,
    Space,
    Alpha,
    Alnum,
    Blank,
    Control,
    Lower,
    Upper,
    Graph,
    Print,
    Punct,
    Xdigit,
    Ascii,
}
impl Kind {
    pub(super) fn posix(name: &str) -> Option<Self> {
        Some(match name {
            "word" => Self::Word,
            "digit" => Self::Digit,
            "space" => Self::Space,
            "alpha" => Self::Alpha,
            "alnum" => Self::Alnum,
            "blank" => Self::Blank,
            "cntrl" => Self::Control,
            "lower" => Self::Lower,
            "upper" => Self::Upper,
            "graph" => Self::Graph,
            "print" => Self::Print,
            "punct" => Self::Punct,
            "xdigit" => Self::Xdigit,
            "ascii" => Self::Ascii,
            _ => return None,
        })
    }
    pub(super) fn escape(byte: u8) -> Option<(Self, bool)> {
        Some(match byte {
            b'w' => (Self::Word, false),
            b'W' => (Self::Word, true),
            b'd' => (Self::Digit, false),
            b'D' => (Self::Digit, true),
            b's' => (Self::Space, false),
            b'S' => (Self::Space, true),
            _ => return None,
        })
    }
    fn ascii(self, caseless: bool) -> &'static str {
        match self {
            Self::Word => "[A-Za-z0-9_]",
            Self::Digit => "[0-9]",
            Self::Space => r"[\x09-\x0d\x20]",
            Self::Alpha => "[A-Za-z]",
            Self::Alnum => "[A-Za-z0-9]",
            Self::Blank => r"[\x09\x20]",
            Self::Control => r"[\x00-\x1f\x7f]",
            Self::Lower | Self::Upper if caseless => "[A-Za-z]",
            Self::Lower => "[a-z]",
            Self::Upper => "[A-Z]",
            Self::Graph => r"[\x21-\x7e]",
            Self::Print => r"[\x20-\x7e]",
            Self::Punct => r"[\x21-\x2f\x3a-\x40\x5b-\x60\x7b-\x7e]",
            Self::Xdigit => "[0-9A-Fa-f]",
            Self::Ascii => r"[\x00-\x7f]",
        }
    }
    fn unicode(self) -> &'static str {
        match self {
            Self::Word => r"[\p{L}\p{N}\p{Mn}\p{Pc}]",
            Self::Digit => r"[\p{Nd}]",
            Self::Space => r"[\p{Z}\x09-\x0d\x85\x{180e}]",
            Self::Alpha => r"[\p{L}]",
            Self::Alnum => r"[\p{L}\p{N}]",
            Self::Blank => {
                r"[\x09\x20\xa0\x{1680}\x{180e}\x{2000}-\x{200a}\x{202f}\x{205f}\x{3000}]"
            }
            Self::Control => r"[\p{Cc}]",
            Self::Lower => r"[\p{Ll}]",
            Self::Upper => r"[\p{Lu}]",
            Self::Graph => {
                r"[[\p{L}\p{M}\p{N}\p{P}\p{S}\p{Cf}]--[\x{61c}\x{180e}\x{2066}-\x{2069}]]"
            }
            Self::Print => {
                r"[[[\p{L}\p{M}\p{N}\p{P}\p{S}\p{Cf}]--[\x{61c}\x{180e}\x{2066}-\x{2069}]]\p{Zs}]"
            }
            Self::Punct => r"[\p{P}[\p{S}&&[\x00-\xff]]]",
            Self::Xdigit => r"[0-9A-Fa-f\x{ff10}-\x{ff19}\x{ff21}-\x{ff26}\x{ff41}-\x{ff46}]",
            Self::Ascii => r"[\x00-\x7f]",
        }
    }
}

pub(super) fn set(
    kind: Kind,
    negative: bool,
    unicode: bool,
    flags: Flags,
    emit: &mut impl FnMut(&str),
) {
    // Ctype/POSIX membership is not literal Unicode case folding.
    emit("(?-i:[");
    if negative {
        emit("^");
    }
    emit("[");
    emit(if unicode {
        kind.unicode()
    } else {
        kind.ascii(flags.caseless())
    });
    if unicode {
        emit(tables::missing(kind));
    }
    emit("]");
    if unicode && !tables::extra(kind).is_empty() {
        emit("--[");
        emit(tables::extra(kind));
        emit("]");
    }
    emit("])");
}

pub(super) fn boundary(negative: bool, ucp: bool, flags: Flags, emit: &mut impl FnMut(&str)) {
    let unicode = flags.unicode(ucp, Kind::Word, false);
    emit("(?:(?<=");
    set(Kind::Word, false, unicode, flags, emit);
    emit(if negative { ")(?=" } else { ")(?!" });
    set(Kind::Word, false, unicode, flags, emit);
    emit(")|(?<!");
    set(Kind::Word, false, unicode, flags, emit);
    emit(if negative { ")(?!" } else { ")(?=" });
    set(Kind::Word, false, unicode, flags, emit);
    emit("))");
}
