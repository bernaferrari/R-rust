# Logical string coercion

`contract.out` is stdout from `contract.R` under pinned GNU R development
r90451, commit bac583951b728e97b9786804d3b4081f0fe18df5. The public test
compares both native and restricted portable sessions, covering exact logical
spellings, UTF-8 nonmatches, attributes, mutation isolation and method dispatch.
The fixture uses character names; a separate integer-names failure is tracked
as rport-clxsb. Direct checked controls cover foreign owners, revocation,
provider callbacks, denial/recovery and malformed native bytes.
