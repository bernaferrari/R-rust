Exact GNU controls from R source commit
`bac583951b728e97b9786804d3b4081f0fe18df5` (R development 2026-08-27 r90451):

```
Rscript --vanilla contract.R > contract.out
Rscript --vanilla neighbors.R > neighbors.out
```

The source contract is `src/main/coerce.c` `do_isfinite`, `do_isinfinite`,
`do_isnan`. All three return logical(0) for NULL and reject unsupported input
kinds after generic dispatch. Real/complex NA and NaN remain distinct.
Only names or dimensions/dimnames are copied; other attributes are dropped.

The fixtures run unchanged in native and restricted portable sessions. The
neighbor fixture includes independently mutated dimnames, S4-marked numeric
vectors, genuine S4 methods, and custom S3 list methods that force GC. No
POSIXlt methods or datetime fixtures are altered.
