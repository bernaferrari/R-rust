# Plan 002: Return malformed-regex errors without terminating the host

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving on. If a
> STOP condition occurs, stop and report; do not improvise. A reviewer maintains
> `plans/README.md`.
>
> **Drift check (run first)**:
> `git diff --stat 462f1280..HEAD -- rmath-rs/rmath/src/tre/compile.rs rmath-rs/rmath/src/tre/regapi.rs rmath-rs/rmath/src/mainutils/grep.rs`
> Any semantic mismatch with the excerpts below is a STOP condition.

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `462f1280`, 2026-08-12
- **Tracker**: `rport-jxfp.4`

## Why this matters

TRE compilation cleanup calls `std::process::exit` for malformed user input.
An invalid pattern in `grep`, `sub`, `regexpr`, or an IDE search can therefore
kill the entire embedding process instead of producing an R error. Callers
already model compilation as recoverable; this plan restores that contract and
adds tests which would catch any future host termination.

## Current state

- `rmath-rs/rmath/src/tre/compile.rs:2033-2059` defines `goto_error(...) -> !`,
  frees compiler state, optionally calls `tre_free`, then exits the process with
  the TRE error code.
- The compile function has thirteen `goto_error` call sites around lines
  1808–1971. Because the helper is diverging, those call sites do not explicitly
  `return` an error code.
- `rmath-rs/rmath/src/tre/regapi.rs:697-701` explicitly omits malformed-regex
  tests because they kill the test process.
- `rmath-rs/rmath/src/mainutils/grep.rs:329-348` expects `tre_regncomp` to return
  a status and converts non-`REG_OK` into `Err("TRE compilation failed...")`.

Match the existing C-compatible convention: TRE public functions return
integer `REG_*` codes, `tre_regfree` is safe on a zero/empty regex object, and
Rust-facing callers translate the status into an R-facing error.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| TRE tests | `cargo test -p rmath tre:: -- --test-threads=1` | exit 0; malformed and valid tests pass |
| Grep tests | `cargo test -p rmath mainutils::grep:: -- --test-threads=1` | exit 0 |
| Host survival | run the new subprocess/integration test filter chosen in Step 3 | child exits normally and reports an R/TRE error |
| Full library | `cargo test -p rmath --lib -- --test-threads=1 </dev/null` | exit 0; no failures |
| Format | `cargo fmt --check --all` | exit 0 |
| Lint | `cargo clippy -p rmath --all-targets --all-features -- -D warnings` | exit 0 |

## Scope

**In scope**:

- `rmath-rs/rmath/src/tre/compile.rs`
- `rmath-rs/rmath/src/tre/regapi.rs`
- One existing rmath integration/subprocess test file, or one new test file
  under `rmath-rs/rmath/tests/`, only if direct unit tests cannot prove process
  survival through the public host boundary.
- `rmath-rs/rmath/src/mainutils/grep.rs` only if a minimal error-message or test
  adjustment is required; do not rewrite the engine.
- `rmath-rs/rmath/src/mainutils/essentials.rs` only for the narrow propagation
  of TRE compilation failures from `ere_*` helpers into the existing R error
  boundary. Review found that the registered app-facing builtins live here and
  currently collapse compile failure into “no match.”

**Out of scope**:

- Replacing TRE with the Rust `regex` crate.
- Changing valid-regex matching semantics or capture layouts.
- Parser, GC, or global R condition-system refactors.
- Swallowing errors or mapping all failures to a single hard-coded status.

## Git workflow

- Work in an isolated worktree on branch `advisor/002-regex-errors`.
- Use a conventional commit such as
  `fix(regex): return compile errors instead of exiting`.
- Do not push or alter the user's branch.

## Steps

### Step 1: Make cleanup return the original TRE status

Change `goto_error` into a cleanup helper which returns `errcode`. Update every
call site to return that code from the compile function. Preserve every cleanup
operation exactly once. Ensure `preg.value` cannot retain a dangling `tnfa`
pointer after the helper frees it.

Do not use panic/unwind across the C ABI and do not introduce `process::abort`
or `process::exit` elsewhere.

**Verify**:
`rg -n 'process::(exit|abort)|std::process' rmath-rs/rmath/src/tre` returns no
host-termination path for regex compilation.

### Step 2: Restore malformed-pattern unit coverage

Replace the omission comment in `regapi.rs` with table-driven tests for at
least unbalanced grouping, unterminated character classes, invalid repetition,
and malformed escapes. Each must return a non-`REG_OK` `REG_*` code and leave a
regex object which can be safely freed. Keep existing valid nested-group,
alternation, and capture tests green.

**Verify**: `cargo test -p rmath tre:: -- --test-threads=1` exits 0 with the new
malformed-pattern test names visible.

### Step 3: Prove the public host survives

Add the narrowest repository-consistent regression that invokes a public
regex-using R surface in a child process. Feed multiple malformed patterns.
Assert the child exits normally (not with a TRE numeric exit status or signal)
and that evaluation reports an error for each case. The registered builtins
route through `mainutils::essentials`; preserve the distinction between a
compile error and a successful compile with no match (for example by returning
a `Result` from the narrow `ere_*` helper). If an existing desktop host fixture
is available, extend it; do not add a new CLI solely for this.

**Verify**: run the specific new test and show all child cases exit 0 at the
harness level with expected R errors.

### Step 4: Run the full affected gate

Run TRE tests, grep tests, full rmath library tests with closed stdin, rustfmt,
and strict Clippy.

**Verify**: every command in the command table exits 0.

## Test plan

- Table-driven direct TRE compilation tests cover four malformed categories.
- Valid-regex tests remain unchanged and pass.
- A subprocess/public-boundary test distinguishes a recoverable R error from
  process exit; merely calling `tre_regncomp` in the same test is insufficient
  for this criterion.
- Where practical, assert returned codes match TRE categories rather than only
  `!= REG_OK`.

## Done criteria

- [ ] No malformed pattern can reach `process::exit` or abort from TRE compile.
- [ ] Cleanup is performed once and returns the originating `REG_*` code.
- [ ] Four malformed categories and representative valid patterns are tested.
- [ ] A public host subprocess survives malformed patterns and yields errors.
- [ ] Targeted/full tests, rustfmt, and strict Clippy pass.
- [ ] No files outside the in-scope list change.

## STOP conditions

Stop and report if:

- Correct cleanup requires unwinding through an `extern "C"` boundary.
- The original error status is unavailable at any call site.
- The proposed fix changes valid-pattern behavior or capture numbering.
- Public-host testing would require inventing a new production binary.
- Verification fails twice after one reasonable correction.

## Maintenance notes

Keep the low-level TRE API status-code based. Rust-facing layers may enrich the
message, but must not erase the original code. Review all future translated C
`goto`/cleanup helpers for other process-termination substitutions.
