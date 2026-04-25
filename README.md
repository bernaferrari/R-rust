# Rport

Rport is a Rust-first port of core R runtime pieces with an Android embedding
target. The implementation aims to stay close enough to R's structure that
upstream behavior remains recognizable, while moving the public surface toward
safe Rust sessions, per-instance state, and UniFFI-friendly Android APIs.

## Release Proof

Run the local release gate before claiming a shippable slice:

```bash
scripts/release_gate.sh
```

The gate covers formatting, focused Rust tests, Android aarch64 checking,
mutable-global scanning, stock C R conformance parity, artifact sanity checks,
and whitespace validation. Use the full gate for slower packaging and binding
checks:

```bash
scripts/release_gate.sh --full
```

See `docs/release-gate.md` for the gate matrix, prerequisites, warning policy,
and generated conformance artifacts.
