#![forbid(unsafe_code)]
//! The deparser owns its current line as bounded bytes, without a C terminator.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineBufferError {
    TooLong,
    Allocation,
}

#[derive(Default)]
pub(crate) struct OwnedLineBuffer {
    bytes: Vec<u8>,
}

impl OwnedLineBuffer {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn clear(&mut self) {
        self.bytes.clear();
    }

    /// Growth, including indentation, is checked before changing the line.
    /// Callers supply payload bytes only; no source terminator is read.
    pub(crate) fn append(
        &mut self,
        bytes: &[u8],
        indentation: usize,
    ) -> Result<i32, LineBufferError> {
        let additional = indentation
            .checked_add(bytes.len())
            .ok_or(LineBufferError::TooLong)?;
        let length = self
            .bytes
            .len()
            .checked_add(additional)
            .ok_or(LineBufferError::TooLong)?;
        let length = i32::try_from(length).map_err(|_| LineBufferError::TooLong)?;
        self.bytes
            .try_reserve(additional)
            .map_err(|_| LineBufferError::Allocation)?;
        self.bytes.resize(self.bytes.len() + indentation, b' ');
        self.bytes.extend_from_slice(bytes);
        Ok(length)
    }

    pub(crate) fn indentation(level: i32) -> Result<usize, LineBufferError> {
        let level = usize::try_from(level.max(0)).map_err(|_| LineBufferError::TooLong)?;
        level
            .min(4)
            .checked_mul(4)
            .and_then(|first| {
                level
                    .saturating_sub(4)
                    .checked_mul(2)
                    .and_then(|rest| first.checked_add(rest))
            })
            .ok_or(LineBufferError::TooLong)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_line_growth_empty_unicode_and_rejection_preserve_bytes() {
        let mut line = OwnedLineBuffer::default();
        assert_eq!(line.append(&[], 0), Ok(0));
        let text = "λ café".as_bytes().to_vec();
        assert_ne!(text.last(), Some(&0));
        assert_eq!(line.append(&text, 4), Ok((4 + text.len()) as i32));
        for _ in 0..200 {
            line.append(&text, 0).unwrap();
        }
        let expected = [b"    ".as_slice(), text.repeat(201).as_slice()].concat();
        assert_eq!(line.bytes(), expected);
        assert_eq!(line.append(b"x", usize::MAX), Err(LineBufferError::TooLong));
        assert_eq!(line.bytes(), expected);
        line.clear();
        assert!(line.bytes().is_empty());
        assert_eq!(line.append(b"next", 0), Ok(4));
        assert_eq!(line.bytes(), b"next");
        assert_eq!(OwnedLineBuffer::indentation(6), Ok(20));
    }
}
