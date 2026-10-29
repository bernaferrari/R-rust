Pinned GNU integer coercion controls cover values and warning messages for
logical, real, complex, string and raw inputs, actual oracle boundary behavior,
public attribute stripping, mutation isolation, shape/class/list/pairlist
neighbors, warning-handler GC/reentry and warn=2 recovery.

Golden command:

    ~/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript --vanilla contract.R > contract.out

The ordinary public controls already match the baseline. The mapper change
addresses independently reproduced foreign-owner acceptance in coerceToInteger;
no baseline UB diagnosis is claimed. The companion public test preserves the
original typed allocation error and recovery under the same memory cap in both
runtime path policies. Internal metadata transfer remains shallow; public
as.integer continues to strip its attributes.

The pinned oracle returns INT_MAX for exactly INT_MAX+1+0i, while the real
neighbor returns NA with a range warning. That established behavior is preserved;
the conversion kernels and boundary operators are unchanged.
