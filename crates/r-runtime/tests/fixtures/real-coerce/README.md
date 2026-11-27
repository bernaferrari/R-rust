Pinned GNU real/double coercion controls cover logical/integer inputs, real
identity neighbors, complex-source conversion, strings/raw, missing and nonfinite
values, warning messages, warning-handler GC/nested coercion, warn=2 recovery,
public attribute stripping and mutation isolation in both directions.

Golden command:

    ~/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript --vanilla contract.R > contract.out

These public values already match the baseline. The production change closes
independently reproduced foreign integer-source acceptance in coerceToReal.
No baseline UB diagnosis is claimed. The companion public test preserves typed
allocation errors and recovery under the same cap in both runtime path policies.
The complex-output mapper and all scalar conversion kernels remain unchanged.
