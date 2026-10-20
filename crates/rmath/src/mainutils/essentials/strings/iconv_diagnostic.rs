#![forbid(unsafe_code)]
//! GNU's bounded embedded-NUL error text. Size and admit before allocating.
use crate::sexp::{
    encoding::TextEncoding,
    memory::TransientReservation,
    object::{Sexp, SexpError, SexpResult},
};
const LIMIT: usize = 999;
const PREFIX: &str = "embedded nul in string: '";
pub(super) struct Diagnostic {
    pub(super) message: String,
    _ticket: TransientReservation,
}
impl Diagnostic {
    /// Error text leaves the operation as a bounded host-facing diagnostic.
    /// Its formatting allocation has completed; no further copy is needed.
    pub(super) fn into_message(self) -> String {
        self.message
    }
}
fn allocation() -> SexpError {
    SexpError::AllocationFailed {
        object: "iconv diagnostic workspace",
    }
}
fn hex(out: &mut [u8], point: u32, upper: bool) {
    let digits = out.len();
    let alphabet = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    for (i, b) in out.iter_mut().enumerate() {
        *b = alphabet[((point >> (4 * (digits - 1 - i))) & 15) as usize];
    }
}
fn visit(bytes: &[u8], mut encoding: TextEncoding, utf8: bool, mut emit: impl FnMut(&str) -> bool) {
    if !emit(PREFIX) {
        return;
    }
    // The temporary GNU character loses any encoding declaration for ASCII.
    if bytes.is_ascii() {
        encoding = TextEncoding::Native;
    }
    let translate = encoding == TextEncoding::Latin1 || (!utf8 && encoding == TextEncoding::Utf8);
    let bytes = if translate {
        &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
    } else {
        bytes
    };
    let mut offset = 0;
    while offset < bytes.len() {
        let b = bytes[offset];
        let mut raw = [0u8; 12];
        let (n, used) = if b < 128 {
            let escape = match b {
                0 => Some(b'0'),
                7 => Some(b'a'),
                8 => Some(b'b'),
                9 => Some(b't'),
                10 => Some(b'n'),
                11 => Some(b'v'),
                12 => Some(b'f'),
                13 => Some(b'r'),
                b'\\' => Some(b'\\'),
                _ => None,
            };
            if let Some(e) = escape {
                raw[..2].copy_from_slice(&[b'\\', e]);
                (2, 1)
            } else if (32..127).contains(&b) {
                raw[0] = b;
                (1, 1)
            } else {
                raw[..4].copy_from_slice(&[
                    b'\\',
                    b'0' + (b >> 6),
                    b'0' + ((b >> 3) & 7),
                    b'0' + (b & 7),
                ]);
                (4, 1)
            }
        } else if encoding == TextEncoding::Latin1 {
            if utf8 {
                if char::from(b).is_control() {
                    raw[..2].copy_from_slice(b"\\u");
                    hex(&mut raw[2..6], u32::from(b), false);
                    (6, 1)
                } else {
                    (char::from(b).encode_utf8(&mut raw).len(), 1)
                }
            } else {
                raw[0] = b'<';
                hex(&mut raw[1..3], u32::from(b), false);
                raw[3] = b'>';
                (4, 1)
            }
        } else if utf8 || encoding == TextEncoding::Utf8 {
            let width = match b {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => 0,
            };
            let ch = bytes
                .get(offset..offset + width)
                .filter(|_| width != 0)
                .and_then(|s| std::str::from_utf8(s).ok())
                .and_then(|s| s.chars().next());
            if let Some(ch) = ch {
                if !utf8 {
                    let point = ch as u32;
                    let digits = if point <= 0xffff {
                        4
                    } else if point <= 0xfffff {
                        5
                    } else {
                        6
                    };
                    raw[..3].copy_from_slice(b"<U+");
                    hex(&mut raw[3..3 + digits], point, true);
                    raw[3 + digits] = b'>';
                    (4 + digits, width)
                } else if ch.is_control() {
                    raw[..2].copy_from_slice(b"\\u");
                    hex(&mut raw[2..6], ch as u32, false);
                    (6, width)
                } else {
                    (ch.encode_utf8(&mut raw).len(), width)
                }
            } else if utf8 {
                raw[..2].copy_from_slice(b"\\x");
                hex(&mut raw[2..4], u32::from(b), false);
                (4, 1)
            } else {
                raw[..4].copy_from_slice(&[
                    b'\\',
                    b'0' + (b >> 6),
                    b'0' + ((b >> 3) & 7),
                    b'0' + (b & 7),
                ]);
                (4, 1)
            }
        } else {
            raw[..4].copy_from_slice(&[
                b'\\',
                b'0' + (b >> 6),
                b'0' + ((b >> 3) & 7),
                b'0' + (b & 7),
            ]);
            (4, 1)
        };
        // Every token is validated UTF8 or an ASCII escape, including when the
        // original wire bytes are malformed or contain embedded NUL.
        if !emit(std::str::from_utf8(&raw[..n]).expect("diagnostic token")) {
            return;
        }
        offset += used;
    }
    emit("'");
}
fn capped_visit(
    bytes: &[u8],
    encoding: TextEncoding,
    utf8: bool,
    mut emit: impl FnMut(&str),
) -> usize {
    let mut size = 0;
    visit(bytes, encoding, utf8, |part| {
        let mut n = part.len().min(LIMIT - size);
        while !part.is_char_boundary(n) {
            n -= 1;
        }
        emit(&part[..n]);
        size += n;
        n == part.len() && size < LIMIT
    });
    size
}
pub(super) fn format(
    parent: &Sexp<'_>,
    bytes: &[u8],
    encoding: TextEncoding,
    utf8: bool,
) -> SexpResult<Diagnostic> {
    let size = capped_visit(bytes, encoding, utf8, |_| {});
    let node = parent.allocation()?;
    let ticket = node
        .heap_identity()
        .reserve_payload_bytes(node, size)
        .ok_or_else(allocation)?;
    let mut message = String::new();
    message.try_reserve_exact(size).map_err(|_| allocation())?;
    let copied = capped_visit(bytes, encoding, utf8, |part| message.push_str(part));
    debug_assert_eq!(copied, size);
    debug_assert_eq!(message.len(), size);
    Ok(Diagnostic {
        message,
        _ticket: ticket,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};
    #[test]
    fn iconv_nul_workspace_admits_exact_bound_and_recovers_under_unchanged_limit() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let parent = owner.node_factory().strings(&["parent"]).unwrap();
            let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
            owner
                .with_arena(|a| a.set_budget(ArenaBudget::new(before + 998, 0)))
                .unwrap();
            assert!(format(&parent, &[0; 10000], TextEncoding::Utf8, true).is_err());
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            let small = format(&parent, b"A\0B", TextEncoding::Utf8, true).unwrap();
            assert_eq!(small.message, "embedded nul in string: 'A\\0B'");
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 30);
            drop(small);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            assert_eq!(
                owner.with_arena(|a| a.budget().max_bytes).unwrap(),
                before + 998
            );
            assert_eq!(
                owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
                before
            );
            owner
                .with_arena(|a| a.set_budget(ArenaBudget::new(before + 999, 0)))
                .unwrap();
            let exact = format(&parent, &[0; 10000], TextEncoding::Utf8, true).unwrap();
            assert_eq!(exact.message.len(), 999);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 999);
            drop(exact);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        });
    }
    #[test]
    fn iconv_nul_workspace_remains_in_original_heap_and_releases_on_unwind() {
        let original = RSession::new_for_gc_tests();
        let foreign = RSession::new_for_gc_tests();
        let owner = original.owner_token().unwrap();
        let foreign_owner = foreign.owner_token().unwrap();
        let parent = original.with_active(|| {
            owner
                .node_factory()
                .strings(&["parent"])
                .unwrap()
                .into_owned()
                .unwrap()
        });
        foreign.with_active(|| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let diagnostic = format(&parent, b"\0", TextEncoding::Native, true).unwrap();
                assert_eq!(diagnostic.message.len(), 28);
                assert_eq!(
                    original.with_active(|| owner.with_arena(|a| a.workspace_bytes()).unwrap()),
                    28
                );
                assert_eq!(
                    foreign_owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                    0
                );
                std::panic::panic_any(713u32);
            }));
            assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 713);
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
    #[test]
    fn iconv_nul_diagnostic_keeps_utf8_boundary_inside_the_byte_limit() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let parent = session
                .owner_token()
                .unwrap()
                .node_factory()
                .strings(&["parent"])
                .unwrap();
            let wire = format!("A{}\0", "é".repeat(1000));
            let diagnostic = format(&parent, wire.as_bytes(), TextEncoding::Utf8, true).unwrap();
            assert_eq!(diagnostic.message.len(), 998);
            assert!(diagnostic.message.ends_with('é'));
            assert!(!diagnostic.message.contains('\u{fffd}'));
        });
    }
    #[test]
    fn iconv_nul_diagnostic_translation_and_native_c_controls_match_gnu() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let parent = session
                .owner_token()
                .unwrap()
                .node_factory()
                .strings(&["parent"])
                .unwrap();
            for (wire, encoding, utf8, expected) in [
                (
                    &b"\xc3\xa9\0A"[..],
                    TextEncoding::Utf8,
                    true,
                    "embedded nul in string: 'é\\0A'",
                ),
                (
                    &b"\xe9\0A"[..],
                    TextEncoding::Latin1,
                    true,
                    "embedded nul in string: 'é'",
                ),
                (
                    &b"\xe9\0A"[..],
                    TextEncoding::Native,
                    true,
                    "embedded nul in string: '\\xe9\\0A'",
                ),
                (
                    &b"\xc3\xa9\0A"[..],
                    TextEncoding::Utf8,
                    false,
                    "embedded nul in string: '<U+00E9>'",
                ),
                (
                    &b"\xc3\xa9\0A"[..],
                    TextEncoding::Native,
                    false,
                    "embedded nul in string: '\\303\\251\\0A'",
                ),
                (
                    &b"\xe9\0A"[..],
                    TextEncoding::Latin1,
                    false,
                    "embedded nul in string: '<e9>'",
                ),
                (
                    &b"\xe9\0\0\0A\0"[..],
                    TextEncoding::Native,
                    true,
                    "embedded nul in string: '\\xe9\\0\\0\\0A\\0'",
                ),
            ] {
                assert_eq!(
                    format(&parent, wire, encoding, utf8).unwrap().message,
                    expected
                );
            }
        });
    }
}
