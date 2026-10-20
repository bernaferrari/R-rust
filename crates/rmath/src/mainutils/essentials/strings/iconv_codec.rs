#![forbid(unsafe_code)]
//! Stateless Unicode codecs. Visit twice to admit exact output before copying.
use super::iconv_translit::ascii_translit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Codec {
    Utf8,
    Latin1,
    Ascii,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}
const ALIASES: &[(&str, Codec)] = &[
    ("ANSI_X3.4-1968", Codec::Ascii),
    ("ASCII", Codec::Ascii),
    ("ISO-8859-1", Codec::Latin1),
    ("ISO8859-1", Codec::Latin1),
    ("ISO_8859-1", Codec::Latin1),
    ("LATIN1", Codec::Latin1),
    ("US-ASCII", Codec::Ascii),
    ("UTF-16BE", Codec::Utf16Be),
    ("UTF-16LE", Codec::Utf16Le),
    ("UTF-32BE", Codec::Utf32Be),
    ("UTF-32LE", Codec::Utf32Le),
    ("UTF-8", Codec::Utf8),
    ("UTF8", Codec::Utf8),
    ("ascii", Codec::Ascii),
    ("iso-8859-1", Codec::Latin1),
    ("latin1", Codec::Latin1),
    ("utf-8", Codec::Utf8),
    ("utf8", Codec::Utf8),
];
pub(super) fn names() -> impl ExactSizeIterator<Item = &'static str> + Clone {
    ALIASES.iter().map(|&(name, _)| name)
}
pub(super) fn lookup(name: &str) -> Option<Codec> {
    let base = name.split("//").next()?;
    ALIASES
        .iter()
        .find(|&&(alias, _)| alias.eq_ignore_ascii_case(base))
        .map(|&(_, codec)| codec)
}
impl Codec {
    fn unit(self) -> usize {
        match self {
            Self::Utf16Le | Self::Utf16Be => 2,
            Self::Utf32Le | Self::Utf32Be => 4,
            _ => 1,
        }
    }
    fn decode(self, bytes: &[u8]) -> Option<(u32, usize)> {
        match self {
            Self::Latin1 => Some((u32::from(*bytes.first()?), 1)),
            Self::Ascii => bytes
                .first()
                .filter(|&&b| b < 128)
                .map(|&b| (u32::from(b), 1)),
            Self::Utf8 => {
                let width = match *bytes.first()? {
                    0..=127 => 1,
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf4 => 4,
                    _ => return None,
                };
                Some((
                    std::str::from_utf8(bytes.get(..width)?)
                        .ok()?
                        .chars()
                        .next()? as u32,
                    width,
                ))
            }
            Self::Utf16Le | Self::Utf16Be => {
                let read = |b: &[u8]| {
                    let b: [u8; 2] = b.get(..2)?.try_into().ok()?;
                    Some(if self == Self::Utf16Le {
                        u16::from_le_bytes(b)
                    } else {
                        u16::from_be_bytes(b)
                    })
                };
                let first = read(bytes)?;
                if (0xd800..=0xdbff).contains(&first) {
                    let second = read(bytes.get(2..)?)?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        return None;
                    }
                    Some((
                        0x10000 + ((u32::from(first) - 0xd800) << 10) + u32::from(second) - 0xdc00,
                        4,
                    ))
                } else {
                    Some((char::from_u32(u32::from(first))? as u32, 2))
                }
            }
            Self::Utf32Le | Self::Utf32Be => {
                let b: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
                let cp = if self == Self::Utf32Le {
                    u32::from_le_bytes(b)
                } else {
                    u32::from_be_bytes(b)
                };
                (!(0xd800..=0xdfff).contains(&cp)).then_some((cp, 4))
            }
        }
    }
    fn encode(self, cp: u32, output: &mut [u8; 8]) -> Option<usize> {
        match self {
            Self::Utf8 => {
                if let Some(ch) = char::from_u32(cp) {
                    return Some(ch.encode_utf8(output).len());
                }
                // GNU's UTF32 converter also accepts the historical 31-bit
                // UCS4 wire range. Encode integers directly; never construct
                // an invalid Rust char or str from these compatibility bytes.
                let width = if cp <= 0x1fffff {
                    4
                } else if cp <= 0x3ffffff {
                    5
                } else if cp <= 0x7fffffff {
                    6
                } else {
                    output[0] = b'?';
                    return Some(1);
                };
                let mut tail = cp;
                for i in (1..width).rev() {
                    output[i] = 0x80 | (tail as u8 & 63);
                    tail >>= 6;
                }
                output[0] = match width {
                    4 => 0xf0,
                    5 => 0xf8,
                    _ => 0xfc,
                } | tail as u8;
                Some(width)
            }
            Self::Latin1 | Self::Ascii => {
                let limit = if self == Self::Latin1 { 255 } else { 127 };
                if cp > limit {
                    return None;
                }
                output[0] = cp as u8;
                Some(1)
            }
            Self::Utf16Le | Self::Utf16Be => {
                let mut buf = [0; 2];
                let units = char::from_u32(cp)?.encode_utf16(&mut buf);
                for (i, &unit) in units.iter().enumerate() {
                    output[2 * i..2 * i + 2].copy_from_slice(&if self == Self::Utf16Le {
                        unit.to_le_bytes()
                    } else {
                        unit.to_be_bytes()
                    });
                }
                Some(units.len() * 2)
            }
            Self::Utf32Le | Self::Utf32Be => {
                output[..4].copy_from_slice(&if self == Self::Utf32Le {
                    cp.to_le_bytes()
                } else {
                    cp.to_be_bytes()
                });
                Some(4)
            }
        }
    }
}
/// The visitor can fail (e.g. checked size overflow); false denotes conversion
/// failure with sub=NA. Substitution text is copied as bytes, as in GNU iconv.
pub(super) fn visit<E>(
    bytes: &[u8],
    from: Codec,
    to: Codec,
    sub: Option<&[u8]>,
    translit: bool,
    mut emit: impl FnMut(&[u8]) -> Result<(), E>,
) -> Result<bool, E> {
    let mut i = 0;
    while i < bytes.len() {
        let decoded = from.decode(&bytes[i..]);
        let mut buffer = [0; 8];
        if let Some((ch, width)) = decoded {
            if let Some(n) = to.encode(ch, &mut buffer) {
                emit(&buffer[..n])?;
                i += width;
                continue;
            }
            if translit && to == Codec::Ascii {
                if let Some(text) = ascii_translit(ch) {
                    emit(text.as_bytes())?;
                    i += width;
                    continue;
                }
            }
            if from == Codec::Utf8 && matches!(sub, Some(b"Unicode" | b"c99")) {
                let cp = ch;
                let mut escape = [0; 12];
                let (prefix, digits, upper): (&[u8], usize, bool) = if sub == Some(b"Unicode") {
                    (
                        b"<U+",
                        if cp <= 0xffff {
                            4
                        } else if cp <= 0xfffff {
                            5
                        } else {
                            6
                        },
                        true,
                    )
                } else if cp <= 0xffff {
                    (b"\\u", 4, false)
                } else {
                    (b"\\U", 8, false)
                };
                escape[..prefix.len()].copy_from_slice(prefix);
                let hex = if upper {
                    b"0123456789ABCDEF"
                } else {
                    b"0123456789abcdef"
                };
                for j in 0..digits {
                    escape[prefix.len() + j] = hex[((cp >> (4 * (digits - 1 - j))) & 15) as usize];
                }
                let mut n = prefix.len() + digits;
                if upper {
                    escape[n] = b'>';
                    n += 1;
                }
                emit(&escape[..n])?;
                i += width;
                continue;
            }
        }
        let Some(sub) = sub else {
            return Ok(false);
        };
        // GNU resumes at the offending source code unit, repeating literal
        // substitutions per byte. A valid unrepresentable UTF8 scalar thus
        // produces two '?' substitutions for é.
        let n = from.unit().min(bytes.len() - i);
        for &b in &bytes[i..i + n] {
            if sub == b"byte" {
                emit(&[
                    b'<',
                    b"0123456789abcdef"[(b >> 4) as usize],
                    b"0123456789abcdef"[(b & 15) as usize],
                    b'>',
                ])?;
            } else {
                emit(sub)?;
            }
        }
        i += n;
    }
    Ok(true)
}
