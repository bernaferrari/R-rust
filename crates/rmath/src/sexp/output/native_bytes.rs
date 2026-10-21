#![forbid(unsafe_code)]
//! One budgeted byte snapshot, with text conversion only at UTF8 captures.
use crate::sexp::{
    memory::TransientReservation,
    object::{SexpError, SexpResult},
};
pub(crate) struct NativeOutputBytes {
    data: Vec<u8>,
    _ticket: TransientReservation,
}
impl NativeOutputBytes {
    pub(crate) fn copy(bytes: &[u8], ticket: TransientReservation) -> SexpResult<Self> {
        let failure = || SexpError::AllocationFailed {
            object: "native output byte snapshot",
        };
        if ticket.bytes() != bytes.len() {
            return Err(failure());
        }
        let mut data = Vec::new();
        data.try_reserve_exact(bytes.len()).map_err(|_| failure())?;
        data.extend_from_slice(bytes);
        Ok(Self {
            data,
            _ticket: ticket,
        })
    }
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}
/// Valid UTF8 spans are unchanged. Each invalid wire byte becomes an ASCII
/// hex escape, never an invalid str. The consumer can stop at its own cap.
pub(crate) fn visit_capture_text(mut bytes: &[u8], mut emit: impl FnMut(&str) -> bool) {
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                emit(text);
                return;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if valid != 0
                    && !emit(std::str::from_utf8(&bytes[..valid]).expect("validated prefix"))
                {
                    return;
                }
                let bad = error.error_len().unwrap_or(bytes.len() - valid);
                for &b in &bytes[valid..valid + bad] {
                    let escape = [
                        b'\\',
                        b'x',
                        b"0123456789abcdef"[(b >> 4) as usize],
                        b"0123456789abcdef"[(b & 15) as usize],
                    ];
                    if !emit(std::str::from_utf8(&escape).expect("ASCII escape")) {
                        return;
                    }
                }
                bytes = &bytes[valid + bad..];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_output_snapshot_owns_its_source_and_rejects_mismatched_admission() {
        let session = crate::sexp::RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let ticket = owner
                .with_arena(|a| a.try_reserve_transient(4).unwrap())
                .unwrap();
            assert!(NativeOutputBytes::copy(b"bad", ticket).is_err());
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            let mut source = vec![0xc3, 0xa9, 0xff];
            let ticket = owner
                .with_arena(|a| a.try_reserve_transient(3).unwrap())
                .unwrap();
            let message = NativeOutputBytes::copy(&source, ticket).unwrap();
            source.fill(0);
            drop(source);
            assert_eq!(message.as_bytes(), [0xc3, 0xa9, 0xff]);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 3);
            drop(message);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        });
    }
}
