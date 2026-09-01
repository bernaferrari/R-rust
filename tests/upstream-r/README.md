# Pinned GNU R Test Corpus

This directory contains the complete 245-file `r-source/tests` tree from the
exact GNU R commit in `oracle/r-oracle.json`, including package fixtures,
expected outputs, binary data, and all 70 top-level `.R`/`.Rin` drivers. The
import is reproducible:

```bash
python3 scripts/import_upstream_r_tests.py --archive /path/to/pinned-r-source.tar.gz
python3 scripts/validate_upstream_r_tests.py
```

`inventory.tsv` binds every vendored file to its SHA-256. `dispositions.tsv`
binds every file to one of three outcomes:

- `pass`: run against the pinned GNU R oracle and require identical normalized
  output from the Rust runtime.
- `xfail`: run both engines, require a current divergence, and fail on XPASS.
- `skip`: do not run yet; an owner bead and a concrete reason are mandatory.

The curated executable slices under `tests/upstream-core` remain the green
tracer bullets while child beads move whole, unedited files from skip to xfail
and then pass. A checked-in vendor file must never be edited by hand; re-import
it from the pinned archive so provenance and checksums remain reviewable.
