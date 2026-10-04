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
| Compiled execution owns its instructions and operands | Source/private/GNU bytecode replacement fixtures, including source-tree release and private-pool detachment | Prior targeted Miri checks pass. GNU CALL syntax constants are retained when its bytecode format requires them; private source-pool erasure tests do not imply GNU constants can all be discarded. |

## Reentry, transfers, and shutdown

| Obligation | Executable boundary | Evidence limits |
| --- | --- | --- |
| A callback cannot redirect cleanup into another runtime | S4 status-restoration and callback dropped-facade/revocation fixtures | Guards retain the original physical allocation and reject publication after revocation. Native fixtures and all seven S4, ten callback, and seven eval strict-provenance Miri regressions pass with the default borrow checker. |
| Direct native entry has an authenticated return-transfer scope | `owned_s4_native_validity_installs_and_releases_transfer_scope`, `owned_task_callback_direct_managed_entry_supports_return_and_on_exit`, `owned_eval_native_direct_entry_installs_and_releases_original_transfer_scope` | Native checks execute real return/on-exit paths and verify scope cleanup. They do not authenticate arbitrary foreign function signatures. |
| Eval consumes only its own return ticket | `owned_eval_return_is_local_to_exact_eval_context`, `owned_eval_preserves_unmatched_original_return_ticket_after_full_gc` | Native checks distinguish a local eval return from an unmatched original transfer. |
| A live owned value has a deliberate relationship with closure | `sexp::owner::tests::{owned_value_preserves_original_graph_after_runtime_close_and_drop_without_cycle,owned_lazy_value_rejects_revoked_provider_before_callback,owned_lazy_callback_can_close_and_drop_runtime_but_cannot_publish_success}` | Pure owned storage can survive runtime closure; runtime-dependent providers must reject a revoked owner. This is different from a host handle remaining usable after its session closes. |
| Resource destruction can reenter only after arena loans end | `sexp::gengc::tests::collected_node_resources_reenter_only_after_collection_and_arena_lends_end` | Does not establish every external resource's construction, explicit close, collection, and shutdown matrix. STARMA has a typed canonical resource boundary; its full lifecycle acceptance remains required. |
| A host handle retains its value privately and rejects reused identities | `owned_retained_*` store regressions | Ten store fixtures cover binding interference, collection, failed publication/writes, foreign identities, generation retirement, sole closure roots, and closure. The separate full embedding suite still requires default package startup. |
| Native invocation uses its actual registered callable signature | `mainutils::native_routines::tests::*`, `mainutils::dotcode::typed_native_handler_tests::*` | Eight native and strict-provenance Miri tests pass, including real lazy name/package providers that collect and close the original runtime. Erased C/Fortran and foreign pointers remain outside this milestone. |
| GC preambles release their exact temporary ownership on unwind | `owned_gc_*` | Nine native tests pass for full/lite GC, allocation torture, eval safe points, detached bindings, callback closure, and panic cleanup. The eleven-case GC/capture Miri group is still running. |
| Captured output preserves emission and original-owner cleanup | `exact_console_capture_*`, `exact_top_level_emission_*`, `owned_output_capture_*`, four `owned_retained_console_*` fixtures | Independent GNU fixtures verify fourteen stdout cases. Native checks cover stream order, custom-print errors, active bindings, revoked printing, later-call rejection, and live panic payloads. The host-root/console and GC/capture Miri groups are still running. |

Unreachable-cycle coverage and retained-memory measurements must include
environments, closures, promises, and external resources together. The current
node counts and root-release assertions do not measure resident memory after
retaining one small value, allocating a large temporary graph, closing the
session, and releasing the final value. Broad runtime borrowing work remains
tracked in `rport-sg9a`; the acceptance matrix must not substitute for that work.

## Verification checkpoint

The current frozen production tree passes 704 selected native tests, including
the two closure-copy regressions, seven S4, ten task callback, eight eval, and
41 new native-dispatch, retained-value, GC, and exact-output regressions. A
separate run of those 41 new cases passes with no failures or ignored tests in
66.37 seconds; the full selection finishes in 257.51 seconds. Clippy's
correctness gate and native embedding/all-target and Wasm Rust-backend checks
pass. Existing compiler warnings remain; this is not a warnings-free gate.

The typed native dispatch group additionally passes all eight strict-provenance
Miri tests in 629.10 seconds, with the default alias checker. The new retained
value/console group (fourteen cases) and GC/output-capture group (eleven cases)
are still running and are not counted as complete passes.

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
conformance run. The complete workspace formatting check currently reports
differences in 291 files; its mechanical repair is tracked separately and must
not overwrite a running agent's source.

Kani should target production identity, generation, workspace, and typed-native
admission helpers. Miri and collecting integration tests remain necessary for
aliasing and reentry. No new Kani proof is claimed by this checkpoint.
