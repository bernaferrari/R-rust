#![forbid(unsafe_code)]
//! Visit validated scalar units and individual invalid wire bytes, without
//! allocation or constructing an invalid Rust char/str.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Unit {
    Ascii(u8),
    Scalar(char),
    Invalid(u8),
}
pub(super) fn units(bytes: &[u8]) -> impl Iterator<Item = Unit> + '_ {
    let mut offset = 0;
    std::iter::from_fn(move || {
        let b = *bytes.get(offset)?;
        let width = match b {
            0..=127 => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 0,
        };
        let ch = bytes
            .get(offset..offset.checked_add(width)?)
            .filter(|_| width != 0)
            .and_then(|part| std::str::from_utf8(part).ok())
            .and_then(|s| s.chars().next());
        match ch {
            Some(ch) => {
                offset += width;
                Some(if ch.is_ascii() {
                    Unit::Ascii(b)
                } else {
                    Unit::Scalar(ch)
                })
            }
            None => {
                offset += 1;
                Some(Unit::Invalid(b))
            }
        }
    })
}
/// GNU Rstrwid uses its historical integer decoder for marked UTF8. This
/// differs from the display encoder on malformed/extended sequences. Retain
/// that width contract with checked bounds, without creating invalid chars.
pub(super) fn width_units(bytes: &[u8]) -> impl Iterator<Item = (Option<u32>, u8)> + '_ {
    let mut offset = 0;
    std::iter::from_fn(move || {
        let b = *bytes.get(offset)?;
        let width = match b {
            0..=127 => 1,
            128..=191 => 0,
            192..=223 => 2,
            224..=239 => 3,
            240..=247 => 4,
            248..=251 => 5,
            _ => 6,
        };
        let decoded = (|| {
            let p = bytes.get(offset..offset.checked_add(width)?)?;
            if width == 0 {
                return None;
            }
            if width == 1 {
                return Some(u32::from(b));
            }
            if width <= 4 && !p[1..].iter().all(|&b| b & 0xc0 == 0x80) {
                return None;
            }
            let cp = match width {
                2 => (u32::from(b & 31) << 6) | u32::from(p[1] & 63),
                3 => (u32::from(b & 15) << 12) | (u32::from(p[1] & 63) << 6) | u32::from(p[2] & 63),
                4 => {
                    (u32::from(b & 15) << 18)
                        | (u32::from(p[1] & 63) << 12)
                        | (u32::from(p[2] & 63) << 6)
                        | u32::from(p[3] & 63)
                }
                5 => {
                    (u32::from(b & 15) << 24)
                        | (u32::from(p[1] & 63) << 12)
                        | (u32::from(p[2] & 63) << 12)
                        | (u32::from(p[3] & 63) << 6)
                        | u32::from(p[4] & 63)
                }
                _ => {
                    (u32::from(b & 15) << 30)
                        | (u32::from(p[1] & 63) << 24)
                        | (u32::from(p[2] & 63) << 18)
                        | (u32::from(p[3] & 63) << 12)
                        | (u32::from(p[4] & 63) << 6)
                        | u32::from(p[5] & 63)
                }
            };
            if width == 3 && ((0xd800..=0xdfff).contains(&cp) || matches!(cp, 0xfffe | 0xffff)) {
                None
            } else {
                Some(cp)
            }
        })();
        offset += if decoded.is_some_and(|cp| cp >= 128) {
            width
        } else {
            1
        };
        Some((decoded, b))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn printed_utf8_legacy_width_decoder_bounds_every_truncation() {
        let wire = b"A\xc3\xa9\xff\xf4\x90\x80\x80\xe9";
        let units = width_units(wire).collect::<Vec<_>>();
        assert_eq!(units.len(), 3);
        assert_eq!(units[0], (Some(65), 65));
        assert_eq!(units[1], (Some(233), 195));
        assert!(units[2].0.unwrap() > 0x10ffff);
        for n in 0..=wire.len() {
            assert!(width_units(&wire[..n]).count() <= n);
            assert!(units_for_prefix(&wire[..n]) <= n);
        }
        fn units_for_prefix(bytes: &[u8]) -> usize {
            super::units(bytes).count()
        }
    }
    #[test]
    fn printed_utf8_units_preserve_neighbors_and_invalid_wire_bytes() {
        use Unit::*;
        assert_eq!(
            units(b"A\xc3\xa9\xff\xf4\x90\x80\x80\xf0\x9f\x99\x82\xe9").collect::<Vec<_>>(),
            [
                Ascii(b'A'),
                Scalar('é'),
                Invalid(255),
                Invalid(244),
                Invalid(144),
                Invalid(128),
                Invalid(128),
                Scalar('🙂'),
                Invalid(233)
            ]
        );
        assert_eq!(
            units(b"\xed\xa0\x80\xc0\x80\xe2\x82").collect::<Vec<_>>(),
            [
                Invalid(237),
                Invalid(160),
                Invalid(128),
                Invalid(192),
                Invalid(128),
                Invalid(226),
                Invalid(130)
            ]
        );
        assert_eq!(
            units(b"\t\"\\").collect::<Vec<_>>(),
            [Ascii(b'\t'), Ascii(b'"'), Ascii(b'\\')]
        );
        assert!(units(b"").next().is_none());
    }
}
