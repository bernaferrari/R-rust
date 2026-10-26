//! GNU lazy-database codec framing with admitted native storage.
#![forbid(unsafe_code)]

use super::{owned_input::InputBytes, workspace::reserve};
use crate::sexp::owner::OwnerToken;
use std::io::{self, Read, Write};

#[derive(Debug)]
pub(super) enum DecodeError {
    Resource(String),
    Invalid,
}
impl From<io::Error> for DecodeError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::OutOfMemory {
            Self::Resource(error.to_string())
        } else {
            Self::Invalid
        }
    }
}

fn output(owner: &OwnerToken<'_>) -> Result<InputBytes, DecodeError> {
    InputBytes::new(reserve(owner, 0).map_err(DecodeError::Resource)?, 0).map_err(Into::into)
}

fn framed(owner: &OwnerToken<'_>, input: &[u8], marker: Option<u8>) -> Result<InputBytes, String> {
    let length =
        u32::try_from(input.len()).map_err(|_| "raw vector too large to compress".to_string())?;
    let mut bytes = InputBytes::new(reserve(owner, 0)?, 0).map_err(|e| e.to_string())?;
    bytes
        .write_all(&length.to_be_bytes())
        .map_err(|e| e.to_string())?;
    if let Some(marker) = marker {
        bytes.push(marker).map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}

pub(super) fn encode(owner: &OwnerToken<'_>, input: &[u8], mode: u8) -> Result<InputBytes, String> {
    let lzma = super::owned_output::lzma_options(input.len());
    let workspace = match mode {
        1 => 512 * 1024,
        2 => 8 * 1024 * 1024,
        _ => super::owned_output::lzma_workspace(lzma.dict_size)?,
    };
    let workspace_guard = reserve(owner, workspace)?;
    let bytes = framed(
        owner,
        input,
        match mode {
            1 => None,
            2 => Some(b'2'),
            _ => Some(b'Z'),
        },
    )?;
    let encoded = match mode {
        1 => {
            let mut writer = flate2::write::ZlibEncoder::new(bytes, flate2::Compression::default());
            writer.write_all(input).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())?
        }
        2 => {
            let mut writer = bzip2::write::BzEncoder::new(bytes, bzip2::Compression::best());
            writer.write_all(input).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())?
        }
        _ => {
            let mut options = lzma_rust2::Lzma2Options::with_preset(6);
            options.lzma_options = lzma;
            let mut writer = lzma_rust2::Lzma2Writer::new(bytes, options);
            writer.write_all(input).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())?
        }
    };
    drop(workspace_guard);
    // GNU stores plain bytes when the bounded compressed body would expand.
    let body_length = encoded.len() - if mode == 1 { 4 } else { 5 };
    if (mode == 2 && body_length > input.len())
        || (mode == 3 && body_length > input.len().saturating_add(5))
    {
        let mut plain = framed(owner, input, Some(b'0'))?;
        plain.write_all(input).map_err(|e| e.to_string())?;
        return Ok(plain);
    }
    Ok(encoded)
}

fn read_exact_output(
    mut reader: impl Read,
    expected: usize,
    mut bytes: InputBytes,
) -> Result<InputBytes, DecodeError> {
    let mut chunk = [0; 8192];
    loop {
        let count = match reader.read(&mut chunk) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        if bytes.len().checked_add(count).is_none_or(|n| n > expected) {
            return Err(DecodeError::Invalid);
        }
        bytes.write_all(&chunk[..count])?;
    }
    if bytes.len() != expected {
        return Err(DecodeError::Invalid);
    }
    Ok(bytes)
}

fn read_lzma2_output(
    mut input: &[u8],
    dictionary: u32,
    expected: usize,
    mut bytes: InputBytes,
) -> Result<InputBytes, DecodeError> {
    // The admitted input already owns all compressed bytes. Decode directly
    // from it rather than copying each chunk to a second 64 KiB range buffer.
    let mut decoder = lzma_rust2::Lzma2Stream::new(dictionary);
    let mut chunk = [0u8; 8192];
    loop {
        let result = decoder.process(input, &mut chunk, lzma_rust2::Action::Finish)?;
        if bytes
            .len()
            .checked_add(result.bytes_produced)
            .is_none_or(|length| length > expected)
        {
            return Err(DecodeError::Invalid);
        }
        bytes.write_all(&chunk[..result.bytes_produced])?;
        input = &input[result.bytes_consumed..];
        if result.status == lzma_rust2::Status::StreamEnd {
            return if bytes.len() == expected {
                Ok(bytes)
            } else {
                Err(DecodeError::Invalid)
            };
        }
        if result.bytes_consumed == 0 && result.bytes_produced == 0 {
            return Err(DecodeError::Invalid);
        }
    }
}

pub(super) fn decode(
    owner: &OwnerToken<'_>,
    input: &[u8],
    mode: u8,
) -> Result<InputBytes, DecodeError> {
    let prefix = input.get(..4).ok_or(DecodeError::Invalid)?;
    let expected =
        u32::from_be_bytes(prefix.try_into().map_err(|_| DecodeError::Invalid)?) as usize;
    let (marker, data) = if mode == 1 {
        (b'1', &input[4..])
    } else {
        (*input.get(4).ok_or(DecodeError::Invalid)?, &input[5..])
    };
    if marker == b'0' {
        let data = data.get(..expected).ok_or(DecodeError::Invalid)?;
        let mut bytes = output(owner)?;
        bytes.write_all(data)?;
        return Ok(bytes);
    }
    // No valid match can refer farther back than the complete output. Keep
    // GNU's 8 MiB maximum, but do not allocate that dictionary for tiny records.
    let dictionary = expected
        .checked_next_power_of_two()
        .unwrap_or(usize::MAX)
        .clamp(4096, 8 * 1024 * 1024) as u32;
    let workspace = match marker {
        // &[u8] already implements BufRead: no 32 KiB Reader adapter copy.
        b'1' => 96 * 1024,
        b'2' => 4 * 1024 * 1024,
        // The stream reads our input directly, so subtract the Reader's
        // 64 KiB range copy. Add 40 KiB for simultaneous old/new probability
        // models on property reset, and 1 KiB for stream header/carry state.
        // Input/output buffers keep their independent admission charges.
        b'Z' if mode == 3 => (lzma_rust2::lzma2_get_memory_usage(dictionary) as usize)
            .checked_sub(64)
            .and_then(|kib| kib.checked_add(41))
            .and_then(|kib| kib.checked_mul(1024))
            .ok_or_else(|| DecodeError::Resource(super::workspace::memory_error()))?,
        _ => return Err(DecodeError::Invalid),
    };
    let _workspace = reserve(owner, workspace).map_err(DecodeError::Resource)?;
    let bytes = output(owner)?;
    match marker {
        b'1' => read_exact_output(flate2::bufread::ZlibDecoder::new(data), expected, bytes),
        b'2' => read_exact_output(bzip2::read::BzDecoder::new(data), expected, bytes),
        _ => read_lzma2_output(data, dictionary, expected, bytes),
    }
}
