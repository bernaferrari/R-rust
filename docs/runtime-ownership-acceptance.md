# Runtime ownership acceptance

This matrix records executable acceptance obligations for the current checked
heap. It is not a completion percentage or a proof that every internal raw
projection is safe. Beads holds the implementation work and unresolved issues;
the names below identify production regressions and bounded evidence.

## Identity and collection

| Obligation | Existing executable boundary | Evidence limits |
| --- | --- | --- |
| A stale identity cannot select a reused slot or recreated page | `sexp::heap::tests::{reused_slots_reject_stale_and_foreign_heap_ids,link_identity_rejects_recreated_page_ordinal_and_generation_reuse,generation_exhaustion_retires_storage_without_aliasing_old_ids}` | Checks the production heap identities and retirement logic; foreign native addresses remain an unsafe admission boundary. |
| Cloning and releasing roots preserves the exact allocation | `sexp::heap::tests::{automatic_root_clones_share_one_lease_and_last_drop_unroots,stale_automatic_root_drop_cannot_unroot_replacement_or_revive_closed_page,automatic_root_overflow_fails_without_changing_the_count}` | Root-count and identity properties do not prove every graph edge is published correctly. |
| Collection cannot invalidate an executing owned graph | `owned_s4_*`, `owned_task_callback_*`, `owned_eval_selected_expression_survives_detachment_and_full_gc`, `owning_closure_duplicate_retains_original_edges_across_detachment_and_gc` | These fixtures detach caches, source slots, or callback lists and collect without an incidental source root. All 27 new native regressions execute and pass. |
| Closure copying retains syntax without deep-copying its graph | `owning_closure_duplicate_shares_syntax_with_constant_allocation_cost` and the detachment fixture | Two native tests and two strict-provenance Miri tests pass with the default borrow checker. Node counts are constant for the tested closure bodies; this is not a whole-runtime performance benchmark. |
| Pairlist copying retains the original values before allocating | `mainutils::duplicate::owned_pairlist_tests::*` | Twelve native cases pass, including a genuine 32-cell detachment regression and exact 32/64-cell allocation counts. Ten strict-provenance Miri cases pass. Nested traversal still uses Rust recursion; vector/atomic snapshots and iterative continuations remain separate work. |
| Translated header reads retain canonical provenance with one admission | `sexp::accessors::projection_tests::owned_checked_header_*` | Five native and strict-provenance Miri cases pass for stale generations, foreign domains, retired storage and original singleton banks. The explicit debug microbenchmark improves lookup cost; both unchanged full methods-startup probes remain incomplete. |
| External-pointer duplication preserves the canonical object | `mainutils::duplicate::extptr_identity_tests::*` | Four native and strict-provenance Miri fixtures pass for deep/shallow identity, collecting children with a sole alias, mutation, explicit resource close, and last-alias release after shutdown. Copying allocates no node or child and invokes no provider. |
| Compiled execution owns its instructions and operands | Source/private/GNU bytecode replacement fixtures, including source-tree release and private-pool detachment | Prior targeted Miri checks pass. GNU CALL syntax constants are retained when its bytecode format requires them; private source-pool erasure tests do not imply GNU constants can all be discarded. |

## Reentry, transfers, and shutdown

| Obligation | Executable boundary | Evidence limits |
| --- | --- | --- |
| A callback cannot redirect cleanup into another runtime | S4 status-restoration and callback dropped-facade/revocation fixtures | Guards retain the original physical allocation and reject publication after revocation. Native fixtures and all seven S4, ten callback, and seven eval strict-provenance Miri regressions pass with the default borrow checker. |
| Direct native entry has an authenticated return-transfer scope | `owned_s4_native_validity_installs_and_releases_transfer_scope`, `owned_task_callback_direct_managed_entry_supports_return_and_on_exit`, `owned_eval_native_direct_entry_installs_and_releases_original_transfer_scope` | Native checks execute real return/on-exit paths and verify scope cleanup. They do not authenticate arbitrary foreign function signatures. |
| Eval consumes only its own return ticket | `owned_eval_return_is_local_to_exact_eval_context`, `owned_eval_preserves_unmatched_original_return_ticket_after_full_gc` | Native checks distinguish a local eval return from an unmatched original transfer. |
| A live owned value has a deliberate relationship with closure | `sexp::owner::tests::{owned_value_preserves_original_graph_after_runtime_close_and_drop_without_cycle,owned_lazy_value_rejects_revoked_provider_before_callback,owned_lazy_callback_can_close_and_drop_runtime_but_cannot_publish_success}` | Pure owned storage can survive runtime closure; runtime-dependent providers must reject a revoked owner. This is different from a host handle remaining usable after its session closes. |
| Resource destruction can reenter only after arena loans end | `sexp::gengc::tests::collected_node_resources_reenter_only_after_collection_and_arena_lends_end` | Does not establish every external resource's construction, explicit close, collection, and shutdown matrix. STARMA has a typed canonical resource boundary; its full lifecycle acceptance remains required. |
| A host handle retains its value privately and rejects reused identities | `owned_retained_*` store regressions | Ten native and strict-provenance Miri store fixtures pass for binding interference, collection, failed publication/writes, foreign identities, generation retirement, sole closure roots, and closure. The separate full embedding suite still requires default package startup. |
| Native invocation uses its actual registered callable signature | `mainutils::native_routines::tests::*`, `mainutils::dotcode::typed_native_handler_tests::*`, `native_routines::buffers::*`, `dotcode::buffer_dispatch::tests::*` | Thirteen Call/External native and strict-provenance Miri fixtures pass. Nine additional native and Miri cases verify checked, independently owned numerical buffers, original lookup ownership, rejected admission and promoted-result attribute barriers. Eighteen of the 26 captured bundled C/Fortran registrations have checked adapters; eight remain unsupported. Matching registration metadata does not prove handler semantics. Foreign libraries remain an unsafe boundary. |
| GC preambles release their exact temporary ownership on unwind | `owned_gc_*` | Nine native and strict-provenance Miri tests pass for full/lite GC, allocation torture, eval safe points, detached bindings, callback closure, and panic cleanup. The Miri run also verifies the original-runtime capture sole-pin fixture. |
| Captured output preserves emission and original-owner cleanup | `exact_console_capture_*`, `exact_top_level_emission_*`, `owned_output_capture_*`, four `owned_retained_console_*`, four `focused_console_*`, and three `public_capture_*` fixtures | Independent GNU fixtures verify fourteen stdout cases. Native checks cover stream order, custom-print errors, active bindings, revoked printing, later-call rejection, and live panic payloads. Four focused interpreter fixtures and three public host-callback fixtures pass strict-provenance Miri. The public capture scope restores its parent or idle bank after a panic, preserves the panic payload, and cleans its original bank after revocation and reentry. |

Unreachable-cycle coverage and retained-memory measurements must include
environments, closures, promises, and external resources together. The current
node counts and root-release assertions do not measure resident memory after
retaining one small value, allocating a large temporary graph, closing the
session, and releasing the final value. Broad runtime borrowing work remains
tracked in `rport-sg9a`; the acceptance matrix must not substitute for that work.

## Verification checkpoint

The pushed production checkpoint `038fd34a` passes 704 selected native tests, including
the two closure-copy regressions, seven S4, ten task callback, eight eval, and
41 new native-dispatch, retained-value, GC, and exact-output regressions. A
separate run of those 41 new cases passes with no failures or ignored tests in
66.37 seconds; the full selection finishes in 257.51 seconds. Clippy's
correctness gate and native embedding/all-target and Wasm Rust-backend checks
pass. Existing compiler warnings remain; this is not a warnings-free gate.

The initial typed native dispatch group additionally passes all eight
strict-provenance Miri tests in 629.10 seconds, with the default alias checker.
The focused GC/output-capture run passes ten cases in 674.85 seconds: all nine
GC fixtures and the exact original-runtime capture sole-pin fixture. The full-base
fourteen-case retained/console and eleven-case GC/capture runs were stopped
during base bootstrap and do not establish completed Miri passes. Their original
native fixtures remain unchanged. A separate ten-case retained-store Miri run
passes in 1235.19 seconds against production `038fd34a`.

The pushed development checkpoint `e92f38ae` passes thirteen native dispatch/admission cases,
four external-pointer identity/lifetime cases, and four additional focused
console cases. Its combined seventeen-case native-dispatch and external-pointer
Miri run passes in 1393.64 seconds; the four focused console cases pass Miri
separately in 1098.11 seconds. All selections finish with zero failures and zero
ignored tests, the default alias checker, strict provenance, and only leak
checking disabled. The corresponding production changes are in `0af869f6`.

Checkpoint `15f4c22f` additionally makes the public `with_output_capture` method
use the existing original-owner scope guard. Three genuine native regressions
fail before the repair and pass afterward; the same three pass strict-provenance
Miri in 328.79 seconds with zero failures and ignored tests and the default alias
checker. The fixtures distinguish the parent, idle, original, and replacement
banks and preserve exact bytes and typed panic payloads. This does not add another
capture implementation or change the presentation contract.

The runtime milestones merged in `3c800eaa` additionally pass the warnings-free
default-feature rmath/all-target Clippy gate in 25.95 seconds and the
workspace/all-target gate in 27.48 seconds. Sixty-three affected managed native
controls pass in 2.71 seconds. The pairlist milestone `4f622efd` passes twelve
native cases in 0.06 seconds; ten original fixtures pass strict-provenance Miri
in 869.62 seconds. The additional 32/64-cell size and allocation cases are native
evidence, rather than part of that ten-case Miri selection. Both the original
three-cell and larger 32-cell regressions fail against the original implementation.

The single-admission header milestone `b9423327` passes five native and five
strict-provenance Miri cases, the latter in 130.56 seconds. Using the same debug
settings, the explicit four-read benchmark has median costs of 2003.86 ns for
same-page nodes, 2264.71 ns for different-page nodes and 327.10 ns for singleton
reads, versus 4046.84, 4268.56 and 432.94 ns before the change. These are bounded
lookup measurements, not a claim about full package startup or resident memory.

The numerical adapter milestone `b672a5ae` passes nine native cases in 0.04
seconds and the same nine strict-provenance Miri cases in 786.14 seconds.
Production remains unchanged between those runs. One equivalent test-only
`find_map` cleanup follows Miri compilation; the compiled source manifest and
production hashes record that distinction. The promoted-result regression
executes real allocation-triggered GC followed by minor collection, without an
incidental old-to-young output edge hiding a missing attribute barrier. All
three new Miri selections use the default alias checker and strict provenance,
with only leak checking disabled.

CI for `038fd34a` completed with failures after successfully building the browser
bundle and executing the real Rust runtime tests. Workspace formatting, Clippy,
embedding expectations, Wasm warnings, and showcase artifact preparation failed;
the conformance job was cancelled and supplies no completed parity checkpoint.
The development checkpoint repairs stale automatic-print expectations and
showcase artifact preparation. All 158 affected embedding tests compile, but
execution was stopped after 646 seconds during unchanged default methods
startup, without a completed test footer. Full embedding execution remains
pending. Frontend checks and the nmath warnings-free Clippy gate pass; they do
not establish warnings-free workspace validation.

Earlier owning-field checkpoints have additional completed evidence:
All 26 selected strict-provenance Miri tests pass with the default borrow
checker: closure copying (two tests, 175.53 seconds, production `40abc01a`), S4
(seven tests, 476.49 seconds, production `1ce740a3`), task callbacks (ten tests,
1359.04 seconds, production `1ce740a3`), and eval (seven tests, 1215.85 seconds,
production `5d4d530d`). The GNU serialized eval fixture is verified natively
and is not part of that Miri selection. These production changes are included
in the pushed checkpoint `1a5215b2`. Miri uses `-Zmiri-strict-provenance` and
`-Zmiri-ignore-leaks`; its alias checker remains enabled.

The earlier unmodified full default methods-startup probe timed out at 180 seconds.
An isolated real stats namespace test completes and retains its S3 method table
through collection; this does not certify full methods startup. The upstream
ledger still has 39 marked-passing and 31 skipped whole drivers. A selected
native checkpoint is not a completed workspace, browser, mobile, or whole GNU R
conformance run. The new production conformance harness preserves runner bytes,
reports each active case and bounds each owned subprocess. A real pinned-oracle
smoke run builds in 39.50 seconds, completes GNU `001_arithmetic`, then reports
the Rust runner's 180-second timeout. Its report records one failure, 1,180
unattempted cases and `execution_complete=false` out of 1,181 captured cases.
This verifies the harness's incomplete-run contract; it is not a parity pass.
Workspace formatting remains a separate mechanical checkpoint.

Kani should target production identity, generation, workspace, and typed-native
admission helpers. Miri and collecting integration tests remain necessary for
aliasing and reentry. No new Kani proof is claimed by this checkpoint.

The native lookup snapshot repair passes 24 native controls in 0.09 seconds and
all six focused strict-provenance Miri cases in 560.11 seconds, with zero
failures or ignored tests and the default alias checker. Its fixtures detach the
original lookup child or argument spine, perform full GC and genuine nested
native invocation, and reject publication after original-runtime revocation.
The operation retains every payload before list-name providers and the selected
lookup child before PACKAGE providers. The Miri proof belongs to its recorded
compiled-source manifest; the independent shared-environment index repair was
edited after that compilation and is not certified by this selection.

The linked numerical profile now uses the existing Rust `lminfl` implementation
for GNU's stats-specific influence routine. The original system-library profile
fails at link time because BLAS/LAPACK do not supply that stats symbol. After the
repair, all six influence cases and 35 LAPACK controls pass in both the system
Accelerate profile (41 tests, 0.04 seconds) and the default Rust profile (41 tests,
0.02 seconds). The directly constructed adapter fixture uses a genuine managed
heap; unchanged full default package startup remains a separate incomplete
obligation. The numerical algorithm and system BLAS/LAPACK selection are unchanged.

The parsed-function deparser now reads the actual pairlist formals field. GNU's
C closure accessor also accepts that pairlist layout; the checked Rust graph
correctly rejects a closure-field projection from a list. That rejection was
overwriting the original uncompiled-function error during call rendering.
Eleven native controls pass in 1.80 seconds, and four strict-provenance Miri
deparser controls pass in 590.20 seconds. The separate scalar error matches the
pinned GNU wording, `argument is not a byte code object`. The public embedding
test compiles, and warnings-free rmath/r-embed all-target Clippy passes in 44.95
seconds; the unchanged public default-startup path is not certified by those
focused checks. Each proof remains tied to its compiled-source ledger.

The Holt-Winters native filtering kernel and checked adapter both forbid unsafe
code. They use disjoint Rust slices and validate workspace extents before writing
outputs; the old internal pointer kernel is removed, and the existing fitting
helper shares the same filtering implementation. Nine independent pinned GNU
cases cover 153 argument records, including exact unchanged inputs and output
tails. Computed floating values use a documented 64-epsilon relative tolerance
for portable recurrence rounding. The checked .C path preserves aliased sources,
NAOK admission, control separation and owned results after full collection.
Eight native cases pass in 0.01 seconds, nine enclosing native controls pass in
0.01 seconds, and all eight strict-provenance Miri cases pass in 380.05 seconds.
Warnings-free default rmath/all-target Clippy passes in 27.84 seconds. The Miri
proof records independent later compiler/duplication edits separately; all owned
numeric source hashes remain unchanged. This covers the native filtering adapter,
not every option of the higher-level fitting API or the eight other missing
numerical registrations.

Dense vector and atomic duplication now retains the actual original children,
attribute cells and scalar buffers before providers or allocation callbacks.
Initialized checked destinations and their source owners survive the sole outer
trace callback; original-runtime revocation rejects publication. Mutable logical
singleton copies match the pinned GNU behavior. All 24 native controls pass in
0.12 seconds, and all 12 strict-provenance Miri cases pass in 942.49 seconds with
memory profiling enabled and the default alias checker. Independent pinned GNU
checks pass for 16 type/deep-shallow combinations and six singleton cases; an
independent ownership review found no introduced lifetime defect. The Miri proof
records the frozen duplication hashes and independent environment-index edits
separately. Deep CAR, attribute and nested-vector traversal remains recursive
and has a separate iterative-traversal acceptance task.

Environment binding indexes are now canonical proofs keyed by the exact frame
allocation generation. Shared base/namespace aliases use direct lookup without
scanning other indexed environments. Promotion permission is nonowning; proof
retention follows the live frame, including release of its former environment,
head insertion, mutation invalidation and slot reuse. Active-binding results
retain their actual owning handle. All 29 native tests pass in 4.33 seconds,
including both unchanged full-base fixtures; all 11 strict-provenance Miri cases
pass in 879.03 seconds with the default alias checker. The paired 512-index
small-frame diagnostic improves from 29.34 to 8.84 microseconds, restoring the
nearly flat original small-frame cost. Original GNU metadata source/bytecode
checks retain their attributes and pass. Unchanged full startup still reaches
the 180-second bound without a completed test footer. Its samples identify
nonmoving collection's graph-remapping phase as the next measured obligation.

The C/Fortran structured list-name seam now retains every original payload,
tag and attribute before executing either name-provider operation. Capturing
the selected lookup child is followed immediately by an original-runtime check.
Four genuine collecting/revoking ALTREP fixtures cover the actual kmeans C and
bvalus Fortran handlers, without incidental roots. All 12 native buffer-dispatch
controls pass in 0.06 seconds, and all four focused strict-provenance Miri cases
pass in 364.79 seconds with the default alias checker. Pinned GNU neighbors
match the expected numeric outputs; warnings-free all-target Clippy passes in
29.95 seconds. Independent later duplication and collector edits remain outside
the loaded proof's frozen source ledger.
