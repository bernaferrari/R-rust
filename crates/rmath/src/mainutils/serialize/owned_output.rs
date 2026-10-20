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

pub(super) fn lzma_workspace(dict_size: u32) -> Result<usize, String> {
    let dict = dict_size as usize;
    if !(4096..=8 * 1024 * 1024).contains(&dict) || !dict.is_power_of_two() {
        return Err("unsupported LZMA dictionary size".into());
    }
    // lzma-rust2 0.21.0's public estimator adds get_buf_size bytes to KiB
    // counts. Audit the actual preset-6/Bt4 allocations instead: input window,
    // Hash234's three i32 arrays, and the two-i32-per-slot binary tree.
    // Two MiB additionally cover the 4096 Optimum cells, probability/price
    // models, match vectors, 64 KiB range buffer, and container bookkeeping.
    let before = (65536usize.saturating_sub(dict)).max(4096);
    let window = dict
        .checked_add(before)
        .and_then(|n| n.checked_add(4096 + 273))
        .and_then(|n| n.checked_add(dict / 2 + 256 * 1024));
    let hash = (dict / 2)
        .max(65536)
        .checked_add(1024 + 65536)
        .and_then(|n| n.checked_mul(std::mem::size_of::<i32>()));
    let tree = dict
        .checked_add(1)
        .and_then(|n| n.checked_mul(2 * std::mem::size_of::<i32>()));
    [window, hash, tree, Some(2 * 1024 * 1024)]
        .into_iter()
        .try_fold(0usize, |sum, bytes| sum.checked_add(bytes?))
        .ok_or_else(super::workspace::memory_error)
}

pub(crate) fn compress_container(
    owner: &OwnerToken<'_>,
    input: &[u8],
    codec: &str,
) -> Result<InputBytes, String> {
    // Pinned Cargo.lock implementations: zlib-rs uses one state/window/hash/
    // pending allocation; libbz2-rs uses two 900k u32 arrays plus ftab/state.
    // The envelopes include their writer buffers. XZ uses the checked bound
    // above rather than the dependency's mixed-unit memory estimate.
    let lzma = lzma_options(input.len());
    let workspace = match codec {
        "gzip" => 512 * 1024,
        "bzip2" => 8 * 1024 * 1024,
        "xz" => lzma_workspace(lzma.dict_size)?,
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
    fn serialization_workspace_lzma_bound_is_checked_on_extreme_input_sizes() {
        let mut previous = 0;
        for input in [0, 1, 4096, 65536, 1024 * 1024, 8 * 1024 * 1024, usize::MAX] {
            let options = lzma_options(input);
            let bytes = lzma_workspace(options.dict_size).unwrap();
            assert!(bytes >= previous);
            assert!(bytes >= options.dict_size as usize * 9);
            assert!(bytes < 128 * 1024 * 1024);
            previous = bytes;
        }
        assert!(lzma_workspace(0).is_err());
        assert!(lzma_workspace(u32::MAX).is_err());
        assert!(lzma_workspace(4096).unwrap() < 4 * 1024 * 1024);
    }

    #[test]
    #[cfg(not(miri))]
    fn serialization_workspace_small_xz_fits_four_mib_without_relaxing_cap() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let limit = owner
            .with_arena(|a| a.total_bytes_allocated() + 4 * 1024 * 1024)
            .unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
            .unwrap();
        let encoded = compress_container(&owner, b"small browser payload", "xz").unwrap();
        assert!(encoded.starts_with(&[0xfd, 0x37, 0x7a, 0x58, 0x5a, 0]));
        drop(encoded);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
    }

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
