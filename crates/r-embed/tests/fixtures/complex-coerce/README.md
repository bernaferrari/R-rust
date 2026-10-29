Pinned GNU controls compare complex type, real and imaginary components, and
warning messages separately. They cover source types, NA/NaN/infinity, metadata
stripping, NULL, same-type attributed source isolation, S3 method-result metadata,
warning-handler GC/nested coercion and warn=2 recovery.

Golden command:

    ~/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript --vanilla contract.R > contract.out

Baseline default as.complex incorrectly preserves attributes and returns NULL
for NULL; the pinned fixture exposes these public differences. Numeric components
and warnings match already. The default wrapper delegates to the existing clean
atomic conversion helper while dispatched method results keep their metadata.
Internal coerceToComplex still performs shallow metadata transfer.

Owner tests prove foreign integer-source acceptance before the fix and cover
both mapper and wrapper roots/pins, GC, replacement sessions, revocation, typed
errors, denial/recovery and component-level NaN/NA/signed-zero distinctions.
No baseline UB is claimed. The companion public budget test retains the original
resource error and recovery under the same cap in both runtime path policies.
