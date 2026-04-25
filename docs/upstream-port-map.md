# Upstream Port Map

The port keeps a machine-checkable source map in
`docs/upstream-port-map.tsv`. Each row links a Rust module to the upstream R
source file and function or subsystem anchor that should be consulted when
changing behavior.

The map is not meant to freeze the Rust code into C structure. It separates two
different promises:

- **Faithful behavior:** user-visible R semantics should match stock R for the
  covered surface, and conformance failures should point back to an upstream
  source area.
- **Rust-shaped ownership:** allocation, mutable runtime state, cancellation,
  paths, output capture, RNG, graphics state, and Android embedding belong to
  explicit sessions and owned Rust values.

## Sync Modes

| Mode | Meaning |
| --- | --- |
| `faithful` | Keep behavior and edge cases close to upstream R. Rust structure may be cleaner, but upstream should be the first reference. |
| `rust-shaped` | Preserve R behavior while deliberately changing ownership, entrypoints, or structure for Rust safety and Android embedding. |
| `policy` | Behavior is intentionally constrained by host or Android policy, such as path handling or native dynamic loading. |
| `generated` | Source is generated or mechanically derived. Regenerate rather than editing by hand. |
| `known-gap` | Upstream parity is intentionally incomplete and must stay visible until closed. |

## Workflow

1. Find the Rust module in `docs/upstream-port-map.tsv`.
2. Open the upstream file and anchor listed in that row.
3. Compare stock R behavior with the Rust implementation before changing
   user-visible semantics.
4. If the Rust code diverges for safety, sessions, Android policy, or API
   design, keep the row tagged `rust-shaped` or `policy` and document the
   reason in the notes column.
5. If a conformance failure is found, use the map to choose the upstream source
   area and add the failing behavior to the parity suite before or with the fix.
6. When adding a new translated core module, add a map row in the same change.

Run the checker directly:

```bash
scripts/check_upstream_port_map.sh
```

The release gate runs this check so stale paths and unknown sync modes are
caught before a slice is called shippable.
