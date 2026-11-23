#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::io::Read;

/// Read an entire R source stream, lossy-decoding it to text.
///
/// Companion to the parser entry points in `gram_main`: callers hand in the
/// already-open source stream instead of a C `FILE*`.
pub(super) fn read_r_source(mut reader: impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|_| "failed reading R source file".to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
