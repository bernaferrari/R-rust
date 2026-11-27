Pinned GNU R logical coercion controls: atomic values, NA/NaN/complex/string/raw,
public attribute stripping, shape neighbors, and source/result mutation isolation.
Golden generated with:

    ~/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript --vanilla contract.R > contract.out

The public values already match on the baseline. The production change closes
an independently reproduced foreign-owner acceptance gap in shared
coerceToLogical and removes its raw output projection across ALTREP callbacks.
No baseline UB is claimed; strict-Miri GC baseline passed.

The companion public test enforces the original typed allocation error and
recovery under the same memory cap in both runtime path policies. The shared
mapper retains Rf_allocVector3 for its typed error and GC-retry semantics.
