# Plan 009: Make malformed R source fail atomically

> **Executor instructions**: Preserve valid GNU R behavior, add differential
> tests first, and stop if a malformed script can still execute a prefix.
>
> **Drift check**:
> `git diff --stat 462f1280..HEAD -- rmath-rs/rmath/src/eval/parser.rs rmath-rs/rmath/src/sexp/session.rs crates/r-embed crates/r-uniffi tests fixtures`

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: Plans 001 and 003
- **Category**: bug
- **Planned at**: commit `462f1280`, 2026-08-12
- **Tracker**: `rport-jxfp.7`

## Why this matters

Unknown characters become EOF, unterminated strings/backticks are accepted,
malformed numbers become zero, and some allocation failures become null SEXPs.
A script with a valid prefix and malformed tail may therefore execute side
effects instead of failing as one parse unit. The IDE must be able to trust that
syntax/resource errors are explicit and side-effect-free.

## Current state

- `eval/parser.rs:161-355` returns `Token::Eof` for unknown characters after
  consuming them.
- `:358-435` narrows hexadecimal `i64 as i32` and uses `0.0` fallbacks for
  malformed numeric text.
- `:437-517` treats EOF as successful termination of raw, quoted, and backtick
  strings.
- `:580-592` tokenizes until EOF with no lexical error channel.
- `:601-652` converts builder allocation failure to raw null SEXP.
- `:1668-1697` adversarial tests assert only “does not panic,” not failure.
- `sexp/session.rs:480-517` parses all expressions before evaluation (good), but
  wraps each raw expression through `expr_or_nil`; single-expression path
  `:602-618` also converts parser output via `expr_or_nil`.

Follow existing public `ParseError`/`REvalError` propagation; do not panic or
unwind across embedding/FFI boundaries.

## Commands

| Purpose | Command | Expected |
|---|---|---|
| Parser tests | `cargo test -p rmath eval::parser::tests -- --test-threads=1` | exit 0 |
| Session tests | `cargo test -p rmath sexp::session::tests -- --test-threads=1` | exit 0 |
| Embedding/FFI | `cargo test -p r-embed -p r-uniffi -- --test-threads=1` | exit 0 |
| Stock-R differential | `PATH="/opt/homebrew/Cellar/r/4.6.1/bin:$PATH" scripts/conformance_parity.sh --check --strict` (or CI `Rscript`) | exit 0 |
| Format/lint | `cargo fmt --check --all && cargo clippy -p rmath -p r-embed -p r-uniffi --all-targets --all-features -- -D warnings` | exit 0 |

## Scope

**In scope**:

- Lexer/token/error handling and checked constructors in
  `rmath-rs/rmath/src/eval/parser.rs`
- Minimal evaluation-boundary propagation in `sexp/session.rs`, `r-embed`, and
  `r-uniffi`
- Stable malformed/numeric/resource fixtures and tests

**Out of scope**:

- General grammar expansion, bytecode compiler, evaluator semantics, or a new
  parser library.
- “Fixing” valid R numeric/string behavior without stock-R evidence.

## Git workflow

- Branch `advisor/009-atomic-parse-errors`, isolated worktree.
- Conventional commit: `fix(parser): reject malformed source atomically`.
- Do not push or modify the user's branch.

## Steps

### Step 1: Add stock-R failure and side-effect fixtures

Cover unknown characters, incomplete operators, malformed exponent/hex/integer,
unterminated quote/raw/backtick/custom infix, and mismatched delimiters. For
multi-expression input such as `x <- 1; <malformed>`, evaluate in a fresh
session and assert `x` was not assigned after the parse error.

**Verify**: fixtures reproduce GNU R's error-vs-value/type categories and fail
against the current implementation for the intended reasons.

### Step 2: Give the lexer spanned errors

Make tokenization return a lexical result that can represent invalid character,
unterminated construct, and invalid numeric literal with a source span. Remove
numeric zero fallbacks and unchecked narrowing; match supported GNU R
integer/double behavior from Step 1.

**Verify**: table-driven lexer/parser tests assert error category and location.

### Step 3: Propagate allocation/resource errors

Replace null-returning parser constructors with checked results and propagate a
distinct resource-limit/allocation error through session, embedding, and
UniFFI. Empty valid input may still produce R NULL if that matches the public
contract; allocation failure must never do so.

**Verify**: end-to-end tests at session, r-embed, and UniFFI boundaries set
small limits and assert a resource error, not `NULL` or partial execution.

### Step 4: Enforce parse-before-execute atomicity

Retain the parse-all-first structure for scripts and remove any `expr_or_nil`
conversion that masks an invalid/null parse node. Add the multi-expression
side-effect regressions to both native and embedding boundaries.

**Verify**: parser/session/embed/FFI and strict stock-R differential commands
all pass.

## Done criteria

- [ ] Unknown/unterminated/malformed tokens return spanned errors, never EOF or
  zero values.
- [ ] Numeric overflow/type behavior matches the supported stock-R fixtures.
- [ ] Allocation/resource failures remain distinct through UniFFI.
- [ ] No valid prefix of a malformed script produces side effects.
- [ ] Parser/session/embed/FFI/differential tests, format, and strict Clippy pass.

## STOP conditions

Stop if stock R behavior is ambiguous for a fixture, error propagation requires
an incompatible public ABI change without a migration, parse-all atomicity
would require evaluator rollback, or verification fails twice.

## Maintenance notes

Keep malformed/adversarial corpora asserting semantic failure and side-effect
absence, not only panic freedom. Fuzzing should compare accepted/rejected input
against pinned GNU R and minimize any discrepancy into a stable fixture.

