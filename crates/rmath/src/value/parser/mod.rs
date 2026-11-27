//! Native R parser producing new-world heap objects directly.
//!
//! Recursive-descent parser mirroring the legacy SEXP parser's grammar
//! exactly — same precedence, same newline-`else` semantics, same upstream
//! error rendering — but allocating parse trees straight into the
//! generational `Heap` as `Language`/`Pairlist`/`Symbol` objects, the very
//! shapes the bridge importer pins and the evaluator consumes.

mod blocks;
mod control;
mod core;
mod expr;
mod lexer;
mod postfix;
mod primary;
mod support;

pub(crate) use lexer::{Lexer, Token};
pub(crate) use support::{is_special_rhs_function, pipebind_enabled, token_display, PARSE_CONTEXT_WINDOW};

use crate::value::heap::Heap;
use crate::value::id::ValueId;
use crate::value::intern::SymbolInterner;
use crate::value::object::RObject;

/// A parse failure rendered in upstream's exact shape.
#[derive(Debug, Clone)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub(crate) struct Parser<'h, 'i> {
    tokens: Vec<Token>,
    /// Half-open char span `[start, end)` of each token in `source`,
    /// parallel to `tokens`. Used to render upstream-style parse errors
    /// (`unexpected ')' in "<context>"`); the EOF token has an empty span.
    spans: Vec<(usize, usize)>,
    /// The parsed source as chars, for slicing token context windows.
    source: Vec<char>,
    pos: usize,
    heap: &'h mut Heap,
    interner: &'i mut SymbolInterner,
    /// The shared node representing the pipe placeholder `_`.
    ///
    /// Upstream represents the placeholder as a distinguished string, so it
    /// prints as `"_"` in error messages and can be spotted by identity in
    /// the post-parse scan. One node per parser keeps identity comparisons
    /// valid while user `"_"` string literals remain distinct objects.
    placeholder: ValueId,
    /// Whether the lexer produced any `=>` (PIPEBIND) token. gram.y's
    /// `HavePipeBind`: it arms the post-parse "invalid use of pipe bind
    /// symbol" scan so inputs without `=>` skip it entirely.
    have_pipebind: bool,
    /// Per-token flag: is this token inside an unclosed `(`/`[`/`{` group?
    inside_group: Vec<bool>,
    /// Newline before an infix operator continues only inside `(` / `[`.
    inside_nest: Vec<bool>,
    /// Token index of the innermost unclosed `(`/`[`/`{` opener at each
    /// position.
    group_opener: Vec<Option<usize>>,
    /// Newline-crossing `else` attaches only inside a group.
    token_literal_warnings: Vec<(usize, String)>,
}

/// Parse R source into top-level expressions as new-world heap objects —
/// the entry point the runtime surface uses once the SEXP parser retires.
pub(crate) fn parse_native_expressions(
    input: &str,
    heap: &mut Heap,
    interner: &mut SymbolInterner,
) -> Result<Vec<ValueId>, ParseError> {
    let mut parser = Parser::new(input, heap, interner);
    parser.parse_top_level_expressions()
}
