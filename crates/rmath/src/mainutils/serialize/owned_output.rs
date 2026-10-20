//! Container compression admits codec workspace and keeps output charged.
#![forbid(unsafe_code)]

use super::{owned_input::InputBytes, workspace::reserve};
use crate::sexp::owner::OwnerToken;
use std::io::Write;

pub(super) fn lzma_options(input_length: usize) -> lzma_rust2::LzmaOptions {
    let mut options = lzma_rust2::LzmaOptions::with_preset(6);
    // A dictionary larger than all available history adds workspace without
    // adding possible matches. Preserve preset-6 coding and GNU's maximum.
    options.dict_size = input_length
        .checked_next_power_of_two()
        .unwrap_or(usize::MAX)
        .clamp(4096, options.dict_size as usize) as u32;
    options
}

pub(crate) fn compress_container(
    owner: &OwnerToken<'_>,
    input: &[u8],
    codec: &str,
) -> Result<InputBytes, String> {
    // Pinned Cargo.lock implementations: zlib-rs uses one state/window/hash/
    // pending allocation; libbz2-rs uses two 900k u32 arrays plus ftab/state.
    // The envelopes include their writer buffers. XZ exposes its preset's
    // encoder estimate; an additional MiB covers container/filter bookkeeping.
    let lzma = lzma_options(input.len());
    let workspace = match codec {
        "gzip" => 512 * 1024,
        "bzip2" => 8 * 1024 * 1024,
        "xz" => (lzma.get_memory_usage() as usize)
            .checked_mul(1024)
            .and_then(|n| n.checked_add(1024 * 1024))
            .ok_or_else(super::workspace::memory_error)?,
        _ => return Err("unsupported compression format".into()),
    };
    let _workspace = reserve(owner, workspace)?;
    let output = InputBytes::new(reserve(owner, 0)?, 0).map_err(|e| e.to_string())?;
    match codec {
        "gzip" => {
            let mut writer = flate2::GzBuilder::new()
                .mtime(0)
                .operating_system(3)
                .write(output, flate2::Compression::new(6));
            writer.write_all(input).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())
        }
        "bzip2" => {
            let mut writer = bzip2::write::BzEncoder::new(output, bzip2::Compression::best());
            writer.write_all(input).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())
        }
        _ => {
            let mut options = lzma_rust2::XzOptions::with_preset(6);
            options.lzma_options = lzma;
            options.set_check_sum_type(lzma_rust2::CheckType::Crc32);
            let mut writer =
                lzma_rust2::XzWriter::new(output, options).map_err(|e| e.to_string())?;
            writer.write_all(input).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};

    #[test]
    fn serialization_workspace_codec_scratch_denies_and_releases_under_original_cap() {
        for codec in ["gzip", "bzip2", "xz"] {
            let session = RSession::new_for_gc_tests();
            let owner = session.owner_token().unwrap();
            let limit = owner
                .with_arena(|a| a.total_bytes_allocated() + 128 * 1024)
                .unwrap();
            owner
                .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
                .unwrap();
            assert!(
                compress_container(&owner, b"payload", codec)
                    .unwrap_err()
                    .contains("session memory limit")
            );
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn serialization_workspace_codec_output_roundtrips_and_retains_its_charge() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        for codec in ["gzip", "bzip2", "xz"] {
            let encoded = compress_container(&owner, b"R stream payload", codec).unwrap();
            assert!(owner.with_arena(|a| a.workspace_bytes()).unwrap() >= encoded.len());
            let decoded = super::super::owned_input::expand(&encoded, |n| {
                reserve(&owner, n).map_err(std::io::Error::other)
            })
            .unwrap()
            .unwrap();
            assert_eq!(&*decoded, b"R stream payload");
            drop((encoded, decoded));
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        }
    }
}
