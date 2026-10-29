# Recursive anyNA GNU control

`contract.out` is the unchanged stdout of `contract.R` under the pinned GNU
R development r90451 oracle, commit
`bac583951b728e97b9786804d3b4081f0fe18df5`.

Regenerate with its `bin/Rscript --vanilla contract.R`. The integration test
compares stdout exactly in both native and restricted portable sessions.

The 38 rows cover nested list and pairlist traversal, non-scalar atomic
children, clean/missing neighbors, recursive FALSE, flag coercion/errors,
short circuiting, original input structure, nested `is.na` GC callbacks, and
top-level `anyNA` generic dispatch with GC, NA-valued `is.na` results,
512-level clean/missing graphs, and shared DAG children.

This slice consolidates two default scanners. It preserves the existing
public top-level generic dispatcher; it does not complete every anyNA edge.
Nested custom `anyNA` methods, formal name matching, and unsupported-type
errors are tracked separately in rport-f0r6d.
Plain recursive list/pairlist descent uses an admitted iterative DFS stack.
Frame storage grows geometrically after original-owner workspace admission.
Growth admits old-plus-new buffer capacity before allocation, then reconciles
to actual final capacity after the old buffer is released;
active-ancestor identities detect cycles without rejecting shared DAGs.
Pairlist spines also have a constant-space checked cycle guard before the
shallow is.na fallback, including classed default fallbacks. Existing top-level
anyNA method dispatch still runs first.
Cycle errors are a safe runtime boundary for checked cyclic graphs, which
ordinary GNU R value assignment cannot construct. Native checked controls
cover 10,000 levels (48 under Miri), cycles, short circuiting before a cycle,
workspace growth/denial, provider GC, and same-cap recovery. Ancestor checks
are linear in active path length. User methods continue through the existing
interpreter evaluation path. No full rmath or original
S4 timing/budget acceptance claim is made by this focused fixture.
