#![forbid(unsafe_code)]
//! Native decode bytes retain original-session admission across R hooks.

use crate::sexp::memory::WorkspaceReservation;
use std::io::{self, Read, Write};

pub(super) fn memory_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::OutOfMemory,
        "unserialize exceeds session memory limit",
    )
}

pub(super) struct InputBytes {
    bytes: Vec<u8>,
    reservation: WorkspaceReservation,
}

impl std::ops::Deref for InputBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

impl InputBytes {
    pub(super) fn new(mut reservation: WorkspaceReservation, capacity: usize) -> io::Result<Self> {
        reservation.try_resize(capacity).ok_or_else(memory_error)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| memory_error())?;
        Ok(Self { bytes, reservation })
    }

    fn read_from(&mut self, mut reader: impl Read) -> io::Result<()> {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => return Ok(()),
                Ok(count) => self.write_all(&chunk[..count])?,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
    }
}

impl Write for InputBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let length = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or_else(memory_error)?;
        if length > self.bytes.capacity() {
            let old_capacity = self.bytes.capacity();
            let capacity = length.checked_next_power_of_two().unwrap_or(length);
            // Retain the old buffer's charge until its replacement is copied
            // and freed. No allocation or zero-fill precedes peak admission.
            let peak = old_capacity
                .checked_add(capacity)
                .ok_or_else(memory_error)?;
            self.reservation.try_resize(peak).ok_or_else(memory_error)?;
            let mut replacement = Vec::new();
            if replacement.try_reserve_exact(capacity).is_err() {
                self.reservation
                    .try_resize(old_capacity)
                    .expect("shrinking admitted input");
                return Err(memory_error());
            }
            replacement.extend_from_slice(&self.bytes);
            drop(std::mem::replace(&mut self.bytes, replacement));
            self.reservation
                .try_resize(capacity)
                .expect("shrinking admitted input");
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn expand(
    bytes: &[u8],
    mut reserve: impl FnMut(usize) -> io::Result<WorkspaceReservation>,
) -> io::Result<Option<InputBytes>> {
    enum Codec {
        Gzip,
        Bzip2,
        Xz,
    }
    let codec = if bytes.starts_with(&[0x1f, 0x8b]) {
        Codec::Gzip
    } else if bytes.starts_with(b"BZh") {
        Codec::Bzip2
    } else if bytes.starts_with(&[0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]) {
        Codec::Xz
    } else {
        return Ok(None);
    };
    let workspace = match codec {
        Codec::Gzip => 128 * 1024,
        Codec::Bzip2 => 4 * 1024 * 1024,
        Codec::Xz => 16 * 1024 * 1024,
    };
    let _decoder_workspace = reserve(workspace)?;
    let mut output = InputBytes::new(reserve(0)?, 0)?;
    match codec {
        Codec::Gzip => output.read_from(flate2::read::GzDecoder::new(bytes))?,
        Codec::Bzip2 => output.read_from(bzip2::read::BzDecoder::new(bytes))?,
        Codec::Xz => output.read_from(lzma_rust2::XzReader::new_mem_limit(
            bytes,
            true,
            (workspace / 1024) as u32,
        ))?,
    }
    if output.bytes.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "read error"));
    }
    Ok(Some(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{memory::ArenaBudget, owner::OwnerToken, session::RSession};

    fn reserve(owner: &OwnerToken<'_>, bytes: usize) -> io::Result<WorkspaceReservation> {
        owner
            .with_arena(|a| a.reserve_workspace(bytes))
            .map_err(|e| io::Error::other(e.to_string()))?
            .ok_or_else(memory_error)
    }

    #[test]
    fn replacement_peak_is_admitted_before_copy_and_denial_preserves_original_bytes() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(a.total_bytes_allocated() + 10240, 0)))
            .unwrap();
        let mut bytes = InputBytes::new(reserve(&owner, 0).unwrap(), 4096).unwrap();
        bytes.write_all(&[b'x'; 4096]).unwrap();
        assert!(bytes.write_all(b"y").is_err());
        assert_eq!(&bytes[..], &[b'x'; 4096]);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 4096);
        drop(bytes);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

    #[test]
    fn all_compressed_expansions_share_admission_and_release_on_denial_or_success() {
        let cases: [(&[u8], usize); 3] = [
            (
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../r-embed/tests/fixtures/native-codec-admission/gzip.rds"
                )),
                128 * 1024,
            ),
            (
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../r-embed/tests/fixtures/native-codec-admission/bzip2.rds"
                )),
                4 * 1024 * 1024,
            ),
            (
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../r-embed/tests/fixtures/native-codec-admission/xz.rds"
                )),
                16 * 1024 * 1024,
            ),
        ];
        for (source, workspace) in cases {
            let session = RSession::new_for_gc_tests();
            let owner = session.owner_token().unwrap();
            owner
                .with_arena(|a| {
                    a.set_budget(ArenaBudget::new(
                        a.total_bytes_allocated() + workspace + 256 * 1024,
                        0,
                    ))
                })
                .unwrap();
            let failure = expand(source, |n| reserve(&owner, n));
            assert!(failure.is_err());
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            owner
                .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
                .unwrap();
            let decoded = expand(source, |n| reserve(&owner, n)).unwrap().unwrap();
            assert_eq!(decoded.len(), 22 + 2 * 1024 * 1024);
            assert!(decoded[22..].iter().all(|b| *b == 0));
            assert_eq!(
                owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                decoded.bytes.capacity()
            );
            drop(decoded);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        }
    }

    #[test]
    fn oversized_xz_dictionary_is_rejected_under_the_decoder_limit() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let source = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../r-embed/tests/fixtures/native-codec-admission/oversized-dictionary.xz"
        ));
        assert!(expand(source, |n| reserve(&owner, n)).is_err());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
}
