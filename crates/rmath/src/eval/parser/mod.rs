//! Full R expression parser.
//!
//! Recursive-descent parser that converts R source code into SEXP trees
//! suitable for evaluation by `eval_safe()`.
//!
//! Supports:
//! - Numbers (integer and real), strings, identifiers
//! - All R keywords: TRUE, FALSE, NULL, NA, Inf, NaN, NA_real_, NA_integer_, NA_character_, NA_complex_
//! - Binary operators: +, -, *, /, ^, <, >, <=, >=, ==, !=, &, &&, |, ||, %%, %/%
//! - Native pipe: x |> f(), x |> f(y = _), x |> _[i] / _$col extractor chains
//! - Custom infix operators: %in%, %o%, %*%, any %xxx% sequence
//! - Unary minus/plus
//! - Assignment: <-, =, ->, <<-
//! - Function calls: f(x, y), f(x = 1)
//! - Parenthesized expressions: (expr)
//! - Blocks: { expr; expr; ... }
//! - Control flow: if/else, for, while, repeat, break, next, return
//! - Function definitions: function(args) body
//! - Subscript: x[i], x[i, j], x[i, ], x[[i]], x[[i, j]] (empty slots → missing arg)
//! - Member access: x$name, x@slot
//! - Formula: y ~ x
//! - Backtick names: `weird name`
//! - ... varargs

use std::ffi::CString;

use crate::sexp::accessors::{
    CADR, CAR, CDR, CHAR, INTEGER, PRINTNAME, SET_ATTRIB, SETCAR, SETTAG, TAG, TYPEOF,
};
use crate::sexp::builder::{
    scalar_bytes_in, scalar_complex_in, scalar_integer_in, scalar_logical_in, scalar_real_in,
    scalar_string_in,
};
use crate::sexp::ffi::{FALSE, NA_LOGICAL, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NaString, R_NilValue};
use crate::sexp::memory::RArena;
use crate::sexp::object::{NodeDomain, SessionNodeFactory, Sexp};
use crate::sexp::symbol::Rf_install;
// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub(crate) struct Parser<'arena, 'session> {
    domain: NodeDomain<'session>,
    tokens: Vec<Token>,
    /// Half-open char span `[start, end)` of each token in `source`,
    /// parallel to `tokens`. Used to render upstream-style parse errors
    /// (`unexpected ')' in "<context>"`); the EOF token has an empty span.
    spans: Vec<(usize, usize)>,
    /// The parsed source as chars, for slicing token context windows.
    source: Vec<char>,
    pos: usize,
    arena: &'arena mut RArena,
    /// The shared node representing the pipe placeholder `_`.
    ///
    /// Upstream represents the placeholder as a distinguished string, so it
    /// prints as `"_"` in error messages and can be spotted by identity in
    /// the post-parse scan. One node per parser keeps identity comparisons
    /// valid while user `"_"` string literals remain distinct objects.
    placeholder: SEXP,
    /// Whether the lexer produced any `=>` (PIPEBIND) token. gram.y's
    /// `HavePipeBind`: it arms the post-parse "invalid use of pipe bind
    /// symbol" scan so inputs without `=>` skip it entirely.
    have_pipebind: bool,
    /// Per-token flag: is this token inside an unclosed `(`/`[`/`{` group?
    /// Mirrors gram.y's contextstack/IfPush: an `else` after a newline only
    /// attaches to its `if` when the `if` sits inside such a group; at top
    /// level the newline terminates the `if` and `else` is a syntax error.
    inside_group: Vec<bool>,
    /// Newline before an infix operator continues only inside `(` / `[`.
    /// A `{` block is a statement list: `0\n-6` is a new unary minus, while
    /// `(1\n-2)` is still binary minus. `1 +\n2` continues everywhere because
    /// the operator is consumed before the newline.
    inside_nest: Vec<bool>,
    /// Token index of the innermost unclosed `(`/`[`/`{` opener at each
    /// position (parallel to `inside_group`): `None` at top level. Used by
    /// `if_closed_before` to tell a `]`/`)`/`}` that merely closes an inner
    /// group (e.g. a call inside the `if` body — the `if` stays open) from
    /// one that closes the `if`'s own group (the `if` is completed).
    group_opener: Vec<Option<usize>>,
    /// Newline-crossing `else` attaches only inside a group. Top-level
    /// `if`/`else` split by a newline is an error for both `parse()` and
    /// interactive input, so there is no separate strict flag.
    /// L-suffix warnings produced while tokenizing, keyed by token index.
    token_literal_warnings: Vec<(usize, String)>,
    /// GNU `parse(keep.source=TRUE)` / `source(keep.source=TRUE)`: attach
    /// `srcref` attributes on `function()` language objects.
    keep_srcrefs: bool,
}
// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Capture the live translated caller's owner before lending its arena.
///
/// # Safety
/// The active instance allocation must outlive every use of the returned
/// factory and handle. Only session-borrowed callers may expose handles to safe code.
pub(crate) unsafe fn active_factory<'session>() -> SessionNodeFactory<'session> {
    let owner =
        crate::sexp::instance::current_instance_ptr().expect("parsing requires an active session");
    SessionNodeFactory::new(unsafe { crate::sexp::owner::OwnerToken::from_raw(owner) })
}

// ---------------------------------------------------------------------------

/// Parse R source into an expression tree allocated in `arena`.
///
/// The arena should belong to the active `RSession`: symbols are interned in
/// the active session while expression nodes and literals are allocated in this
/// borrowed arena.
pub(crate) fn parse<'session>(
    input: &str,
    arena: &mut RArena,
    domain: impl Into<NodeDomain<'session>>,
) -> Result<Sexp<'session>, ParseError> {
    let mut parser = Parser::new(input, arena, domain);
    parser.parse_program()
}

/// Parse R source into the sequence of top-level expressions it contains.
///
/// This mirrors GNU R's `parse()` result shape: a source stream with multiple
/// complete expressions becomes an `EXPRSXP` with one element per expression,
/// instead of a synthetic `{ ... }` block used by direct evaluation.
pub(crate) fn parse_expressions<'session>(
    input: &str,
    arena: &mut RArena,
    domain: impl Into<NodeDomain<'session>>,
) -> Result<Vec<Sexp<'session>>, ParseError> {
    let mut parser = Parser::new(input, arena, domain);
    parser.parse_top_level_expressions()
}

/// Parse R source the way `parse()`/`source()` do: newline-crossing `else`
/// is always rejected (gram.y reports ELSE after a newline only when the
/// `if` is still on the lexer's context stack, and the file-parse path
/// never carries a statement-continuation context across the newline).
/// The interactive top-level path (`parse_program`, used by eval) keeps
/// the lenient gate that attaches `else` inside an unclosed group.
pub(crate) fn parse_expressions_strict<'session>(
    input: &str,
    arena: &mut RArena,
    domain: impl Into<NodeDomain<'session>>,
) -> Result<Vec<Sexp<'session>>, ParseError> {
    let mut parser = Parser::new(input, arena, domain);
    let _ = &mut parser;
    parser.parse_top_level_expressions()
}
mod blocks;
mod control;
mod core;
mod expr;
mod lexer;
mod postfix;
mod primary;
mod support;
#[cfg(test)]
mod tests;

// reexport path kept via mod blocks; items are pub(crate)
// reexport path kept via mod control; items are pub(crate)
// reexport path kept via mod core; items are pub(crate)
// reexport path kept via mod expr; items are pub(crate)
pub(crate) use lexer::*;
// reexport path kept via mod postfix; items are pub(crate)
// reexport path kept via mod primary; items are pub(crate)
pub use support::*;
