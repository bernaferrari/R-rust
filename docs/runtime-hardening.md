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
These changes reduce concrete aliasing hazards; they do not remove all raw core
internals or establish whole-interpreter soundness.

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
shared coordinates and layered fitted curves. See [contracts and limits](loess-and-portable-graphics.md); legacy native helper ABIs remain unsupported.

## Platform behavior

The browser defaults to Rust in a Worker. It handles ordinary R errors without
losing the session, but cancellation resets the Worker and its in-memory state.
Unexpected interpreter panics close the session before reuse. Browser builds use
a pinned nightly with Wasm exception handling because R control flow relies on
unwinding. The production size gate measures the delivered runtime assets.

UniFFI is restored to the workspace, so its worker, cancellation, callback and
ownership tests participate in the standard suite again. Kotlin binding
regeneration is checked. This is host-side validation; it does not replace
Android or iOS device testing. Package tests unpack committed fixtures into
independent temporary directories, avoiding partial developer-library caches.

## Validation recorded during hardening

- Workspace: 2,654 passing tests after LOESS, rooting, panel and character-header
  integration.
- Strict pinned-GNU-R three-way parity: 633/633, zero expected failures/skips.
- LOESS numerical oracle tests: 23/23 with faer and 23/23 with the system-backend
  profile, including multivariate, weighted, robust and uncertainty contracts.
- Public LOESS: five tests, including model frames, matrix predictors, malformed
  models and serialization under forced collection.
- Linked system LAPACK adapter contracts: 32/32 on macOS Accelerate.
- Targeted Miri: nested arena lend rejection, bytecode lookup during collection,
  and character-header initialization passed in this pass. The header test first
  reproduced an uninitialized true-length read, then passed after all four
  constructors initialized the full shared header. Earlier serialization under
  GC torture, nested on.exit/GC contexts and deep cyclic traversal also passed. Leak checking was disabled; these
  runs do not establish strict-provenance or whole-interpreter soundness.
- Actual Wasm execution: Node contracts and Chromium production UI/Worker
  evaluation, errors, nonlocal control flow, PNG and cancellation/reset passed.
- Production assets: Kotlin UI 374,563 bytes; Rust runtime 9,908,312 bytes, including bundled Noto Sans.
- Kotlin UniFFI generation matches the checked-in bindings.

Reproduce with cargo test --workspace, scripts/conformance_parity.sh --check
--strict (using the pinned oracle), scripts/wasm_m3_smoke.sh and the production
browser procedure in [web architecture](web-architecture.md). CI also checks
formatting, warnings-denied Clippy, the capability inventory and API boundary.

## What prevents a whole-port 9/10 claim

The [generated compatibility inventory](capability-evidence.md) records 633
curated fixtures and selected probes for seven packages. Of 70 tracked upstream
whole files, only one is marked passing; nine are expected failures and 60 are
skipped. These are coverage limitations, not percentages of the language.

The next acceptance bar is substantially broader upstream whole-file and real
package execution, an audit of remaining raw union access and ambient session
aliasing, complete graphics contracts, implemented statistical gaps such as
larger statistical workloads, and measured device/performance evidence. The issue tracker retains
these follow-ups. Additional translated code or a higher subjective score would
not substitute for those checks.
