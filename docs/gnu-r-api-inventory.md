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

## Joining native resolver evidence

After exporting the actual Rust resolver inventory, join it to the validated
census without invoking any native handlers:

```sh
python3 scripts/join_gnu_r_api_evidence.py CENSUS_DIR RESOLVER_DIR NEW_JOIN_DIR \
  --source-root COMPILED_SOURCE_SNAPSHOT
```

The importer validates the pinned manifest commit, manifest digest, oracle build
profile, and every recorded census file digest, including the binary inventories.
It retains incomplete namespace evidence and copies the census issue table.
A completed join can contain unsupported native rows and an incomplete census.
The destination must be new or empty; validation failures publish no report.

`RESOLVER_DIR/resolver.tsv` uses these exact eight columns, in this order:

| Column | Meaning |
| --- | --- |
| `dll` | Exact captured DLL scope passed to the package-scoped resolver |
| `interface` | Requested census interface: `.C`, `.Fortran`, `.Call`, or `.External` |
| `name` | Exact captured routine name |
| `num_parameters` | Captured GNU registration arity, with `-1` meaning variadic |
| `resolver_status` | `resolved` or `unsupported` |
| `actual_interface` | Actual descriptor interface, also permitting distinct `.External2` |
| `actual_arity_kind` | `fixed` or `variadic` |
| `actual_num_parameters` | Actual fixed arity, or `-1` for variadic |

Unsupported rows must leave the last three columns empty. Every captured
`(dll, interface, name)` key must appear exactly once. Missing, extra, duplicate,
or relabelled requested keys are errors. Contradictory descriptor metadata,
such as a fixed arity labelled variadic, is also rejected. Genuine differences
between requested GNU metadata and consistent Rust descriptor metadata remain
explicit in `native_routines.csv` and `resolver_mismatches.csv`.

GNU's reflected `.External` registration group does not distinguish `.External2`.
An actual `.External2` descriptor is preserved and labelled
`external_family_only`; this establishes no equivalence between their calling
conventions. Registration arity is distinct from the handler's Rust/C signature.

The resolver's schema-1 `provenance.json` binds `resolver.tsv`, `probe.log`, the
census native table and provenance, and the oracle manifest by SHA-256. It also
records the actual source revision, dirty-state boolean, all recorded source
file hashes, compiler version, command, successful exit status, target, build
profile, flags, default features, and compiled binary hash. The source snapshot
must cover the workspace Cargo files and every rmath Rust/Cargo input. All
recorded source files are rechecked, including any captured build configuration.
`--compiled-artifact PATH` additionally rechecks a retained binary; without that
option, its digest remains a recorded identifier, explicitly marked as not
rechecked. Dependency sources are not a complete build attestation.

Output provenance includes the joiner script digest and deterministic data-file
digests. Identical inputs and source snapshot produce identical output bytes.
Hashes establish integrity and identity against the supplied manifest and
producer metadata; they are not signatures and cannot prove that fabricated
resolver evidence came from an execution. The actual producer and its completed
probe log remain part of the evidence chain.

The actual package-scoped resolver export at source `fd015216` reconciles all
512 captured registrations: 297 descriptors resolve and 215 remain unsupported.
There are no reflected interface-family/arity mismatches; 14 `.External2`
descriptors retain the family-only caveat. All 5,875 function bindings remain
`not_probed`. Resolved entries keep implementation `unclassified`, behavior
`not_tested`, and safety `not_assessed`; options and targets also remain untested.
The report explicitly sets `full_gnu_r_parity` and inventory-completeness flags
false. These are registration counts, not behavior results or a completion score.

Run the importer admission tests separately:

```sh
python3 -m unittest scripts.tests.test_join_gnu_r_api_evidence -v
```

Those tests deliberately use synthetic resolver/census fixtures. They exercise
integrity rejection, source and compile metadata, scoped keys, mismatches,
unsupported rows, transactional output, and reproducibility. They provide no
runtime or GNU behavior proof.

## Reproducing the Rust resolver inventory

Generate the resolver artifact directly from a validated census:

```sh
python3 scripts/generate_rust_native_inventory.py CENSUS_DIR NEW_RESOLVER_DIR \
  --source-root RPORT_SOURCE --manifest oracle/r-oracle.json \
  --target-dir EXISTING_AGENT_TARGET --timeout 600
python3 scripts/join_gnu_r_api_evidence.py CENSUS_DIR NEW_RESOLVER_DIR NEW_JOIN_DIR \
  --source-root NEW_RESOLVER_DIR/source
```

`--source-root` defaults to this checkout. `--target-dir` is optional: leaving it
out preserves the caller's `CARGO_TARGET_DIR` and Cargo configuration. Compiler
flags, toolchain selection and profile settings are inherited without changes.
The timeout must be positive and finite; its default is 600 seconds. The existing
`run_parity_case.py` runner owns the POSIX process group and deadline cleanup.
The producer neither removes the target directory nor invokes registered native
handlers or R session startup.

The producer authenticates all census inputs, writes the exporter's exact four
input columns, and runs only the ignored
`mainutils::dotcode::native_inventory::export_native_registration_inventory`
test through the selected source checkout's `scripts/cargo_dev.sh`. It requires
successful process completion, exactly one completed one-test footer, the exact
census row count, a Cargo JSON test-executable artifact, and a complete,
consistent resolver table. Missing, extra and duplicate keys are failures.
Unsupported keys remain explicit rows. These checks do not exercise handlers.

The fresh output retains `census-input.tsv`, `resolver.tsv`, `probe.log`, and
`source/`. `provenance.json` is published only after checking that the source
bytes, revision, dirty state, Cargo configuration, compiler selection, wrapper,
producer, deadline helper, executable and exporter input stayed unchanged.
Failed builds, incomplete runs and timeouts retain logs and `failure.json` with
`execution_complete: false`; they publish no completed provenance. A timeout
also retains the runner's marker. Invalid input admission can fail before the
output directory is created.

The source archive includes workspace Cargo files, every rmath Rust file,
rmath Cargo metadata, available workspace Cargo configuration and Rust toolchain
selectors. On source `fd015216`, this scope contains 674 files, including
`rust-toolchain.toml`; the earlier manually prepared inventory retained 673 and
omitted that selector. Existing ancestor/Cargo-home configurations are hashed
separately. The profile records actual Cargo artifact features and compiler
profile, the selected target, inherited build environment and configuration
hashes. The `flags` list records inherited `CARGO_ENCODED_RUSTFLAGS` or
`RUSTFLAGS`; it does not flatten additional configuration/target-specific flags.
The retained configuration identities and environment describe those settings.
This is an integrity record, not a hermetic dependency or compiler attestation.

Run the producer's tooling admission cases with:

```sh
python3 -W error::ResourceWarning -m unittest \
  scripts.tests.test_generate_rust_native_inventory -v
```

These tests use an explicitly fake build tool. They prove rejection of changed
sources, altered input, incomplete/duplicate footers, absent/malformed artifacts,
resolver key inconsistencies and timeouts, and preservation of inherited
settings. They make no GNU or Rust runtime behavior claim. A genuine producer
run and its actual exporter footer are required for runtime registration evidence.

The reproducible producer has completed against the clean immutable source
`fd015216e5d6659f7386618d0978af6c37e0738f`: its real exporter reports one passing
test, no failures or ignored tests, and all 512 captured rows. A subsequent join
rechecks the retained executable hash and reconciles 297 resolved registrations,
215 unsupported registrations and zero metadata mismatches. It retains all
5,875 unprobed function bindings and the incomplete namespace-scan evidence.
The 45 separate producer/join tooling tests pass in 5.489 seconds; their fake
build-tool cases remain distinct from this genuine registration run.


## Refreshed main registration snapshot

The genuine exporter completed against clean main
`b3dd29a3a2e0743f5c4fd241c84dfb5eaeabd5d5`: one passing test, no failures or
ignored tests, and all 512 captured registrations. The producer retains and
checks 762 compiled-source inputs. The join rechecks the exact compiled
executable and resolves 302 entries, leaving 210 unsupported, with zero
reflected interface-family or arity mismatches. Fourteen `.External2`
descriptors retain the census family-only caveat.

Five entries have newly resolved descriptors since the earlier snapshot:
`stats::.C(HoltWinters)`, `stats::.C(multi_burg)`, and the stats Fortran
registrations `pppred`, `stl`, and `supsmu`. This exporter invokes no handlers;
these changes are registration evidence, with behavior and safety still
unclassified by this report. All 5,875 function bindings remain unprobed, the
namespace census remains incomplete, and full GNU R parity remains false.

The existing generation and join commands reproduce this snapshot using the
pinned full census and this immutable source revision. Subsequent development
requires another source-bound export; these counts do not describe every later
commit or provide a completion percentage.

## Joining selected public workflows to execution

The [public-workflow execution receipt](ci-checkpoints/public-workflows-d339e355.json)
uses namespace and binding names as inventory keys and resolves methods, structure,
deparse and dput entries to their public wrappers, implementations, tested options
and edge cases, target profiles and last verified source. Each execution check
retains its raw-log hash. The native and portable package suites and production
Chromium run execute those operations through public constructors.

This is a partial execution join. It does not classify an untested inventory name
as passing. The enclosing `structure.R` strictly passes; `eval-etc.R` completes but
still differs from GNU output. Its remaining semantic and printing differences,
and the separate internal dput routing gap, remain explicit in the receipt.

The later [5483fec6 execution join](ci-checkpoints/utils-console-5483fec6.json)
links original utils console/progress-bar calls, base close and binning wrappers,
print-gap behavior and the stats seasonal ARIMA0 workflow to implementations,
options, exact GNU fixtures, native/portable profiles and the verified source.
Its complete selected native suite passes 54 tests; 13 production Wasm contracts
pass in two fresh sessions, and all 60 local Chromium tests pass on the same
production runtime. This remains a partial join. The same receipt records
the enclosing driver failure at unsupported `C_mtext` and the independent ARIMA0
AR1 CSS coefficient mismatch, rather than treating namespace presence as passing.

The [b5f9f8a5 public-family execution join](ci-checkpoints/repetition-compiled-calls-b5f9f8a5.json)
links base repetition, vector growth and unserialize, utils fixed-width input, and
compiler calls to their actual wrappers and execution paths. It records argument
admission, reflection, typed tails, original compiled expressions, GC and cached
promise evaluation, together with native/portable and production Wasm targets.
Six unchanged original cases pass strict pinned GNU comparison; 19 public Wasm
contracts pass in two fresh sessions and the original Chromium suite passes60/60.
The independent complete inventory at1d remains1086 pass/95 fail. This partial
join does not declare all options or unexecuted inventory entries passing.
