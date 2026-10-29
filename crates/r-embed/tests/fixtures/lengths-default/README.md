# lengths default GNU shape control

contract.out is exact stdout from contract.R under GNU R development r90451,
commit bac583951b728e97b9786804d3b4081f0fe18df5. Regenerate with that oracle's
bin/Rscript --vanilla contract.R. The integration test compares all 38 rows
unchanged in native and restricted portable sessions.

This bounded migration preserves dim for both use.names values and copies
names/dimnames only when requested. It validates missing/NA use.names like
GNU and uses rooted copied element reads/writes instead of a raw result
span across lazy provider callbacks. Shape values are selected after child
callbacks, in GNU dim/names/dimnames order, and rooted before setter
allocation. No native host Vec or Rust-recursive walk is introduced.
Pairlist children and attribute spines use constant-space checked cycle
validation. Long children restart in REAL output width as GNU does; both
INT and replacement REAL outputs stay admitted at the allocation peak.

Controls include atomic/list/array/matrix/expression/empty neighbors, invalid
input/flag errors, and independent source/result shape mutation. Checked
owner tests additionally cover provider GC and input detachment, post-read
shape mutation, errors/panics/recursive reads, output denial before reads,
same-cap recovery, foreign/revoked owners, cyclic pairlist children, and a
non-materialized 2^31 child with width retry/node-peak denial. A separate
pinned GNU long-control.R/out is saved with the validation evidence.

Full lengths conformance is not claimed. Existing top-level lengths and
per-child length/[[ method dispatch, complete formal matching, environment
child counts and detailed POSIXlt policy are tracked in rport-e9hnf. This
slice preserves the existing POSIXlt observation-count policy. Full rmath
and original S4 package-workflow budget/timing gates are outside this slice.
