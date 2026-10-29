# anyNA unsupported types

`contract.out` is stdout from `contract.R` using the pinned GNU R development
r90451 oracle (commit bac583951b728e97b9786804d3b4081f0fe18df5). The test
compares top-level and recursive unsupported inputs and the NULL/raw neighbors
in native and restricted portable sessions. It does not cover nested generic
dispatch or argument matching, which remain tracked by rport-f0r6d.
