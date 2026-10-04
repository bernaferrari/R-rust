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
| Native invocation uses its actual registered callable signature | `mainutils::native_routines::tests::*`, `mainutils::dotcode::typed_native_handler_tests::*`, `native_routines::buffers::*`, `dotcode::buffer_dispatch::tests::*` | Thirteen Call/External native and strict-provenance Miri fixtures pass. Nine additional native and Miri cases verify checked, independently owned numerical buffers, original lookup ownership, rejected admission and promoted-result attribute barriers. Twenty of the 26 captured bundled C/Fortran registrations have checked adapters; six remain unsupported. Burg and STL acceptance evidence appears below. Matching registration metadata does not prove handler semantics. Foreign libraries remain an unsafe boundary. |
| GC preambles release their exact temporary ownership on unwind | `owned_gc_*` | Nine native and strict-provenance Miri tests pass for full/lite GC, allocation torture, eval safe points, detached bindings, callback closure, and panic cleanup. The Miri run also verifies the original-runtime capture sole-pin fixture. |
| Captured output preserves emission and original-owner cleanup | `exact_console_capture_*`, `exact_top_level_emission_*`, `owned_output_capture_*`, four `owned_retained_console_*`, four `focused_console_*`, and three `public_capture_*` fixtures | Independent GNU fixtures verify fourteen stdout cases. Native checks cover stream order, custom-print errors, active bindings, revoked printing, later-call rejection, and live panic payloads. Four focused interpreter fixtures and three public host-callback fixtures pass strict-provenance Miri. The public capture scope restores its parent or idle bank after a panic, preserves the panic payload, and cleans its original bank after revocation and reentry. |

Unreachable-cycle coverage must include environments, closures, promises, and
external resources together. The targeted retention acceptance below measures
accounted bytes, physical backing release, and process resident memory while
retaining one small value and collecting a large temporary graph. It does not
establish reclamation of every mixed resource cycle or immediate return of
resident pages to the operating system. Broad runtime borrowing work remains
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

The immutable fd015216 CI run completed without a green checkpoint: workspace
execution failed at the compiler diagnostic, linked numerical validation failed
on the stats-only lminfl symbol, and exact parity reported a 300-second Rust
startup timeout. The first two failures are repaired in subsequent verified
units above. Format, Rust WASM execution and actual Rust browser-workbench
execution pass. The separate showcase job exceeded its unchanged 35-minute
budget; no cancelled-job logs were available to attribute its combined phase.
The CI repair splits that phase into six observable commands using the existing
owned-subprocess deadline helper, explicit Bash pipeline failure propagation,
phase logs and timeout markers, with unconditional evidence upload. Four helper
tests and a deliberate command-exit-seven pipeline control pass locally; the
real showcase job still requires a completed new checkpoint.

Nonmoving sweep cleanup now uses a callback-free exact-link map. It retains
only changed reference positions, and checks bounds, immutability and replacement
identity before publishing a header or payload. This also repairs a genuine
generic-visitor transaction failure: rejecting immutable cells previously left
the attribute changed. The general visitor keeps its detached snapshot and
reentry validation. A separate sweep-only path skips marked strong graphs after
complete tracing and ready-finalizer retention; it still clears marked weak keys
and all relevant edges of unmarked partial-collection survivors. All 14 focused
native tests pass in 0.02 seconds, 14 enclosing collection/unwind/resource
controls pass in 0.04 seconds, and eight strict-provenance Miri cases pass in
529.52 seconds with the default alias checker. Warnings-free all-target Clippy
with memory profiling passes in 26.37 seconds; frozen collector hashes and
independent later source edits are recorded separately.

The unchanged paired 2,000-vector diagnostic asserts every collection count,
retained value and retired generation. The extra one-node-sweep cost falls from
20.31 to 0.77 milliseconds; total measured collection time falls from 29.06 to
14.36 milliseconds. No-sweep timing also varies, so this is a bounded diagnostic,
not a universal speedup claim. Unchanged default startup still times out at
180.26 seconds without a test footer. Its new sample contains marking and
deserialization work, with no sweep-remapping frames. Completion of startup and
whole GNU parity remains a separate obligation.

Deep duplication now uses fallibly reserved, owning continuation frames for
vector children, CAR values and attributes. Frames retain the original runtime
authority, saved sources and initialized destinations; active-path membership
is removed on completion, preserving independent copies of repeated siblings
and cycle rejection. The unchanged recursive implementation overflows on the
valid 32,768-layer mixed graph; the iterative implementation completes that
graph on an explicit 2 MiB stack with iterative output validation. All 29 native
cases pass in 2.01 seconds, including the original 24 ownership controls. Four
focused strict-provenance Miri cases pass in 442.21 seconds with memory profiling
and the default alias checker; the native subprocess is excluded by target,
not counted as an ignored Miri pass. Pinned GNU mixed-depth occurrence checks,
independent ownership review and formatting pass. Warnings-free all-target
Clippy with memory profiling passes in 45.19 seconds. All assigned source hashes
remain frozen throughout those checks.

Checked tracing now marks exact allocation identities at worklist admission,
after heap, generation and projection validation. Repeated incoming references
enqueue one task; the regression previously enqueued 3,000 tasks for one node.
Pop-time validation still rejects retirement, physical page destruction and
address reuse. New tasks use fallible reservation; a tracing error aborts before
sweeping, and production worklists drain synchronously before graph cleanup.
All 11 native admission cases pass in 0.01 seconds and all 11 strict-provenance
Miri cases pass in 105.25 seconds. Eight collector transaction, weak-key,
partial-sweep, finalizer and resource-destruction controls pass natively in
0.01 seconds and under strict Miri in 376.41 seconds, with the default alias
checker. Warnings-free all-target Clippy with memory profiling passes in
29.86 seconds. The frozen admission source and later independent compiler
edits have separate loaded-source evidence.

Independent review confirms that all production worklists complete before
sweeping. The unchanged base graph diagnostic validates every managed header,
62,744 live nodes and 13,377 roots. Minor collection's median changes from
143.659 to 143.046 milliseconds, which does not establish a material speedup.
This milestone bounds duplicate worklist storage and preserves checked
collection behavior; it does not resolve full methods startup.

The multivariate Burg native routine now executes through checked, typed buffers
and an owned Rust matrix/Householder QR implementation. Both numerical modules
and the buffer adapter forbid unsafe code; the obsolete raw Burg route is
removed. Workspace arithmetic and fallible allocations precede computation,
and all eleven buffers publish only after fitting succeeds. Eight independent
pinned GNU cases compare every buffer, including residuals, coefficient and
partial-correlation grids, variance, AIC, selected order and untouched tails.
A separate genuine GNU order-zero case accepts the unread empty variance-method
buffer. The initial adapter correctly rejected that case, and conditional
admission now preserves GNU's branch-sensitive input contract.

All seven native Burg cases pass in 0.02 seconds, all thirteen neighboring buffer
controls pass, and all seven strict-provenance Miri cases pass in 376.02 seconds
with the default alias checker. Warnings-free all-target Clippy passes in
27.05 seconds, and formatting passes. The corrected seven-case proof is tied to
its frozen source manifest; the superseded six-case candidate is retained as
earlier evidence. The structured numerical registry covers 19 of its 26 declared
C/Fortran entries; that count does not claim complete stats or GNU R parity.

Holt-Winters buffer admission now follows the actual executed GNU branches.
Initialization-only calls accept unread empty data, coefficients and SSE;
enabled trend and seasonal initialization still require their real inputs.
Calls with updates require the previous trend values even when trend updates
are disabled, preserving GNU's level recurrence. The safe kernel receives the
actual caller's SSE slice and validates its length before any output write.
Three new independent GNU branch cases reproduce all 51 argument-buffer rows,
with a durable generator and byte-identical fixture regeneration. All eleven
native cases pass in 0.02 seconds, fifteen neighboring buffer controls pass in
0.09 seconds, and all eleven strict-provenance Miri cases pass in 399.41 seconds
with the default alias checker. Warnings-free all-target Clippy passes in
30.95 seconds; assigned source hashes and formatting remain verified.

GNU's serialized base-namespace token now restores the distinct original base
namespace, preserving its identity instead of substituting the base environment.
Three native original-byte tests pass in 1.63 seconds, including the genuine
full-base bootstrap and collection. Two exact managed strict-provenance Miri
tests pass in 68.82 and 111.73 seconds: the four special environment tokens and
two original session domains. These Miri fixtures use explicitly declared
minimal heaps; they do not certify full methods startup. The one-branch reader
repair and frozen original-byte tests have separate source-ledger evidence.

Scalar admission now checks both the requested GNU SEXPTYPE and the scalar
header flag. The public helper is safe Rust: it reads an authenticated owning
header snapshot, without dereferencing the supplied projection. The original
implementation admits a scalar under a different requested type; that exact
regression fails before the repair. Three native cases pass in 0.01 seconds and
three strict-provenance Miri cases pass in 119.86 seconds with the default alias
checker. They cover every supported vector kind against requested types -1
through 31, both scalar flags, unregistered and retired storage, and an original
immutable logical singleton retained across bank retirement. Existing explicit
caller type checks remain. The pinned GNU macro and frozen source hashes are
recorded independently; this establishes the helper's checked contract, not
whole-engine safety. Workspace all-target Clippy passes with warnings denied in
31.06 seconds and workspace formatting passes for the current development tree.

Flat compiled `[<-` and `[[<-` now use the existing owning replacement
executor. They evaluate the RHS first, preserve its exact returned identity and
GNU invisibility, and retain raw index syntax for lazy custom setters. Typed
scalar fast paths explicitly require real RHS storage. Bytecode execution keeps
the original runtime pin through unwinding and rechecks its authority before
publishing either a result or an error; live callback panic payloads propagate
unchanged, while revoked original runtimes reject foreign replacement authority.

Twenty-five compiler and twenty-four executor native cases pass. Six focused
strict-provenance Miri cases pass in 2,086.95 seconds with the default alias
checker, including collecting RHS/index callbacks, lazy setters, error recovery,
panic identity and original-runtime revocation. Five independent pinned GNU
serialized fixtures execute without retaining the source pool or an interpreter
fallback. The Miri ledger records its original loaded accessor and separate
later scalar, numerical and serialization changes. Warnings-denied all-target
Clippy with memory profiling passes in 25.29 seconds. A subsequent comment-only
clarification has an explicit hash delta; executable behavior stays frozen.
Nested replacement targets, superassignment and full methods parity remain
separate acceptance obligations.

Public compiler tests now follow the independently executed GNU contract:
user-function calls compile and execute, original closures retain their source,
builtins retain identity, and accepted options do not cause artificial errors.
The durable GNU script also checks non-function rejection, named/positional
matching and duplicate arguments. That same script passes through the Rust
compiler namespace in the real base runtime. Both focused native cases pass
in 2.46 seconds; all-target warnings-denied Clippy with memory profiling passes
in 24.65 seconds and assigned formatting passes. Production compiler behavior
stays unchanged; the portable optimization strategy still needs broader parity.
The original full-default embedding constructor tests remain intact and did
not reach a footer within their recorded local bounds. These base-runtime
contract tests do not certify completion of full default-package startup.

STL now uses a checked seventeen-buffer Fortran descriptor and a fallible Rust
kernel. The old raw adapter is removed; the kernel, mathematical helpers and
new buffer adapter forbid unsafe code. Workspace bounds, executed jump/period
requirements and branch-sensitive unread inputs are admitted before output
writes. Robust fitting reuses disjoint scratch storage. Nine independent GNU
cases compare all 153 argument buffers, including normalization, zero/negative
iterations, robust fitting, minimal valid periods and untouched tails. Durable
fixture regeneration is byte-identical; empty trailing columns are retained.

Eight native cases pass in 0.03 seconds, twenty neighboring buffer cases pass
in 0.11 seconds, and eight strict-provenance Miri cases pass in 930.51 seconds
with the default alias checker. Actual managed Fortran calls cover collecting
names, aliased inputs with independent outputs, original-session closure and
wrong-package refusal. Warnings-denied all-target Clippy passes in 24.63 seconds
and assigned formatting passes. Frozen source and interpreter ledgers are
recorded separately. Structured admission covers 20 of the 26 declared stats
C/Fortran entries; this does not establish complete numerical API parity.

Compiled replacement now handles a symbol or one dollar-field path through
typed continuation operands, including assignment into an enclosing environment.
The executor owns the selected root, child, original RHS and deferred argument
syntax across getters and both setters. It preserves GNU evaluation order,
custom `substitute` behavior, exact invisible RHS identity and enclosing-scope
lookup without exposing a temporary compiler binding. Malformed operands fail
before any getter executes, and original-runtime revocation denies publication.

Thirty-three compiler and twenty-four executor native cases pass in 27.39 and
32.88 seconds. Four focused strict-provenance Miri cases pass in 1,118.32 seconds
with the default alias checker, covering detached constant pools, collection,
error and panic recovery, revocation and malformed paths. Two independent GNU
serialized closures execute after the source tree and fallback pool are removed.
Warnings-denied all-target Clippy passes in 23.74 seconds, and assigned formatting
passes. Frozen owned sources and the interpreter ledger distinguish subsequent
independent numerical and methods edits. General replacement chains and full
methods startup remain separate obligations.

Promise forcing now retains the original promise, expression, environment and
returned value through evaluation and context cleanup. Fresh and cached force
results carry the same owning identity. Publication uses the original runtime
authority; a foreign or closed runtime cannot replace it. Live callback panics
preserve their exact payload, while original-runtime revocation returns a checked
failure before caching a result.

The baseline fails four of five focused ownership regressions. All six final
native cases pass in 0.03 seconds, including sole-result-root collection, and
all six strict-provenance Miri cases pass in 436.62 seconds with the default
alias checker. Warnings-denied all-target Clippy passes in 25.06 seconds and
assigned formatting passes. The recorded frozen source and interpreter manifest
define this proof's scope. Recursive evaluation and interrupted-promise restart
states remain a separate tracked obligation; full methods startup is unresolved.

Heap retention now has exact byte and physical-owner acceptance checks. A small
owning real value survives while a discarded 257-node graph releases its 64 MiB
real payload during full GC. Only 30,968 accounted bytes remain, including
reusable headers; dropping the session retains the small value's physical
backing, and releasing its last handle destroys that backing and balances the
arena's byte ledger to zero. Eight repeated sessions release every backing.
Cloned handles share one root lease; independently wrapped handles release all
their additional roots without leaving collector nodes alive.

All three native cases pass in 0.37 seconds. Three strict-provenance Miri cases
pass in 497.31 seconds with the default alias checker and the same ownership
transitions on smaller buffers. Warnings-denied all-target Clippy passes in
27.14 seconds and assigned formatting passes. The owned fixtures forbid unsafe
code; frozen source and interpreter ledgers distinguish later compiler changes.
These fixtures use the minimal managed collector profile, not default package
startup. Local measurements report a 120-byte node, a 144-byte handle, and
median times of 0.914 ms for 10,000 clones and 5.898 ms for 10,000 independent
wraps. There is no previous-version or GNU performance baseline. Observed
process RSS remains about 89 MiB after release, so exact heap reclamation does
not establish immediate return of those resident pages to the operating system.

Promise forcing now admits evaluation through typed states over the canonical
full `gp` field. An owning guard marks ordinary interrupted evaluation and
releases its root on unwind. Recursive forcing reports GNU's specific error
with the original call. Restart-warning handlers already observe evaluating
state; if that warning aborts, including conversion to an error, the promise
remains evaluating as independently verified in GNU R. Successful publication
authenticates the original live domain and every edge, then updates sharedness,
the cached value, environment and idle state through checked header storage.
The state module and its minimal managed lifecycle fixtures forbid unsafe code.

Warnings, recursive-error signaling and evaluation share one original-owner
unwind check. A live callback preserves its exact panic payload; revocation
returns a checked failed force. The bounded recursive/retry baseline fails all
three GNU contracts, and the actual restart-warning GC baseline fails its
revoked-owner case. The durable GNU script independently asserts error calls,
retry warnings, caching, warning-handler reentry and warning-to-error behavior.

Twenty focused native cases pass, including the complete public GNU script
through real base startup. Fourteen strict-provenance Miri cases pass with the
default alias checker and only leak checking disabled: six state lifecycle
cases in 382.86 seconds, two actual warning-GC cases in 110.17 seconds, and six
owning evaluation controls in 397.41 seconds. The first six exercise the
unchanged state helper before the separately verified callback-unwind repair;
the recorded source ledgers retain that distinction. Public base fixtures are
native evidence, while the Miri fixtures use the minimal managed profile.
All-target warnings-denied Clippy and assigned formatting pass. Full default
methods startup remains unresolved, and nested source temporary-binding
cleanup is tracked separately in `rport-hah9u.32.5`.

ALTREP storage now has a private generation-qualified vector edge, independent
of public attributes. Both compact sequence producers use the canonical
SequenceClass. Public attribute replacement, duplication and dense materialization
preserve class and state identity; materialized writes take precedence over
formula reads. A sealed built-in expansion can initialize its existing parent
payload during an arena lend without callbacks or a second vector header.
Generic providers retain their separate callback admission requirements.

Passive sequence reads authenticate an immutable primitive permit on the actual
class allocation, then validate the complete formula and cache. A same-heap
private-edge remap to a generic provider genuinely failed before this repair.
Foreign, retired, reused, wrongly typed or mismatched class identities cannot
grant the permit. The permit contains no provider, runtime or owning R value,
and external-pointer erased resource APIs remain unchanged.

The final candidate passes 54 selected native cases, six strict-provenance Miri
boundary cases in 643.51 seconds, warnings-denied Clippy in 27.53 seconds, and
formatting for all 28 assigned files. Ten earlier canonical-storage Miri cases
pass in 699.09 seconds on their explicitly recorded predecessor; they are not
relabelled as the final class-permit snapshot. Twenty STARMA and three retention
native controls also pass after private storage migration. GNU serialized compact
vectors currently reload as dense values; the fixture verifies values and the
first public attribute, without claiming lazy class preservation on reload.

Methods implicit-generics setup now invokes the installed original helper in
the methods namespace without eagerly loading stats, tools or utils. The base
namespace no longer contains an invented toeplitz wrapper. Five native fixtures
verify the original table and closure identities, explicit later stats loading,
collection, revocation and exact integer/asymmetric real toeplitz results.
Two earlier minimal parser/ownership Miri fixtures cover their stated input
lifetimes and revocation only; they do not execute real methods registration.
The unchanged full default startup still reaches no footer within its recorded
180-second bound. This fixes eager registration and sequence behavior while the
broader package-loading frontier remains tracked separately.

Compiled replacement continuations now handle arbitrary valid symbol getter
and setter heads across deeper paths. Explicit typed operands own the root,
each selected child, deferred arguments, cached values and their original
expressions. Getters and setters receive fresh argument promises in GNU order;
the first replacement RHS retains its source expression and later outward
setters observe `*vtmp*`. The interpreted nested replacement call builder now
retains that same cached-value/code distinction through allocation and collection.

All 46 compiler, 24 executor and ten source-assignment native cases pass,
including thirteen new replacement fixtures. Five focused strict-provenance
Miri cases pass in 1,694.46 seconds with the default alias checker. The independent
GNU generator supplies four serialized closures that execute after source-tree
release; private execution also removes its fallback pool. Malformed operands,
cyclic syntax, collecting callbacks, error/panic recovery and revocation have
explicit coverage. Clippy and formatting pass. The proof ledger distinguishes
later independent class-permit changes. Qualified replacement heads and nested
source temporary-binding cleanup remain separate tracked work.

Supersmoother now uses a checked ten-buffer descriptor and a slice-based Rust
kernel for upstream's default running-linear method. The kernel and buffer
adapter forbid unsafe code. Public calls retain all actual inputs before
provider callbacks, apply exact/partial/positional argument matching, filter and
sort numerical inputs, and authenticate the original runtime before results
are published. Eleven independent GNU cases compare all 110 argument buffers;
public fixtures cover fixed and cross-validated spans and unchanged tails.

Thirteen native cases, twenty distinct neighboring buffer cases and all thirteen
strict-provenance Miri cases pass. The full Miri selection completes in 1,723.65
seconds with the default alias checker. A genuine intermediate CV bandwidth
difference is isolated in the pure kernel and repaired by replacing two
`powi(2)` calls with upstream-order multiplication, retaining the original tie
rules and tolerance. Warnings-denied Clippy passes in 23.65 seconds and formatting
passes. Structured admission now covers 21 of the 26 captured stats C/Fortran
registrations; this count does not certify foreign ABI signatures or complete
semantics. PPR spline/tracing settings and character-span coercion remain tracked
separately.

## ALTREP provider admission during arena loans

Generic safe ALTREP providers now pass an original-owner admission gate before
callbacks, including class configuration. A live arena loan rejects the call
before provider code runs. Passive sealed compact-sequence reads retain their
existing loan-safe path. Callback activation restores the original live owner
before post-callback checks; original revocation refuses continuation, and a
live owner's callback panic retains its exact payload.

Two scalar-only counter fixtures reproduced the previous missing gate without
executing an allocator or interpreter under the loan (0 passed, 2 failed).
The final implementation passes 55 native controls (45 ALTREP, nine compact
sequence, one private-edge remapping) and seven strict Miri tests with the
default alias checker and strict provenance (420.51 seconds). Clippy with
warnings denied and formatting pass. One old declaration-change fixture was
updated to reach canonical private metadata instead of public attributes;
its mutation, rejection, cache, and retry assertions remain intact.

## Owned foreign routine declarations and library leases

Foreign registration now copies names, interface, arity and primitive argument
types into a Rust registry that forbids unsafe code. Resolution owns a
declaration snapshot and retains the physical loaded library through invocation;
re-registration, visible unload and session retirement cannot invalidate that
snapshot. The DLL table ends its mutable loan before initialization, unload or
physical loader destruction can execute host code. Registered wrong interface,
arity and primitive type requests are rejected before the handler runs.

A counter-only wrong-arity fixture genuinely failed before the repair while
using a compatible one-list External ABI. The final seven selected native cases
pass, including actual buffer type admission and re-registration/unload/session
retirement. The same seven cases pass strict-provenance Miri with the default
alias checker in two completed selections (three in 237.82 seconds and four in
255.27 seconds). Warnings-denied Clippy and assigned formatting pass. An initial
Miri run stopped at an unsupported platform strcmp call; final path comparisons
use CStr byte equality. The separate existing default-session DLL controls did
not complete within their bounded startup observation and are not counted as
passing.

The foreign input and invocation seam remains explicitly unsafe: declarations
cannot authenticate arbitrary host addresses, actual ABI signatures or foreign
package global state. Dynamic unregistered lookup remains a trusted host
facility. Registered single-precision buffers are rejected before invocation
until proper float marshalling exists; authenticated R-visible native symbol
handles and full foreign API semantics remain tracked in rport-wpdk.3.2.

## Public supersmoother character coercion

The public supersmoother wrapper now preserves GNU's scalar-condition admission
and plain-character comparison/coercion order. It retains the original inputs
through each provider read, collecting callbacks and coercion warnings, and
rejects successful publication after original-session revocation. Numeric
coercion uses the canonical Rust R_strtod parser within the original owner gate;
this unit does not change the numerical supersmoother kernel.

Twenty-one independently generated GNU cases compare values, errors and warnings
under C collation. The final enclosing 15 native supersmoother cases pass
(including the nine public cases), and three strict-provenance Miri fixtures
pass in 767.19 seconds with the default alias checker. Those fixtures cover the
complete character-case table, detached inputs with full collection, and
revocation at each of four provider reads. Warnings-denied Clippy, assigned
formatting and a byte-identical independent GNU fixture rerun pass. The source
ledger records independent later changes without claiming they were exercised
by this selection. List/classed span coercion and locale breadth remain tracked
in rport-wpdk.6.5.2.

## Original temporary-binding cleanup

Nested source replacement now owns its original environment, binding cell and
saved value, with a checked cleanup helper that forbids unsafe code. Admission
reads an existing active getter in GNU order before lock/active rejection; it
remains unarmed through that admission. Normal cleanup disarms before restoring
the original recorded cell. Error cleanup passively removes the temporary
binding from the original physical heap, including after revocation, without
invoking a provider, evaluator or new ambient runtime.

Eleven final native regressions, ten enclosing source-assignment controls, and
all eleven strict-provenance Miri cases pass. The completed Miri selection takes
2,473.20 seconds with the default alias checker and memory-profiling enabled.
Independent pinned GNU cases establish getter side effects, bound NULL, error
removal and late lock/detachment behavior. Tests remove incidental roots before
collection and preserve live panic payloads through cleanup. Warnings-denied
Clippy and assigned formatting pass; source ledgers distinguish independent
later changes. The existing protected raw evalseq chain remains a separate
modernization target. Malformed or cyclic frames are left unchanged during
unwind rather than raising a second cleanup panic.

## Checked packed PPR prediction

The registered five-buffer Fortran prediction entry now invokes a Rust kernel
and admission module that both forbid unsafe code. Checked packed dimensions,
workspace extents and projection ordering are admitted before sorting or output
writes. The kernel preserves GNU's projection sorting, permutation temporaries,
interpolation and observable model/workspace mutation, including zero-term
short models and zero prediction observations.

A genuine managed-entry regression failed before registration. Nine independent
pinned GNU cases compare all 45 argument buffers. The final seven native cases,
42 enclosing buffer controls and all seven strict-provenance Miri cases pass;
the completed Miri selection takes 654.33 seconds with the default alias checker.
Managed cases remove incidental inputs before collection, exercise aliased
model/workspace inputs and foreign-runtime reentry, and refuse wrong packages or
original-owner revocation before invocation. Warnings-denied Clippy, assigned
formatting and byte-identical GNU fixture reproduction pass. Assigned source
hashes remain unchanged through the proof; later independent loader/serializer
changes are recorded separately.

Checked admission now covers 22 of the 26 captured stats C/Fortran registrations.
This unit implements stateless prediction; PPR training, COMMON settings,
tracing and spline branches remain separate compatibility work.

## Explicit paths before session bootstrap

Core, Android and embedding sessions can now receive an explicit RuntimePathPolicy
before base initialization and default package loading. This path performs no
host R discovery, and the detached constructor reuses the original dispatch
guard. The existing default constructors retain their behavior.

Two native core controls pass in 4.12 seconds, covering explicit path admission
and original dispatch restoration with the exact panic payload after real base
bootstrap. A public embedding case also passes with two isolated policies,
distinct session identities and recoverable evaluation errors. Its four separate
dataset cases genuinely fail because portable dataset loading is still absent;
they are recorded as remaining work, not constructor acceptance failures.
Warnings-denied Clippy across rmath/r-embed targets with memory profiling and
assigned formatting pass. This configuration seam adds no unsafe code and makes
no new Miri or full package-compatibility claim.

## Named file construction on native and browser runtimes

File anonymity now follows GNU's original empty description. A nonempty path
remains a deferred named connection even when it resembles a generated temporary
filename. Named browser constructors no longer call an unsupported host process-ID
function. Anonymous browser update files report a recoverable R error before host
temporary-file operations; their full implementation remains separate work.

The original code fails two of three native controls and all four selected browser
cases. The repaired code passes all three native controls, all four fresh Chromium
connection cases and three output/RNG cases against the same Rust Wasm asset.
Two strict-provenance Miri cases pass in 176.44 seconds with the default alias
checker, covering the virtual backend and controlled-error recovery. The host-file
nontruncation case runs natively. The pinned GNU oracle, warnings-denied Clippy and
assigned formatting pass. The frozen source/artifact ledger distinguishes later
independent startup and dataset changes; this is not whole-tree certification.
Anonymous virtual update files and the remaining raw constructor operand lifetime
audit are tracked separately. Native full-bootstrap output acceptance is still
pending and is not inferred from browser results.

## Owning installed package workspace loading

Installed data topics now load original GNU workspace files rather than rejecting
`.rda` and `.RData`. The safe owning admission module bounds stream expansion,
validates the complete binding graph before publication and retains the original
environment and every pending value through setter callbacks. Rust gzip, bzip2
and XZ readers feed the checked decoder. Revocation refuses further publication;
live callback panic payloads survive unchanged. ASCII NA scalar tokens are admitted
without conflating real NA with NaN.

Six independently reproduced GNU fixtures cover version 2/3, ASCII and compression,
including factors, Date attributes, shared cyclic environment identity and bound
NULL. Ten final native cases pass, including three public data-loader cases and
seven ownership/admission controls. Warnings-denied rmath/r-embed Clippy and assigned
formatting pass. Six selected strict-provenance Miri lifecycle/admission cases
complete in 986.30 seconds with the default alias checker. All six file formats
remain in native acceptance; the earlier costly seven-case codec selection was
stopped without an aggregate result and is not counted as passing. The checked
owning sources remain unchanged through the completed proof. This milestone does
not certify all package loading, complete compression resource behavior or
whole-tree aliasing safety.

## Serialized promises and original lazy-load reference restoration

The serializer preserves GNU promise attributes, environment, cached value and
expression without forcing the binding. Decoding owns each field across recursion
and seals the complete promise before releasing the arena to allocation callbacks.
The extracted reader keeps the ordinary recursive reader's stack frame smaller.

Internal lazy-load reference hooks now use owning Rust error transport. Corrupt
references raise their original errors instead of fabricating empty environments.
Provisional cache entries roll back only the exact original binding they published;
caller and callback entries survive, including after runtime revocation. The decoder
copies raw input before callbacks so reentrant mutation cannot alias its reader.

Six promise cases, two persistence-error/rollback cases and three enclosing
environment controls pass natively. All six isolated promise Miri cases pass in
503.95 seconds, and both file-backed persistence cases pass in 596.17 seconds.
Both use strict provenance and the default alias checker; only the file-backed
selection disables isolation for its actual unique temporary-file IO. GNU fixtures,
warnings-denied Clippy and assigned formatting support this scoped checkpoint.
Original installed cache identity also has a passing regression. Full startup and
portable dataset work remain separate units; temporary diagnostic startup results
are not substituted for their unchanged final acceptance tests.

## Generic namespace export introspection

The base environment now defines GNU's `getNamespaceExports` closure. It resolves
the original namespace and reads its actual exports environment; base uses its
own namespace names. A focused real-base test passes for base, ordinary populated
and empty exports, invalid namespaces and recovery. Its completed native footer
is one pass in 2.40 seconds. The independent pinned GNU control agrees. This
checkpoint adds no unsafe code and does not certify portable dataset loading or
the separate namespace lifecycle work.

## Serializer checkpoint correction

Partial staging previously placed several promise-reader and hook declarations
at incorrect locations in the committed files. The tested working source was
correct. The correction restores those declarations without including concurrent
numeric or portable dataset changes. A separately archived Git index tree passes
rmath/r-embed all-target checks and all six promise cases in 0.04 seconds. Future
shared-file checkpoints must compare the index contents to the intended source
and validate an archived index tree when partial staging moves declarations.

## GNU ASCII numeric stream contracts

The safe scalar module preserves integer/real NA, NaN, infinity and signed zero,
emits GNU decimal or hexadecimal tokens, and admits bounded hexadecimal input
with IEEE ties-to-even rounding. ASCII NA mode selects hexadecimal output;
version 2 and 3 declare their original distinct minimum reader versions.

All 43 independently reproduced GNU scalar cases agree. Nine native cases and
nine strict-provenance Miri cases pass, with the latter completing in 149.59
seconds using the default alias checker. Four complete vector streams retain
every scalar across both modes and both versions, with full collection before
serialization; scalar rounding and malformed-input cases remain independent.
Warnings-denied facade Clippy and assigned formatting pass. Earlier incomplete
or failed proof attempts are retained separately. Later writer-version metadata
and symbol encoding changes are separate units.

## Owning portable model covariance ratios

The public `covratio` entry now uses a safe owning Rust kernel. QR-backed models
use their actual QR factors, rank and original influence calculation. Supplied
influence/residual arguments, recycling, names and exceptional numeric results
follow the independent GNU controls. Portable `lm` retains the original response
and row labels through callbacks and publishes matching fitted/residual names.

Eight native cases pass, as does the unchanged public GNU comparison of all 32
mtcars values and names. Seven strict-provenance Miri cases pass in 531.07 seconds
with the default alias checker and isolation. The full public bootstrap case
remains native acceptance. Warnings-denied facade Clippy, assigned formatting and
reproducible GNU fixtures pass. GLM dispersion and weighted preprocessing are
tracked separately and are not certified by this linear-model checkpoint.

## Permanent native character encoding

The safe permanent character allocator now seals GNU's ASCII flag with the
original bytes before publishing the checked node. Non-ASCII native bytes retain
their original encoding identity. This repairs symbol printname flags at their
producer rather than modifying serialized output.

The original ASCII regression fails; all eight enclosing heap cases then pass
natively and under strict-provenance Miri in 57.78 seconds with the default alias
checker. Sealed payload and retained ownership controls remain covered. Assigned
formatting and warnings-denied facade Clippy pass. Together with the separate
writer metadata fix, the unchanged independent 592,931-byte graph of all 108
portable datasets compares exactly; that enclosing result does not certify every
character encoding or native symbol conversion path.

## Portable browser facade initialization and output budgets

The Wasm facade now chooses the same explicit portable library policy on every
target before full base/default initialization. Native facade tests previously
discovered host GNU packages and exceeded the unchanged browser heap limits
before user evaluation. Real portable initialization admits ordinary evaluation
under those same limits.

Four native cases pass in 9.88 seconds, including exact independent capture and
result truncation markers, printer newlines and recovery. Three fresh Chromium
cases pass in 30.2 seconds. Native output uses 16 real cat calls for the same
two-MiB stream; browser coverage retains 2048 calls. Memory, node, time and output
limits remain unchanged. Warnings-denied Wasm Clippy, assigned formatting and
website lint pass. Artifact ledgers distinguish the earlier browser writer
metadata from later native metadata changes. Host-discovering embedding sessions
and the remaining showcase cases require their separate full acceptance gates.

## Owning sequential mget promise forcing

The safe mget implementation owns its operands and selected values through name
providers, promise forcing and result allocation. Each lookup is followed by
forcing before the next name is looked up, preserving GNU side effects and
cached-value identity. Original-runtime revocation refuses publication.

Six native cases and all six strict-provenance Miri cases pass. The completed
Miri selection takes 1489.76 seconds with the default alias checker. Tests detach
actual argument and binding graphs before collection, exercise reentry and
remove incidental promise roots. Pinned GNU controls, warnings-denied Clippy and
assigned formatting pass. Mode filtering, callable fallback and sharing semantics
remain a separately tracked contract extension.

## Verified GLM and weighted covariance-ratio extension (rport-wszw.6.1)

The owning Rust kernel now scales working residuals by working weights, filters
zero prior-weight cases, retains original row labels, and follows the pinned
GNU covratio body for estimated or fixed GLM dispersion. Fixed dispersion does
not coerce an unused influence sigma. These changes retain the original model
domain across callbacks and reject publication after its authority is revoked.

The independent GNU contract includes actual QR data, explicit dispersion and
working/prior-weight differences. The native enclosing suite passed 13 tests;
the unchanged public mtcars 32-value/row-name contract also passed. All five new
strict-provenance Miri cases passed with the default alias checker and isolation
(690.33 seconds), including detached fields through full collection, exact live
panic propagation, revocation and label filtering. All-target Clippy and assigned
formatting passed. Frozen source hashes remained unchanged through that footer.

This closes the weighted/GLM unit, not the model-system family. Original
na.exclude restoration remains tracked separately as rport-wszw.6.2.

## Pinned version metadata and GNU NULL contracts (rport-wszw.7)

A forbid-unsafe compatibility_target module now defines the oracle version, date,
revision, nickname, real target architecture/OS and explicit Rust-port identity.
R.Version(), R.version, getRversion(), the C metadata projections and serialized
writer headers use that same target. Historical minimum-reader versions retain
their original version-2/version-3 values. This identifies the pinned oracle; it
does not declare complete GNU R coverage. Static C getters now require no unsafe
operation. The buffer boundary delegates to a checked slice writer and reports
text bytes without counting the C terminator.

Original GNU numeric-version formatting preserves names; character conversion
removes them while retaining missing and empty values. Shared repairs admit NULL
in the internal pairlist predicate, preserve expression(NULL), and let as.vector
apply the requested mode to NULL. Empty atomic assignment initializes only a
NULL LHS; the ordinary VectorAssign type admission handles other vectors. That
removes an extra coercion which had incorrectly tried to convert an empty
character LHS into NULL.

Exact staged tree f712fa9efa9d270b94ccda7923cbd701db28d42b was archived separately
from ongoing agent edits. Its public suite passed all three tests (15.91 seconds),
its three shared-kernel tests passed, and all-target Clippy plus formatting passed.
Three metadata tests passed strict-provenance Miri with the default checker and
isolation (65.53 seconds). Independent pinned GNU controls cover ten NULL modes,
named/missing/empty versions, empty assignment, type promotion, growth and invalid
replacement. The additional NULL-kernel Miri run subsequently passed all three
cases (441.54 seconds), with the default alias checker, strict provenance and
isolation unchanged. Its original loaded production paths stayed fixed through
the actual footer; this scoped evidence does not attest unrelated concurrent
agent edits as one whole-HEAD run. rport-wszw.7.1 is complete. The separate
isVectorizable admission gap remains rport-wszw.7.1.1.

## Installed methods startup preserves original caches (rport-2cxuy.13)

Installed methods images already contain their class and implicit-generic tables.
Startup now initializes dispatch without manually recaching that namespace or
rewriting matrix/array initializers. The original initializer closures retain
their shared local capture environment and its two helper functions, rather
than publishing replacement helpers in the namespace.

The independent pinned GNU oracle confirms cache identity, the captured helper
environment and absence of those helpers from namespace bindings. The canonical
default-session test passed (60.34 seconds), preserving its four assertions,
compiled-body/JIT restoration and collection checks. Its dispatch query was
corrected to the actual base primitive `.isMethodsDispatchOn()`; GNU rejects the
former methods-qualified spelling. Thus its constructor and assertions were
preserved, but the fixture is not byte-identical to the preceding commit.
The separate installed cache/initializer identity test passed (22.83 seconds),
and all-target Clippy plus assigned formatting passed. These are completed
native startup checks, not a strict-Miri proof of the entire package bootstrap.

## Scalar coercion fixtures avoid unrelated package bootstrap (rport-jxfp.3.12.5)

Thirty-three scalar conversion, sentinel, warning-flag and raw-vector unit
fixtures now use the existing lightweight owning GC runtime. Their conversion
assertions remain, and the separate full default-session methods integration
fixture continues to exercise installed package startup. This removes repeated
package bootstrap from tests which never query a package, without changing the
public session constructor or compilation settings.

The faster enclosing run exposed an obsolete real-to-complex NA assertion.
Both the pinned GNU executable and its coerce.c confirm a zero imaginary part
for this build. The corrected assertion also verifies the exact real NA bits
and ordinary NaN/positive and negative infinity preservation; a durable GNU
fixture records that contract. The final enclosing coerce suite passed all 41
cases (0.37 seconds), including the seven new vectorizable tests. All-target
Clippy and assigned formatting passed. These timings describe unit-test setup,
not a measured change in ordinary runtime startup. The earlier stopped package
bootstrap attempt and the intermediate 40-pass/one-failure result are not green
verification checkpoints. Strict vectorizable validation remains independent.

## Owning mget contracts: native candidate (rport-wszw.9)

The retained lookup graph now applies GNU mode normalization, searches parents
past mismatched child bindings, matches named operands before positional ones,
recycles atomic/list fallbacks and evaluates callable fallbacks in the original
caller environment. Name, environment, mode, fallback and inherits admission
follow the pinned GNU ordering. Existing sequential promise-forcing tests remain.

All 15 native cases passed (0.20 seconds), with independent pinned GNU controls,
warning-denied Clippy and formatting completed. The frozen four-file candidate
is checkpointed so broader CI can run in parallel. Its strict 15-case Miri run
continues unchanged; no completed strict aggregate or full lookup-family parity
is claimed here, and rport-wszw.9 remains open until its acceptance checks finish.

## Owning na.exclude covariance rows: native candidate (rport-wszw.6.2)

A forbid-unsafe observation map retains omission indices and row labels, and
restores default residuals and influence fields at their separate GNU evaluation
phases. Supplied operands retain their original behavior, including recycling,
name precedence, exact NA bits, warning counts and warn=2 error recovery.

The enclosing native suite passed all 22 tests (3.11 seconds), preserving the
earlier 13 assertions and adding nine omission/ownership cases. Independent GNU
controls, all-target Clippy and formatting passed. The exact six-file candidate
is checkpointed while its nine-case strict Miri run continues unchanged; this is
not a completed strict aggregate, and rport-wszw.6.2 remains open. Exact GNU
arithmetic warning-call attribution is a separate remaining item, rport-wszw.6.3.

## Checked GNU vectorizable admission (rport-wszw.7.1.1)

A forbid-unsafe helper now admits GNU NULL/list/pairlist categories and vector
children of length zero or one, while rejecting atomic/expression outer inputs.
Checked pairlist identities detect cycles and improper tails. List providers use
original owning values and authority checks after success or unwind, retaining
selected children through collection and rejecting revoked-runtime publication.

The unchanged production baseline genuinely failed both regression tests before
repair. The completed candidate passed seven new native tests, the enclosing
41-case suite, all-target Clippy and formatting. Independent pinned GNU controls
passed the public coercion cases and its actual native predicate's 12 category
cases. All seven tests passed strict-provenance Miri with the default alias
checker and isolation (734.14 seconds), with the five source hashes unchanged
through the reaped successful footer. An earlier stale cached artifact selected
zero tests and is explicitly excluded from that proof. This closes predicate
admission; raw counted string/list coercion paths remain rport-wszw.7.1.1.1.

## Public compiler namespace acceptance (rport-jxfp.3.12.4)

The retained public embedding fixture now completes all four tests with the full
default constructor (358.86 seconds). It preserves checks for source, formals,
environment, executable user calls, builtin identity, GNU options, named argument
matching and rejected duplicate arguments. Its current source and executable
hashes remained fixed through the successful reaped footer. The preceding pinned
GNU contract and runtime namespace tests remain independent supporting evidence.
This completes the formerly bounded/incomplete public gate; it does not claim
all private compiler operations, every execution path or whole-HEAD CI parity.

## Portable original datasets: native and Wasm candidate (rport-wszw.2)

The portable package retains all 108 original lazy objects, their captured key
syntax and the 91 public data topics. Authenticated assets include the original
compressed database, namespace metadata, index and independent value contracts;
loading does not require an installed host R library. A forbid-unsafe owning
kernel holds operands and codec buffers through collection and reader callbacks,
and publishes the namespace only after construction. Thin native bridges remain.

All seven public native tests passed (180.54 seconds), including every original
type/attribute contract, topic, lazy expression, copy-on-write behavior, model
result and all 592,931 serialized bytes without normalization. Eight owning
native tests passed, and the fresh release Wasm runtime passed 205 real Node
checks including that complete byte stream. Independent pinned GNU contracts,
reproducible asset generation, five tooling tests, all-target Clippy and assigned
formatting passed. The frozen 30-file candidate is checkpointed for broader CI.

Its eight-case strict Miri run continues unchanged and has no aggregate footer
yet, so rport-wszw.2 stays open. Removed lazy bindings/malformed namespace
metadata, generic browser filename I/O and public namespace listing are separate
remaining items (rport-wszw.2.3, rport-wszw.11 and rport-wszw.12). This checkpoint
does not claim arbitrary namespace mutation compatibility or complete R parity.
