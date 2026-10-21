#![forbid(unsafe_code)]
//! Bounded native bytes: count and render the same streaming escape plan.
use super::{Rprt_adj, utf8};
use crate::sexp::{
    encoding::{CE_BYTES, CE_LATIN1, CE_UTF8},
    memory::{AllocationCharge, MemoryLedger},
    object::{SexpError, SexpResult},
};

fn failure() -> SexpError {
    SexpError::AllocationFailed {
        object: "encoded string buffer",
    }
}
fn ascii(byte: u8, quote: u8, emit: &mut impl FnMut(&[u8]) -> bool) -> bool {
    let escaped = match byte {
        7 => Some(b'a'),
        8 => Some(b'b'),
        12 => Some(b'f'),
        b'\n' => Some(b'n'),
        b'\r' => Some(b'r'),
        b'\t' => Some(b't'),
        11 => Some(b'v'),
        0 => Some(b'0'),
        _ => None,
    };
    if let Some(letter) = escaped {
        return emit(&[b'\\', letter]);
    }
    if byte == b'\\' || (quote != 0 && byte == quote && matches!(byte, b'\'' | b'"' | b'`')) {
        return emit(&[b'\\', byte]);
    }
    if (0x20..0x7f).contains(&byte) {
        emit(&[byte])
    } else {
        octal(byte, emit)
    }
}
fn octal(byte: u8, emit: &mut impl FnMut(&[u8]) -> bool) -> bool {
    emit(&[
        b'\\',
        b'0' + (byte >> 6),
        b'0' + ((byte >> 3) & 7),
        b'0' + (byte & 7),
    ])
}
fn unicode(character: char, emit: &mut impl FnMut(&[u8]) -> bool) -> bool {
    let cp = character as u32;
    let digits = ((32 - cp.leading_zeros()).div_ceil(4) as usize).max(4);
    let mut unit = [0; 10];
    unit[..3].copy_from_slice(b"<U+");
    for i in 0..digits {
        unit[3 + i] = b"0123456789ABCDEF"[((cp >> ((digits - i - 1) * 4)) & 15) as usize];
    }
    unit[3 + digits] = b'>';
    emit(&unit[..4 + digits])
}
fn body(
    bytes: &[u8],
    encoding: i32,
    quote: u8,
    utf8_profile: bool,
    emit: &mut impl FnMut(&[u8]) -> bool,
) -> bool {
    let mut offset = 0;
    while let Some(&byte) = bytes.get(offset) {
        let remaining = &bytes[offset..];
        // Plain ASCII is already in its final wire representation. Forward
        // whole runs rather than decoding and copying one byte at a time.
        let plain = remaining
            .iter()
            .position(|&b| {
                !(0x20..0x7f).contains(&b)
                    || b == b'\\'
                    || (quote != 0 && b == quote && matches!(b, b'\'' | b'"' | b'`'))
            })
            .unwrap_or(remaining.len());
        if plain != 0 {
            if !emit(&remaining[..plain]) {
                return false;
            }
            offset += plain;
            continue;
        }
        let (keep, consumed) = if !utf8_profile && encoding != CE_UTF8 {
            let keep = if byte < 0x80 {
                ascii(byte, quote, emit)
            } else if encoding == CE_LATIN1 {
                emit(&[
                    b'<',
                    b"0123456789abcdef"[(byte >> 4) as usize],
                    b"0123456789abcdef"[(byte & 15) as usize],
                    b'>',
                ])
            } else {
                octal(byte, emit)
            };
            (keep, 1)
        } else {
            match utf8::units(remaining)
                .next()
                .expect("remaining string unit")
            {
                utf8::Unit::Ascii(byte) => (ascii(byte, quote, emit), 1),
                utf8::Unit::Scalar(character) if !utf8_profile => {
                    (unicode(character, emit), character.len_utf8())
                }
                utf8::Unit::Scalar(character) => {
                    let mut encoded = [0; 4];
                    (
                        emit(character.encode_utf8(&mut encoded).as_bytes()),
                        character.len_utf8(),
                    )
                }
                utf8::Unit::Invalid(byte) if !utf8_profile => (octal(byte, emit), 1),
                utf8::Unit::Invalid(byte) if encoding == CE_LATIN1 || encoding == CE_BYTES => {
                    (emit(&[byte]), 1)
                }
                utf8::Unit::Invalid(byte) => (
                    emit(&[
                        b'\\',
                        b'x',
                        b"0123456789abcdef"[(byte >> 4) as usize],
                        b"0123456789abcdef"[(byte & 15) as usize],
                    ]),
                    1,
                ),
            }
        };
        if !keep {
            return false;
        }
        offset += consumed;
    }
    true
}
pub(super) fn body_len(
    bytes: &[u8],
    encoding: i32,
    quote: u8,
    utf8_profile: bool,
) -> SexpResult<usize> {
    let mut count = 0usize;
    if body(bytes, encoding, quote, utf8_profile, &mut |part| {
        if let Some(next) = count.checked_add(part.len()) {
            count = next;
            true
        } else {
            false
        }
    }) {
        Ok(count)
    } else {
        Err(failure())
    }
}

pub(super) struct StringPlan<'a> {
    bytes: &'a [u8],
    encoding: i32,
    quote: Option<u8>,
    utf8_profile: bool,
    literal: bool,
    left: usize,
    right: usize,
    length: usize,
}
impl<'a> StringPlan<'a> {
    pub(super) fn text(
        bytes: &'a [u8],
        encoding: i32,
        utf8_profile: bool,
        quote: i32,
        width: i32,
        display_width: usize,
        justify: Rprt_adj,
    ) -> SexpResult<Self> {
        let quote = (quote != 0).then_some(quote as u8);
        let quotes = if quote.is_some() { 2 } else { 0 };
        let body = body_len(bytes, encoding, quote.unwrap_or(0), utf8_profile)?;
        let padding = if justify == Rprt_adj::none {
            0
        } else {
            (i128::from(width) - display_width as i128 - quotes).max(0) as usize
        };
        let left = match justify {
            Rprt_adj::right => padding,
            Rprt_adj::centre => padding / 2,
            _ => 0,
        };
        let right = if justify == Rprt_adj::right {
            0
        } else {
            padding - left
        };
        let length = body
            .checked_add(quotes as usize)
            .and_then(|n| n.checked_add(left))
            .and_then(|n| n.checked_add(right))
            .and_then(|n| n.checked_add(1))
            .ok_or_else(failure)?;
        Ok(Self {
            bytes,
            encoding,
            quote,
            utf8_profile,
            literal: false,
            left,
            right,
            length,
        })
    }
    pub(super) fn na(quoted: bool, width: i32, justify: Rprt_adj) -> SexpResult<Self> {
        let bytes: &[u8] = if quoted { b"NA" } else { b"<NA>" };
        let padding = if justify == Rprt_adj::none {
            0
        } else {
            (i64::from(width) - bytes.len() as i64).max(0) as usize
        };
        let left = match justify {
            Rprt_adj::right => padding,
            Rprt_adj::centre => padding / 2,
            _ => 0,
        };
        // Preserve the native NA encoder's current adjustment policy.
        let right = if justify == Rprt_adj::left {
            padding
        } else {
            0
        };
        let length = bytes
            .len()
            .checked_add(left)
            .and_then(|n| n.checked_add(right))
            .and_then(|n| n.checked_add(1))
            .ok_or_else(failure)?;
        Ok(Self {
            bytes,
            encoding: 0,
            quote: None,
            utf8_profile: true,
            literal: true,
            left,
            right,
            length,
        })
    }
}
#[derive(Default)]
pub(super) struct StringBuffer {
    data: Vec<u8>,
    // The actual Vec allocation disappears before its original ledger charge.
    charge: Option<AllocationCharge>,
}
impl StringBuffer {
    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.data.capacity()
    }
    fn prepare(&mut self, needed: usize, ledger: &MemoryLedger) -> SexpResult<()> {
        if needed <= self.data.capacity() {
            return Ok(());
        }
        let desired = self
            .data
            .capacity()
            .checked_mul(2)
            .unwrap_or(needed)
            .max(needed)
            .max(64);
        let (capacity, mut admission) = if let Some(admission) = ledger.reserve(desired) {
            (desired, admission)
        } else {
            (needed, ledger.reserve(needed).ok_or_else(failure)?)
        };
        // The old buffer remains funded and untouched through every fallible step.
        let mut replacement = Vec::new();
        replacement
            .try_reserve_exact(capacity)
            .map_err(|_| failure())?;
        admission
            .try_resize(replacement.capacity())
            .ok_or_else(failure)?;
        let charge = admission.commit_allocation().ok_or_else(failure)?;
        let old = std::mem::replace(&mut self.data, replacement);
        drop(old);
        self.charge = Some(charge);
        Ok(())
    }
    pub(super) fn render(
        &mut self,
        plan: &StringPlan<'_>,
        ledger: &MemoryLedger,
    ) -> SexpResult<*const u8> {
        self.prepare(plan.length, ledger)?;
        self.data.resize(plan.length, 0);
        let mut position = 0usize;
        let mut write = |bytes: &[u8]| {
            let Some(end) = position.checked_add(bytes.len()) else {
                return false;
            };
            let Some(target) = self.data.get_mut(position..end) else {
                return false;
            };
            target.copy_from_slice(bytes);
            position = end;
            true
        };
        // Padding uses a fixed stack chunk, including for very wide fields.
        fn pad(mut count: usize, write: &mut impl FnMut(&[u8]) -> bool) -> bool {
            let spaces = [b' '; 64];
            while count != 0 {
                let n = count.min(spaces.len());
                if !write(&spaces[..n]) {
                    return false;
                }
                count -= n;
            }
            true
        }
        if !pad(plan.left, &mut write) {
            return Err(failure());
        }
        if let Some(quote) = plan.quote
            && !write(&[quote])
        {
            return Err(failure());
        }
        let complete = if plan.literal {
            write(plan.bytes)
        } else {
            body(
                plan.bytes,
                plan.encoding,
                plan.quote.unwrap_or(0),
                plan.utf8_profile,
                &mut write,
            )
        };
        if !complete
            || plan.quote.is_some_and(|quote| !write(&[quote]))
            || !pad(plan.right, &mut write)
            || !write(&[0])
        {
            return Err(failure());
        }
        if position != plan.length {
            return Err(failure());
        }
        Ok(self.data.as_ptr())
    }
}
