# Runtime hardening assessment

This change improves memory ownership, R behavior and deployable browser/mobile
boundaries. It does not establish complete GNU R compatibility or a proof that
all internal unsafe code is sound.

## Garbage collection and ownership

Rust roots have stable slot/generation identities. Guards retain their owner
lifetime and cannot move between threads. Reprotection rejects stale identities
and values belonging to another session. Managed roots and the legacy
PROTECT/UNPROTECT stack have distinct removal rules; scope cleanup cannot revoke
an independently held managed guard. Context pointers are derived through
UnsafeCell after the owning allocation enters the context stack.

The collector marks through an explicit worklist, visiting objects before their
children. Deep and cyclic graphs no longer consume one native stack frame per
edge. The native regression covers 30,000 nodes; its Miri variant covers 256.
Raw interpreter modules are now crate-private. Embedders use owned values and
session handles; downstream raw-module compile-fail tests check this boundary.
See [the safety audit](safe-api-audit.md) for limits of this containment.

Arena access now rejects a second mutable lend of the same session before
creating the reference. The guard unwinds cleanly and allows deferred collection
after the borrow ends. Session parsing no longer obtains `&mut RInstance` through
a shared `&self`. Vector header access checks the active union tag, exposing and
correcting invalid reads in indexing, comparisons and symbol formatting.
Graphics parameter queries snapshot owned values before allocating R results.
Evaluator input expressions, evaluated builtin arguments and on-demand internal
primitives are rooted across re-entry. Bytecode variable lookup now roots live
operands before forcing promises or invoking active bindings; zeallot and small
forced-collection regressions exercise the reclaimed-vector counterexample.
String/list element reads and writes now check tags, buffers and indices before
pointer arithmetic. String writes also record old-to-young GC edges, verified
by a previously failing remembered-set regression. A Miri reproducer that wrote
past a string buffer now reports a checked error instead.
These changes reduce concrete aliasing hazards; they do not remove all raw core
internals or establish whole-interpreter soundness.

Pending signalled conditions and math-library warning calls now belong to each
session's error state. The collector marks and updates these roots, including
nested warning-call stacks. Scoped warning cleanup restores its creating
session, even when another session is ambient. Two targeted tests force GC and
switch sessions on the same thread. Calling-handler markers, tryCatch
handler-class stacks and source-reference state are now session-owned as well. Process environment reads and timezone
behavior still need an explicit isolation contract. Both public and legacy
internal environment mutators enforce the default deny policy; a host that
opts into process environment mutation still needs appropriate process isolation.

## Faithfulness

The public solve implementation uses the selected numerical backend, with
column-major layout, real/complex coercion, result shape and tolerance handling.
The faer LAPACK adapters use packed-LU condition estimation and corrected pivoted
Cholesky rank/permutation handling. Complex products retain matrix names and use
plain transposes for crossprod, matching R.

Wrappers use normal R evaluation and promise behavior. Plot requests execute R
code and S3 dispatch, including user methods. Atomic as.list copies payloads;
closure conversion exposes the original expression instead of bytecode. Call
coercion counts linked cells instead of reading a pointer through the vector
length union, eliminating the intermittent enormous allocation seen in zeallot.

LOESS now has an owned Rust fitting and prediction engine, with faer SVD,
robust iterations, interpolation and standard errors. Portable graphics supports
shared coordinates and layered fitted curves, with GEPretty linear axis ticks.
Numerical operations reject oversized workspace estimates before allocation and
poll cancellation during local fits, interpolation and exact diagnostics. See [contracts and limits](loess-and-portable-graphics.md); legacy native helper ABIs remain unsupported.

## Platform behavior

The browser defaults to Rust in a Worker. It handles ordinary R errors without
losing the session, but cancellation resets the Worker and its in-memory state.
Unexpected interpreter panics close the session before reuse. Browser builds use
a pinned nightly with Wasm exception handling because R control flow relies on
unwinding. The production size gate measures the delivered runtime assets.

UniFFI is restored to the workspace, so its worker, cancellation, callback and
ownership tests participate in the standard suite again. Kotlin binding
regeneration is checked. UniFFI now publishes retained results before queuing
terminal callbacks; callbacks can immediately retrieve completed eval/plot
results. This is host-side validation; it does not replace Android or iOS device
testing. Package tests unpack committed fixtures into
independent temporary directories, avoiding partial developer-library caches.

## Validation recorded during hardening

- Workspace: 2,674 passing tests after accessor, LOESS resource/NA, axis and callback
  integration.
- Strict pinned-GNU-R three-way parity: 634/634, zero expected failures/skips.
- LOESS numerical oracle tests: 23/23 with faer and 23/23 with the system-backend
  profile, including multivariate, weighted, robust and uncertainty contracts.
  Five additional execution/NA regressions pass on both backends.
- Public LOESS: eleven tests, including model frames, matrix predictors, malformed
  models, omission metadata, cancellation/recovery and serialization under forced
  collection.
- Linked system LAPACK adapter contracts: 32/32 on macOS Accelerate.
- Targeted Miri this pass: the out-of-bounds string setter regression and shared
  invalid-index/tag access tests passed. The setter test first reproduced a
  write beyond its allocation; native tests also reproduce and fix a missing
  string write barrier.
- Earlier targeted Miri: nested arena lend rejection, bytecode lookup during collection,
  and character-header initialization passed in the preceding pass. The header test first
  reproduced an uninitialized true-length read, then passed after all four
  constructors initialized the full shared header. Earlier serialization under
  GC torture, nested on.exit/GC contexts and deep cyclic traversal also passed.
  Leak checking was disabled; these
  runs do not establish strict-provenance or whole-interpreter soundness.
- Actual Wasm execution: Node contracts and Chromium production UI/Worker
  evaluation, errors, nonlocal control flow, PNG and cancellation/reset passed.
- Production assets: Kotlin UI 374,563 bytes; Rust runtime 9,901,261 bytes, including bundled Noto Sans.
- Kotlin UniFFI generation matches the checked-in bindings.

Reproduce with cargo test --workspace, scripts/conformance_parity.sh --check
--strict (using the pinned oracle), scripts/wasm_m3_smoke.sh and the production
browser procedure in [web architecture](web-architecture.md). CI also checks
formatting, warnings-denied Clippy, the capability inventory and API boundary.

## What prevents a whole-port 9/10 claim

The [generated compatibility inventory](capability-evidence.md) records 634
curated fixtures and selected probes for seven packages. Of 70 tracked upstream
whole files, only one is marked passing; nine are expected failures and 60 are
skipped. These are coverage limitations, not percentages of the language.

The next acceptance bar is substantially broader upstream whole-file and real
package execution, an audit of remaining raw union access and ambient session
aliasing, complete graphics contracts, larger statistical workloads, and
measured device/performance evidence. The issue tracker retains these follow-ups. Additional translated code or a higher subjective score would
not substitute for those checks.

## September 8 follow-up contracts

Interactive embedding now retains an owned scene between commands: a `plot()`
followed by `lines()` produces the same image as one combined evaluation. A
resize scales the retained scene, and closing the session releases it. Retained
operation accounting has a 16 MiB budget, checked before cloning paths, text or
rasters; exceeding it returns a recoverable error. This budget does not include
all renderer/transient allocations. Partial console/plot output on errors and
complete device lifecycle semantics remain unfinished.

Source-reference locations and warning/tryCatch bookkeeping now belong to the
session. Handler cleanup restores its creating session. Embedded sessions deny
process environment mutation by default; trusted desktop hosts can opt in.
Process-global reads and timezone handling are still separate isolation concerns.

The memory deserializer rejects vector lengths whose minimum encoded payload
cannot fit in the input before allocating R vectors. ASCII string decoding
checks the available bytes before reserving its buffer. Recursive object decoding
is bounded to 128 levels to protect the native stack; legitimate deeper input is
also rejected by this reader. These checks do not establish full serialization
or bytecode wire compatibility.

Named S4 method signatures are reordered to match generic arguments, with invalid
and duplicate names rejected. Generic method tables now have separate environments that preserve lexical
captures; registering one generic cannot overwrite another generic’s methods.
Broader method inheritance and whole upstream methods-suite compatibility remain
unproven.

The same-font GNU R typography corpus now covers 14 expressions at 6, 12 and 24
points. It caught incorrect accent glyphs, spacing and loss of signed glyph depth;
those discrepancies are corrected. These 42 metric comparisons do not establish
pixel parity across arbitrary fonts and devices.

Validation for this follow-up: 2,803 workspace tests passed (five ignored),
followed by all 25 portable-grid tests after the final graphical-parameter merge
fix. Six real-Wasm browser tests passed, including the existing 29 FFT/RNG oracle
cases, persistent layers, S4 signature ordering, independent generic tables and
nonmutating nested grob edits. The safe API audit, formatting, website lint and
production build also passed. These results do not change the whole-upstream
coverage ledger into a full compatibility claim.

## September 8 safety checkpoint

Immutable singleton slots now retain pointers in `OnceLock<AtomicPtr<_>>`
instead of converting their addresses to integers and back. This preserves
pointer provenance without asserting that mutable interpreter objects are
`Send` or `Sync`. The singleton-only regression passes Miri with strict
provenance; this is a narrow result, not a strict-provenance proof for the
interpreter as a whole.

PNG encoding now unpremultiplies its rendered pixmap in place and borrows the
bytes for encoding. It avoids the previous duplicate RGBA canvas (up to 64 MiB
at the 16,777,216-pixel limit). Rasterization, geometry, compression and other
native workspaces still prevent this from being a total process-memory cap.

Conformance helpers default to `target/conformance` and honor explicit Cargo
target directories, so ordinary workspace artifacts cannot silently substitute
for an isolated build. Concurrent conformance runs using different build flags
must still use separate target directories.

Full GNU R remains an open compatibility target. The current whole-file
upstream ledger lists one passing driver, nine expected failures and sixty
skips. Curated cases and narrow oracle comparisons establish useful contracts;
they do not establish full language, package, graphics or I/O compatibility.
Compiler/bytecode and lazy-load formats, broader methods behavior, advanced grid
and device semantics, font/device typography, host-state isolation and total
resource accounting remain substantial work alongside package support.

## Compiled-closure import and dispatch follow-up

GNU R bytecode version 12 instruction framing is checked against all 129 pinned
opcode widths. Imported compiled closure bodies are decoded with bounded
language/repetition records and evaluated from their retained source expression.
This supports interpreted execution of the tested compiler-produced closures,
including defaults, branches, captured environments, nested functions and loops.
It does **not** implement the GNU bytecode VM or preserve compiled-body identity.
If bytecode is independently modified to disagree with its retained source, this
fallback follows the source and cannot reproduce the modified bytecode behavior.
Standalone GNU bytecode objects and serialization of the private VM dialect
still fail explicitly. Compiler package APIs, older bytecode versions, namespace
restoration, package lazy-load databases and full wire-format parity remain open.
The reproducible fixtures and their GNU R generator live in
`crates/r-embed/tests/fixtures/generate-compiled-closures.R`.

Environment serialization now preserves binding frames, shared/cyclic references,
parents and environment locks; imported GNU hash buckets are restored as bindings.
Read-reference entries stay rooted until decoding finishes, including compiled
constants that are discarded after source extraction. This does not implement
active-binding or per-binding lock serialization contracts.

S4 table dispatch now supports unambiguous exact/ANY combinations, omitted
trailing signature arguments, and distinct NULL/missing dispatch. Oversized
signatures fail before registration. Ambiguous wildcard combinations still fail
explicitly; complete class-distance selection and its diagnostics remain open.

Default host capabilities now also deny process working-directory and locale
mutations. An empty string passed to `Sys.setlocale` is a mutation request, not a
query, and is denied too; `Sys.getlocale` remains available. This is containment,
not a complete virtual filesystem, locale or environment snapshot per session.

Interactive errors now carry partial console output and an optional plot through
r-embed, Wasm and the browser console. Retained-scene budget errors take priority
and do not return a truncated plot as a successful drawing. Wasm callers should
inspect `WasmInteractiveOutput.has_error()` as well as output and PNG; evaluation
errors with partial output now return this result instead of throwing away the
output in a JavaScript exception.

Validation for this batch: `cargo test --workspace` passed 2,823 tests with five
ignored tests; the console and Wasm contract browser suites passed all 13 tests.
Website lint, production/prerender build and the Wasm rebuild passed. These are
regression results, not evidence of complete GNU R compatibility or a security
certification.

The additional malformed-bytecode Miri run was stopped during runtime
initialization without a test result. It reported an exposed-provenance warning
in protection-guard owner reconstruction (`sexp/protect.rs:474`), tracked as
`rport-q1in`; this run is not counted as a Miri pass.
