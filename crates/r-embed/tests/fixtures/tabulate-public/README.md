Pinned GNU R: source commit bac583951b728e97b9786804d3b4081f0fe18df5,
development r90451 (2026-08-27).

Generate exact output with `Rscript --vanilla contract.R > contract.out`.
The unchanged bundled upstream/base/R/tabulate.R supplies public validation,
coercion, lazy default-nbins evaluation and size checks. The kernel contract is
src/main/util.c::do_tabulate: integer bins, exactly two arguments, valid nbin.

The same fixture runs in native and restricted portable sessions. It covers
wrapper formals, invalid inputs/sizes, factor/real/all-missing defaults, warnings
with GC, >=2^31 rejection, internal type/arity and source/result isolation.
No normalization or physical ALTREP representation requirement is imposed.
