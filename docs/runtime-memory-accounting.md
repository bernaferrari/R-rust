# Session memory admission

`RResourceLimits.max_alloc_bytes` admits arena backing and registered native
workspaces against one original-session ledger. `RSession::memory_usage()`
reports arena bytes, workspace reservations and their sum. A reservation must
exist before the host allocates its buffer and remain alive while that buffer
is retained. `try_resize` checks the current limit and other allocations before
changing its charge; rejection preserves the old charge. Shrinking and dropping
release only the original session's bytes. Guards are thread confined and do not
keep the interpreter alive. Growth after interpreter closure is rejected.

The embedding renderer reserves its conservative canvas/PNG workspace before
creating the rasterizer or evaluating code. Interactive scene operations grow a
separate reservation before publication. That reservation stays with the scene
between commands and releases on scene discard or session closure. Canvas and
scene reservations therefore compete with the actual R heap and numerical
reservations rather than passing independent limits in isolation.

Raw unserialization reserves its native input snapshot before copying cells.
Compressed input expands through an admitted streaming byte buffer. Replacement
buffers are charged together while both allocations coexist; gzip and bzip2
workspace estimates and a bounded XZ decoder reservation remain alive until
codec teardown. These charges remain with the original session across R
restoration hooks and unwind. Connection streaming remains a separate path.

Production serialization writers use the same admitted buffer. Its reservation
survives the allocation and copy into the final R raw vector, or the external
stream callback, so source values, native output and the destination compete
for the session's one byte limit. Reference tables and saveRDS compressor scratch
are still tracked separately in `rport-cvs2.6`.

`max_arena_nodes` limits simultaneously live nodes. Reclaimed slots remain
reusable, as in GNU R; a live-node cap is not also charged as a cumulative
allocation quota. Execution time and cancellation independently bound work.
Bootstrap finishes with full collection; bounded requests reclaim old temporary
graphs at an already quiescent host entry when the live baseline exceeds three
quarters of either limit.

These are accounted bytes, not process resident memory. Allocator metadata,
shared font caches, unregistered library scratch, caller-owned drawing targets,
GPU allocations, browser infrastructure and network/AI hosts are outside this
ledger. The canvas reservation is a conservative admission estimate rather than
an allocator-level measurement. Browser linear-memory containment and host
worker cancellation remain separate controls. Bead `rport-cvs2` tracks completion
of the remaining allocation paths; this change does not close that parent.
