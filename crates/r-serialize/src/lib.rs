use byteorder::{ByteOrder, NativeEndian, ReadBytesExt, WriteBytesExt};
use r_ffi::SEXP;
use std::io::{Read, Write};

mod constants;
mod writer;
mod reader;
mod header;
mod types;

pub use constants::*;
pub use header::SerializeHeader;
pub use reader::Deserializer;
pub use writer::Serializer;

#[derive(Debug, thiserror::Error)]
pub enum SerializeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid magic number")]
    InvalidMagic,

    #[error("Unsupported version {0}")]
    UnsupportedVersion(u32),

    #[error("Unknown type code: {0}")]
    UnknownTypeCode(u8),

    #[error("Object reference out of bounds")]
    ReferenceOutOfBounds,
}

pub type Result<T> = std::result::Result<T, SerializeError>;
