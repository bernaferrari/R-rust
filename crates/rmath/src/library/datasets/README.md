These are generated assets from the authenticated GNU R oracle identified in
`assets/inventory.json`. The datasets belong to R Core Team and contributors
worldwide; their installed DESCRIPTION identifies their license as part of R.
The repository's GNU GPL license applies. Original object names, attributes,
data-topic aliases and index titles are retained.

From the repository root, regenerate into a new or empty directory:

```sh
python3 crates/rmath/src/library/datasets/generate.py OUTPUT --rscript PINNED_RSCRIPT
PINNED_RSCRIPT --vanilla crates/rmath/src/library/datasets/oracle_contract.R OUTPUT
python3 -m unittest discover -s crates/rmath/src/library/datasets -p 'test_*.py'
```

The generator verifies the oracle manifest and runtime marker, records every
installed input hash, checks complete key coverage, and copies the original
compressed lazy database without changing its keys. `all.rda` is a separate
version-2 uncompressed workspace used as the independent expected value graph.
It is not a replacement for GNU's lazy promises: eager loading observably changes
`substitute()` from retained promise code to the actual data value. The runtime
uses the original database through a checked virtual path, retaining literal
promise syntax and original environments. An installed datasets package remains
authoritative when one is available.

The public native acceptance checks all 108 value types and attributes, all 91
topic groups, 32 `covratio` values and names, and every byte of the independently
generated 592931-byte version-2 `values.rds` graph. Run it with:

```sh
scripts/cargo_dev.sh test -p r-embed --test portable_datasets -- --test-threads=1
```

The same original fixtures can be exercised through a freshly built Wasm facade:

```sh
bash scripts/build_wasm_runtime.sh --target nodejs --release --out-dir /tmp/datasets-wasm
node crates/rmath/src/library/datasets/wasm_contract.cjs /tmp/datasets-wasm
```

Node reads fixtures for these assertions and imports the expected graph into the
browser's in-memory file store. Dataset loading itself uses embedded bytes and
does not discover an installed host R library. These scoped contracts do not
establish complete GNU R package, evaluator or API parity.
