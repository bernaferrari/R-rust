//! Bounded byte and wide input shared by the TRE matchers.

use std::{
    ffi::CStr,
    os::raw::{c_int, c_void},
};

use super::ast::{REG_ESPACE, REG_NOMATCH, tre_char_t, tre_cint_t, tre_str_type_t};

#[derive(Clone, Copy)]
pub(super) enum MatchInput<'a> {
    Byte(&'a [u8]),
    Wide(&'a [tre_char_t]),
}

impl<'a> MatchInput<'a> {
    /// # Safety
    /// The input is readable for `len` code units or through its first NUL
    /// when `len` is negative. Null is accepted for a bounded empty input.
    pub(super) unsafe fn from_raw(
        string: *const c_void,
        len: c_int,
        kind: tre_str_type_t,
    ) -> Result<Self, c_int> {
        match kind {
            tre_str_type_t::STR_BYTE => {
                let bytes = if len == 0 {
                    &[]
                } else if len < 0 {
                    // SAFETY: the legacy byte input is NUL-terminated.
                    unsafe { CStr::from_ptr(string.cast()) }.to_bytes()
                } else {
                    // SAFETY: the caller supplies this bounded byte input.
                    unsafe { std::slice::from_raw_parts(string.cast(), len as usize) }
                };
                c_int::try_from(bytes.len()).map_err(|_| REG_ESPACE)?;
                Ok(Self::Byte(bytes))
            }
            tre_str_type_t::STR_WIDE => {
                let wide = string.cast::<tre_char_t>();
                let max = (isize::MAX as usize / std::mem::size_of::<tre_char_t>())
                    .min(c_int::MAX as usize);
                let count = if len < 0 {
                    let mut count = 0;
                    loop {
                        if count >= max {
                            return Err(REG_ESPACE);
                        }
                        // SAFETY: the caller supplies a terminator and we stop at it.
                        if unsafe { *wide.add(count) } == 0 {
                            break count;
                        }
                        count += 1;
                    }
                } else {
                    len as usize
                };
                if count > max {
                    return Err(REG_ESPACE);
                }
                let units = if count == 0 {
                    &[]
                } else {
                    // SAFETY: the bounded extent was supplied or scanned above.
                    unsafe { std::slice::from_raw_parts(wide, count) }
                };
                Ok(Self::Wide(units))
            }
            _ => Err(REG_NOMATCH),
        }
    }

    pub(super) fn len(self) -> usize {
        match self {
            Self::Byte(input) => input.len(),
            Self::Wide(input) => input.len(),
        }
    }

    pub(super) fn first_byte(self, needle: u8) -> Option<usize> {
        match self {
            Self::Byte(input) => input.iter().position(|&byte| byte == needle),
            Self::Wide(_) => None,
        }
    }

    pub(super) fn at(self, pos: c_int) -> tre_cint_t {
        let Ok(index) = usize::try_from(pos) else {
            return 0;
        };
        match self {
            Self::Byte(input) => input.get(index).copied().map_or(0, tre_cint_t::from),
            Self::Wide(input) => input.get(index).copied().unwrap_or(0),
        }
    }

    pub(super) fn backref_end(self, so: c_int, eo: c_int, pos: c_int) -> Option<c_int> {
        // TRE treats a capture that did not participate as empty.
        if (so, eo) == (-1, -1) {
            return Some(pos);
        }
        let (start, end, pos) = (
            usize::try_from(so).ok()?,
            usize::try_from(eo).ok()?,
            usize::try_from(pos).ok()?,
        );
        let target_end = pos.checked_add(end.checked_sub(start)?)?;
        let equal = match self {
            Self::Byte(input) => input.get(start..end)? == input.get(pos..target_end)?,
            Self::Wide(input) => input.get(start..end)? == input.get(pos..target_end)?,
        };
        equal.then_some(target_end as c_int)
    }
}
