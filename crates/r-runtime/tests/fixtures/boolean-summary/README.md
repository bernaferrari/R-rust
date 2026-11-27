# Boolean Summary pinned GNU control

contract.out is the exact stdout of contract.R from GNU R development r90451,
commit bac583951b728e97b9786804d3b4081f0fe18df5. Regenerate with that oracle's
bin/Rscript --vanilla contract.R. Each case is evaluated through a variable
lookup (including symbols and calls as values); explicit newline output avoids
cat(fill) wrapping differences. No normalization is applied to oracle output.

The integration test compares all 112 rows in native and restricted portable
sessions. Rows cover logical/missing/integer/real/complex/raw/text/list/pairlist/
expression/symbol/language/closure/empty-environment inputs, empty identities,
exact string parsing, flag NA/empty/nonzero/length errors, duplicate na.rm,
decisive-value/NA ordering, short circuiting, and Summary-method dispatch.
Coercion warning handlers and Summary methods force GC. Common nonlogical
inputs are fully converted into an admitted logical vector before folding,
matching GNU's validation and access order. Logical inputs retain the existing
checked element-streaming policy.

An exactly sized, originally admitted argument snapshot roots values before
flag/warning/provider callbacks. It never grows; no replacement buffer peak
is hidden. Coercion outputs use admitted R allocation and copied typed writes,
with no raw output span across callbacks. Linked argument/data spines use
constant-space checked cycle validation. No Rust-recursive descent occurs.

Checked native/Miri controls cover flag callbacks detaching caller args with
exact original workspace observation; all-selected roots surviving GC and
releasing afterward; provider error/panic/recursive reads; full integer
coercion before short circuiting; large logical-output denial before reads;
snapshot denial before reads; same-cap recovery/resident restoration; cyclic
args and repair; foreign/revoked owners; and list plus nested scalar provider
GC during two-pass bounded coercion.

This is not complete Summary/ALTREP conformance. Method argument fixup/default
presence, uncommon S4/external/builtin errors and logical ALTREP materialization
policy remain separate audit rport-5x6zm. The existing shared coerceToLogical
raw-span path is not reused and has separate source-audit rport-az9av. Full
rmath and original S4 workflow budget/timing acceptance are outside this slice.
