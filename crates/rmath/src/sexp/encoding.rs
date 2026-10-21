#![forbid(unsafe_code)]
//! One text-encoding domain for owned characters and GNU native adapters.
//! C encoding numbers are distinct from the serialized CHARSXP GP flag bits.

pub(crate) const BYTES_MASK: u16 = 1 << 1;
pub(crate) const LATIN1_MASK: u16 = 1 << 2;
pub(crate) const UTF8_MASK: u16 = 1 << 3;
pub(crate) const ASCII_MASK: u16 = 1 << 6;
pub(crate) const ENCODING_MASK: u16 = BYTES_MASK | LATIN1_MASK | UTF8_MASK;

/// GNU Rinternals.h cetype_t values. Symbol/Any are device/text adapter
/// requests, never character encoding flags stored in a CHARSXP.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextEncoding {
    Native = 0,
    Utf8 = 1,
    Latin1 = 2,
    Bytes = 3,
    Symbol = 5,
    Any = 99,
}

impl TextEncoding {
    pub(crate) const fn as_gnu(self) -> i32 {
        self as i32
    }
    pub(crate) const fn from_gnu(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Native),
            1 => Some(Self::Utf8),
            2 => Some(Self::Latin1),
            3 => Some(Self::Bytes),
            5 => Some(Self::Symbol),
            99 => Some(Self::Any),
            _ => None,
        }
    }
    pub(crate) const fn from_gp(gp: u16) -> Self {
        // Preserve GNU getCharCE's precedence even for a legacy flag word
        // containing more than one declaration. Never rewrite serialized bits.
        if gp & UTF8_MASK != 0 {
            Self::Utf8
        } else if gp & LATIN1_MASK != 0 {
            Self::Latin1
        } else if gp & BYTES_MASK != 0 {
            Self::Bytes
        } else {
            Self::Native
        }
    }
    pub(crate) const fn gp_bits(self) -> Option<u16> {
        match self {
            Self::Native => Some(0),
            Self::Utf8 => Some(UTF8_MASK),
            Self::Latin1 => Some(LATIN1_MASK),
            Self::Bytes => Some(BYTES_MASK),
            Self::Symbol | Self::Any => None,
        }
    }
    pub(crate) fn from_label(label: &str) -> Self {
        match label {
            "UTF-8" => Self::Utf8,
            "latin1" => Self::Latin1,
            "bytes" => Self::Bytes,
            _ => Self::Native,
        }
    }
}

pub const CE_NATIVE: i32 = TextEncoding::Native.as_gnu();
pub const CE_UTF8: i32 = TextEncoding::Utf8.as_gnu();
pub const CE_LATIN1: i32 = TextEncoding::Latin1.as_gnu();
pub const CE_BYTES: i32 = TextEncoding::Bytes.as_gnu();
pub const CE_SYMBOL: i32 = TextEncoding::Symbol.as_gnu();
pub const CE_ANY: i32 = TextEncoding::Any.as_gnu();

pub(crate) mod declared_latin1;
pub(crate) mod nul_diagnostic;
mod owned;
pub(crate) use owned::{CharacterFlags, Mode, NulText, flags, translate};

/// Publish the requested character tag before any constructor notification.
pub(crate) fn allocate_character(
    arena: &mut super::memory::RArena,
    bytes: &[u8],
    encoding: TextEncoding,
) -> Option<super::ffi::SEXP> {
    let bits = encoding.gp_bits()?;
    let pointer = arena.alloc_charsxp(bytes);
    let node = arena.node_token(pointer)?;
    let heap = node.heap_identity();
    let mut header = heap.node_snapshot(&node)?;
    let bits = if bytes.is_ascii() { 0 } else { bits };
    header
        .sxpinfo
        .set_gp((header.sxpinfo.gp() & !ENCODING_MASK) | bits);
    heap.replace_node(&node, header)?;
    Some(pointer)
}
