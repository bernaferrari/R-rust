//! Serialization constants from R's serialize.c

pub const MAGIC: u32 = 0x42585932; // "BXY2"
pub const VERSION: u32 = 3;
pub const MIN_VERSION: u32 = 2;

// Serialization type codes
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeCode {
    Nil = 0,
    Symbol = 1,
    List = 2,
    Closure = 3,
    Env = 4,
    Promise = 5,
    Lang = 6,
    Special = 7,
    Builtin = 8,
    Char = 9,
    Logical = 10,
    Integer = 13,
    Real = 14,
    Complex = 15,
    String = 16,
    DotDotDot = 17,
    Any = 18,
    Vector = 19,
    Expr = 20,
    Bytecode = 21,
    Pointer = 22,
    WeakRef = 23,
    Raw = 24,
    S4 = 25,
    Ref = 255,
}

impl TryFrom<u8> for TypeCode {
    type Error = u8;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Nil),
            1 => Ok(Self::Symbol),
            2 => Ok(Self::List),
            3 => Ok(Self::Closure),
            4 => Ok(Self::Env),
            5 => Ok(Self::Promise),
            6 => Ok(Self::Lang),
            7 => Ok(Self::Special),
            8 => Ok(Self::Builtin),
            9 => Ok(Self::Char),
            10 => Ok(Self::Logical),
            13 => Ok(Self::Integer),
            14 => Ok(Self::Real),
            15 => Ok(Self::Complex),
            16 => Ok(Self::String),
            17 => Ok(Self::DotDotDot),
            18 => Ok(Self::Any),
            19 => Ok(Self::Vector),
            20 => Ok(Self::Expr),
            21 => Ok(Self::Bytecode),
            22 => Ok(Self::Pointer),
            23 => Ok(Self::WeakRef),
            24 => Ok(Self::Raw),
            25 => Ok(Self::S4),
            255 => Ok(Self::Ref),
            _ => Err(value),
        }
    }
}

// Header flags
pub const FLAG_ASCII: u32 = 1 << 0;
pub const FLAG_SWAP: u32 = 1 << 1;
pub const FLAG_UNUSED_BITS: u32 = 0b11111100;
