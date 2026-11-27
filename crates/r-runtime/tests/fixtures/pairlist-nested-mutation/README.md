# Pairlist names: nested mutation during coercion

This is a GNU R oracle probe, not part of the Rust runtime. Build `probe.c`
with the R oracle pinned by `oracle/r-oracle.json`, then run:

```sh
R CMD SHLIB probe.c
Rscript --vanilla -e 'dyn.load("probe.so"); print(.Call("probe"))'
```

At pinned commit `bac583951b728e97b9786804d3b4081f0fe18df5`, the first
ALTREP element triggers GC and changes the dense scalar inside the second
pairlist element. GNU completes the assignment with names `c("1", "99")`;
the scalar is `99`. This establishes the ordering for this particular graph,
not the semantics of arbitrary nested objects, cycles, or provider mutation.

`direct.c` changes the second pairlist element from a list containing the
scalar to the scalar itself. Compile it with `R CMD SHLIB direct.c` and load
`direct.so` in a fresh R process using the same `.Call("probe")` expression.
GNU again returns names `c("1", "99")`. Rport's current direct-scalar
mutation test expects rejection; this separate mismatch remains open.
