# GNU R API inventory evidence

Generate a namespace and native-registration census with the pinned oracle:

```sh
python3 scripts/generate_gnu_r_census.py /tmp/gnu-r-census
```

The output directory must be new or empty. Optional package arguments select a
smaller scan. `--runtime` selects an installation; the existing oracle validator
checks its manifest marker and runtime version before reflection runs.
`provenance.json` records that validation mode, the manifest, launcher and census
script hashes, build profile, selected packages, exit status, and output hashes.
The raw R census records a declared commit without authenticating it; use the
Python entry point to obtain the validated installation metadata.

An incomplete scan exits unsuccessfully and preserves its tables and issues.
Pure R namespaces without a DLL table are valid. Active bindings are recorded
without invocation, and package initialization failures remain explicit.

At the pinned `bac583951b728e97b9786804d3b4081f0fe18df5` installation, the full
selected-package scan records 5,875 function bindings and 512 native
registrations. It remains incomplete: the base `.Library.site` active binding
is not forced, and this installation cannot load Tcl/Tk. The three pure R
namespaces `compiler`, `datasets`, and `stats4` scan successfully with zero
issues. The pinned build omits recommended packages, so neither result inventories
the complete GNU distribution.

Port registration evidence is a separate boundary. The native admission fixture
`owning_native_external_payload_metadata_matches_independent_gnu_registry`
reconciles 107 independent GNU External entries against the actual Rust resolver.
It checks the fixed or variadic payload metadata of all 66 resolved entries.
Unresolved entries are not counted as passing invocations. Some resolved graphics
entries still use incomplete handlers; matching arity does not establish their
behavior.

The namespace tables leave implementation, behavior, and safety unclassified.
Completion requires joining actual resolver results and independent behavior
evidence for each relevant option, execution path, and target. Primitive/internal
tables, public headers, conditional registrations, S4 tables, package assets,
and packages absent from this oracle profile also need separate reconciliation.
These limits are recorded in the generated metadata; function or driver counts
do not give a defensible completion percentage.
