#![forbid(unsafe_code)]
//! Actual text writer storage, moved into one operation during publication.
use crate::sexp::Sexp;

pub(super) struct TextOutput {
    pub(super) lines: Vec<String>,
    pub(super) name: Option<String>,
    pub(super) environment: Option<Sexp<'static>>,
    pub(super) incomplete: bool,
    pub(super) published: usize,
}
impl TextOutput {
    pub(super) fn new(name: Option<String>, environment: Option<Sexp<'static>>) -> Self {
        Self {
            lines: Vec::new(),
            name,
            environment,
            incomplete: false,
            published: 0,
        }
    }
    pub(super) fn append(&mut self, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return false;
        }
        let text = String::from_utf8_lossy(bytes);
        let newline = text.ends_with('\n');
        let mut pieces = text.split('\n').peekable();
        let mut first = true;
        while let Some(piece) = pieces.next() {
            if newline && pieces.peek().is_none() {
                break;
            }
            if first && self.incomplete {
                if let Some(last) = self.lines.last_mut() {
                    last.push_str(piece);
                } else {
                    self.lines.push(piece.to_owned());
                }
            } else {
                self.lines.push(piece.to_owned());
            }
            first = false;
        }
        if !first {
            self.incomplete = !newline;
        }
        text.contains('\n')
    }
    pub(super) fn complete_len(&self, include_partial: bool) -> usize {
        self.lines.len()
            - usize::from(!include_partial && self.incomplete && !self.lines.is_empty())
    }
}
