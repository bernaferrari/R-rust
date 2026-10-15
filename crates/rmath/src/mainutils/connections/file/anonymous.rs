//! Session-owned anonymous update files. These operations never call R.
#![forbid(unsafe_code)]

use crate::sexp::{heap::CheckedNode, memory::TransientReservation};
use std::io::Cursor;

/// The current file offset and the two GNU read/write bookmarks are distinct.
/// In particular, querying `seek(..., NA, rw=...)` changes the active mode
/// without repositioning the underlying stream.
pub struct AnonymousFile {
    data: Cursor<Vec<u8>>,
    read_position: u64,
    write_position: u64,
    last_was_write: bool,
    anchor: CheckedNode,
    reservation: Option<TransientReservation>,
}

impl std::fmt::Debug for AnonymousFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AnonymousFile")
            .field("bytes", &self.data.get_ref().len())
            .field("position", &self.data.position())
            .finish_non_exhaustive()
    }
}

impl AnonymousFile {
    pub(crate) fn new(anchor: CheckedNode) -> Self {
        Self {
            data: Cursor::new(Vec::new()),
            read_position: 0,
            write_position: 0,
            last_was_write: false,
            anchor,
            reservation: None,
        }
    }
    fn reading(&mut self) {
        if self.last_was_write {
            self.write_position = self.data.position();
            self.data.set_position(self.read_position);
            self.last_was_write = false;
        }
    }

    pub(crate) fn read_byte(&mut self) -> Option<u8> {
        self.reading();
        let position = usize::try_from(self.data.position()).ok()?;
        let byte = self.data.get_ref().get(position).copied()?;
        self.data.set_position(self.data.position() + 1);
        Some(byte)
    }

    pub(crate) fn read_all(&mut self) -> Result<Vec<u8>, String> {
        self.reading();
        let data = self.data.get_ref();
        let position = usize::try_from(self.data.position())
            .unwrap_or(usize::MAX)
            .min(data.len());
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(data.len() - position)
            .map_err(|_| "allocation failed while reading anonymous file")?;
        bytes.extend_from_slice(&data[position..]);
        self.data.set_position(data.len() as u64);
        Ok(bytes)
    }

    pub(crate) fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() {
            return Ok(());
        }
        let previous_current = self.data.position();
        let position = if self.last_was_write {
            self.data.position()
        } else {
            self.write_position
        };
        let position = usize::try_from(position).map_err(|_| "anonymous file is too large")?;
        let end = position
            .checked_add(bytes.len())
            .ok_or("anonymous file is too large")?;
        if end > crate::mainutils::browser_files::MAX_FILE_BYTES {
            return Err("anonymous file exceeds 1048576 byte limit".into());
        }
        if end > self.data.get_ref().capacity() {
            // This is direct checked heap-ledger admission, with no arena
            // loan, allocation hooks, R evaluation, or destructor callbacks.
            // The old allocation and token remain live until replacement.
            let reservation = self
                .anchor
                .heap_identity()
                .reserve_payload_bytes(&self.anchor, end)
                .ok_or("anonymous file exceeds session memory limit")?;
            let mut replacement = Vec::new();
            replacement
                .try_reserve_exact(end)
                .map_err(|_| "allocation failed while writing anonymous file")?;
            replacement.extend_from_slice(self.data.get_ref());
            self.data = Cursor::new(replacement);
            self.data.set_position(position as u64);
            self.reservation = Some(reservation);
        }
        let data = self.data.get_mut();
        if end > data.len() {
            data.resize(end, 0);
        }
        data[position..end].copy_from_slice(bytes);
        if !self.last_was_write {
            self.read_position = previous_current;
            self.last_was_write = true;
        }
        self.data.set_position(end as u64);
        Ok(())
    }

    pub(crate) fn read_line(&mut self) -> Result<Option<(Vec<u8>, TransientReservation)>, String> {
        self.reading();
        let data = self.data.get_ref();
        let start = usize::try_from(self.data.position()).unwrap_or(usize::MAX);
        if start >= data.len() {
            return Ok(None);
        }
        let end = data[start..]
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n'))
            .map_or(data.len(), |offset| start + offset);
        let mut next = end;
        if next < data.len() {
            next += 1;
            if data[end] == b'\r' && data.get(next) == Some(&b'\n') {
                next += 1;
            }
        }
        // Retain the byte copy plus worst-case lossy UTF-8 string workspace
        // until the caller has published all resulting R strings.
        let reservation = self
            .anchor
            .heap_identity()
            .reserve_payload_bytes(
                &self.anchor,
                (end - start)
                    .checked_mul(4)
                    .ok_or("anonymous line is too large")?,
            )
            .ok_or("anonymous line exceeds session memory limit")?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(end - start)
            .map_err(|_| "allocation failed while reading anonymous line")?;
        bytes.extend_from_slice(&data[start..end]);
        self.data.set_position(next as u64);
        Ok(Some((bytes, reservation)))
    }

    pub(crate) fn seek(&mut self, offset: f64, origin: i32, rw: i32) -> Result<f64, String> {
        let current = self.data.position();
        if self.last_was_write {
            self.write_position = current;
        } else {
            self.read_position = current;
        }
        let previous = match rw {
            1 => {
                self.last_was_write = false;
                self.read_position
            }
            2 => {
                self.last_was_write = true;
                self.write_position
            }
            _ => current,
        };
        if offset.is_nan() {
            return Ok(previous as f64);
        }
        if !offset.is_finite() {
            return Err("invalid 'where' argument".into());
        }
        let base = match origin {
            2 => current,
            3 => self.data.get_ref().len() as u64,
            _ => 0,
        };
        let target = i128::from(base)
            .checked_add(offset.trunc() as i128)
            .ok_or("invalid 'where' argument")?;
        // GNU's file_seek ignores a failed negative fseek and keeps the offset.
        if target < 0 {
            return Ok(previous as f64);
        }
        let target = u64::try_from(target).map_err(|_| "invalid 'where' argument")?;
        self.data.set_position(target);
        if self.last_was_write {
            self.write_position = target;
        } else {
            self.read_position = target;
        }
        Ok(previous as f64)
    }
}

/// Select and own strings before the connection table is borrowed. A provider
/// may collect or detach its source slot; the selected character stays rooted.
pub(crate) fn text_chunks(
    source: &crate::sexp::Sexp<'static>,
    separator: &str,
) -> Result<(Vec<Vec<u8>>, Vec<TransientReservation>), String> {
    let anchor = source
        .allocation()
        .map_err(|_| "anonymous text backing is unavailable")?;
    let mut chunks = Vec::new();
    let mut reservations = Vec::new();
    for index in 0..source.len() {
        let character = source.try_string_elt(index).map_err(|e| e.to_string())?;
        let missing = character.is_na_string();
        let count = if missing {
            2
        } else {
            usize::try_from(character.try_char_len().map_err(|e| e.to_string())?)
                .map_err(|_| "anonymous text is too large")?
        };
        let capacity = count
            .checked_mul(3)
            .and_then(|count| count.checked_add(separator.len()))
            .ok_or("anonymous text is too large")?;
        let reservation = anchor
            .heap_identity()
            .reserve_payload_bytes(
                anchor,
                capacity
                    .checked_mul(2)
                    .ok_or("anonymous text is too large")?,
            )
            .ok_or("anonymous text exceeds session memory limit")?;
        let text = if missing {
            "NA".to_owned()
        } else {
            character.try_as_string().map_err(|e| e.to_string())?
        };
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| "allocation failed while writing anonymous text")?;
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend_from_slice(separator.as_bytes());
        chunks
            .try_reserve(1)
            .map_err(|_| "allocation failed while writing anonymous text")?;
        reservations
            .try_reserve(1)
            .map_err(|_| "allocation failed while writing anonymous text")?;
        chunks.push(bytes);
        reservations.push(reservation);
    }
    Ok((chunks, reservations))
}
