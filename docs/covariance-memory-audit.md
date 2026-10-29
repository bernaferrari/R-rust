# Covariance workspace and runtime admission

The covariance parity change routes `C_cov` through the existing `C_cor`
implementation in `crates/rmath/src/library/stats/correlation.rs`. It fixes
missing-row policy, shape, dimension validation and Kendall results. The
change also admits the shared numerical workspace, addressing **rport-2izvo**.
This is a bounded accounting contract for these buffers, not a claim of total
engine memory safety or a process-RSS ceiling.

## Allocation accounting

| Storage | Admission |
| --- | --- |
| Original R arguments and any real-vector coercions | Managed R heap; ordinary arena byte/node budgets apply. |
| Published R result, dimensions and copied dimnames | Managed R heap; allocation can collect/retry and report a bounded allocation error on denial. |
| Copied `Columns::values` for x and non-NULL y | Fallible fixed `Workspace<f64>`, charged to the original session ledger. |
| Complete-row flags and reused per-pair row indices | Fixed charged workspaces; row-index storage is reused across column pairs. |
| Numerical result before R publication | Fixed charged workspace whose ticket remains live while the R result and metadata are allocated. |

Existing correlation previously used uncharged Rust vectors for all numerical
scratch. Covariance and variance previously read raw spans; sharing the engine
adds an O(input-size) copied-column cost. NULL y reuses x's copied columns.
Explicit y receives a separate copy even when it names the same R object.
Those copies are now admitted alongside the existing R arguments rather than
bypassing the session limit.

For actual capacities Cx, Cy, Cflags, Crows and Cresult, numerical scratch is
charged as `8*(Cx + Cy + Cresult) + Cflags + sizeof(usize)*Crows`, using the
actual element widths on the target. This excludes allocator overhead and
stack/local control metadata, consistent with the existing workspace ledger.
During publication the column copies and numerical result remain charged
alongside the new R result. Complete-row flags and row indices have already
been released when numerical computation returns.

`correlation/workspace.rs` reserves through the original owner's
`RArena::reserve_workspace` before allocation, uses `try_reserve_exact`, checks
byte arithmetic and reconciles actual Vec capacity before initialization.
An oversized allocator result or allocation failure returns a recoverable
workspace error. Each holder declares its Vec before its reservation field,
so actual storage drops before admission credit is released. No growth or
replacement allocation is needed. Tickets retain their original ledger and
release against it even after foreign activation or facade teardown.

The focused tests cover admission before allocation, surplus-capacity denial,
allocator failure, unwind cleanup, native cov/cor denial and recovery, overlap
with R-result admission, owner revocation during allocation, original-ledger
release after facade teardown, and forced-GC publication for both statistics.
Public covariance tests and the existing correlation suites provide numerical
and shape evidence under native and empty-runtime-path policies. Test results
are recorded in the associated beads; this document does not substitute for
those execution receipts.

## GC and owner revocation

The typed `.Call` dispatcher uses `with_runtime` in `mainutils/dotcode.rs`,
retaining an operation-local strong owner pin. It owns native arguments and
rechecks activity before invocation and before wrapping the result;
`with_runtime` also rechecks after the operation. Successful publication after
owner revocation is rejected while physical storage remains pinned for cleanup.

The engine protects both coerced R inputs and snapshots numeric elements
through checked handles. Copy completion and workspace allocation recheck the
original owner. No borrowed R payload span survives result allocation or
warning delivery. Scratch contains copied numbers/indices, so R collection
does not invalidate it. The output is protected before allocating dimensions
and dimnames. Checked lazy-element callbacks retain their ALTREP activation
and liveness checks. No new owner authority is created from an address.

## Limits of this change

Numerical loops retain their previous cancellation behavior. In particular,
Kendall's O(n^2) loop has no new cancellation/depth checkpoint and can delay
interruption or the next managed activity check. No speculative change to the
GNU operation's cancellation semantics is included. Accounting these numerical
buffers does not establish admission for unrelated dispatcher metadata,
allocator overhead, or every allocation elsewhere in the engine. No total
memory safety guarantee, RSS ceiling, or new prompt-cancellation contract is
claimed.
