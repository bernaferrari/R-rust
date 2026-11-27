Pinned GNU raw coercion controls cover truncation, range/NA handling, combined
warning messages and order, default metadata removal, same-type attributed raw
source isolation, preserved S3 method-result metadata, warning-handler GC/nested
coercion and warn=2 recovery. Character coercion and scalar kernels are separate.

Golden command:

    ~/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript --vanilla contract.R > contract.out

Baseline byte values and warnings match GNU. Default as.raw retains attributes
and an attributed same-type result mutates the source; the default wrapper now
uses the existing clean atomic helper, copying before clearing metadata.
Internal coerceToRaw still performs shallow metadata transfer. Dispatched method
results retain their attributes. The owner tests independently reproduce foreign
integer-source acceptance and cover both mapper and wrapper GC/owner/errors.
No baseline UB is claimed; public budget controls recover under the same cap.
